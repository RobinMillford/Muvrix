use muvrix_core::playlist::{export_m3u, generate_m3u, load_m3u, M3uEntry};
use std::fs;

#[test]
fn test_m3u_and_m3u8_full_roundtrip_with_unicode_and_spaces() {
    let temp_dir = std::env::temp_dir().join(format!("muvrix_test_{}", std::process::id()));
    fs::create_dir_all(&temp_dir).expect("create temp dir");

    let entries = vec![
        M3uEntry {
            path: "/media/movies/Cosmos Laundromat (2015) [4K UHD].mkv".to_string(),
            title: Some("Cosmos Laundromat (2015)".to_string()),
            duration_ms: 720000,
            logo: None,
            group: Some("Short Films".to_string()),
        },
        M3uEntry {
            path: "/media/music/日本語の曲 - 桜の花びら (Sakura).flac".to_string(),
            title: Some("桜の花びら (Sakura Petals)".to_string()),
            duration_ms: 245000,
            logo: None,
            group: Some("J-Pop".to_string()),
        },
        M3uEntry {
            path: "/media/audio/Песня_О_Друге_Владимир_Высоцкий.mp3".to_string(),
            title: Some("Владимир Высоцкий - Песня о друге".to_string()),
            duration_ms: 185000,
            logo: None,
            group: Some("Russian Classics".to_string()),
        },
        M3uEntry {
            path: "/nonexistent/path/missing_media_file.mp4".to_string(),
            title: Some("Inaccessible File Entry".to_string()),
            duration_ms: 60000,
            logo: None,
            group: None,
        },
    ];

    // 1. Test .m3u export
    let m3u_path = temp_dir.join("test_playlist.m3u");
    let m3u_path_str = m3u_path.to_str().expect("valid path");
    export_m3u(&entries, m3u_path_str).expect("export to .m3u must succeed");
    assert!(m3u_path.exists(), "Exported .m3u file must exist on disk");

    let m3u_content = fs::read_to_string(&m3u_path).expect("read m3u");
    assert!(
        m3u_content.starts_with("#EXTM3U"),
        "Must start with #EXTM3U"
    );
    assert!(m3u_content.contains("#EXTINF:720,Cosmos Laundromat (2015)"));
    assert!(m3u_content.contains("/media/movies/Cosmos Laundromat (2015) [4K UHD].mkv"));
    assert!(m3u_content.contains("#EXTINF:245,桜の花びら (Sakura Petals)"));
    assert!(m3u_content.contains("/media/music/日本語の曲 - 桜の花びら (Sakura).flac"));
    assert!(m3u_content.contains("Владимир Высоцкий - Песня о друге"));
    assert!(m3u_content.contains("/nonexistent/path/missing_media_file.mp4"));

    // 2. Test .m3u8 export (UTF-8 standard)
    let m3u8_path = temp_dir.join("test_playlist.m3u8");
    let m3u8_path_str = m3u8_path.to_str().expect("valid path");
    export_m3u(&entries, m3u8_path_str).expect("export to .m3u8 must succeed");
    assert!(m3u8_path.exists(), "Exported .m3u8 file must exist on disk");

    let m3u8_content = fs::read_to_string(&m3u8_path).expect("read m3u8");
    assert!(
        m3u8_content.starts_with("#EXTM3U"),
        "Must start with #EXTM3U"
    );
    assert!(m3u8_content.contains("桜の花びら"));

    // 3. Test roundtrip re-import of the generated .m3u8
    let imported_entries = load_m3u(m3u8_path_str).expect("import exported m3u8");
    assert_eq!(imported_entries.len(), 4, "Must reconstruct all 4 entries");

    assert_eq!(imported_entries[0].path, entries[0].path);
    assert_eq!(imported_entries[0].title, entries[0].title);
    assert_eq!(imported_entries[0].duration_ms, entries[0].duration_ms);

    assert_eq!(imported_entries[1].path, entries[1].path);
    assert_eq!(imported_entries[1].title, entries[1].title);
    assert_eq!(imported_entries[1].duration_ms, entries[1].duration_ms);

    assert_eq!(imported_entries[2].path, entries[2].path);
    assert_eq!(imported_entries[2].title, entries[2].title);
    assert_eq!(imported_entries[2].duration_ms, entries[2].duration_ms);

    assert_eq!(imported_entries[3].path, entries[3].path);
    assert_eq!(imported_entries[3].title, entries[3].title);
    assert_eq!(imported_entries[3].duration_ms, entries[3].duration_ms);

    // 4. Test generate_m3u direct generation
    let direct_gen = generate_m3u(&entries);
    assert_eq!(direct_gen, m3u8_content);

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
}
