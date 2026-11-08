//! SMB2/SMB3 network storage provider for Windows and LAN network shares.

use super::credentials::{redact_credentials_from_url, CredentialVault};
use super::provider::{NetworkError, NetworkProvider, NetworkResult, RemoteFileEntry};

/// Configuration and client interface for SMB2/SMB3 shares.
#[derive(Debug, Clone)]
pub struct SmbProvider {
    pub host: String,
    pub port: u16,
    pub share: String,
    pub remote_path: String,
    pub username: Option<String>,
    pub domain: Option<String>,
    pub credential_ref: Option<String>,
    pub is_guest: bool,
    pub vault: CredentialVault,
}

impl SmbProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: &str,
        port: u16,
        share: &str,
        remote_path: &str,
        username: Option<String>,
        domain: Option<String>,
        credential_ref: Option<String>,
        is_guest: bool,
        vault: CredentialVault,
    ) -> Self {
        Self {
            host: host.trim().to_string(),
            port: if port == 0 { 445 } else { port },
            share: share.trim_matches('/').to_string(),
            remote_path: remote_path.trim().to_string(),
            username,
            domain,
            credential_ref,
            is_guest,
            vault,
        }
    }

    /// Formats the UNC-style canonical resource identifier (e.g. `smb://nas.local/Media`).
    pub fn unc_path(&self) -> String {
        if self.port == 445 {
            format!("smb://{}/{}", self.host, self.share)
        } else {
            format!("smb://{}:{}/{}", self.host, self.port, self.share)
        }
    }
}

impl NetworkProvider for SmbProvider {
    fn protocol(&self) -> &'static str {
        "smb"
    }

    fn test_connection(&self) -> NetworkResult<bool> {
        if self.host.is_empty() {
            return Err(NetworkError::ConnectionFailed(
                "SMB host cannot be empty".to_string(),
            ));
        }
        if self.share.is_empty() {
            return Err(NetworkError::ConnectionFailed(
                "SMB share name cannot be empty".to_string(),
            ));
        }
        if !self.is_guest && self.username.is_none() {
            return Err(NetworkError::AuthenticationFailed(
                "Username required for non-guest SMB authentication".to_string(),
            ));
        }
        Ok(true)
    }

    fn list_directory(&self, path: &str) -> NetworkResult<Vec<RemoteFileEntry>> {
        self.test_connection()?;

        let norm_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        };

        // Standard directory entries simulation for SMB shares
        Ok(vec![
            RemoteFileEntry {
                name: "Movies".to_string(),
                remote_path: format!("{}/Movies", norm_path.trim_end_matches('/')),
                is_dir: true,
                size_bytes: 0,
                modified_timestamp: 1700000000,
            },
            RemoteFileEntry {
                name: "film_hd.mkv".to_string(),
                remote_path: format!("{}/film_hd.mkv", norm_path.trim_end_matches('/')),
                is_dir: false,
                size_bytes: 1024 * 1024 * 700,
                modified_timestamp: 1700000000,
            },
        ])
    }

    fn resolve_stream_url(&self, remote_file_path: &str) -> NetworkResult<String> {
        self.test_connection()?;

        let clean_path = remote_file_path.trim_start_matches('/');
        let host_port = if self.port == 445 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        };

        if self.is_guest {
            Ok(format!("smb://{}/{}/{}", host_port, self.share, clean_path))
        } else if let (Some(ref user), Some(ref cred_ref)) = (&self.username, &self.credential_ref)
        {
            if let Some(secret) = self.vault.get(cred_ref) {
                let user_part = if let Some(ref dom) = self.domain {
                    format!("{};{}", dom, user)
                } else {
                    user.clone()
                };
                return Ok(format!(
                    "smb://{}:{}@{}/{}/{}",
                    user_part, secret, host_port, self.share, clean_path
                ));
            }
            Ok(format!(
                "smb://{}@{}/{}/{}",
                user, host_port, self.share, clean_path
            ))
        } else {
            Ok(format!("smb://{}/{}/{}", host_port, self.share, clean_path))
        }
    }
}

impl std::fmt::Display for SmbProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let unc = self.unc_path();
        write!(f, "SMB ({})", redact_credentials_from_url(&unc))
    }
}
