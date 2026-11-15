//! libmpv custom stream callback bridge (`mpv/stream_cb.h`).
//!
//! Provides a safe, RAII-managed virtual filesystem stream bridge enabling
//! libmpv to read and seek directly from abstract Rust streams (memory, SMB, SFTP, WebDAV)
//! without external local HTTP proxies or unbounded disk caching.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::io::{Read, Seek, SeekFrom};
use std::os::raw::{c_char, c_int, c_void};
use std::panic::catch_unwind;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, OnceLock, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::error::MpvError;
use crate::handle::MpvHandle;
use libmpv_sys::{mpv_error, mpv_stream_cb_add_ro, mpv_stream_cb_info};

/// Trait for seekable, readable binary streams that can be mounted into libmpv.
pub trait SeekableStream: Send + Sync {
    /// Reads up to `buf.len()` bytes into `buf`.
    /// Returns the number of bytes read, or 0 on EOF.
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;

    /// Sets the 64-bit absolute stream position.
    /// Returns the new absolute position.
    fn seek(&mut self, position: u64) -> std::io::Result<u64>;

    /// Seeks using standard `Start`, `Current`, or `End` semantics.
    ///
    /// Streams that cannot resolve relative positions retain the default
    /// absolute-only behavior; HTTP, SMB and SFTP override this.
    fn seek_from(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        match position {
            SeekFrom::Start(position) => self.seek(position),
            SeekFrom::Current(_) | SeekFrom::End(_) => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "stream does not support relative seek",
            )),
        }
    }

    /// Returns total stream size in bytes if known.
    fn total_size(&self) -> Option<u64>;

    /// Signals cancellation of blocking I/O if supported.
    fn cancel(&mut self) {}
}

/// Allows boxed trait objects to pass through `MpvStreamBridge::register`.
impl<T: SeekableStream + ?Sized> SeekableStream for Box<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        (**self).read(buf)
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        (**self).seek(position)
    }

    fn total_size(&self) -> Option<u64> {
        (**self).total_size()
    }

    fn cancel(&mut self) {
        (**self).cancel();
    }
}

/// In-memory stream implementation backed by `Vec<u8>` or static slices.
#[derive(Debug, Clone)]
pub struct MemoryStream {
    data: Vec<u8>,
    position: usize,
}

impl MemoryStream {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, position: 0 }
    }
}

impl SeekableStream for MemoryStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.position >= self.data.len() {
            return Ok(0); // EOF
        }
        let available = &self.data[self.position..];
        let bytes_to_copy = available.len().min(buf.len());
        buf[..bytes_to_copy].copy_from_slice(&available[..bytes_to_copy]);
        self.position += bytes_to_copy;
        Ok(bytes_to_copy)
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        let pos = position as usize;
        if pos > self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Seek beyond EOF",
            ));
        }
        self.position = pos;
        Ok(position)
    }

    fn total_size(&self) -> Option<u64> {
        Some(self.data.len() as u64)
    }
}

/// Adapter converting any standard `std::io::Read + Seek + Send + Sync` into `SeekableStream`.
pub struct IoSeekableStream<T> {
    inner: T,
    size: Option<u64>,
}

impl<T: Read + Seek + Send + Sync> IoSeekableStream<T> {
    pub fn new(mut inner: T) -> Self {
        let size = inner.seek(SeekFrom::End(0)).ok();
        let _ = inner.seek(SeekFrom::Start(0));
        Self { inner, size }
    }
}

impl<T: Read + Seek + Send + Sync> SeekableStream for IoSeekableStream<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        self.inner.seek(SeekFrom::Start(position))
    }

    fn total_size(&self) -> Option<u64> {
        self.size
    }
}

/// Seekable binary stream backed by an HTTP/1.1 or WebDAV endpoint supporting `Range: bytes=start-end`.
pub struct HttpRangeStream {
    scheme: String,
    host: String,
    port: u16,
    path: String,
    auth_header: Option<String>,
    position: u64,
    total_size: Option<u64>,
    cancelled: AtomicBool,
    agent: ureq::Agent,
}

impl HttpRangeStream {
    /// Creates and connects a new HTTP range-seekable stream.
    ///
    /// Performs an initial probe (HEAD or 1-byte GET) to discover total resource size
    /// and confirm byte range support without buffering full media into memory.
    pub fn new(
        host: &str,
        port: u16,
        path: &str,
        auth_header: Option<String>,
    ) -> std::io::Result<Self> {
        Self::new_with_scheme("http", host, port, path, auth_header)
    }

    fn new_with_scheme(
        scheme: &str,
        host: &str,
        port: u16,
        path: &str,
        auth_header: Option<String>,
    ) -> std::io::Result<Self> {
        let mut stream = Self {
            scheme: scheme.to_string(),
            host: host.to_string(),
            port,
            path: if path.starts_with('/') {
                path.to_string()
            } else {
                format!("/{}", path)
            },
            auth_header,
            position: 0,
            total_size: None,
            cancelled: AtomicBool::new(false),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(Duration::from_secs(20))
                .build(),
        };

        stream.probe_size()?;
        Ok(stream)
    }

