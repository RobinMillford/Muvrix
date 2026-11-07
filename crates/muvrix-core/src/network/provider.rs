//! Abstract network provider interface and remote filesystem structures.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Failed to establish network connection: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Remote resource not found: {0}")]
    NotFound(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type NetworkResult<T> = std::result::Result<T, NetworkError>;

/// Representation of a remote filesystem item (file or directory).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteFileEntry {
    pub name: String,
    pub remote_path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified_timestamp: u64,
}

/// Abstract provider interface for remote network media access.
pub trait NetworkProvider: Send + Sync {
    /// Returns the lowercase protocol name (e.g. "webdav", "smb", "sftp", "ftp").
    fn protocol(&self) -> &'static str;

    /// Tests reachability and credentials against the remote server.
    fn test_connection(&self) -> NetworkResult<bool>;

    /// Lists files and directories at `path`.
    fn list_directory(&self, path: &str) -> NetworkResult<Vec<RemoteFileEntry>>;

    /// Resolves a streaming URL or endpoint compatible with libmpv playback.
    fn resolve_stream_url(&self, remote_file_path: &str) -> NetworkResult<String>;
}

/// Mock network provider for deterministic testing without external servers.
#[derive(Debug, Clone)]
pub struct MockNetworkProvider {
    pub protocol_name: &'static str,
    pub should_succeed: bool,
    pub entries: Vec<RemoteFileEntry>,
}

impl MockNetworkProvider {
    pub fn new(protocol_name: &'static str) -> Self {
        Self {
            protocol_name,
            should_succeed: true,
            entries: vec![
                RemoteFileEntry {
                    name: "Movies".to_string(),
                    remote_path: "/Movies".to_string(),
                    is_dir: true,
                    size_bytes: 0,
                    modified_timestamp: 1700000000,
                },
                RemoteFileEntry {
                    name: "sample.mp4".to_string(),
                    remote_path: "/sample.mp4".to_string(),
                    is_dir: false,
                    size_bytes: 1024 * 1024 * 10,
                    modified_timestamp: 1700000000,
                },
            ],
        }
    }
}

impl NetworkProvider for MockNetworkProvider {
    fn protocol(&self) -> &'static str {
        self.protocol_name
    }

    fn test_connection(&self) -> NetworkResult<bool> {
        if self.should_succeed {
            Ok(true)
        } else {
            Err(NetworkError::ConnectionFailed(
                "Host unreachable".to_string(),
            ))
        }
    }

    fn list_directory(&self, _path: &str) -> NetworkResult<Vec<RemoteFileEntry>> {
        if self.should_succeed {
            Ok(self.entries.clone())
        } else {
            Err(NetworkError::NotFound("Directory not found".to_string()))
        }
    }

    fn resolve_stream_url(&self, remote_file_path: &str) -> NetworkResult<String> {
        if self.should_succeed {
            Ok(format!("http://127.0.0.1:8080{}", remote_file_path))
        } else {
            Err(NetworkError::NotFound("File not found".to_string()))
        }
    }
}
