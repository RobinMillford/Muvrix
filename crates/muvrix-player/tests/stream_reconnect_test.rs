use std::io::{Error, ErrorKind, Result as IoResult};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use muvrix_player::{
    classify_io_error, MemoryStream, ReadAheadStream, RecoverableStream, RecoveryState,
    SeekableStream, StreamErrorKind, StreamRetryPolicy, StreamTelemetry,
};

#[test]
fn test_retry_policy_exponential_backoff_calculation() {
    let policy = StreamRetryPolicy {
        max_retries: 5,
        initial_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(3000),
        backoff_factor: 2.0,
    };

    assert_eq!(policy.delay_for_attempt(0), Duration::ZERO);
    assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100)); // 100 * 2^0
    assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200)); // 100 * 2^1
    assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(400)); // 100 * 2^2
    assert_eq!(policy.delay_for_attempt(4), Duration::from_millis(800)); // 100 * 2^3
    assert_eq!(policy.delay_for_attempt(5), Duration::from_millis(1600)); // 100 * 2^4
    assert_eq!(policy.delay_for_attempt(6), Duration::from_millis(3000)); // capped at max_delay
}

#[test]
fn test_classify_io_error_transient_and_permanent() {
    // Transient errors
    assert_eq!(
        classify_io_error(&Error::new(ErrorKind::TimedOut, "timeout")),
        StreamErrorKind::Transient
    );
    assert_eq!(
        classify_io_error(&Error::new(ErrorKind::ConnectionReset, "connection reset")),
        StreamErrorKind::Transient
    );
    assert_eq!(
        classify_io_error(&Error::new(ErrorKind::BrokenPipe, "pipe broken")),
        StreamErrorKind::Transient
    );
    assert_eq!(
        classify_io_error(&Error::new(ErrorKind::UnexpectedEof, "unexpected eof")),
        StreamErrorKind::Transient
    );

    // Permanent errors
    assert_eq!(
        classify_io_error(&Error::new(ErrorKind::NotFound, "file not found")),
        StreamErrorKind::Permanent
    );
    assert_eq!(
        classify_io_error(&Error::new(ErrorKind::PermissionDenied, "unauthorized")),
        StreamErrorKind::Permanent
    );

    // Cancelled errors
    assert_eq!(
        classify_io_error(&Error::new(ErrorKind::Interrupted, "operation cancelled")),
        StreamErrorKind::Cancelled
    );
}

/// A simulated stream that fails with a transient error on the first read,
/// but succeeds after reconnection.
struct FlakyStream {
    data: Vec<u8>,
    pos: usize,
    fail_on_read: bool,
}

impl FlakyStream {
    fn new(data: Vec<u8>, fail_on_read: bool) -> Self {
        Self {
            data,
            pos: 0,
            fail_on_read,
        }
    }
}

impl SeekableStream for FlakyStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if self.fail_on_read {
            self.fail_on_read = false; // fail only once
            return Err(Error::new(ErrorKind::ConnectionReset, "connection dropped"));
        }
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let available = &self.data[self.pos..];
        let n = available.len().min(buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        self.pos += n;
        Ok(n)
    }

    fn seek(&mut self, position: u64) -> IoResult<u64> {
        let p = position as usize;
        if p > self.data.len() {
            return Err(Error::new(ErrorKind::InvalidInput, "seek out of bounds"));
        }
        self.pos = p;
        Ok(position)
    }

    fn total_size(&self) -> Option<u64> {
        Some(self.data.len() as u64)
    }
}

#[test]
fn test_recoverable_stream_transient_auto_reconnect() {
    let source_data = (0..512).map(|i| (i % 256) as u8).collect::<Vec<u8>>();
    let test_data = source_data.clone();
    let opener_calls = Arc::new(AtomicUsize::new(0));
    let calls_clone = opener_calls.clone();

    let telemetry = StreamTelemetry::new();
    let retry_policy = StreamRetryPolicy {
        max_retries: 3,
        initial_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(50),
        backoff_factor: 2.0,
    };

    let opener = move || -> IoResult<Box<dyn SeekableStream>> {
        let attempt = calls_clone.fetch_add(1, Ordering::SeqCst);
        let fail = attempt == 0; // Fail only on first opener connection
        Ok(Box::new(FlakyStream::new(test_data.clone(), fail)))
    };

    let mut recoverable =
        RecoverableStream::new_with_telemetry(opener, retry_policy, telemetry.clone())
            .expect("Recoverable stream initialization");

    let mut read_buf = vec![0u8; 512];
    let n = recoverable
        .read(&mut read_buf)
        .expect("Read with auto-reconnect");

    assert_eq!(n, 512);
    assert_eq!(&read_buf[..n], &source_data[..]);
    assert_eq!(telemetry.current_state(), RecoveryState::Connected);
    assert_eq!(telemetry.bytes_read(), 512);
    assert!(opener_calls.load(Ordering::SeqCst) >= 2);
    assert!(telemetry.reconnect_attempts() >= 1);
}

#[test]
fn test_readahead_stream_with_memory_source() {
    let source_data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
    let mem = MemoryStream::new(source_data.clone());
    let mut readahead = ReadAheadStream::with_capacity(mem, 256);

    let mut chunk1 = [0u8; 128];
    let n1 = readahead.read(&mut chunk1).expect("Read chunk 1");
    assert_eq!(n1, 128);
    assert_eq!(&chunk1, &source_data[..128]);

    let mut chunk2 = [0u8; 128];
    let n2 = readahead.read(&mut chunk2).expect("Read chunk 2");
    assert_eq!(n2, 128);
    assert_eq!(&chunk2, &source_data[128..256]);

    // Test seek within stream
    readahead.seek(500).expect("Seek to 500");
    let mut chunk3 = [0u8; 64];
    let n3 = readahead.read(&mut chunk3).expect("Read after seek");
    assert_eq!(n3, 64);
    assert_eq!(&chunk3, &source_data[500..564]);
}