    /// Creates an `HttpRangeStream` parsed from a full HTTP/HTTPS URL.
    ///
    /// HTTPS is handled by `ureq`'s TLS stack; the URL is never downgraded to a
    /// plaintext socket request.
    pub fn from_url(url: &str, auth_header: Option<String>) -> std::io::Result<Self> {
        let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
            ("https", rest)
        } else if let Some(rest) = url.strip_prefix("http://") {
            ("http", rest)
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported HTTP stream scheme: {url}"),
            ));
        };

        let (host_port, path) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };

        let (host, port) = match host_port.find(':') {
            Some(idx) => {
                let h = &host_port[..idx];
                let p = host_port[idx + 1..].parse::<u16>().map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("Invalid port: {e}"),
                    )
                })?;
                (h, p)
            }
            None => {
                let p = if scheme == "https" { 443 } else { 80 };
                (host_port, p)
            }
        };

        Self::new_with_scheme(scheme, host, port, path, auth_header)
    }

    /// Transport scheme selected for this stream.
    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    fn url(&self) -> String {
        format!("{}://{}:{}{}", self.scheme, self.host, self.port, self.path)
    }

    fn auth_value(&self) -> Option<&str> {
        self.auth_header.as_deref().map(|value| {
            value
                .strip_prefix("Authorization:")
                .map(str::trim)
                .unwrap_or(value)
        })
    }

    fn request(&self) -> ureq::Request {
        let request = self.agent.get(&self.url());
        match self.auth_value() {
            Some(value) => request.set("Authorization", value),
            None => request,
        }
    }

    fn map_error(error: ureq::Error) -> std::io::Error {
        std::io::Error::other(format!("HTTP range transport: {error}"))
    }

    fn parse_total(response: &ureq::Response) -> Option<u64> {
        if let Some(range) = response.header("Content-Range") {
            if let Some((_, total)) = range.rsplit_once('/') {
                if let Ok(size) = total.trim().parse::<u64>() {
                    return Some(size);
                }
            }
        }
        response
            .header("Content-Length")
            .and_then(|value| value.trim().parse::<u64>().ok())
    }

    fn probe_size(&mut self) -> std::io::Result<()> {
        let url = self.url();
        let mut head = self.agent.head(&url);
        if let Some(value) = self.auth_value() {
            head = head.set("Authorization", value);
        }
        if let Ok(response) = head.call() {
            self.total_size = Self::parse_total(&response);
            if self.total_size.is_some() {
                return Ok(());
            }
        }

        // Some servers reject HEAD. A one-byte range GET discovers the total.
        let response = self
            .request()
            .set("Range", "bytes=0-0")
            .call()
            .map_err(Self::map_error)?;
        self.total_size = Self::parse_total(&response);
        if self.total_size.is_none() {
            return Err(std::io::Error::other(
                "HTTP server did not expose Content-Length or Content-Range",
            ));
        }
        Ok(())
    }
}

impl SeekableStream for HttpRangeStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Stream cancelled",
            ));
        }
        if buf.is_empty() {
            return Ok(0);
        }
        if let Some(total) = self.total_size {
            if self.position >= total {
                return Ok(0);
            }
        }

        let start = self.position;
        let mut end = start.saturating_add(buf.len() as u64 - 1);
        if let Some(total) = self.total_size {
            end = end.min(total - 1);
        }
        if start > end {
            return Ok(0);
        }

        let response = self
            .request()
            .set("Range", &format!("bytes={start}-{end}"))
            .call()
            .map_err(Self::map_error)?;
        let status = response.status();
        if status != 206 && !(status == 200 && start == 0) {
            return Err(std::io::Error::other(format!(
                "HTTP Range request failed with status {status}"
            )));
        }
        if let Some(total) = Self::parse_total(&response) {
            self.total_size = Some(total);
        }

        let mut reader = response.into_reader();
        let max_to_read = (end - start + 1) as usize;
        let mut total_read = 0;
        while total_read < max_to_read {
            let n = reader.read(&mut buf[total_read..max_to_read])?;
            if n == 0 {
                break;
            }
            total_read += n;
        }
        self.position = self.position.saturating_add(total_read as u64);
        Ok(total_read)
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        if let Some(total) = self.total_size {
            if position > total {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Seek beyond EOF",
                ));
            }
        }
        self.position = position;
        Ok(position)
    }

    fn seek_from(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let target = match position {
            SeekFrom::Start(position) => position,
            SeekFrom::Current(delta) => {
                self.position.checked_add_signed(delta).ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek overflow")
                })?
            }
            SeekFrom::End(delta) => {
                let total = self.total_size.ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "cannot seek from end without a known size",
                    )
                })?;
                total.checked_add_signed(delta).ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek overflow")
                })?
            }
        };
        self.seek(target)
    }

    fn total_size(&self) -> Option<u64> {
        self.total_size
    }

    fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

