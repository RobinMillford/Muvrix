use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Pos2};
use muvrix_core::browser::MediaBrowser;
use muvrix_core::database::{
    BatchMetadataRequest, Bookmark, CleanupPreview, DatabaseManager, IntegrityScanProgress,
    LibraryFilter, LibraryItemView, LibrarySortField, RelinkCandidate, RuleCombinationMode,
    SelectedMetadataSummary, SmartCollection, SmartRule, SortDirection,
};
use muvrix_core::history::HistoryStore;
use muvrix_core::lyrics::LrcFile;
use muvrix_core::network::CredentialVault;
use muvrix_core::scanner::ScanProgress;
use muvrix_core::thumbnails::ThumbnailCache;
use muvrix_player::equalizer::{AudioDeviceInfo, EqualizerSettings};
use muvrix_player::shader::{Anime4KPreset, ShaderQuality, VideoAdjustmentSettings};
use muvrix_player::{
    AccentColor, AudioChannelsSetting, DecodeObservation, DecodePolicy, MediaCapabilities,
    MpvStreamBridge, PlaybackController, PresentationState, SteppingPreset, SubtitleStyling,
    ThemeMode,
};

use crate::config_editor::ConfigEditorFile;
use crate::library_enrichment::EnrichmentProgress;
use crate::os_integration::{MediaSession, SleepInhibitor};
use crate::render_engine::RenderEngine;
use crate::views::{AppView, SettingsTab};
use crate::wake::WakeBridge;

/// A/V timing snapshot (see [`MuvrixApp::playback_stats`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct PlaybackStats {
    pub avsync: Option<f64>,
    pub display_fps: Option<f64>,
    pub estimated_vf_fps: Option<f64>,
    pub dropped: Option<i64>,
    pub decoder_dropped: Option<i64>,
}

impl PlaybackStats {
    /// Max absolute A/V difference when both sides are known.
    pub fn avsync_abs(&self) -> Option<f64> {
        self.avsync.map(|v| v.abs())
    }
}

/// In-app sleep timer mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SleepTimer {
    #[default]
    Off,
    Minutes(u32),
    EndOfFile,
}

impl SleepTimer {
    pub fn label(self) -> String {
        match self {
            Self::Off => "Off".to_string(),
            Self::Minutes(minutes) => format!("{minutes} min"),
            Self::EndOfFile => "End of file".to_string(),
        }
    }
}

/// Computes the deadline for a timed sleep mode.
pub fn sleep_deadline_for(timer: SleepTimer, now: Instant) -> Option<Instant> {
    match timer {
        SleepTimer::Minutes(minutes) => Some(now + Duration::from_secs(minutes as u64 * 60)),
        SleepTimer::Off | SleepTimer::EndOfFile => None,
    }
}

/// Multi-select state for batch library actions.
#[derive(Debug, Clone, Default)]
pub struct LibrarySelection {
    ids: std::collections::HashSet<i64>,
}

impl LibrarySelection {
    pub fn toggle(&mut self, id: i64) {
        if !self.ids.remove(&id) {
            self.ids.insert(id);
        }
    }

    pub fn select_all<I: IntoIterator<Item = i64>>(&mut self, ids: I) {
        self.ids.clear();
        self.ids.extend(ids);
    }

    pub fn clear(&mut self) {
        self.ids.clear();
    }

    pub fn contains(&self, id: i64) -> bool {
        self.ids.contains(&id)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn ids(&self) -> Vec<i64> {
        self.ids.iter().copied().collect()
    }
}

/// Window decorations and level requested for mini-player mode.
pub fn mini_player_window_state(enabled: bool) -> (bool, egui::WindowLevel) {
    if enabled {
        (false, egui::WindowLevel::AlwaysOnTop)
    } else {
        (true, egui::WindowLevel::Normal)
    }
}

/// Main Muvrix desktop player application.
pub struct MuvrixApp {
    pub(crate) controller: PlaybackController,
    pub(crate) render_engine: Arc<Mutex<RenderEngine>>,
    pub(crate) gl: Arc<glow::Context>,

    // Hardware and OpenGL diagnostics
    pub(crate) gpu_vendor: String,
    pub(crate) gpu_renderer: String,
    pub(crate) gl_version: String,

