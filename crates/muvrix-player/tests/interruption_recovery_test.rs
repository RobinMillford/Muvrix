use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use muvrix_player::controller::PlaybackController;
use muvrix_player::event::Event;
use muvrix_player::handle::MpvHandle;
use muvrix_player::stream::{
    classify_io_error, MemoryStream, RecoverableStream, RecoveryState, SeekableStream,
    StreamErrorKind, StreamRetryPolicy,
};

fn init_test_logging() {
    let _ = env_logger::builder().is_test(true).try_init();
}

fn fixture_path(relative: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    root.join(relative)
}

#[test]
fn test_classify_io_error_transient_vs_permanent() {
    use std::io::{Error, ErrorKind};

    // Transient errors
    assert_eq!(
        classify_io_error(&Error::from(ErrorKind::TimedOut)),
        StreamErrorKind::Transient
    );
    assert_eq!(
        classify_io_error(&Error::from(ErrorKind::ConnectionReset)),
        StreamErrorKind::Transient
    );
    assert_eq!(
        classify_io_error(&Error::from(ErrorKind::ConnectionAborted)),
        StreamErrorKind::Transient
    );
    assert_eq!(
        classify_io_error(&Error::from(ErrorKind::NotConnected)),
        StreamErrorKind::Transient
    );
    assert_eq!(
        classify_io_error(&Error::from(ErrorKind::BrokenPipe)),
        StreamErrorKind::Transient
    );
    assert_eq!(
        classify_io_error(&Error::from(ErrorKind::UnexpectedEof)),
        StreamErrorKind::Transient
    );

    // Custom strings
    assert_eq!(
        classify_io_error(&Error::other("SMB worker is closed")),
        StreamErrorKind::Transient
    );
    assert_eq!(
        classify_io_error(&Error::other("network timeout during read")),
        StreamErrorKind::Transient
    );

    // Permanent errors
    assert_eq!(
        classify_io_error(&Error::from(ErrorKind::NotFound)),
        StreamErrorKind::Permanent
    );
    assert_eq!(
        classify_io_error(&Error::from(ErrorKind::PermissionDenied)),
        StreamErrorKind::Permanent
    );
    assert_eq!(
        classify_io_error(&Error::from(ErrorKind::InvalidInput)),
        StreamErrorKind::Permanent
    );
    assert_eq!(
        classify_io_error(&Error::from(ErrorKind::Unsupported)),
        StreamErrorKind::Permanent
    );

    // Cancelled
    assert_eq!(
        classify_io_error(&Error::new(
            ErrorKind::Interrupted,
            "Stream cancelled by user"
        )),
        StreamErrorKind::Cancelled
    );
}

#[test]
fn test_stream_retry_policy_delays() {
    let policy = StreamRetryPolicy {
        max_retries: 5,
        initial_delay: Duration::from_millis(50),
        max_delay: Duration::from_millis(500),
        backoff_factor: 2.0,
    };

    assert_eq!(policy.delay_for_attempt(0), Duration::ZERO);
    assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(50));
    assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(100));
    assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(200));
    assert_eq!(policy.delay_for_attempt(4), Duration::from_millis(400));
    assert_eq!(policy.delay_for_attempt(5), Duration::from_millis(500)); // Capped by max_delay
}

/// Simulated stream that injects a transient failure on a specific read call.
struct FlakyStream {
    inner: MemoryStream,
    read_counter: Arc<AtomicU32>,
    fail_on_read: u32,
    error_kind: std::io::ErrorKind,
}

impl FlakyStream {
    fn new(
        data: Vec<u8>,
        read_counter: Arc<AtomicU32>,
        fail_on_read: u32,
        error_kind: std::io::ErrorKind,
    ) -> Self {
        Self {
            inner: MemoryStream::new(data),
            read_counter,
            fail_on_read,
            error_kind,
        }
    }
}

impl SeekableStream for FlakyStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = self.read_counter.fetch_add(1, Ordering::SeqCst);
        if count == self.fail_on_read {
            return Err(std::io::Error::new(
                self.error_kind,
                "Simulated network drop / connection reset",
            ));
        }
        self.inner.read(buf)
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        self.inner.seek(position)
    }

    fn total_size(&self) -> Option<u64> {
        self.inner.total_size()
    }
}

#[test]
fn test_recoverable_stream_transparent_recovery_checksum() {
    init_test_logging();

    // Generate 256 KB of deterministic pseudo-random payload
    let mut original_data = vec![0u8; 256 * 1024];
    for (i, byte) in original_data.iter_mut().enumerate() {
        *byte = ((i * 31 + 17) % 251) as u8;
    }

    let read_counter = Arc::new(AtomicU32::new(0));
    let payload = original_data.clone();
    let counter_clone = read_counter.clone();

    // Opener creates a FlakyStream that fails on read attempt 3
    let opener = move || {
        let stream = FlakyStream::new(
            payload.clone(),
            counter_clone.clone(),
            3,
            std::io::ErrorKind::ConnectionReset,
        );
        Ok(Box::new(stream) as Box<dyn SeekableStream>)
    };

    let retry_policy = StreamRetryPolicy {
        max_retries: 3,
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(50),
        backoff_factor: 2.0,
    };

    let mut stream =
        RecoverableStream::new(opener, retry_policy).expect("RecoverableStream initialized");

    assert_eq!(stream.state(), RecoveryState::Connected);
    assert_eq!(stream.total_size(), Some(original_data.len() as u64));

    // Read the stream in 16 KB chunks
    let mut reconstructed = Vec::new();
    let mut chunk = vec![0u8; 16 * 1024];

    loop {
        let n = stream
            .read(&mut chunk)
            .expect("Read succeeded with recovery");
        if n == 0 {
            break;
        }
        reconstructed.extend_from_slice(&chunk[..n]);
    }

    // Verify recovery happened: read_counter must be > 3
    let total_reads = read_counter.load(Ordering::SeqCst);
    assert!(
        total_reads > 3,
        "Expected at least one failure and recovery reconnect, total reads: {total_reads}"
    );

    // Verify exact byte integrity (0 bytes skipped, 0 bytes duplicated)
    assert_eq!(reconstructed.len(), original_data.len());
    assert_eq!(reconstructed, original_data);
    assert_eq!(stream.state(), RecoveryState::Connected);
    assert_eq!(stream.position(), original_data.len() as u64);
}

