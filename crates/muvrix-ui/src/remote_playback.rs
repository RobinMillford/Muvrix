//! Protocol-aware remote playback construction.
//!
//! The UI never passes a provider scheme (`smb://`, `sftp://`, etc.) to libmpv.
//! It builds a seekable byte stream, wraps it in recovery/read-ahead, and hands
//! it to the existing `MpvStreamBridge` as `muvrix-stream://...`.

use std::io;

use muvrix_core::database::{DatabaseManager, NetworkConnection};
use muvrix_core::network::{
    credential_ref_username, CredentialVault, JellyfinProvider, NetworkProvider, NfsProvider,
    NfsVersion, PlexProvider, SftpProvider, SmbProvider, WebDavProvider,
};
use muvrix_player::{
    HttpRangeStream, NfsSeekableStream, ReadAheadStream, RecoverableStream, SeekableStream,
    SftpSeekableStream, SmbSeekableStream, StreamRetryPolicy, StreamTelemetry,
};

#[derive(Clone)]
struct RemoteSpec {
    protocol: String,
    host: String,
    port: u16,
    share: String,
    remote_path: String,
    username: Option<String>,
    credential_ref: Option<String>,
    is_anonymous: bool,
    use_https: bool,
    db: DatabaseManager,
    vault: CredentialVault,
}

impl RemoteSpec {
    fn from_connection(
        conn: &NetworkConnection,
        remote_path: &str,
        db: &DatabaseManager,
        vault: &CredentialVault,
    ) -> Result<Self, String> {
        let protocol = conn.protocol.trim().to_ascii_lowercase();
        let username = if conn.is_anonymous {
            None
        } else {
            conn.credential_ref
                .as_deref()
                .and_then(credential_ref_username)
                .map(str::to_string)
        };
        let configured = conn.remote_path.trim_matches('/');
        let share = configured.split('/').next().unwrap_or("").to_string();
        if protocol == "smb" && share.is_empty() {
            return Err("SMB connection has no share in its remote path".to_string());
        }

        let mut path = remote_path.trim().to_string();
        if !path.starts_with('/') {
            path.insert(0, '/');
        }
        // A direct "Stream" action passes the connection's configured path,
        // which may include the SMB share prefix. The provider wants the path
        // relative to that share.
        if protocol == "smb" {
            let prefix = format!("/{share}");
            if path == prefix {
                path = "/".to_string();
            } else if let Some(rest) = path.strip_prefix(&format!("{prefix}/")) {
                path = format!("/{rest}");
            }
        }

        Ok(Self {
            protocol,
            host: conn.host.clone(),
            port: if conn.port == 0 {
                default_port(&conn.protocol)
            } else {
                conn.port
            },
            share,
            remote_path: path,
            username,
            credential_ref: conn.credential_ref.clone(),
            is_anonymous: conn.is_anonymous,
            use_https: conn.protocol.eq_ignore_ascii_case("https")
                || conn.port == 443
                || conn.port == 8443,
            db: db.clone(),
            vault: vault.clone(),
        })
    }