/// Bounded read-ahead stream wrapper smoothing network latency and micro chunk reads.
pub struct ReadAheadStream<S: SeekableStream> {
    inner: S,
    buffer: Vec<u8>,
    buffer_start: u64,
    buffer_len: usize,
    position: u64,
    capacity: usize,
}

impl<S: SeekableStream> ReadAheadStream<S> {
    pub const DEFAULT_CAPACITY: usize = 256 * 1024; // 256 KB bounded read-ahead buffer

    pub fn new(inner: S) -> Self {
        Self::with_capacity(inner, Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(inner: S, capacity: usize) -> Self {
        Self {
            inner,
            buffer: vec![0u8; capacity],
            buffer_start: 0,
            buffer_len: 0,
            position: 0,
            capacity,
        }
    }

    fn fill_buffer(&mut self) -> std::io::Result<()> {
        self.inner.seek(self.position)?;
        self.buffer_start = self.position;
        let mut read_total = 0;
        while read_total < self.capacity {
            let n = self.inner.read(&mut self.buffer[read_total..])?;
            if n == 0 {
                break;
            }
            read_total += n;
        }
        self.buffer_len = read_total;
        Ok(())
    }
}

impl<S: SeekableStream> SeekableStream for ReadAheadStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let buf_end = self.buffer_start + self.buffer_len as u64;
        if self.position < self.buffer_start || self.position >= buf_end || self.buffer_len == 0 {
            self.fill_buffer()?;
            if self.buffer_len == 0 {
                return Ok(0);
            }
        }

        let offset_in_buf = (self.position - self.buffer_start) as usize;
        let available = self.buffer_len.saturating_sub(offset_in_buf);
        let to_copy = available.min(buf.len());

        buf[..to_copy].copy_from_slice(&self.buffer[offset_in_buf..offset_in_buf + to_copy]);
        self.position += to_copy as u64;

        Ok(to_copy)
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        self.position = position;
        let buf_end = self.buffer_start + self.buffer_len as u64;
        if position < self.buffer_start || position >= buf_end {
            self.buffer_len = 0;
        }
        Ok(position)
    }

    fn total_size(&self) -> Option<u64> {
        self.inner.total_size()
    }

    fn cancel(&mut self) {
        self.inner.cancel();
    }
}

/// Classification of stream I/O errors for recovery decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamErrorKind {
    /// Transient network interruption (e.g. timeout, connection reset, broken pipe).
    /// Safe to retry after reconnecting and seeking to last byte offset.
    Transient,
    /// Permanent unrecoverable failure (e.g. 404 Not Found, Permission Denied, corrupt request).
    /// Immediate failure without retrying.
    Permanent,
    /// Operation cancelled by caller.
    Cancelled,
}

/// Classifies a standard `std::io::Error` into transient, permanent, or cancelled.
pub fn classify_io_error(err: &std::io::Error) -> StreamErrorKind {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::TimedOut
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::NotConnected
        | ErrorKind::BrokenPipe
        | ErrorKind::UnexpectedEof => StreamErrorKind::Transient,
        ErrorKind::Interrupted => {
            let msg = err.to_string().to_lowercase();
            if msg.contains("cancel") {
                StreamErrorKind::Cancelled
            } else {
                StreamErrorKind::Transient
            }
        }
        ErrorKind::NotFound
        | ErrorKind::PermissionDenied
        | ErrorKind::InvalidInput
        | ErrorKind::Unsupported => StreamErrorKind::Permanent,
        _ => {
            let msg = err.to_string().to_lowercase();
            if msg.contains("cancel") {
                StreamErrorKind::Cancelled
            } else if msg.contains("timed out")
                || msg.contains("timeout")
                || msg.contains("connection reset")
                || msg.contains("connection aborted")
                || msg.contains("broken pipe")
                || msg.contains("not connected")
                || msg.contains("closed channel")
                || msg.contains("worker is closed")
                || msg.contains("stream worker is closed")
                || msg.contains("unexpected eof")
                || msg.contains("channel closed")
            {
                StreamErrorKind::Transient
            } else {
                StreamErrorKind::Permanent
            }
        }
    }
}

/// Exponential backoff retry policy for remote stream reconnects.
#[derive(Debug, Clone)]
pub struct StreamRetryPolicy {
    /// Maximum number of reconnect attempts before giving up.
    pub max_retries: u32,
    /// Initial delay before the first reconnect attempt.
    pub initial_delay: Duration,
    /// Upper bound on backoff delay.
    pub max_delay: Duration,
    /// Multiplicative factor applied after each failed attempt.
    pub backoff_factor: f64,
}

impl Default for StreamRetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(3000),
            backoff_factor: 2.0,
        }
    }
}

