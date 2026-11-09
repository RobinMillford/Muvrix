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
    assert!(provider.validate_config().is_ok());

    let stream_url = provider.resolve_stream_url("movies/film.mkv").unwrap();
    assert_eq!(stream_url, "https://nas.local:8443/movies/film.mkv");
    assert!(!stream_url.contains("secret_dav_pass"));
    assert!(!stream_url.contains("media_user"));

    let auth_header = provider.auth_header().expect("Auth header present");
    assert!(auth_header.starts_with("Authorization: Basic "));
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

                    // WebDAV PROPFIND handling
                    if request_line.starts_with("PROPFIND") {
                        let has_valid_auth = headers.iter().any(|h| h.contains("dXNlcjpwYXNz"));
                        if (request_line.contains("/protected") || request_line.contains("/dav"))
                            && !has_valid_auth
                        {
                            let response = "HTTP/1.1 401 Unauthorized\r\n\
                                            WWW-Authenticate: Basic realm=\"Muvrix\"\r\n\
                                            Content-Length: 0\r\n\
                                            Connection: close\r\n\r\n";
                            let _ = stream.write_all(response.as_bytes());
                            continue;
                        }

                        let propfind_body = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/protected/</D:href>
    <D:propstat>
      <D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/protected/clip.mp4</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>256</D:getcontentlength>
        <D:getlastmodified>Sun, 08 Nov 2026 12:00:00 GMT</D:getlastmodified>
        <D:getetag>"deadbeef-256"</D:getetag>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;
                        let response = format!(
                            "HTTP/1.1 207 Multi-Status\r\n\
                             Content-Type: application/xml; charset=\"utf-8\"\r\n\
                             Content-Length: {}\r\n\
                             Connection: close\r\n\r\n{}",
                            propfind_body.len(),
                            propfind_body
                        );
                        let _ = stream.write_all(response.as_bytes());
                        continue;
                    }

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

    // Helper to send HTTP request and read full response. No half-close
    // (shutdown Write): sending FIN immediately after the request lets the
    // mock server close with a RST that discards unread response bytes on
    // macOS (delivered fine on Linux), which broke header reads there. The
    // server stops parsing after the blank line, so it never needs the EOF.
    let send_request = |req: &str| -> (String, Vec<u8>) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("Connect to mock server");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("Set read timeout");
        stream.write_all(req.as_bytes()).expect("Write request");

        let mut reader = BufReader::new(stream);
        let mut header_text = String::new();
        let mut line = String::new();
        loop {
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    header_text.push_str(&line);
                    if line == "\r\n" || line == "\n" {
                        break;
                    }
                    line.clear();
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::ConnectionReset
                        || e.kind() == std::io::ErrorKind::NotConnected =>
                {
                    break;
                }
                Err(e) => panic!("read_line: {e}"),
            }
        }

        // Read exactly Content-Length bytes instead of relying on connection
        // close to signal EOF, so body reads are not timing-dependent.
        let content_length = header_text
            .lines()
            .find_map(|l| {
                let lower = l.to_ascii_lowercase();
                if let Some(val) = lower.strip_prefix("content-length:") {
                    val.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        let mut body = vec![0u8; content_length];
        match reader.read_exact(&mut body) {
            Ok(()) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::ConnectionReset
                    || e.kind() == std::io::ErrorKind::NotConnected
                    || e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                body.clear();
            }
            Err(e) => panic!("Read body: {e}"),
        }
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

    // 5. Test WebDavProvider live connection, directory listing, and stream URL resolution against this server
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

    assert!(provider
        .test_connection()
        .expect("WebDAV test_connection should succeed with valid auth"));

    let entries = provider
        .list_directory("/protected")
        .expect("WebDAV list_directory should succeed");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "clip.mp4");
    assert_eq!(entries[0].size_bytes, 256);
    assert!(!entries[0].is_dir);

    // Test bad credentials return AuthenticationFailed
    let bad_vault = CredentialVault::new();
    bad_vault.store("bad_cred", "wrong_pass");
    let bad_provider = WebDavProvider::new(
        "127.0.0.1",
        port,
        "/protected",
        false,
        Some("user".to_string()),
        Some("bad_cred".to_string()),
        bad_vault,
    );
    assert!(matches!(
        bad_provider.test_connection(),
        Err(NetworkError::AuthenticationFailed(_))
    ));

    let stream_url = provider.resolve_stream_url("clip.mp4").unwrap();
    assert_eq!(stream_url, format!("http://127.0.0.1:{}/clip.mp4", port));
    assert!(!stream_url.contains("pass"));
    assert!(!stream_url.contains("user"));

    let auth_header = provider.auth_header().expect("Auth header present");
    assert!(auth_header.starts_with("Authorization: Basic "));

    // Stop server
    is_running.store(false, Ordering::SeqCst);
    let _ = server_handle.join();
}

