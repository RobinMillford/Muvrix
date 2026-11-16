use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};

use eframe::egui;
use muvrix_core::database::Pagination;
use muvrix_core::scanner::{ScanOptions, ScannerService};

use crate::app::MuvrixApp;
use crate::library_enrichment::{enrich_pending, EnrichmentProgress};

impl MuvrixApp {
    pub fn refresh_library(&mut self) {
        let pagination = Pagination {
            offset: self.library_page * 24,
            limit: 24,
        };
        if let Ok((items, total)) = self.db.query_library(
            &self.library_filter,
            self.library_sort_field,
            self.library_sort_dir,
            pagination,
        ) {
            self.library_items = items;
            self.library_total_count = total;
        }
    }

    /// Performs global search across all library items and playlists.
    /// Results are stored in `global_search_videos`, `global_search_audio`,
    /// and `global_search_playlists`.
    pub fn run_global_search(&mut self, query: &str) {
        use muvrix_core::database::{
            LibraryFilter, LibrarySortField, MediaType, Pagination, SortDirection,
        };
        let q = query.trim().to_string();
        if q.is_empty() {
            self.global_search_videos.clear();
            self.global_search_audio.clear();
            self.global_search_playlists.clear();
            self.global_search_last_query.clear();
            return;
        }
        // Video results
        let video_filter = LibraryFilter {
            search_query: Some(q.clone()),
            media_type: Some(MediaType::Video),
            ..Default::default()
        };
        if let Ok((items, _)) = self.db.query_library(
            &video_filter,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        ) {
            self.global_search_videos = items;
        }
        // Audio results
        let audio_filter = LibraryFilter {
            search_query: Some(q.clone()),
            media_type: Some(MediaType::Audio),
            ..Default::default()
        };
        if let Ok((items, _)) = self.db.query_library(
            &audio_filter,
            LibrarySortField::Title,
            SortDirection::Ascending,
            Pagination {
                offset: 0,
                limit: 10,
            },
        ) {
            self.global_search_audio = items;
        }
        // Playlist results (filter by name)
        let lower_q = q.to_ascii_lowercase();
        self.global_search_playlists = self
            .db
            .list_playlists()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.name.to_ascii_lowercase().contains(&lower_q))
            .collect();
        self.global_search_last_query = q;
    }

    /// Loads quick-access home sections (recently added, favorites, longest, largest).
    /// Only executes once per session unless `force_reload` is true.
    pub fn load_home_sections(&mut self, force_reload: bool) {
        use muvrix_core::database::{LibraryFilter, LibrarySortField, Pagination, SortDirection};
        if self.home_sections_loaded && !force_reload {
            return;
        }
        let pagination = Pagination {
            offset: 0,
            limit: 8,
        };

        // Recently Added
        if let Ok((items, _)) = self.db.query_library(
            &LibraryFilter::default(),
            LibrarySortField::DateAdded,
            SortDirection::Descending,
            pagination,
        ) {
            self.home_recently_added = items;
        }

        // Favorites
        if let Ok((items, _)) = self.db.query_library(
            &LibraryFilter {
                favourites_only: true,
                ..Default::default()
            },
            LibrarySortField::Title,
            SortDirection::Ascending,
            pagination,
        ) {
            self.home_favorites = items;
        }

        // Longest
        if let Ok((items, _)) = self.db.query_library(
            &LibraryFilter::default(),
            LibrarySortField::Duration,
            SortDirection::Descending,
            pagination,
        ) {
            self.home_longest = items;
        }

        // Largest
        if let Ok((items, _)) = self.db.query_library(
            &LibraryFilter::default(),
            LibrarySortField::FileSize,
            SortDirection::Descending,
            pagination,
        ) {
            self.home_largest = items;
        }

        self.home_sections_loaded = true;
    }

    pub fn start_library_scan(&mut self, root_id: i64) {
        if self.active_scan_cancel.is_some() {
            return;
        }
        self.cancel_library_enrichment();
        let root = match self.db.get_root_by_id(root_id) {
            Ok(Some(r)) => r,
            _ => return,
        };

        let cancel_token = Arc::new(AtomicBool::new(false));
        self.active_scan_cancel = Some(cancel_token.clone());

        let (tx, rx) = mpsc::channel();
        self.active_scan_rx = Some(rx);
        let db_clone = self.db.clone();

        std::thread::spawn(move || {
            let options = ScanOptions::default();
            let _ = ScannerService::scan_root(&db_clone, &root, &options, cancel_token, Some(&tx));
        });
    }

    pub fn cancel_library_scan(&mut self) {
        if let Some(ref token) = self.active_scan_cancel {
            token.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Starts a background metadata/thumbnail pass for all pending library rows.
    ///
    /// One worker owns one reusable metadata engine; it writes through the
    /// cloned database handle and reports progress to the UI thread.
    pub fn start_library_enrichment(&mut self, ctx: &egui::Context) {
        if self.active_enrichment_cancel.is_some() {
            self.enrichment_pending = true;
            return;
        }
        self.enrichment_pending = false;

        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        self.active_enrichment_cancel = Some(cancel.clone());
        self.active_enrichment_rx = Some(rx);
        self.enrichment_progress = Some(EnrichmentProgress::default());

        let db = self.db.clone();
        let cache = self.thumbnail_cache.clone();
        let repaint = ctx.clone();
        let result = std::thread::Builder::new()
            .name("muvrix-library-enrichment".to_string())
            .spawn(move || {
                let summary = enrich_pending(db, cache, cancel, tx, Some(repaint));
                log::info!(
                    "library enrichment: processed={} metadata={} thumbnails={} failed={} cancelled={} error={:?}",
                    summary.processed,
                    summary.metadata_extracted,
                    summary.thumbnails,
                    summary.failed,
                    summary.cancelled,
                    summary.error
                );
            });
        if let Err(e) = result {
            log::error!("failed to spawn library enrichment worker: {e}");
            self.active_enrichment_cancel = None;
            self.active_enrichment_rx = None;
            self.enrichment_progress = Some(EnrichmentProgress {
                finished: true,
                error: Some(format!("enrichment worker: {e}")),
                ..Default::default()
            });
        }
    }

    pub fn cancel_library_enrichment(&mut self) {
        if let Some(ref token) = self.active_enrichment_cancel {
            token.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub(crate) fn poll_library_enrichment(&mut self, ctx: &egui::Context) {
        let mut latest = None;
        let mut finished = false;
        if let Some(rx) = self.active_enrichment_rx.as_ref() {
            while let Ok(progress) = rx.try_recv() {
                finished |= progress.finished;
                latest = Some(progress);
            }
        }
        if let Some(progress) = latest {
            let refresh = progress.finished || progress.processed.is_multiple_of(8);
            self.enrichment_progress = Some(progress);
            if refresh {
                self.refresh_library();
            }
        }
        if finished {
            self.active_enrichment_rx = None;
            self.active_enrichment_cancel = None;
            self.refresh_library();
            if std::mem::take(&mut self.enrichment_pending) {
                self.start_library_enrichment(ctx);
            }
        }
    }
}