    fn open_once(&self) -> io::Result<Box<dyn SeekableStream>> {
        match self.protocol.as_str() {
            "webdav" | "http" | "https" => {
                let provider = WebDavProvider::new(
                    &self.host,
                    self.port,
                    &self.remote_path,
                    self.use_https,
                    self.username.clone(),
                    self.credential_ref.clone(),
                    self.vault.clone(),
                );
                let url = provider
                    .resolve_stream_url(&self.remote_path)
                    .map_err(|e| io::Error::other(format!("WebDAV URL: {e}")))?;
                let auth = provider.auth_header_value();
                let stream = HttpRangeStream::from_url(&url, auth)
                    .map_err(|e| io::Error::other(format!("WebDAV TLS/range stream: {e}")))?;
                Ok(Box::new(stream))
            }
            "smb" => {
                if self.share.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "SMB share is empty",
                    ));
                }
                let provider = SmbProvider::new(
                    &self.host,
                    self.port,
                    &self.share,
                    &self.remote_path,
                    self.username.clone(),
                    None,
                    self.credential_ref.clone(),
                    self.is_anonymous,
                    self.vault.clone(),
                );
                let stream = SmbSeekableStream::connect_via_provider(&provider, &self.remote_path)?;
                Ok(Box::new(stream))
            }
            "sftp" => {
                let username = self.username.clone().unwrap_or_default();
                if username.trim().is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "SFTP username is empty",
                    ));
                }
                let provider = SftpProvider::new(
                    &self.host,
                    self.port,
                    &self.remote_path,
                    &username,
                    self.credential_ref.clone(),
                    None,
                    None,
                    self.vault.clone(),
                )
                .with_database(self.db.clone());
                let (session, file) = provider
                    .open_file(&self.remote_path)
                    .map_err(|e| io::Error::other(format!("SFTP open: {e}")))?;
                let stream = SftpSeekableStream::with_session(session, file)?;
                Ok(Box::new(stream))
            }
            "nfs" => {
                let provider = NfsProvider::new(
                    &self.host,
                    self.port,
                    &self.remote_path,
                    NfsVersion::V4,
                    self.username.clone(),
                    self.credential_ref.clone(),
                    self.vault.clone(),
                );
                let stream = NfsSeekableStream::connect_via_provider(&provider, &self.remote_path)?;
                Ok(Box::new(stream))
            }
            "jellyfin" => {
                let token = self
                    .credential_ref
                    .as_deref()
                    .and_then(|cr| self.vault.get(cr));
                let provider = JellyfinProvider::new(
                    &self.host,
                    self.port,
                    self.use_https,
                    token,
                    self.credential_ref.clone(),
                    None,
                    self.vault.clone(),
                );
                let url = provider
                    .resolve_stream_url(&self.remote_path)
                    .map_err(|e| {
                        io::Error::other(format!("Jellyfin stream URL resolution: {e}"))
                    })?;
                let stream = HttpRangeStream::from_url(&url, None)
                    .map_err(|e| io::Error::other(format!("Jellyfin stream open: {e}")))?;
                Ok(Box::new(stream))
            }
            "plex" => {
                let token = self
                    .credential_ref
                    .as_deref()
                    .and_then(|cr| self.vault.get(cr));
                let provider = PlexProvider::new(
                    &self.host,
                    self.port,
                    self.use_https,
                    token,
                    self.credential_ref.clone(),
                    self.vault.clone(),
                );
                let url = provider
                    .resolve_stream_url(&self.remote_path)
                    .map_err(|e| io::Error::other(format!("Plex stream URL resolution: {e}")))?;
                let stream = HttpRangeStream::from_url(&url, None)
                    .map_err(|e| io::Error::other(format!("Plex stream open: {e}")))?;
                Ok(Box::new(stream))
            }
            other => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("unsupported remote protocol: {other}"),
            )),
        }
    }
}

/// Builds a recoverable, read-ahead remote stream with telemetry observation for the UI stream bridge.
pub fn build_remote_stream_with_telemetry(
    conn: &NetworkConnection,
    remote_path: &str,
    db: &DatabaseManager,
    vault: &CredentialVault,
    retry_policy: StreamRetryPolicy,
    telemetry: Option<StreamTelemetry>,
) -> Result<Box<dyn SeekableStream>, String> {
    let spec = RemoteSpec::from_connection(conn, remote_path, db, vault)?;
    let opener = move || spec.open_once();
    let recoverable = if let Some(t) = telemetry {
        RecoverableStream::new_with_telemetry(opener, retry_policy, t)
            .map_err(|e| format!("remote stream open: {e}"))?
    } else {
        RecoverableStream::new(opener, retry_policy)
            .map_err(|e| format!("remote stream open: {e}"))?
    };
    Ok(Box::new(ReadAheadStream::new(recoverable)))
}

/// Builds a recoverable, read-ahead remote stream for the UI stream bridge.
pub fn build_remote_stream(
    conn: &NetworkConnection,
    remote_path: &str,
    db: &DatabaseManager,
    vault: &CredentialVault,
) -> Result<Box<dyn SeekableStream>, String> {
    build_remote_stream_with_telemetry(
        conn,
        remote_path,
        db,
        vault,
        StreamRetryPolicy::default(),
        None,
    )
}

fn default_port(protocol: &str) -> u16 {
    match protocol.to_ascii_lowercase().as_str() {
        "nfs" => 2049,
        "jellyfin" => 8096,
        "plex" => 32400,
        "smb" => 445,
        "sftp" => 22,
        "https" => 443,
        _ => 80,
    }
}

/// Resolves the SMB share from the connection's configured path.
pub fn smb_share(configured_path: &str) -> Option<String> {
    let value = configured_path.trim_matches('/');
    let share = value.split('/').next().unwrap_or("");
    (!share.is_empty()).then(|| share.to_string())
}

