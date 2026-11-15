//! M3U/M3U8 parsing for local files and HTTP(S) playlists.

use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum M3uError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("invalid playlist location: {0}")]
    InvalidSource(String),
}

pub type M3uResult<T> = std::result::Result<T, M3uError>;

/// One resolved playlist entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3uEntry {
    pub path: String,
    pub title: Option<String>,
    pub duration_ms: u64,
    pub logo: Option<String>,
    pub group: Option<String>,
}

/// Parses M3U text, resolving relative paths against `source`.
pub fn parse_m3u(content: &str, source: &str) -> M3uResult<Vec<M3uEntry>> {
    let mut entries = Vec::new();
    let mut pending: Option<M3uEntry> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let (attrs, meta_title, duration) = parse_extinf(rest);
            let title = if meta_title.trim().is_empty() {
                attrs
                    .get("tvg-name")
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty())
            } else {
                Some(meta_title.trim().to_string())
            };
            pending = Some(M3uEntry {
                path: String::new(),
                title,
                duration_ms: if duration > 0.0 {
                    (duration * 1000.0) as u64
                } else {
                    0
                },
                logo: attrs.get("tvg-logo").cloned(),
                group: attrs
                    .get("group-title")
                    .cloned()
                    .or_else(|| attrs.get("group").cloned()),
            });
        } else if line.starts_with('#') {
            continue;
        } else {
            let resolved = resolve_entry(source, line)?;
            let mut entry = pending.take().unwrap_or(M3uEntry {
                path: String::new(),
                title: None,
                duration_ms: 0,
                logo: None,
                group: None,
            });
            entry.path = resolved;
            entries.push(entry);
        }
    }

    Ok(entries)
}

/// Loads a local M3U/M3U8 file or an HTTP(S) playlist URL.
pub fn load_m3u(source: &str) -> M3uResult<Vec<M3uEntry>> {
    let source = source.trim();
    if source.starts_with("http://") || source.starts_with("https://") {
        let response = ureq::get(source)
            .call()
            .map_err(|e| M3uError::Http(e.to_string()))?;
        let body = response
            .into_string()
            .map_err(|e| M3uError::Http(e.to_string()))?;
        parse_m3u(&body, source)
    } else {
        let content = std::fs::read_to_string(source)?;
        parse_m3u(&content, source)
    }
}

/// Generates M3U8 playlist text from a list of entries.
///
/// Produces an extended M3U8 file with `#EXTM3U` header, `#EXTINF` lines for
/// each entry that has a known duration or title, followed by the media path.
pub fn generate_m3u(entries: &[M3uEntry]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for entry in entries {
        let duration_secs = if entry.duration_ms > 0 {
            (entry.duration_ms / 1000) as i64
        } else {
            -1
        };
        let title = entry.title.as_deref().unwrap_or("");
        out.push_str(&format!("#EXTINF:{},{}\n", duration_secs, title));
        out.push_str(&entry.path);
        out.push('\n');
    }
    out
}

/// Writes M3U8 playlist content to a local file at `dest`.
///
/// Creates or truncates the destination file. Returns an I/O error on failure.
pub fn export_m3u(entries: &[M3uEntry], dest: &str) -> M3uResult<()> {
    let content = generate_m3u(entries);
    std::fs::write(dest, content.as_bytes())?;
    Ok(())
}

fn parse_extinf(rest: &str) -> (HashMap<String, String>, String, f64) {
    let mut attrs = HashMap::new();
    let mut quoted = false;
    let mut split_at = None;
    for (index, ch) in rest.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                split_at = Some(index);
                break;
            }
            _ => {}
        }
    }
    let (head, title) = match split_at {
        Some(index) => (&rest[..index], &rest[index + 1..]),
        None => (rest, ""),
    };
    let duration = head
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(-1.0);
    attrs.extend(parse_attributes(head));
    (attrs, title.to_string(), duration)
}

fn parse_attributes(input: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let mut rest = input.trim();
    // Skip the leading duration token.
    if let Some(index) = rest.find(char::is_whitespace) {
        rest = rest[index..].trim();
    } else {
        return attrs;
    }
    while !rest.is_empty() {
        if let Some(eq) = rest.find('=') {
            let key = rest[..eq].trim();
            rest = rest[eq + 1..].trim_start();
            if let Some(quoted) = rest.strip_prefix('"') {
                if let Some(end) = quoted.find('"') {
                    attrs.insert(key.to_ascii_lowercase(), quoted[..end].to_string());
                    rest = quoted[end + 1..].trim_start();
                    continue;
                }
            }
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            attrs.insert(key.to_ascii_lowercase(), rest[..end].to_string());
            rest = rest[end..].trim_start();
        } else {
            break;
        }
    }
    attrs
}

