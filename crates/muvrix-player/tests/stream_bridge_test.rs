//! Integration and unit tests for libmpv custom stream callback bridge (`stream_cb.h`).

use muvrix_player::{
    HttpRangeStream, IoSeekableStream, MemoryStream, MpvHandle, MpvStreamBridge,
    PlaybackController, ReadAheadStream, SeekMode, SeekableStream,
};
use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn test_stream_bridge_registration_and_url_format() {
    let bridge = MpvStreamBridge::new();
    let sample_data = b"Hello, Muvrix Custom Stream Bridge!".to_vec();
    let stream = MemoryStream::new(sample_data.clone());

    let uri = bridge.register(stream);
    assert!(
        uri.starts_with("muvrix-stream://stream-"),
        "URI must match expected scheme: {uri}"
    );

    // Verify retrieval from registry
    let key = uri.strip_prefix("muvrix-stream://").unwrap();
    let taken = bridge.registry().take(key);
    assert!(taken.is_some(), "Stream must be retrievable from registry");
    assert_eq!(taken.unwrap().total_size(), Some(sample_data.len() as u64));
}

#[test]
fn test_stream_bridge_memory_read_and_seek() {
    let data = (0..256).map(|b| b as u8).collect::<Vec<u8>>();
    let mut stream = MemoryStream::new(data.clone());

    assert_eq!(stream.total_size(), Some(256));

    // Sequential read
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).expect("Read succeeded");
    assert_eq!(n, 64);
    assert_eq!(&buf[..], &data[0..64]);

    // Seek forward
    let pos = stream.seek(128).expect("Seek succeeded");
    assert_eq!(pos, 128);

    let n = stream.read(&mut buf).expect("Read succeeded");
    assert_eq!(n, 64);
    assert_eq!(&buf[..], &data[128..192]);

    // Seek backward
    let pos = stream.seek(10).expect("Seek backward succeeded");
    assert_eq!(pos, 10);

    let mut small_buf = [0u8; 5];
    let n = stream.read(&mut small_buf).expect("Read succeeded");
    assert_eq!(n, 5);
    assert_eq!(&small_buf[..], &data[10..15]);

    // Read to EOF
    stream.seek(250).unwrap();
    let n = stream.read(&mut buf).expect("Read near EOF");
    assert_eq!(n, 6); // Only 6 bytes remain (250..256)

    // Read past EOF returns 0
    let n = stream.read(&mut buf).expect("Read at EOF");
    assert_eq!(n, 0);

    // Seek beyond EOF returns error
    assert!(stream.seek(500).is_err());
}

#[test]
fn test_stream_bridge_io_seekable_stream() {
    let payload = b"Deterministic payload for IoSeekableStream testing".to_vec();
    let cursor = Cursor::new(payload.clone());
    let mut stream = IoSeekableStream::new(cursor);

    assert_eq!(stream.total_size(), Some(payload.len() as u64));

    let mut buf = [0u8; 13];
    let n = stream.read(&mut buf).unwrap();
    assert_eq!(n, 13);
    assert_eq!(&buf[..], b"Deterministic");

    let pos = stream.seek(14).unwrap();
    assert_eq!(pos, 14);

    let mut buf2 = [0u8; 7];
    let n2 = stream.read(&mut buf2).unwrap();
    assert_eq!(n2, 7);
    assert_eq!(&buf2[..], b"payload");
}

