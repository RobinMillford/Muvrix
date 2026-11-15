//! Secure credential management, in-memory isolation, and URL secret redaction.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zeroize::{Zeroize, Zeroizing};

/// Service name identifier for Muvrix in the operating system's native credential store.
pub const MUVRIX_KEYRING_SERVICE: &str = "io.muvrix.desktop";

/// Granular error types for native credential store interactions.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum CredentialError {
    #[error("Keyring entry creation failed: {0}")]
    Entry(String),
    #[error("Native credential storage failed: {0}")]
    NativeStore(String),
    #[error("Native credential retrieval failed: {0}")]
    NativeRetrieve(String),
    #[error("Native credential deletion failed: {0}")]
    NativeDelete(String),
    #[error("Credential not found: {0}")]
    NotFound(String),
    #[error("Keyring is locked or unavailable: {0}")]
    KeyringLockedOrUnavailable(String),
}

fn map_keyring_error(err: keyring::Error, op: &str) -> CredentialError {
    match err {
        keyring::Error::NoEntry => CredentialError::NotFound("Credential not found".to_string()),
        keyring::Error::PlatformFailure(ref e) => {
            let msg = e.to_string();
            let lower = msg.to_lowercase();
            if lower.contains("locked")
                || lower.contains("denied")
                || lower.contains("not available")
                || lower.contains("cannot connect")
                || lower.contains("no such interface")
                || lower.contains("not supported")
            {
                CredentialError::KeyringLockedOrUnavailable(msg)
            } else {
                match op {
                    "storage" => CredentialError::NativeStore(msg),
                    "retrieval" => CredentialError::NativeRetrieve(msg),
                    "deletion" => CredentialError::NativeDelete(msg),
                    _ => CredentialError::NativeStore(msg),
                }
            }
        }
        other => match op {
            "storage" => CredentialError::NativeStore(other.to_string()),
            "retrieval" => CredentialError::NativeRetrieve(other.to_string()),
            "deletion" => CredentialError::NativeDelete(other.to_string()),
            _ => CredentialError::NativeStore(other.to_string()),
        },
    }
}

/// Thread-safe in-memory vault for network credentials (passwords, auth tokens)
/// backed by the operating system native keyring (Secret Service, Windows Credential Manager, macOS Keychain)
/// with transparent, infallible in-memory fallback.
/// Prevents plaintext credential leakage into SQLite, history files, or console logs.
/// All in-memory secret values are protected with memory zeroization on drop/removal.
#[derive(Debug, Default, Clone)]
pub struct CredentialVault {
    secrets: Arc<Mutex<HashMap<String, Zeroizing<String>>>>,
}

impl CredentialVault {
    pub fn new() -> Self {
        Self {
            secrets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Stores a secret credential for a given connection reference UUID.
    /// Attempts persistence in the native OS keyring, while maintaining an in-memory
    /// zeroized cache for fast lookups and headless/unsupported fallback.
    pub fn store(&self, credential_ref: &str, secret: &str) {
        if let Ok(entry) = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, credential_ref) {
            let _ = entry.set_password(secret);
        }
        if let Ok(mut lock) = self.secrets.lock() {
            lock.insert(
                credential_ref.to_string(),
                Zeroizing::new(secret.to_string()),
            );
        }
    }

    /// Retrieves a secret credential by reference UUID.
    /// Fast in-memory cache is queried first; falls back to querying the OS keyring.
    pub fn get(&self, credential_ref: &str) -> Option<String> {
        if let Ok(lock) = self.secrets.lock() {
            if let Some(secret) = lock.get(credential_ref) {
                return Some((**secret).clone());
            }
        }

        // Try OS keyring
        if let Ok(entry) = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, credential_ref) {
            if let Ok(secret) = entry.get_password() {
                if let Ok(mut lock) = self.secrets.lock() {
                    lock.insert(credential_ref.to_string(), Zeroizing::new(secret.clone()));
                }
                return Some(secret);
            }
        }

        None
    }

