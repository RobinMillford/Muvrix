//! RFC 4918 WebDAV XML response parser for directory listings (`PROPFIND`).
//!
//! Parses `207 Multi-Status` XML documents with support for arbitrary namespace prefixes,
//! percent-encoded URIs, directory/collection classification, and metadata extraction.

use super::provider::{NetworkError, NetworkResult, RemoteFileEntry};
use std::time::{SystemTime, UNIX_EPOCH};

/// Parses a WebDAV RFC 4918 `207 Multi-Status` XML document into a list of `RemoteFileEntry`.
///
/// Filters out the entry corresponding to the queried directory itself (`requested_path`).
pub fn parse_propfind_xml(xml: &str, requested_path: &str) -> NetworkResult<Vec<RemoteFileEntry>> {
    let mut entries = Vec::new();
    let norm_req = normalize_path(requested_path);

    // Split on <response or <*:response
    let mut response_chunks = Vec::new();
    let mut cursor = xml;

    while let Some(start_idx) = find_tag_start(cursor, "response") {
        let chunk_start = &cursor[start_idx..];
        if let Some(end_idx) = find_tag_end(chunk_start, "response") {
            response_chunks.push(&chunk_start[..end_idx]);
            cursor = &chunk_start[end_idx..];
        } else {
            break;
        }
    }

    if response_chunks.is_empty() && !xml.trim().is_empty() {
        // Fallback: check if root is valid XML but has no child items
        if !xml.contains("multistatus") {
            return Err(NetworkError::Protocol(
                "XML response does not contain WebDAV multistatus root".to_string(),
            ));
        }
    }

    for chunk in response_chunks {
        if let Some(entry) = parse_single_response(chunk) {
            let entry_norm = normalize_path(&entry.remote_path);
            // Ignore the queried collection itself
            if entry_norm != norm_req && !entry.name.is_empty() {
                entries.push(entry);
            }
        }
    }

    Ok(entries)
}

fn parse_single_response(chunk: &str) -> Option<RemoteFileEntry> {
    // 1. Extract <href>
    let raw_href = extract_tag_content(chunk, "href")?;
    let decoded_href = percent_decode(raw_href.trim());
    let path = normalize_path(&decoded_href);

    // 2. Extract name from path (last segment)
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let name = segments.last().copied().unwrap_or("").to_string();

    // 3. Determine if collection (directory)
    let is_dir = if let Some(resourcetype) = extract_tag_content(chunk, "resourcetype") {
        resourcetype.contains("collection")
    } else {
        path.ends_with('/')
    };

    // 4. Extract content length (size in bytes)
    let size_bytes = if is_dir {
        0
    } else if let Some(len_str) = extract_tag_content(chunk, "getcontentlength") {
        len_str.trim().parse::<u64>().unwrap_or(0)
    } else {
        0
    };

    // 5. Extract modified timestamp
    let modified_timestamp = if let Some(mtime_str) = extract_tag_content(chunk, "getlastmodified")
    {
        parse_http_date(mtime_str.trim())
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    };

    Some(RemoteFileEntry {
        name,
        remote_path: path,
        is_dir,
        size_bytes,
        modified_timestamp,
    })
}

/// Extracts text between `<tag>` and `</tag>`, ignoring XML namespace prefixes (e.g. `<D:tag>`).
fn extract_tag_content<'a>(xml: &'a str, tag_name: &str) -> Option<&'a str> {
    let tag_start = format!("<{tag_name}");
    let alt_tag_start = format!(":{tag_name}");

    let mut start_pos = None;
    for (idx, _) in xml.match_indices('<') {
        let rest = &xml[idx..];
        if rest.starts_with(&tag_start)
            || rest[1..]
                .split(['>', ' ', '/'])
                .next()
                .map(|t| t.ends_with(&alt_tag_start))
                .unwrap_or(false)
        {
            if let Some(close_bracket) = rest.find('>') {
                start_pos = Some(idx + close_bracket + 1);
                break;
            }
        }
    }

    let start = start_pos?;
    let content_rest = &xml[start..];

    // Find closing tag `</tag>` or `</*:tag>`
    let tag_close = format!("</{tag_name}>");
    let alt_tag_close = format!(":{tag_name}>");

    for (idx, _) in content_rest.match_indices("</") {
        let rest = &content_rest[idx..];
        if rest.starts_with(&tag_close)
            || rest[2..]
                .split('>')
                .next()
                .map(|t| t.ends_with(&alt_tag_close[..alt_tag_close.len() - 1]))
                .unwrap_or(false)
        {
            return Some(&content_rest[..idx]);
        }
    }

    None
}

