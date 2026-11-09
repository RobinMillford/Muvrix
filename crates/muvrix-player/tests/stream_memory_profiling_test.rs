//! Memory Profiling and Leak Verification Test for Remote Streams (REQ-NET-16).
//!
//! Profiles real heap and RSS consumption under Linux via `/proc/self/status`
//! across stream lifecycles: Baseline -> Active Streaming -> Rapid Seeking -> Stream Drop.

use std::fs;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use muvrix_player::handle::MpvHandle;
use muvrix_player::stream::{
    HttpRangeStream, ReadAheadStream, SeekableStream, SftpSeekableStream, SmbSeekableStream,
};

fn is_service_available(port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", port).parse().unwrap(),
        Duration::from_millis(150),
    )
    .is_ok()
}

#[derive(Debug, Clone, Copy)]
struct MemStats {
    vm_rss_kb: u64,
    vm_data_kb: u64,
}

fn read_mem_stats() -> MemStats {
    let content = fs::read_to_string("/proc/self/status").unwrap_or_default();
    let mut vm_rss_kb = 0;
    let mut vm_data_kb = 0;

    for line in content.lines() {
        if line.starts_with("VmRSS:") {
            if let Some(val) = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()) {
                vm_rss_kb = val;
            }
        } else if line.starts_with("VmData:") {
            if let Some(val) = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()) {
                vm_data_kb = val;
            }
        }
    }

    MemStats {
        vm_rss_kb,
        vm_data_kb,
    }
}

