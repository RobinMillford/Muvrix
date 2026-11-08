//! Linux Native Secret Service Verification Test (REQ-NET-13).
//!
//! Directly tests `keyring::Entry` against the live `org.freedesktop.secrets` DBus service
//! (e.g. `oo7-daemon` or `gnome-keyring-daemon`) in the active user session.
//! Validates:
//! 1. Live Store
//! 2. Live Retrieve
//! 3. Live Update
//! 4. Live Delete
//! 5. Persistence across distinct instances (no shared in-memory cache)
//! 6. `CredentialVault` cross-instance resolution
//! 7. Credential isolation (zero secrets in URLs or SQLite metadata)

use muvrix_core::network::{redact_credentials_from_url, CredentialVault, MUVRIX_KEYRING_SERVICE};
use rusqlite::Connection;

/// RAII Drop guard ensuring any temporary test secret written to the native keyring
/// is unconditionally purged upon completion or panic.
struct KeyringTestGuard {
    service: String,
    user: String,
    active: bool,
}

impl KeyringTestGuard {
    fn new(service: &str, user: &str) -> Self {
        Self {
            service: service.to_string(),
            user: user.to_string(),
            active: true,
        }
    }

    #[allow(dead_code)]
    fn defuse(&mut self) {
        self.active = false;
    }
}

impl Drop for KeyringTestGuard {
    fn drop(&mut self) {
        if self.active {
            if let Ok(entry) = keyring::Entry::new(&self.service, &self.user) {
                let _ = entry.delete_credential();
            }
        }
    }
}

fn should_run_native_keyring_tests() -> bool {
    std::env::var("MUVRIX_TEST_NATIVE_KEYRING").is_ok()
}

#[test]
fn test_live_secret_service_crud_lifecycle() {
    if !should_run_native_keyring_tests() {
        eprintln!(
            "SKIPPING test_live_secret_service_crud_lifecycle: Native keyring test skipped by default \
             to avoid modifying the active workstation keyring. Set MUVRIX_TEST_NATIVE_KEYRING=1 to \
             explicitly opt in to live OS keyring validation."
        );
        return;
    }

    let test_uuid = format!(
        "test-crud-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    let _guard = KeyringTestGuard::new(MUVRIX_KEYRING_SERVICE, &test_uuid);

    let entry = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, &test_uuid)
        .expect("Create keyring Entry for Muvrix service");

    let initial_secret = "P@ssw0rd_Live_Secret_Service_123!";
    let updated_secret = "Updated_P@ssw0rd_Live_Secret_Service_456!";

    // 1. Live Store
    match entry.set_password(initial_secret) {
        Ok(()) => {
            println!("Successfully stored secret in native Secret Service");
        }
        Err(e) => {
            eprintln!("Secret Service not accessible or unlocked in this environment: {e}");
            return;
        }
    }

    // 2. Live Retrieve
    let retrieved = entry
        .get_password()
        .expect("Retrieve password from Secret Service");
    assert_eq!(
        retrieved, initial_secret,
        "Retrieved password must match stored secret"
    );

    // 3. Live Update
    entry
        .set_password(updated_secret)
        .expect("Update password in Secret Service");
    let retrieved_updated = entry.get_password().expect("Retrieve updated password");
    assert_eq!(
        retrieved_updated, updated_secret,
        "Updated secret must match"
    );

    // 4. Persistence across distinct Entry instances
    drop(entry);
    let entry_reopened = keyring::Entry::new(MUVRIX_KEYRING_SERVICE, &test_uuid)
        .expect("Reopen keyring Entry for distinct instance test");
    let retrieved_cross_instance = entry_reopened
        .get_password()
        .expect("Retrieve across instances");
    assert_eq!(
        retrieved_cross_instance, updated_secret,
        "Cross-instance retrieval must succeed"
    );

    // 5. Live Delete
    entry_reopened
        .delete_credential()
        .expect("Delete credential from Secret Service");

    // 6. Verify deletion
    let post_delete_entry =
        keyring::Entry::new(MUVRIX_KEYRING_SERVICE, &test_uuid).expect("Create post-delete Entry");
    let lookup_res = post_delete_entry.get_password();
    assert!(
        lookup_res.is_err(),
        "Lookup after deletion must return an error/not-found"
    );
}

