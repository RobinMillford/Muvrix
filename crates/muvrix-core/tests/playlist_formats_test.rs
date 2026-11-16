//! Multi-format playlist tests: M3U, PLS, XSPF, and auto-detection.

use muvrix_core::playlist::*;

#[test]
fn test_pls_parsing_and_generation_roundtrip() {
    let pls_data = r#"[playlist]
NumberOfEntries=3
File1=/media/music/queen_bohemian.flac
Title1=Queen - Bohemian Rhapsody
Length1=354
File2=https://stream.example.com/live.aac
Title2=Live Radio Stream
Length2=-1
File3=relative/track3.mp3
Title3=Acoustic Track
Length3=180
Version=2
"#;

    let entries = parse_pls(pls_data, "/media/music/playlist.pls").expect("parse PLS");
    assert_eq!(entries.len(), 3);

    // Track 1
    assert_eq!(entries[0].path, "/media/music/queen_bohemian.flac");
    assert_eq!(
        entries[0].title.as_deref(),
        Some("Queen - Bohemian Rhapsody")
    );
    assert_eq!(entries[0].duration_ms, 354_000);

    // Track 2 (stream with Length=-1)
    assert_eq!(entries[1].path, "https://stream.example.com/live.aac");
    assert_eq!(entries[1].title.as_deref(), Some("Live Radio Stream"));
    assert_eq!(entries[1].duration_ms, 0);

    // Track 3 (relative path resolved)
    assert_eq!(entries[2].path, "/media/music/relative/track3.mp3");
    assert_eq!(entries[2].title.as_deref(), Some("Acoustic Track"));
    assert_eq!(entries[2].duration_ms, 180_000);

    // Generate PLS from parsed entries
    let generated = generate_pls(&entries);
    assert!(generated.starts_with("[playlist]\n"));
    assert!(generated.contains("NumberOfEntries=3\n"));
    assert!(generated.contains("File1=/media/music/queen_bohemian.flac\n"));
    assert!(generated.contains("Title1=Queen - Bohemian Rhapsody\n"));
    assert!(generated.contains("Length1=354\n"));
    assert!(generated.contains("Version=2\n"));

    // Roundtrip export and re-read
    let temp_dest = std::env::temp_dir().join("test_roundtrip.pls");
    export_pls(&entries, temp_dest.to_str().unwrap()).expect("export PLS");
    let reread = std::fs::read_to_string(&temp_dest).expect("read temp PLS");
    let reparsed = parse_pls(&reread, temp_dest.to_str().unwrap()).expect("reparse PLS");
    assert_eq!(reparsed.len(), 3);
    assert_eq!(reparsed[0].title, entries[0].title);
    assert_eq!(reparsed[0].duration_ms, entries[0].duration_ms);
    let _ = std::fs::remove_file(&temp_dest);
}

#[test]
fn test_xspf_parsing_and_generation_roundtrip() {
    let xspf_data = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <title>Muvrix Audio Showcase</title>
  <trackList>
    <track>
      <location>file:///media/lossless/track01.flac</location>
      <title>Hotel California</title>
      <duration>391000</duration>
      <image>http://art.example.com/album.jpg</image>
    </track>
    <track>
      <location>https://cdn.example.com/audio/podcast.mp3</location>
      <title>Engineering Podcast</title>
      <duration>1800000</duration>
    </track>
    <track>
      <location>relative/clip.ogg</location>
      <title>Short Jingle</title>
      <duration>15000</duration>
    </track>
  </trackList>
</playlist>
"#;

    let entries = parse_xspf(xspf_data, "/media/lossless/catalog.xspf").expect("parse XSPF");
    assert_eq!(entries.len(), 3);

    // Track 1: file:// prefix stripped and path preserved
    assert_eq!(entries[0].path, "/media/lossless/track01.flac");
    assert_eq!(entries[0].title.as_deref(), Some("Hotel California"));
    assert_eq!(entries[0].duration_ms, 391_000);
    assert_eq!(
        entries[0].logo.as_deref(),
        Some("http://art.example.com/album.jpg")
    );

    // Track 2: HTTP URL preserved
    assert_eq!(entries[1].path, "https://cdn.example.com/audio/podcast.mp3");
    assert_eq!(entries[1].duration_ms, 1_800_000);

    // Track 3: relative path resolved against catalog.xspf
    assert_eq!(entries[2].path, "/media/lossless/relative/clip.ogg");
    assert_eq!(entries[2].title.as_deref(), Some("Short Jingle"));
    assert_eq!(entries[2].duration_ms, 15_000);

    // Generate XSPF and verify XML tags
    let generated = generate_xspf("Generated Showcase", &entries);
    assert!(generated.contains("<playlist version=\"1\""));
    assert!(generated.contains("<title>Generated Showcase</title>"));
    assert!(generated.contains("<location>file:///media/lossless/track01.flac</location>"));
    assert!(generated.contains("<title>Hotel California</title>"));
    assert!(generated.contains("<duration>391000</duration>"));
    assert!(generated.contains("<image>http://art.example.com/album.jpg</image>"));

    // Roundtrip export and re-read
    let temp_dest = std::env::temp_dir().join("test_roundtrip.xspf");
    export_xspf("Export Test", &entries, temp_dest.to_str().unwrap()).expect("export XSPF");
    let reread = std::fs::read_to_string(&temp_dest).expect("read temp XSPF");
    let reparsed = parse_xspf(&reread, temp_dest.to_str().unwrap()).expect("reparse XSPF");
    assert_eq!(reparsed.len(), 3);
    assert_eq!(reparsed[0].title, entries[0].title);
    assert_eq!(reparsed[0].duration_ms, entries[0].duration_ms);
    let _ = std::fs::remove_file(&temp_dest);
}

#[test]
fn test_parse_playlist_auto_detection() {
    // 1. Detects M3U
    let m3u = "#EXTM3U\n#EXTINF:60,M3U Track\n/path/to/m3u.mp3\n";
    let entries_m3u = parse_playlist_auto(m3u, "test.m3u").expect("auto m3u");
    assert_eq!(entries_m3u.len(), 1);
    assert_eq!(entries_m3u[0].title.as_deref(), Some("M3U Track"));

    // 2. Detects PLS
    let pls =
        "[playlist]\nNumberOfEntries=1\nFile1=/path/to/pls.mp3\nTitle1=PLS Track\nVersion=2\n";
    let entries_pls = parse_playlist_auto(pls, "test.pls").expect("auto pls");
    assert_eq!(entries_pls.len(), 1);
    assert_eq!(entries_pls[0].title.as_deref(), Some("PLS Track"));

    // 3. Detects XSPF
    let xspf = "<?xml version=\"1.0\"?><playlist><trackList><track><location>/path/to/xspf.mp3</location><title>XSPF Track</title></track></trackList></playlist>";
    let entries_xspf = parse_playlist_auto(xspf, "test.xspf").expect("auto xspf");
    assert_eq!(entries_xspf.len(), 1);
    assert_eq!(entries_xspf[0].title.as_deref(), Some("XSPF Track"));
}
