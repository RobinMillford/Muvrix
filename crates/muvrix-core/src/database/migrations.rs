//! Database migration runner and versioned DDL definitions.

use rusqlite::{Connection, Result, Transaction};
use std::time::{SystemTime, UNIX_EPOCH};

pub const LATEST_SCHEMA_VERSION: u32 = 6;

/// Apply all pending database migrations within a transaction.
pub fn apply_migrations(conn: &mut Connection) -> Result<u32> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL,
            description TEXT NOT NULL
        );
        ",
    )?;

    let current_version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current_version < 1 {
        let tx = conn.transaction()?;
        migration_001_core_schema(&tx)?;
        record_migration(&tx, 1, "Initial core schema for library and network")?;
        tx.commit()?;
    }

    if current_version < 2 {
        let tx = conn.transaction()?;
        migration_002_user_bookmarks(&tx)?;
        record_migration(&tx, 2, "User bookmarks schema with foreign key cascade")?;
        tx.commit()?;
    }

    if current_version < 3 {
        let tx = conn.transaction()?;
        migration_003_ssh_host_trust(&tx)?;
        record_migration(&tx, 3, "Persistent SSH host key trust store")?;
        tx.commit()?;
    }

    if current_version < 4 {
        let tx = conn.transaction()?;
        migration_004_playlists(&tx)?;
        record_migration(&tx, 4, "Persistent playlists and ordered M3U entries")?;
        tx.commit()?;
    }

    if current_version < 5 {
        let tx = conn.transaction()?;
        migration_005_watch_state(&tx)?;
        record_migration(&tx, 5, "Library watched state for batch actions")?;
        tx.commit()?;
    }

    if current_version < 6 {
        let tx = conn.transaction()?;
        migration_006_fts5_search(&tx)?;
        record_migration(&tx, 6, "FTS5 full-text search virtual table and triggers")?;
        tx.commit()?;
    }

    Ok(LATEST_SCHEMA_VERSION)
}

fn record_migration(tx: &Transaction, version: u32, description: &str) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    tx.execute(
        "INSERT INTO schema_migrations (version, applied_at, description) VALUES (?1, ?2, ?3)",
        rusqlite::params![version, now, description],
    )?;
    Ok(())
}

