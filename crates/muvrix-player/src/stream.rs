//! libmpv custom stream callback bridge (`mpv/stream_cb.h`).
//!
//! Provides a safe, RAII-managed virtual filesystem stream bridge enabling
//! libmpv to read and seek directly from abstract Rust streams (memory, SMB, SFTP, WebDAV)
//! without external local HTTP proxies or unbounded disk caching.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::os::raw::{c_char, c_int, c_void};
use std::panic::catch_unwind;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
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

    /// Returns total stream size in bytes if known.
    fn total_size(&self) -> Option<u64>;

    /// Signals cancellation of blocking I/O if supported.
    fn cancel(&mut self) {}
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
    host: String,
    port: u16,
    path: String,
    auth_header: Option<String>,
    position: u64,
    total_size: Option<u64>,
    cancelled: AtomicBool,
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
        let mut stream = Self {
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
        };

        stream.probe_size()?;
        Ok(stream)
    }

    fn connect(&self) -> std::io::Result<TcpStream> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream = TcpStream::connect_timeout(
            &addr
                .parse()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?,
            Duration::from_secs(4),
        )?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        Ok(stream)
    }

    fn probe_size(&mut self) -> std::io::Result<()> {
        let mut tcp = self.connect()?;
        let mut req = format!(
            "HEAD {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n",
            self.path, self.host, self.port
        );
        if let Some(ref auth) = self.auth_header {
            req.push_str(auth);
            req.push_str("\r\n");
        }
        req.push_str("\r\n");
        tcp.write_all(req.as_bytes())?;

        let mut reader = BufReader::new(tcp);
        let mut status_line = String::new();
        reader.read_line(&mut status_line)?;

        if status_line.contains(" 200 ") || status_line.contains(" 206 ") {
            let mut line = String::new();
            while reader.read_line(&mut line)? > 0 {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    break;
                }
                if let Some((k, v)) = trimmed.split_once(':') {
                    if k.trim().eq_ignore_ascii_case("content-length") {
                        if let Ok(sz) = v.trim().parse::<u64>() {
                            self.total_size = Some(sz);
                        }
                    } else if k.trim().eq_ignore_ascii_case("content-range") {
                        if let Some((_, total_str)) = v.trim().rsplit_once('/') {
                            if let Ok(sz) = total_str.trim().parse::<u64>() {
                                self.total_size = Some(sz);
                            }
                        }
                    }
                }
                line.clear();
            }
        }

        // If HEAD didn't yield total_size, probe with 1-byte GET
        if self.total_size.is_none() {
            let mut tcp = self.connect()?;
            let mut req = format!(
                "GET {} HTTP/1.1\r\nHost: {}:{}\r\nRange: bytes=0-0\r\nConnection: close\r\n",
                self.path, self.host, self.port
            );
            if let Some(ref auth) = self.auth_header {
                req.push_str(auth);
                req.push_str("\r\n");
            }
            req.push_str("\r\n");
            tcp.write_all(req.as_bytes())?;

            let mut reader = BufReader::new(tcp);
            let mut status_line = String::new();
            reader.read_line(&mut status_line)?;

            let mut line = String::new();
            while reader.read_line(&mut line)? > 0 {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    break;
                }
                if let Some((k, v)) = trimmed.split_once(':') {
                    if k.trim().eq_ignore_ascii_case("content-range") {
                        if let Some((_, total_str)) = v.trim().rsplit_once('/') {
                            if let Ok(sz) = total_str.trim().parse::<u64>() {
                                self.total_size = Some(sz);
                            }
                        }
                    }
                }
                line.clear();
            }
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

        if let Some(total) = self.total_size {
            if self.position >= total {
                return Ok(0); // EOF
            }
        }

        let start = self.position;
        let mut end = start + buf.len() as u64 - 1;
        if let Some(total) = self.total_size {
            if end >= total {
                end = total - 1;
            }
        }

        if start > end {
            return Ok(0);
        }

        let mut tcp = self.connect()?;
        let mut req = format!(
            "GET {} HTTP/1.1\r\nHost: {}:{}\r\nRange: bytes={}-{}\r\nConnection: close\r\n",
            self.path, self.host, self.port, start, end
        );
        if let Some(ref auth) = self.auth_header {
            req.push_str(auth);
            req.push_str("\r\n");
        }
        req.push_str("\r\n");
        tcp.write_all(req.as_bytes())?;

        let mut reader = BufReader::new(tcp);
        let mut status_line = String::new();
        reader.read_line(&mut status_line)?;

        if !status_line.contains(" 206 ") && !status_line.contains(" 200 ") {
            return Err(std::io::Error::other(format!(
                "HTTP Range request failed: {}",
                status_line.trim()
            )));
        }

        // Consume headers until blank line
        let mut line = String::new();
        while reader.read_line(&mut line)? > 0 {
            if line.trim().is_empty() {
                break;
            }
            line.clear();
        }

        // Read body payload
        let max_to_read = (end - start + 1) as usize;
        let mut total_read = 0;
        while total_read < max_to_read {
            let n = reader.read(&mut buf[total_read..max_to_read])?;
            if n == 0 {
                break; // EOF
            }
            total_read += n;
        }

        self.position += total_read as u64;
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

    fn total_size(&self) -> Option<u64> {
        self.total_size
    }

    fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
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
    pub fn attach_to_handle(&self, handle: &MpvHandle) -> Result<(), MpvError> {
        let ctx = handle.as_ptr();
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