/// Returns the username suffix of a credential reference, if present.
pub fn connection_username(conn: &NetworkConnection) -> Option<String> {
    if conn.is_anonymous {
        return None;
    }
    conn.credential_ref
        .as_deref()
        .and_then(credential_ref_username)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smb_share_and_relative_path_are_derived_without_hardcoding() {
        assert_eq!(
            smb_share("/Media/Movies/movie.mkv"),
            Some("Media".to_string())
        );
        assert_eq!(smb_share("Share"), Some("Share".to_string()));
        assert_eq!(smb_share("/"), None);
    }

    #[test]
    fn credential_username_is_non_secret_and_parsed() {
        let conn = NetworkConnection {
            id: 1,
            name: "test".to_string(),
            protocol: "sftp".to_string(),
            host: "example".to_string(),
            port: 22,
            remote_path: "/".to_string(),
            auth_type: "basic".to_string(),
            credential_ref: Some("muvrix:sftp:example:22:alice".to_string()),
            is_anonymous: false,
            auto_connect: false,
            last_connected_at: 0,
        };
        assert_eq!(connection_username(&conn).as_deref(), Some("alice"));
    }

    #[test]
    fn remote_spec_uses_form_identity_and_strips_smb_share_prefix() {
        let db = DatabaseManager::open_in_memory().expect("in-memory db");
        let vault = CredentialVault::new();
        let conn = NetworkConnection {
            id: 1,
            name: "test".to_string(),
            protocol: "smb".to_string(),
            host: "nas".to_string(),
            port: 445,
            remote_path: "Private/Movies/movie.mkv".to_string(),
            auth_type: "basic".to_string(),
            credential_ref: Some("muvrix:smb:nas:445:bob".to_string()),
            is_anonymous: false,
            auto_connect: false,
            last_connected_at: 0,
        };
        let spec = RemoteSpec::from_connection(&conn, "/Private/Movies/movie.mkv", &db, &vault)
            .expect("spec");
        assert_eq!(spec.share, "Private");
        assert_eq!(spec.remote_path, "/Movies/movie.mkv");
        assert_eq!(spec.username.as_deref(), Some("bob"));
    }

    #[test]
    fn remote_spec_supports_nfs_jellyfin_and_plex_protocols() {
        let db = DatabaseManager::open_in_memory().expect("in-memory db");
        let vault = CredentialVault::new();

        // NFS Spec
        let nfs_conn = NetworkConnection {
            id: 10,
            name: "NFS Share".to_string(),
            protocol: "nfs".to_string(),
            host: "storage.home".to_string(),
            port: 2049,
            remote_path: "/srv/nfs/movies".to_string(),
            auth_type: "anonymous".to_string(),
            credential_ref: None,
            is_anonymous: true,
            auto_connect: false,
            last_connected_at: 0,
        };
        let nfs_spec =
            RemoteSpec::from_connection(&nfs_conn, "/srv/nfs/movies/clip.mkv", &db, &vault)
                .expect("nfs spec");
        assert_eq!(nfs_spec.protocol, "nfs");
        assert_eq!(nfs_spec.port, 2049);
        assert_eq!(nfs_spec.remote_path, "/srv/nfs/movies/clip.mkv");

        // Jellyfin Spec
        let jf_conn = NetworkConnection {
            id: 11,
            name: "Jellyfin Server".to_string(),
            protocol: "jellyfin".to_string(),
            host: "jf.local".to_string(),
            port: 8096,
            remote_path: "/items/12345".to_string(),
            auth_type: "token".to_string(),
            credential_ref: Some("muvrix:jellyfin:jf.local:8096:user".to_string()),
            is_anonymous: false,
            auto_connect: false,
            last_connected_at: 0,
        };
        let jf_spec =
            RemoteSpec::from_connection(&jf_conn, "/items/12345", &db, &vault).expect("jf spec");
        assert_eq!(jf_spec.protocol, "jellyfin");
        assert_eq!(jf_spec.port, 8096);

        // Plex Spec
        let plex_conn = NetworkConnection {
            id: 12,
            name: "Plex Server".to_string(),
            protocol: "plex".to_string(),
            host: "plex.local".to_string(),
            port: 32400,
            remote_path: "/library/parts/999/file.mp4".to_string(),
            auth_type: "token".to_string(),
            credential_ref: Some("muvrix:plex:plex.local:32400:owner".to_string()),
            is_anonymous: false,
            auto_connect: false,
            last_connected_at: 0,
        };
        let plex_spec =
            RemoteSpec::from_connection(&plex_conn, "/library/parts/999/file.mp4", &db, &vault)
                .expect("plex spec");
        assert_eq!(plex_spec.protocol, "plex");
        assert_eq!(plex_spec.port, 32400);
    }
}
