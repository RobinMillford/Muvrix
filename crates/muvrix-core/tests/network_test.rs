//! Integration tests for network providers, streaming URL resolution, and credential protection.

use muvrix_core::network::*;

#[test]
fn test_credential_vault_isolation_and_lifecycle() {
    let vault = CredentialVault::new();
    let ref_id = "cred-uuid-1234";

    assert!(vault.get(ref_id).is_none());

    vault.store(ref_id, "SuperSecretPassword123!");
    assert_eq!(
        vault.get(ref_id).as_deref(),
        Some("SuperSecretPassword123!")
    );

    vault.remove(ref_id);
    assert!(vault.get(ref_id).is_none());
}

#[test]
fn test_url_credential_redaction() {
    let secret_url = "http://admin:MyPassword@192.168.1.100:8080/Videos/movie.mkv";
    let redacted = redact_credentials_from_url(secret_url);

    assert_eq!(
        redacted,
        "http://[redacted]@192.168.1.100:8080/Videos/movie.mkv"
    );
    assert!(!redacted.contains("admin"));
    assert!(!redacted.contains("MyPassword"));

    let plain_url = "https://nas.local/public/video.mp4";
    assert_eq!(redact_credentials_from_url(plain_url), plain_url);
}

#[test]
fn test_webdav_provider_stream_url_and_endpoints() {
    let vault = CredentialVault::new();
    let cred_ref = "dav-cred-5678";
    vault.store(cred_ref, "secret_dav_pass");

    let provider = WebDavProvider::new(
        "nas.local",
        8443,
        "/dav",
        true,
        Some("media_user".to_string()),
        Some(cred_ref.to_string()),
        vault,
    );

    assert_eq!(provider.protocol(), "webdav");
    assert_eq!(provider.base_url(), "https://nas.local:8443");
    assert!(provider.test_connection().unwrap());

    let stream_url = provider.resolve_stream_url("movies/film.mkv").unwrap();
    assert_eq!(
        stream_url,
        "https://media_user:secret_dav_pass@nas.local:8443/movies/film.mkv"
    );

    let redacted_stream = redact_credentials_from_url(&stream_url);
    assert_eq!(
        redacted_stream,
        "https://[redacted]@nas.local:8443/movies/film.mkv"
    );
}

#[test]
fn test_mock_network_provider_traversal() {
    let provider = MockNetworkProvider::new("smb");
    assert_eq!(provider.protocol(), "smb");
    assert!(provider.test_connection().unwrap());

    let items = provider.list_directory("/").unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|i| i.is_dir && i.name == "Movies"));
    assert!(items.iter().any(|i| !i.is_dir && i.name == "sample.mp4"));
}

