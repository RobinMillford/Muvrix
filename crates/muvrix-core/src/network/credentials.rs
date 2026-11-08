//! Secure credential management, in-memory isolation, and URL secret redaction.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Service name identifier for Muvrix in the operating system's native credential store.
pub const MUVRIX_KEYRING_SERVICE: &str = "io.muvrix.desktop";

/// Thread-safe in-memory vault for network credentials (passwords, auth tokens)
/// backed by the operating system native keyring (Secret Service, Windows Credential Manager, macOS Keychain)
/// with transparent, infallible in-memory fallback.
/// Prevents plaintext credential leakage into SQLite, history files, or console logs.
#[derive(Debug, Default, Clone)]
pub struct CredentialVault {
    secrets: Arc<Mutex<HashMap<String, String>>>,
}

impl CredentialVault {
    pub fn new() -> Self {
        Self {
            secrets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Stores a secret credential for a given connection reference UUID.
    /// Attempts persistence in the native OS keyring, while maintaining an in-memory
    /// cache for fast lookups and headless/unsupported fallback.
    pub fn store(&self, credential_ref: &str, secret: &str) {
        if let Ok(entry) = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, credential_ref) {
            let _ = entry.set_password(secret);
        }
        if let Ok(mut lock) = self.secrets.lock() {
            lock.insert(credential_ref.to_string(), secret.to_string());
        }
    }

    /// Retrieves a secret credential by reference UUID.
    /// Fast in-memory cache is queried first; falls back to querying the OS keyring.
    pub fn get(&self, credential_ref: &str) -> Option<String> {
        if let Ok(lock) = self.secrets.lock() {
            if let Some(secret) = lock.get(credential_ref) {
                return Some(secret.clone());
            }
        }

        // Try OS keyring
        if let Ok(entry) = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, credential_ref) {
            if let Ok(secret) = entry.get_password() {
                if let Ok(mut lock) = self.secrets.lock() {
                    lock.insert(credential_ref.to_string(), secret.clone());
                }
                return Some(secret);
            }
        }

        None
    }

    /// Removes a secret credential from both the in-memory vault and native OS keyring.
    pub fn remove(&self, credential_ref: &str) {
        if let Ok(entry) = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, credential_ref) {
            let _ = entry.delete_credential();
        }
        if let Ok(mut lock) = self.secrets.lock() {
            lock.remove(credential_ref);
        }
    }

    /// Clears all credentials in the in-memory vault.
    pub fn clear(&self) {
        if let Ok(mut lock) = self.secrets.lock() {
            lock.clear();
        }
    }
}

/// Helper encoding byte buffers into standard RFC 4648 Base64 strings without external dependencies.
#[allow(clippy::chunks_exact_to_as_chunks)]
pub fn base64_encode(input: &[u8]) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut chunks = input.chunks_exact(3);
    for chunk in &mut chunks {
        let b0 = chunk[0] as usize;
        let b1 = chunk[1] as usize;
        let b2 = chunk[2] as usize;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARSET[(triple >> 18) & 0x3F] as char);
        out.push(CHARSET[(triple >> 12) & 0x3F] as char);
        out.push(CHARSET[(triple >> 6) & 0x3F] as char);
        out.push(CHARSET[triple & 0x3F] as char);
    }
    let rem = chunks.remainder();
    if rem.len() == 1 {
        let triple = (rem[0] as usize) << 16;
        out.push(CHARSET[(triple >> 18) & 0x3F] as char);
        out.push(CHARSET[(triple >> 12) & 0x3F] as char);
        out.push('=');
        out.push('=');
    } else if rem.len() == 2 {
        let triple = ((rem[0] as usize) << 16) | ((rem[1] as usize) << 8);
        out.push(CHARSET[(triple >> 18) & 0x3F] as char);
        out.push(CHARSET[(triple >> 12) & 0x3F] as char);
        out.push(CHARSET[(triple >> 6) & 0x3F] as char);
        out.push('=');
    }
    out
}

/// Redacts sensitive username and password credentials from a URL for safe logging and error reporting.
pub fn redact_credentials_from_url(url: &str) -> String {
    if let Some(scheme_idx) = url.find("://") {
        let scheme = &url[..scheme_idx + 3];
        let rest = &url[scheme_idx + 3..];
        if let Some(at_idx) = rest.find('@') {
            let host_and_path = &rest[at_idx + 1..];
            return format!("{}[redacted]@{}", scheme, host_and_path);
        }
    }
    url.to_string()
}
