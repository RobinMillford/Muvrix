//! Media Server REST clients and catalog synchronization for Jellyfin, Emby, and Plex.
//!
//! Provides token-authenticated REST access for remote library browsing,
//! metadata synchronization, and direct stream URL generation for libmpv.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::credentials::{redact_credentials_from_url, CredentialVault};
use super::provider::{NetworkError, NetworkProvider, NetworkResult, RemoteFileEntry};

// ============================================================================
// JELLYFIN & EMBY DATA STRUCTURES
// ============================================================================

/// Public server information returned by Jellyfin / Emby `/System/Info/Public`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinSystemInfo {
    pub server_name: Option<String>,
    pub version: Option<String>,
    pub id: Option<String>,
    pub operating_system: Option<String>,
}

/// A media item in Jellyfin (Movie, Episode, Music, Series, or Folder).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinItem {
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub item_type: String,
    pub run_time_ticks: Option<u64>,
    pub container: Option<String>,
    pub size: Option<u64>,
    pub production_year: Option<u32>,
    #[serde(default)]
    pub media_sources: Vec<JellyfinMediaSource>,
}

/// Underlying media source file associated with a Jellyfin item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinMediaSource {
    pub id: Option<String>,
    pub path: Option<String>,
    pub container: Option<String>,
    pub size: Option<u64>,
    pub bitrate: Option<u64>,
}

/// Paginated item collection returned by Jellyfin `/Items` or `/Users/{id}/Items`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JellyfinCatalogResponse {
    #[serde(default)]
    pub items: Vec<JellyfinItem>,
    pub total_record_count: Option<usize>,
    pub start_index: Option<usize>,
}

// ============================================================================
// PLEX DATA STRUCTURES
// ============================================================================

/// Top-level Plex MediaContainer wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexContainer<T> {
    #[serde(rename = "MediaContainer")]
    pub media_container: T,
}

/// Plex library sections container (`/library/sections`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexSectionsContainer {
    pub size: Option<usize>,
    pub title1: Option<String>,
    #[serde(rename = "Directory", default)]
    pub directory: Vec<PlexDirectory>,
}

/// Plex library section directory (e.g. Movies, TV Shows, Music).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexDirectory {
    pub key: String,
    pub title: String,
    #[serde(rename = "type")]
    pub section_type: String,
    pub agent: Option<String>,
    pub scanner: Option<String>,
    pub language: Option<String>,
}

/// Plex library items container (`/library/sections/{id}/all`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexLibraryContainer {
    pub size: Option<usize>,
    #[serde(rename = "Metadata", default)]
    pub metadata: Vec<PlexMetadataItem>,
}

/// Individual media item metadata in Plex.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexMetadataItem {
    #[serde(rename = "ratingKey")]
    pub rating_key: String,
    pub key: String,
    pub title: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub duration: Option<u64>,
    pub year: Option<u32>,
    #[serde(rename = "Media", default)]
    pub media: Vec<PlexMediaItem>,
}

/// Plex media representation describing stream codecs and containers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexMediaItem {
    pub id: u64,
    pub duration: Option<u64>,
    pub bitrate: Option<u64>,
    pub container: Option<String>,
    #[serde(rename = "videoCodec")]
    pub video_codec: Option<String>,
    #[serde(rename = "audioCodec")]
    pub audio_codec: Option<String>,
    #[serde(rename = "Part", default)]
    pub part: Vec<PlexPartItem>,
}

/// File part describing the exact stream URL path and size on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexPartItem {
    pub id: u64,
    pub key: String,
    pub duration: Option<u64>,
    pub file: Option<String>,
    pub size: Option<u64>,
    pub container: Option<String>,
}

// ============================================================================
// JELLYFIN PROVIDER IMPLEMENTATION
// ============================================================================

/// Concrete REST provider for Jellyfin / Emby media servers.
#[derive(Debug, Clone)]
pub struct JellyfinProvider {
    pub host: String,
    pub port: u16,
    pub use_https: bool,
    pub token: Option<String>,
    pub credential_ref: Option<String>,
    pub user_id: Option<String>,
    pub vault: CredentialVault,
    pub timeout_ms: u64,
}