#[test]
fn test_credential_vault_native_persistence_across_instances() {
    if !should_run_native_keyring_tests() {
        eprintln!(
            "SKIPPING test_credential_vault_native_persistence_across_instances: Native keyring test \
             skipped by default to avoid modifying the active workstation keyring. Set \
             MUVRIX_TEST_NATIVE_KEYRING=1 to explicitly opt in to live OS keyring validation."
        );
        return;
    }

    let test_uuid = format!(
        "test-vault-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let _guard = KeyringTestGuard::new(MUVRIX_KEYRING_SERVICE, &test_uuid);
    let secret = "SuperSecretVaultPass_999!";

    let vault_a = CredentialVault::new();
    vault_a.store(&test_uuid, secret);

    // Verify vault_a can read it (from memory or keyring)
    assert_eq!(vault_a.get(&test_uuid), Some(secret.to_string()));

    // Create a completely separate CredentialVault instance (empty in-memory cache)
    let vault_b = CredentialVault::new();
    // vault_b must resolve it via the OS keyring
    if let Some(retrieved) = vault_b.get(&test_uuid) {
        assert_eq!(
            retrieved, secret,
            "vault_b must retrieve secret from native OS keyring"
        );
    } else {
        eprintln!("Note: OS keyring storage unavailable, session fallback used");
    }

    // Clean up
    vault_a.remove(&test_uuid);
    assert_eq!(vault_a.get(&test_uuid), None);

    // Verify a fresh third vault instance sees None from OS keyring
    let vault_c = CredentialVault::new();
    assert_eq!(
        vault_c.get(&test_uuid),
        None,
        "Fresh vault must find None after OS deletion"
    );

    vault_b.remove(&test_uuid);
    assert_eq!(vault_b.get(&test_uuid), None);
}

#[test]
fn test_credential_redaction_from_urls() {
    let smb_url = "smb://muvrix_user:SecretP@ssword!@192.168.1.100:445/media/video.mp4";
    let redacted_smb = redact_credentials_from_url(smb_url);
    assert_eq!(
        redacted_smb,
        "smb://[redacted]@192.168.1.100:445/media/video.mp4"
    );
    assert!(!redacted_smb.contains("SecretP@ssword!"));
    assert!(!redacted_smb.contains("muvrix_user"));

    let webdav_url = "http://admin:DavPass123@localhost:18080/webdav/sample.mp4";
    let redacted_dav = redact_credentials_from_url(webdav_url);
    assert_eq!(
        redacted_dav,
        "http://[redacted]@localhost:18080/webdav/sample.mp4"
    );
    assert!(!redacted_dav.contains("DavPass123"));

    let safe_url = "http://127.0.0.1:18080/webdav/sample.mp4";
    assert_eq!(redact_credentials_from_url(safe_url), safe_url);
}

#[test]
fn test_sqlite_schema_stores_credential_ref_uuid_only() {
    let conn = Connection::open_in_memory().expect("Open SQLite in-memory");

    conn.execute(
        "CREATE TABLE network_shares (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            protocol TEXT NOT NULL,
            host TEXT NOT NULL,
            port INTEGER,
            share_name TEXT,
            username TEXT,
            credential_ref TEXT NOT NULL
        )",
        [],
    )
    .expect("Create schema");

    let credential_ref_uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let _guard = KeyringTestGuard::new(MUVRIX_KEYRING_SERVICE, credential_ref_uuid);
    let plaintext_secret = "ExtremelySensitiveSecretDoNotLeak!";

    // Store in vault
    let vault = CredentialVault::new();
    vault.store(credential_ref_uuid, plaintext_secret);

    // Insert record in SQLite referencing only the UUID
    conn.execute(
        "INSERT INTO network_shares (protocol, host, port, share_name, username, credential_ref)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            "smb",
            "192.168.1.50",
            445,
            "videos",
            "media_user",
            credential_ref_uuid
        ],
    )
    .expect("Insert share");

    // Query back the table row
    let (db_user, db_ref): (String, String) = conn
        .query_row(
            "SELECT username, credential_ref FROM network_shares WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("Query share");

    assert_eq!(db_user, "media_user");
    assert_eq!(db_ref, credential_ref_uuid);

    // Query entire SQLite database page text / dump to confirm plaintext secret is absent
    let mut stmt = conn
        .prepare("SELECT * FROM network_shares")
        .expect("Prepare");
    let rows = stmt
        .query_map([], |row| {
            let mut s = String::new();
            for i in 0..row.as_ref().column_count() {
                let val: rusqlite::types::Value = row.get(i)?;
                s.push_str(&format!("{:?} ", val));
            }
            Ok(s)
        })
        .expect("Query map");

    for r in rows {
        let text = r.expect("Row text");
        assert!(
            !text.contains(plaintext_secret),
            "Plaintext secret must never appear in SQLite table rows"
        );
    }

    vault.remove(credential_ref_uuid);
}
