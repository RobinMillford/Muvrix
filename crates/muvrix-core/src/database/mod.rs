//! SQLite Embedded Media Library database manager and access layer.

pub mod migrations;
pub mod models;
pub mod repository;

pub use migrations::LATEST_SCHEMA_VERSION;
pub use models::*;
pub use repository::Repository;

use rusqlite::{Connection, Transaction};
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("SQLite database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Database migration error: {0}")]
    Migration(String),

    #[error("Database integrity check failed: {0}")]
    Corruption(String),

    #[error("Lock acquisition failed")]
    LockPoisoned,
}

pub type DbResult<T> = std::result::Result<T, DatabaseError>;

/// Thread-safe central manager for the SQLite media library database.
#[derive(Debug, Clone)]
pub struct DatabaseManager {
    conn: Arc<Mutex<Connection>>,
    database_path: Option<String>,
}

impl DatabaseManager {
    /// Opens or creates an embedded media library database at the specified path.
    pub fn open(path: &Path) -> DbResult<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut conn = Connection::open(path)?;
        Self::configure_pragmas(&conn, true)?;
        migrations::apply_migrations(&mut conn)
            .map_err(|e| DatabaseError::Migration(e.to_string()))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            database_path: Some(path.to_string_lossy().to_string()),
        })
    }

    /// Opens an in-memory database for isolated unit and integration tests.
    pub fn open_in_memory() -> DbResult<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::configure_pragmas(&conn, false)?;
        migrations::apply_migrations(&mut conn)
            .map_err(|e| DatabaseError::Migration(e.to_string()))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            database_path: None,
        })
    }

    fn configure_pragmas(conn: &Connection, is_file: bool) -> DbResult<()> {
        if is_file {
            conn.execute_batch(
                "
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = NORMAL;
                PRAGMA foreign_keys = ON;
                PRAGMA busy_timeout = 5000;
                PRAGMA temp_store = MEMORY;
                ",
            )?;
        } else {
            conn.execute_batch(
                "
                PRAGMA foreign_keys = ON;
                PRAGMA busy_timeout = 5000;
                ",
            )?;
        }
        Ok(())
    }

    /// Evaluates database integrity via `PRAGMA quick_check;`.
    pub fn check_integrity(&self) -> DbResult<bool> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        let result: String = conn.query_row("PRAGMA quick_check;", [], |row| row.get(0))?;
        if result.eq_ignore_ascii_case("ok") {
            Ok(true)
        } else {
            Err(DatabaseError::Corruption(result))
        }
    }

    /// Returns the highest applied schema migration version.
    pub fn get_schema_version(&self) -> DbResult<u32> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        let version: u32 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        Ok(version)
    }

    pub fn database_path(&self) -> Option<&str> {
        self.database_path.as_deref()
    }

    /// Executes a closure within an explicit SQLite transaction, rolling back automatically on error.
    pub fn with_transaction<T, F>(&self, f: F) -> DbResult<T>
    where
        F: FnOnce(&Transaction) -> DbResult<T>,
    {
        let mut conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    // -------------------------------------------------------------------------
    // High-Level Data Access Methods
    // -------------------------------------------------------------------------

    pub fn insert_root(&self, root: &LibraryRoot) -> DbResult<i64> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::insert_root(&conn, root).map_err(DatabaseError::Sqlite)
    }

    pub fn get_roots(&self) -> DbResult<Vec<LibraryRoot>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_roots(&conn).map_err(DatabaseError::Sqlite)
    }

    pub fn get_root_by_id(&self, id: i64) -> DbResult<Option<LibraryRoot>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_root_by_id(&conn, id).map_err(DatabaseError::Sqlite)
    }

    pub fn get_root_by_path(&self, path: &str) -> DbResult<Option<LibraryRoot>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_root_by_path(&conn, path).map_err(DatabaseError::Sqlite)
    }

    pub fn update_root_status(&self, id: i64, status: &str, last_scanned_at: u64) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::update_root_status(&conn, id, status, last_scanned_at)
            .map_err(DatabaseError::Sqlite)
    }

    pub fn delete_root(&self, id: i64) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::delete_root(&conn, id).map_err(DatabaseError::Sqlite)
    }

    pub fn upsert_media_item(&self, item: &MediaItem) -> DbResult<i64> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::upsert_media_item(&conn, item).map_err(DatabaseError::Sqlite)
    }

    pub fn get_media_item_by_id(&self, id: i64) -> DbResult<Option<MediaItem>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_media_item_by_id(&conn, id).map_err(DatabaseError::Sqlite)
    }

    pub fn get_media_item_by_canonical_path(&self, path: &str) -> DbResult<Option<MediaItem>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_media_item_by_canonical_path(&conn, path).map_err(DatabaseError::Sqlite)
    }

    pub fn get_media_items_by_root(&self, root_id: i64) -> DbResult<Vec<MediaItem>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_media_items_by_root(&conn, root_id).map_err(DatabaseError::Sqlite)
    }

    /// Returns the next batch of items awaiting metadata/thumbnail enrichment.
    pub fn pending_media_items(&self, limit: usize) -> DbResult<Vec<MediaItem>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::pending_media_items(&conn, limit).map_err(DatabaseError::Sqlite)
    }

    pub fn pending_media_items_after(
        &self,
        after_id: i64,
        limit: usize,
    ) -> DbResult<Vec<MediaItem>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::pending_media_items_after(&conn, after_id, limit).map_err(DatabaseError::Sqlite)
    }

    pub fn count_pending_media_items(&self) -> DbResult<u64> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::count_pending_media_items(&conn).map_err(DatabaseError::Sqlite)
    }

    pub fn update_availability(&self, id: i64, state: AvailabilityState) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::update_availability(&conn, id, state).map_err(DatabaseError::Sqlite)
    }

    pub fn update_metadata_status(&self, id: i64, status: MetadataStatus) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::update_metadata_status(&conn, id, status).map_err(DatabaseError::Sqlite)
    }

    pub fn delete_media_item(&self, id: i64) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::delete_media_item(&conn, id).map_err(DatabaseError::Sqlite)
    }

    pub fn upsert_metadata(&self, meta: &MediaMetadata) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::upsert_metadata(&conn, meta).map_err(DatabaseError::Sqlite)
    }

    pub fn get_metadata(&self, media_id: i64) -> DbResult<Option<MediaMetadata>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_metadata(&conn, media_id).map_err(DatabaseError::Sqlite)
    }

    pub fn replace_tracks(&self, media_id: i64, tracks: &[MediaTrack]) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::replace_tracks(&conn, media_id, tracks).map_err(DatabaseError::Sqlite)
    }

    pub fn get_tracks(&self, media_id: i64) -> DbResult<Vec<MediaTrack>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_tracks(&conn, media_id).map_err(DatabaseError::Sqlite)
    }

    pub fn upsert_artwork(&self, artwork: &ArtworkRecord) -> DbResult<i64> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::upsert_artwork(&conn, artwork).map_err(DatabaseError::Sqlite)
    }

    pub fn get_artwork(&self, media_id: i64) -> DbResult<Option<ArtworkRecord>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_artwork(&conn, media_id).map_err(DatabaseError::Sqlite)
    }

    pub fn set_favourite(&self, media_id: i64, is_fav: bool) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::set_favourite(&conn, media_id, is_fav).map_err(DatabaseError::Sqlite)
    }

    pub fn is_favourite(&self, media_id: i64) -> DbResult<bool> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::is_favourite(&conn, media_id).map_err(DatabaseError::Sqlite)
    }

    pub fn create_collection(&self, name: &str, description: &str) -> DbResult<i64> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::create_collection(&conn, name, description).map_err(DatabaseError::Sqlite)
    }

    pub fn list_collections(&self) -> DbResult<Vec<Collection>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::list_collections(&conn).map_err(DatabaseError::Sqlite)
    }

    pub fn delete_collection(&self, id: i64) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::delete_collection(&conn, id).map_err(DatabaseError::Sqlite)
    }

    pub fn add_to_collection(
        &self,
        collection_id: i64,
        media_id: i64,
        position: i32,
    ) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::add_to_collection(&conn, collection_id, media_id, position)
            .map_err(DatabaseError::Sqlite)
    }

    pub fn remove_from_collection(&self, collection_id: i64, media_id: i64) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::remove_from_collection(&conn, collection_id, media_id)
            .map_err(DatabaseError::Sqlite)
    }

    pub fn get_collection_media_ids(&self, collection_id: i64) -> DbResult<Vec<i64>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_collection_media_ids(&conn, collection_id).map_err(DatabaseError::Sqlite)
    }

    pub fn create_playlist(&self, name: &str) -> DbResult<i64> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::create_playlist(&conn, name).map_err(DatabaseError::Sqlite)
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::rename_playlist(&conn, id, name).map_err(DatabaseError::Sqlite)
    }

    pub fn delete_playlist(&self, id: i64) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::delete_playlist(&conn, id).map_err(DatabaseError::Sqlite)
    }

    pub fn list_playlists(&self) -> DbResult<Vec<Playlist>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::list_playlists(&conn).map_err(DatabaseError::Sqlite)
    }

    pub fn add_playlist_entry(
        &self,
        playlist_id: i64,
        path: &str,
        title: Option<&str>,
        duration_ms: u64,
    ) -> DbResult<i64> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::add_playlist_entry(&conn, playlist_id, path, title, duration_ms)
            .map_err(DatabaseError::Sqlite)
    }

    pub fn list_playlist_entries(&self, playlist_id: i64) -> DbResult<Vec<PlaylistEntry>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::list_playlist_entries(&conn, playlist_id).map_err(DatabaseError::Sqlite)
    }

    pub fn remove_playlist_entry(&self, playlist_id: i64, entry_id: i64) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::remove_playlist_entry(&conn, playlist_id, entry_id)
            .map_err(DatabaseError::Sqlite)
    }

    pub fn reorder_playlist_entry(
        &self,
        playlist_id: i64,
        entry_id: i64,
        new_position: usize,
    ) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::reorder_playlist_entry(&conn, playlist_id, entry_id, new_position)
            .map_err(DatabaseError::Sqlite)
    }

    pub fn clear_playlist(&self, playlist_id: i64) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::clear_playlist(&conn, playlist_id).map_err(DatabaseError::Sqlite)
    }

    pub fn add_tag_to_media(&self, media_id: i64, tag_name: &str) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::add_tag_to_media(&conn, media_id, tag_name).map_err(DatabaseError::Sqlite)
    }

    pub fn remove_tag_from_media(&self, media_id: i64, tag_name: &str) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::remove_tag_from_media(&conn, media_id, tag_name).map_err(DatabaseError::Sqlite)
    }

    pub fn get_tags_for_media(&self, media_id: i64) -> DbResult<Vec<String>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_tags_for_media(&conn, media_id).map_err(DatabaseError::Sqlite)
    }

    pub fn insert_connection(&self, nc: &NetworkConnection) -> DbResult<i64> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::insert_connection(&conn, nc).map_err(DatabaseError::Sqlite)
    }

    pub fn list_connections(&self) -> DbResult<Vec<NetworkConnection>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::list_connections(&conn).map_err(DatabaseError::Sqlite)
    }

    pub fn get_connection_by_id(&self, id: i64) -> DbResult<Option<NetworkConnection>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_connection_by_id(&conn, id).map_err(DatabaseError::Sqlite)
    }

    pub fn delete_connection(&self, id: i64) -> DbResult<()> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::delete_connection(&conn, id).map_err(DatabaseError::Sqlite)
    }

    pub fn query_library(
        &self,
        filter: &LibraryFilter,
        sort_field: LibrarySortField,
        sort_dir: SortDirection,
        pagination: Pagination,
    ) -> DbResult<(Vec<LibraryItemView>, u64)> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::query_library(&conn, filter, sort_field, sort_dir, pagination)
            .map_err(DatabaseError::Sqlite)
    }

    // -------------------------------------------------------------------------
    // User Bookmarks
    // -------------------------------------------------------------------------

    pub fn insert_bookmark(
        &self,
        media_id: i64,
        timestamp_ms: i64,
        label: &str,
        note: Option<&str>,
    ) -> DbResult<i64> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::insert_bookmark(&conn, media_id, timestamp_ms, label, note)
            .map_err(DatabaseError::Sqlite)
    }

    pub fn list_bookmarks_for_media(&self, media_id: i64) -> DbResult<Vec<Bookmark>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::list_bookmarks_for_media(&conn, media_id).map_err(DatabaseError::Sqlite)
    }

    pub fn get_bookmark(&self, id: i64) -> DbResult<Option<Bookmark>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_bookmark(&conn, id).map_err(DatabaseError::Sqlite)
    }

    pub fn update_bookmark(&self, id: i64, label: &str, note: Option<&str>) -> DbResult<bool> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::update_bookmark(&conn, id, label, note).map_err(DatabaseError::Sqlite)
    }

    pub fn delete_bookmark(&self, id: i64) -> DbResult<bool> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::delete_bookmark(&conn, id).map_err(DatabaseError::Sqlite)
    }

    // -------------------------------------------------------------------------
    // Persistent SSH Host Trust (Phase 1.2)
    // -------------------------------------------------------------------------

    pub fn get_ssh_host_trust(
        &self,
        host: &str,
        port: u16,
    ) -> DbResult<Option<SshHostTrustRecord>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::get_ssh_host_trust(&conn, host, port).map_err(DatabaseError::Sqlite)
    }

    pub fn record_ssh_host_trust(
        &self,
        host: &str,
        port: u16,
        key_algorithm: &str,
        fingerprint_sha256: &str,
        notes: Option<&str>,
    ) -> DbResult<SshHostTrustRecord> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::record_ssh_host_trust(
            &conn,
            host,
            port,
            key_algorithm,
            fingerprint_sha256,
            notes,
        )
        .map_err(DatabaseError::Sqlite)
    }

    pub fn update_ssh_host_last_seen(&self, host: &str, port: u16) -> DbResult<bool> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::update_ssh_host_last_seen(&conn, host, port).map_err(DatabaseError::Sqlite)
    }

    pub fn revoke_ssh_host_trust(&self, host: &str, port: u16) -> DbResult<bool> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::revoke_ssh_host_trust(&conn, host, port).map_err(DatabaseError::Sqlite)
    }

    pub fn list_ssh_host_trust(&self) -> DbResult<Vec<SshHostTrustRecord>> {
        let conn = self.conn.lock().map_err(|_| DatabaseError::LockPoisoned)?;
        Repository::list_ssh_host_trust(&conn).map_err(DatabaseError::Sqlite)
    }
}