impl StreamRetryPolicy {
    /// Computes the exponential delay for a given 1-based attempt index.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let factor = self.backoff_factor.powi((attempt - 1) as i32);
        let millis = (self.initial_delay.as_millis() as f64 * factor) as u64;
        let bounded = millis.min(self.max_delay.as_millis() as u64);
        Duration::from_millis(bounded)
    }
}

/// Observable state of a recoverable remote stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryState {
    /// Connected and operating normally.
    Connected,
    /// Actively attempting reconnection after transient disconnect.
    Reconnecting { attempt: u32, max_attempts: u32 },
    /// Failed after exhausting retry attempts or encountering a permanent error.
    Failed(String),
    /// Stream was closed or cancelled.
    Closed,
}

/// Factory function closure for creating or recreating seekable streams.
pub type StreamOpener = Box<dyn Fn() -> std::io::Result<Box<dyn SeekableStream>> + Send + Sync>;

/// Stream wrapper providing Level A transparent network reconnect and byte-exact resume.
///
/// If underlying I/O fails with a transient network error, `RecoverableStream` reconnects
/// using its factory closure, seeks to the exact previous byte offset, and continues reading
/// without skipping or duplicating bytes.
pub struct RecoverableStream {
    opener: StreamOpener,
    inner: Option<Box<dyn SeekableStream>>,
    position: u64,
    size: Option<u64>,
    retry_policy: StreamRetryPolicy,
    state: RecoveryState,
    cancelled: Arc<AtomicBool>,
}

impl RecoverableStream {
    /// Creates a new `RecoverableStream` by calling `opener` and initializing tracking state.
    pub fn new<F>(opener: F, retry_policy: StreamRetryPolicy) -> std::io::Result<Self>
    where
        F: Fn() -> std::io::Result<Box<dyn SeekableStream>> + Send + Sync + 'static,
    {
        let stream = (opener)()?;
        let size = stream.total_size();
        Ok(Self {
            opener: Box::new(opener),
            inner: Some(stream),
            position: 0,
            size,
            retry_policy,
            state: RecoveryState::Connected,
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Current observable recovery state.
    pub fn state(&self) -> RecoveryState {
        self.state.clone()
    }

    /// Current absolute byte position.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Active retry policy.
    pub fn retry_policy(&self) -> &StreamRetryPolicy {
        &self.retry_policy
    }

    fn reconnect(&mut self) -> std::io::Result<()> {
        let max_attempts = self.retry_policy.max_retries;
        let mut last_err =
            std::io::Error::new(std::io::ErrorKind::NotConnected, "Stream disconnected");

        for attempt in 1..=max_attempts {
            if self.cancelled.load(Ordering::Relaxed) {
                self.state = RecoveryState::Closed;
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "Stream recovery cancelled",
                ));
            }

            self.state = RecoveryState::Reconnecting {
                attempt,
                max_attempts,
            };

            let delay = self.retry_policy.delay_for_attempt(attempt);
            let sleep_step = Duration::from_millis(20);
            let mut elapsed = Duration::ZERO;
            while elapsed < delay {
                if self.cancelled.load(Ordering::Relaxed) {
                    self.state = RecoveryState::Closed;
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "Stream recovery cancelled during backoff",
                    ));
                }
                let step = sleep_step.min(delay - elapsed);
                std::thread::sleep(step);
                elapsed += step;
            }

            match (self.opener)() {
                Ok(mut new_stream) => match new_stream.seek(self.position) {
                    Ok(pos) => {
                        if pos != self.position {
                            log::warn!(
                                "Stream recovery seek returned unexpected position {pos} != {}",
                                self.position
                            );
                        }
                        self.inner = Some(new_stream);
                        self.state = RecoveryState::Connected;
                        log::info!(
                            "Stream successfully reconnected at offset {} on attempt {attempt}",
                            self.position
                        );
                        return Ok(());
                    }
                    Err(seek_err) => {
                        log::warn!(
                            "Stream seek failed after reconnect at {}: {seek_err}",
                            self.position
                        );
                        if classify_io_error(&seek_err) == StreamErrorKind::Permanent {
                            self.state = RecoveryState::Failed(seek_err.to_string());
                            return Err(seek_err);
                        }
                        last_err = seek_err;
                    }
                },
                Err(open_err) => {
                    log::warn!("Stream opener attempt {attempt}/{max_attempts} failed: {open_err}");
                    if classify_io_error(&open_err) == StreamErrorKind::Permanent {
                        self.state = RecoveryState::Failed(open_err.to_string());
                        return Err(open_err);
                    }
                    last_err = open_err;
                }
            }
        }

        let fail_msg =
            format!("Stream recovery retries exhausted ({max_attempts} attempts): {last_err}");
        self.state = RecoveryState::Failed(fail_msg.clone());
        Err(std::io::Error::new(std::io::ErrorKind::TimedOut, fail_msg))
    }
}

