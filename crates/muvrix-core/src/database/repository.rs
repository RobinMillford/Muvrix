//! Repository implementation for database entities and complex library queries.

use super::batch_editor::*;
use super::integrity::*;
use super::models::*;
use super::smart_collections::*;
use rusqlite::{params, Connection, OptionalExtension, Result};
use std::time::{SystemTime, UNIX_EPOCH};

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub struct Repository;

impl Repository {
    // -------------------------------------------------------------------------
    // Library Roots
    // -------------------------------------------------------------------------

    pub fn insert_root(conn: &Connection, root: &LibraryRoot) -> Result<i64> {
        let exclusions_json =
            serde_json::to_string(&root.exclusion_patterns).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO library_roots (display_name, root_path, source_type, is_recursive, is_enabled, exclusion_patterns, last_scanned_at, scan_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                root.display_name,
                root.root_path,
                root.source_type,
                root.is_recursive as i32,
                root.is_enabled as i32,
                exclusions_json,
                root.last_scanned_at as i64,
                root.scan_status,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_roots(conn: &Connection) -> Result<Vec<LibraryRoot>> {
        let mut stmt = conn.prepare(
            "SELECT id, display_name, root_path, source_type, is_recursive, is_enabled, exclusion_patterns, last_scanned_at, scan_status
             FROM library_roots ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let exclusions_raw: String = row.get(6)?;
            let exclusion_patterns =
                serde_json::from_str(&exclusions_raw).unwrap_or_else(|_| Vec::new());
            Ok(LibraryRoot {
                id: row.get(0)?,
                display_name: row.get(1)?,
                root_path: row.get(2)?,
                source_type: row.get(3)?,
                is_recursive: row.get::<_, i32>(4)? != 0,
                is_enabled: row.get::<_, i32>(5)? != 0,
                exclusion_patterns,
                last_scanned_at: row.get::<_, i64>(7)? as u64,
                scan_status: row.get(8)?,
            })
        })?;

        let mut roots = Vec::new();
        for r in rows {
            roots.push(r?);
        }
        Ok(roots)
    }

    pub fn get_root_by_id(conn: &Connection, id: i64) -> Result<Option<LibraryRoot>> {
        let mut stmt = conn.prepare(
            "SELECT id, display_name, root_path, source_type, is_recursive, is_enabled, exclusion_patterns, last_scanned_at, scan_status
             FROM library_roots WHERE id = ?1",
        )?;
        stmt.query_row(params![id], |row| {
            let exclusions_raw: String = row.get(6)?;
            let exclusion_patterns =
                serde_json::from_str(&exclusions_raw).unwrap_or_else(|_| Vec::new());
            Ok(LibraryRoot {
                id: row.get(0)?,
                display_name: row.get(1)?,
                root_path: row.get(2)?,
                source_type: row.get(3)?,
                is_recursive: row.get::<_, i32>(4)? != 0,
                is_enabled: row.get::<_, i32>(5)? != 0,
                exclusion_patterns,
                last_scanned_at: row.get::<_, i64>(7)? as u64,
                scan_status: row.get(8)?,
            })
        })
        .optional()
    }

    pub fn get_root_by_path(conn: &Connection, path: &str) -> Result<Option<LibraryRoot>> {
        let mut stmt = conn.prepare(
            "SELECT id, display_name, root_path, source_type, is_recursive, is_enabled, exclusion_patterns, last_scanned_at, scan_status
             FROM library_roots WHERE root_path = ?1",
        )?;
        stmt.query_row(params![path], |row| {
            let exclusions_raw: String = row.get(6)?;
            let exclusion_patterns =
                serde_json::from_str(&exclusions_raw).unwrap_or_else(|_| Vec::new());
            Ok(LibraryRoot {
                id: row.get(0)?,
                display_name: row.get(1)?,
                root_path: row.get(2)?,
                source_type: row.get(3)?,
                is_recursive: row.get::<_, i32>(4)? != 0,
                is_enabled: row.get::<_, i32>(5)? != 0,
                exclusion_patterns,
                last_scanned_at: row.get::<_, i64>(7)? as u64,
                scan_status: row.get(8)?,
            })
        })
        .optional()
    }

    pub fn update_root_status(
        conn: &Connection,
        id: i64,
        status: &str,
        last_scanned_at: u64,
    ) -> Result<()> {
        conn.execute(
            "UPDATE library_roots SET scan_status = ?1, last_scanned_at = ?2 WHERE id = ?3",
            params![status, last_scanned_at as i64, id],
        )?;
        Ok(())
    }

    pub fn delete_root(conn: &Connection, id: i64) -> Result<()> {
        conn.execute("DELETE FROM library_roots WHERE id = ?1", params![id])?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Media Items
    // -------------------------------------------------------------------------

    pub fn upsert_media_item(conn: &Connection, item: &MediaItem) -> Result<i64> {
        conn.execute(
            "INSERT INTO media_items (root_id, relative_path, canonical_path, file_name, file_extension, media_type, file_size, modified_time, added_time, last_scanned_at, availability_state, metadata_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(canonical_path) DO UPDATE SET
                file_size = excluded.file_size,
                modified_time = excluded.modified_time,
                last_scanned_at = excluded.last_scanned_at,
                availability_state = excluded.availability_state,
                metadata_status = CASE
                    WHEN media_items.modified_time != excluded.modified_time THEN 'pending'
                    ELSE media_items.metadata_status
                END",
            params![
                item.root_id,
                item.relative_path,
                item.canonical_path,
                item.file_name,
                item.file_extension,
                item.media_type.as_str(),
                item.file_size as i64,
                item.modified_time as i64,
                item.added_time as i64,
                item.last_scanned_at as i64,
                item.availability_state.as_str(),
                item.metadata_status.as_str(),
            ],
        )?;

        let id: i64 = conn.query_row(
            "SELECT id FROM media_items WHERE canonical_path = ?1",
            params![item.canonical_path],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn get_media_item_by_id(conn: &Connection, id: i64) -> Result<Option<MediaItem>> {
        let mut stmt = conn.prepare(
            "SELECT id, root_id, relative_path, canonical_path, file_name, file_extension, media_type, file_size, modified_time, added_time, last_scanned_at, availability_state, metadata_status
             FROM media_items WHERE id = ?1",
        )?;
        stmt.query_row(params![id], Self::map_media_item).optional()
    }

    pub fn get_media_item_by_canonical_path(
        conn: &Connection,
        path: &str,
    ) -> Result<Option<MediaItem>> {
        let mut stmt = conn.prepare(
            "SELECT id, root_id, relative_path, canonical_path, file_name, file_extension, media_type, file_size, modified_time, added_time, last_scanned_at, availability_state, metadata_status
             FROM media_items WHERE canonical_path = ?1",
        )?;
        stmt.query_row(params![path], Self::map_media_item)
            .optional()
    }

    pub fn get_media_items_by_root(conn: &Connection, root_id: i64) -> Result<Vec<MediaItem>> {
        let mut stmt = conn.prepare(
            "SELECT id, root_id, relative_path, canonical_path, file_name, file_extension, media_type, file_size, modified_time, added_time, last_scanned_at, availability_state, metadata_status
             FROM media_items WHERE root_id = ?1 ORDER BY relative_path ASC",
        )?;
        let rows = stmt.query_map(params![root_id], Self::map_media_item)?;
        let mut items = Vec::new();
        for item in rows {
            items.push(item?);
        }
        Ok(items)
    }

    /// Returns the next batch of available items needing enrichment: either
    /// metadata is pending, or metadata is already extracted but no artwork row
    /// exists yet. `Failed` items are deliberately excluded so a corrupt file
    /// is not retried on every enrichment pass.
    pub fn pending_media_items(conn: &Connection, limit: usize) -> Result<Vec<MediaItem>> {
        Self::pending_media_items_after(conn, 0, limit)
    }

    /// Cursor variant used by the enrichment worker. `after_id` prevents a
    /// failed thumbnail (which remains artwork-less by design) from starving
    /// later items in the same pass.
    pub fn pending_media_items_after(
        conn: &Connection,
        after_id: i64,
        limit: usize,
    ) -> Result<Vec<MediaItem>> {
        let mut stmt = conn.prepare(
            "SELECT id, root_id, relative_path, canonical_path, file_name, file_extension, media_type, file_size, modified_time, added_time, last_scanned_at, availability_state, metadata_status
             FROM media_items
             WHERE availability_state = 'available'
               AND id > ?1
               AND (
                    metadata_status = 'pending'
                    OR (
                        metadata_status = 'extracted'
                        AND NOT EXISTS (
                            SELECT 1 FROM artwork_cache
                            WHERE artwork_cache.media_id = media_items.id
                        )
                    )
               )
             ORDER BY id ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![after_id, limit as i64], Self::map_media_item)?;
        let mut items = Vec::new();
        for item in rows {
            items.push(item?);
        }
        Ok(items)
    }

    /// Number of items currently awaiting metadata or missing artwork.
    pub fn count_pending_media_items(conn: &Connection) -> Result<u64> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM media_items
             WHERE availability_state = 'available'
               AND (
                    metadata_status = 'pending'
                    OR (
                        metadata_status = 'extracted'
                        AND NOT EXISTS (
                            SELECT 1 FROM artwork_cache
                            WHERE artwork_cache.media_id = media_items.id
                        )
                    )
               )",
            [],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as u64)
    }

    pub fn set_watched(conn: &Connection, media_id: i64, watched: bool) -> Result<()> {
        conn.execute(
            "INSERT INTO media_watch_state (media_id, is_watched, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(media_id) DO UPDATE SET
                is_watched = excluded.is_watched,
                updated_at = excluded.updated_at",
            params![media_id, watched as i32, current_timestamp() as i64],
        )?;
        Ok(())
    }

    pub fn is_watched(conn: &Connection, media_id: i64) -> Result<bool> {
        let value: Option<i64> = conn
            .query_row(
                "SELECT is_watched FROM media_watch_state WHERE media_id = ?1",
                params![media_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value.unwrap_or(0) != 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_availability(conn: &Connection, id: i64, state: AvailabilityState) -> Result<()> {
        conn.execute(
            "UPDATE media_items SET availability_state = ?1 WHERE id = ?2",
            params![state.as_str(), id],
        )?;
        Ok(())
    }

    pub fn update_metadata_status(
        conn: &Connection,
        id: i64,
        status: MetadataStatus,
    ) -> Result<()> {
        conn.execute(
            "UPDATE media_items SET metadata_status = ?1 WHERE id = ?2",
            params![status.as_str(), id],
        )?;
        Ok(())
    }

    pub fn delete_media_item(conn: &Connection, id: i64) -> Result<()> {
        conn.execute("DELETE FROM media_items WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn map_media_item(row: &rusqlite::Row) -> Result<MediaItem> {
        let media_type_str: String = row.get(6)?;
        let avail_str: String = row.get(11)?;
        let meta_status_str: String = row.get(12)?;

        Ok(MediaItem {
            id: row.get(0)?,
            root_id: row.get(1)?,
            relative_path: row.get(2)?,
            canonical_path: row.get(3)?,
            file_name: row.get(4)?,
            file_extension: row.get(5)?,
            media_type: MediaType::parse(&media_type_str),
            file_size: row.get::<_, i64>(7)? as u64,
            modified_time: row.get::<_, i64>(8)? as u64,
            added_time: row.get::<_, i64>(9)? as u64,
            last_scanned_at: row.get::<_, i64>(10)? as u64,
            availability_state: AvailabilityState::parse(&avail_str),
            metadata_status: MetadataStatus::parse(&meta_status_str),
        })
    }

    // -------------------------------------------------------------------------
    // Media Metadata & Tracks
    // -------------------------------------------------------------------------

    pub fn upsert_metadata(conn: &Connection, meta: &MediaMetadata) -> Result<()> {
        conn.execute(
            "INSERT INTO media_metadata (media_id, title, original_title, description, genre, release_year, duration_ms, overall_bitrate, container, video_codec, audio_codec, width, height, fps, hdr_type, audio_channels, sample_rate, bit_depth, artist, album, album_artist, track_number, disc_number)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)
             ON CONFLICT(media_id) DO UPDATE SET
                title = excluded.title,
                original_title = excluded.original_title,
                description = excluded.description,
                genre = excluded.genre,
                release_year = excluded.release_year,
                duration_ms = excluded.duration_ms,
                overall_bitrate = excluded.overall_bitrate,
                container = excluded.container,
                video_codec = excluded.video_codec,
                audio_codec = excluded.audio_codec,
                width = excluded.width,
                height = excluded.height,
                fps = excluded.fps,
                hdr_type = excluded.hdr_type,
                audio_channels = excluded.audio_channels,
                sample_rate = excluded.sample_rate,
                bit_depth = excluded.bit_depth,
                artist = excluded.artist,
                album = excluded.album,
                album_artist = excluded.album_artist,
                track_number = excluded.track_number,
                disc_number = excluded.disc_number",
            params![
                meta.media_id,
                meta.title,
                meta.original_title,
                meta.description,
                meta.genre,
                meta.release_year,
                meta.duration_ms as i64,
                meta.overall_bitrate as i64,
                meta.container,
                meta.video_codec,
                meta.audio_codec,
                meta.width as i64,
                meta.height as i64,
                meta.fps,
                meta.hdr_type,
                meta.audio_channels as i64,
                meta.sample_rate as i64,
                meta.bit_depth as i64,
                meta.artist,
                meta.album,
                meta.album_artist,
                meta.track_number,
                meta.disc_number,
            ],
        )?;

        // Update parent media item status to 'extracted'
        conn.execute(
            "UPDATE media_items SET metadata_status = 'extracted' WHERE id = ?1",
            params![meta.media_id],
        )?;

        Ok(())
    }

    pub fn get_metadata(conn: &Connection, media_id: i64) -> Result<Option<MediaMetadata>> {
        let mut stmt = conn.prepare(
            "SELECT media_id, title, original_title, description, genre, release_year, duration_ms, overall_bitrate, container, video_codec, audio_codec, width, height, fps, hdr_type, audio_channels, sample_rate, bit_depth, artist, album, album_artist, track_number, disc_number
             FROM media_metadata WHERE media_id = ?1",
        )?;
        stmt.query_row(params![media_id], |row| {
            Ok(MediaMetadata {
                media_id: row.get(0)?,
                title: row.get(1)?,
                original_title: row.get(2)?,
                description: row.get(3)?,
                genre: row.get(4)?,
                release_year: row.get(5)?,
                duration_ms: row.get::<_, i64>(6)? as u64,
                overall_bitrate: row.get::<_, i64>(7)? as u64,
                container: row.get(8)?,
                video_codec: row.get(9)?,
                audio_codec: row.get(10)?,
                width: row.get::<_, i64>(11)? as u32,
                height: row.get::<_, i64>(12)? as u32,
                fps: row.get(13)?,
                hdr_type: row.get(14)?,
                audio_channels: row.get::<_, i64>(15)? as u32,
                sample_rate: row.get::<_, i64>(16)? as u32,
                bit_depth: row.get::<_, i64>(17)? as u32,
                artist: row.get(18)?,
                album: row.get(19)?,
                album_artist: row.get(20)?,
                track_number: row.get(21)?,
                disc_number: row.get(22)?,
            })
        })
        .optional()
    }

    pub fn replace_tracks(conn: &Connection, media_id: i64, tracks: &[MediaTrack]) -> Result<()> {
        conn.execute(
            "DELETE FROM media_tracks WHERE media_id = ?1",
            params![media_id],
        )?;
        for t in tracks {
            conn.execute(
                "INSERT INTO media_tracks (media_id, track_index, track_type, title, language, codec, channel_layout, is_default, is_selected)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    media_id,
                    t.track_index,
                    t.track_type.as_str(),
                    t.title,
                    t.language,
                    t.codec,
                    t.channel_layout,
                    t.is_default as i32,
                    t.is_selected as i32,
                ],
            )?;
        }
        Ok(())
    }

    pub fn get_tracks(conn: &Connection, media_id: i64) -> Result<Vec<MediaTrack>> {
        let mut stmt = conn.prepare(
            "SELECT id, media_id, track_index, track_type, title, language, codec, channel_layout, is_default, is_selected
             FROM media_tracks WHERE media_id = ?1 ORDER BY track_index ASC",
        )?;
        let rows = stmt.query_map(params![media_id], |row| {
            let track_type_str: String = row.get(3)?;
            Ok(MediaTrack {
                id: row.get(0)?,
                media_id: row.get(1)?,
                track_index: row.get(2)?,
                track_type: TrackType::parse(&track_type_str),
                title: row.get(4)?,
                language: row.get(5)?,
                codec: row.get(6)?,
                channel_layout: row.get(7)?,
                is_default: row.get::<_, i32>(8)? != 0,
                is_selected: row.get::<_, i32>(9)? != 0,
            })
        })?;

        let mut tracks = Vec::new();
        for t in rows {
            tracks.push(t?);
        }
        Ok(tracks)
    }

    // -------------------------------------------------------------------------
    // Artwork & Thumbnails
    // -------------------------------------------------------------------------

    pub fn upsert_artwork(conn: &Connection, artwork: &ArtworkRecord) -> Result<i64> {
        conn.execute(
            "INSERT INTO artwork_cache (media_id, cache_key, file_path, source_type, width, height, generated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(media_id) DO UPDATE SET
                cache_key = excluded.cache_key,
                file_path = excluded.file_path,
                source_type = excluded.source_type,
                width = excluded.width,
                height = excluded.height,
                generated_at = excluded.generated_at",
            params![
                artwork.media_id,
                artwork.cache_key,
                artwork.file_path,
                artwork.source_type.as_str(),
                artwork.width as i64,
                artwork.height as i64,
                artwork.generated_at as i64,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_artwork(conn: &Connection, media_id: i64) -> Result<Option<ArtworkRecord>> {
        let mut stmt = conn.prepare(
            "SELECT id, media_id, cache_key, file_path, source_type, width, height, generated_at
             FROM artwork_cache WHERE media_id = ?1",
        )?;
        stmt.query_row(params![media_id], |row| {
            let source_str: String = row.get(4)?;
            Ok(ArtworkRecord {
                id: row.get(0)?,
                media_id: row.get(1)?,
                cache_key: row.get(2)?,
                file_path: row.get(3)?,
                source_type: ArtworkSource::parse(&source_str),
                width: row.get::<_, i64>(5)? as u32,
                height: row.get::<_, i64>(6)? as u32,
                generated_at: row.get::<_, i64>(7)? as u64,
            })
        })
        .optional()
    }

    // -------------------------------------------------------------------------
    // Favourites
    // -------------------------------------------------------------------------

    pub fn set_favourite(conn: &Connection, media_id: i64, is_fav: bool) -> Result<()> {
        if is_fav {
            conn.execute(
                "INSERT INTO favourites (media_id, added_at) VALUES (?1, ?2) ON CONFLICT(media_id) DO NOTHING",
                params![media_id, current_timestamp() as i64],
            )?;
        } else {
            conn.execute(
                "DELETE FROM favourites WHERE media_id = ?1",
                params![media_id],
            )?;
        }
        Ok(())
    }

    pub fn is_favourite(conn: &Connection, media_id: i64) -> Result<bool> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM favourites WHERE media_id = ?1",
            params![media_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // -------------------------------------------------------------------------
    // Collections
    // -------------------------------------------------------------------------

    pub fn create_collection(conn: &Connection, name: &str, description: &str) -> Result<i64> {
        let now = current_timestamp() as i64;
        conn.execute(
            "INSERT INTO collections (name, description, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![name, description, now, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_collections(conn: &Connection) -> Result<Vec<Collection>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, created_at, updated_at FROM collections ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Collection {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                created_at: row.get::<_, i64>(3)? as u64,
                updated_at: row.get::<_, i64>(4)? as u64,
            })
        })?;

        let mut colls = Vec::new();
        for c in rows {
            colls.push(c?);
        }
        Ok(colls)
    }

    pub fn delete_collection(conn: &Connection, id: i64) -> Result<()> {
        conn.execute("DELETE FROM collections WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn add_to_collection(
        conn: &Connection,
        collection_id: i64,
        media_id: i64,
        position: i32,
    ) -> Result<()> {
        let now = current_timestamp() as i64;
        conn.execute(
            "INSERT INTO collection_items (collection_id, media_id, position, added_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(collection_id, media_id) DO UPDATE SET position = excluded.position",
            params![collection_id, media_id, position, now],
        )?;
        Ok(())
    }

    pub fn remove_from_collection(
        conn: &Connection,
        collection_id: i64,
        media_id: i64,
    ) -> Result<()> {
        conn.execute(
            "DELETE FROM collection_items WHERE collection_id = ?1 AND media_id = ?2",
            params![collection_id, media_id],
        )?;
        Ok(())
    }

    pub fn get_collection_media_ids(conn: &Connection, collection_id: i64) -> Result<Vec<i64>> {
        let mut stmt = conn.prepare(
            "SELECT media_id FROM collection_items WHERE collection_id = ?1 ORDER BY position ASC, added_at ASC",
        )?;
        let rows = stmt.query_map(params![collection_id], |row| row.get(0))?;
        let mut ids = Vec::new();
        for id in rows {
            ids.push(id?);
        }
        Ok(ids)
    }

    // -------------------------------------------------------------------------
    // Persistent Playlists
    // -------------------------------------------------------------------------

    pub fn create_playlist(conn: &Connection, name: &str) -> Result<i64> {
        let now = current_timestamp() as i64;
        conn.execute(
            "INSERT INTO playlists (name, created_at, updated_at) VALUES (?1, ?2, ?3)",
            params![name.trim(), now, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn rename_playlist(conn: &Connection, id: i64, name: &str) -> Result<()> {
        conn.execute(
            "UPDATE playlists SET name = ?1, updated_at = ?2 WHERE id = ?3",
            params![name.trim(), current_timestamp() as i64, id],
        )?;
        Ok(())
    }

    pub fn delete_playlist(conn: &Connection, id: i64) -> Result<()> {
        conn.execute("DELETE FROM playlists WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_playlists(conn: &Connection) -> Result<Vec<Playlist>> {
        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, p.created_at, p.updated_at, COUNT(i.id)
             FROM playlists p
             LEFT JOIN playlist_items i ON i.playlist_id = p.id
             GROUP BY p.id
             ORDER BY p.name COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get::<_, i64>(2)? as u64,
                updated_at: row.get::<_, i64>(3)? as u64,
                item_count: row.get::<_, i64>(4)?.max(0) as u32,
            })
        })?;
        let mut playlists = Vec::new();
        for playlist in rows {
            playlists.push(playlist?);
        }
        Ok(playlists)
    }

    pub fn add_playlist_entry(
        conn: &Connection,
        playlist_id: i64,
        path: &str,
        title: Option<&str>,
        duration_ms: u64,
    ) -> Result<i64> {
        let next_position: i64 = conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_items WHERE playlist_id = ?1",
            params![playlist_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO playlist_items (playlist_id, position, path, title, duration_ms, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                playlist_id,
                next_position,
                path,
                title,
                duration_ms as i64,
                current_timestamp() as i64
            ],
        )?;
        conn.execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![current_timestamp() as i64, playlist_id],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_playlist_entries(
        conn: &Connection,
        playlist_id: i64,
    ) -> Result<Vec<PlaylistEntry>> {
        let mut stmt = conn.prepare(
            "SELECT id, playlist_id, position, path, title, duration_ms, added_at
             FROM playlist_items WHERE playlist_id = ?1 ORDER BY position ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![playlist_id], |row| {
            Ok(PlaylistEntry {
                id: row.get(0)?,
                playlist_id: row.get(1)?,
                position: row.get(2)?,
                path: row.get(3)?,
                title: row.get(4)?,
                duration_ms: row.get::<_, i64>(5)? as u64,
                added_at: row.get::<_, i64>(6)? as u64,
            })
        })?;
        let mut entries = Vec::new();
        for entry in rows {
            entries.push(entry?);
        }
        Ok(entries)
    }

    pub fn remove_playlist_entry(conn: &Connection, playlist_id: i64, entry_id: i64) -> Result<()> {
        conn.execute(
            "DELETE FROM playlist_items WHERE playlist_id = ?1 AND id = ?2",
            params![playlist_id, entry_id],
        )?;
        Self::renumber_playlist(conn, playlist_id)?;
        Ok(())
    }

    pub fn reorder_playlist_entry(
        conn: &Connection,
        playlist_id: i64,
        entry_id: i64,
        new_position: usize,
    ) -> Result<()> {
        let mut ids: Vec<i64> = Self::list_playlist_entries(conn, playlist_id)?
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        let Some(index) = ids.iter().position(|id| *id == entry_id) else {
            return Ok(());
        };
        let id = ids.remove(index);
        let target = new_position.min(ids.len());
        ids.insert(target, id);
        for (position, id) in ids.iter().enumerate() {
            conn.execute(
                "UPDATE playlist_items SET position = ?1 WHERE playlist_id = ?2 AND id = ?3",
                params![position as i64, playlist_id, id],
            )?;
        }
        conn.execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![current_timestamp() as i64, playlist_id],
        )?;
        Ok(())
    }

    pub fn clear_playlist(conn: &Connection, playlist_id: i64) -> Result<()> {
        conn.execute(
            "DELETE FROM playlist_items WHERE playlist_id = ?1",
            params![playlist_id],
        )?;
        conn.execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![current_timestamp() as i64, playlist_id],
        )?;
        Ok(())
    }

    fn renumber_playlist(conn: &Connection, playlist_id: i64) -> Result<()> {
        let ids: Vec<i64> = Self::list_playlist_entries(conn, playlist_id)?
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        for (position, id) in ids.iter().enumerate() {
            conn.execute(
                "UPDATE playlist_items SET position = ?1 WHERE playlist_id = ?2 AND id = ?3",
                params![position as i64, playlist_id, id],
            )?;
        }
        conn.execute(
            "UPDATE playlists SET updated_at = ?1 WHERE id = ?2",
            params![current_timestamp() as i64, playlist_id],
        )?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Tags
    // -------------------------------------------------------------------------

    pub fn get_or_create_tag(conn: &Connection, name: &str) -> Result<i64> {
        let clean = name.trim().to_lowercase();
        conn.execute(
            "INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
            params![clean],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![clean],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn add_tag_to_media(conn: &Connection, media_id: i64, tag_name: &str) -> Result<()> {
        let tag_id = Self::get_or_create_tag(conn, tag_name)?;
        conn.execute(
            "INSERT INTO media_tags (media_id, tag_id) VALUES (?1, ?2) ON CONFLICT(media_id, tag_id) DO NOTHING",
            params![media_id, tag_id],
        )?;
        Ok(())
    }

    pub fn remove_tag_from_media(conn: &Connection, media_id: i64, tag_name: &str) -> Result<()> {
        let clean = tag_name.trim().to_lowercase();
        if let Ok(tag_id) = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![clean],
            |row| row.get::<_, i64>(0),
        ) {
            conn.execute(
                "DELETE FROM media_tags WHERE media_id = ?1 AND tag_id = ?2",
                params![media_id, tag_id],
            )?;
        }
        Ok(())
    }

    pub fn get_tags_for_media(conn: &Connection, media_id: i64) -> Result<Vec<String>> {
        let mut stmt = conn.prepare(
            "SELECT t.name FROM tags t
             JOIN media_tags mt ON t.id = mt.tag_id
             WHERE mt.media_id = ?1 ORDER BY t.name ASC",
        )?;
        let rows = stmt.query_map(params![media_id], |row| row.get(0))?;
        let mut tags = Vec::new();
        for t in rows {
            tags.push(t?);
        }
        Ok(tags)
    }

    // -------------------------------------------------------------------------
    // Network Connections
    // -------------------------------------------------------------------------

    pub fn insert_connection(conn: &Connection, nc: &NetworkConnection) -> Result<i64> {
        conn.execute(
            "INSERT INTO network_connections (name, protocol, host, port, remote_path, auth_type, credential_ref, is_anonymous, auto_connect, last_connected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                nc.name,
                nc.protocol,
                nc.host,
                nc.port as i64,
                nc.remote_path,
                nc.auth_type,
                nc.credential_ref,
                nc.is_anonymous as i32,
                nc.auto_connect as i32,
                nc.last_connected_at as i64,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_connections(conn: &Connection) -> Result<Vec<NetworkConnection>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, protocol, host, port, remote_path, auth_type, credential_ref, is_anonymous, auto_connect, last_connected_at
             FROM network_connections ORDER BY name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(NetworkConnection {
                id: row.get(0)?,
                name: row.get(1)?,
                protocol: row.get(2)?,
                host: row.get(3)?,
                port: row.get::<_, i64>(4)? as u16,
                remote_path: row.get(5)?,
                auth_type: row.get(6)?,
                credential_ref: row.get(7)?,
                is_anonymous: row.get::<_, i32>(8)? != 0,
                auto_connect: row.get::<_, i32>(9)? != 0,
                last_connected_at: row.get::<_, i64>(10)? as u64,
            })
        })?;

        let mut conns = Vec::new();
        for c in rows {
            conns.push(c?);
        }
        Ok(conns)
    }

    pub fn get_connection_by_id(conn: &Connection, id: i64) -> Result<Option<NetworkConnection>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, protocol, host, port, remote_path, auth_type, credential_ref, is_anonymous, auto_connect, last_connected_at
             FROM network_connections WHERE id = ?1",
        )?;
        stmt.query_row(params![id], |row| {
            Ok(NetworkConnection {
                id: row.get(0)?,
                name: row.get(1)?,
                protocol: row.get(2)?,
                host: row.get(3)?,
                port: row.get::<_, i64>(4)? as u16,
                remote_path: row.get(5)?,
                auth_type: row.get(6)?,
                credential_ref: row.get(7)?,
                is_anonymous: row.get::<_, i32>(8)? != 0,
                auto_connect: row.get::<_, i32>(9)? != 0,
                last_connected_at: row.get::<_, i64>(10)? as u64,
            })
        })
        .optional()
    }

    pub fn delete_connection(conn: &Connection, id: i64) -> Result<()> {
        conn.execute("DELETE FROM network_connections WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn find_connection_by_endpoint(
        conn: &Connection,
        protocol: &str,
        host: &str,
        port: u16,
        remote_path: &str,
    ) -> Result<Option<NetworkConnection>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, protocol, host, port, remote_path, auth_type, credential_ref, is_anonymous, auto_connect, last_connected_at
             FROM network_connections
             WHERE LOWER(protocol) = LOWER(?1) AND LOWER(host) = LOWER(?2) AND port = ?3 AND remote_path = ?4
             LIMIT 1",
        )?;
        stmt.query_row(params![protocol, host, port as i64, remote_path], |row| {
            Ok(NetworkConnection {
                id: row.get(0)?,
                name: row.get(1)?,
                protocol: row.get(2)?,
                host: row.get(3)?,
                port: row.get::<_, i64>(4)? as u16,
                remote_path: row.get(5)?,
                auth_type: row.get(6)?,
                credential_ref: row.get(7)?,
                is_anonymous: row.get::<_, i32>(8)? != 0,
                auto_connect: row.get::<_, i32>(9)? != 0,
                last_connected_at: row.get::<_, i64>(10)? as u64,
            })
        })
        .optional()
    }

    // -------------------------------------------------------------------------
    // Complex Library Query & Filter (Joined, Zero N+1 Queries)
    // -------------------------------------------------------------------------

    pub fn query_library(
        conn: &Connection,
        filter: &LibraryFilter,
        sort_field: LibrarySortField,
        sort_dir: SortDirection,
        pagination: Pagination,
    ) -> Result<(Vec<LibraryItemView>, u64)> {
        let mut where_clauses = Vec::new();
        let mut query_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref q) = filter.search_query {
            let sanitized = crate::search::sanitize_fts5_query(q);
            if !sanitized.is_empty() {
                where_clauses.push(
                    "mi.id IN (SELECT rowid FROM media_items_fts WHERE media_items_fts MATCH ?)",
                );
                query_params.push(Box::new(sanitized));
            } else if !q.trim().is_empty() {
                let pattern = format!("%{}%", q.trim());
                where_clauses.push(
                    "(mi.file_name LIKE ? OR mm.title LIKE ? OR mm.artist LIKE ? OR mm.album LIKE ? OR mm.genre LIKE ?)"
                );
                query_params.push(Box::new(pattern.clone()));
                query_params.push(Box::new(pattern.clone()));
                query_params.push(Box::new(pattern.clone()));
                query_params.push(Box::new(pattern.clone()));
                query_params.push(Box::new(pattern));
            }
        }

        if let Some(media_type) = filter.media_type {
            where_clauses.push("mi.media_type = ?");
            query_params.push(Box::new(media_type.as_str().to_string()));
        }

        if let Some(root_id) = filter.root_id {
            where_clauses.push("mi.root_id = ?");
            query_params.push(Box::new(root_id));
        }

        if filter.favourites_only {
            where_clauses.push("fav.media_id IS NOT NULL");
        }

        if let Some(col_id) = filter.collection_id {
            where_clauses.push("ci.collection_id = ?");
            query_params.push(Box::new(col_id));
        }

        if let Some(ref tag_name) = filter.tag {
            where_clauses.push("mi.id IN (SELECT mt.media_id FROM media_tags mt JOIN tags t ON mt.tag_id = t.id WHERE t.name = ?)");
            query_params.push(Box::new(tag_name.trim().to_lowercase()));
        }

        if let Some(ref codec) = filter.codec {
            where_clauses.push("(mm.video_codec = ? OR mm.audio_codec = ?)");
            query_params.push(Box::new(codec.clone()));
            query_params.push(Box::new(codec.clone()));
        }

        if let Some(ref tier) = filter.resolution_tier {
            match tier.as_str() {
                "4K" => where_clauses.push("mm.width >= 3840 OR mm.height >= 2160"),
                "1080p" => where_clauses.push("(mm.width >= 1920 OR mm.height >= 1080) AND mm.width < 3840 AND mm.height < 2160"),
                "720p" => where_clauses.push("(mm.width >= 1280 OR mm.height >= 720) AND mm.width < 1920 AND mm.height < 1080"),
                "SD" => where_clauses.push("mm.width < 1280 AND mm.height < 720 AND mm.width > 0"),
                _ => {}
            }
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        // Determine sorting SQL
        let sort_column = match sort_field {
            LibrarySortField::Title => "COALESCE(mm.title, mi.file_name)",
            LibrarySortField::FileName => "mi.file_name",
            LibrarySortField::DateAdded => "mi.added_time",
            LibrarySortField::DateModified => "mi.modified_time",
            LibrarySortField::Duration => "mm.duration_ms",
            LibrarySortField::FileSize => "mi.file_size",
        };
        let dir_str = match sort_dir {
            SortDirection::Ascending => "ASC",
            SortDirection::Descending => "DESC",
        };

        // 1. Count query with only necessary joins
        let needs_mm = where_sql.contains("mm.");
        let needs_fav = where_sql.contains("fav.");
        let needs_ci = where_sql.contains("ci.");

        let mut count_joins = String::new();
        if needs_mm {
            count_joins.push_str(" LEFT JOIN media_metadata mm ON mi.id = mm.media_id");
        }
        if needs_fav {
            count_joins.push_str(" LEFT JOIN favourites fav ON mi.id = fav.media_id");
        }
        if needs_ci {
            count_joins.push_str(" LEFT JOIN collection_items ci ON mi.id = ci.media_id");
        }

        let count_sql = format!(
            "SELECT COUNT(DISTINCT mi.id)
             FROM media_items mi
             {count_joins}
             {where_sql}"
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> =
            query_params.iter().map(|p| p.as_ref()).collect();
        let total_count: u64 = conn.query_row(
            &count_sql,
            rusqlite::params_from_iter(param_refs.iter().copied()),
            |row| {
                let count: i64 = row.get(0)?;
                Ok(count as u64)
            },
        )?;

        // 2. Select query with pagination
        let ci_join = if needs_ci {
            "LEFT JOIN collection_items ci ON mi.id = ci.media_id"
        } else {
            ""
        };
        let group_by = if needs_ci { "GROUP BY mi.id" } else { "" };

        let select_sql = format!(
            "SELECT
                mi.id, mi.root_id, mi.relative_path, mi.canonical_path, mi.file_name, mi.file_extension, mi.media_type, mi.file_size, mi.modified_time, mi.added_time, mi.last_scanned_at, mi.availability_state, mi.metadata_status,
                mm.title, mm.original_title, mm.description, mm.genre, mm.release_year, mm.duration_ms, mm.overall_bitrate, mm.container, mm.video_codec, mm.audio_codec, mm.width, mm.height, mm.fps, mm.hdr_type, mm.audio_channels, mm.sample_rate, mm.bit_depth, mm.artist, mm.album, mm.album_artist, mm.track_number, mm.disc_number,
                ac.file_path,
                CASE WHEN fav.media_id IS NOT NULL THEN 1 ELSE 0 END as is_fav,
                COALESCE(ws.is_watched, 0)
             FROM media_items mi
             LEFT JOIN media_metadata mm ON mi.id = mm.media_id
             LEFT JOIN artwork_cache ac ON mi.id = ac.media_id
             LEFT JOIN media_watch_state ws ON mi.id = ws.media_id
             LEFT JOIN favourites fav ON mi.id = fav.media_id
             {ci_join}
             {where_sql}
             {group_by}
             ORDER BY {} {}
             LIMIT ? OFFSET ?",
            sort_column, dir_str
        );

        let mut final_params = query_params;
        final_params.push(Box::new(pagination.limit as i64));
        final_params.push(Box::new(pagination.offset as i64));
        let final_refs: Vec<&dyn rusqlite::ToSql> =
            final_params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&select_sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(final_refs), |row| {
            let media_type_str: String = row.get(6)?;
            let avail_str: String = row.get(11)?;
            let meta_status_str: String = row.get(12)?;

            let item = MediaItem {
                id: row.get(0)?,
                root_id: row.get(1)?,
                relative_path: row.get(2)?,
                canonical_path: row.get(3)?,
                file_name: row.get(4)?,
                file_extension: row.get(5)?,
                media_type: MediaType::parse(&media_type_str),
                file_size: row.get::<_, i64>(7)? as u64,
                modified_time: row.get::<_, i64>(8)? as u64,
                added_time: row.get::<_, i64>(9)? as u64,
                last_scanned_at: row.get::<_, i64>(10)? as u64,
                availability_state: AvailabilityState::parse(&avail_str),
                metadata_status: MetadataStatus::parse(&meta_status_str),
            };

            let title: Option<String> = row.get(13)?;
            let metadata = if title.is_some() || row.get::<_, Option<i64>>(18)?.is_some() {
                Some(MediaMetadata {
                    media_id: item.id,
                    title,
                    original_title: row.get(14)?,
                    description: row.get(15)?,
                    genre: row.get(16)?,
                    release_year: row.get(17)?,
                    duration_ms: row.get::<_, Option<i64>>(18)?.unwrap_or(0) as u64,
                    overall_bitrate: row.get::<_, Option<i64>>(19)?.unwrap_or(0) as u64,
                    container: row.get(20)?,
                    video_codec: row.get(21)?,
                    audio_codec: row.get(22)?,
                    width: row.get::<_, Option<i64>>(23)?.unwrap_or(0) as u32,
                    height: row.get::<_, Option<i64>>(24)?.unwrap_or(0) as u32,
                    fps: row.get::<_, Option<f64>>(25)?.unwrap_or(0.0),
                    hdr_type: row.get(26)?,
                    audio_channels: row.get::<_, Option<i64>>(27)?.unwrap_or(0) as u32,
                    sample_rate: row.get::<_, Option<i64>>(28)?.unwrap_or(0) as u32,
                    bit_depth: row.get::<_, Option<i64>>(29)?.unwrap_or(0) as u32,
                    artist: row.get(30)?,
                    album: row.get(31)?,
                    album_artist: row.get(32)?,
                    track_number: row.get(33)?,
                    disc_number: row.get(34)?,
                })
            } else {
                None
            };

            let thumbnail_path: Option<String> = row.get(35)?;
            let is_favourite: bool = row.get::<_, i32>(36)? != 0;
            let is_watched: bool = row.get::<_, i32>(37)? != 0;

            Ok(LibraryItemView {
                item,
                metadata,
                thumbnail_path,
                is_favourite,
                is_watched,
                tags: Vec::new(), // Populated if needed
            })
        })?;

        let mut items = Vec::new();
        for item in rows {
            items.push(item?);
        }

        Ok((items, total_count))
    }

    // -------------------------------------------------------------------------
    // User Bookmarks
    // -------------------------------------------------------------------------

    pub fn insert_bookmark(
        conn: &Connection,
        media_id: i64,
        timestamp_ms: i64,
        label: &str,
        note: Option<&str>,
    ) -> Result<i64> {
        let now = current_timestamp() as i64;
        conn.execute(
            "INSERT INTO user_bookmarks (media_id, timestamp_ms, label, note, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![media_id, timestamp_ms, label, note, now, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_bookmarks_for_media(conn: &Connection, media_id: i64) -> Result<Vec<Bookmark>> {
        let mut stmt = conn.prepare(
            "SELECT id, media_id, timestamp_ms, label, note, created_at, updated_at
             FROM user_bookmarks
             WHERE media_id = ?1
             ORDER BY timestamp_ms ASC",
        )?;

        let rows = stmt.query_map(params![media_id], |row| {
            Ok(Bookmark {
                id: row.get(0)?,
                media_id: row.get(1)?,
                timestamp_ms: row.get(2)?,
                label: row.get(3)?,
                note: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;

        let mut bookmarks = Vec::new();
        for b in rows {
            bookmarks.push(b?);
        }
        Ok(bookmarks)
    }

    pub fn get_bookmark(conn: &Connection, id: i64) -> Result<Option<Bookmark>> {
        let mut stmt = conn.prepare(
            "SELECT id, media_id, timestamp_ms, label, note, created_at, updated_at
             FROM user_bookmarks
             WHERE id = ?1",
        )?;

        stmt.query_row(params![id], |row| {
            Ok(Bookmark {
                id: row.get(0)?,
                media_id: row.get(1)?,
                timestamp_ms: row.get(2)?,
                label: row.get(3)?,
                note: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .optional()
    }

    pub fn update_bookmark(
        conn: &Connection,
        id: i64,
        label: &str,
        note: Option<&str>,
    ) -> Result<bool> {
        let now = current_timestamp() as i64;
        let rows = conn.execute(
            "UPDATE user_bookmarks SET label = ?1, note = ?2, updated_at = ?3 WHERE id = ?4",
            params![label, note, now, id],
        )?;
        Ok(rows > 0)
    }

    pub fn delete_bookmark(conn: &Connection, id: i64) -> Result<bool> {
        let rows = conn.execute("DELETE FROM user_bookmarks WHERE id = ?1", params![id])?;
        Ok(rows > 0)
    }

    // -------------------------------------------------------------------------
    // SSH Host Trust Management (Phase 1.2)
    // -------------------------------------------------------------------------

    pub fn get_ssh_host_trust(
        conn: &Connection,
        host: &str,
        port: u16,
    ) -> Result<Option<SshHostTrustRecord>> {
        let norm_host = SshHostTrustRecord::normalize_host(host);
        let mut stmt = conn.prepare(
            "SELECT id, host, port, key_algorithm, fingerprint_sha256, trusted_at, last_seen_at, notes
             FROM ssh_host_trust
             WHERE host = ?1 AND port = ?2",
        )?;

        stmt.query_row(params![norm_host, port], |row| {
            Ok(SshHostTrustRecord {
                id: row.get(0)?,
                host: row.get(1)?,
                port: row.get(2)?,
                key_algorithm: row.get(3)?,
                fingerprint_sha256: row.get(4)?,
                trusted_at: row.get(5)?,
                last_seen_at: row.get(6)?,
                notes: row.get(7)?,
            })
        })
        .optional()
    }

    pub fn record_ssh_host_trust(
        conn: &Connection,
        host: &str,
        port: u16,
        key_algorithm: &str,
        fingerprint_sha256: &str,
        notes: Option<&str>,
    ) -> Result<SshHostTrustRecord> {
        let norm_host = SshHostTrustRecord::normalize_host(host);
        let now = current_timestamp() as i64;
        let clean_fp = fingerprint_sha256.trim().to_string();

        conn.execute(
            "INSERT INTO ssh_host_trust (host, port, key_algorithm, fingerprint_sha256, trusted_at, last_seen_at, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(host, port) DO UPDATE SET
                key_algorithm = excluded.key_algorithm,
                fingerprint_sha256 = excluded.fingerprint_sha256,
                last_seen_at = excluded.last_seen_at,
                notes = excluded.notes",
            params![norm_host, port, key_algorithm, clean_fp, now, now, notes],
        )?;

        let id = conn.last_insert_rowid();
        Ok(SshHostTrustRecord {
            id,
            host: norm_host,
            port,
            key_algorithm: key_algorithm.to_string(),
            fingerprint_sha256: clean_fp,
            trusted_at: now,
            last_seen_at: now,
            notes: notes.map(|s| s.to_string()),
        })
    }

    pub fn update_ssh_host_last_seen(conn: &Connection, host: &str, port: u16) -> Result<bool> {
        let norm_host = SshHostTrustRecord::normalize_host(host);
        let now = current_timestamp() as i64;
        let rows = conn.execute(
            "UPDATE ssh_host_trust SET last_seen_at = ?1 WHERE host = ?2 AND port = ?3",
            params![now, norm_host, port],
        )?;
        Ok(rows > 0)
    }

    pub fn revoke_ssh_host_trust(conn: &Connection, host: &str, port: u16) -> Result<bool> {
        let norm_host = SshHostTrustRecord::normalize_host(host);
        let rows = conn.execute(
            "DELETE FROM ssh_host_trust WHERE host = ?1 AND port = ?2",
            params![norm_host, port],
        )?;
        Ok(rows > 0)
    }

    pub fn list_ssh_host_trust(conn: &Connection) -> Result<Vec<SshHostTrustRecord>> {
        let mut stmt = conn.prepare(
            "SELECT id, host, port, key_algorithm, fingerprint_sha256, trusted_at, last_seen_at, notes
             FROM ssh_host_trust
             ORDER BY host ASC, port ASC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(SshHostTrustRecord {
                id: row.get(0)?,
                host: row.get(1)?,
                port: row.get(2)?,
                key_algorithm: row.get(3)?,
                fingerprint_sha256: row.get(4)?,
                trusted_at: row.get(5)?,
                last_seen_at: row.get(6)?,
                notes: row.get(7)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    // -------------------------------------------------------------------------
    // Persistent Active Playback Queue
    // -------------------------------------------------------------------------

    pub fn save_active_queue(conn: &mut Connection, items: &[ActiveQueueEntry]) -> Result<()> {
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM active_playback_queue", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO active_playback_queue (position, path, title, duration_ms, is_current)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for item in items {
                stmt.execute(params![
                    item.position as i64,
                    item.path,
                    item.title,
                    item.duration_ms as i64,
                    item.is_current as i32,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_active_queue(conn: &Connection) -> Result<Vec<ActiveQueueEntry>> {
        let mut stmt = conn.prepare(
            "SELECT position, path, title, duration_ms, is_current
             FROM active_playback_queue
             ORDER BY position ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ActiveQueueEntry {
                position: row.get::<_, i64>(0)? as usize,
                path: row.get(1)?,
                title: row.get(2)?,
                duration_ms: row.get::<_, i64>(3)? as u64,
                is_current: row.get::<_, i32>(4)? != 0,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn clear_active_queue(conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM active_playback_queue", [])?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Smart Collections (Phase E3.1)
    // -------------------------------------------------------------------------

    pub fn create_smart_collection(conn: &Connection, sc: &SmartCollection) -> Result<i64> {
        let rules_json = serde_json::to_string(&sc.rules)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
        let now = current_timestamp() as i64;

        conn.execute(
            "INSERT INTO smart_collections (name, description, icon, rule_definition, combination_mode, sort_field, sort_direction, display_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                sc.name,
                sc.description,
                sc.icon,
                rules_json,
                sc.combination_mode.as_str(),
                sc.sort_field,
                sc.sort_direction,
                sc.display_order,
                now,
                now,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_smart_collection_by_id(
        conn: &Connection,
        id: i64,
    ) -> Result<Option<SmartCollection>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, icon, rule_definition, combination_mode, sort_field, sort_direction, display_order, created_at, updated_at
             FROM smart_collections WHERE id = ?1",
        )?;
        stmt.query_row(params![id], |row| {
            let rules_raw: String = row.get(4)?;
            let rules: RuleDefinition = serde_json::from_str(&rules_raw).unwrap_or_default();
            let mode_str: String = row.get(5)?;
            Ok(SmartCollection {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                icon: row.get(3)?,
                rules,
                combination_mode: RuleCombinationMode::parse(&mode_str),
                sort_field: row.get(6)?,
                sort_direction: row.get(7)?,
                display_order: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })
        .optional()
    }

    pub fn get_smart_collection_by_name(
        conn: &Connection,
        name: &str,
    ) -> Result<Option<SmartCollection>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, icon, rule_definition, combination_mode, sort_field, sort_direction, display_order, created_at, updated_at
             FROM smart_collections WHERE name = ?1",
        )?;
        stmt.query_row(params![name], |row| {
            let rules_raw: String = row.get(4)?;
            let rules: RuleDefinition = serde_json::from_str(&rules_raw).unwrap_or_default();
            let mode_str: String = row.get(5)?;
            Ok(SmartCollection {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                icon: row.get(3)?,
                rules,
                combination_mode: RuleCombinationMode::parse(&mode_str),
                sort_field: row.get(6)?,
                sort_direction: row.get(7)?,
                display_order: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })
        .optional()
    }

    pub fn update_smart_collection(conn: &Connection, sc: &SmartCollection) -> Result<()> {
        let rules_json = serde_json::to_string(&sc.rules)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
        let now = current_timestamp() as i64;

        conn.execute(
            "UPDATE smart_collections SET
                name = ?1,
                description = ?2,
                icon = ?3,
                rule_definition = ?4,
                combination_mode = ?5,
                sort_field = ?6,
                sort_direction = ?7,
                display_order = ?8,
                updated_at = ?9
             WHERE id = ?10",
            params![
                sc.name,
                sc.description,
                sc.icon,
                rules_json,
                sc.combination_mode.as_str(),
                sc.sort_field,
                sc.sort_direction,
                sc.display_order,
                now,
                sc.id,
            ],
        )?;
        Ok(())
    }

    pub fn delete_smart_collection(conn: &Connection, id: i64) -> Result<()> {
        conn.execute("DELETE FROM smart_collections WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_smart_collections(conn: &Connection) -> Result<Vec<SmartCollection>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, icon, rule_definition, combination_mode, sort_field, sort_direction, display_order, created_at, updated_at
             FROM smart_collections ORDER BY display_order ASC, name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let rules_raw: String = row.get(4)?;
            let rules: RuleDefinition = serde_json::from_str(&rules_raw).unwrap_or_default();
            let mode_str: String = row.get(5)?;
            Ok(SmartCollection {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                icon: row.get(3)?,
                rules,
                combination_mode: RuleCombinationMode::parse(&mode_str),
                sort_field: row.get(6)?,
                sort_direction: row.get(7)?,
                display_order: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn count_smart_collection_items(conn: &Connection, sc: &SmartCollection) -> Result<u64> {
        let compiled = compile_smart_rules(&sc.rules)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;

        let count_sql = format!(
            "SELECT COUNT(DISTINCT mi.id)
             FROM media_items mi
             LEFT JOIN media_metadata mm ON mi.id = mm.media_id
             LEFT JOIN media_watch_state ws ON mi.id = ws.media_id
             LEFT JOIN favourites fav ON mi.id = fav.media_id
             WHERE {}",
            compiled.where_clause
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> =
            compiled.params.iter().map(|p| p.as_ref()).collect();

        let count: i64 =
            conn.query_row(&count_sql, rusqlite::params_from_iter(param_refs), |row| {
                row.get(0)
            })?;
        Ok(count as u64)
    }

    pub fn query_smart_collection_items(
        conn: &Connection,
        sc: &SmartCollection,
        pagination: Pagination,
    ) -> Result<(Vec<LibraryItemView>, u64)> {
        let compiled = compile_smart_rules(&sc.rules)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;

        let count_sql = format!(
            "SELECT COUNT(DISTINCT mi.id)
             FROM media_items mi
             LEFT JOIN media_metadata mm ON mi.id = mm.media_id
             LEFT JOIN media_watch_state ws ON mi.id = ws.media_id
             LEFT JOIN favourites fav ON mi.id = fav.media_id
             WHERE {}",
            compiled.where_clause
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> =
            compiled.params.iter().map(|p| p.as_ref()).collect();

        let total_count: u64 = conn.query_row(
            &count_sql,
            rusqlite::params_from_iter(param_refs.iter().copied()),
            |row| {
                let count: i64 = row.get(0)?;
                Ok(count as u64)
            },
        )?;

        let sort_column = sc
            .sort_field
            .as_deref()
            .unwrap_or("COALESCE(mm.title, mi.file_name)");
        let sort_dir = sc.sort_direction.as_deref().unwrap_or("ASC");

        let select_sql = format!(
            "SELECT
                mi.id, mi.root_id, mi.relative_path, mi.canonical_path, mi.file_name, mi.file_extension, mi.media_type, mi.file_size, mi.modified_time, mi.added_time, mi.last_scanned_at, mi.availability_state, mi.metadata_status,
                mm.title, mm.original_title, mm.description, mm.genre, mm.release_year, mm.duration_ms, mm.overall_bitrate, mm.container, mm.video_codec, mm.audio_codec, mm.width, mm.height, mm.fps, mm.hdr_type, mm.audio_channels, mm.sample_rate, mm.bit_depth, mm.artist, mm.album, mm.album_artist, mm.track_number, mm.disc_number,
                ac.file_path,
                CASE WHEN fav.media_id IS NOT NULL THEN 1 ELSE 0 END as is_fav,
                COALESCE(ws.is_watched, 0)
             FROM media_items mi
             LEFT JOIN media_metadata mm ON mi.id = mm.media_id
             LEFT JOIN artwork_cache ac ON mi.id = ac.media_id
             LEFT JOIN media_watch_state ws ON mi.id = ws.media_id
             LEFT JOIN favourites fav ON mi.id = fav.media_id
             WHERE {}
             GROUP BY mi.id
             ORDER BY {} {}
             LIMIT ? OFFSET ?",
            compiled.where_clause, sort_column, sort_dir
        );

        let mut final_params = compiled.params;
        final_params.push(Box::new(pagination.limit as i64));
        final_params.push(Box::new(pagination.offset as i64));
        let final_refs: Vec<&dyn rusqlite::ToSql> =
            final_params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&select_sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(final_refs), |row| {
            let media_type_str: String = row.get(6)?;
            let avail_str: String = row.get(11)?;
            let meta_status_str: String = row.get(12)?;

            let item = MediaItem {
                id: row.get(0)?,
                root_id: row.get(1)?,
                relative_path: row.get(2)?,
                canonical_path: row.get(3)?,
                file_name: row.get(4)?,
                file_extension: row.get(5)?,
                media_type: MediaType::parse(&media_type_str),
                file_size: row.get::<_, i64>(7)? as u64,
                modified_time: row.get::<_, i64>(8)? as u64,
                added_time: row.get::<_, i64>(9)? as u64,
                last_scanned_at: row.get::<_, i64>(10)? as u64,
                availability_state: AvailabilityState::parse(&avail_str),
                metadata_status: MetadataStatus::parse(&meta_status_str),
            };

            let title: Option<String> = row.get(13)?;
            let metadata = if title.is_some() || row.get::<_, Option<i64>>(18)?.is_some() {
                Some(MediaMetadata {
                    media_id: item.id,
                    title,
                    original_title: row.get(14)?,
                    description: row.get(15)?,
                    genre: row.get(16)?,
                    release_year: row.get(17)?,
                    duration_ms: row.get::<_, Option<i64>>(18)?.unwrap_or(0) as u64,
                    overall_bitrate: row.get::<_, Option<i64>>(19)?.unwrap_or(0) as u64,
                    container: row.get(20)?,
                    video_codec: row.get(21)?,
                    audio_codec: row.get(22)?,
                    width: row.get::<_, Option<i64>>(23)?.unwrap_or(0) as u32,
                    height: row.get::<_, Option<i64>>(24)?.unwrap_or(0) as u32,
                    fps: row.get::<_, Option<f64>>(25)?.unwrap_or(0.0),
                    hdr_type: row.get(26)?,
                    audio_channels: row.get::<_, Option<i64>>(27)?.unwrap_or(0) as u32,
                    sample_rate: row.get::<_, Option<i64>>(28)?.unwrap_or(0) as u32,
                    bit_depth: row.get::<_, Option<i64>>(29)?.unwrap_or(0) as u32,
                    artist: row.get(30)?,
                    album: row.get(31)?,
                    album_artist: row.get(32)?,
                    track_number: row.get(33)?,
                    disc_number: row.get(34)?,
                })
            } else {
                None
            };

            let thumbnail_path: Option<String> = row.get(35)?;
            let is_favourite: bool = row.get::<_, i32>(36)? != 0;
            let is_watched: bool = row.get::<_, i32>(37)? != 0;

            Ok(LibraryItemView {
                item,
                metadata,
                thumbnail_path,
                is_favourite,
                is_watched,
                tags: Vec::new(),
            })
        })?;

        let mut items = Vec::new();
        for r in rows {
            items.push(r?);
        }
        Ok((items, total_count))
    }

    // -------------------------------------------------------------------------
    // Batch Metadata Updates (Phase E3.2)
    // -------------------------------------------------------------------------

    pub fn batch_update_metadata(
        conn: &mut Connection,
        media_ids: &[i64],
        request: &BatchMetadataRequest,
    ) -> Result<BatchUpdateResult> {
        let tx = conn.transaction()?;
        let res = execute_batch_metadata_update(&tx, media_ids, request)?;
        tx.commit()?;
        Ok(res)
    }

    // -------------------------------------------------------------------------
    // Missing Media Integrity, Relinking & Cleanup (Phase E3.3)
    // -------------------------------------------------------------------------

    pub fn scan_library_integrity(
        conn: &Connection,
        cancel_flag: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<IntegrityScanProgress> {
        super::integrity::scan_library_integrity(conn, cancel_flag)
    }

    pub fn relink_media_item(
        conn: &Connection,
        media_id: i64,
        replacement_path: &std::path::Path,
    ) -> std::result::Result<MediaItem, String> {
        super::integrity::relink_media_item(conn, media_id, replacement_path)
    }

    pub fn generate_cleanup_preview(conn: &Connection) -> Result<CleanupPreview> {
        super::integrity::generate_cleanup_preview(conn)
    }

    pub fn get_unavailable_media_items(conn: &Connection) -> Result<Vec<LibraryItemView>> {
        super::integrity::query_missing_media_items(conn)
    }

    pub fn execute_catalog_cleanup(
        conn: &mut Connection,
        preview: &CleanupPreview,
    ) -> Result<usize> {
        let tx = conn.transaction()?;
        let removed = super::integrity::execute_catalog_cleanup(&tx, preview)?;
        tx.commit()?;
        Ok(removed)
    }

    // -------------------------------------------------------------------------
    // User Playback Profiles (Phase E7)
    // -------------------------------------------------------------------------

    pub fn insert_profile(conn: &Connection, profile: &PlaybackProfile) -> Result<i64> {
        conn.execute(
            "INSERT INTO profiles (
                name, description, is_active, volume, audio_delay_ms,
                equalizer_preset, equalizer_bands, loudness_target,
                sub_font_size, sub_color, sub_bold, shader_preset,
                settings_json, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                profile.name,
                profile.description,
                if profile.is_active { 1 } else { 0 },
                profile.volume,
                profile.audio_delay_ms,
                profile.equalizer_preset,
                profile.equalizer_bands,
                profile.loudness_target,
                profile.sub_font_size,
                profile.sub_color,
                if profile.sub_bold { 1 } else { 0 },
                profile.shader_preset,
                profile.settings_json,
                profile.created_at,
                profile.updated_at,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_profile_by_id(conn: &Connection, id: i64) -> Result<Option<PlaybackProfile>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, is_active, volume, audio_delay_ms,
                    equalizer_preset, equalizer_bands, loudness_target,
                    sub_font_size, sub_color, sub_bold, shader_preset,
                    settings_json, created_at, updated_at
             FROM profiles WHERE id = ?1",
        )?;

        let mut rows = stmt.query(rusqlite::params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_profile(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_profile_by_name(conn: &Connection, name: &str) -> Result<Option<PlaybackProfile>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, is_active, volume, audio_delay_ms,
                    equalizer_preset, equalizer_bands, loudness_target,
                    sub_font_size, sub_color, sub_bold, shader_preset,
                    settings_json, created_at, updated_at
             FROM profiles WHERE name = ?1",
        )?;

        let mut rows = stmt.query(rusqlite::params![name])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_profile(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn get_active_profile(conn: &Connection) -> Result<Option<PlaybackProfile>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, is_active, volume, audio_delay_ms,
                    equalizer_preset, equalizer_bands, loudness_target,
                    sub_font_size, sub_color, sub_bold, shader_preset,
                    settings_json, created_at, updated_at
             FROM profiles WHERE is_active = 1
             ORDER BY id ASC LIMIT 1",
        )?;

        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_profile(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_profiles(conn: &Connection) -> Result<Vec<PlaybackProfile>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, is_active, volume, audio_delay_ms,
                    equalizer_preset, equalizer_bands, loudness_target,
                    sub_font_size, sub_color, sub_bold, shader_preset,
                    settings_json, created_at, updated_at
             FROM profiles ORDER BY id ASC",
        )?;

        let profile_iter = stmt.query_map([], Self::row_to_profile)?;
        let mut list = Vec::new();
        for p in profile_iter {
            list.push(p?);
        }
        Ok(list)
    }

    pub fn set_active_profile(conn: &mut Connection, id: i64) -> Result<()> {
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM profiles WHERE id = ?1)",
            rusqlite::params![id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        tx.execute("UPDATE profiles SET is_active = 0", [])?;
        let affected = tx.execute(
            "UPDATE profiles SET is_active = 1 WHERE id = ?1",
            rusqlite::params![id],
        )?;
        if affected == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        tx.commit()?;
        Ok(())
    }

    pub fn update_profile(conn: &Connection, profile: &PlaybackProfile) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            "UPDATE profiles SET
                name = ?1,
                description = ?2,
                volume = ?3,
                audio_delay_ms = ?4,
                equalizer_preset = ?5,
                equalizer_bands = ?6,
                loudness_target = ?7,
                sub_font_size = ?8,
                sub_color = ?9,
                sub_bold = ?10,
                shader_preset = ?11,
                settings_json = ?12,
                updated_at = ?13
             WHERE id = ?14",
            rusqlite::params![
                profile.name,
                profile.description,
                profile.volume,
                profile.audio_delay_ms,
                profile.equalizer_preset,
                profile.equalizer_bands,
                profile.loudness_target,
                profile.sub_font_size,
                profile.sub_color,
                if profile.sub_bold { 1 } else { 0 },
                profile.shader_preset,
                profile.settings_json,
                now,
                profile.id,
            ],
        )?;
        Ok(())
    }

    pub fn delete_profile(conn: &Connection, id: i64) -> Result<bool> {
        let is_default: bool = conn
            .query_row(
                "SELECT (name = 'Default' OR id = 1) FROM profiles WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if is_default {
            return Ok(false);
        }

        let affected = conn.execute("DELETE FROM profiles WHERE id = ?1", rusqlite::params![id])?;
        Ok(affected > 0)
    }

    fn row_to_profile(row: &rusqlite::Row) -> Result<PlaybackProfile> {
        Ok(PlaybackProfile {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            is_active: row.get::<_, i64>(3)? != 0,
            volume: row.get(4)?,
            audio_delay_ms: row.get(5)?,
            equalizer_preset: row.get(6)?,
            equalizer_bands: row.get(7)?,
            loudness_target: row.get(8)?,
            sub_font_size: row.get(9)?,
            sub_color: row.get(10)?,
            sub_bold: row.get::<_, i64>(11)? != 0,
            shader_preset: row.get(12)?,
            settings_json: row.get(13)?,
            created_at: row.get(14)?,
            updated_at: row.get(15)?,
        })
    }
}