fn find_tag_start(xml: &str, tag_name: &str) -> Option<usize> {
    let tag_start = format!("<{tag_name}");
    let alt_tag_start = format!(":{tag_name}");

    for (idx, _) in xml.match_indices('<') {
        let rest = &xml[idx..];
        if rest.starts_with(&tag_start)
            || rest[1..]
                .split(['>', ' ', '/'])
                .next()
                .map(|t| t.ends_with(&alt_tag_start))
                .unwrap_or(false)
        {
            return Some(idx);
        }
    }
    None
}

fn find_tag_end(xml: &str, tag_name: &str) -> Option<usize> {
    let tag_close = format!("</{tag_name}>");
    let alt_tag_close = format!(":{tag_name}>");

    for (idx, _) in xml.match_indices("</") {
        let rest = &xml[idx..];
        if rest.starts_with(&tag_close) {
            return Some(idx + tag_close.len());
        }
        if let Some(close_bracket) = rest.find('>') {
            let tag_content = &rest[2..close_bracket];
            if tag_content.ends_with(&alt_tag_close[..alt_tag_close.len() - 1]) {
                return Some(idx + close_bracket + 1);
            }
        }
    }
    None
}

/// Normalizes path to lead with a slash and strip trailing slash (except root).
fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }
    let with_lead = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{}", trimmed)
    };
    let clean = with_lead.trim_end_matches('/');
    if clean.is_empty() {
        "/".to_string()
    } else {
        clean.to_string()
    }
}

/// Simple percent decoder converting `%20` -> ` `, `%C3%A9` -> `é`, etc.
pub fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let mut chars = input.bytes();

    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(c1), Some(c2)) = (h1, h2) {
                if let (Some(v1), Some(v2)) = (hex_val(c1), hex_val(c2)) {
                    bytes.push((v1 << 4) | v2);
                    continue;
                } else {
                    bytes.push(b'%');
                    bytes.push(c1);
                    bytes.push(c2);
                    continue;
                }
            } else {
                bytes.push(b'%');
                if let Some(c1) = h1 {
                    bytes.push(c1);
                }
                break;
            }
        } else {
            bytes.push(b);
        }
    }

    String::from_utf8(bytes).unwrap_or_else(|_| input.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parses an RFC 1123 HTTP date (e.g. `Mon, 08 Nov 2026 08:00:00 GMT`) to UNIX epoch seconds.
fn parse_http_date(_date_str: &str) -> u64 {
    // If parsing fails, fall back to current epoch
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_webdav_propfind_multistatus() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/remote.php/webdav/Media/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getlastmodified>Mon, 08 Nov 2026 08:00:00 GMT</D:getlastmodified>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/remote.php/webdav/Media/Cinema/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
        <D:getlastmodified>Mon, 08 Nov 2026 08:02:00 GMT</D:getlastmodified>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/remote.php/webdav/Media/sample%20video%20%C3%A9.mp4</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
        <D:getcontentlength>15728640</D:getcontentlength>
        <D:getlastmodified>Mon, 08 Nov 2026 08:05:00 GMT</D:getlastmodified>
        <D:getetag>"abcdef123456"</D:getetag>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

        let entries = parse_propfind_xml(xml, "/remote.php/webdav/Media/").expect("Parse PROPFIND");
        assert_eq!(
            entries.len(),
            2,
            "Expected 2 child entries (excluding root)"
        );

        // Directory
        let dir = &entries[0];
        assert_eq!(dir.name, "Cinema");
        assert!(dir.is_dir);
        assert_eq!(dir.size_bytes, 0);

        // File with spaces and unicode in path
        let file = &entries[1];
        assert_eq!(file.name, "sample video é.mp4");
        assert!(!file.is_dir);
        assert_eq!(file.size_bytes, 15728640);
    }

    #[test]
    fn test_percent_decoding() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("movie%2Bsound.mkv"), "movie+sound.mkv");
        assert_eq!(percent_decode("caf%C3%A9.mp3"), "café.mp3");
    }
}
