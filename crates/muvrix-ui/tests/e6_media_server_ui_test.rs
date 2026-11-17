//! Phase E6 Media Server & NFS UI Integration Test Suite.

use std::time::Duration;

use muvrix_core::network::{
    ConnectionPool, CredentialVault, JellyfinProvider, NetworkProvider, NfsProvider, NfsVersion,
    PlexProvider,
};
use muvrix_player::{MemoryStream, MpvStreamBridge, NfsSeekableStream, SeekableStream};

#[test]
fn test_e6_network_connection_protocol_defaults() {
    let vault = CredentialVault::new();

    // NFS Provider
    let nfs = NfsProvider::new(
        "nas.local",
        0, // default
        "/export/media",
        NfsVersion::V4,
        None,
        None,
        vault.clone(),
    );
    assert_eq!(nfs.protocol(), "nfs");
    assert_eq!(nfs.port, 2049);
    assert_eq!(nfs.base_url(), "nfs://nas.local/export/media");

    // Jellyfin Provider
    let jf_http = JellyfinProvider::new(
        "jf.local",
        0, // default
        false,
        Some("tok123".to_string()),
        None,
        None,
        vault.clone(),
    );
    assert_eq!(jf_http.protocol(), "jellyfin");
    assert_eq!(jf_http.port, 8096);
    assert_eq!(jf_http.base_url(), "http://jf.local:8096");

    let jf_https = JellyfinProvider::new(
        "jf.local",
        0, // default
        true,
        Some("tok123".to_string()),
        None,
        None,
        vault.clone(),
    );
    assert_eq!(jf_https.port, 8920);
    assert_eq!(jf_https.base_url(), "https://jf.local:8920");

    // Plex Provider
    let plex = PlexProvider::new(
        "plex.local",
        0, // default
        false,
        Some("plex_token".to_string()),
        None,
        vault,
    );
    assert_eq!(plex.protocol(), "plex");
    assert_eq!(plex.port, 32400);
    assert_eq!(plex.base_url(), "http://plex.local:32400");
}

#[test]
fn test_e6_media_server_catalog_browsing_simulation() {
    let sample_jellyfin = r#"{
        "Items": [
            {
                "Name": "Tears of Steel",
                "Id": "jf_tos_99",
                "Type": "Movie",
                "RunTimeTicks": 7340000000,
                "Container": "mkv",
                "Size": 524288000,
                "MediaSources": []
            }
        ],
        "TotalRecordCount": 1
    }"#;

    let entries = JellyfinProvider::parse_catalog_json(sample_jellyfin)
        .expect("Parse Jellyfin simulated catalog");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "Tears of Steel.mkv");
    assert_eq!(entries[0].remote_path, "/items/jf_tos_99");
    assert_eq!(entries[0].size_bytes, 524288000);
    assert!(!entries[0].is_dir);

    let sample_plex = r#"{
        "MediaContainer": {
            "size": 1,
            "Metadata": [
                {
                    "ratingKey": "777",
                    "key": "/library/metadata/777",
                    "title": "Spring",
                    "type": "movie",
                    "duration": 480000,
                    "Media": [
                        {
                            "id": 888,
                            "container": "mp4",
                            "Part": [
                                {
                                    "id": 999,
                                    "key": "/library/parts/999/spring.mp4",
                                    "duration": 480000,
                                    "size": 314572800
                                }
                            ]
                        }
                    ]
                }
            ]
        }
    }"#;

    let plex_items =
        PlexProvider::parse_library_items_json(sample_plex).expect("Parse Plex simulated items");
    assert_eq!(plex_items.len(), 1);
    assert_eq!(plex_items[0].name, "Spring.mp4");
    assert_eq!(plex_items[0].remote_path, "/library/parts/999/spring.mp4");
    assert_eq!(plex_items[0].size_bytes, 314572800);
}

#[test]
fn test_e6_connection_pool_telemetry_ui_integration() {
    let pool = ConnectionPool::new();

    // Initially zero
    let t0 = pool.telemetry();
    assert_eq!(t0.active_connections, 0);
    assert_eq!(t0.idle_connections, 0);
    assert_eq!(t0.socket_reuse_count, 0);

    // Acquire, record RTT, release
    let h1 = pool.acquire("jellyfin", "nas.home", 8096);
    pool.record_rtt("jellyfin", "nas.home", 8096, Duration::from_millis(45));
    let t1 = pool.telemetry();
    assert_eq!(t1.active_connections, 1);
    assert_eq!(t1.idle_connections, 0);

    pool.release(h1);
    let t2 = pool.telemetry();
    assert_eq!(t2.active_connections, 0);
    assert_eq!(t2.idle_connections, 1);
    assert_eq!(t2.average_rtt_ms, 45.0);

    // Re-acquire reuses pooled connection
    let h2 = pool.acquire("jellyfin", "nas.home", 8096);
    assert_eq!(h2.reuse_count, 1);
    pool.release(h2);

    let t3 = pool.telemetry();
    assert_eq!(t3.socket_reuse_count, 1);
}

#[test]
fn test_e6_nfs_seekable_stream_bridge_registration() {
    let bridge = MpvStreamBridge::default();

    // Construct seekable NFS stream from memory buffer
    let data = vec![0xCA, 0xFE, 0xBA, 0xBE, 0x01, 0x02, 0x03, 0x04];
    let mem_stream = MemoryStream::new(data);
    let mut nfs_stream = NfsSeekableStream::new(Box::new(mem_stream), Some(8));

    assert_eq!(nfs_stream.total_size(), Some(8));

    let mut buf = [0u8; 4];
    let n = nfs_stream.read(&mut buf).expect("Read from NFS stream");
    assert_eq!(n, 4);
    assert_eq!(&buf, &[0xCA, 0xFE, 0xBA, 0xBE]);

    let pos = nfs_stream.seek(2).expect("Seek in NFS stream");
    assert_eq!(pos, 2);

    let uri = bridge.register(Box::new(nfs_stream));
    assert!(uri.starts_with("muvrix-stream://"));
}

#[test]
fn test_e6_duplicate_connection_prevention_ui_flow() {
    let db = muvrix_core::database::DatabaseManager::open_in_memory().expect("in-memory db");
    let c1 = muvrix_core::database::NetworkConnection {
        id: 0,
        name: "TrueNAS Primary".to_string(),
        protocol: "nfs".to_string(),
        host: "192.168.1.50".to_string(),
        port: 2049,
        remote_path: "/volume1/video".to_string(),
        auth_type: "anonymous".to_string(),
        credential_ref: None,
        is_anonymous: true,
        auto_connect: false,
        last_connected_at: 0,
    };

    let id1 = db.insert_connection(&c1).expect("insert connection");
    assert!(id1 > 0);

    // Verify duplicate detection by endpoint
    let found = db
        .find_connection_by_endpoint("nfs", "192.168.1.50", 2049, "/volume1/video")
        .expect("find connection");
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "TrueNAS Primary");

    // Case-insensitivity check (e.g. NFS vs nfs)
    let found_ci = db
        .find_connection_by_endpoint("NFS", "192.168.1.50", 2049, "/volume1/video")
        .expect("find connection case insensitive");
    assert!(found_ci.is_some());
}

#[test]
fn test_e6_nfs_version_selector_and_display() {
    assert_eq!(format!("{}", NfsVersion::V4), "NFSv4");
    assert_eq!(format!("{}", NfsVersion::V3), "NFSv3");
    assert_eq!(NfsVersion::default(), NfsVersion::V4);
}
