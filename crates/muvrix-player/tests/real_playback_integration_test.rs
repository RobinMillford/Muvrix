//! Real Network Media Playback Integration Test Suite for Muvrix.
//!
//! Validates actual end-to-end media demuxing, decoding initialization, seeking,
//! and clean stream closure using real `libmpv` connected via `MpvStreamBridge`
//! to live local network daemons on loopback (127.0.0.1):
//! - WebDAV (18080)
//! - Samba SMB2/3 (18445)
//! - OpenSSH SFTP (18222)

use std::net::TcpStream;
use std::time::Duration;

use muvrix_core::network::{CredentialVault, SftpProvider};
use muvrix_player::stream::{
    HttpRangeStream, MpvStreamBridge, ReadAheadStream, SeekableStream, SftpSeekableStream,
    SmbSeekableStream,
};
use muvrix_player::{EndFileReason, Event, MpvHandle};

fn is_service_available(port: u16) -> bool {
    let addr = format!("127.0.0.1:{}", port);
    TcpStream::connect_timeout(
        &addr.parse().expect("Valid socket address"),
        Duration::from_millis(300),
    )
    .is_ok()
}

fn create_test_mpv_handle() -> MpvHandle {
    let handle = MpvHandle::new().expect("Create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize mpv handle");
    handle
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
    panic!("Could not discover dynamic SFTP host fingerprint. Run setup.sh first.");
}

fn wait_for_file_loaded(handle: &MpvHandle, max_polls: usize) -> bool {
    for _ in 0..max_polls {
        let event = handle.wait_event(0.1);
        match event {
            Event::FileLoaded => return true,
            Event::EndFile {
                reason: EndFileReason::Error,
                error,
                ..
            } => {
                panic!("libmpv playback failed with EndFile(Error: {:?})", error);
            }
            _ => {}
        }
    }
    false
}

#[test]
fn test_real_webdav_stream_reads_and_mpv_playback() {
    if !is_service_available(18080) {
        eprintln!(
            "SKIPPING test_real_webdav_stream_reads_and_mpv_playback: WebDAV on 18080 unavailable"
        );
        return;
    }

    // 1. Validate raw HttpRangeStream chunk reads and seeks
    let auth = Some("Authorization: Basic bXV2cml4X2RhdjpEYXZQYXNzd29yZDEyMyE=".to_string());
    let mut stream = HttpRangeStream::new("127.0.0.1", 18080, "/webdav/sample.mp4", auth.clone())
        .expect("Connect HttpRangeStream");

    let total = stream.total_size().expect("Total size discovered");
    assert!(total > 50_000, "sample.mp4 size must match fixture");

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).expect("Read initial 1024 bytes");
    assert_eq!(n, 1024);
    assert_eq!(&buf[4..8], b"ftyp", "Must match MP4 ftyp header");

    // Seek forward
    let new_pos = stream.seek(10_000).expect("Seek to offset 10000");
    assert_eq!(new_pos, 10_000);
    let n2 = stream.read(&mut buf).expect("Read after seek");
    assert_eq!(n2, 1024);

    // Seek backward
    let back_pos = stream.seek(0).expect("Seek back to 0");
    assert_eq!(back_pos, 0);

    // 2. Wrap in ReadAheadStream and mount into libmpv via MpvStreamBridge
    let buffered_stream = ReadAheadStream::new(stream);
    let bridge = MpvStreamBridge::new();
    let mpv = create_test_mpv_handle();
    bridge
        .attach_to_handle(&mpv)
        .expect("Attach stream bridge to mpv");

    let stream_uri = bridge.register(buffered_stream);
    mpv.command(&["loadfile", &stream_uri])
        .expect("Load WebDAV stream into mpv");

    let loaded = wait_for_file_loaded(&mpv, 60);
    assert!(
        loaded,
        "libmpv must successfully demux and load live WebDAV stream"
    );

    let duration: Result<f64, _> = mpv.get_property("duration");
    if let Ok(dur) = duration {
        assert!(dur > 0.0, "Stream duration must be positive: {dur}");
    }

    // Seek within live remote stream via libmpv
    mpv.command(&["seek", "2.0", "absolute"])
        .expect("Seek within WebDAV stream");
    let _ = mpv.wait_event(0.3);

    // Clean teardown
    let _ = mpv.command(&["stop"]);
}

