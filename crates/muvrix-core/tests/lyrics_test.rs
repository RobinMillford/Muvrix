//! Integration tests for synchronized LRC lyrics parser and sidecar detector.

use muvrix_core::lyrics::{find_sidecar_lrc, LrcFile, LrcParseError};
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

#[test]
fn test_lrc_parser_unicode_and_offsets() {
    let content = r#"
[ti:春の海 - Haru no Umi]
[ar:宮城道雄]
[al:Traditional Japanese Koto]
[offset:-500]
[00:01.00]春の光が海に満ちる
[00:06.50]波の音が静かに響く
[00:12.80]琴の調べが風に乗る
"#;

    let lrc = LrcFile::parse(content).expect("Parse Unicode LRC");
    assert_eq!(lrc.title.as_deref(), Some("春の海 - Haru no Umi"));
    assert_eq!(lrc.artist.as_deref(), Some("宮城道雄"));
    assert_eq!(lrc.offset_ms, -500);

    // Negative offset -500ms shifts timestamps earlier (minimum clamped to 0)
    // 00:01.00 (1000ms) - 500ms = 500ms
    // 00:06.50 (6500ms) - 500ms = 6000ms
    // 00:12.80 (12800ms) - 500ms = 12300ms
    assert_eq!(lrc.lines.len(), 3);
    assert_eq!(lrc.lines[0].timestamp_ms, 500);
    assert_eq!(lrc.lines[0].text, "春の光が海に満ちる");

    assert_eq!(lrc.lines[1].timestamp_ms, 6000);
    assert_eq!(lrc.lines[1].text, "波の音が静かに響く");

    assert_eq!(lrc.lines[2].timestamp_ms, 12300);
    assert_eq!(lrc.lines[2].text, "琴の調べが風に乗る");
}

#[test]
fn test_lrc_malformed_and_empty_handling() {
    // Empty
    assert_eq!(LrcFile::parse("").unwrap_err(), LrcParseError::EmptyContent);
    assert_eq!(
        LrcFile::parse("   \n\t  ").unwrap_err(),
        LrcParseError::EmptyContent
    );

    // Malformed lines mixed with valid lines
    let noisy_content = r#"
Random non-lyric heading line
[invalid tag without colon]
[00:03.00]First valid line
Malformed line without brackets
[00:09.50]Second valid line
"#;

    let lrc = LrcFile::parse(noisy_content).expect("Parse noisy LRC");
    assert_eq!(lrc.lines.len(), 2);
    assert_eq!(lrc.lines[0].text, "First valid line");
    assert_eq!(lrc.lines[1].text, "Second valid line");
}

#[test]
fn test_lrc_sidecar_detection_and_loading() {
    let dir = tempdir().unwrap();
    let audio_path = dir.path().join("track01.flac");
    let lrc_path = dir.path().join("track01.lrc");

    // Write empty audio placeholder
    File::create(&audio_path).unwrap();

    // No sidecar yet
    assert_eq!(find_sidecar_lrc(&audio_path), None);

    // Write sidecar .lrc
    let mut file = File::create(&lrc_path).unwrap();
    writeln!(file, "[ti:Synthetic Track]").unwrap();
    writeln!(file, "[00:02.00]Sidecar line one").unwrap();
    writeln!(file, "[00:05.50]Sidecar line two").unwrap();
    file.flush().unwrap();

    let discovered = find_sidecar_lrc(&audio_path).expect("Sidecar should be discovered");
    assert_eq!(discovered, lrc_path);

    let lrc_text = std::fs::read_to_string(&discovered).unwrap();
    let lrc = LrcFile::parse(&lrc_text).unwrap();
    assert_eq!(lrc.lines.len(), 2);
    assert_eq!(lrc.active_line(3000).unwrap().text, "Sidecar line one");
    assert_eq!(lrc.active_line(6000).unwrap().text, "Sidecar line two");
}

#[test]
fn test_enhanced_lrc_word_karaoke_parsing() {
    let content = r#"
[ti:Quantum Echo]
[ar:Muvrix Soundlab]
[00:02.00]<00:02.00>Signals <00:02.80>propagate <00:03.50>across <00:04.20>fiber
[00:06.00]<00:06.00>Zero <00:06.50>latency <00:07.20>stream
"#;

    let lrc = LrcFile::parse(content).expect("Parse Enhanced LRC");
    assert_eq!(lrc.lines.len(), 2);

    let l0 = &lrc.lines[0];
    assert_eq!(l0.text, "Signals propagate across fiber");
    assert_eq!(l0.words.len(), 4);
    assert_eq!(l0.words[0].text, "Signals ");
    assert_eq!(l0.words[0].timestamp_ms, 2000);
    assert_eq!(l0.words[1].text, "propagate ");
    assert_eq!(l0.words[1].timestamp_ms, 2800);
    assert_eq!(l0.words[2].text, "across ");
    assert_eq!(l0.words[2].timestamp_ms, 3500);
    assert_eq!(l0.words[3].text, "fiber");
    assert_eq!(l0.words[3].timestamp_ms, 4200);

    // Timeline seeking and word activation
    assert_eq!(l0.find_active_word_index(1500), None);
    assert_eq!(l0.find_active_word_index(2000), Some(0)); // "Signals "
    assert_eq!(l0.find_active_word_index(2900), Some(1)); // "propagate "
    assert_eq!(l0.find_active_word_index(3600), Some(2)); // "across "
    assert_eq!(l0.find_active_word_index(4500), Some(3)); // "fiber"

    let l1 = &lrc.lines[1];
    assert_eq!(l1.text, "Zero latency stream");
    assert_eq!(l1.words.len(), 3);
    assert_eq!(l1.find_active_word_index(6100), Some(0)); // "Zero "
    assert_eq!(l1.find_active_word_index(6700), Some(1)); // "latency "
    assert_eq!(l1.find_active_word_index(7500), Some(2)); // "stream"
}