#[test]
fn test_mpv_custom_stream_live_playback() {
    // 1. Locate test fixture
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/test_video.mp4");

    if !fixture_path.exists() {
        eprintln!(
            "Fixture test_video.mp4 not found at {:?}, skipping live test",
            fixture_path
        );
        return;
    }

    let file_bytes = std::fs::read(&fixture_path).expect("Read test_video.mp4");
    assert!(!file_bytes.is_empty());

    // 2. Initialize mpv handle with headless null vo/ao
    let handle = MpvHandle::new().expect("Create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize mpv handle");

    // 3. Attach custom stream bridge
    let bridge = MpvStreamBridge::new();
    bridge
        .attach_to_handle(&handle)
        .expect("Attach stream bridge to handle");

    // 4. Register memory stream with MP4 fixture bytes
    let stream = MemoryStream::new(file_bytes);
    let stream_url = bridge.register(stream);

    // 5. Issue loadfile command to mpv
    handle
        .command(&["loadfile", &stream_url])
        .expect("Load custom stream file into mpv");

    // 6. Wait for file-loaded event or check duration
    let mut loaded = false;
    for _ in 0..50 {
        let event = handle.wait_event(0.1);
        match event {
            muvrix_player::Event::FileLoaded => {
                loaded = true;
                break;
            }
            muvrix_player::Event::EndFile {
                reason: muvrix_player::EndFileReason::Error,
                error,
                ..
            } => {
                panic!("mpv failed to demux stream: EndFile(Error: {:?})", error);
            }
            _ => {}
        }
    }

    assert!(loaded, "FileLoaded event must be emitted for custom stream");

    // 7. Verify demuxer read file duration successfully from custom stream
    let duration: Result<f64, _> = handle.get_property("duration");
    if let Ok(dur) = duration {
        assert!(dur > 0.0, "Stream duration must be positive, got {dur}");
    }

    // 8. Test seek on custom stream
    handle
        .command(&["seek", "0.5", "absolute"])
        .expect("Seek command on custom stream");

    // Allow demuxer to seek
    let _ = handle.wait_event(0.2);

    // Clean stop
    let _ = handle.command(&["stop"]);
}

#[test]
fn test_webdav_range_server_and_mpv_stream_playback() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/test_video.mp4");

    if !fixture_path.exists() {
        eprintln!("Fixture test_video.mp4 not found, skipping");
        return;
    }

    let file_bytes = Arc::new(std::fs::read(&fixture_path).expect("Read test_video.mp4"));
    let listener = TcpListener::bind("127.0.0.1:0").expect("Bind local server");
    let port = listener.local_addr().unwrap().port();
    let is_running = Arc::new(AtomicBool::new(true));

    let server_bytes = Arc::clone(&file_bytes);
    let server_running = Arc::clone(&is_running);

    let server_handle = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        while server_running.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut req_line = String::new();
                    if reader.read_line(&mut req_line).is_err() || req_line.is_empty() {
                        continue;
                    }

                    let mut range_header = None;
                    let mut line = String::new();
                    while let Ok(n) = reader.read_line(&mut line) {
                        if n == 0 || line.trim().is_empty() {
                            break;
                        }
                        if line.to_lowercase().starts_with("range:") {
                            range_header = Some(line.trim().to_string());
                        }
                        line.clear();
                    }

                    if req_line.starts_with("HEAD") {
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                            server_bytes.len()
                        );
                        let _ = stream.write_all(resp.as_bytes());
                        let _ = stream.shutdown(std::net::Shutdown::Write);
                    } else if req_line.starts_with("GET") {
                        if let Some(ref range) = range_header {
                            if let Some(spec) = range
                                .strip_prefix("Range: bytes=")
                                .or_else(|| range.strip_prefix("range: bytes="))
                            {
                                let parts: Vec<&str> = spec.split('-').collect();
                                let start: usize = parts[0].trim().parse().unwrap_or(0);
                                let end: usize = if parts.len() > 1 && !parts[1].trim().is_empty() {
                                    parts[1].trim().parse().unwrap_or(server_bytes.len() - 1)
                                } else {
                                    server_bytes.len() - 1
                                };
                                let end = end.min(server_bytes.len() - 1);
                                if start <= end && start < server_bytes.len() {
                                    let chunk = &server_bytes[start..=end];
                                    let resp = format!(
                                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {}-{}/{}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                                        start,
                                        end,
                                        server_bytes.len(),
                                        chunk.len()
                                    );
                                    let _ = stream.write_all(resp.as_bytes());
                                    let _ = stream.write_all(chunk);
                                    let _ = stream.shutdown(std::net::Shutdown::Write);
                                }
                            }
                        } else {
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                                server_bytes.len()
                            );
                            let _ = stream.write_all(resp.as_bytes());
                            let _ = stream.write_all(&server_bytes);
                            let _ = stream.shutdown(std::net::Shutdown::Write);
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                _ => break,
            }
        }
    });

    // 1. Instantiate HttpRangeStream connecting to our local HTTP Range server
    let range_stream = HttpRangeStream::new(
        "127.0.0.1",
        port,
        "/test_video.mp4",
        Some("Authorization: Basic dXNlcjpwYXNz".to_string()),
    )
    .expect("Connect HttpRangeStream");

    assert_eq!(range_stream.total_size(), Some(file_bytes.len() as u64));

    // 2. Set up libmpv handle and custom stream bridge
    let handle = Arc::new(MpvHandle::new().expect("Create mpv handle"));
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize handle");

    let bridge = MpvStreamBridge::new();
    bridge
        .attach_to_handle(&handle)
        .expect("Attach stream bridge");

    let uri = bridge.register(range_stream);
    assert!(uri.starts_with("muvrix-stream://"));

    // 3. Play via PlaybackController
    let mut controller = PlaybackController::new(handle).expect("Create controller");
    controller
        .load_file(&uri)
        .expect("Load stream into controller");

    let mut loaded = false;
    for _ in 0..50 {
        for event in controller.poll_events() {
            if matches!(event, muvrix_player::Event::FileLoaded) {
                loaded = true;
                break;
            }
        }
        if loaded {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        loaded,
        "libmpv must successfully load and demux media over HttpRangeStream"
    );

    // 4. Seek forward and backward over HTTP Range
    let _ = controller.seek(1.5, SeekMode::Absolute);
    thread::sleep(Duration::from_millis(100));

    let _ = controller.seek(0.2, SeekMode::Absolute);
    thread::sleep(Duration::from_millis(100));

    // 5. Clean teardown
    let _ = controller.stop();
    is_running.store(false, Ordering::SeqCst);
    let _ = server_handle.join();
}