impl SeekableStream for RecoverableStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Stream cancelled",
            ));
        }

        if buf.is_empty() {
            return Ok(0);
        }

        if self.inner.is_none() {
            self.reconnect()?;
        }

        let inner = self.inner.as_mut().expect("Inner stream established");
        match inner.read(buf) {
            Ok(n) => {
                self.position += n as u64;
                self.state = RecoveryState::Connected;
                Ok(n)
            }
            Err(e) => match classify_io_error(&e) {
                StreamErrorKind::Cancelled => {
                    self.state = RecoveryState::Closed;
                    Err(e)
                }
                StreamErrorKind::Transient => {
                    log::warn!(
                        "Transient read error at position {}: {e}. Triggering reconnect...",
                        self.position
                    );
                    self.inner = None;
                    self.reconnect()?;
                    let inner = self.inner.as_mut().expect("Inner stream reconnected");
                    let n = inner.read(buf)?;
                    self.position += n as u64;
                    Ok(n)
                }
                StreamErrorKind::Permanent => {
                    self.state = RecoveryState::Failed(e.to_string());
                    Err(e)
                }
            },
        }
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Stream cancelled",
            ));
        }

        self.position = position;
        if let Some(inner) = self.inner.as_mut() {
            match inner.seek(position) {
                Ok(pos) => return Ok(pos),
                Err(e) => {
                    if classify_io_error(&e) == StreamErrorKind::Transient {
                        log::warn!(
                            "Transient seek error at {position}: {e}. Triggering reconnect..."
                        );
                        self.inner = None;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        self.reconnect()?;
        Ok(self.position)
    }

    fn total_size(&self) -> Option<u64> {
        self.size
            .or_else(|| self.inner.as_ref().and_then(|s| s.total_size()))
    }

    fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.state = RecoveryState::Closed;
        if let Some(inner) = self.inner.as_mut() {
            inner.cancel();
        }
    }
}

/// Seekable stream reading directly from an authenticated SFTP remote file using `ssh2`.
pub struct SftpSeekableStream {
    file: ssh2::File,
    size: Option<u64>,
    position: u64,
    cancelled: AtomicBool,
    _session: Option<ssh2::Session>,
}

impl SftpSeekableStream {
    pub fn new(mut file: ssh2::File) -> std::io::Result<Self> {
        let size = file.stat().ok().and_then(|st| st.size);
        let _ = file.seek(SeekFrom::Start(0));
        Ok(Self {
            file,
            size,
            position: 0,
            cancelled: AtomicBool::new(false),
            _session: None,
        })
    }

    /// Creates an `SftpSeekableStream` holding a dedicated `ssh2::Session` to preserve session isolation.
    pub fn with_session(session: ssh2::Session, mut file: ssh2::File) -> std::io::Result<Self> {
        let size = file.stat().ok().and_then(|st| st.size);
        let _ = file.seek(SeekFrom::Start(0));
        Ok(Self {
            file,
            size,
            position: 0,
            cancelled: AtomicBool::new(false),
            _session: Some(session),
        })
    }
}

impl SeekableStream for SftpSeekableStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "SFTP stream cancelled",
            ));
        }
        let n = self.file.read(buf)?;
        self.position += n as u64;
        Ok(n)
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        let pos = self.file.seek(SeekFrom::Start(position))?;
        self.position = pos;
        Ok(pos)
    }

    fn seek_from(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let pos = self.file.seek(position)?;
        self.position = pos;
        Ok(pos)
    }

    fn total_size(&self) -> Option<u64> {
        self.size
    }

    fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

enum SmbWorkerCommand {
    ReadAt {
        offset: u64,
        len: u64,
        reply: SyncSender<std::io::Result<Vec<u8>>>,
    },
    Close,
}

/// Seekable stream reading chunks on-demand from an SMB2/3 share file using pure-Rust `smb2`.
///
/// Uses a dedicated background worker thread with a persistent Tokio runtime to eliminate
/// thread-per-read construction churn and guarantee socket-driver affinity, while ensuring
/// libmpv's synchronous C callbacks never deadlock or block the player UI thread.
pub struct SmbSeekableStream {
    tx: Option<SyncSender<SmbWorkerCommand>>,
    worker: Option<JoinHandle<()>>,
    size: u64,
    position: u64,
    cancelled: Arc<AtomicBool>,
}

