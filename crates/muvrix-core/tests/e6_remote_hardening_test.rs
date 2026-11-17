//! Phase E6 Remote Protocol Hardening, NFS & Media Server Sync Integration Test Suite.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use muvrix_core::network::{
    redact_credentials_from_url, ConnectionPool, CredentialVault, JellyfinProvider,
    NetworkProvider, NfsProvider, NfsUrl, NfsVersion, PlexProvider, PoolConfig,
};

#[test]
fn test_nfs_share_url_parsing_and_stream_construction() {
    // 1. Standard NFSv4 export URL
    let url_v4 = "nfs://192.168.1.50:2049/volume1/video/movies/avatar.mkv";
    let parsed_v4 = NfsUrl::parse(url_v4).expect("Parse NFSv4 URL");
    assert_eq!(parsed_v4.host, "192.168.1.50");
    assert_eq!(parsed_v4.port, 2049);
    assert_eq!(parsed_v4.export_path, "/volume1");
    assert_eq!(parsed_v4.file_path, "/video/movies/avatar.mkv");
    assert_eq!(
        parsed_v4.to_url(),
        "nfs://192.168.1.50/volume1/video/movies/avatar.mkv"
    );

    // 2. Default port resolution (omitted port => 2049)
    let url_default_port = "nfs://nas.home.local/export/media/music/track01.flac";
    let parsed_default = NfsUrl::parse(url_default_port).expect("Parse default port URL");
    assert_eq!(parsed_default.host, "nas.home.local");
    assert_eq!(parsed_default.port, 2049);
    assert_eq!(parsed_default.export_path, "/export");
    assert_eq!(parsed_default.file_path, "/media/music/track01.flac");

    // 3. IPv6 literal address support
    let url_ipv6 = "nfs://[2001:db8::1]:2049/srv/nfs4/stream.ts";
    let parsed_ipv6 = NfsUrl::parse(url_ipv6).expect("Parse IPv6 NFS URL");
    assert_eq!(parsed_ipv6.host, "2001:db8::1");
    assert_eq!(parsed_ipv6.port, 2049);
    assert_eq!(parsed_ipv6.export_path, "/srv");
    assert_eq!(parsed_ipv6.file_path, "/nfs4/stream.ts");

    // 4. Provider configuration validation
    let vault = CredentialVault::new();
    let provider = NfsProvider::new(
        "nas.home.local",
        2049,
        "/volume1/video",
        NfsVersion::V4,
        Some("media_user".to_string()),
        None,
        vault,
    );

    assert_eq!(provider.protocol(), "nfs");
    assert_eq!(provider.base_url(), "nfs://nas.home.local/volume1/video");
    assert!(provider.validate_config().is_ok());

    let resolved = provider
        .resolve_stream_url("/movies/avatar.mkv")
        .expect("Resolve stream URL");
    assert_eq!(
        resolved,
        "nfs://nas.home.local/volume1/video/movies/avatar.mkv"
    );
}

#[test]
fn test_jellyfin_catalog_json_deserialization() {
    let raw_json = r#"{
        "Items": [
            {
                "Name": "Sita Sings the Blues",
                "Id": "jf_item_101",
                "Type": "Movie",
                "RunTimeTicks": 49200000000,
                "Container": "mkv",
                "Size": 850400000,
                "ProductionYear": 2008,
                "MediaSources": [
                    {
                        "Id": "ms_sita_1",
                        "Path": "/media/movies/sita.mkv",
                        "Container": "mkv",
                        "Size": 850400000,
                        "Bitrate": 4500000
                    }
                ]
            },
            {
                "Name": "Open Source Documentaries",
                "Id": "jf_folder_202",
                "Type": "Folder",
                "RunTimeTicks": null,
                "Container": null,
                "Size": 0,
                "ProductionYear": null,
                "MediaSources": []
            }
        ],
        "TotalRecordCount": 2,
        "StartIndex": 0
    }"#;

    let entries =
        JellyfinProvider::parse_catalog_json(raw_json).expect("Parse Jellyfin catalog JSON");
    assert_eq!(entries.len(), 2);

    let movie = &entries[0];
    assert_eq!(movie.name, "Sita Sings the Blues.mkv");
    assert_eq!(movie.remote_path, "/items/jf_item_101");
    assert!(!movie.is_dir);
    assert_eq!(movie.size_bytes, 850400000);

    let folder = &entries[1];
    assert_eq!(folder.name, "Open Source Documentaries");
    assert_eq!(folder.remote_path, "/items/jf_folder_202");
    assert!(folder.is_dir);
    assert_eq!(folder.size_bytes, 0);
}