#[test]
fn test_webdav_propfind_xml_directory_listing_and_filtering() {
    let xml_payload = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/remote/webdav/Series/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/remote/webdav/Series/Season%2001/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/remote/webdav/Series/Episode%2001%20-%20Pilot.mkv</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>524288000</D:getcontentlength>
        <D:getlastmodified>Tue, 03 Nov 2026 14:20:00 GMT</D:getlastmodified>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/remote/webdav/Series/Episode%2002.mkv</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>480000000</D:getcontentlength>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

    let vault = CredentialVault::new();
    let provider = WebDavProvider::new(
        "nas.local",
        443,
        "/remote/webdav/Series",
        true,
        None,
        None,
        vault,
    );

    let entries = provider
        .parse_propfind(xml_payload, "/remote/webdav/Series/")
        .expect("Parse WebDAV PROPFIND");

    assert_eq!(entries.len(), 3, "Should exclude parent /Series/ directory");

    assert_eq!(entries[0].name, "Season 01");
    assert!(entries[0].is_dir);
    assert_eq!(entries[0].size_bytes, 0);

    assert_eq!(entries[1].name, "Episode 01 - Pilot.mkv");
    assert!(!entries[1].is_dir);
    assert_eq!(entries[1].size_bytes, 524288000);

    assert_eq!(entries[2].name, "Episode 02.mkv");
    assert!(!entries[2].is_dir);
    assert_eq!(entries[2].size_bytes, 480000000);
}

#[test]
fn test_smb_provider_lifecycle_and_url_resolution() {
    let vault = CredentialVault::new();
    let cred_ref = "smb-secret-uuid";
    vault.store(cred_ref, "StrongSmbPass#99");

    let provider = SmbProvider::new(
        "192.168.1.50",
        445,
        "MediaShare",
        "/Movies",
        Some("smb_user".to_string()),
        Some("WORKGROUP".to_string()),
        Some(cred_ref.to_string()),
        false,
        vault,
    );

    assert_eq!(provider.protocol(), "smb");
    assert_eq!(provider.unc_path(), "smb://192.168.1.50/MediaShare");
    assert!(provider.test_connection().unwrap());

    let stream_url = provider.resolve_stream_url("Movies/matrix.mkv").unwrap();
    assert_eq!(
        stream_url,
        "smb://WORKGROUP;smb_user:StrongSmbPass#99@192.168.1.50/MediaShare/Movies/matrix.mkv"
    );

    let redacted = redact_credentials_from_url(&stream_url);
    assert_eq!(
        redacted,
        "smb://[redacted]@192.168.1.50/MediaShare/Movies/matrix.mkv"
    );

    let listing = provider.list_directory("/Movies").unwrap();
    assert_eq!(listing.len(), 2);
    assert_eq!(listing[0].name, "Movies");
    assert!(listing[0].is_dir);
    assert_eq!(listing[1].name, "film_hd.mkv");
    assert!(!listing[1].is_dir);
}

