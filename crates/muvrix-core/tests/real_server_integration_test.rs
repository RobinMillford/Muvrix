//! Real-Server Integration Test Suite for Muvrix Network Providers.
//!
//! Validates actual network wire transactions against live local daemons on loopback (127.0.0.1):
//! - WebDAV: Lighttpd RFC 4918 with HTTP Basic Authentication and PROPFIND Depth 0/1.
//! - Samba: Samba 4 `smbd` daemon with SMB 2.0.2 to 3.1.1 dialect negotiation.
//! - OpenSSH: OpenSSH 9.7 `sshd` with SFTP subsystem and pinned Ed25519 host key.

use muvrix_core::network::{
    CredentialVault, NetworkError, NetworkProvider, SftpProvider, SmbProvider, WebDavProvider,
};
use std::net::TcpStream;
use std::time::Duration;

fn is_service_available(port: u16) -> bool {
    let addr = format!("127.0.0.1:{}", port);
    TcpStream::connect_timeout(
        &addr.parse().expect("Valid socket address"),
        Duration::from_millis(300),
    )
    .is_ok()
}

fn get_test_sftp_fingerprint() -> String {
    if let Ok(fp) = std::env::var("MUVRIX_TEST_SFTP_FINGERPRINT") {
        let t = fp.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fp_file = manifest
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/network-integration/sftp_fingerprint.txt");
    if let Ok(content) = std::fs::read_to_string(&fp_file) {
        let t = content.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Ok(out) = std::process::Command::new("podman")
        .args([
            "exec",
            "muvrix-test-openssh",
            "ssh-keygen",
            "-l",
            "-f",
            "/etc/ssh/ssh_host_ed25519_key.pub",
            "-E",
            "sha256",
        ])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(token) = s.split_whitespace().nth(1) {
                return token.to_string();
            }
        }
    }
    String::new()
}

#[test]
fn test_real_webdav_connection_and_propfind_traversal() {
    if !is_service_available(18080) {
        eprintln!("SKIPPING test_real_webdav_connection_and_propfind_traversal: WebDAV daemon on 127.0.0.1:18080 unavailable");
        return;
    }

    let vault = CredentialVault::new();
    vault.store("cred_webdav_1", "DavPassword123!");

    let provider = WebDavProvider::new(
        "127.0.0.1",
        18080,
        "/webdav",
        false,
        Some("muvrix_dav".to_string()),
        Some("cred_webdav_1".to_string()),
        vault.clone(),
    );

    // 1. Live reachability and PROPFIND Depth 0 probe
    let connection_ok = provider.test_connection().expect("WebDAV connection probe");
    assert!(connection_ok, "WebDAV server should report reachability");

    // 2. Collection listing via PROPFIND Depth 1
    let entries = provider
        .list_directory("/webdav")
        .expect("List WebDAV root collection");
    assert!(!entries.is_empty(), "Collection should not be empty");

    let sample_file = entries.iter().find(|e| e.name == "sample.mp4");
    assert!(sample_file.is_some(), "sample.mp4 must be listed");
    let sample = sample_file.unwrap();
    assert!(!sample.is_dir, "sample.mp4 should be a file");
    assert!(
        sample.size_bytes > 50_000,
        "sample.mp4 size should exceed 50KB"
    );

    let doc_file = entries.iter().find(|e| e.name == "document.txt");
    assert!(doc_file.is_some(), "document.txt must be listed");

    let nested_dir = entries.iter().find(|e| e.name == "nested");
    assert!(nested_dir.is_some(), "nested directory must be listed");
    assert!(nested_dir.unwrap().is_dir, "nested should be a directory");

    // 3. Nested traversal and unicode / special character handling
    let nested_entries = provider
        .list_directory("/webdav/nested")
        .expect("List WebDAV nested directory");
    assert!(
        nested_entries.iter().any(|e| e.name == "deep_video.mp4"),
        "nested/deep_video.mp4 must be present"
    );
    assert!(
        nested_entries.iter().any(|e| e.name.contains("файл")),
        "nested/unicode_файл.mp4 must be preserved without corrupting UTF-8"
    );
    assert!(
        nested_entries.iter().any(|e| e.name.contains("special")),
        "nested special characters must be preserved without double-escaping"
    );

    // 4. Authentication failure with invalid password
    let bad_vault = CredentialVault::new();
    bad_vault.store("cred_bad", "IncorrectPassword999!");
    let bad_provider = WebDavProvider::new(
        "127.0.0.1",
        18080,
        "/webdav",
        false,
        Some("muvrix_dav".to_string()),
        Some("cred_bad".to_string()),
        bad_vault,
    );
    let auth_res = bad_provider.test_connection();
    assert!(
        matches!(auth_res, Err(NetworkError::AuthenticationFailed(_))),
        "Bad password must yield AuthenticationFailed error, got: {:?}",
        auth_res
    );

    // 5. NotFound error for missing path
    let not_found_res = provider.list_directory("/webdav/nonexistent_dir_xyz");
    assert!(
        matches!(not_found_res, Err(NetworkError::NotFound(_))),
        "Missing remote path must yield NotFound error, got: {:?}",
        not_found_res
    );
}

