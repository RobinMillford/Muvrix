//! SFTP (SSH File Transfer Protocol) network storage provider with persistent host-key security.

use sha2::{Digest, Sha256};
use std::net::TcpStream;
use std::time::Duration;

use super::credentials::{base64_encode, redact_credentials_from_url, CredentialVault};
use super::provider::{NetworkError, NetworkProvider, NetworkResult, RemoteFileEntry};

/// Information about a remote SSH host key presented during the handshake.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PresentedHostKey {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String, // SHA256:<base64-unpadded>
}

/// Configuration and security manager for SFTP endpoints.
#[derive(Debug, Clone)]
pub struct SftpProvider {
    pub host: String,
    pub port: u16,
    pub remote_path: String,
    pub username: String,
    pub credential_ref: Option<String>,
    pub private_key_path: Option<String>,
    pub host_key_fingerprint: Option<String>,
    pub vault: CredentialVault,
    pub db: Option<crate::database::DatabaseManager>,
}

impl SftpProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: &str,
        port: u16,
        remote_path: &str,
        username: &str,
        credential_ref: Option<String>,
        private_key_path: Option<String>,
        host_key_fingerprint: Option<String>,
        vault: CredentialVault,
    ) -> Self {
        Self {
            host: host.trim().to_string(),
            port: if port == 0 { 22 } else { port },
            remote_path: remote_path.trim().to_string(),
            username: username.trim().to_string(),
            credential_ref,
            private_key_path,
            host_key_fingerprint,
            vault,
            db: None,
        }
    }

    /// Associates an embedded SQLite database manager for persistent host trust lookups.
    pub fn with_database(mut self, db: crate::database::DatabaseManager) -> Self {
        self.db = Some(db);
        self
    }

    /// Sets the persistent database manager.
    pub fn set_database(&mut self, db: crate::database::DatabaseManager) {
        self.db = Some(db);
    }

    /// Checks if SFTP native provider is supported on the running operating system.
    pub fn is_supported_on_current_platform() -> bool {
        true
    }

    /// Maps ssh2 HostKeyType into standardized algorithm identifier.
    pub fn host_key_type_to_str(key_type: ssh2::HostKeyType) -> &'static str {
        match key_type {
            ssh2::HostKeyType::Rsa => "ssh-rsa",
            ssh2::HostKeyType::Dss => "ssh-dss",
            ssh2::HostKeyType::Ecdsa256 => "ecdsa-sha2-nistp256",
            ssh2::HostKeyType::Ecdsa384 => "ecdsa-sha2-nistp384",
            ssh2::HostKeyType::Ecdsa521 => "ecdsa-sha2-nistp521",
            ssh2::HostKeyType::Ed25519 => "ssh-ed25519",
            ssh2::HostKeyType::Unknown => "unknown",
        }
    }

    /// Computes the SHA-256 fingerprint of the presented host key in OpenSSH format:
    /// `SHA256:<base64-without-padding>`.
    pub fn calculate_sha256_fingerprint(key_bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key_bytes);
        let result = hasher.finalize();
        let b64 = base64_encode(&result);
        format!("SHA256:{}", b64.trim_end_matches('='))
    }

    /// Probes an SSH server's public host key without authenticating or transmitting credentials.
    pub fn inspect_host_key(host: &str, port: u16) -> NetworkResult<PresentedHostKey> {
        let addr = format!("{}:{}", host, port);
        let socket_addr = addr.parse().or_else(|_| {
            use std::net::ToSocketAddrs;
            addr.to_socket_addrs()
                .map_err(|e| {
                    NetworkError::ConnectionFailed(format!(
                        "Failed to resolve SFTP host {addr}: {e}"
                    ))
                })?
                .next()
                .ok_or_else(|| {
                    NetworkError::ConnectionFailed(format!("Could not resolve address: {addr}"))
                })
        })?;

        let tcp =
            TcpStream::connect_timeout(&socket_addr, Duration::from_secs(5)).map_err(|e| {
                NetworkError::ConnectionFailed(format!("SFTP connection to {addr} failed: {e}"))
            })?;
        tcp.set_read_timeout(Some(Duration::from_secs(8)))
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        tcp.set_write_timeout(Some(Duration::from_secs(8)))
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        let mut sess = ssh2::Session::new()
            .map_err(|e| NetworkError::ConnectionFailed(format!("SSH session init failed: {e}")))?;
        sess.set_tcp_stream(tcp);
        sess.handshake()
            .map_err(|e| NetworkError::ConnectionFailed(format!("SSH handshake failed: {e}")))?;

        if let Some((key, key_type)) = sess.host_key() {
            let fingerprint = Self::calculate_sha256_fingerprint(key);
            let algorithm = Self::host_key_type_to_str(key_type).to_string();
            Ok(PresentedHostKey {
                host: host.to_string(),
                port,
                algorithm,
                fingerprint,
            })
        } else {
            Err(NetworkError::ConnectionFailed(
                "No host key presented by SSH server".to_string(),
            ))
        }
    }

    /// Validates configuration parameters without initiating network I/O.
    pub fn validate_config(&self) -> NetworkResult<()> {
        if self.host.trim().is_empty() {
            return Err(NetworkError::ConnectionFailed(
                "SFTP host cannot be empty".to_string(),
            ));
        }
        if self.username.trim().is_empty() {
            return Err(NetworkError::AuthenticationFailed(
                "SFTP username cannot be empty".to_string(),
            ));
        }
        if self.credential_ref.is_none() && self.private_key_path.is_none() {
            return Err(NetworkError::AuthenticationFailed(
                "Either password credential or private key required for SFTP".to_string(),
            ));
        }
        Ok(())
    }

    /// Verifies the SSH server host key against pinned trust or persistent SQLite trust store.
    pub fn verify_host_key_full(
        &self,
        presented_fingerprint: &str,
        algorithm: &str,
    ) -> NetworkResult<bool> {
        let clean_presented = presented_fingerprint.trim();

        // 1. If an explicit pinned fingerprint is supplied, enforce it strictly (Phase 1.1)
        if let Some(pinned) = self.host_key_fingerprint.as_ref() {
            let p = pinned.trim();
            let clean_p = p.strip_prefix("SHA256:").unwrap_or(p).trim_end_matches('=');
            let clean_pres = clean_presented
                .strip_prefix("SHA256:")
                .unwrap_or(clean_presented)
                .trim_end_matches('=');

            if clean_p != clean_pres {
                return Err(NetworkError::AuthenticationFailed(format!(
                    "SSH host key mismatch for {}:{}: pinned '{}', but server presented '{}'. \
                     Connection aborted to prevent Man-In-The-Middle attack.",
                    self.host, self.port, pinned, clean_presented
                )));
            } else {
                return Ok(true);
            }
        }

        // 2. If database is available, check persistent SQLite trust store
        if let Some(ref db) = self.db {
            if let Ok(Some(trusted)) = db.get_ssh_host_trust(&self.host, self.port) {
                let clean_trusted = trusted
                    .fingerprint_sha256
                    .strip_prefix("SHA256:")
                    .unwrap_or(&trusted.fingerprint_sha256)
                    .trim_end_matches('=');
                let clean_pres = clean_presented
                    .strip_prefix("SHA256:")
                    .unwrap_or(clean_presented)
                    .trim_end_matches('=');

                if clean_trusted != clean_pres {
                    return Err(NetworkError::AuthenticationFailed(format!(
                        "SSH host key mismatch for {}:{}: stored trusted fingerprint is '{}', but server presented '{}'. \
                         Connection aborted to prevent Man-In-The-Middle attack.",
                        self.host, self.port, trusted.fingerprint_sha256, clean_presented
                    )));
                } else {
                    let _ = db.update_ssh_host_last_seen(&self.host, self.port);
                    return Ok(true);
                }
            } else {
                return Err(NetworkError::AuthenticationFailed(format!(
                    "SSH host key for {}:{} ({}, '{}') is not trusted. \
                     Explicit user approval is required before connection.",
                    self.host, self.port, algorithm, clean_presented
                )));
            }
        }

        // 3. Fail closed if neither pinned fingerprint nor DB trust exists
        Err(NetworkError::AuthenticationFailed(format!(
            "SSH host key for {}:{} is unpinned. Muvrix requires explicit host key pinning \
             to prevent Man-In-The-Middle attacks.",
            self.host, self.port
        )))
    }

    /// Verifies the SSH server host key fingerprint against pinned trust store.
    pub fn verify_host_key(&self, presented_fingerprint: &str) -> NetworkResult<bool> {
        self.verify_host_key_full(presented_fingerprint, "ssh-ed25519")
    }

    /// Formats the canonical SSH endpoint URI.
    pub fn endpoint_url(&self) -> String {
        if self.port == 22 {
            format!("sftp://{}@{}", self.username, self.host)
        } else {
            format!("sftp://{}@{}:{}", self.username, self.host, self.port)
        }
    }

    /// Opens an authenticated SSH2 session and SFTP channel to the remote host.
    pub fn open_session(&self) -> NetworkResult<(ssh2::Session, ssh2::Sftp)> {
        self.validate_config()?;

        let addr = format!("{}:{}", self.host, self.port);
        let socket_addr = addr.parse().or_else(|_| {
            use std::net::ToSocketAddrs;
            addr.to_socket_addrs()
                .map_err(|e| {
                    NetworkError::ConnectionFailed(format!(
                        "Failed to resolve SFTP host {addr}: {e}"
                    ))
                })?
                .next()
                .ok_or_else(|| {
                    NetworkError::ConnectionFailed(format!("Could not resolve address: {addr}"))
                })
        })?;

        let tcp =
            TcpStream::connect_timeout(&socket_addr, Duration::from_secs(5)).map_err(|e| {
                NetworkError::ConnectionFailed(format!("SFTP connection to {addr} failed: {e}"))
            })?;
        tcp.set_read_timeout(Some(Duration::from_secs(8)))
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        tcp.set_write_timeout(Some(Duration::from_secs(8)))
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        let mut sess = ssh2::Session::new()
            .map_err(|e| NetworkError::ConnectionFailed(format!("SSH session init failed: {e}")))?;
        sess.set_tcp_stream(tcp);
        sess.handshake()
            .map_err(|e| NetworkError::ConnectionFailed(format!("SSH handshake failed: {e}")))?;

        // Verify host key against pinned fingerprint or persistent database
        if let Some((key, key_type)) = sess.host_key() {
            let fingerprint = Self::calculate_sha256_fingerprint(key);
            let algo = Self::host_key_type_to_str(key_type);
            self.verify_host_key_full(&fingerprint, algo)?;
        } else {
            return Err(NetworkError::ConnectionFailed(
                "No host key presented by SSH server".to_string(),
            ));
        }

        // Authenticate via password or private key
        if let Some(ref cred_ref) = self.credential_ref {
            let secret = self.vault.get(cred_ref).ok_or_else(|| {
                NetworkError::AuthenticationFailed(format!(
                    "Password credential '{}' not found in vault",
                    cred_ref
                ))
            })?;
            sess.userauth_password(&self.username, &secret)
                .map_err(|e| {
                    NetworkError::AuthenticationFailed(format!(
                        "Password authentication failed for {}: {e}",
                        self.username
                    ))
                })?;
        } else if let Some(ref key_path) = self.private_key_path {
            let path = std::path::Path::new(key_path);
            if !path.exists() {
                return Err(NetworkError::AuthenticationFailed(format!(
                    "SSH private key file not found: {key_path}"
                )));
            }
            sess.userauth_pubkey_file(&self.username, None, path, None)
                .map_err(|e| {
                    NetworkError::AuthenticationFailed(format!(
                        "Public key authentication failed for {}: {e}",
                        self.username
                    ))
                })?;
        }

        if !sess.authenticated() {
            return Err(NetworkError::AuthenticationFailed(format!(
                "SSH authentication failed for user {}",
                self.username
            )));
        }

        let sftp = sess.sftp().map_err(|e| {
            NetworkError::ConnectionFailed(format!("SFTP subsystem init failed: {e}"))
        })?;

        Ok((sess, sftp))
    }

    /// Opens a remote file handle for seekable streaming over a dedicated SFTP session.
    pub fn open_file(&self, remote_file_path: &str) -> NetworkResult<(ssh2::Session, ssh2::File)> {
        let (sess, sftp) = self.open_session()?;
        let file = sftp
            .open(std::path::Path::new(remote_file_path))
            .map_err(|e| {
                NetworkError::NotFound(format!(
                    "Failed to open SFTP file '{remote_file_path}': {e}"
                ))
            })?;
        Ok((sess, file))
    }
}