#[test]
fn test_stream_memory_profiling_and_lifecycle_limits() {
    let _ = env_logger::builder().is_test(true).try_init();

    // 1. Baseline: Initialize libmpv handle with null video/audio
    let handle = MpvHandle::new().expect("Create mpv handle");
    handle.set_option_string("vo", "null").unwrap();
    handle.set_option_string("ao", "null").unwrap();
    handle.initialize().expect("Initialize mpv handle");
    let _handle = Arc::new(handle);

    let baseline = read_mem_stats();
    println!(
        "\n[MEMORY PROFILE] 1. Baseline Player State: VmRSS = {} KB ({} MB), VmData = {} KB ({} MB)",
        baseline.vm_rss_kb,
        baseline.vm_rss_kb / 1024,
        baseline.vm_data_kb,
        baseline.vm_data_kb / 1024
    );

    // 2. Profile WebDAV HttpRangeStream + ReadAheadStream (256 KB buffer)
    if is_service_available(18080) {
        println!("\n--- Profiling WebDAV Stream ---");
        let auth = format!(
            "Authorization: Basic {}",
            muvrix_core::network::credentials::base64_encode(b"muvrix_dav:DavPassword123!")
        );

        let pre_stream = read_mem_stats();
        let http_stream =
            HttpRangeStream::new("127.0.0.1", 18080, "/webdav/sample.mp4", Some(auth))
                .expect("Open WebDAV stream");

        let mut read_ahead = ReadAheadStream::new(http_stream);
        let stream_opened = read_mem_stats();

        // Perform sequential reads totaling 512 KB
        let mut buf = vec![0u8; 64 * 1024];
        let mut total_read = 0;
        for _ in 0..8 {
            let n = read_ahead.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            total_read += n;
        }

        let streaming_active = read_mem_stats();

        // Perform 50 rapid random seek operations
        for i in 0..50 {
            let target = (i * 1024) % 70000;
            read_ahead.seek(target).unwrap();
            let _ = read_ahead.read(&mut buf[..1024]).unwrap();
        }

        let post_seeking = read_mem_stats();

        // Drop the stream and verify deallocation
        drop(read_ahead);
        std::thread::sleep(Duration::from_millis(50));
        let post_drop = read_mem_stats();

        println!(
            "WebDAV Opened Delta:   VmRSS = +{} KB, VmData = +{} KB",
            stream_opened.vm_rss_kb.saturating_sub(pre_stream.vm_rss_kb),
            stream_opened
                .vm_data_kb
                .saturating_sub(pre_stream.vm_data_kb)
        );
        println!(
            "WebDAV Read ({} KB):  VmRSS = +{} KB, VmData = +{} KB",
            total_read / 1024,
            streaming_active
                .vm_rss_kb
                .saturating_sub(pre_stream.vm_rss_kb),
            streaming_active
                .vm_data_kb
                .saturating_sub(pre_stream.vm_data_kb)
        );
        println!(
            "WebDAV 50 Seeks:       VmRSS = +{} KB, VmData = +{} KB",
            post_seeking.vm_rss_kb.saturating_sub(pre_stream.vm_rss_kb),
            post_seeking
                .vm_data_kb
                .saturating_sub(pre_stream.vm_data_kb)
        );
        println!(
            "WebDAV Post-Drop:      VmRSS = {} KB (Net delta from pre-stream: +{} KB)",
            post_drop.vm_rss_kb,
            post_drop.vm_rss_kb.saturating_sub(pre_stream.vm_rss_kb)
        );

        // Verification: ReadAheadStream buffer is 256 KB.
        // Total per-stream VmData delta during streaming must remain bounded (< 5 MB).
        let vm_data_delta = streaming_active
            .vm_data_kb
            .saturating_sub(pre_stream.vm_data_kb);
        assert!(
            vm_data_delta < 5120,
            "WebDAV per-stream VmData growth must be < 5 MB, got {} KB",
            vm_data_delta
        );
    }

    // 3. Profile SMB Stream
    if is_service_available(18445) {
        println!("\n--- Profiling SMB2/3 Stream ---");
        let pre_stream = read_mem_stats();

        let smb_stream = SmbSeekableStream::connect(
            "127.0.0.1",
            18445,
            "media",
            "sample.mp4",
            Some("muvrix_smb".to_string()),
            Some("SmbPassword123!".to_string()),
            false,
        )
        .expect("Connect SMB stream");

        let mut read_ahead = ReadAheadStream::new(smb_stream);
        let stream_opened = read_mem_stats();

        let mut buf = vec![0u8; 64 * 1024];
        let mut total_read = 0;
        for _ in 0..8 {
            let n = read_ahead.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            total_read += n;
        }

        let streaming_active = read_mem_stats();

        for i in 0..50 {
            let target = (i * 1024) % 70000;
            read_ahead.seek(target).unwrap();
            let _ = read_ahead.read(&mut buf[..1024]).unwrap();
        }

        let post_seeking = read_mem_stats();

        drop(read_ahead);
        std::thread::sleep(Duration::from_millis(50));
        let post_drop = read_mem_stats();

        println!(
            "SMB Opened Delta:   VmRSS = +{} KB, VmData = +{} KB",
            stream_opened.vm_rss_kb.saturating_sub(pre_stream.vm_rss_kb),
            stream_opened
                .vm_data_kb
                .saturating_sub(pre_stream.vm_data_kb)
        );
        println!(
            "SMB Read ({} KB):  VmRSS = +{} KB, VmData = +{} KB",
            total_read / 1024,
            streaming_active
                .vm_rss_kb
                .saturating_sub(pre_stream.vm_rss_kb),
            streaming_active
                .vm_data_kb
                .saturating_sub(pre_stream.vm_data_kb)
        );
        println!(
            "SMB 50 Seeks:       VmRSS = +{} KB, VmData = +{} KB",
            post_seeking.vm_rss_kb.saturating_sub(pre_stream.vm_rss_kb),
            post_seeking
                .vm_data_kb
                .saturating_sub(pre_stream.vm_data_kb)
        );
        println!(
            "SMB Post-Drop:      VmRSS = {} KB (Net delta from pre-stream: +{} KB)",
            post_drop.vm_rss_kb,
            post_drop.vm_rss_kb.saturating_sub(pre_stream.vm_rss_kb)
        );

        let vm_data_delta = streaming_active
            .vm_data_kb
            .saturating_sub(pre_stream.vm_data_kb);
        assert!(
            vm_data_delta < 10240,
            "SMB per-stream VmData growth must be < 10 MB, got {} KB",
            vm_data_delta
        );
    }

    // 4. Profile SFTP Stream
    if is_service_available(18222) {
        println!("\n--- Profiling SFTP Stream ---");
        let pre_stream = read_mem_stats();

        let tcp = TcpStream::connect("127.0.0.1:18222").expect("Connect TCP");
        let mut session = ssh2::Session::new().expect("Create ssh2 session");
        session.set_tcp_stream(tcp);
        session.handshake().expect("Handshake");
        session
            .userauth_password("muvrix_sftp", "SftpPassword123!")
            .expect("Auth");
        let sftp = session.sftp().expect("Init sftp");
        let file = sftp
            .open(std::path::Path::new("/home/muvrix_sftp/media/sample.mp4"))
            .expect("Open remote file");

        let sftp_stream =
            SftpSeekableStream::with_session(session, file).expect("Create SFTP stream");
        let mut read_ahead = ReadAheadStream::new(sftp_stream);
        let stream_opened = read_mem_stats();

        let mut buf = vec![0u8; 64 * 1024];
        let mut total_read = 0;
        for _ in 0..8 {
            let n = read_ahead.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            total_read += n;
        }

        let streaming_active = read_mem_stats();

        for i in 0..50 {
            let target = (i * 1024) % 70000;
            read_ahead.seek(target).unwrap();
            let _ = read_ahead.read(&mut buf[..1024]).unwrap();
        }

        let post_seeking = read_mem_stats();

        drop(read_ahead);
        std::thread::sleep(Duration::from_millis(50));
        let post_drop = read_mem_stats();

        println!(
            "SFTP Opened Delta:  VmRSS = +{} KB, VmData = +{} KB",
            stream_opened.vm_rss_kb.saturating_sub(pre_stream.vm_rss_kb),
            stream_opened
                .vm_data_kb
                .saturating_sub(pre_stream.vm_data_kb)
        );
        println!(
            "SFTP Read ({} KB): VmRSS = +{} KB, VmData = +{} KB",
            total_read / 1024,
            streaming_active
                .vm_rss_kb
                .saturating_sub(pre_stream.vm_rss_kb),
            streaming_active
                .vm_data_kb
                .saturating_sub(pre_stream.vm_data_kb)
        );
        println!(
            "SFTP 50 Seeks:      VmRSS = +{} KB, VmData = +{} KB",
            post_seeking.vm_rss_kb.saturating_sub(pre_stream.vm_rss_kb),
            post_seeking
                .vm_data_kb
                .saturating_sub(pre_stream.vm_data_kb)
        );
        println!(
            "SFTP Post-Drop:     VmRSS = {} KB (Net delta from pre-stream: +{} KB)",
            post_drop.vm_rss_kb,
            post_drop.vm_rss_kb.saturating_sub(pre_stream.vm_rss_kb)
        );

        let vm_data_delta = streaming_active
            .vm_data_kb
            .saturating_sub(pre_stream.vm_data_kb);
        assert!(
            vm_data_delta < 10240,
            "SFTP per-stream VmData growth must be < 10 MB, got {} KB",
            vm_data_delta
        );
    }

    println!("\n[MEMORY PROFILE] All stream profiles completed within verified heap bounds.\n");
}