struct PanickingStream;

impl SeekableStream for PanickingStream {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        panic!("Simulated read panic in C-FFI callback!");
    }
    fn seek(&mut self, _pos: u64) -> std::io::Result<u64> {
        panic!("Simulated seek panic in C-FFI callback!");
    }
    fn total_size(&self) -> Option<u64> {
        panic!("Simulated size panic in C-FFI callback!");
    }
    fn cancel(&mut self) {
        panic!("Simulated cancel panic in C-FFI callback!");
    }
}

#[test]
fn test_c_ffi_stream_callbacks_panic_and_error_handling() {
    let handle = Arc::new(MpvHandle::new().expect("Create handle"));
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize handle");

    let bridge = MpvStreamBridge::new();
    bridge.attach_to_handle(&handle).expect("Attach bridge");

    // Test unknown / unregistered stream returns error cleanly without crashing
    let invalid_uri = "muvrix-stream://non-existent-stream-key";
    let _ = handle.command(&["loadfile", invalid_uri]);
    for _ in 0..20 {
        let event = handle.wait_event(0.05);
        if let muvrix_player::Event::EndFile { reason, .. } = event {
            assert_eq!(reason, muvrix_player::EndFileReason::Error);
            break;
        }
    }

    // Test panicking stream: must not crash the process or unwind across C-ABI.
    // Suppress default panic hook output during deliberate panic verification.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let panicking_stream = PanickingStream;
    let panic_uri = bridge.register(panicking_stream);
    let _ = handle.command(&["loadfile", &panic_uri]);
    for _ in 0..20 {
        let event = handle.wait_event(0.05);
        if let muvrix_player::Event::EndFile { reason, .. } = event {
            assert_eq!(reason, muvrix_player::EndFileReason::Error);
            break;
        }
    }
    std::panic::set_hook(prev_hook);

    let _ = handle.command(&["stop"]);
}

struct NonSeekableStream {
    data: Vec<u8>,
    pos: usize,
    cancelled: bool,
}

impl SeekableStream for NonSeekableStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancelled {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled",
            ));
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

    fn seek(&mut self, _pos: u64) -> std::io::Result<u64> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Seeking unsupported on live stream",
        ))
    }

    fn total_size(&self) -> Option<u64> {
        None // Indeterminate stream length
    }

    fn cancel(&mut self) {
        self.cancelled = true;
    }
}