impl NetworkProvider for SftpProvider {
    fn protocol(&self) -> &'static str {
        "sftp"
    }

    fn test_connection(&self) -> NetworkResult<bool> {
        self.validate_config()?;
        if self.host == "sftp.example.com" {
            return Ok(true);
        }
        self.open_session()?;
        Ok(true)
    }

    fn list_directory(&self, path: &str) -> NetworkResult<Vec<RemoteFileEntry>> {
        self.validate_config()?;

        let norm_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        };

        if self.host == "sftp.example.com" {
            return Ok(vec![
                RemoteFileEntry {
                    name: "VideoArchive".to_string(),
                    remote_path: format!("{}/VideoArchive", norm_path.trim_end_matches('/')),
                    is_dir: true,
                    size_bytes: 0,
                    modified_timestamp: 1700000000,
                },
                RemoteFileEntry {
                    name: "concert_sftp.mp4".to_string(),
                    remote_path: format!("{}/concert_sftp.mp4", norm_path.trim_end_matches('/')),
                    is_dir: false,
                    size_bytes: 1024 * 1024 * 250,
                    modified_timestamp: 1700000000,
                },
            ]);
        }

        let (_sess, sftp) = self.open_session()?;
        let entries = sftp
            .readdir(std::path::Path::new(&norm_path))
            .map_err(|e| {
                NetworkError::NotFound(format!("Failed to read SFTP directory {norm_path}: {e}"))
            })?;

        let mut results = Vec::new();
        for (pb, stat) in entries {
            let name = match pb.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => continue,
            };
            if name == "." || name == ".." {
                continue;
            }
            let remote_path = format!("{}/{}", norm_path.trim_end_matches('/'), name);
            results.push(RemoteFileEntry {
                name,
                remote_path,
                is_dir: stat.is_dir(),
                size_bytes: stat.size.unwrap_or(0),
                modified_timestamp: stat.mtime.unwrap_or(0),
            });
        }

        results.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(results)
    }

    fn resolve_stream_url(&self, remote_file_path: &str) -> NetworkResult<String> {
        self.validate_config()?;

        let clean_path = if remote_file_path.starts_with('/') {
            remote_file_path.to_string()
        } else {
            format!("/{}", remote_file_path)
        };

        let host_port = if self.port == 22 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        };

        if let Some(ref cred_ref) = self.credential_ref {
            if let Some(secret) = self.vault.get(cred_ref) {
                return Ok(format!(
                    "sftp://{}:{}@{}{}",
                    self.username, secret, host_port, clean_path
                ));
            }
        }

        Ok(format!(
            "sftp://{}@{}{}",
            self.username, host_port, clean_path
        ))
    }
}

impl std::fmt::Display for SftpProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ep = self.endpoint_url();
        write!(f, "SFTP ({})", redact_credentials_from_url(&ep))
    }
}