#[test]
fn test_http_range_and_webdav_streaming_simulation() {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    let listener = TcpListener::bind("127.0.0.1:0").expect("Bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    let is_running = Arc::new(AtomicBool::new(true));
    let is_running_clone = is_running.clone();

    let payload: Arc<Vec<u8>> = Arc::new((0u8..=255).collect());
    let payload_clone = payload.clone();

    // Spawn mock HTTP/WebDAV server thread
    let server_handle = thread::spawn(move || {
        listener.set_nonblocking(true).expect("set nonblocking");

        while is_running_clone.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut request_line = String::new();
                    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
                        continue;
                    }

                    let mut headers = Vec::new();
                    let mut line = String::new();
                    while reader.read_line(&mut line).is_ok() {
                        if line == "\r\n" || line == "\n" || line.is_empty() {
                            break;
                        }
                        headers.push(line.trim().to_string());
                        line.clear();
                    }

                    // Check for Authorization
                    let has_auth = headers
                        .iter()
                        .any(|h| h.to_lowercase().starts_with("authorization: basic"));

                    // Protected path check
                    if request_line.contains("/protected/") && !has_auth {
                        let response = "HTTP/1.1 401 Unauthorized\r\n\
                                        WWW-Authenticate: Basic realm=\"Muvrix\"\r\n\
                                        Content-Length: 0\r\n\
                                        Connection: close\r\n\r\n";
                        let _ = stream.write_all(response.as_bytes());
                        continue;
                    }

                    // Range check
                    let range_header = headers
                        .iter()
                        .find(|h| h.to_lowercase().starts_with("range: bytes="));

                    if let Some(range) = range_header {
                        // e.g. "Range: bytes=64-127"
                        let spec = range.split('=').nth(1).unwrap_or("").trim();
                        let parts: Vec<&str> = spec.split('-').collect();
                        let start: usize = parts[0].parse().unwrap_or(0);
                        let end: usize = if parts.len() > 1 && !parts[1].is_empty() {
                            parts[1].parse().unwrap_or(payload_clone.len() - 1)
                        } else {
                            payload_clone.len() - 1
                        };

                        let end = end.min(payload_clone.len() - 1);
                        let slice = &payload_clone[start..=end];
                        let len = slice.len();
                        let total = payload_clone.len();

                        let header = format!(
                            "HTTP/1.1 206 Partial Content\r\n\
                             Content-Type: video/mp4\r\n\
                             Content-Range: bytes {}-{}/{}\r\n\
                             Content-Length: {}\r\n\
                             Accept-Ranges: bytes\r\n\
                             Connection: close\r\n\r\n",
                            start, end, total, len
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(slice);
                    } else {
                        // Full content response
                        let header = format!(
                            "HTTP/1.1 200 OK\r\n\
                             Content-Type: video/mp4\r\n\
                             Content-Length: {}\r\n\
                             Accept-Ranges: bytes\r\n\
                             Connection: close\r\n\r\n",
                            payload_clone.len()
                        );
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(&payload_clone);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    // Helper to send HTTP request and read full response
    let send_request = |req: &str| -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Connect to mock server");
        stream.write_all(req.as_bytes()).expect("Write request");
        stream.shutdown(std::net::Shutdown::Write).unwrap();

        let mut reader = BufReader::new(stream);
        let mut header_text = String::new();
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap() > 0 {
            header_text.push_str(&line);
            if line == "\r\n" || line == "\n" {
                break;
            }
            line.clear();
        }

        let mut body = Vec::new();
        reader.read_to_end(&mut body).expect("Read body");
        (header_text, body)
    };

    // 1. Full content request (no Range)
    let (head, body) = send_request("GET /video.mp4 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert!(head.contains("200 OK"));
    assert!(head.contains("Accept-Ranges: bytes"));
    assert_eq!(body.len(), 256);
    assert_eq!(body[..], (0u8..=255).collect::<Vec<u8>>()[..]);

    // 2. HTTP Range request: bytes 64-127
    let (head, body) =
        send_request("GET /video.mp4 HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=64-127\r\n\r\n");
    assert!(head.contains("206 Partial Content"));
    assert!(head.contains("Content-Range: bytes 64-127/256"));
    assert_eq!(body.len(), 64);
    assert_eq!(body[..], (64u8..=127).collect::<Vec<u8>>()[..]);

    // 3. HTTP Range request: bytes 200-255 with basic auth
    let (head, body) = send_request(
        "GET /protected/clip.mp4 HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=200-255\r\nAuthorization: Basic dXNlcjpwYXNz\r\n\r\n",
    );
    assert!(head.contains("206 Partial Content"));
    assert!(head.contains("Content-Range: bytes 200-255/256"));
    assert_eq!(body.len(), 56);
    assert_eq!(body[..], (200u8..=255).collect::<Vec<u8>>()[..]);

    // 4. Protected request without credentials: 401 Unauthorized
    let (head, _) = send_request("GET /protected/clip.mp4 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert!(head.contains("401 Unauthorized"));

    // 5. Test WebDavProvider stream URL resolution against this server
    let vault = CredentialVault::new();
    vault.store("test_cred", "pass");
    let provider = WebDavProvider::new(
        "127.0.0.1",
        port,
        "/protected",
        false,
        Some("user".to_string()),
        Some("test_cred".to_string()),
        vault,
    );

    let stream_url = provider.resolve_stream_url("clip.mp4").unwrap();
    assert_eq!(
        stream_url,
        format!("http://user:pass@127.0.0.1:{}/clip.mp4", port)
    );
    let sanitized_url = redact_credentials_from_url(&stream_url);
    assert_eq!(
        sanitized_url,
        format!("http://[redacted]@127.0.0.1:{}/clip.mp4", port)
    );

    // Stop server
    is_running.store(false, Ordering::SeqCst);
    let _ = server_handle.join();
}
