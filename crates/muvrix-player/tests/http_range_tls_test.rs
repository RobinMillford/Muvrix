use std::io::{Read, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use muvrix_player::{HttpRangeStream, SeekableStream};

fn read_request(stream: &mut TcpStream) -> Option<String> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buffer.extend_from_slice(&chunk[..n]);
                if buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    String::from_utf8(buffer).ok()
}

fn range_bounds(request: &str) -> Option<(usize, usize)> {
    let line = request
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("range:"))?;
    let value = line.split_once(':')?.1.trim();
    let value = value.strip_prefix("bytes=")?;
    let (start, end) = value.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?))
}

#[test]
fn http_range_stream_reads_and_seeks_over_real_http() {
    let payload = (0..64 * 1024).map(|i| (i % 251) as u8).collect::<Vec<_>>();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind HTTP test server");
    let port = listener.local_addr().expect("addr").port();
    let stop = Arc::new(AtomicBool::new(false));
    let server_payload = payload.clone();
    let server_stop = stop.clone();
    let handle = thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        while !server_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let Some(request) = read_request(&mut stream) else {
                        continue;
                    };
                    let method = request.split_whitespace().next().unwrap_or("");
                    let bounds = range_bounds(&request);
                    let (status, start, end) = match bounds {
                        Some((start, end)) => {
                            let start = start.min(server_payload.len().saturating_sub(1));
                            let end = end.min(server_payload.len().saturating_sub(1));
                            (206, start, end)
                        }
                        None => (200, 0, server_payload.len().saturating_sub(1)),
                    };
                    let body = if method == "HEAD" {
                        &[][..]
                    } else {
                        &server_payload[start..=end]
                    };
                    let headers = if status == 206 {
                        format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                            body.len(),
                            server_payload.len()
                        )
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                            server_payload.len()
                        )
                    };
                    let _ = stream.write_all(headers.as_bytes());
                    let _ = stream.write_all(body);
                }
                Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    let url = format!("http://127.0.0.1:{port}/media/sample.mp4");
    let mut stream =
        HttpRangeStream::from_url(&url, Some("Authorization: Basic dGVzdDp0ZXN0".to_string()))
            .expect("open TLS-capable range stream");
    assert_eq!(stream.total_size(), Some(payload.len() as u64));

    let mut first = [0u8; 7];
    assert_eq!(stream.read(&mut first).expect("read prefix"), first.len());
    assert_eq!(&first, &payload[..7]);

    assert_eq!(
        stream
            .seek_from(SeekFrom::Current(993))
            .expect("seek from current"),
        1000
    );
    let mut middle = [0u8; 11];
    assert_eq!(stream.read(&mut middle).expect("read middle"), middle.len());
    assert_eq!(&middle, &payload[1000..1011]);

    let tail = payload.len() as u64 - 7;
    assert_eq!(
        stream.seek_from(SeekFrom::End(-7)).expect("seek from end"),
        tail
    );
    let mut last = [0u8; 7];
    assert_eq!(stream.read(&mut last).expect("read tail"), last.len());
    assert_eq!(&last, &payload[payload.len() - 7..]);

    assert_eq!(
        stream
            .seek_from(SeekFrom::Start(0))
            .expect("seek from start"),
        0
    );
    let mut again = [0u8; 7];
    assert_eq!(stream.read(&mut again).expect("read rewind"), again.len());
    assert_eq!(&again, &payload[..7]);

    stop.store(true, Ordering::Relaxed);
    let _ = TcpStream::connect(("127.0.0.1", port));
    handle.join().expect("join HTTP server");
}

#[test]
fn https_is_not_downgraded_to_plaintext_http() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind TLS probe server");
    let port = listener.local_addr().expect("addr").port();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("server read timeout");
            let mut bytes = [0u8; 512];
            let n = stream.read(&mut bytes).unwrap_or(0);
            let _ = tx.send(bytes[..n].to_vec());
        }
    });

    let url = format!("https://127.0.0.1:{port}/media/sample.mp4");
    let result = HttpRangeStream::from_url(&url, None);
    assert!(
        result.is_err(),
        "plain HTTP server must reject TLS handshake"
    );

    let first_bytes = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("server observed TLS client bytes");
    assert!(
        !first_bytes.starts_with(b"GET ") && !first_bytes.starts_with(b"HEAD "),
        "https must never emit a plaintext HTTP request, got {:?}",
        String::from_utf8_lossy(&first_bytes[..first_bytes.len().min(16)])
    );
    handle.join().expect("join TLS probe server");
}