impl JellyfinProvider {
    /// Creates a new Jellyfin provider instance.
    pub fn new(
        host: &str,
        port: u16,
        use_https: bool,
        token: Option<String>,
        credential_ref: Option<String>,
        user_id: Option<String>,
        vault: CredentialVault,
    ) -> Self {
        Self {
            host: host.trim().to_string(),
            port: if port == 0 {
                if use_https {
                    8920
                } else {
                    8096
                }
            } else {
                port
            },
            use_https,
            token,
            credential_ref,
            user_id,
            vault,
            timeout_ms: 6000,
        }
    }

    /// Sets connection timeout in milliseconds.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Resolves the active API token from parameters, credential vault, or keyring.
    pub fn resolve_token(&self) -> Option<String> {
        if let Some(ref t) = self.token {
            if !t.trim().is_empty() {
                return Some(t.clone());
            }
        }
        if let Some(ref cr) = self.credential_ref {
            if let Some(secret) = self.vault.get(cr) {
                if !secret.trim().is_empty() {
                    return Some(secret);
                }
            }
        }
        None
    }

    /// Formats the base HTTP/HTTPS URL (e.g. `http://jellyfin.local:8096`).
    pub fn base_url(&self) -> String {
        let scheme = if self.use_https { "https" } else { "http" };
        let default_port = if self.use_https { 8920 } else { 8096 };
        if self.port == default_port && (self.port == 80 || self.port == 443) {
            format!("{}://{}", scheme, self.host)
        } else {
            format!("{}://{}:{}", scheme, self.host, self.port)
        }
    }