#[test]
fn test_real_samba_stream_reads_and_mpv_playback() {
    if !is_service_available(18445) {
        eprintln!(
            "SKIPPING test_real_samba_stream_reads_and_mpv_playback: Samba on 18445 unavailable"
        );
        return;
    }

    // 1. Open remote reader via pure-Rust smb2 using persistent worker thread
    let mut stream = SmbSeekableStream::connect(
        "127.0.0.1",
        18445,
        "media",
        "sample.mp4",
        Some("muvrix_smb".to_string()),
        Some("SmbPassword123!".to_string()),
        false,
    )
    .expect("Connect SMB seekable stream");

    let total = stream.total_size().expect("Samba file size");
    assert!(total > 50_000, "Samba sample.mp4 size must match fixture");

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).expect("Read 1024 bytes over SMB");
    assert_eq!(n, 1024);
    assert_eq!(&buf[4..8], b"ftyp", "Must match MP4 ftyp header");

    // Seek test over SMB
    let pos = stream.seek(5000).expect("Seek on SMB stream");
    assert_eq!(pos, 5000);
    let n = stream.read(&mut buf).expect("Read after seek on SMB");
    assert_eq!(n, 1024);

    // Seek back to start for playback
    stream.seek(0).expect("Seek back to 0");

    // 2. Mount into libmpv via ReadAheadStream and MpvStreamBridge
    let buffered_stream = ReadAheadStream::new(stream);
    let bridge = MpvStreamBridge::new();
    let mpv = create_test_mpv_handle();
    bridge
        .attach_to_handle(&mpv)
        .expect("Attach stream bridge to mpv");

    let stream_uri = bridge.register(buffered_stream);
    mpv.command(&["loadfile", &stream_uri])
        .expect("Load SMB stream into mpv");

    let loaded = wait_for_file_loaded(&mpv, 60);
    assert!(
        loaded,
        "libmpv must successfully demux and load live SMB2/3 stream"
    );

    let duration: Result<f64, _> = mpv.get_property("duration");
    if let Ok(dur) = duration {
        assert!(dur > 0.0, "Stream duration must be positive: {dur}");
    }

    mpv.command(&["seek", "1.5", "absolute"])
        .expect("Seek within SMB stream");
    let _ = mpv.wait_event(0.3);

    let _ = mpv.command(&["stop"]);
}

#[test]
fn test_real_sftp_stream_reads_and_mpv_playback() {
    if !is_service_available(18222) {
        eprintln!(
            "SKIPPING test_real_sftp_stream_reads_and_mpv_playback: OpenSSH on 18222 unavailable"
        );
        return;
    }

    let pinned_fingerprint = get_test_sftp_fingerprint();
    let vault = CredentialVault::new();
    vault.store("cred_sftp", "SftpPassword123!");

    let provider = SftpProvider::new(
        "127.0.0.1",
        18222,
        "/home/muvrix_sftp/media",
        "muvrix_sftp",
        Some("cred_sftp".to_string()),
        None,
        Some(pinned_fingerprint.to_string()),
        vault,
    );

    // 1. Open remote SFTP file with dedicated session
    let (sess, file) = provider
        .open_file("/home/muvrix_sftp/media/sample.mp4")
        .expect("Open SFTP sample.mp4 file");

    let mut stream = SftpSeekableStream::with_session(sess, file).expect("Init SftpSeekableStream");
    let total = stream.total_size().expect("SFTP file size");
    assert!(total > 50_000, "SFTP sample.mp4 size must match fixture");

    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).expect("Read 1024 bytes over SFTP");
    assert_eq!(n, 1024);
    assert_eq!(&buf[4..8], b"ftyp", "Must match MP4 ftyp header");

    // Seek test over SFTP
    let pos = stream.seek(8000).expect("Seek on SFTP stream");
    assert_eq!(pos, 8000);
    let n = stream.read(&mut buf).expect("Read after seek on SFTP");
    assert_eq!(n, 1024);

    stream.seek(0).expect("Seek back to 0");

    // 2. Mount into libmpv via ReadAheadStream and MpvStreamBridge
    let buffered_stream = ReadAheadStream::new(stream);
    let bridge = MpvStreamBridge::new();
    let mpv = create_test_mpv_handle();
    bridge
        .attach_to_handle(&mpv)
        .expect("Attach stream bridge to mpv");

    let stream_uri = bridge.register(buffered_stream);
    mpv.command(&["loadfile", &stream_uri])
        .expect("Load SFTP stream into mpv");

    let loaded = wait_for_file_loaded(&mpv, 60);
    assert!(
        loaded,
        "libmpv must successfully demux and load live SFTP stream"
    );

    let duration: Result<f64, _> = mpv.get_property("duration");
    if let Ok(dur) = duration {
        assert!(dur > 0.0, "Stream duration must be positive: {dur}");
    }

    mpv.command(&["seek", "2.0", "absolute"])
        .expect("Seek within SFTP stream");
    let _ = mpv.wait_event(0.3);

    let _ = mpv.command(&["stop"]);
}