#[test]
fn test_real_samba_connection_and_share_traversal() {
    if !is_service_available(18445) {
        eprintln!("SKIPPING test_real_samba_connection_and_share_traversal: Samba daemon on 127.0.0.1:18445 unavailable");
        return;
    }

    let vault = CredentialVault::new();
    vault.store("cred_smb_1", "SmbPassword123!");

    let provider = SmbProvider::new(
        "127.0.0.1",
        18445,
        "media",
        "",
        Some("muvrix_smb".to_string()),
        None,
        Some("cred_smb_1".to_string()),
        false,
        vault.clone(),
    );

    // 1. Live SMB2/3 connection and tree connect
    let conn_ok = provider.test_connection().expect("Samba connection probe");
    assert!(conn_ok, "Samba daemon should connect successfully");

    // 2. Share directory traversal
    let entries = provider.list_directory("").expect("List Samba root share");
    assert!(!entries.is_empty(), "Samba media share cannot be empty");

    let sample_file = entries.iter().find(|e| e.name == "sample.mp4");
    assert!(sample_file.is_some(), "sample.mp4 must exist on share");
    assert!(
        sample_file.unwrap().size_bytes > 50_000,
        "sample.mp4 must have non-zero size"
    );

    let nested_dir = entries.iter().find(|e| e.name == "nested");
    assert!(nested_dir.is_some(), "nested directory must exist");
    assert!(nested_dir.unwrap().is_dir, "nested must be a directory");

    // 3. Subdirectory traversal
    let nested_entries = provider
        .list_directory("nested")
        .expect("List Samba nested directory");
    assert!(
        nested_entries.iter().any(|e| e.name == "deep_video.mp4"),
        "nested/deep_video.mp4 must exist"
    );

    // 4. Authentication rejection on bad credentials
    let bad_vault = CredentialVault::new();
    bad_vault.store("cred_bad_smb", "WrongPasswordXYZ!");
    let bad_provider = SmbProvider::new(
        "127.0.0.1",
        18445,
        "media",
        "",
        Some("muvrix_smb".to_string()),
        None,
        Some("cred_bad_smb".to_string()),
        false,
        bad_vault,
    );
    let bad_conn = bad_provider.test_connection();
    assert!(
        bad_conn.is_err(),
        "Bad credentials must fail SMB connection, got: {:?}",
        bad_conn
    );

    // 5. Guest share connection
    let guest_provider = SmbProvider::new(
        "127.0.0.1",
        18445,
        "guest_media",
        "",
        None,
        None,
        None,
        true,
        vault,
    );
    let guest_ok = guest_provider.test_connection();
    assert!(
        guest_ok.is_ok(),
        "Guest access to guest_media share should succeed"
    );
}

