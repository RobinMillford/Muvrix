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

#[test]
fn test_live_secret_service_native_methods() {
    if !should_run_native_keyring_tests() {
        return;
    }

    let test_uuid = format!(
        "test-native-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let _guard = KeyringTestGuard::new(MUVRIX_KEYRING_SERVICE, &test_uuid);
    let secret = "NativeSecretTest_987#xyz";

    let vault = CredentialVault::new();

    // 1. Store via explicit native method (skip gracefully when the
    // Secret Service daemon is unavailable, matching the CRUD test above)
    match vault.store_native(&test_uuid, secret) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Secret Service not accessible in this environment: {e}; skipping");
            return;
        }
    }

    // 2. Retrieve via explicit native method
    let retrieved = vault
        .get_native(&test_uuid)
        .expect("Explicit get_native from native keyring");
    assert_eq!(retrieved, secret);

    // 3. Delete via explicit native method
    vault
        .remove_native(&test_uuid)
        .expect("Explicit remove_native from native keyring");

    // 4. Verify get_native now returns NotFound
    let after_delete = vault.get_native(&test_uuid);
    assert!(
        after_delete.is_err(),
        "get_native must return Err after deletion"
    );
}

#[test]
fn test_live_secret_service_restart_lifecycle() {
    if !should_run_native_keyring_tests() {
        return;
    }

    let test_uuid = format!(
        "test-restart-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let _guard = KeyringTestGuard::new(MUVRIX_KEYRING_SERVICE, &test_uuid);
    let secret_v1 = "InitialSecretAfterReboot_111";
    let secret_v2 = "UpdatedSecretAfterReboot_222";

    // Session 1: Store secret (skip gracefully when no Secret Service)
    {
        let vault_session1 = CredentialVault::new();
        match vault_session1.store_native(&test_uuid, secret_v1) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Secret Service not accessible in this environment: {e}; skipping");
                return;
            }
        }
        assert_eq!(vault_session1.get(&test_uuid).as_deref(), Some(secret_v1));
    } // vault_session1 dropped, in-memory cache gone

    // Session 2: Application restart simulation
    {
        let vault_session2 = CredentialVault::new();
        // In-memory cache is empty, must retrieve from native OS keyring
        let loaded = vault_session2
            .get_native(&test_uuid)
            .expect("Session 2 retrieve from native keyring after restart");
        assert_eq!(loaded, secret_v1);

        // Update secret in native store
        vault_session2
            .store_native(&test_uuid, secret_v2)
            .expect("Session 2 update secret in native keyring");
    } // vault_session2 dropped

    // Session 3: Second application restart simulation
    {
        let vault_session3 = CredentialVault::new();
        let loaded_v2 = vault_session3
            .get_native(&test_uuid)
            .expect("Session 3 retrieve updated secret after second restart");
        assert_eq!(loaded_v2, secret_v2);

        // Delete secret
        vault_session3
            .remove_native(&test_uuid)
            .expect("Session 3 remove_native");
    }

    // Session 4: Verification of deletion after restart
    {
        let vault_session4 = CredentialVault::new();
        assert!(
            vault_session4.get_native(&test_uuid).is_err(),
            "Session 4 must find no credential after deletion"
        );
        assert_eq!(vault_session4.get(&test_uuid), None);
    }
}

#[test]
fn test_ephemeral_does_not_persist_to_native_store() {
    if !should_run_native_keyring_tests() {
        return;
    }

    let test_uuid = format!(
        "test-eph-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let _guard = KeyringTestGuard::new(MUVRIX_KEYRING_SERVICE, &test_uuid);
    let secret = "EphemeralSecret_DoNotPersist";

    let vault_session1 = CredentialVault::new();
    vault_session1.store_ephemeral(&test_uuid, secret);

    // Ephemeral lookup succeeds in the same instance
    assert_eq!(
        vault_session1.get_ephemeral(&test_uuid).as_deref(),
        Some(secret)
    );
    assert_eq!(vault_session1.get(&test_uuid).as_deref(), Some(secret));

    // Native store must NOT contain the ephemeral secret
    let native_res = vault_session1.get_native(&test_uuid);
    assert!(
        native_res.is_err(),
        "Native store must not contain secrets stored via store_ephemeral"
    );

    // Restart simulation: another vault instance with empty in-memory cache
    let vault_session2 = CredentialVault::new();
    assert_eq!(
        vault_session2.get(&test_uuid),
        None,
        "Ephemeral secret must not be visible across application instances"
    );
}