#[test]
fn test_stream_bridge_concurrent_streams_and_independent_seeking() {
    let bridge = MpvStreamBridge::new();
    let data1 = b"Stream One Payload AAAAAAAAAA".to_vec();
    let data2 = b"Stream Two Payload BBBBBBBBBB".to_vec();

    let uri1 = bridge.register(MemoryStream::new(data1.clone()));
    let uri2 = bridge.register(MemoryStream::new(data2.clone()));

    assert_ne!(uri1, uri2);

    let key1 = uri1.strip_prefix("muvrix-stream://").unwrap();
    let key2 = uri2.strip_prefix("muvrix-stream://").unwrap();

    let mut stream1 = bridge.registry().take(key1).expect("Take stream 1");
    let mut stream2 = bridge.registry().take(key2).expect("Take stream 2");

    assert_eq!(stream1.total_size(), Some(data1.len() as u64));
    assert_eq!(stream2.total_size(), Some(data2.len() as u64));

    // Seek stream 1 forward, leave stream 2 at 0
    stream1.seek(11).unwrap();
    let mut buf1 = [0u8; 7];
    let n1 = stream1.read(&mut buf1).unwrap();
    assert_eq!(n1, 7);
    assert_eq!(&buf1, b"Payload");

    // Stream 2 should still be at 0
    let mut buf2 = [0u8; 10];
    let n2 = stream2.read(&mut buf2).unwrap();
    assert_eq!(n2, 10);
    assert_eq!(&buf2, b"Stream Two");

    // Independent seek on stream 2
    stream2.seek(19).unwrap();
    let mut buf3 = [0u8; 5];
    let n3 = stream2.read(&mut buf3).unwrap();
    assert_eq!(n3, 5);
    assert_eq!(&buf3, b"BBBBB");
}

#[test]
fn test_stream_bridge_unseekable_stream_and_cancellation() {
    let mut stream = NonSeekableStream {
        data: b"Live radio broadcast chunk 12345".to_vec(),
        pos: 0,
        cancelled: false,
    };

    assert_eq!(stream.total_size(), None);
    assert!(stream.seek(10).is_err());

    let mut buf = [0u8; 4];
    let n = stream.read(&mut buf).unwrap();
    assert_eq!(n, 4);
    assert_eq!(&buf, b"Live");

    // Cancel stream and verify subsequent read fails with Interrupted
    stream.cancel();
    let err = stream.read(&mut buf).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
}

/// Mock instrumented stream tracking physical read and seek invocations.
struct InstrumentedStream {
    data: Vec<u8>,
    pos: usize,
    read_calls: usize,
    seek_calls: usize,
}

impl SeekableStream for InstrumentedStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.read_calls += 1;
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let avail = &self.data[self.pos..];
        let n = avail.len().min(buf.len());
        buf[..n].copy_from_slice(&avail[..n]);
        self.pos += n;
        Ok(n)
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        self.seek_calls += 1;
        self.pos = position as usize;
        Ok(position)
    }

    fn total_size(&self) -> Option<u64> {
        Some(self.data.len() as u64)
    }
}

#[test]
fn test_read_ahead_stream_caching_and_seeking() {
    let raw_payload: Vec<u8> = (0..256).map(|i| (i % 256) as u8).collect();
    let mock = InstrumentedStream {
        data: raw_payload,
        pos: 0,
        read_calls: 0,
        seek_calls: 0,
    };

    // 64-byte bounded buffer capacity
    let mut buffered = ReadAheadStream::with_capacity(mock, 64);
    assert_eq!(buffered.total_size(), Some(256));

    // 1. Read first 16 bytes: fills 64 bytes into buffer in 1 underlying read
    let mut buf = [0u8; 16];
    let n1 = buffered.read(&mut buf).unwrap();
    assert_eq!(n1, 16);
    assert_eq!(
        &buf[..],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
    );

    // 2. Read next 16 bytes: served entirely from memory buffer (no extra underlying read)
    let n2 = buffered.read(&mut buf).unwrap();
    assert_eq!(n2, 16);
    assert_eq!(
        &buf[..],
        &[16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]
    );

    // 3. Seek backward within buffered window [0..64)
    let pos = buffered.seek(8).unwrap();
    assert_eq!(pos, 8);
    let mut small_buf = [0u8; 4];
    let n3 = buffered.read(&mut small_buf).unwrap();
    assert_eq!(n3, 4);
    assert_eq!(&small_buf, &[8, 9, 10, 11]);

    // 4. Seek forward beyond buffered window
    let pos2 = buffered.seek(100).unwrap();
    assert_eq!(pos2, 100);
    let mut jump_buf = [0u8; 4];
    let n4 = buffered.read(&mut jump_buf).unwrap();
    assert_eq!(n4, 4);
    assert_eq!(&jump_buf, &[100, 101, 102, 103]);
}