#[test]
fn test_plex_catalog_json_deserialization() {
    // 1. Sections catalog
    let sections_json = r#"{
        "MediaContainer": {
            "size": 2,
            "title1": "Plex Server",
            "Directory": [
                {
                    "key": "3",
                    "title": "4K Cinema",
                    "type": "movie"
                },
                {
                    "key": "7",
                    "title": "Documentaries",
                    "type": "show"
                }
            ]
        }
    }"#;

    let sections =
        PlexProvider::parse_sections_json(sections_json).expect("Parse Plex sections JSON");
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].name, "4K Cinema");
    assert_eq!(sections[0].remote_path, "/library/sections/3");
    assert!(sections[0].is_dir);

    // 2. Section items
    let library_json = r#"{
        "MediaContainer": {
            "size": 1,
            "Metadata": [
                {
                    "ratingKey": "888",
                    "key": "/library/metadata/888",
                    "title": "Cosmos Laundromat",
                    "type": "movie",
                    "duration": 725000,
                    "year": 2015,
                    "Media": [
                        {
                            "id": 901,
                            "duration": 725000,
                            "bitrate": 8000000,
                            "container": "mp4",
                            "Part": [
                                {
                                    "id": 1001,
                                    "key": "/library/parts/1001/file.mp4",
                                    "duration": 725000,
                                    "file": "/media/movies/cosmos.mp4",
                                    "size": 720000000,
                                    "container": "mp4"
                                }
                            ]
                        }
                    ]
                }
            ]
        }
    }"#;

    let items =
        PlexProvider::parse_library_items_json(library_json).expect("Parse Plex library JSON");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Cosmos Laundromat.mp4");
    assert_eq!(items[0].remote_path, "/library/parts/1001/file.mp4");
    assert!(!items[0].is_dir);
    assert_eq!(items[0].size_bytes, 720000000);
}

#[test]
fn test_connection_pool_keepalive_recycling() {
    let config = PoolConfig {
        max_idle_per_endpoint: 2,
        max_total_connections: 8,
        idle_timeout_secs: 1, // Rapid eviction for test
        keepalive_interval_secs: 1,
        wan_latency_threshold_ms: 200,
    };
    let pool = ConnectionPool::with_config(config);

    // Acquire and release two connections to "nfs.remote.nas:2049"
    let h1 = pool.acquire("nfs", "nfs.remote.nas", 2049);
    let h2 = pool.acquire("nfs", "nfs.remote.nas", 2049);

    pool.release(h1);
    pool.release(h2);

    let t1 = pool.telemetry();
    assert_eq!(t1.active_connections, 0);
    assert_eq!(t1.idle_connections, 2);
    assert_eq!(t1.socket_reuse_count, 0);

    // Re-acquire from pool: reuse count must increment
    let h3 = pool.acquire("nfs", "nfs.remote.nas", 2049);
    assert_eq!(h3.reuse_count, 1);
    pool.release(h3);

    let t2 = pool.telemetry();
    assert_eq!(t2.socket_reuse_count, 1);

    // Keepalive sweep before timeout
    let sweep = pool.perform_keepalive_sweep();
    assert!(sweep.active_retained >= 1);

    // Sleep past idle timeout and sweep again: verify connections are recycled
    thread::sleep(Duration::from_millis(1100));
    let sweep_after_timeout = pool.perform_keepalive_sweep();
    assert!(sweep_after_timeout.connections_recycled >= 1);

    let t3 = pool.telemetry();
    assert!(t3.recycled_count >= 1);
    assert_eq!(t3.idle_connections, 0);
}

#[test]
fn test_connection_pool_wan_high_latency_adaptation() {
    let pool = ConnectionPool::new();

    // Low latency LAN endpoint (12ms)
    pool.acquire("jellyfin", "lan.nas.home", 8096);
    pool.record_rtt("jellyfin", "lan.nas.home", 8096, Duration::from_millis(12));
    assert!(!pool.is_wan_connection("jellyfin", "lan.nas.home", 8096));

    // High latency WAN endpoint (250ms)
    pool.acquire("jellyfin", "wan.cloud.media", 8096);
    pool.record_rtt(
        "jellyfin",
        "wan.cloud.media",
        8096,
        Duration::from_millis(250),
    );
    assert!(pool.is_wan_connection("jellyfin", "wan.cloud.media", 8096));

    let telemetry = pool.telemetry();
    assert!(telemetry.average_rtt_ms > 0.0);
}