#[test]
fn test_sftp_provider_host_key_verification_and_security() {
    let vault = CredentialVault::new();
    let cred_ref = "sftp-pass-uuid";
    vault.store(cred_ref, "SshSecretKeyPassword!");

    let pinned_fingerprint = "SHA256:uThbg6kIfTh2XX6mZa20WIPC8P48T0dEvn6g9Z91ioE";
    let provider = SftpProvider::new(
        "sftp.example.com",
        2222,
        "/home/media",
        "ssh_admin",
        Some(cred_ref.to_string()),
        None,
        Some(pinned_fingerprint.to_string()),
        vault.clone(),
    );

    assert_eq!(provider.protocol(), "sftp");
    assert!(provider.test_connection().unwrap());

    // 1. Valid host key matching pinned fingerprint
    assert!(provider.verify_host_key(pinned_fingerprint).is_ok());

    // 2. Mismatched host key triggers security error (MITM protection)
    let fake_fingerprint = "SHA256:FAKEFINGERPRINTDOESNOTMATCH0000000000000000";
    let err = provider.verify_host_key(fake_fingerprint).unwrap_err();
    match err {
        NetworkError::AuthenticationFailed(msg) => {
            assert!(msg.contains("SSH host key mismatch"));
            assert!(msg.contains("Man-In-The-Middle"));
        }
        other => panic!("Expected AuthenticationFailed, got: {:?}", other),
    }

    // 3. Unpinned host key is strictly rejected by default
    let unpinned_provider = SftpProvider::new(
        "sftp.example.com",
        2222,
        "/home/media",
        "ssh_admin",
        Some("cred_key".to_string()),
        None,
        None, // explicitly unpinned
        vault,
    );
    let unpinned_err = unpinned_provider
        .verify_host_key(pinned_fingerprint)
        .unwrap_err();
    match unpinned_err {
        NetworkError::AuthenticationFailed(msg) => {
            assert!(msg.contains("unpinned"));
        }
        other => panic!(
            "Expected AuthenticationFailed for unpinned host, got: {:?}",
            other
        ),
    }

    // 4. Stream URL resolution with credential redaction
    let stream_url = provider
        .resolve_stream_url("/home/media/sample.mp4")
        .unwrap();
    assert_eq!(
        stream_url,
        "sftp://ssh_admin:SshSecretKeyPassword!@sftp.example.com:2222/home/media/sample.mp4"
    );

    let redacted = redact_credentials_from_url(&stream_url);
    assert_eq!(
        redacted,
        "sftp://[redacted]@sftp.example.com:2222/home/media/sample.mp4"
    );
}

#[test]
fn test_sftp_persistent_host_trust_workflow_with_sqlite() {
    let db = muvrix_core::database::DatabaseManager::open_in_memory().expect("In-memory DB");
    let vault = CredentialVault::new();
    let cred_ref = "sftp-pass-test";
    vault.store(cred_ref, "SecretPass123!");

    let valid_fp = "SHA256:uThbg6kIfTh2XX6mZa20WIPC8P48T0dEvn6g9Z91ioE";
    let fake_fp = "SHA256:ATTACKERKEYFAKE00000000000000000000000000";

    // Create provider without pinned fingerprint, but with database attached
    let provider = SftpProvider::new(
        "sftp.mycloud.org",
        18222,
        "/media",
        "cloud_user",
        Some(cred_ref.to_string()),
        None,
        None, // No static pinning
        vault,
    )
    .with_database(db.clone());

    // 1. Unknown host without prior approval is rejected
    let err = provider.verify_host_key(valid_fp).unwrap_err();
    match err {
        NetworkError::AuthenticationFailed(msg) => {
            assert!(msg.contains("is not trusted"));
            assert!(msg.contains("Explicit user approval is required"));
        }
        other => panic!("Expected AuthenticationFailed, got: {:?}", other),
    }

    // 2. User explicitly reviews fingerprint and records trust (TOFU approval)
    db.record_ssh_host_trust(
        "sftp.mycloud.org",
        18222,
        "ssh-ed25519",
        valid_fp,
        Some("Approved production server"),
    )
    .expect("Record host trust");

    // 3. Subsequent connection with matching key passes
    assert!(provider.verify_host_key(valid_fp).is_ok());

    // 4. Server presents different key (potential MITM attack) -> Rejected with fatal warning!
    let mitm_err = provider.verify_host_key(fake_fp).unwrap_err();
    match mitm_err {
        NetworkError::AuthenticationFailed(msg) => {
            assert!(msg.contains("SSH host key mismatch"));
            assert!(msg.contains("Man-In-The-Middle"));
            assert!(msg.contains(valid_fp));
            assert!(msg.contains(fake_fp));
        }
        other => panic!("Expected AuthenticationFailed for MITM, got: {:?}", other),
    }

    // 5. User revokes trust -> connection is again blocked
    let revoked = db
        .revoke_ssh_host_trust("sftp.mycloud.org", 18222)
        .expect("Revoke");
    assert!(revoked);

    let re_blocked_err = provider.verify_host_key(valid_fp).unwrap_err();
    assert!(format!("{:?}", re_blocked_err).contains("is not trusted"));
}