    // UI and interaction state
    pub(crate) is_seeking: bool,
    pub(crate) seek_target: f64,
    pub(crate) stepping_preset: SteppingPreset,
    pub(crate) jog_drag_accumulator: f32,
    pub(crate) status_message: String,
    pub(crate) path_input: String,
    pub(crate) is_fullscreen: bool,
    pub(crate) show_shortcuts: bool,
    pub(crate) show_diagnostics: bool,
    pub(crate) screenshot_requested: bool,
    pub(crate) screenshot_frames: usize,
    /// Diagnostics: number of `update()` calls. Telemetry exposes it so a
    /// frozen counter can be told apart from a suspended window: a swap stall
    /// keeps `upd` and `renders` advancing together (until the block), an
    /// occluded/minimised window stops both because eframe delivers no paint.
    pub(crate) update_count: u64,
    /// WP4 chain counter: mpv update callbacks raised by the engine. It lives
    /// behind an atomic because the callback runs on an mpv thread.
    pub(crate) update_callbacks: Arc<AtomicU64>,
    /// Independent event-loop wake path for the mpv update callback.
    ///
    /// The callback still raises an egui repaint for its rendering role, but
    /// egui 0.31.1 can deduplicate that backend callback. This bridge is the
    /// wake path that does not depend on egui or on a successful draw.
    pub(crate) wake_bridge: Arc<WakeBridge>,
    /// Diagnostics: last known viewport flags `(focused, minimized, fullscreen)`.
    pub(crate) viewport_flags: (Option<bool>, Option<bool>, Option<bool>),

    // Phase 0.7 navigation and feature state
    pub(crate) current_view: AppView,
    pub(crate) media_browser: MediaBrowser,
    pub(crate) history_store: HistoryStore,
    pub(crate) show_playlist_drawer: bool,
    pub(crate) pending_resume_prompt: Option<(String, f64)>,
    pub(crate) is_pip: bool,
    pub(crate) last_history_save: Instant,
    pub(crate) active_styling: SubtitleStyling,
    pub(crate) theme_mode: ThemeMode,
    pub(crate) accent_color: AccentColor,
    pub(crate) audio_channels: AudioChannelsSetting,
    pub(crate) volume_normalization: bool,
    pub(crate) audio_pitch_correction: bool,
    pub(crate) preferred_audio_languages: String,
    pub(crate) default_audio_delay_ms: i32,
    pub(crate) background_audio: bool,
    pub(crate) verbose_logging: bool,
    pub(crate) advanced_status: Option<String>,
    pub(crate) ui_overlay_visible: bool,
    pub(crate) osd_manually_hidden: bool,
    pub(crate) last_mouse_activity: Instant,
    pub(crate) last_mouse_pos: Option<Pos2>,
    pub(crate) osd_autohide_delay: Duration,
    pub(crate) sleep_timer: SleepTimer,
    pub(crate) sleep_deadline: Option<Instant>,
    pub(crate) settings_search: String,
    pub(crate) config_editor_file: ConfigEditorFile,
    pub(crate) config_editor_text: String,
    pub(crate) config_editor_loaded: bool,
    pub(crate) config_editor_status: String,
    pub(crate) selected_persistent_playlist: Option<i64>,
    pub(crate) playlist_name_input: String,
    pub(crate) playlist_import_input: String,
    pub(crate) playlist_status: Option<String>,
    pub(crate) media_session: Option<MediaSession>,
    pub(crate) sleep_inhibitor: SleepInhibitor,
    pub(crate) sleep_inhibited: bool,
    pub(crate) crash_notice: Option<std::path::PathBuf>,
    pub(crate) library_selection: LibrarySelection,
    pub(crate) library_multi_select: bool,
    pub(crate) library_batch_confirm_delete: bool,