fn migration_001_core_schema(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "
        CREATE TABLE library_roots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            display_name TEXT NOT NULL,
            root_path TEXT NOT NULL UNIQUE,
            source_type TEXT NOT NULL DEFAULT 'local',
            is_recursive INTEGER NOT NULL DEFAULT 1,
            is_enabled INTEGER NOT NULL DEFAULT 1,
            exclusion_patterns TEXT NOT NULL DEFAULT '[]',
            last_scanned_at INTEGER NOT NULL DEFAULT 0,
            scan_status TEXT NOT NULL DEFAULT 'idle'
        );

        CREATE TABLE media_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            root_id INTEGER NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
            relative_path TEXT NOT NULL,
            canonical_path TEXT NOT NULL UNIQUE,
            file_name TEXT NOT NULL,
            file_extension TEXT NOT NULL,
            media_type TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            modified_time INTEGER NOT NULL,
            added_time INTEGER NOT NULL,
            last_scanned_at INTEGER NOT NULL,
            availability_state TEXT NOT NULL DEFAULT 'available',
            metadata_status TEXT NOT NULL DEFAULT 'pending'
        );

        CREATE INDEX idx_media_items_root_id ON media_items(root_id);
        CREATE INDEX idx_media_items_media_type ON media_items(media_type);
        CREATE INDEX idx_media_items_canonical_path ON media_items(canonical_path);
        CREATE INDEX idx_media_items_added_time ON media_items(added_time DESC);
        CREATE INDEX idx_media_items_modified_time ON media_items(modified_time DESC);

        CREATE TABLE media_metadata (
            media_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
            title TEXT,
            original_title TEXT,
            description TEXT,
            genre TEXT,
            release_year INTEGER,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            overall_bitrate INTEGER NOT NULL DEFAULT 0,
            container TEXT,
            video_codec TEXT,
            audio_codec TEXT,
            width INTEGER NOT NULL DEFAULT 0,
            height INTEGER NOT NULL DEFAULT 0,
            fps REAL NOT NULL DEFAULT 0.0,
            hdr_type TEXT,
            audio_channels INTEGER NOT NULL DEFAULT 0,
            sample_rate INTEGER NOT NULL DEFAULT 0,
            bit_depth INTEGER NOT NULL DEFAULT 0,
            artist TEXT,
            album TEXT,
            album_artist TEXT,
            track_number INTEGER,
            disc_number INTEGER
        );

        CREATE INDEX idx_media_metadata_title ON media_metadata(title);
        CREATE INDEX idx_media_metadata_artist ON media_metadata(artist);
        CREATE INDEX idx_media_metadata_album ON media_metadata(album);
        CREATE INDEX idx_media_metadata_genre ON media_metadata(genre);

        CREATE TABLE media_tracks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            media_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
            track_index INTEGER NOT NULL,
            track_type TEXT NOT NULL,
            title TEXT,
            language TEXT,
            codec TEXT,
            channel_layout TEXT,
            is_default INTEGER NOT NULL DEFAULT 0,
            is_selected INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX idx_media_tracks_media_id ON media_tracks(media_id);

        CREATE TABLE artwork_cache (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            media_id INTEGER NOT NULL UNIQUE REFERENCES media_items(id) ON DELETE CASCADE,
            cache_key TEXT NOT NULL UNIQUE,
            file_path TEXT NOT NULL,
            source_type TEXT NOT NULL,
            width INTEGER NOT NULL,
            height INTEGER NOT NULL,
            generated_at INTEGER NOT NULL
        );

        CREATE INDEX idx_artwork_cache_media_id ON artwork_cache(media_id);

        CREATE TABLE collections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            description TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE collection_items (
            collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
            media_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            added_at INTEGER NOT NULL,
            PRIMARY KEY (collection_id, media_id)
        );

        CREATE TABLE favourites (
            media_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
            added_at INTEGER NOT NULL
        );

        CREATE TABLE tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE media_tags (
            media_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (media_id, tag_id)
        );

        CREATE TABLE scan_jobs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            root_id INTEGER NOT NULL REFERENCES library_roots(id) ON DELETE CASCADE,
            started_at INTEGER NOT NULL,
            completed_at INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL,
            items_discovered INTEGER NOT NULL DEFAULT 0,
            items_updated INTEGER NOT NULL DEFAULT 0,
            items_missing INTEGER NOT NULL DEFAULT 0,
            errors_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE network_connections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            protocol TEXT NOT NULL,
            host TEXT NOT NULL,
            port INTEGER NOT NULL,
            remote_path TEXT NOT NULL DEFAULT '/',
            auth_type TEXT NOT NULL DEFAULT 'anonymous',
            credential_ref TEXT,
            is_anonymous INTEGER NOT NULL DEFAULT 0,
            auto_connect INTEGER NOT NULL DEFAULT 0,
            last_connected_at INTEGER NOT NULL DEFAULT 0
        );
        ",
    )?;

    Ok(())
}

fn migration_002_user_bookmarks(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "
        CREATE TABLE user_bookmarks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            media_id INTEGER NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
            timestamp_ms INTEGER NOT NULL,
            label TEXT NOT NULL,
            note TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE INDEX idx_user_bookmarks_media_id ON user_bookmarks(media_id);
        CREATE INDEX idx_user_bookmarks_timestamp ON user_bookmarks(media_id, timestamp_ms);
        ",
    )?;

    Ok(())
}

fn migration_003_ssh_host_trust(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "
        CREATE TABLE ssh_host_trust (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            host TEXT NOT NULL,
            port INTEGER NOT NULL,
            key_algorithm TEXT NOT NULL,
            fingerprint_sha256 TEXT NOT NULL,
            trusted_at INTEGER NOT NULL,
            last_seen_at INTEGER NOT NULL,
            notes TEXT,
            UNIQUE(host, port)
        );

        CREATE INDEX idx_ssh_host_trust_lookup ON ssh_host_trust(host, port);
        ",
    )?;

    Ok(())
}