impl SmbSeekableStream {
    /// Opens a seekable stream through `SmbProvider::open_reader_async`.
    ///
    /// The provider remains the single place that validates configuration and
    /// resolves credentials from the credential vault.
    pub fn connect_via_provider(
        provider: &muvrix_core::network::SmbProvider,
        file_path: &str,
    ) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| std::io::Error::other(format!("SMB runtime: {e}")))?;
        let reader = runtime
            .block_on(provider.open_reader_async(file_path))
            .map_err(|e| std::io::Error::other(format!("SMB provider open: {e}")))?;
        Ok(Self::new(reader))
    }

    /// Connects to a remote SMB share and opens a seekable file stream on a dedicated worker thread.
    pub fn connect(
        host: &str,
        port: u16,
        share: &str,
        file_path: &str,
        username: Option<String>,
        password: Option<String>,
        is_guest: bool,
    ) -> std::io::Result<Self> {
        let (init_tx, init_rx) =
            sync_channel::<std::io::Result<(u64, SyncSender<SmbWorkerCommand>)>>(1);
        let cancelled = Arc::new(AtomicBool::new(false));

        let host = host.to_string();
        let port = if port == 0 { 445 } else { port };
        let share = share.trim_matches('/').to_string();
        let file_path = file_path.trim_matches('/').to_string();

        let worker = std::thread::Builder::new()
            .name("muvrix-smb-stream-worker".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = init_tx.send(Err(std::io::Error::other(format!(
                            "Failed to build SMB worker runtime: {e}"
                        ))));
                        return;
                    }
                };

                rt.block_on(async move {
                    let addr = format!("{}:{}", host, port);
                    let user = username
                        .as_deref()
                        .unwrap_or(if is_guest { "guest" } else { "" });
                    let pass = password.as_deref().unwrap_or("");

                    let mut client = match smb2::connect(&addr, user, pass).await {
                        Ok(c) => c,
                        Err(e) => {
                            let _ = init_tx.send(Err(std::io::Error::other(format!(
                                "SMB worker connect to {addr} failed: {e}"
                            ))));
                            return;
                        }
                    };

                    let tree = match client.connect_share(&share).await {
                        Ok(t) => t,
                        Err(e) => {
                            let _ = init_tx.send(Err(std::io::Error::other(format!(
                                "SMB worker connect share '{share}' failed: {e}"
                            ))));
                            return;
                        }
                    };

                    let reader = match client.open_file_reader(&tree, &file_path).await {
                        Ok(r) => r,
                        Err(e) => {
                            let _ = init_tx.send(Err(std::io::Error::other(format!(
                                "SMB worker open file '{file_path}' failed: {e}"
                            ))));
                            return;
                        }
                    };

                    let size = reader.size();
                    let (cmd_tx, cmd_rx) = sync_channel::<SmbWorkerCommand>(32);

                    if init_tx.send(Ok((size, cmd_tx))).is_err() {
                        return;
                    }

                    while let Ok(cmd) = cmd_rx.recv() {
                        match cmd {
                            SmbWorkerCommand::ReadAt { offset, len, reply } => {
                                let chunk_res = reader.read_at(offset, len).await.map_err(|e| {
                                    std::io::Error::other(format!(
                                        "SMB read failed at offset {offset}: {e}"
                                    ))
                                });
                                let _ = reply.send(chunk_res);
                            }
                            SmbWorkerCommand::Close => break,
                        }
                    }
                });
            })
            .map_err(|e| {
                std::io::Error::other(format!("Failed to spawn SMB worker thread: {e}"))
            })?;

        let (size, tx) = init_rx.recv().map_err(|e| {
            std::io::Error::other(format!("SMB worker init communication error: {e}"))
        })??;

        Ok(Self {
            tx: Some(tx),
            worker: Some(worker),
            size,
            position: 0,
            cancelled,
        })
    }

    /// Creates an `SmbSeekableStream` from an existing reader instance.
    pub fn new(reader: smb2::client::stream::FileReader) -> Self {
        let size = reader.size();
        let (cmd_tx, cmd_rx) = sync_channel::<SmbWorkerCommand>(32);
        let cancelled = Arc::new(AtomicBool::new(false));

        let worker = std::thread::Builder::new()
            .name("muvrix-smb-stream-worker".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Build worker Tokio runtime");

                rt.block_on(async move {
                    while let Ok(cmd) = cmd_rx.recv() {
                        match cmd {
                            SmbWorkerCommand::ReadAt { offset, len, reply } => {
                                let chunk_res = reader.read_at(offset, len).await.map_err(|e| {
                                    std::io::Error::other(format!(
                                        "SMB read failed at offset {offset}: {e}"
                                    ))
                                });
                                let _ = reply.send(chunk_res);
                            }
                            SmbWorkerCommand::Close => break,
                        }
                    }
                });
            })
            .expect("Spawn SMB stream worker thread");

        Self {
            tx: Some(cmd_tx),
            worker: Some(worker),
            size,
            position: 0,
            cancelled,
        }
    }
}