#[test]
fn test_http_range_stream_from_url_resolution() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Bind test port");
    let port = listener.local_addr().unwrap().port();

    let payload = b"MUVRIX_URL_SEEKABLE_RANGE_PAYLOAD_TEST_DATA";
    let server_bytes = payload.to_vec();

    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buffer = [0u8; 1024];
            if let Ok(n) = stream.read(&mut buffer) {
                let req_str = String::from_utf8_lossy(&buffer[..n]);
                if req_str.starts_with("HEAD") {
                    let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                            server_bytes.len()
                        );
                    let _ = stream.write_all(resp.as_bytes());
                } else if req_str.starts_with("GET") {
                    if let Some(range_idx) = req_str.find("Range: bytes=") {
                        let range_part = &req_str[range_idx + 13..];
                        if let Some(end_idx) = range_part.find("\r\n") {
                            let spec = &range_part[..end_idx];
                            let parts: Vec<&str> = spec.split('-').collect();
                            let start: usize = parts[0].trim().parse().unwrap_or(0);
                            let end: usize = if parts.len() > 1 && !parts[1].trim().is_empty() {
                                parts[1].trim().parse().unwrap_or(server_bytes.len() - 1)
                            } else {
                                server_bytes.len() - 1
                            };
                            let end = end.min(server_bytes.len() - 1);
                            if start <= end && start < server_bytes.len() {
                                let chunk = &server_bytes[start..=end];
                                let resp = format!(
                                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {}-{}/{}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                                        start, end, server_bytes.len(), chunk.len()
                                    );
                                let _ = stream.write_all(resp.as_bytes());
                                let _ = stream.write_all(chunk);
                            }
                        }
                    }
                }
            }
        }
    });

    let full_url = format!("http://127.0.0.1:{}/media/test_stream.mp4", port);
    let mut range_stream =
        HttpRangeStream::from_url(&full_url, None).expect("Initialize HttpRangeStream::from_url");

    assert_eq!(range_stream.total_size(), Some(payload.len() as u64));

    // Seek to offset 7 ("URL_S")
    range_stream.seek(7).unwrap();
    let mut buf = [0u8; 5];
    let n = range_stream.read(&mut buf).unwrap();
    assert_eq!(n, 5);
    assert_eq!(&buf, b"URL_S");
}

struct TrackedLifecycleStream {
    data: Vec<u8>,
    pos: usize,
    drop_count: Arc<AtomicUsize>,
}

impl TrackedLifecycleStream {
    fn new(data: Vec<u8>, drop_count: Arc<AtomicUsize>) -> Self {
        Self {
            data,
            pos: 0,
            drop_count,
        }
    }
}

impl Drop for TrackedLifecycleStream {
    fn drop(&mut self) {
        self.drop_count.fetch_add(1, Ordering::SeqCst);
    }
}

impl SeekableStream for TrackedLifecycleStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let avail = &self.data[self.pos..];
        let n = avail.len().min(buf.len());
        buf[..n].copy_from_slice(&avail[..n]);
        self.pos += n;
        Ok(n)
    }

    fn seek(&mut self, position: u64) -> std::io::Result<u64> {
        if position as usize > self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Seek beyond EOF",
            ));
        }
        self.pos = position as usize;
        Ok(position)
    }

    fn total_size(&self) -> Option<u64> {
        Some(self.data.len() as u64)
    }
}

#[test]
fn test_stream_bridge_repeated_open_read_seek_close_lifecycle_regression() {
    let bridge = MpvStreamBridge::new();
    let drop_counter = Arc::new(AtomicUsize::new(0));
    const ITERATIONS: usize = 50;

    for i in 0..ITERATIONS {
        let payload =
            format!("STREAM_PAYLOAD_LIFECYCLE_ITERATION_{i}_DATA_BLOCK_PADDING").into_bytes();
        let stream = TrackedLifecycleStream::new(payload.clone(), drop_counter.clone());
        let buffered = ReadAheadStream::with_capacity(stream, 512);

        let uri = bridge.register(buffered);
        let key = uri.strip_prefix("muvrix-stream://").unwrap();

        // 1. Emulate libmpv stream open via registry.take()
        let mut active_stream = bridge
            .registry()
            .take(key)
            .expect("Retrieve registered stream");
        assert_eq!(active_stream.total_size(), Some(payload.len() as u64));

        // 2. Read first chunk
        let mut buf = [0u8; 14];
        let n = active_stream.read(&mut buf).unwrap();
        assert_eq!(n, 14);
        assert_eq!(&buf, b"STREAM_PAYLOAD");

        // 3. Seek forward and read
        active_stream.seek(15).unwrap();
        let mut buf2 = [0u8; 9];
        let n2 = active_stream.read(&mut buf2).unwrap();
        assert_eq!(n2, 9);
        assert_eq!(&buf2, b"LIFECYCLE");

        // 4. Seek backward within buffered window
        active_stream.seek(0).unwrap();
        let mut buf3 = [0u8; 6];
        let n3 = active_stream.read(&mut buf3).unwrap();
        assert_eq!(n3, 6);
        assert_eq!(&buf3, b"STREAM");

        // 5. Emulate stream close / teardown by explicitly dropping active_stream
        drop(active_stream);

        // Verify drop occurred
        assert_eq!(drop_counter.load(Ordering::SeqCst), i + 1);
    }

    assert_eq!(drop_counter.load(Ordering::SeqCst), ITERATIONS);

    // Verify registry is completely empty (no lingering handles)
    let registry_map = bridge.registry();
    for i in 1..=ITERATIONS {
        let key = format!("stream-{i}");
        assert!(registry_map.take(&key).is_none());
    }
}