fn migration_004_playlists(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "
        CREATE TABLE playlists (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE playlist_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            path TEXT NOT NULL,
            title TEXT,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            added_at INTEGER NOT NULL
        );

        CREATE INDEX idx_playlist_items_playlist ON playlist_items(playlist_id, position);
        ",
    )?;
    Ok(())
}

fn migration_005_watch_state(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "
        CREATE TABLE media_watch_state (
            media_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
            is_watched INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL
        );
        ",
    )?;
    Ok(())
}

fn migration_006_fts5_search(tx: &Transaction) -> Result<()> {
    tx.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS media_metadata (
            media_id INTEGER PRIMARY KEY REFERENCES media_items(id) ON DELETE CASCADE,
            title TEXT,
            original_title TEXT,
            description TEXT,
            genre TEXT,
            release_year INTEGER,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            overall_bitrate INTEGER NOT NULL DEFAULT 0,
            container TEXT,
            video_codec TEXT,
            audio_codec TEXT,
            width INTEGER NOT NULL DEFAULT 0,
            height INTEGER NOT NULL DEFAULT 0,
            fps REAL NOT NULL DEFAULT 0.0,
            hdr_type TEXT,
            audio_channels INTEGER NOT NULL DEFAULT 0,
            sample_rate INTEGER NOT NULL DEFAULT 0,
            bit_depth INTEGER NOT NULL DEFAULT 0,
            artist TEXT,
            album TEXT,
            album_artist TEXT,
            track_number INTEGER,
            disc_number INTEGER
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS media_items_fts USING fts5(
            file_name,
            title,
            artist,
            album,
            genre,
            tokenize='unicode61'
        );

        -- Synchronize any existing media items and metadata into FTS5
        INSERT OR REPLACE INTO media_items_fts (rowid, file_name, title, artist, album, genre)
        SELECT
            mi.id,
            mi.file_name,
            COALESCE(mm.title, ''),
            COALESCE(mm.artist, ''),
            COALESCE(mm.album, ''),
            COALESCE(mm.genre, '')
        FROM media_items mi
        LEFT JOIN media_metadata mm ON mi.id = mm.media_id;

        -- Triggers to maintain real-time bidirectional FTS index synchronization
        CREATE TRIGGER IF NOT EXISTS trg_media_items_fts_insert AFTER INSERT ON media_items
        BEGIN
            INSERT INTO media_items_fts (rowid, file_name, title, artist, album, genre)
            VALUES (new.id, new.file_name, '', '', '', '');
        END;

        CREATE TRIGGER IF NOT EXISTS trg_media_items_fts_delete AFTER DELETE ON media_items
        BEGIN
            DELETE FROM media_items_fts WHERE rowid = old.id;
        END;

        CREATE TRIGGER IF NOT EXISTS trg_media_items_fts_update AFTER UPDATE OF file_name ON media_items
        BEGIN
            UPDATE media_items_fts
            SET file_name = new.file_name
            WHERE rowid = new.id;
        END;

        CREATE TRIGGER IF NOT EXISTS trg_media_metadata_fts_insert AFTER INSERT ON media_metadata
        BEGIN
            UPDATE media_items_fts
            SET title = COALESCE(new.title, ''),
                artist = COALESCE(new.artist, ''),
                album = COALESCE(new.album, ''),
                genre = COALESCE(new.genre, '')
            WHERE rowid = new.media_id;
        END;

        CREATE TRIGGER IF NOT EXISTS trg_media_metadata_fts_update AFTER UPDATE ON media_metadata
        BEGIN
            UPDATE media_items_fts
            SET title = COALESCE(new.title, ''),
                artist = COALESCE(new.artist, ''),
                album = COALESCE(new.album, ''),
                genre = COALESCE(new.genre, '')
            WHERE rowid = new.media_id;
        END;
        ",
    )?;
    Ok(())
}