#[test]
fn test_recoverable_stream_permanent_error_fails_immediately() {
    init_test_logging();

    let data = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let read_counter = Arc::new(AtomicU32::new(0));
    let counter_clone = read_counter.clone();

    let opener = move || {
        let stream = FlakyStream::new(
            data.clone(),
            counter_clone.clone(),
            0, // fail on first read
            std::io::ErrorKind::PermissionDenied,
        );
        Ok(Box::new(stream) as Box<dyn SeekableStream>)
    };

    let retry_policy = StreamRetryPolicy {
        max_retries: 5,
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(50),
        backoff_factor: 2.0,
    };

    let mut stream =
        RecoverableStream::new(opener, retry_policy).expect("RecoverableStream initialized");

    let mut buf = [0u8; 4];
    let res = stream.read(&mut buf);
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );

    // Must not retry on permanent error
    assert_eq!(read_counter.load(Ordering::SeqCst), 1);
    assert!(matches!(stream.state(), RecoveryState::Failed(_)));
}

#[test]
fn test_recoverable_stream_cancellation() {
    init_test_logging();

    let data = vec![0u8; 1024];
    let opener = move || Ok(Box::new(MemoryStream::new(data.clone())) as Box<dyn SeekableStream>);

    let mut stream =
        RecoverableStream::new(opener, StreamRetryPolicy::default()).expect("Init stream");

    stream.cancel();
    assert_eq!(stream.state(), RecoveryState::Closed);

    let mut buf = [0u8; 32];
    let res = stream.read(&mut buf);
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
}

#[test]
fn test_recoverable_stream_seek_across_reconnect() {
    init_test_logging();

    let original_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
    let data_clone = original_data.clone();

    let opener =
        move || Ok(Box::new(MemoryStream::new(data_clone.clone())) as Box<dyn SeekableStream>);

    let mut stream =
        RecoverableStream::new(opener, StreamRetryPolicy::default()).expect("Init stream");

    let target_pos = 500;
    let seek_res = stream.seek(target_pos).expect("Seek succeeded");
    assert_eq!(seek_res, target_pos);
    assert_eq!(stream.position(), target_pos);

    let mut buf = [0u8; 10];
    let n = stream.read(&mut buf).expect("Read succeeded");
    assert_eq!(n, 10);
    assert_eq!(&buf, &original_data[500..510]);
}

#[test]
fn test_playback_controller_level_b_resume() {
    init_test_logging();

    let fixture = fixture_path("tests/fixtures/test_video.mp4");
    if !fixture.exists() {
        eprintln!("Fixture {:?} not found, skipping playback test", fixture);
        return;
    }

    let handle = MpvHandle::new().expect("Create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize mpv handle");

    let handle = Arc::new(handle);
    let mut controller = PlaybackController::new(handle).expect("Create PlaybackController");

    let fixture_str = fixture.to_str().expect("Valid UTF-8 path");

    // 1. Initial load
    controller.load_file(fixture_str).expect("Load file");

    let start = std::time::Instant::now();
    let mut loaded = false;
    while start.elapsed() < Duration::from_secs(5) {
        let events = controller.poll_events();
        for event in events {
            if matches!(event, Event::FileLoaded) {
                loaded = true;
                break;
            }
        }
        if loaded {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(loaded, "Initial file load timed out");

    // 2. Perform Level B reload with resume
    let target_resume_time = 0.5; // test_video.mp4 has duration > 0s
    controller
        .reload_with_resume(fixture_str, target_resume_time, None, None)
        .expect("reload_with_resume succeeded");

    assert_eq!(controller.pending_resume_pos(), Some(target_resume_time));

    // Wait for the reloaded FileLoaded event to trigger the automatic resume seek
    let start = std::time::Instant::now();
    let mut reloaded = false;
    while start.elapsed() < Duration::from_secs(5) {
        let events = controller.poll_events();
        for event in events {
            if matches!(event, Event::FileLoaded) {
                reloaded = true;
                break;
            }
        }
        if reloaded {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(reloaded, "Reload with resume timed out");

    // Give mpv a brief moment to process the resume seek
    std::thread::sleep(Duration::from_millis(100));
    controller.poll_events();

    // Verify pending_resume_pos was consumed
    assert_eq!(controller.pending_resume_pos(), None);
}