#[test]
fn test_stream_bridge_lifetime_when_bridge_dropped_early() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/test_video.mp4");
    if !fixture_path.exists() {
        eprintln!("SKIPPING test_stream_bridge_lifetime_when_bridge_dropped_early: test media fixture not found");
        return;
    }
    let file_bytes = std::fs::read(&fixture_path).expect("Read test media fixture");

    let handle = MpvHandle::new().expect("Create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize mpv handle");

    let stream_url = {
        let bridge = MpvStreamBridge::new();
        bridge
            .attach_to_handle(&handle)
            .expect("Attach stream bridge to handle");

        let stream = MemoryStream::new(file_bytes);
        let url = bridge.register(stream);
        // `bridge` is dropped here, but `handle.retain_resource` retains the registry
        url
    };

    handle
        .command(&["loadfile", &stream_url])
        .expect("Load custom stream file into mpv");

    let mut loaded = false;
    for _ in 0..50 {
        let event = handle.wait_event(0.1);
        match event {
            muvrix_player::Event::FileLoaded => {
                loaded = true;
                break;
            }
            muvrix_player::Event::EndFile {
                reason: muvrix_player::EndFileReason::Error,
                error,
                ..
            } => {
                panic!(
                    "mpv failed to demux stream after early bridge drop: EndFile(Error: {:?})",
                    error
                );
            }
            _ => {}
        }
    }

    assert!(
        loaded,
        "FileLoaded must be emitted when bridge was dropped early (structural ownership preserved)"
    );
}

#[test]
fn test_stream_bridge_handle_destruction_while_stream_active() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/test_video.mp4");
    if !fixture_path.exists() {
        eprintln!("SKIPPING test_stream_bridge_handle_destruction_while_stream_active: test fixture not found");
        return;
    }
    let file_bytes = std::fs::read(&fixture_path).expect("Read test media fixture");

    let drop_counter = Arc::new(AtomicUsize::new(0));
    let tracked_stream = TrackedLifecycleStream::new(file_bytes, drop_counter.clone());

    let handle = MpvHandle::new().expect("Create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize mpv handle");

    let bridge = MpvStreamBridge::new();
    bridge
        .attach_to_handle(&handle)
        .expect("Attach stream bridge");

    let url = bridge.register(tracked_stream);

    handle
        .command(&["loadfile", &url])
        .expect("Load stream into mpv");

    let mut loaded = false;
    for _ in 0..50 {
        if let muvrix_player::Event::FileLoaded = handle.wait_event(0.1) {
            loaded = true;
            break;
        }
    }
    assert!(loaded, "File must be loaded and actively streaming");

    // Explicitly destroy the handle while stream is active
    handle.terminate_destroy();

    // Verify stream was closed and dropped by mpv without leaking
    assert_eq!(
        drop_counter.load(Ordering::SeqCst),
        1,
        "Active stream must be cleanly dropped upon handle termination"
    );
}

#[test]
fn test_mpv_handle_repeated_teardown_safety() {
    let handle = MpvHandle::new().expect("Create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize handle");

    // First teardown
    handle.terminate_destroy();

    // Verify calling terminate_destroy or dropping does not crash or double-free
    // (MpvHandle implements Drop with atomic check-and-set)
}