#[test]
fn test_sftp_fingerprint_generation_and_padding_tolerance() {
    let raw_key = b"ssh-ed25519-mock-public-key-data";
    let computed_fp = SftpProvider::calculate_sha256_fingerprint(raw_key);
    assert!(computed_fp.starts_with("SHA256:"));

    let vault = CredentialVault::new();
    let provider = SftpProvider::new(
        "10.0.0.1",
        22,
        "/data",
        "user",
        None,
        Some("/id_rsa".into()),
        Some(computed_fp.clone()),
        vault,
    );

    // Exact match
    assert!(provider.verify_host_key(&computed_fp).is_ok());

    // Tolerance for trailing '='
    let with_padding = format!("{computed_fp}=");
    assert!(provider.verify_host_key(&with_padding).is_ok());

    // Tolerance for raw base64 without prefix
    let raw_b64 = computed_fp.strip_prefix("SHA256:").unwrap();
    assert!(provider.verify_host_key(raw_b64).is_ok());

    // Tampered / altered key must fail
    let tampered = format!("{computed_fp}XYZ");
    assert!(provider.verify_host_key(&tampered).is_err());
}

#[test]
fn test_sftp_real_openssh_tofu_approval_and_rejection_lifecycle() {
    let sshd_path = if std::path::Path::new("/usr/sbin/sshd").exists() {
        "/usr/sbin/sshd"
    } else if std::path::Path::new("/usr/bin/sshd").exists() {
        "/usr/bin/sshd"
    } else {
        println!("sshd not found on system; skipping real OpenSSH daemon test");
        return;
    };

    let temp_dir = std::env::temp_dir().join(format!(
        "muvrix_sshd_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let host_key1 = temp_dir.join("ssh_host_ed25519_key");
    let user_key = temp_dir.join("id_ed25519");
    let auth_keys = temp_dir.join("authorized_keys");

    // 1. Generate host key 1
    let gen_host = std::process::Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-f", host_key1.to_str().unwrap(), "-N", ""])
        .output()
        .expect("Generate host key");
    assert!(gen_host.status.success());

    // 2. Generate client key
    let gen_user = std::process::Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-f", user_key.to_str().unwrap(), "-N", ""])
        .output()
        .expect("Generate user key");
    assert!(gen_user.status.success());

    // Copy user_key.pub to authorized_keys
    let pub_data = std::fs::read(format!("{}.pub", user_key.to_str().unwrap())).unwrap();
    std::fs::write(&auth_keys, pub_data).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&auth_keys, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let port = 28228u16;
    let username = std::env::var("USER").unwrap_or_else(|_| "itsyamin".to_string());
    let cfg1 = temp_dir.join("sshd_config");
    let sftp_subsystem = if std::path::Path::new("/usr/libexec/openssh/sftp-server").exists() {
        "/usr/libexec/openssh/sftp-server"
    } else {
        "internal-sftp"
    };

    let sshd_conf_content = format!(
        "Port {}\nHostKey {}\nAuthorizedKeysFile {}\nLogLevel DEBUG3\nStrictModes no\nPubkeyAuthentication yes\nPasswordAuthentication no\nSubsystem sftp {}\nPidFile {}/sshd.pid\n",
        port, host_key1.display(), auth_keys.display(), sftp_subsystem, temp_dir.display()
    );
    std::fs::write(&cfg1, sshd_conf_content).unwrap();

    let log_file1 = std::fs::File::create(temp_dir.join("sshd1.log")).unwrap();
    let mut sshd_proc = std::process::Command::new(sshd_path)
        .args(["-f", cfg1.to_str().unwrap(), "-D", "-e"])
        .stderr(log_file1)
        .spawn()
        .expect("Spawn ephemeral sshd");

    std::thread::sleep(std::time::Duration::from_millis(400));

    let db_path = temp_dir.join("test_ssh_trust.db");
    let db = muvrix_core::database::DatabaseManager::open(&db_path).expect("Open SQLite DB");
    let vault = CredentialVault::new();

    let provider = SftpProvider::new(
        "127.0.0.1",
        port,
        "/",
        &username,
        None,
        Some(user_key.to_str().unwrap().to_string()),
        None, // No static pinning
        vault,
    )
    .with_database(db.clone());

    // Step 1: Probe fingerprint via inspect_host_key
    let presented = SftpProvider::inspect_host_key("127.0.0.1", port).expect("Inspect host key");
    assert!(presented.fingerprint.starts_with("SHA256:"));
    assert_eq!(presented.algorithm, "ssh-ed25519");

    // Step 2: Confirm server log proves zero user auth transmitted during key probing
    let log_pre_auth = std::fs::read_to_string(temp_dir.join("sshd1.log")).unwrap_or_default();
    assert!(
        !log_pre_auth.contains("userauth-request"),
        "Inspect host key must never initiate user authentication requests"
    );

    // Step 3: Fail-closed before approval
    let connect_err = provider.test_connection().unwrap_err();
    match connect_err {
        NetworkError::AuthenticationFailed(ref msg) => {
            assert!(msg.contains("is not trusted"));
            assert!(msg.contains("Explicit user approval is required"));
        }
        ref other => panic!("Expected AuthenticationFailed, got: {:?}", other),
    }

    // Step 4: Record user approval into SQLite trust store
    db.record_ssh_host_trust(
        "127.0.0.1",
        port,
        &presented.algorithm,
        &presented.fingerprint,
        Some("Approved local test SSH server"),
    )
    .expect("Record host trust");

    // Step 5: Authenticate successfully
    assert!(
        provider.test_connection().is_ok(),
        "Connection must succeed after host key approval"
    );

    // Step 6: Verify trust persists across DB restart (reopen DB from disk)
    drop(db);
    let reopened_db = muvrix_core::database::DatabaseManager::open(&db_path).expect("Reopen DB");
    let provider_reopened = SftpProvider::new(
        "127.0.0.1",
        port,
        "/",
        &username,
        None,
        Some(user_key.to_str().unwrap().to_string()),
        None,
        CredentialVault::new(),
    )
    .with_database(reopened_db.clone());
    assert!(
        provider_reopened.test_connection().is_ok(),
        "Trust must persist across DB reload"
    );

    // Step 7: Server changes host key (MITM attack / key rotation)
    let _ = sshd_proc.kill();
    let _ = sshd_proc.wait();

    let host_key2 = temp_dir.join("ssh_host_ed25519_key_tampered");
    let gen_host2 = std::process::Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-f", host_key2.to_str().unwrap(), "-N", ""])
        .output()
        .expect("Generate host key 2");
    assert!(gen_host2.status.success());

    let cfg2 = temp_dir.join("sshd_config_tampered");
    let sshd_conf_tampered = format!(
        "Port {}\nHostKey {}\nAuthorizedKeysFile {}\nLogLevel DEBUG3\nStrictModes no\nPubkeyAuthentication yes\nPasswordAuthentication no\nSubsystem sftp {}\nPidFile {}/sshd2.pid\n",
        port, host_key2.display(), auth_keys.display(), sftp_subsystem, temp_dir.display()
    );
    std::fs::write(&cfg2, sshd_conf_tampered).unwrap();

    let log_file2 = std::fs::File::create(temp_dir.join("sshd2.log")).unwrap();
    let mut sshd_proc2 = std::process::Command::new(sshd_path)
        .args(["-f", cfg2.to_str().unwrap(), "-D", "-e"])
        .stderr(log_file2)
        .spawn()
        .expect("Spawn tampered sshd");

    std::thread::sleep(std::time::Duration::from_millis(400));

    let mitm_err = provider_reopened.test_connection().unwrap_err();
    match mitm_err {
        NetworkError::AuthenticationFailed(ref msg) => {
            assert!(msg.contains("SSH host key mismatch"));
            assert!(msg.contains("Man-In-The-Middle"));
        }
        ref other => panic!("Expected AuthenticationFailed for MITM, got: {:?}", other),
    }

    // Step 8: Revoke trust in database
    let revoked = reopened_db
        .revoke_ssh_host_trust("127.0.0.1", port)
        .expect("Revoke trust");
    assert!(revoked);

    let reblocked_err = provider_reopened.test_connection().unwrap_err();
    match reblocked_err {
        NetworkError::AuthenticationFailed(ref msg) => {
            assert!(msg.contains("is not trusted"));
        }
        ref other => panic!(
            "Expected AuthenticationFailed after revoke, got: {:?}",
            other
        ),
    }

    let _ = sshd_proc2.kill();
    let _ = sshd_proc2.wait();
    let _ = std::fs::remove_dir_all(&temp_dir);
}