fn resolve_entry(source: &str, entry: &str) -> M3uResult<String> {
    let entry = entry.trim();
    if is_absolute_url(entry) {
        return Ok(entry.to_string());
    }
    if Path::new(entry).is_absolute() {
        return Ok(entry.to_string());
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        return join_url(source, entry);
    }
    let parent = Path::new(source).parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(entry).to_string_lossy().to_string())
}

fn is_absolute_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn join_url(base: &str, relative: &str) -> M3uResult<String> {
    let scheme_end = base
        .find("://")
        .ok_or_else(|| M3uError::InvalidSource(base.to_string()))?;
    let scheme = &base[..scheme_end + 3];
    let rest = &base[scheme_end + 3..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    let origin = &rest[..host_end];
    let base_path = &rest[host_end..];
    let (base_path, _) = base_path.split_once('?').unwrap_or((base_path, ""));

    let joined = if relative.starts_with('/') {
        relative.to_string()
    } else {
        let directory = base_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        format!("{directory}/{relative}")
    };
    Ok(format!("{scheme}{origin}{}", normalize_url_path(&joined)))
}

fn normalize_url_path(path: &str) -> String {
    let mut output: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                output.pop();
            }
            value => output.push(value),
        }
    }
    format!("/{}", output.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn parses_extinf_attributes_and_relative_local_paths() {
        let content = "#EXTM3U\n\
            #EXTINF:-1 tvg-logo=\"logo.png\" group-title=\"News\",Evening News\n\
            news/evening.mp4\n\
            #EXTINF:42.5,Trailer\n\
            https://example.test/trailer.mp4\n";
        let entries = parse_m3u(content, "/media/playlists/list.m3u").expect("parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title.as_deref(), Some("Evening News"));
        assert_eq!(entries[0].logo.as_deref(), Some("logo.png"));
        assert_eq!(entries[0].group.as_deref(), Some("News"));
        assert_eq!(
            entries[0].path,
            "/media/playlists/news/evening.mp4".to_string()
        );
        assert_eq!(entries[1].duration_ms, 42_500);
        assert_eq!(entries[1].path, "https://example.test/trailer.mp4");
    }

    #[test]
    fn resolves_relative_entries_against_remote_playlist() {
        let entries = parse_m3u(
            "#EXTM3U\n#EXTINF:-1,One\nstreams/one.m3u8\n",
            "https://example.test/lists/main.m3u8",
        )
        .expect("parse");
        assert_eq!(
            entries[0].path,
            "https://example.test/lists/streams/one.m3u8"
        );
    }

    #[test]
    fn loads_playlist_over_http() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let body = "#EXTM3U\n#EXTINF:-1,Remote\nrelative/file.mp4\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        });
        let entries = load_m3u(&format!("http://127.0.0.1:{port}/lists/list.m3u8")).expect("load");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].path,
            format!("http://127.0.0.1:{port}/lists/relative/file.mp4")
        );
        handle.join().expect("join");
    }

    #[test]
    fn generate_m3u_produces_valid_extended_m3u() {
        let entries = vec![
            M3uEntry {
                path: "/media/video.mp4".to_string(),
                title: Some("My Video".to_string()),
                duration_ms: 90_500,
                logo: None,
                group: None,
            },
            M3uEntry {
                path: "/media/music.mp3".to_string(),
                title: None,
                duration_ms: 0,
                logo: None,
                group: None,
            },
        ];
        let output = generate_m3u(&entries);
        assert!(output.starts_with("#EXTM3U\n"));
        assert!(output.contains("#EXTINF:90,My Video\n/media/video.mp4\n"));
        assert!(output.contains("#EXTINF:-1,\n/media/music.mp3\n"));
    }

    #[test]
    fn export_m3u_writes_file_and_round_trips() {
        let dir = std::env::temp_dir();
        let dest = dir.join("muvrix_test_export.m3u");
        let entries = vec![M3uEntry {
            path: "/home/user/clip.mkv".to_string(),
            title: Some("Test Clip".to_string()),
            duration_ms: 5_000,
            logo: None,
            group: None,
        }];
        export_m3u(&entries, dest.to_str().unwrap()).expect("export");
        let content = std::fs::read_to_string(&dest).expect("read");
        assert!(content.contains("Test Clip"));
        assert!(content.contains("/home/user/clip.mkv"));
        let _ = std::fs::remove_file(&dest);
    }
}