    // Phase 0.8 Media Library & Enhancement state
    pub(crate) db: DatabaseManager,
    #[allow(dead_code)]
    pub(crate) thumbnail_cache: ThumbnailCache,
    pub(crate) library_filter: LibraryFilter,
    pub(crate) library_sort_field: LibrarySortField,
    pub(crate) library_sort_dir: SortDirection,
    pub(crate) library_page: u64,
    pub(crate) library_items: Vec<LibraryItemView>,
    pub(crate) library_total_count: u64,
    pub(crate) library_is_grid: bool,
    pub(crate) selected_library_item: Option<LibraryItemView>,
    pub(crate) add_root_dialog_open: bool,
    pub(crate) add_root_path_input: String,
    pub(crate) thumbnail_textures: crate::thumbnail_cache::LruTextureCache,
    pub(crate) logo_texture: Option<egui::TextureHandle>,
    pub(crate) active_scan_cancel: Option<Arc<AtomicBool>>,
    pub(crate) active_scan_rx: Option<mpsc::Receiver<ScanProgress>>,
    pub(crate) scan_progress: Option<ScanProgress>,
    pub(crate) active_enrichment_cancel: Option<Arc<AtomicBool>>,
    pub(crate) active_enrichment_rx: Option<mpsc::Receiver<EnrichmentProgress>>,
    pub(crate) enrichment_progress: Option<EnrichmentProgress>,
    pub(crate) enrichment_pending: bool,
    pub(crate) video_settings: VideoAdjustmentSettings,
    pub(crate) anime4k_preset: Anime4KPreset,
    pub(crate) anime4k_quality: ShaderQuality,
    pub(crate) custom_shader_path_input: String,
    pub(crate) custom_shader_error: Option<String>,
    pub(crate) tone_mapping_hover_nits: Option<f64>,
    pub(crate) equalizer_settings: EqualizerSettings,
    pub(crate) eq_show_q_factors: bool,
    pub(crate) audio_devices: Vec<AudioDeviceInfo>,
    pub(crate) selected_audio_device: String,
    pub(crate) media_capabilities: MediaCapabilities,
    pub(crate) pending_ssh_host_trust: Option<muvrix_core::network::PresentedHostKey>,
    pub(crate) ssh_host_trust_error: Option<String>,
    pub(crate) network_vault: CredentialVault,
    pub(crate) connection_pool: Arc<muvrix_core::network::ConnectionPool>,
    pub(crate) add_network_open: bool,
    pub(crate) network_name_input: String,
    pub(crate) network_host_input: String,
    pub(crate) network_port_input: u16,
    pub(crate) network_path_input: String,
    pub(crate) network_user_input: String,
    pub(crate) network_pass_input: String,
    pub(crate) network_https_input: bool,
    pub(crate) network_protocol_input: String,
    pub(crate) network_nfs_version: muvrix_core::network::NfsVersion,
    pub(crate) network_status_msg: Option<String>,
    pub(crate) network_host_key_input: String,
    pub(crate) stream_buffer_mb: u32,
    pub(crate) stream_back_buffer_mb: u32,
    pub(crate) stream_readahead_secs: u32,
    pub(crate) stream_max_retries: u32,
    pub(crate) stream_bridge: Arc<MpvStreamBridge>,
    pub(crate) browsing_conn_id: Option<i64>,
    pub(crate) browsing_path: String,
    pub(crate) browsing_entries: Vec<muvrix_core::network::RemoteFileEntry>,
    pub(crate) browsing_loading: bool,
    pub(crate) browsing_error: Option<String>,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) lyrics: Option<LrcFile>,
    pub(crate) lyrics_auto_scroll: bool,
    pub(crate) active_bookmarks: Vec<Bookmark>,
    pub(crate) editing_bookmark_id: Option<i64>,
    pub(crate) editing_bookmark_label: String,
    pub(crate) editing_bookmark_note: String,

    // Frame timing and UI responsiveness measurements
    pub(crate) last_frame_instant: Instant,
    pub(crate) frame_durations: VecDeque<Duration>,
    pub(crate) fps_measured: f64,
    pub(crate) frame_time_ms: f64,
    pub(crate) spinner_angle: f32,

    // Decode-path selection and observation
    pub(crate) decode_policy: DecodePolicy,
    pub(crate) decode_attempt: usize,
    pub(crate) decode_settled: bool,
    pub(crate) decode_observation: Option<DecodeObservation>,
    pub(crate) hdr_state: Option<PresentationState>,
    pub(crate) native_display_kind: Option<&'static str>,
    /// What the VA-API probe found *before* the engine was created: resolved
    /// client library, driver, and the reason initialisation failed, if it did.
    pub(crate) vaapi: muvrix_player::VaapiCapability,
    /// What the shell observed about the output path. Drives the HDR decision.
    pub(crate) output: crate::output::OutputCaps,

