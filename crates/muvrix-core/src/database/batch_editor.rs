//! Multi-selection Batch Metadata Editor data models, mixed-value detection, and transactional updates.

use super::models::MediaMetadata;
use rusqlite::{params, Result, Transaction};
use serde::{Deserialize, Serialize};

/// Action to perform on a specific metadata field in a batch edit operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FieldAction<T> {
    #[default]
    Keep,
    Set(T),
    Clear,
}

impl<T> FieldAction<T> {
    pub fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }

    pub fn is_modified(&self) -> bool {
        !self.is_keep()
    }
}

/// Mixed-value summary for a collection of selected media items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldMixedState<T> {
    Uniform(Option<T>),
    Mixed,
}

impl<T> Default for FieldMixedState<T> {
    fn default() -> Self {
        Self::Uniform(None)
    }
}

impl<T> FieldMixedState<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Uniform(Some(v)) => Some(v),
            _ => None,
        }
    }

    pub fn is_mixed(&self) -> bool {
        matches!(self, Self::Mixed)
    }
}

/// Consolidated mixed-value state across all editable metadata fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SelectedMetadataSummary {
    pub count: usize,
    pub title: FieldMixedState<String>,
    pub artist: FieldMixedState<String>,
    pub album: FieldMixedState<String>,
    pub album_artist: FieldMixedState<String>,
    pub genre: FieldMixedState<String>,
    pub release_year: FieldMixedState<i32>,
    pub track_number: FieldMixedState<i32>,
    pub disc_number: FieldMixedState<i32>,
}

impl SelectedMetadataSummary {
    /// Analyzes a slice of metadata records and identifies uniform vs mixed values.
    pub fn analyze(items: &[Option<MediaMetadata>]) -> Self {
        let count = items.len();
        if count == 0 {
            return Self {
                count: 0,
                title: FieldMixedState::Uniform(None),
                artist: FieldMixedState::Uniform(None),
                album: FieldMixedState::Uniform(None),
                album_artist: FieldMixedState::Uniform(None),
                genre: FieldMixedState::Uniform(None),
                release_year: FieldMixedState::Uniform(None),
                track_number: FieldMixedState::Uniform(None),
                disc_number: FieldMixedState::Uniform(None),
            };
        }

        fn evaluate_opt<T: Clone + PartialEq>(
            items: &[Option<MediaMetadata>],
            f: impl Fn(&MediaMetadata) -> Option<T>,
        ) -> FieldMixedState<T> {
            let mut first_val: Option<Option<T>> = None;
            for item in items {
                let val = item.as_ref().and_then(&f);
                match &first_val {
                    None => first_val = Some(val),
                    Some(first) => {
                        if first != &val {
                            return FieldMixedState::Mixed;
                        }
                    }
                }
            }
            FieldMixedState::Uniform(first_val.flatten())
        }

        Self {
            count,
            title: evaluate_opt(items, |m| m.title.clone()),
            artist: evaluate_opt(items, |m| m.artist.clone()),
            album: evaluate_opt(items, |m| m.album.clone()),
            album_artist: evaluate_opt(items, |m| m.album_artist.clone()),
            genre: evaluate_opt(items, |m| m.genre.clone()),
            release_year: evaluate_opt(items, |m| m.release_year),
            track_number: evaluate_opt(items, |m| m.track_number),
            disc_number: evaluate_opt(items, |m| m.disc_number),
        }
    }
}

/// Specifications for an atomic batch metadata update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BatchMetadataRequest {
    pub title: FieldAction<String>,
    pub artist: FieldAction<String>,
    pub album: FieldAction<String>,
    pub album_artist: FieldAction<String>,
    pub genre: FieldAction<String>,
    pub release_year: FieldAction<i32>,
    pub track_number: FieldAction<i32>,
    pub disc_number: FieldAction<i32>,
}

impl BatchMetadataRequest {
    pub fn is_empty(&self) -> bool {
        self.title.is_keep()
            && self.artist.is_keep()
            && self.album.is_keep()
            && self.album_artist.is_keep()
            && self.genre.is_keep()
            && self.release_year.is_keep()
            && self.track_number.is_keep()
            && self.disc_number.is_keep()
    }

    /// List of field names that are modified in this request.
    pub fn modified_fields(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if self.title.is_modified() {
            fields.push("Title");
        }
        if self.artist.is_modified() {
            fields.push("Artist");
        }
        if self.album.is_modified() {
            fields.push("Album");
        }
        if self.album_artist.is_modified() {
            fields.push("Album Artist");
        }
        if self.genre.is_modified() {
            fields.push("Genre");
        }
        if self.release_year.is_modified() {
            fields.push("Release Year");
        }
        if self.track_number.is_modified() {
            fields.push("Track Number");
        }
        if self.disc_number.is_modified() {
            fields.push("Disc Number");
        }
        fields
    }
}

/// Result of an executed batch update operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchUpdateResult {
    pub affected_items: usize,
    pub modified_fields: Vec<&'static str>,
}

/// Executes an atomic batch metadata update across multiple media items inside a transaction.
///
/// Unmodified fields are strictly preserved. FTS5 full-text index synchronization is maintained.
pub fn execute_batch_metadata_update(
    tx: &Transaction,
    media_ids: &[i64],
    request: &BatchMetadataRequest,
) -> Result<BatchUpdateResult> {
    if media_ids.is_empty() || request.is_empty() {
        return Ok(BatchUpdateResult {
            affected_items: 0,
            modified_fields: Vec::new(),
        });
    }

    let modified_fields = request.modified_fields();

    for &id in media_ids {
        // Ensure parent record exists in media_metadata
        tx.execute(
            "INSERT OR IGNORE INTO media_metadata (media_id, duration_ms) VALUES (?1, 0)",
            params![id],
        )?;

        let mut set_clauses = Vec::new();
        let mut query_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        macro_rules! handle_field {
            ($action:expr, $col:expr) => {
                match &$action {
                    FieldAction::Keep => {}
                    FieldAction::Set(val) => {
                        set_clauses.push(format!("{} = ?", $col));
                        query_params.push(Box::new(val.clone()));
                    }
                    FieldAction::Clear => {
                        set_clauses.push(format!("{} = NULL", $col));
                    }
                }
            };
        }

        handle_field!(request.title, "title");
        handle_field!(request.artist, "artist");
        handle_field!(request.album, "album");
        handle_field!(request.album_artist, "album_artist");
        handle_field!(request.genre, "genre");
        handle_field!(request.release_year, "release_year");
        handle_field!(request.track_number, "track_number");
        handle_field!(request.disc_number, "disc_number");

        if !set_clauses.is_empty() {
            let sql = format!(
                "UPDATE media_metadata SET {} WHERE media_id = ?",
                set_clauses.join(", ")
            );
            query_params.push(Box::new(id));

            let refs: Vec<&dyn rusqlite::ToSql> = query_params.iter().map(|p| p.as_ref()).collect();
            tx.execute(&sql, rusqlite::params_from_iter(refs))?;

            // Mark media item metadata status as extracted
            tx.execute(
                "UPDATE media_items SET metadata_status = 'extracted' WHERE id = ?1",
                params![id],
            )?;
        }
    }

    Ok(BatchUpdateResult {
        affected_items: media_ids.len(),
        modified_fields,
    })
}