    /// Builds a configured `ureq::Agent` with appropriate timeouts.
    fn http_agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(self.timeout_ms.min(5000)))
            .timeout_read(Duration::from_millis(self.timeout_ms))
            .build()
    }

    /// Validates the server configuration.
    pub fn validate_config(&self) -> NetworkResult<()> {
        if self.host.is_empty() {
            return Err(NetworkError::Protocol(
                "Jellyfin host address cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    /// Parses a JSON catalog payload returned by Jellyfin into strongly-typed `RemoteFileEntry` items.
    pub fn parse_catalog_json(json_str: &str) -> NetworkResult<Vec<RemoteFileEntry>> {
        let catalog: JellyfinCatalogResponse = serde_json::from_str(json_str).map_err(|e| {
            NetworkError::Protocol(format!("Failed to deserialize Jellyfin catalog JSON: {e}"))
        })?;

        let mut entries = Vec::with_capacity(catalog.items.len());
        for item in catalog.items {
            let is_dir = item.item_type.eq_ignore_ascii_case("Folder")
                || item.item_type.eq_ignore_ascii_case("CollectionFolder")
                || item.item_type.eq_ignore_ascii_case("Series")
                || item.item_type.eq_ignore_ascii_case("Season");

            let size_bytes = item
                .size
                .or_else(|| item.media_sources.first().and_then(|ms| ms.size))
                .unwrap_or(0);

            let ext = item
                .container
                .as_deref()
                .or_else(|| {
                    item.media_sources
                        .first()
                        .and_then(|ms| ms.container.as_deref())
                })
                .unwrap_or("");

            let display_name = if !is_dir && !ext.is_empty() && !item.name.ends_with(ext) {
                format!("{}.{}", item.name, ext)
            } else {
                item.name
            };

            entries.push(RemoteFileEntry {
                name: display_name,
                remote_path: format!("/items/{}", item.id),
                is_dir,
                size_bytes,
                modified_timestamp: 0,
            });
        }

        Ok(entries)
    }
}

impl NetworkProvider for JellyfinProvider {
    fn protocol(&self) -> &'static str {
        "jellyfin"
    }

    fn test_connection(&self) -> NetworkResult<bool> {
        self.validate_config()?;
        let url = format!("{}/System/Info/Public", self.base_url());
        let agent = self.http_agent();

        let mut req = agent.get(&url);
        if let Some(token) = self.resolve_token() {
            req = req.set("X-Emby-Token", &token);
        }

        match req.call() {
            Ok(resp) => {
                if resp.status() == 200 {
                    Ok(true)
                } else {
                    Err(NetworkError::Protocol(format!(
                        "Jellyfin system info returned HTTP status {}",
                        resp.status()
                    )))
                }
            }
            Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
                Err(NetworkError::AuthenticationFailed(
                    "Jellyfin server authentication failed (unauthorized token)".to_string(),
                ))
            }
            Err(ureq::Error::Transport(e)) => Err(NetworkError::ConnectionFailed(format!(
                "Jellyfin connection to {url} failed: {e}"
            ))),
            Err(e) => Err(NetworkError::Protocol(e.to_string())),
        }
    }

    fn list_directory(&self, path: &str) -> NetworkResult<Vec<RemoteFileEntry>> {
        self.validate_config()?;
        let token = self.resolve_token().ok_or_else(|| {
            NetworkError::AuthenticationFailed(
                "Jellyfin API token is required for catalog access".to_string(),
            )
        })?;

        let base = self.base_url();
        let agent = self.http_agent();

        let url = if path == "/" || path.is_empty() {
            if let Some(ref uid) = self.user_id {
                format!("{base}/Users/{uid}/Views")
            } else {
                format!("{base}/Library/MediaFolders")
            }
        } else if let Some(parent_id) = path.strip_prefix("/items/") {
            format!("{base}/Items?ParentId={parent_id}&Recursive=true&IncludeItemTypes=Movie,Episode,Audio,Video")
        } else {
            let clean = path.trim_matches('/');
            format!("{base}/Items?ParentId={clean}&Recursive=true&IncludeItemTypes=Movie,Episode,Audio,Video")
        };

        let resp = agent
            .get(&url)
            .set("X-Emby-Token", &token)
            .set(
                "X-Emby-Authorization",
                "MediaBrowser Client=\"Muvrix\", Device=\"Desktop\", DeviceId=\"muvrix-1\", Version=\"1.4\"",
            )
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => {
                    NetworkError::AuthenticationFailed("Jellyfin token rejected".to_string())
                }
                ureq::Error::Status(404, _) => NetworkError::NotFound(format!("Jellyfin resource not found: {url}")),
                ureq::Error::Transport(t) => NetworkError::ConnectionFailed(format!("Jellyfin transport error: {t}")),
                other => NetworkError::Protocol(other.to_string()),
            })?;

        let json_str = resp.into_string().map_err(|e| {
            NetworkError::Protocol(format!("Failed to read Jellyfin response string: {e}"))
        })?;

        Self::parse_catalog_json(&json_str)
    }

    fn resolve_stream_url(&self, remote_file_path: &str) -> NetworkResult<String> {
        self.validate_config()?;
        let token = self.resolve_token().ok_or_else(|| {
            NetworkError::AuthenticationFailed(
                "Jellyfin API token required for stream URL resolution".to_string(),
            )
        })?;

        let item_id = remote_file_path
            .strip_prefix("/items/")
            .unwrap_or(remote_file_path)
            .trim_matches('/');

        if item_id.is_empty() {
            return Err(NetworkError::Protocol(
                "Invalid empty Jellyfin item ID for stream playback".to_string(),
            ));
        }

        // Direct play streaming endpoint without server-side transcoding
        Ok(format!(
            "{}/Videos/{item_id}/stream?static=true&api_key={token}",
            self.base_url()
        ))
    }
}

impl fmt::Display for JellyfinProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base = self.base_url();
        write!(f, "Jellyfin ({})", redact_credentials_from_url(&base))
    }
}

// ============================================================================
// PLEX PROVIDER IMPLEMENTATION
// ============================================================================

/// Concrete REST provider for Plex Media Server.
#[derive(Debug, Clone)]
pub struct PlexProvider {
    pub host: String,
    pub port: u16,
    pub use_https: bool,
    pub token: Option<String>,
    pub credential_ref: Option<String>,
    pub vault: CredentialVault,
    pub timeout_ms: u64,
}

impl PlexProvider {
    /// Creates a new Plex provider instance.
    pub fn new(
        host: &str,
        port: u16,
        use_https: bool,
        token: Option<String>,
        credential_ref: Option<String>,
        vault: CredentialVault,
    ) -> Self {
        Self {
            host: host.trim().to_string(),
            port: if port == 0 { 32400 } else { port },
            use_https,
            token,
            credential_ref,
            vault,
            timeout_ms: 6000,
        }
    }