    /// Removes a secret credential from both the in-memory vault and native OS keyring.
    /// Securely zeroizes the removed in-memory buffer before deallocation.
    pub fn remove(&self, credential_ref: &str) {
        if let Ok(entry) = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, credential_ref) {
            let _ = entry.delete_credential();
        }
        if let Ok(mut lock) = self.secrets.lock() {
            if let Some(mut val) = lock.remove(credential_ref) {
                val.zeroize();
            }
        }
    }

    /// Clears all credentials in the in-memory vault, zeroizing each secret buffer
    /// and removing the corresponding entries from the native OS keyring.
    pub fn clear(&self) {
        if let Ok(mut lock) = self.secrets.lock() {
            for (key, mut val) in lock.drain() {
                if let Ok(entry) = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, &key) {
                    let _ = entry.delete_credential();
                }
                val.zeroize();
            }
        }
    }

    /// Explicitly persists a secret to the native platform credential store (Secret Service,
    /// Windows Credential Manager, or Apple Keychain).
    /// Returns `Ok(())` on successful native persistence, or a typed `CredentialError`
    /// (e.g. if the native keyring is locked, missing a daemon, or permissions are denied).
    pub fn store_native(&self, credential_ref: &str, secret: &str) -> Result<(), CredentialError> {
        let entry = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, credential_ref)
            .map_err(|e| CredentialError::Entry(e.to_string()))?;
        entry
            .set_password(secret)
            .map_err(|e| map_keyring_error(e, "storage"))?;

        if let Ok(mut lock) = self.secrets.lock() {
            lock.insert(
                credential_ref.to_string(),
                Zeroizing::new(secret.to_string()),
            );
        }
        Ok(())
    }

    /// Directly retrieves a secret from the native platform credential store without
    /// reading from the in-memory cache.
    pub fn get_native(&self, credential_ref: &str) -> Result<String, CredentialError> {
        let entry = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, credential_ref)
            .map_err(|e| CredentialError::Entry(e.to_string()))?;
        entry
            .get_password()
            .map_err(|e| map_keyring_error(e, "retrieval"))
    }

    /// Explicitly deletes a secret from the native platform credential store.
    pub fn remove_native(&self, credential_ref: &str) -> Result<(), CredentialError> {
        let entry = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, credential_ref)
            .map_err(|e| CredentialError::Entry(e.to_string()))?;
        entry
            .delete_credential()
            .map_err(|e| map_keyring_error(e, "deletion"))?;

        if let Ok(mut lock) = self.secrets.lock() {
            if let Some(mut val) = lock.remove(credential_ref) {
                val.zeroize();
            }
        }
        Ok(())
    }

    /// Stores a secret strictly in memory for session-only / ephemeral use (e.g. testing connection endpoints).
    /// Does NOT persist the secret to the native OS keyring.
    pub fn store_ephemeral(&self, credential_ref: &str, secret: &str) {
        if let Ok(mut lock) = self.secrets.lock() {
            lock.insert(
                credential_ref.to_string(),
                Zeroizing::new(secret.to_string()),
            );
        }
    }

    /// Retrieves an ephemeral secret stored exclusively in memory.
    pub fn get_ephemeral(&self, credential_ref: &str) -> Option<String> {
        if let Ok(lock) = self.secrets.lock() {
            if let Some(secret) = lock.get(credential_ref) {
                return Some((**secret).clone());
            }
        }
        None
    }

    /// Removes an ephemeral secret from memory, securely zeroizing its buffer.
    pub fn remove_ephemeral(&self, credential_ref: &str) {
        if let Ok(mut lock) = self.secrets.lock() {
            if let Some(mut val) = lock.remove(credential_ref) {
                val.zeroize();
            }
        }
    }

    /// Probes if the native platform credential store is accessible in the current session.
    pub fn is_native_available() -> bool {
        let probe_key = format!("muvrix_probe_{}", std::process::id());
        if let Ok(entry) = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, &probe_key) {
            match entry.get_password() {
                Ok(_) => true,
                Err(keyring::Error::NoEntry) => true,
                Err(keyring::Error::PlatformFailure(ref e)) => {
                    let msg = e.to_string().to_lowercase();
                    !msg.contains("cannot connect")
                        && !msg.contains("not available")
                        && !msg.contains("no such interface")
                }
                Err(_) => false,
            }
        } else {
            false
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
        let authority_end = rest.find('/').unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        let path = &rest[authority_end..];
        if let Some(at_idx) = authority.rfind('@') {
            let host = &authority[at_idx + 1..];
            return format!("{}[redacted]@{}{}", scheme, host, path);
        }
    }
    url.to_string()
}