#[test]
fn test_real_sftp_connection_and_security() {
    if !is_service_available(18222) {
        eprintln!("SKIPPING test_real_sftp_connection_and_security: OpenSSH daemon on 127.0.0.1:18222 unavailable");
        return;
    }

    let pinned_fingerprint = get_test_sftp_fingerprint();
    let vault = CredentialVault::new();
    vault.store("cred_sftp_1", "SftpPassword123!");

    let provider = SftpProvider::new(
        "127.0.0.1",
        18222,
        "/home/muvrix_sftp/media",
        "muvrix_sftp",
        Some("cred_sftp_1".to_string()),
        None,
        Some(pinned_fingerprint.clone()),
        vault.clone(),
    );

    // 1. Connection test with pinned host key
    let conn_ok = provider.test_connection().expect("SFTP connection test");
    assert!(conn_ok, "SFTP connection with pinned host key must succeed");

    // 2. Remote directory listing
    let entries = provider
        .list_directory("/home/muvrix_sftp/media")
        .expect("List SFTP directory");
    assert!(!entries.is_empty(), "SFTP media directory cannot be empty");

    let sample_file = entries.iter().find(|e| e.name == "sample.mp4");
    assert!(sample_file.is_some(), "sample.mp4 must exist via SFTP");
    assert!(
        sample_file.unwrap().size_bytes > 50_000,
        "sample.mp4 size must match"
    );

    let nested_dir = entries.iter().find(|e| e.name == "nested");
    assert!(nested_dir.is_some(), "nested dir must exist via SFTP");
    assert!(nested_dir.unwrap().is_dir, "nested must be directory");

    // 3. Security: Host key mismatch detection (MITM protection)
    let fake_fingerprint = "SHA256:FAKEFINGERPRINTDOESNOTMATCH00000000000000";
    let mitm_provider = SftpProvider::new(
        "127.0.0.1",
        18222,
        "/home/muvrix_sftp/media",
        "muvrix_sftp",
        provider.credential_ref.clone(),
        None,
        Some(fake_fingerprint.to_string()),
        vault.clone(),
    );
    let mitm_res = mitm_provider.test_connection();
    assert!(
        matches!(mitm_res, Err(NetworkError::AuthenticationFailed(ref msg)) if msg.contains("mismatch")),
        "Host key mismatch must abort connection and trigger AuthenticationFailed, got: {:?}",
        mitm_res
    );

    // 4. Security: Unpinned host key rejection
    let unpinned_provider = SftpProvider::new(
        "127.0.0.1",
        18222,
        "/home/muvrix_sftp/media",
        "muvrix_sftp",
        provider.credential_ref.clone(),
        None,
        None,
        vault,
    );
    let unpinned_res = unpinned_provider.test_connection();
    assert!(
        matches!(unpinned_res, Err(NetworkError::AuthenticationFailed(ref msg)) if msg.contains("unpinned")),
        "Unpinned host key must be rejected to prevent silent TOFU MITM, got: {:?}",
        unpinned_res
    );
}

#[test]
fn test_real_sftp_shell_access_rejected() {
    if !is_service_available(18222) {
        eprintln!("SKIPPING test_real_sftp_shell_access_rejected: OpenSSH daemon on 127.0.0.1:18222 unavailable");
        return;
    }

    let pinned_fingerprint = get_test_sftp_fingerprint();
    let vault = CredentialVault::new();
    vault.store("cred_sftp_shell", "SftpPassword123!");

    let provider = SftpProvider::new(
        "127.0.0.1",
        18222,
        "/home/muvrix_sftp/media",
        "muvrix_sftp",
        Some("cred_sftp_shell".to_string()),
        None,
        Some(pinned_fingerprint),
        vault,
    );

    let (sess, _sftp) = provider.open_session().expect("SSH session opened");
    let mut channel = sess.channel_session().expect("Open session channel");

    // Hardened sshd_config enforces ForceCommand internal-sftp and shell /sbin/nologin
    let exec_res = channel.exec("id");
    if exec_res.is_ok() {
        use std::io::Read;
        let mut output = String::new();
        let _ = channel.read_to_string(&mut output);
        assert!(
            !output.contains("uid="),
            "Hardened OpenSSH server must NOT execute interactive shell commands; got: {output}"
        );
    }
}