impl SeekableStream for SmbSeekableStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "SMB stream cancelled",
            ));
        }
        if self.position >= self.size {
            return Ok(0);
        }

        let start = self.position;
        let max_to_read = (buf.len() as u64).min(self.size - start);
        if max_to_read == 0 {
            return Ok(0);
        }

        let tx = match self.tx.as_ref() {
            Some(t) => t,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "SMB stream worker is closed",
                ))
            }
        };

        let (reply_tx, reply_rx) = sync_channel::<std::io::Result<Vec<u8>>>(1);
        tx.send(SmbWorkerCommand::ReadAt {
            offset: start,
            len: max_to_read,
            reply: reply_tx,
        })
        .map_err(|e| {
            std::io::Error::other(format!("Failed to send read request to SMB worker: {e}"))
        })?;

        let chunk = reply_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("SMB read timed out: {e}"),
                )
            })??;

        let n = chunk.len();
        buf[..n].copy_from_slice(&chunk);
        self.position += n as u64;
        Ok(n)
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        if position > self.size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Seek beyond EOF",
            ));
        }
        self.position = position;
        Ok(position)
    }

    fn seek_from(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let target = match position {
            SeekFrom::Start(position) => position,
            SeekFrom::Current(delta) => {
                self.position.checked_add_signed(delta).ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek overflow")
                })?
            }
            SeekFrom::End(delta) => self.size.checked_add_signed(delta).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "seek overflow")
            })?,
        };
        self.seek(target)
    }

    fn total_size(&self) -> Option<u64> {
        Some(self.size)
    }

    fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

impl Drop for SmbSeekableStream {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(SmbWorkerCommand::Close);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Global stream registry for mapping protocol identifiers to active stream instances.
#[derive(Default)]
pub struct StreamRegistry {
    streams: RwLock<HashMap<String, Box<dyn SeekableStream>>>,
    counter: AtomicU64,
}

impl StreamRegistry {
    pub fn new() -> Self {
        Self {
            streams: RwLock::new(HashMap::new()),
            counter: AtomicU64::new(1),
        }
    }

    /// Registers a stream and returns a URI suitable for libmpv (e.g. `muvrix-stream://stream-1`).
    pub fn register(&self, stream: Box<dyn SeekableStream>) -> String {
        let id = self.counter.fetch_add(1, Ordering::Relaxed);
        let key = format!("stream-{}", id);
        let uri = format!("muvrix-stream://{}", key);
        let mut map = self.streams.write().expect("StreamRegistry lock poisoned");
        map.insert(key, stream);
        uri
    }

    /// Takes a stream from the registry by its identifier.
    pub fn take(&self, key: &str) -> Option<Box<dyn SeekableStream>> {
        let mut map = self.streams.write().expect("StreamRegistry lock poisoned");
        map.remove(key)
    }
}

static GLOBAL_REGISTRY: OnceLock<Arc<StreamRegistry>> = OnceLock::new();

fn get_global_registry() -> &'static Arc<StreamRegistry> {
    GLOBAL_REGISTRY.get_or_init(|| Arc::new(StreamRegistry::new()))
}

// ---------------------------------------------------------------------------
// C-FFI Trampoline Callbacks matching `mpv/stream_cb.h`
//
// NOTE: `stream_cb.h` is explicitly designated upstream in libmpv as an experimental/
// unstable API subject to change without standard deprecation cycles.
// ABI Return Contracts:
// - `open_fn`: returns 0 (`MPV_ERROR_SUCCESS`) on success, or `MPV_ERROR_LOADING_FAILED` (-6) on failure.
// - `read_fn`: returns byte count (>=0), 0 for EOF, or negative error code (`MPV_ERROR_GENERIC` = -1).
// - `seek_fn`: returns new byte offset (>=0), or `MPV_ERROR_UNSUPPORTED` (-14).
// - `size_fn`: returns total size in bytes (>=0), or `MPV_ERROR_UNSUPPORTED` (-14).
// - `close_fn`: return type is `void`; drops the boxed stream and safely swallows any panics.
// - `cancel_fn`: return type is `void`; triggers asynchronous stream interruption and swallows panics.
// ---------------------------------------------------------------------------

unsafe extern "C" fn stream_read_trampoline(
    cookie: *mut c_void,
    buf: *mut c_char,
    nbytes: u64,
) -> i64 {
    let result = catch_unwind(|| {
        if cookie.is_null() || buf.is_null() {
            return mpv_error::MPV_ERROR_GENERIC as i64;
        }
        let stream = &mut *(cookie as *mut Box<dyn SeekableStream>);
        let slice = std::slice::from_raw_parts_mut(buf as *mut u8, nbytes as usize);
        match stream.read(slice) {
            Ok(bytes_read) => bytes_read as i64,
            Err(e) => {
                log::error!("Custom stream read error: {e}");
                mpv_error::MPV_ERROR_GENERIC as i64
            }
        }
    });

    result.unwrap_or(mpv_error::MPV_ERROR_GENERIC as i64)
}

unsafe extern "C" fn stream_seek_trampoline(cookie: *mut c_void, offset: i64) -> i64 {
    let result = catch_unwind(|| {
        if cookie.is_null() || offset < 0 {
            return mpv_error::MPV_ERROR_UNSUPPORTED as i64;
        }
        let stream = &mut *(cookie as *mut Box<dyn SeekableStream>);
        match stream.seek(offset as u64) {
            Ok(new_pos) => new_pos as i64,
            Err(e) => {
                log::warn!("Custom stream seek to {offset} failed: {e}");
                mpv_error::MPV_ERROR_UNSUPPORTED as i64
            }
        }
    });

    result.unwrap_or(mpv_error::MPV_ERROR_UNSUPPORTED as i64)
}