    // Optional telemetry sink (`MUVRIX_TELEMETRY`)
    pub(crate) telemetry_path: Option<String>,
    pub(crate) last_telemetry: Instant,
    pub(crate) session_start: Instant,

    // Phase C1: Library per-card context menu
    pub(crate) library_context_item: Option<muvrix_core::database::LibraryItemView>,
    pub(crate) show_library_context_menu: bool,

    // Phase C2: Browser grid view + context menu
    pub(crate) browser_is_grid: bool,
    pub(crate) browser_context_entry: Option<std::path::PathBuf>,
    pub(crate) show_browser_context_menu: bool,

    // Phase C3: Playlist rename + confirmation dialogs
    pub(crate) playlist_rename_id: Option<i64>,
    pub(crate) playlist_rename_input: String,
    pub(crate) playlist_confirm_delete: Option<i64>,
    pub(crate) playlist_confirm_clear: Option<i64>,

    // Phase C5: Global search
    pub(crate) global_search_query: String,
    pub(crate) global_search_active: bool,
    pub(crate) global_search_videos: Vec<muvrix_core::database::LibraryItemView>,
    pub(crate) global_search_audio: Vec<muvrix_core::database::LibraryItemView>,
    pub(crate) global_search_playlists: Vec<muvrix_core::database::Playlist>,
    pub(crate) global_search_last_query: String,

    // Phase C5: Home view quick-access sections
    pub(crate) home_recently_added: Vec<muvrix_core::database::LibraryItemView>,
    pub(crate) home_favorites: Vec<muvrix_core::database::LibraryItemView>,
    pub(crate) home_longest: Vec<muvrix_core::database::LibraryItemView>,
    pub(crate) home_largest: Vec<muvrix_core::database::LibraryItemView>,
    pub(crate) home_sections_loaded: bool,

    // Phase C Final Fix: Carousel offset + Playlist export modal
    pub(crate) home_carousel_scroll_offset: f32,
    pub(crate) show_export_playlist_modal: bool,
    pub(crate) export_playlist_path_input: String,
    pub(crate) export_playlist_status: Option<String>,
    pub(crate) export_playlist_target_id: Option<i64>,
    pub(crate) export_playlist_format: PlaylistExportFormat,
    pub(crate) playlist_drag_source_idx: Option<usize>,
    pub(crate) playlist_drag_target_idx: Option<usize>,
    pub(crate) playlist_use_smart_shuffle: bool,

    // Phase E3: Library Intelligence & Smart Collections
    pub(crate) selected_smart_collection: Option<i64>,
    pub(crate) smart_collections: Vec<SmartCollection>,
    pub(crate) smart_collection_dialog_open: bool,
    pub(crate) editing_smart_collection: Option<SmartCollection>,
    pub(crate) smart_collection_name_input: String,
    pub(crate) smart_collection_desc_input: String,
    pub(crate) smart_collection_icon_input: String,
    pub(crate) smart_collection_rules: Vec<SmartRule>,
    pub(crate) smart_collection_combination_mode: RuleCombinationMode,
    pub(crate) smart_collection_preview_count: Option<u64>,
    pub(crate) smart_collection_error: Option<String>,

    // Phase E3: Batch Metadata Editor
    pub(crate) batch_metadata_dialog_open: bool,
    pub(crate) batch_metadata_summary: Option<SelectedMetadataSummary>,
    #[allow(dead_code)]
    pub(crate) batch_metadata_request: BatchMetadataRequest,
    pub(crate) batch_edit_title_action: u8,
    pub(crate) batch_title_input: String,
    pub(crate) batch_edit_artist_action: u8,
    pub(crate) batch_artist_input: String,
    pub(crate) batch_edit_album_action: u8,
    pub(crate) batch_album_input: String,
    pub(crate) batch_edit_album_artist_action: u8,
    pub(crate) batch_album_artist_input: String,
    pub(crate) batch_edit_genre_action: u8,
    pub(crate) batch_genre_input: String,
    pub(crate) batch_edit_year_action: u8,
    pub(crate) batch_year_input: String,
    pub(crate) batch_edit_track_action: u8,
    pub(crate) batch_track_input: String,
    pub(crate) batch_edit_disc_action: u8,
    pub(crate) batch_disc_input: String,
    pub(crate) batch_edit_status: Option<String>,