    /// Sets connection timeout in milliseconds.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Resolves the active Plex auth token from parameters or vault.
    pub fn resolve_token(&self) -> Option<String> {
        if let Some(ref t) = self.token {
            if !t.trim().is_empty() {
                return Some(t.clone());
            }
        }
        if let Some(ref cr) = self.credential_ref {
            if let Some(secret) = self.vault.get(cr) {
                if !secret.trim().is_empty() {
                    return Some(secret);
                }
            }
        }
        None
    }

    /// Formats the base Plex server URL (e.g. `http://plex.local:32400`).
    pub fn base_url(&self) -> String {
        let scheme = if self.use_https { "https" } else { "http" };
        if self.port == 32400 {
            format!("{}://{}:32400", scheme, self.host)
        } else {
            format!("{}://{}:{}", scheme, self.host, self.port)
        }
    }

    /// Builds a configured `ureq::Agent` with appropriate timeouts.
    fn http_agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_millis(self.timeout_ms.min(5000)))
            .timeout_read(Duration::from_millis(self.timeout_ms))
            .build()
    }

    /// Validates basic configuration requirements.
    pub fn validate_config(&self) -> NetworkResult<()> {
        if self.host.is_empty() {
            return Err(NetworkError::Protocol(
                "Plex host address cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    /// Parses a JSON sections payload returned by Plex `/library/sections`.
    pub fn parse_sections_json(json_str: &str) -> NetworkResult<Vec<RemoteFileEntry>> {
        let container: PlexContainer<PlexSectionsContainer> = serde_json::from_str(json_str)
            .map_err(|e| {
                NetworkError::Protocol(format!("Failed to deserialize Plex sections JSON: {e}"))
            })?;

        let mut entries = Vec::with_capacity(container.media_container.directory.len());
        for dir in container.media_container.directory {
            entries.push(RemoteFileEntry {
                name: dir.title,
                remote_path: format!("/library/sections/{}", dir.key),
                is_dir: true,
                size_bytes: 0,
                modified_timestamp: 0,
            });
        }

        Ok(entries)
    }

    /// Parses a JSON library items payload returned by Plex `/library/sections/{id}/all`.
    pub fn parse_library_items_json(json_str: &str) -> NetworkResult<Vec<RemoteFileEntry>> {
        let container: PlexContainer<PlexLibraryContainer> = serde_json::from_str(json_str)
            .map_err(|e| {
                NetworkError::Protocol(format!("Failed to deserialize Plex library JSON: {e}"))
            })?;

        let mut entries = Vec::with_capacity(container.media_container.metadata.len());
        for item in container.media_container.metadata {
            let part = item.media.first().and_then(|m| m.part.first());
            let size_bytes = part.and_then(|p| p.size).unwrap_or(0);
            let part_key = part
                .map(|p| p.key.clone())
                .unwrap_or_else(|| item.key.clone());

            let container_ext = item
                .media
                .first()
                .and_then(|m| m.container.as_deref())
                .unwrap_or("");

            let display_name = if !container_ext.is_empty() && !item.title.ends_with(container_ext)
            {
                format!("{}.{}", item.title, container_ext)
            } else {
                item.title
            };

            entries.push(RemoteFileEntry {
                name: display_name,
                remote_path: part_key,
                is_dir: false,
                size_bytes,
                modified_timestamp: 0,
            });
        }

        Ok(entries)
    }
}

impl NetworkProvider for PlexProvider {
    fn protocol(&self) -> &'static str {
        "plex"
    }

    fn test_connection(&self) -> NetworkResult<bool> {
        self.validate_config()?;
        let url = format!("{}/identity", self.base_url());
        let agent = self.http_agent();

        let mut req = agent.get(&url).set("Accept", "application/json");
        if let Some(token) = self.resolve_token() {
            req = req.set("X-Plex-Token", &token);
        }

        match req.call() {
            Ok(resp) => {
                if resp.status() == 200 {
                    Ok(true)
                } else {
                    Err(NetworkError::Protocol(format!(
                        "Plex identity check returned HTTP status {}",
                        resp.status()
                    )))
                }
            }
            Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
                Err(NetworkError::AuthenticationFailed(
                    "Plex authentication failed (invalid or missing X-Plex-Token)".to_string(),
                ))
            }
            Err(ureq::Error::Transport(e)) => Err(NetworkError::ConnectionFailed(format!(
                "Plex connection to {url} failed: {e}"
            ))),
            Err(e) => Err(NetworkError::Protocol(e.to_string())),
        }
    }

    fn list_directory(&self, path: &str) -> NetworkResult<Vec<RemoteFileEntry>> {
        self.validate_config()?;
        let token = self.resolve_token().ok_or_else(|| {
            NetworkError::AuthenticationFailed(
                "Plex auth token is required for library browsing".to_string(),
            )
        })?;

        let base = self.base_url();
        let agent = self.http_agent();

        let is_root = path == "/" || path.is_empty() || path == "/library/sections";
        let url = if is_root {
            format!("{base}/library/sections")
        } else {
            let clean = path.trim_matches('/');
            format!("{base}/{clean}/all")
        };

        let resp = agent
            .get(&url)
            .set("Accept", "application/json")
            .set("X-Plex-Token", &token)
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => {
                    NetworkError::AuthenticationFailed("Plex token rejected".to_string())
                }
                ureq::Error::Status(404, _) => {
                    NetworkError::NotFound(format!("Plex section not found: {url}"))
                }
                ureq::Error::Transport(t) => {
                    NetworkError::ConnectionFailed(format!("Plex transport error: {t}"))
                }
                other => NetworkError::Protocol(other.to_string()),
            })?;

        let json_str = resp.into_string().map_err(|e| {
            NetworkError::Protocol(format!("Failed to read Plex response body: {e}"))
        })?;

        if is_root {
            Self::parse_sections_json(&json_str)
        } else {
            Self::parse_library_items_json(&json_str)
        }
    }

    fn resolve_stream_url(&self, remote_file_path: &str) -> NetworkResult<String> {
        self.validate_config()?;
        let token = self.resolve_token().ok_or_else(|| {
            NetworkError::AuthenticationFailed(
                "Plex auth token required for stream URL resolution".to_string(),
            )
        })?;

        let part_path = if remote_file_path.starts_with('/') {
            remote_file_path.to_string()
        } else {
            format!("/{}", remote_file_path)
        };

        // Direct play URL with auth token query parameter
        Ok(format!(
            "{}{part_path}?X-Plex-Token={token}",
            self.base_url()
        ))
    }
}

