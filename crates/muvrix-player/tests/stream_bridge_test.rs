//! Integration and unit tests for libmpv custom stream callback bridge (`stream_cb.h`).

use muvrix_player::{
    HttpRangeStream, IoSeekableStream, MemoryStream, MpvHandle, MpvStreamBridge,
    PlaybackController, SeekMode, SeekableStream,
};
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
                                }
                            }
                        } else {
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                                server_bytes.len()
                            );
                            let _ = stream.write_all(resp.as_bytes());
                            let _ = stream.write_all(&server_bytes);
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
