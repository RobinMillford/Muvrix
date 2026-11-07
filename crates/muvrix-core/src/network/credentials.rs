//! Secure credential management, in-memory isolation, and URL secret redaction.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Thread-safe in-memory vault for network credentials (passwords, auth tokens).
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
    pub fn store(&self, credential_ref: &str, secret: &str) {
        if let Ok(mut lock) = self.secrets.lock() {
            lock.insert(credential_ref.to_string(), secret.to_string());
        }
    }

    /// Retrieves a secret credential by reference UUID.
    pub fn get(&self, credential_ref: &str) -> Option<String> {
        self.secrets
            .lock()
            .ok()
            .and_then(|lock| lock.get(credential_ref).cloned())
    }

    /// Removes a secret credential from the vault.
    pub fn remove(&self, credential_ref: &str) {
        if let Ok(mut lock) = self.secrets.lock() {
            lock.remove(credential_ref);
        }
    }

    /// Clears all credentials in the vault.
    pub fn clear(&self) {
        if let Ok(mut lock) = self.secrets.lock() {
            lock.clear();
        }
    }
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