impl fmt::Display for PlexProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base = self.base_url();
        write!(f, "Plex ({})", redact_credentials_from_url(&base))
    }
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jellyfin_catalog_json_deserialization() {
        let sample_jellyfin_json = r#"{
            "Items": [
                {
                    "Name": "Big Buck Bunny",
                    "Id": "9b1deb4d3b7d48f9ac1b539c",
                    "Type": "Movie",
                    "RunTimeTicks": 5960000000,
                    "Container": "mp4",
                    "Size": 158293740,
                    "ProductionYear": 2008,
                    "MediaSources": [
                        {
                            "Id": "ms_bbb_1",
                            "Path": "/media/movies/bbb.mp4",
                            "Container": "mp4",
                            "Size": 158293740,
                            "Bitrate": 2100000
                        }
                    ]
                },
                {
                    "Name": "Animation Showcase",
                    "Id": "folder_anim_01",
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

        let entries = JellyfinProvider::parse_catalog_json(sample_jellyfin_json)
            .expect("Deserialize Jellyfin catalog JSON");

        assert_eq!(entries.len(), 2);

        // First item: Movie with container formatting
        let movie = &entries[0];
        assert_eq!(movie.name, "Big Buck Bunny.mp4");
        assert_eq!(movie.remote_path, "/items/9b1deb4d3b7d48f9ac1b539c");
        assert!(!movie.is_dir);
        assert_eq!(movie.size_bytes, 158293740);

        // Second item: Folder
        let folder = &entries[1];
        assert_eq!(folder.name, "Animation Showcase");
        assert_eq!(folder.remote_path, "/items/folder_anim_01");
        assert!(folder.is_dir);
        assert_eq!(folder.size_bytes, 0);
    }

    #[test]
    fn test_plex_catalog_json_deserialization() {
        // Sections JSON
        let sample_sections_json = r#"{
            "MediaContainer": {
                "size": 2,
                "title1": "Plex Media Server",
                "Directory": [
                    {
                        "key": "1",
                        "title": "Movies",
                        "type": "movie",
                        "agent": "tv.plex.agents.movie",
                        "scanner": "Plex Movie"
                    },
                    {
                        "key": "2",
                        "title": "TV Series",
                        "type": "show",
                        "agent": "tv.plex.agents.series",
                        "scanner": "Plex TV Series"
                    }
                ]
            }
        }"#;

        let sections = PlexProvider::parse_sections_json(sample_sections_json)
            .expect("Deserialize Plex sections JSON");

        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].name, "Movies");
        assert_eq!(sections[0].remote_path, "/library/sections/1");
        assert!(sections[0].is_dir);

        assert_eq!(sections[1].name, "TV Series");
        assert_eq!(sections[1].remote_path, "/library/sections/2");
        assert!(sections[1].is_dir);

        // Library items JSON
        let sample_library_json = r#"{
            "MediaContainer": {
                "size": 1,
                "Metadata": [
                    {
                        "ratingKey": "402",
                        "key": "/library/metadata/402",
                        "title": "Elephants Dream",
                        "type": "movie",
                        "duration": 653000,
                        "year": 2006,
                        "Media": [
                            {
                                "id": 1002,
                                "duration": 653000,
                                "bitrate": 4500000,
                                "container": "mkv",
                                "videoCodec": "h264",
                                "audioCodec": "ac3",
                                "Part": [
                                    {
                                        "id": 2002,
                                        "key": "/library/parts/2002/123456/file.mkv",
                                        "duration": 653000,
                                        "file": "/media/movies/Elephants_Dream.mkv",
                                        "size": 367001600,
                                        "container": "mkv"
                                    }
                                ]
                            }
                        ]
                    }
                ]
            }
        }"#;

        let items = PlexProvider::parse_library_items_json(sample_library_json)
            .expect("Deserialize Plex library items JSON");

        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.name, "Elephants Dream.mkv");
        assert_eq!(item.remote_path, "/library/parts/2002/123456/file.mkv");
        assert!(!item.is_dir);
        assert_eq!(item.size_bytes, 367001600);
    }

    #[test]
    fn test_jellyfin_stream_url_resolution() {
        let vault = CredentialVault::new();
        let provider = JellyfinProvider::new(
            "jellyfin.nas.home",
            8096,
            false,
            Some("test_secret_token_123".to_string()),
            None,
            None,
            vault,
        );

        assert_eq!(provider.protocol(), "jellyfin");
        assert_eq!(provider.base_url(), "http://jellyfin.nas.home:8096");

        let stream_url = provider
            .resolve_stream_url("/items/9b1deb4d3b7d48f9ac1b539c")
            .expect("Resolve stream URL");

        assert_eq!(
            stream_url,
            "http://jellyfin.nas.home:8096/Videos/9b1deb4d3b7d48f9ac1b539c/stream?static=true&api_key=test_secret_token_123"
        );
    }

    #[test]
    fn test_plex_stream_url_resolution() {
        let vault = CredentialVault::new();
        let provider = PlexProvider::new(
            "plex.lan",
            32400,
            false,
            Some("plex_user_auth_token_xyz".to_string()),
            None,
            vault,
        );

        assert_eq!(provider.protocol(), "plex");
        assert_eq!(provider.base_url(), "http://plex.lan:32400");

        let stream_url = provider
            .resolve_stream_url("/library/parts/2002/123456/file.mkv")
            .expect("Resolve stream URL");

        assert_eq!(
            stream_url,
            "http://plex.lan:32400/library/parts/2002/123456/file.mkv?X-Plex-Token=plex_user_auth_token_xyz"
        );
    }
}