#[test]
fn test_url_credential_token_redaction() {
    // Redact password in user authority
    let basic_url = "nfs://admin:super_secret@192.168.1.1:2049/export";
    let redacted_basic = redact_credentials_from_url(basic_url);
    assert_eq!(redacted_basic, "nfs://[redacted]@192.168.1.1:2049/export");

    // Redact Jellyfin API token in query parameter
    let jf_url = "http://jellyfin.local:8096/Videos/123/stream?static=true&api_key=SECRET_TOKEN_XYZ&sub=none";
    let redacted_jf = redact_credentials_from_url(jf_url);
    assert!(!redacted_jf.contains("SECRET_TOKEN_XYZ"));
    assert!(redacted_jf.contains("api_key=[redacted]"));
    assert!(redacted_jf.contains("static=true"));

    // Redact Plex auth token in query parameter
    let plex_url =
        "http://plex.local:32400/library/parts/456/file.mkv?X-Plex-Token=PLEX_SUPER_KEY_999";
    let redacted_plex = redact_credentials_from_url(plex_url);
    assert!(!redacted_plex.contains("PLEX_SUPER_KEY_999"));
    assert!(redacted_plex.contains("X-Plex-Token=[redacted]"));
}

#[test]
fn test_mock_nfs_rpc_null_ping_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    let is_running = Arc::new(AtomicBool::new(true));
    let is_running_clone = is_running.clone();

    // Spawn ephemeral mock NFS server responding to ONC-RPC NULL
    let server_handle = thread::spawn(move || {
        listener.set_nonblocking(true).expect("set nonblocking");
        while is_running_clone.load(Ordering::SeqCst) {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 64];
                if let Ok(n) = stream.read(&mut buf) {
                    if n >= 28 {
                        let xid = &buf[4..8];
                        // Construct RPC NULL SUCCESS reply
                        let mut reply = Vec::new();
                        reply.extend_from_slice(&(0x8000_0018u32).to_be_bytes()); // 24-byte reply body
                        reply.extend_from_slice(xid); // Echo XID
                        reply.extend_from_slice(&1u32.to_be_bytes()); // MSG_REPLY = 1
                        reply.extend_from_slice(&0u32.to_be_bytes()); // MSG_ACCEPTED = 0
                        reply.extend_from_slice(&0u32.to_be_bytes()); // Verf flavor = 0
                        reply.extend_from_slice(&0u32.to_be_bytes()); // Verf length = 0
                        reply.extend_from_slice(&0u32.to_be_bytes()); // accept_stat SUCCESS = 0
                        let _ = stream.write_all(&reply);
                    }
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    });

    let vault = CredentialVault::new();
    let provider = NfsProvider::new(
        "127.0.0.1",
        port,
        "/volume1/test",
        NfsVersion::V4,
        None,
        None,
        vault,
    )
    .with_timeout(1000);

    let test_res = provider.test_connection();
    is_running.store(false, Ordering::SeqCst);
    let _ = server_handle.join();

    assert!(
        test_res.is_ok(),
        "NFS test connection must succeed against mock server"
    );
}