unsafe extern "C" fn stream_size_trampoline(cookie: *mut c_void) -> i64 {
    let result = catch_unwind(|| {
        if cookie.is_null() {
            return mpv_error::MPV_ERROR_UNSUPPORTED as i64;
        }
        let stream = &*(cookie as *mut Box<dyn SeekableStream>);
        match stream.total_size() {
            Some(sz) => sz as i64,
            None => mpv_error::MPV_ERROR_UNSUPPORTED as i64,
        }
    });

    result.unwrap_or(mpv_error::MPV_ERROR_UNSUPPORTED as i64)
}

unsafe extern "C" fn stream_close_trampoline(cookie: *mut c_void) {
    let _ = catch_unwind(|| {
        if !cookie.is_null() {
            // Reconstitute the Box and drop it
            let _ = Box::from_raw(cookie as *mut Box<dyn SeekableStream>);
        }
    });
}

unsafe extern "C" fn stream_cancel_trampoline(cookie: *mut c_void) {
    let _ = catch_unwind(|| {
        if !cookie.is_null() {
            let stream = &mut *(cookie as *mut Box<dyn SeekableStream>);
            stream.cancel();
        }
    });
}

unsafe extern "C" fn stream_open_trampoline(
    user_data: *mut c_void,
    uri: *mut c_char,
    info: *mut mpv_stream_cb_info,
) -> c_int {
    let result = catch_unwind(|| {
        if uri.is_null() || info.is_null() {
            return mpv_error::MPV_ERROR_LOADING_FAILED as c_int;
        }

        let c_str = CStr::from_ptr(uri);
        let uri_str = match c_str.to_str() {
            Ok(s) => s,
            Err(_) => return mpv_error::MPV_ERROR_LOADING_FAILED as c_int,
        };

        // Parse key from "muvrix-stream://<key>"
        let key = if let Some(stripped) = uri_str.strip_prefix("muvrix-stream://") {
            stripped.trim_end_matches('/')
        } else {
            return mpv_error::MPV_ERROR_LOADING_FAILED as c_int;
        };

        let registry: &StreamRegistry = if !user_data.is_null() {
            &*(user_data as *const StreamRegistry)
        } else {
            get_global_registry().as_ref()
        };

        let stream = match registry.take(key) {
            Some(s) => s,
            None => {
                log::error!("Custom stream not found in registry: {key}");
                return mpv_error::MPV_ERROR_LOADING_FAILED as c_int;
            }
        };

        // Heap-allocate stream handle for cookie
        let cookie = Box::into_raw(Box::new(stream));

        (*info).cookie = cookie as *mut c_void;
        (*info).read_fn = Some(stream_read_trampoline);
        (*info).seek_fn = Some(stream_seek_trampoline);
        (*info).size_fn = Some(stream_size_trampoline);
        (*info).close_fn = Some(stream_close_trampoline);
        (*info).cancel_fn = Some(stream_cancel_trampoline);

        mpv_error::MPV_ERROR_SUCCESS as c_int
    });

    result.unwrap_or(mpv_error::MPV_ERROR_LOADING_FAILED as c_int)
}

/// Custom Stream Bridge managing stream registration and protocol binding with libmpv.
pub struct MpvStreamBridge {
    registry: Arc<StreamRegistry>,
    protocol: CString,
}

impl Default for MpvStreamBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl MpvStreamBridge {
    /// Creates a new `MpvStreamBridge` with default protocol `muvrix-stream`.
    pub fn new() -> Self {
        Self {
            registry: Arc::new(StreamRegistry::new()),
            protocol: CString::new("muvrix-stream").expect("Valid CString"),
        }
    }

    /// Accesses the underlying registry.
    pub fn registry(&self) -> &Arc<StreamRegistry> {
        &self.registry
    }

    /// Registers a seekable stream and returns a URI playable by mpv.
    pub fn register<S: SeekableStream + 'static>(&self, stream: S) -> String {
        self.registry.register(Box::new(stream))
    }

    /// Attaches this custom stream protocol to an uninitialized or initialized `MpvHandle`.
    /// Structurally retains ownership of the underlying `StreamRegistry` in `MpvHandle`
    /// to prevent dangling pointer hazards when C callbacks are invoked.
    pub fn attach_to_handle(&self, handle: &MpvHandle) -> Result<(), MpvError> {
        let ctx = handle.as_ptr();
        handle.retain_resource(self.registry.clone());
        let user_data = Arc::as_ptr(&self.registry) as *mut c_void;
        let res = unsafe {
            mpv_stream_cb_add_ro(
                ctx,
                self.protocol.as_ptr(),
                user_data,
                Some(stream_open_trampoline),
            )
        };
        MpvError::from_raw(res)
    }
}