/// Generates a deterministic, namespaced credential reference identifier based on non-secret connection identity.
/// Strictly non-secret: contains no passwords, private keys, or secret tokens.
pub fn generate_credential_ref(protocol: &str, host: &str, port: u16, username: &str) -> String {
    let p = protocol.trim().to_lowercase();
    let h = host.trim().to_lowercase();
    let u = username.trim().to_lowercase();
    format!("muvrix:{p}:{h}:{port}:{u}")
}

/// Extracts the non-secret username suffix from a credential reference.
///
/// References generated by [`generate_credential_ref`] contain no password.
pub fn credential_ref_username(reference: &str) -> Option<&str> {
    reference
        .rsplit_once(':')
        .map(|(_, username)| username)
        .filter(|username| !username.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_store_get_remove_lifecycle() {
        let vault = CredentialVault::new();
        vault.store("test_ref_1", "super_secret_value_123");
        assert_eq!(
            vault.get("test_ref_1"),
            Some("super_secret_value_123".to_string())
        );

        vault.remove("test_ref_1");
        assert_eq!(vault.get("test_ref_1"), None);
    }

    #[test]
    fn test_vault_clear() {
        let vault = CredentialVault::new();
        vault.store("ref_a", "secret_a");
        vault.store("ref_b", "secret_b");
        assert!(vault.get("ref_a").is_some());
        assert!(vault.get("ref_b").is_some());

        vault.clear();
        assert_eq!(vault.get("ref_a"), None);
        assert_eq!(vault.get("ref_b"), None);
    }

    #[test]
    fn test_zeroizing_memory_cleanup() {
        use zeroize::Zeroize;
        let mut secret = zeroize::Zeroizing::new("temporary_sensitive_token".to_string());
        assert_eq!(&*secret, "temporary_sensitive_token");
        secret.zeroize();
        assert_eq!(&*secret, "");
    }

    #[test]
    fn test_credential_error_mapping_and_display() {
        let err_locked = map_keyring_error(
            keyring::Error::PlatformFailure("The collection is locked".to_string().into()),
            "storage",
        );
        assert!(matches!(
            err_locked,
            CredentialError::KeyringLockedOrUnavailable(_)
        ));
        assert!(err_locked.to_string().contains("locked or unavailable"));

        let err_not_found = map_keyring_error(keyring::Error::NoEntry, "retrieval");
        assert!(matches!(err_not_found, CredentialError::NotFound(_)));

        let err_general = map_keyring_error(
            keyring::Error::PlatformFailure("Disk I/O error".to_string().into()),
            "storage",
        );
        assert!(matches!(err_general, CredentialError::NativeStore(_)));
    }

    #[test]
    fn test_ephemeral_storage_lifecycle() {
        let vault = CredentialVault::new();
        vault.store_ephemeral("eph_1", "ephemeral_secret_abc");
        assert_eq!(
            vault.get_ephemeral("eph_1"),
            Some("ephemeral_secret_abc".to_string())
        );
        assert_eq!(vault.get("eph_1"), Some("ephemeral_secret_abc".to_string()));

        vault.remove_ephemeral("eph_1");
        assert_eq!(vault.get_ephemeral("eph_1"), None);
        assert_eq!(vault.get("eph_1"), None);
    }

    #[test]
    fn test_generate_credential_ref_determinism_and_namespace() {
        let ref1 = generate_credential_ref("SFTP", "Example.COM", 22, "Alice");
        let ref2 = generate_credential_ref("sftp", "example.com", 22, "alice");
        assert_eq!(ref1, "muvrix:sftp:example.com:22:alice");
        assert_eq!(
            ref1, ref2,
            "Credential reference must be normalized and deterministic"
        );

        let ref3 = generate_credential_ref("smb", "example.com", 445, "alice");
        assert_ne!(ref1, ref3, "Different protocols must not collide");

        let ref4 = generate_credential_ref("sftp", "example.com", 2222, "alice");
        assert_ne!(ref1, ref4, "Different ports must not collide");

        let ref5 = generate_credential_ref("sftp", "example.com", 22, "bob");
        assert_ne!(ref1, ref5, "Different users must not collide");
    }

    #[test]
    fn test_native_availability_probe() {
        // Probe should execute without panicking
        let _ = CredentialVault::is_native_available();
    }
}