#[test]
fn test_mock_jellyfin_http_real_server_integration() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    let is_running = Arc::new(AtomicBool::new(true));
    let is_running_clone = is_running.clone();

    let server_handle = thread::spawn(move || {
        listener.set_nonblocking(true).expect("set nonblocking");
        while is_running_clone.load(Ordering::SeqCst) {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                if let Ok(n) = stream.read(&mut buf) {
                    let req_str = String::from_utf8_lossy(&buf[..n]);
                    let (status_line, body) = if req_str.contains("BAD_TOKEN") {
                        ("HTTP/1.1 401 Unauthorized", r#"{"error":"Unauthorized"}"#)
                    } else if req_str.contains("/System/Info/Public") {
                        (
                            "HTTP/1.1 200 OK",
                            r#"{"ServerName":"Mock Jellyfin","Version":"10.9.11","Id":"mock-jf-1","OperatingSystem":"Linux"}"#,
                        )
                    } else if req_str.contains("/Items") {
                        (
                            "HTTP/1.1 200 OK",
                            r#"{"Items":[{"Id":"jf_101","Name":"Sita Sings the Blues","Type":"Movie","Container":"mkv","Size":850400000,"MediaSources":[{"Id":"ms1","Container":"mkv","Size":850400000}]}]}"#,
                        )
                    } else {
                        ("HTTP/1.1 404 Not Found", r#"{"error":"Not Found"}"#)
                    };

                    let resp = format!(
                        "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
    });

    let vault = CredentialVault::new();
    let provider = JellyfinProvider::new(
        "127.0.0.1",
        port,
        false,
        Some("VALID_TOKEN_123".to_string()),
        None,
        None,
        vault.clone(),
    )
    .with_timeout(1000);

    // 1. Valid test_connection
    assert!(provider.test_connection().is_ok());

    // 2. Valid list_directory
    let entries = provider
        .list_directory("/items/jf_movies")
        .expect("list directory");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "Sita Sings the Blues.mkv");
    assert_eq!(entries[0].remote_path, "/items/jf_101");

    // 3. Unauthorized token failure
    let bad_provider = JellyfinProvider::new(
        "127.0.0.1",
        port,
        false,
        Some("BAD_TOKEN".to_string()),
        None,
        None,
        vault,
    )
    .with_timeout(1000);
    let auth_res = bad_provider.test_connection();
    assert!(
        auth_res.is_err(),
        "Bad token must return authentication error"
    );

    is_running.store(false, Ordering::SeqCst);
    let _ = server_handle.join();
}

#[test]
fn test_mock_plex_http_real_server_integration() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    let is_running = Arc::new(AtomicBool::new(true));
    let is_running_clone = is_running.clone();

    let server_handle = thread::spawn(move || {
        listener.set_nonblocking(true).expect("set nonblocking");
        while is_running_clone.load(Ordering::SeqCst) {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                if let Ok(n) = stream.read(&mut buf) {
                    let req_str = String::from_utf8_lossy(&buf[..n]);
                    let (status_line, body) = if req_str.contains("INVALID_PLEX_TOKEN") {
                        ("HTTP/1.1 401 Unauthorized", r#"{"error":"Unauthorized"}"#)
                    } else if req_str.contains("/identity") {
                        (
                            "HTTP/1.1 200 OK",
                            r#"{"MediaContainer":{"size":0,"claimed":true,"machineIdentifier":"mock-plex-1","version":"1.40.0"}}"#,
                        )
                    } else if req_str.contains("/library/sections/1") {
                        (
                            "HTTP/1.1 200 OK",
                            r#"{"MediaContainer":{"size":1,"Metadata":[{"ratingKey":"1","key":"/library/metadata/1","title":"Elephants Dream","type":"movie","Media":[{"id":10,"container":"mkv","Part":[{"id":20,"key":"/library/parts/20/file.mkv","size":367001600,"container":"mkv"}]}]}]}}"#,
                        )
                    } else if req_str.contains("/library/sections") {
                        (
                            "HTTP/1.1 200 OK",
                            r#"{"MediaContainer":{"size":1,"Directory":[{"key":"1","title":"Movies","type":"movie"}]}}"#,
                        )
                    } else {
                        ("HTTP/1.1 404 Not Found", r#"{"error":"Not Found"}"#)
                    };

                    let resp = format!(
                        "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
    });

    let vault = CredentialVault::new();
    let provider = PlexProvider::new(
        "127.0.0.1",
        port,
        false,
        Some("VALID_PLEX_TOKEN".to_string()),
        None,
        vault.clone(),
    )
    .with_timeout(1000);

    // 1. Valid test_connection
    assert!(provider.test_connection().is_ok());

    // 2. Valid sections listing
    let sections = provider.list_directory("/").expect("list sections");
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].name, "Movies");

    // 3. Valid items listing
    let items = provider
        .list_directory("/library/sections/1")
        .expect("list items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name, "Elephants Dream.mkv");
    assert_eq!(items[0].remote_path, "/library/parts/20/file.mkv");

    // 4. Invalid token rejection
    let bad_provider = PlexProvider::new(
        "127.0.0.1",
        port,
        false,
        Some("INVALID_PLEX_TOKEN".to_string()),
        None,
        vault,
    )
    .with_timeout(1000);
    assert!(bad_provider.test_connection().is_err());

    is_running.store(false, Ordering::SeqCst);
    let _ = server_handle.join();
}

#[test]
fn test_duplicate_connection_prevention_in_database() {
    let db = muvrix_core::database::DatabaseManager::open_in_memory().expect("in-memory db");
    let conn1 = muvrix_core::database::NetworkConnection {
        id: 0,
        name: "Test NFS Primary".to_string(),
        protocol: "nfs".to_string(),
        host: "192.168.1.100".to_string(),
        port: 2049,
        remote_path: "/export/media".to_string(),
        auth_type: "anonymous".to_string(),
        credential_ref: None,
        is_anonymous: true,
        auto_connect: false,
        last_connected_at: 0,
    };

    let id1 = db.insert_connection(&conn1).expect("insert connection");
    assert!(id1 > 0);

    // Check if duplicate endpoint exists
    let existing = db
        .find_connection_by_endpoint("nfs", "192.168.1.100", 2049, "/export/media")
        .expect("query endpoint");
    assert!(existing.is_some());
    assert_eq!(existing.unwrap().id, id1);

    // Endpoint with different port or path does not match
    let non_existing = db
        .find_connection_by_endpoint("nfs", "192.168.1.100", 2050, "/export/media")
        .expect("query endpoint");
    assert!(non_existing.is_none());
}

#[test]
fn test_connection_pool_concurrent_access() {
    let pool = Arc::new(ConnectionPool::new());
    let mut handles = Vec::new();

    for i in 0..8 {
        let pool_clone = pool.clone();
        handles.push(thread::spawn(move || {
            for j in 0..50 {
                let host = format!("server{}.internal", (i + j) % 3);
                let handle = pool_clone.acquire("jellyfin", &host, 8096);
                thread::sleep(Duration::from_micros(100));
                pool_clone.release(handle);
            }
        }));
    }

    for h in handles {
        h.join().expect("Thread joined cleanly");
    }

    let telemetry = pool.telemetry();
    assert_eq!(telemetry.active_connections, 0);
    assert!(telemetry.idle_connections > 0);
}

#[test]
fn test_emby_server_compatibility() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock Emby server");
    let port = listener.local_addr().expect("port").port();
    let is_running = Arc::new(AtomicBool::new(true));
    let server_running = is_running.clone();

    let server_handle = thread::spawn(move || {
        listener.set_nonblocking(true).expect("set nonblocking");
        while server_running.load(Ordering::SeqCst) {
            if let Ok((mut stream, _)) = listener.accept() {
                stream
                    .set_read_timeout(Some(Duration::from_millis(500)))
                    .unwrap();
                let mut buf = [0u8; 4096];
                if let Ok(n) = stream.read(&mut buf) {
                    let req_str = String::from_utf8_lossy(&buf[..n]);

                    // Verify Emby header presence
                    let has_valid_token = req_str.contains("X-Emby-Token: VALID_EMBY_TOKEN");
                    let has_emby_auth = req_str.contains("X-Emby-Authorization: MediaBrowser");

                    let (status_line, body) = if !has_valid_token {
                        ("HTTP/1.1 401 Unauthorized", r#"{"error":"Unauthorized"}"#)
                    } else if req_str.contains("/System/Info/Public") {
                        assert!(has_emby_auth || has_valid_token);
                        (
                            "HTTP/1.1 200 OK",
                            r#"{"ServerName":"Emby Home Server","Version":"4.8.8.0","OperatingSystem":"Linux","Id":"emby-uuid-99"}"#,
                        )
                    } else if req_str.contains("/Items") {
                        (
                            "HTTP/1.1 200 OK",
                            r#"{"Items":[{"Id":"emby_mov_1","Name":"Cosmos Laundromat","Type":"Movie","Size":250000000,"Container":"mkv","MediaSources":[{"Id":"src1","Size":250000000,"Container":"mkv"}]}],"TotalRecordCount":1}"#,
                        )
                    } else {
                        ("HTTP/1.1 404 Not Found", r#"{"error":"Not Found"}"#)
                    };

                    let resp = format!(
                        "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
    });

    let vault = CredentialVault::new();
    let provider = JellyfinProvider::new(
        "127.0.0.1",
        port,
        false,
        Some("VALID_EMBY_TOKEN".to_string()),
        None,
        None,
        vault.clone(),
    )
    .with_timeout(1000);

    // 1. Emby connection test with X-Emby-Token
    assert!(provider.test_connection().is_ok());

    // 2. Emby items listing with X-Emby-Authorization
    let entries = provider
        .list_directory("/items/root")
        .expect("list Emby items");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "Cosmos Laundromat.mkv");
    assert_eq!(entries[0].size_bytes, 250000000);

    // 3. Direct play stream URL resolution
    let stream_url = provider
        .resolve_stream_url("/items/emby_mov_1")
        .expect("resolve stream URL");
    assert_eq!(
        stream_url,
        format!(
            "http://127.0.0.1:{port}/Videos/emby_mov_1/stream?static=true&api_key=VALID_EMBY_TOKEN"
        )
    );

    // 4. Invalid token verification
    let bad_provider = JellyfinProvider::new(
        "127.0.0.1",
        port,
        false,
        Some("BAD_EMBY_TOKEN".to_string()),
        None,
        None,
        vault,
    )
    .with_timeout(1000);
    assert!(bad_provider.test_connection().is_err());

    is_running.store(false, Ordering::SeqCst);
    let _ = server_handle.join();
}

#[test]
fn test_connection_pool_deterministic_heartbeat_probes() {
    use std::net::TcpStream;

    // 1. Create a real TCP listener to act as healthy endpoint
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind live probe server");
    let active_port = listener.local_addr().expect("port").port();

    // 2. Find a dead port that is not listening
    let dead_listener = TcpListener::bind("127.0.0.1:0").expect("bind temporary dead probe server");
    let dead_port = dead_listener.local_addr().expect("port").port();
    drop(dead_listener); // Close immediately so attempts fail

    let config = PoolConfig {
        max_idle_per_endpoint: 4,
        max_total_connections: 16,
        idle_timeout_secs: 2,       // 2s idle timeout
        keepalive_interval_secs: 1, // 1s keepalive heartbeat interval
        wan_latency_threshold_ms: 200,
    };
    let pool = ConnectionPool::with_config(config);

    // Acquire and release healthy connection -> enters Idle
    let h_active = pool.acquire("tcp", "127.0.0.1", active_port);
    pool.release(h_active);

    // Acquire and release dead connection -> enters Idle
    let h_dead = pool.acquire("tcp", "127.0.0.1", dead_port);
    pool.release(h_dead);

    let t0 = pool.telemetry();
    assert_eq!(t0.idle_connections, 2);
    assert_eq!(t0.probes_sent, 0);

    // Sleep past keepalive interval (1s)
    thread::sleep(Duration::from_millis(1100));

    // Dispatch deterministic heartbeat sweep with actual TCP connection probe
    let sweep = pool.perform_keepalive_sweep_with_probe(|_proto, host, port| {
        TcpStream::connect_timeout(
            &format!("{}:{}", host, port).parse().unwrap(),
            Duration::from_millis(200),
        )
        .is_ok()
    });

    assert_eq!(sweep.probes_dispatched, 2);
    assert_eq!(sweep.probes_successful, 1);
    assert_eq!(sweep.probes_failed, 1);
    assert_eq!(sweep.connections_recycled, 1); // Dead endpoint recycled

    let t1 = pool.telemetry();
    assert_eq!(t1.probes_sent, 2);
    assert_eq!(t1.probes_successful, 1);
    assert_eq!(t1.probes_failed, 1);
    assert_eq!(t1.recycled_count, 1);
    assert_eq!(t1.idle_connections, 1); // Healthy connection retained

    // Test WAN classification
    pool.record_rtt("tcp", "127.0.0.1", active_port, Duration::from_millis(220));
    assert!(pool.is_wan_connection("tcp", "127.0.0.1", active_port));

    pool.record_rtt("tcp", "127.0.0.1", active_port, Duration::from_millis(50));
    assert!(!pool.is_wan_connection("tcp", "127.0.0.1", active_port));

    // Sleep past idle timeout (2s) and sweep: healthy idle connection is recycled
    thread::sleep(Duration::from_millis(2100));
    let sweep2 = pool.perform_keepalive_sweep();
    assert_eq!(sweep2.connections_recycled, 1);

    let t2 = pool.telemetry();
    assert_eq!(t2.idle_connections, 0);
    assert_eq!(t2.recycled_count, 2);
}