    // Phase E3: Integrity Scanner & Relinking
    pub(crate) integrity_dialog_open: bool,
    pub(crate) integrity_scan_progress: Option<IntegrityScanProgress>,
    #[allow(dead_code)]
    pub(crate) integrity_scan_cancel: Option<Arc<AtomicBool>>,
    pub(crate) missing_media_items: Vec<LibraryItemView>,
    pub(crate) selected_missing_item: Option<LibraryItemView>,
    pub(crate) relink_candidates: Vec<RelinkCandidate>,
    pub(crate) relink_search_dir_input: String,
    pub(crate) cleanup_preview: Option<CleanupPreview>,
    pub(crate) cleanup_confirm_open: bool,
    pub(crate) integrity_status_msg: Option<String>,

    // Phase E4: Adaptive Player HUD, Three-Tier Collapsible Decks & Overflow
    pub(crate) player_secondary_deck_expanded: bool,
    pub(crate) player_secondary_deck_collapsed_wide: bool,
    pub(crate) player_more_overflow_open: bool,
    pub(crate) player_empty_url_input_open: bool,
    pub(crate) seeded_ab_loop_applied_file: Option<String>,

    // Phase E5: Audio DSP Expansion, EBU R128 & 32-Band Spectrum Visualizer
    pub(crate) spectrum_visualizer: muvrix_player::SpectrumVisualizer,
    /// Adaptive cache for blocking engine probes; refreshed by the paint path
    /// on a bounded, backoff-limited interval so a slow core cannot stall a
    /// frame.
    pub(crate) slow_metrics: crate::app::slow_metrics::SlowMetrics,
    pub(crate) autoeq_status_msg: Option<(String, bool)>,

    // Phase E7: Multi-Profile Support, Custom Theme Builder & Shortcut Mapper
    pub(crate) keybindings: crate::app::shortcuts::KeyBindingMap,
    pub(crate) recording_shortcut: Option<crate::app::shortcuts::ShortcutAction>,
    pub(crate) active_profile: Option<muvrix_core::database::PlaybackProfile>,
    pub(crate) available_profiles: Vec<muvrix_core::database::PlaybackProfile>,
    pub(crate) profile_switch_elapsed: Option<Duration>,
    pub(crate) profile_status_msg: Option<(String, bool)>,
    pub(crate) new_profile_dialog_open: bool,
    pub(crate) new_profile_name: String,
    pub(crate) new_profile_desc: String,
    pub(crate) custom_theme: crate::theme::CustomTheme,
    pub(crate) custom_theme_json: String,
    pub(crate) theme_status_msg: Option<(String, bool)>,

    // Phase E8: Multi-Platform Release & Containerized Packaging Diagnostics
    pub(crate) packaging_info: crate::packaging::PackagingInfo,
}

impl MuvrixApp {
    pub const RESPONSIVE_BREAKPOINT_WIDTH: f32 = 1300.0;

    /// Evaluates whether the secondary controls deck should be visible.
    /// Wide layout (>=1300px): Visible by default unless explicitly collapsed.
    /// Compact layout (<1300px): Collapsed by default unless explicitly expanded.
    #[inline]
    pub fn is_secondary_deck_visible(&self, deck_width: f32) -> bool {
        if deck_width >= Self::RESPONSIVE_BREAKPOINT_WIDTH {
            !self.player_secondary_deck_collapsed_wide
        } else {
            self.player_secondary_deck_expanded
        }
    }

    /// Toggles secondary controls deck visibility based on current deck width.
    #[inline]
    pub fn toggle_secondary_deck(&mut self, deck_width: f32) {
        if deck_width >= Self::RESPONSIVE_BREAKPOINT_WIDTH {
            self.player_secondary_deck_collapsed_wide = !self.player_secondary_deck_collapsed_wide;
        } else {
            self.player_secondary_deck_expanded = !self.player_secondary_deck_expanded;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaylistExportFormat {
    #[default]
    M3u8,
    Pls,
    Xspf,
}