#[test]
fn test_real_sftp_port_forwarding_rejected() {
    if !is_service_available(18222) {
        eprintln!("SKIPPING test_real_sftp_port_forwarding_rejected: OpenSSH daemon on 127.0.0.1:18222 unavailable");
        return;
    }

    let pinned_fingerprint = get_test_sftp_fingerprint();
    let vault = CredentialVault::new();
    vault.store("cred_sftp_fwd", "SftpPassword123!");

    let provider = SftpProvider::new(
        "127.0.0.1",
        18222,
        "/home/muvrix_sftp/media",
        "muvrix_sftp",
        Some("cred_sftp_fwd".to_string()),
        None,
        Some(pinned_fingerprint),
        vault,
    );

    let (sess, _sftp) = provider.open_session().expect("SSH session opened");

    // AllowTcpForwarding no must administratively reject TCP tunneling
    let fwd_res = sess.channel_direct_tcpip("127.0.0.1", 80, None);
    assert!(
        fwd_res.is_err(),
        "Port forwarding must be administratively rejected by hardened sshd; got: {:?}",
        fwd_res.ok().map(|_| "channel opened")
    );
}

#[test]
fn test_real_sftp_public_key_authentication() {
    if !is_service_available(18222) {
        eprintln!("SKIPPING test_real_sftp_public_key_authentication: OpenSSH daemon on 127.0.0.1:18222 unavailable");
        return;
    }

    let pinned_fingerprint = get_test_sftp_fingerprint();
    let temp_dir = tempfile::tempdir().expect("Create temp dir for SSH keys");
    let client_key_path = temp_dir.path().join("test_client_ed25519");

    // 1. Generate client key pair
    let gen_status = std::process::Command::new("ssh-keygen")
        .args([
            "-t",
            "ed25519",
            "-f",
            client_key_path.to_str().unwrap(),
            "-N",
            "",
            "-q",
        ])
        .status()
        .expect("Generate test client Ed25519 key");
    assert!(gen_status.success(), "ssh-keygen must succeed");

    let client_pub_path = temp_dir.path().join("test_client_ed25519.pub");
    let pub_key_content = std::fs::read_to_string(&client_pub_path).expect("Read public key");

    // 2. Authorize client public key in the running container
    let mut child = std::process::Command::new("podman")
        .args([
            "exec",
            "-i",
            "muvrix-test-openssh",
            "sh",
            "-c",
            "cat >> /home/muvrix_sftp/.ssh/authorized_keys",
        ])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("Spawn podman exec to write authorized_keys");

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(pub_key_content.as_bytes()).unwrap();
    }
    let write_status = child.wait().expect("Wait for authorized_keys update");
    assert!(write_status.success(), "Adding authorized key must succeed");

    // 3. Connect using public key auth (no password)
    let vault = CredentialVault::new();
    let provider = SftpProvider::new(
        "127.0.0.1",
        18222,
        "/home/muvrix_sftp/media",
        "muvrix_sftp",
        None,
        Some(client_key_path.to_str().unwrap().to_string()),
        Some(pinned_fingerprint.clone()),
        vault.clone(),
    );

    let conn_ok = provider
        .test_connection()
        .expect("Pubkey SFTP connection test");
    assert!(
        conn_ok,
        "SFTP connection with valid client key must succeed"
    );

    let entries = provider
        .list_directory("/home/muvrix_sftp/media")
        .expect("List directory via pubkey auth");
    assert!(
        !entries.is_empty(),
        "Directory listing via pubkey auth must succeed"
    );

    // 4. Test unauthorized client key
    let unauthorized_key_path = temp_dir.path().join("unauthorized_ed25519");
    let gen_status2 = std::process::Command::new("ssh-keygen")
        .args([
            "-t",
            "ed25519",
            "-f",
            unauthorized_key_path.to_str().unwrap(),
            "-N",
            "",
            "-q",
        ])
        .status()
        .expect("Generate unauthorized client key");
    assert!(gen_status2.success());

    let unauth_provider = SftpProvider::new(
        "127.0.0.1",
        18222,
        "/home/muvrix_sftp/media",
        "muvrix_sftp",
        None,
        Some(unauthorized_key_path.to_str().unwrap().to_string()),
        Some(pinned_fingerprint),
        vault,
    );

    let unauth_res = unauth_provider.test_connection();
    assert!(
        matches!(unauth_res, Err(NetworkError::AuthenticationFailed(_))),
        "Unauthorized client key must be rejected; got: {:?}",
        unauth_res
    );

    // 5. Clean up authorized_keys in container
    let _ = std::process::Command::new("podman")
        .args([
            "exec",
            "muvrix-test-openssh",
            "sh",
            "-c",
            "echo -n > /home/muvrix_sftp/.ssh/authorized_keys",
        ])
        .status();
}
