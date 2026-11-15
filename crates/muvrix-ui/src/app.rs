//! Desktop application window and playback UI implemented with eframe and egui_glow.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, Vec2};
use glow::HasContext as _;
use muvrix_core::browser::MediaBrowser;
use muvrix_core::database::{
    Bookmark, DatabaseManager, LibraryFilter, LibraryItemView, LibrarySortField, Pagination,
    SortDirection,
};
use muvrix_core::history::HistoryStore;
use muvrix_core::lyrics::{find_sidecar_lrc, LrcFile};
use muvrix_core::network::CredentialVault;
use muvrix_core::scanner::{ScanOptions, ScanProgress, ScannerService};
use muvrix_core::thumbnails::ThumbnailCache;
use muvrix_player::equalizer::{AudioDeviceInfo, EqualizerManager, EqualizerSettings};
use muvrix_player::shader::{Anime4KPreset, ShaderManager, ShaderQuality, VideoAdjustmentSettings};
use muvrix_player::{
    classify_source, plan_presentation, DecodeObservation, DecodePath, DecodePolicy, Event,
    MediaCapabilities, MpvHandle, MpvRenderContext, MpvStreamBridge, PlaybackController,
    PlaybackState, PresentationState, SeekMode, SubtitleStyling, TrackInfo, TrackType,
};

use crate::gl_loader;
use crate::icons::{self, Icon};
use crate::render_engine::RenderEngine;
use crate::views::{AppView, SettingsTab};
use crate::wake::{WakeBridge, WakeHandle};

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
    pub(crate) thumbnail_textures: std::collections::HashMap<String, egui::TextureHandle>,
    pub(crate) active_scan_cancel: Option<Arc<AtomicBool>>,
    pub(crate) active_scan_rx: Option<mpsc::Receiver<ScanProgress>>,
    pub(crate) scan_progress: Option<ScanProgress>,
    pub(crate) video_settings: VideoAdjustmentSettings,
    pub(crate) anime4k_preset: Anime4KPreset,
    pub(crate) anime4k_quality: ShaderQuality,
    pub(crate) equalizer_settings: EqualizerSettings,
    pub(crate) eq_show_q_factors: bool,
    pub(crate) audio_devices: Vec<AudioDeviceInfo>,
    pub(crate) selected_audio_device: String,
    pub(crate) media_capabilities: MediaCapabilities,
    pub(crate) pending_ssh_host_trust: Option<muvrix_core::network::PresentedHostKey>,
    pub(crate) ssh_host_trust_error: Option<String>,
    pub(crate) network_vault: CredentialVault,
    pub(crate) add_network_open: bool,
    pub(crate) network_name_input: String,
    pub(crate) network_host_input: String,
    pub(crate) network_port_input: u16,
    pub(crate) network_path_input: String,
    pub(crate) network_user_input: String,
    pub(crate) network_pass_input: String,
    pub(crate) network_https_input: bool,
    pub(crate) network_protocol_input: String,
    pub(crate) network_status_msg: Option<String>,
    pub(crate) network_host_key_input: String,
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
    /// What the shell observed about the output path. Drives the HDR decision.
    pub(crate) output: crate::output::OutputCaps,

    // Optional telemetry sink (`MUVRIX_TELEMETRY`)
    pub(crate) telemetry_path: Option<String>,
    pub(crate) last_telemetry: Instant,
    pub(crate) session_start: Instant,
}

impl MuvrixApp {
    /// Creates a new `MuvrixApp` inside eframe's creation callback.
    ///
    /// This is now a thin adapter: eframe supplies a context and a loader, and
    /// the shell itself is built by [`MuvrixApp::new_shell`], which the native
    /// runner calls with its own EGL context. Keeping one constructor means the
    /// two shells cannot drift apart in behaviour.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, Box<dyn std::error::Error>> {
        let gl = cc
            .gl
            .clone()
            .ok_or("Glow OpenGL context was not provided by eframe")?;
        let native_display = crate::native_display::from_creation_context(cc);
        Self::new_shell(
            gl,
            cc.egui_ctx.clone(),
            cc.get_proc_address,
            native_display,
            crate::output::OutputCaps::default(),
        )
    }

    /// Builds the application around an existing GL context and window.
    ///
    /// `get_proc_address` must resolve symbols for that same context.
    pub fn new_shell(
        gl: Arc<glow::Context>,
        egui_ctx: egui::Context,
        get_proc_address: Option<&dyn Fn(&std::ffi::CStr) -> *const std::ffi::c_void>,
        native_display: Option<muvrix_player::NativeDisplay>,
        output: crate::output::OutputCaps,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        log::info!("Initializing MuvrixApp OpenGL 3.3+ Desktop Shell");

        // 1. Inspect OpenGL context and GPU diagnostics
        let (gpu_vendor, gpu_renderer, gl_version) = unsafe {
            let vendor = gl.get_parameter_string(glow::VENDOR);
            let renderer = gl.get_parameter_string(glow::RENDERER);
            let version = gl.get_parameter_string(glow::VERSION);
            log::info!("GPU Vendor:   {vendor}");
            log::info!("GPU Renderer: {renderer}");
            log::info!("GL Version:   {version}");
            (vendor, renderer, version)
        };

        // 2. Initialize libmpv handle with required options
        let mpv = Arc::new(MpvHandle::new()?);
        let stream_bridge = Arc::new(MpvStreamBridge::new());
        let _ = stream_bridge.attach_to_handle(&mpv);
        // The native display is what allows a direct (zero-copy) hardware decode
        // path; without it the engine cannot create an interop context and
        // silently degrades to copy-back or software decoding.
        let native_display_kind = native_display.map(|d| d.kind());
        let decode_policy = DecodePolicy::for_current(native_display.is_some());
        let pacing = crate::render_engine::PacingMode::from_env();

        log::info!(
            "Decode policy: family={} native_display={} candidates={:?} pacing={:?}",
            decode_policy.family().as_str(),
            native_display_kind.unwrap_or("none"),
            decode_policy.candidates(),
            pacing
        );

        mpv.set_option_string("vo", "libmpv")?;
        mpv.set_option_string("hwdec", decode_policy.initial_mode())?;
        mpv.set_option_string("keepaspect", "yes")?;
        // `video-timing-offset` is the headroom the renderer gets before the
        // frame's target display time. At 0 there is no headroom, which makes
        // target-time blocking a no-op; it must be non-zero for paced playback.
        let timing_offset =
            std::env::var("MUVRIX_TIMING_OFFSET").unwrap_or_else(|_| "0.050".to_string());
        mpv.set_option_string("video-timing-offset", &timing_offset)?;
        log::info!("video-timing-offset={timing_offset}");

        // Colour output. When the runner established a PQ/BT.2020 surface, the
        // engine must encode for that surface — otherwise PQ pixels are written
        // into an SDR-looking surface (or, worse, sRGB values are presented as
        // PQ). `target-peak` is a configured value, not a panel measurement.
        if output.native_hdr_available {
            mpv.set_option_string("target-trc", "pq")?;
            mpv.set_option_string("target-prim", "bt.2020")?;
            if let Some(peak) = output.target_peak_nits {
                mpv.set_option_string("target-peak", &peak.to_string())?;
            }
            log::info!(
                "engine colour target: pq/bt.2020 peak={:?} (source: {})",
                output.target_peak_nits,
                output.peak_source
            );
        } else {
            log::info!(
                "engine colour target: SDR ({})",
                output
                    .fallback_reason
                    .as_deref()
                    .unwrap_or("HDR path unavailable")
            );
        }
        // Exclude 'lrc' from mpv's automatic subtitle track detection to prevent
        // mpv from loading LRC files as video subtitles and rendering raw timestamps.
        mpv.set_option_string(
            "sub-auto-exts",
            "ass,idx,mks,pgs,rs,sbv,scc,smi,srt,ssa,sub,sup,utf,utf-8,utf8,vtt",
        )?;
        // Phase 2.10 diagnostic: set pre-initialization options before the
        // read-only `display-fps` property path. The option path is the one mpv
        // actually accepts; the Phase 2.9 post-init property set was rejected.
        if let Ok(video_sync) = std::env::var("MUVRIX_TEST_VIDEO_SYNC") {
            mpv.set_option_string("video-sync", &video_sync)?;
            log::warn!("TEST-ONLY: video-sync={video_sync} option set");
        }
        if let Ok(display_fps) = std::env::var("MUVRIX_TEST_DISPLAY_FPS_OPTION") {
            // Pinned mpv uses `display-fps-override`; `display-fps` is the
            // runtime-reported property, not the writable option.
            match mpv.set_option_string("display-fps-override", &display_fps) {
                Ok(()) => log::warn!("TEST-ONLY: display-fps-override={display_fps} option set"),
                Err(e) => log::warn!("TEST-ONLY: display-fps-override option unavailable: {e}"),
            }
        }
        mpv.initialize()?;

        // 3. Initialize libmpv OpenGL render context
        let get_proc_address = get_proc_address.ok_or("GL get_proc_address required for libmpv")?;
        let mpv_render_ctx = unsafe {
            MpvRenderContext::new_gl_with_display(
                &mpv,
                Some(gl_loader::gl_get_proc_address_trampoline),
                &get_proc_address as *const _ as *mut std::ffi::c_void,
                native_display,
            )?
        };
        if let Some(kind) = mpv_render_ctx.native_display_kind() {
            log::info!("Render context created with native display ({kind})");
        } else {
            log::warn!(
                "Render context created without a native display: direct hardware \
                 decode is unavailable in this session"
            );
        }

        // Colour of the engine's working target. A PQ surface needs a
        // floating-point target: quantising PQ to 8 bits per channel costs
        // roughly half a stop of shadow resolution in HDR10 content.
        let engine_options = crate::render_engine::EngineOptions {
            internal_format: if output.native_hdr_available {
                glow::RGBA16F
            } else {
                glow::RGBA8
            },
            deferred_composite: output.is_native_shell(),
        };

        // 4. Create RenderEngine with off-screen FBO and quad compositor
        let mut render_engine = unsafe {
            RenderEngine::new_with_options(&gl, mpv_render_ctx, 1280, 720, engine_options)
                .map_err(|e| format!("Failed to create RenderEngine: {e}"))?
        };

        // 4b. Paced rendering needs the egui context (to schedule wake-ups) and
        //     an anchor between the engine's clock and the local monotonic clock.
        render_engine.set_egui_context(egui_ctx.clone());
        render_engine.set_clock_base(mpv.get_time_us());
        let frame_signal = Arc::new(std::sync::atomic::AtomicBool::new(false));
        render_engine.set_frame_signal(frame_signal.clone());

        // 5. Connect mpv update callback to request eframe repaints.
        //    The callback runs on an engine thread: it must only signal a
        //    repaint, never render or call libmpv APIs.
        let egui_ctx = egui_ctx.clone();
        let callback_signal = frame_signal.clone();
        let update_callbacks = Arc::new(AtomicU64::new(0));
        let callback_counter = update_callbacks.clone();
        // Independent wake path, installed by the native runner once the event
        // loop proxy exists. Until then (and always in the eframe shell) this is
        // a no-op and egui remains the only repaint mechanism.
        let wake_bridge = Arc::new(WakeBridge::new());
        let callback_wake = wake_bridge.clone();
        render_engine
            .mpv_render_ctx_mut()
            .set_update_callback(move || {
                // Engine thread: only raise a flag, bump a counter, ask egui for
                // a repaint, and deliver an independent event-loop wake. No
                // rendering, no libmpv calls.
                let cb = crate::causal_trace::record(
                    crate::causal_trace::Kind::CallbackEntry,
                    0,
                    0,
                    0,
                    0,
                    "mpv_update",
                );
                crate::causal_trace::set_last_callback(cb);
                callback_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                callback_signal.store(true, std::sync::atomic::Ordering::Release);
                crate::trace::record(crate::trace::Ev::UpdateCallback, "mpv_thread", 0, 0);
                // This wake must not depend on egui's repaint-deduplication
                // state: an egui pass is exactly what cannot run while the
                // compositor withholds frame callbacks.
                let accepted = callback_wake.wake_now();
                crate::causal_trace::record(
                    crate::causal_trace::Kind::CallbackWakeBridge,
                    cb,
                    accepted as i64,
                    0,
                    0,
                    if accepted { "accepted" } else { "coalesced" },
                );
                if accepted {
                    crate::trace::record(crate::trace::Ev::Nudge, "mpv_update", 0, 0);
                }
                egui_ctx.request_repaint();
                crate::causal_trace::record(
                    crate::causal_trace::Kind::CallbackEgui,
                    cb,
                    0,
                    0,
                    0,
                    "request_repaint",
                );
                crate::causal_trace::record(
                    crate::causal_trace::Kind::CallbackExit,
                    cb,
                    0,
                    0,
                    0,
                    "mpv_update",
                );
            });

        let render_engine = Arc::new(Mutex::new(render_engine));

        // 6. Wrap in PlaybackController
        let controller = PlaybackController::new(mpv)?;
        let media_capabilities = controller.handle().detect_media_capabilities();

        let db_path = if let Ok(custom) = std::env::var("MUVRIX_DATABASE_PATH") {
            std::path::PathBuf::from(custom)
        } else if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            std::path::PathBuf::from(xdg)
                .join("muvrix")
                .join("muvrix_library.db")
        } else if let Ok(home) = std::env::var("HOME") {
            std::path::PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("muvrix")
                .join("muvrix_library.db")
        } else {
            std::env::temp_dir()
                .join("muvrix")
                .join("muvrix_library.db")
        };
        let db = DatabaseManager::open(&db_path).unwrap_or_else(|_| {
            DatabaseManager::open_in_memory().expect("In-memory database initialization failed")
        });
        let thumbnail_cache =
            ThumbnailCache::new(ThumbnailCache::default_cache_dir(), 500 * 1024 * 1024);

        let current_view = match std::env::var("MUVRIX_INITIAL_VIEW").as_deref() {
            Ok("library") => AppView::Library,
            Ok("browser") => AppView::Browser,
            Ok("playlist") => AppView::Playlist,
            Ok("lyrics") => AppView::Lyrics,
            Ok("history" | "recent") => AppView::Recent,
            Ok("settings") => AppView::Settings,
            _ => AppView::Player,
        };
        let show_playlist_drawer = std::env::var("MUVRIX_OPEN_DRAWER")
            .map(|v| v == "1")
            .unwrap_or(false);

        let initial_audio_devices = vec![AudioDeviceInfo {
            name: "auto".to_string(),
            description: "Auto-select device".to_string(),
        }];
        let mut persisted = muvrix_player::PersistedMediaSettings::load(None);
        persisted.validate_and_fallback(&initial_audio_devices);

        let mut app = Self {
            controller,
            render_engine,
            gl,
            gpu_vendor,
            gpu_renderer,
            gl_version,
            is_seeking: false,
            seek_target: 0.0,
            status_message: "Ready. Open a media file to begin playback.".to_string(),
            last_frame_instant: Instant::now(),
            frame_durations: VecDeque::with_capacity(60),
            fps_measured: 0.0,
            frame_time_ms: 0.0,
            spinner_angle: 0.0,
            decode_policy,
            decode_attempt: 0,
            decode_settled: false,
            decode_observation: None,
            hdr_state: None,
            native_display_kind,
            output: output.clone(),
            telemetry_path: std::env::var("MUVRIX_TELEMETRY").ok(),
            last_telemetry: Instant::now(),
            session_start: Instant::now(),
            path_input: String::new(),
            is_fullscreen: false,
            show_shortcuts: false,
            show_diagnostics: false,
            screenshot_requested: false,
            screenshot_frames: 0,
            update_count: 0,
            update_callbacks,
            wake_bridge,
            viewport_flags: (None, None, None),
            current_view,
            media_browser: MediaBrowser::default(),
            history_store: HistoryStore::load(None),
            show_playlist_drawer,
            pending_resume_prompt: None,
            is_pip: false,
            last_history_save: Instant::now(),
            active_styling: SubtitleStyling::default(),
            db,
            thumbnail_cache,
            library_filter: LibraryFilter::default(),
            library_sort_field: LibrarySortField::Title,
            library_sort_dir: SortDirection::Ascending,
            library_page: 0,
            library_items: Vec::new(),
            library_total_count: 0,
            library_is_grid: true,
            selected_library_item: None,
            add_root_dialog_open: false,
            add_root_path_input: String::new(),
            thumbnail_textures: std::collections::HashMap::new(),
            active_scan_cancel: None,
            active_scan_rx: None,
            scan_progress: None,
            video_settings: if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("enhancement")
            {
                VideoAdjustmentSettings {
                    brightness: 4,
                    contrast: 8,
                    saturation: 12,
                    gamma: 0,
                    hue: 0,
                    sharpen: 1.2,
                    deband: true,
                    active_shaders: Vec::new(),
                    ..Default::default()
                }
            } else {
                persisted.video_adjustments
            },
            anime4k_preset: if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("enhancement")
            {
                Anime4KPreset::ModeA
            } else {
                persisted.anime4k_preset
            },
            anime4k_quality: persisted.anime4k_quality,
            equalizer_settings: if std::env::var("MUVRIX_SETTINGS_TAB").as_deref()
                == Ok("equalizer")
            {
                EqualizerSettings {
                    is_enabled: true,
                    ..Default::default()
                }
            } else {
                persisted.equalizer
            },
            eq_show_q_factors: false,
            audio_devices: initial_audio_devices,
            selected_audio_device: persisted.selected_audio_device,
            media_capabilities,
            pending_ssh_host_trust: None,
            ssh_host_trust_error: None,
            network_vault: CredentialVault::new(),
            add_network_open: std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network")
                || std::env::var("MUVRIX_OPEN_NETWORK_FORM").is_ok(),
            network_name_input: if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network")
            {
                "Demo WebDAV Share (Example)".to_string()
            } else {
                String::new()
            },
            network_host_input: if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network")
            {
                "dav.example.com".to_string()
            } else {
                String::new()
            },
            network_port_input: 8443,
            network_path_input: "/remote.php/webdav/Media/".to_string(),
            network_user_input: if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network")
            {
                "media_streamer".to_string()
            } else {
                String::new()
            },
            network_pass_input: if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network")
            {
                "VaultSuperSecretPassword123!".to_string()
            } else {
                String::new()
            },
            network_https_input: true,
            network_protocol_input: "webdav".to_string(),
            network_status_msg: if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network")
            {
                Some(
                    "Credentials are held in application memory for this session and are not persisted by Muvrix."
                        .to_string(),
                )
            } else {
                None
            },
            network_host_key_input: String::new(),
            stream_bridge,
            browsing_conn_id: None,
            browsing_path: "/".to_string(),
            browsing_entries: Vec::new(),
            browsing_loading: false,
            browsing_error: None,
            settings_tab: match std::env::var("MUVRIX_SETTINGS_TAB").as_deref() {
                Ok("enhancement" | "video") => SettingsTab::VideoEnhancement,
                Ok("equalizer" | "audio") => SettingsTab::EqualizerAudio,
                Ok("network" | "webdav") => SettingsTab::NetworkMedia,
                Ok("preferences" | "privacy" | "window") => SettingsTab::Preferences,
                _ => SettingsTab::Subtitles,
            },
            lyrics: None,
            lyrics_auto_scroll: true,
            active_bookmarks: Vec::new(),
            editing_bookmark_id: None,
            editing_bookmark_label: String::new(),
            editing_bookmark_note: String::new(),
        };

        app.refresh_library();
        app.refresh_audio_devices();
        app.apply_video_enhancements();
        app.apply_equalizer_settings();
        let _ = app
            .controller
            .set_volume_boost(app.equalizer_settings.volume_boost);
        let _ = app.controller.set_volume(persisted.volume);

        if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network") {
            if let Ok(conns) = app.db.list_connections() {
                if conns.is_empty() {
                    let c1 = muvrix_core::database::NetworkConnection {
                        id: 0,
                        name: "Demo WebDAV Share (Nextcloud)".to_string(),
                        protocol: "webdav".to_string(),
                        host: "dav.example.com".to_string(),
                        port: 8443,
                        remote_path: "/remote.php/webdav/Media/sample.mp4".to_string(),
                        auth_type: "basic".to_string(),
                        credential_ref: Some("cred_1".to_string()),
                        is_anonymous: false,
                        auto_connect: false,
                        last_connected_at: 0,
                    };
                    let c2 = muvrix_core::database::NetworkConnection {
                        id: 0,
                        name: "Home NAS Media Share (Samba)".to_string(),
                        protocol: "smb".to_string(),
                        host: "nas.local".to_string(),
                        port: 445,
                        remote_path: "/storage/movies/sample.mkv".to_string(),
                        auth_type: "basic".to_string(),
                        credential_ref: Some("cred_2".to_string()),
                        is_anonymous: false,
                        auto_connect: false,
                        last_connected_at: 0,
                    };
                    let c3 = muvrix_core::database::NetworkConnection {
                        id: 0,
                        name: "Remote Studio Media Server (SSH)".to_string(),
                        protocol: "sftp".to_string(),
                        host: "studio.media.internal".to_string(),
                        port: 2222,
                        remote_path: "/srv/media/dailies.mov".to_string(),
                        auth_type: "basic".to_string(),
                        credential_ref: Some("cred_3".to_string()),
                        is_anonymous: false,
                        auto_connect: false,
                        last_connected_at: 0,
                    };
                    let _ = app.db.insert_connection(&c1);
                    let _ = app.db.insert_connection(&c2);
                    let _ = app.db.insert_connection(&c3);
                }
            }
        }

        if let Ok(seed) = std::env::var("MUVRIX_INITIAL_MEDIA") {
            app.load_file(&seed);
        }
        if let Ok(seed_queue) = std::env::var("MUVRIX_SEED_PLAYLIST") {
            for item in seed_queue.split(';') {
                if !item.trim().is_empty() {
                    let _ = app.controller.playlist_append(item.trim());
                }
            }
        }
        if let Ok(seed_bms) = std::env::var("MUVRIX_SEED_BOOKMARKS") {
            for bm_entry in seed_bms.split(';') {
                let parts: Vec<&str> = bm_entry.split(':').collect();
                if parts.len() >= 2 {
                    if let Ok(pos) = parts[0].parse::<f64>() {
                        let label = parts[1];
                        let note = parts.get(2).copied();
                        app.add_bookmark_at(pos, label, note);
                    }
                }
            }
        }
        if let Ok(pos_str) = std::env::var("MUVRIX_INITIAL_SEEK") {
            if let Ok(pos) = pos_str.parse::<f64>() {
                let _ = app.controller.seek(pos, muvrix_player::SeekMode::Absolute);
            }
        }
        if std::env::var("MUVRIX_INITIAL_PAUSE").is_ok() {
            let _ = app.controller.pause();
        }
        // Phase 2.9 diagnostic: test whether mpv's display-sync path changes VO
        // frame drops when it is told the actual output refresh instead of
        // leaving `display-fps` unset. Test-only until the comparison proves it.
        if let Ok(raw) = std::env::var("MUVRIX_TEST_DISPLAY_FPS") {
            match raw.parse::<f64>() {
                Ok(fps) if fps.is_finite() && fps > 0.0 => {
                    match app.controller.handle().set_property("display-fps", fps) {
                        Ok(()) => {
                            log::warn!("TEST-ONLY: display-fps={fps:.6} set for drop diagnostic")
                        }
                        Err(e) => log::warn!("TEST-ONLY: failed to set display-fps={fps}: {e}"),
                    }
                }
                _ => log::warn!("TEST-ONLY: invalid MUVRIX_TEST_DISPLAY_FPS={raw:?}"),
            }
        }

        Ok(app)
    }

    /// Loads a video file or URL into the playback controller, resolving relative paths,
    /// home directories (~), and candidate fixtures.
    pub fn load_file(&mut self, raw_path: &str) {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            return;
        }

        log::info!("Resolving media path: {trimmed}");
        match muvrix_core::path::resolve_media_path(trimmed) {
            Ok(resolved) => {
                let path_str = resolved.to_string_lossy().to_string();
                log::info!("Resolved media path -> {path_str}");
                self.path_input = path_str.clone();

                // A new independent media item always starts its own decoder
                // attempt. `decode_attempt` is transient per-item state; the
                // live libmpv `hwdec` property is reset explicitly before the
                // load so a previous item's `vaapi-copy` fallback cannot leak
                // into this one. A deliberate pin is preserved by
                // `DecodePolicy::initial_mode()`.
                self.reset_decoder_for_new_media();

                // Check for saved resume position before loading
                let resume_pos = self.history_store.get_resume_position(&path_str);

                match self.controller.load_file(&path_str) {
                    Ok(()) => {
                        // Test-only: long-duration reliability runs need a
                        // source longer than the fixture. Production never sets
                        // this variable.
                        if std::env::var("MUVRIX_TEST_LOOP").is_ok() {
                            match self
                                .controller
                                .handle()
                                .set_property_string("loop-file", "inf")
                            {
                                Ok(()) => log::warn!(
                                    "TEST-ONLY: loop-file=inf enabled for long-duration run"
                                ),
                                Err(e) => {
                                    log::warn!("TEST-ONLY: failed to enable loop-file=inf: {e}")
                                }
                            }
                        }
                        // New file: re-observe which decoder the engine picks.
                        // The reset already happened before `load_file()`; this
                        // keeps the observation state clear if the load is
                        // asynchronous.
                        self.decode_settled = false;
                        self.decode_attempt = 0;
                        self.decode_observation = None;
                        self.hdr_state = None;
                        self.status_message = format!("Loading: {path_str}");
                        // Immediately record initial entry in history store
                        self.history_store.record_playback(&path_str, 0.0, 0.0);
                        let _ = self.history_store.save();

                        // Offer resume prompt if saved position > 10s
                        if let Some(pos) = resume_pos {
                            self.pending_resume_prompt = Some((path_str.clone(), pos));
                        } else {
                            self.pending_resume_prompt = None;
                        }

                        // Auto-detect and parse sidecar LRC lyrics
                        if let Some(lrc_path) = find_sidecar_lrc(&resolved) {
                            match LrcFile::from_file(&lrc_path) {
                                Ok(lrc) => {
                                    log::info!("Auto-loaded sidecar lyrics from {:?}", lrc_path);
                                    self.lyrics = Some(lrc);
                                }
                                Err(e) => {
                                    log::warn!("Failed to parse sidecar lyrics: {e}");
                                    self.lyrics = None;
                                }
                            }
                        } else {
                            self.lyrics = None;
                        }

                        // Refresh active media bookmarks
                        self.refresh_active_bookmarks();
                    }
                    Err(e) => {
                        let err_msg = format!("Failed to initiate playback on '{path_str}': {e}");
                        self.status_message = err_msg.clone();
                        self.controller.set_error(err_msg);
                        log::error!("Controller failed to load {path_str}: {e}");
                    }
                }
            }
            Err(e) => {
                self.path_input = trimmed.to_string();
                let err_msg = format!("File error: {e}");
                self.status_message = err_msg.clone();
                self.controller.set_error(err_msg);
                log::error!("Failed to resolve media path '{trimmed}': {e}");
            }
        }
    }

    /// Resolves and loads the first existing preset fixture from a list of candidate paths.
    pub fn load_preset(&mut self, candidates: &[&str]) {
        match muvrix_core::path::resolve_preset(candidates) {
            Ok(resolved) => {
                let path_str = resolved.to_string_lossy().to_string();
                self.load_file(&path_str);
            }
            Err(e) => {
                let err_msg = format!("Preset unavailable: {e}");
                self.status_message = err_msg.clone();
                self.controller.set_error(err_msg);
                log::warn!("Preset fixture missing: {e}");
            }
        }
    }

    /// Resets the decoder request for a new independent media item.
    ///
    /// `decode_attempt`/`decode_settled` are transient per-item state, but they
    /// are not sufficient on their own: the previous item may have left the live
    /// libmpv `hwdec` property at `vaapi-copy`. Writing `initial_mode()` back
    /// before the load is what prevents cross-media policy inheritance. A
    /// deliberate user/test pin is returned unchanged by `initial_mode()`.
    fn reset_decoder_for_new_media(&mut self) {
        let mode = self.decode_policy.initial_mode();
        match self.controller.set_hwdec_mode(mode) {
            Ok(()) => log::info!(
                "Decoder policy reset for new media: hwdec='{mode}' pinned={}",
                self.decode_policy.is_pinned()
            ),
            Err(e) => log::warn!(
                "Failed to reset hwdec to '{mode}' for new media ({e}); \
                 the decoder may retain the previous item's fallback"
            ),
        }
        self.decode_attempt = 0;
        self.decode_settled = false;
        self.decode_observation = None;
        self.hdr_state = None;
    }

    /// Opens the native file picker to select a media file.
    pub fn open_file_dialog(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Open Media File")
            .add_filter(
                "Media Files",
                &[
                    "mp4", "mkv", "webm", "avi", "mov", "flv", "ts", "m4v", "wmv", "mp3", "flac",
                    "wav", "m4a", "aac", "ogg", "opus", "ac3", "dts",
                ],
            )
            .pick_file()
        {
            let path_str = file.display().to_string();
            self.load_file(&path_str);
        }
    }

    /// Opens the native file picker to load an external subtitle file.
    pub fn add_subtitle_dialog(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Add Subtitle Track")
            .add_filter(
                "Subtitle Files",
                &["srt", "ass", "ssa", "vtt", "sub", "idx"],
            )
            .pick_file()
        {
            let path_str = file.display().to_string();
            let _ = self.controller.add_subtitle_file(&path_str, true);
        }
    }

    /// Toggles play/pause state.
    pub fn toggle_pause(&mut self) {
        let _ = self.controller.toggle_pause();
    }

    /// Seeks relative to current playback position by `seconds`.
    pub fn seek_relative(&mut self, seconds: f64) {
        let _ = self.controller.seek(seconds, SeekMode::Relative);
    }

    /// Toggles fullscreen window state.
    pub fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        self.is_fullscreen = !self.is_fullscreen;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.is_fullscreen));
    }

    /// Exits fullscreen mode if currently active.
    pub fn exit_fullscreen(&mut self, ctx: &egui::Context) {
        if self.is_fullscreen {
            self.is_fullscreen = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
    }

    /// Observes the decoder the engine *actually* selected and applies a
    /// platform-appropriate fallback when hardware decoding was requested but
    /// software was chosen.
    ///
    /// The request (`hwdec`) is never treated as evidence: only `hwdec-current`
    /// plus the decoded frame's pixel format decide the reported path.
    fn check_decoder_path(&mut self) {
        if self.decode_settled {
            return;
        }
        if *self.controller.state() != PlaybackState::Playing {
            return;
        }
        if self.controller.video_format().is_empty() {
            return;
        }

        // Before the decoder initialises, `hwdec-current` is empty or "inactive".
        let hwdec = self.controller.hwdec_current().to_string();
        if hwdec.is_empty() || hwdec == "inactive" {
            return;
        }

        let observation = self.controller.decode_observation();
        if observation.path == DecodePath::Inactive {
            return;
        }

        self.decode_settled = true;
        log::info!(
            "Decoder observed: file='{}' hwdec='{}' pixelformat='{}' path={:?} requested='{}'",
            self.controller.current_file().unwrap_or("<none>"),
            observation.active,
            observation.pixel_format,
            observation.path,
            self.controller.requested_hwdec()
        );

        // Source HDR classification + presentation plan. `native_hdr_available`
        // is false on every path this build can offer today: no platform has a
        // proven HDR surface hand-off yet, so HDR sources are tone-mapped and
        // reported as such rather than as HDR.
        let source = classify_source(
            &self.controller.video_gamma(),
            &self.controller.video_primaries(),
            None,
        );
        // The decision uses what the shell actually observed: a PQ surface is
        // only "available" when the surface format, the precision probe and the
        // compositor's colour description all succeeded. Anything less falls
        // back to SDR with the exact reason recorded.
        let display = self.output.display.clone();
        // `ColorPlan` is the source-side decision record. The engine target is
        // owned by `apply_video_enhancements` -> `apply_output_target`, where the
        // output surface is authoritative; it is deliberately not re-applied
        // here and cannot fight the PQ/BT.2020 target.
        let (mut state, _plan) =
            plan_presentation(&source, &display, self.output.native_hdr_available);
        if state.reason.is_none() {
            state.reason = self.output.fallback_reason.clone();
        }
        if state.hdr_presented && !self.output.physical_verified {
            state.reason = Some(
                "PQ surface + accepted description; physical panel HDR not verified".to_string(),
            );
        }
        log::info!(
            "Presentation: source={} mode={:?} tone_mapping={} hdr_presented={} reason={}",
            state.source.label(),
            state.mode,
            state.tone_mapping,
            state.hdr_presented,
            state.reason.clone().unwrap_or_else(|| "-".to_string())
        );
        let hdr_note = if state.source.is_hdr() {
            format!(" | {} → SDR (tone-mapped)", state.source.label())
        } else {
            String::new()
        };
        self.hdr_state = Some(state);

        // Fallback: a hardware mode was requested but software was selected.
        let expects_hardware = self.controller.requested_hwdec() != "no"
            && !self.controller.requested_hwdec().is_empty();
        if expects_hardware && observation.path == DecodePath::Software {
            if let Some(next) = self.decode_policy.next_mode(self.decode_attempt) {
                let _ = self.controller.set_hwdec_mode(next);
                // Force a decoder reinit so the retry is observable in this
                // session. If the command is unavailable, the new mode applies
                // on the next file load instead (logged, not assumed).
                let reload = self.controller.handle().command(&["video-reload"]);
                self.decode_attempt += 1;
                self.decode_settled = false;
                match reload {
                    Ok(()) => log::warn!(
                        "Requested hardware decoding but engine selected software; \
                         retrying with hwdec='{next}' (video-reload issued)"
                    ),
                    Err(e) => log::warn!(
                        "Requested hardware decoding but engine selected software; \
                         hwdec='{next}' set but video-reload failed ({e}); it applies on next load"
                    ),
                }
                self.status_message = format!(
                    "Hardware decoding unavailable ({}), retrying with '{next}'",
                    observation.active
                );
                return;
            }
            log::error!(
                "Hardware decoding unavailable: all decoder modes exhausted ({:?})",
                self.decode_policy.candidates()
            );
            self.status_message =
                "Software decoding — hardware acceleration unavailable".to_string();
        } else {
            self.status_message = format!("{}{hdr_note}", observation.describe());
        }

        self.decode_observation = Some(observation);
    }

    /// Appends one telemetry line per second when `MUVRIX_TELEMETRY` is set.
    ///
    /// The line is written for measurement only; nothing in the playback path
    /// depends on it.
    fn flush_telemetry(&mut self) {
        let Some(path) = self.telemetry_path.clone() else {
            return;
        };
        if self.last_telemetry.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_telemetry = Instant::now();

        let (
            renders,
            skips,
            swaps,
            block_us,
            block_max,
            block_total,
            block_hist,
            pacing,
            swap_mode,
        ) = {
            let Ok(engine) = self.render_engine.lock() else {
                return;
            };
            let t = engine.telemetry();
            (
                t.mpv_renders,
                t.mpv_skips,
                t.swap_reports,
                t.last_render_block_us,
                t.max_render_block_us,
                t.render_block_us_total,
                t.render_block_hist,
                format!("{:?}", engine.pacing()),
                format!("{:?}", engine.swap_report()),
            )
        };
        let hist = block_hist
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let avsync = self
            .controller
            .avsync()
            .map(|v| format!("{v:.6}"))
            .unwrap_or_else(|| "na".to_string());
        let src_fps = self
            .controller
            .container_fps()
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "na".to_string());
        let vf_fps = self
            .controller
            .estimated_vf_fps()
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "na".to_string());

        let line = format!(
            "t={:.1} state={:?} renders={renders} skips={skips} swaps={swaps} drops={} \
             decdrops={} delayed={} avsync={avsync} hwdec={} pix={} gamma={} prims={} \
             srcfps={src_fps} vfps={vf_fps} block_us={block_us} block_max={block_max} \
             block_total={block_total} block_hist={hist} pacing={pacing} swap={swap_mode} \
             upd={} foc={:?} min={:?} fs={:?}\n",
            self.session_start.elapsed().as_secs_f64(),
            self.controller.state(),
            self.controller.frame_drop_count(),
            self.controller.decoder_frame_drop_count(),
            self.controller.vo_delayed_frame_count(),
            self.controller.hwdec_current(),
            self.controller.video_pixel_format(),
            self.controller.video_gamma(),
            self.controller.video_primaries(),
            self.update_count,
            self.viewport_flags.0,
            self.viewport_flags.1,
            self.viewport_flags.2,
        );

        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write as _;
                let _ = file.write_all(line.as_bytes());
            }
            Err(e) => log::warn!("Telemetry write to {path} failed: {e}"),
        }
    }

    /// Updates playback properties and polls events from mpv.
    fn sync_playback_state(&mut self) {
        let events = self.controller.poll_events();
        for event in events {
            match event {
                Event::FileLoaded => {
                    let (w, h) = self.controller.video_dimensions();
                    let vfmt = self.controller.video_format();
                    let afmt = self.controller.audio_codec();
                    let ao = self.controller.current_ao();
                    let hw = self.controller.hwdec_current();
                    log::info!(
                        "File loaded: {w}x{h} [video: {vfmt}, audio: {afmt}, ao: {ao}, hwdec: {hw}]"
                    );
                    self.status_message =
                        format!("Playback active: {vfmt} ({w}x{h}) | HWDEC: {hw} | AO: {ao}");
                    self.refresh_active_bookmarks();
                    if let Ok(pos_str) = std::env::var("MUVRIX_INITIAL_SEEK") {
                        if let Ok(pos) = pos_str.parse::<f64>() {
                            let _ = self.controller.seek(pos, muvrix_player::SeekMode::Absolute);
                        }
                    }
                    if std::env::var("MUVRIX_INITIAL_PAUSE").is_ok() {
                        let _ = self.controller.pause();
                    }
                }
                Event::EndFile { reason, error, .. } => {
                    if let Some(path) = self.controller.current_file() {
                        let time_pos = self.controller.time_pos();
                        let duration = self.controller.duration();
                        self.history_store.record_playback(path, time_pos, duration);
                        let _ = self.history_store.save();
                    }

                    if let Some(err) = error {
                        let path = self.controller.current_file().unwrap_or("<unknown>");
                        self.status_message = format!("Playback error on '{path}': {err}");
                    } else if reason == muvrix_player::EndFileReason::Eof {
                        self.status_message = "End of file reached".to_string();
                    } else {
                        self.status_message = format!("Playback stopped: {reason:?}");
                    }
                }
                Event::PlaybackRestart => {
                    self.status_message = "Playback active".to_string();
                }
                _ => {}
            }
        }

        // Periodic history persistence every 3 seconds during playback
        if *self.controller.state() == PlaybackState::Playing
            && self.last_history_save.elapsed() > Duration::from_secs(3)
        {
            self.last_history_save = Instant::now();
            if let Some(path) = self.controller.current_file() {
                let time_pos = self.controller.time_pos();
                let duration = self.controller.duration();
                if duration > 0.0 && time_pos > 0.0 {
                    self.history_store.record_playback(path, time_pos, duration);
                    let _ = self.history_store.save();
                }
            }
        }
    }

    /// Toggles compact mini-player floating mode (resizes main window to 480x270 and requests AlwaysOnTop).
    pub fn toggle_pip(&mut self, ctx: &egui::Context) {
        self.is_pip = !self.is_pip;
        if self.is_pip {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(480.0, 270.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            self.status_message =
                "Compact Mini-Player mode enabled (Always on Top is best-effort on Wayland)"
                    .to_string();
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(1280.0, 720.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::Normal,
            ));
            self.status_message = "Exited Compact Mini-Player mode".to_string();
        }
    }

    /// Measures actual frame presentation rate and frame delta times.
    fn update_frame_timing(&mut self) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame_instant);
        self.last_frame_instant = now;

        let is_playing = *self.controller.state() == PlaybackState::Playing;

        if is_playing {
            if self.frame_durations.len() >= 60 {
                self.frame_durations.pop_front();
            }
            self.frame_durations.push_back(delta);

            let total: Duration = self.frame_durations.iter().sum();
            let count = self.frame_durations.len() as f64;
            if count > 0.0 && total.as_secs_f64() > 0.0 {
                self.frame_time_ms = (total.as_secs_f64() * 1000.0) / count;
                self.fps_measured = count / total.as_secs_f64();
            }
            self.spinner_angle = (self.spinner_angle + 0.08) % (2.0 * std::f32::consts::PI);
        } else {
            self.frame_time_ms = delta.as_secs_f64() * 1000.0;
            self.frame_durations.clear();
            self.fps_measured = 0.0;
        }
    }

    /// Handles keyboard shortcuts for desktop playback control.
    ///
    /// Checks `ctx.wants_keyboard_input()` to ensure shortcuts do not trigger
    /// while the user is actively typing in a text field.
    fn handle_keyboard_inputs(&mut self, ctx: &egui::Context) {
        // Do NOT intercept shortcuts if user is typing into an active text field!
        if ctx.wants_keyboard_input() {
            return;
        }

        // Space: Toggle Play/Pause
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.toggle_pause();
        }

        // Left / Right Arrow: Relative Seek (-5s / +5s)
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            let _ = self.controller.seek_relative(-5.0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            let _ = self.controller.seek_relative(5.0);
        }

        // Up / Down Arrow: Volume adjustments (+5% / -5%)
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            let _ = self.controller.adjust_volume(5.0);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            let _ = self.controller.adjust_volume(-5.0);
        }

        // M: Mute / Unmute toggle
        if ctx.input(|i| i.key_pressed(egui::Key::M)) {
            let _ = self.controller.toggle_mute();
        }

        // F or F11: Fullscreen toggle
        if ctx.input(|i| i.key_pressed(egui::Key::F) || i.key_pressed(egui::Key::F11)) {
            self.toggle_fullscreen(ctx);
        }

        // Escape: Exit Fullscreen or dismiss modal
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.show_shortcuts {
                self.show_shortcuts = false;
            } else if self.is_fullscreen {
                self.exit_fullscreen(ctx);
            }
        }

        // [ and ]: Playback speed decrease / increase (-0.25x / +0.25x)
        if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
            let _ = self.controller.adjust_speed(-0.25);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
            let _ = self.controller.adjust_speed(0.25);
        }

        // Backspace: Reset speed to 1.0x
        if ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
            let _ = self.controller.reset_speed();
        }

        // Ctrl+O (or Cmd+O): Open File dialog
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
            self.open_file_dialog();
        }

        // P: Toggle Playlist drawer
        if ctx.input(|i| i.key_pressed(egui::Key::P)) {
            self.show_playlist_drawer = !self.show_playlist_drawer;
        }

        // B: Toggle Media Browser view
        if ctx.input(|i| i.key_pressed(egui::Key::B)) {
            self.current_view = if self.current_view == AppView::Browser {
                AppView::Player
            } else {
                AppView::Browser
            };
        }

        // I: Toggle Picture-in-Picture mode
        if ctx.input(|i| i.key_pressed(egui::Key::I)) {
            self.toggle_pip(ctx);
        }

        // F1: Toggle shortcuts help dialog
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_shortcuts = !self.show_shortcuts;
        }
    }

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

    pub fn refresh_audio_devices(&mut self) {
        if let Ok(devs) = EqualizerManager::query_audio_devices(self.controller.handle()) {
            self.audio_devices = devs;
        }
    }

    /// Reloads the list of user bookmarks for the currently playing media item.
    pub fn refresh_active_bookmarks(&mut self) {
        let current_path = self
            .controller
            .current_file()
            .map(|s| s.to_string())
            .or_else(|| {
                if !self.path_input.is_empty() {
                    Some(self.path_input.clone())
                } else {
                    None
                }
            });
        if let Some(path) = current_path {
            let canon = std::fs::canonicalize(&path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| path.clone());
            if let Ok(Some(item)) = self
                .db
                .get_media_item_by_canonical_path(&canon)
                .or_else(|_| self.db.get_media_item_by_canonical_path(&path))
            {
                if let Ok(bms) = self.db.list_bookmarks_for_media(item.id) {
                    self.active_bookmarks = bms;
                    return;
                }
            } else if let Ok(Some(item)) = self.db.get_media_item_by_canonical_path(&path) {
                if let Ok(bms) = self.db.list_bookmarks_for_media(item.id) {
                    self.active_bookmarks = bms;
                    return;
                }
            }
        }
        self.active_bookmarks.clear();
    }

    /// Creates and persists a user bookmark at an explicit playback position.
    pub fn add_bookmark_at(&mut self, time_pos: f64, label: &str, note: Option<&str>) {
        let current_path = self
            .controller
            .current_file()
            .map(|s| s.to_string())
            .or_else(|| {
                if !self.path_input.is_empty() {
                    Some(self.path_input.clone())
                } else {
                    None
                }
            });
        if let Some(path) = current_path {
            let canon = std::fs::canonicalize(&path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| path.clone());
            let timestamp_ms = (time_pos * 1000.0) as i64;
            let media_id = match self
                .db
                .get_media_item_by_canonical_path(&canon)
                .or_else(|_| self.db.get_media_item_by_canonical_path(&path))
            {
                Ok(Some(item)) => item.id,
                _ => {
                    let root_id = match self.db.get_roots() {
                        Ok(roots) if !roots.is_empty() => roots[0].id,
                        _ => {
                            let root = muvrix_core::database::LibraryRoot {
                                id: 0,
                                display_name: "Standalone Media".to_string(),
                                root_path: "/".to_string(),
                                source_type: "local".to_string(),
                                is_recursive: false,
                                is_enabled: true,
                                exclusion_patterns: Vec::new(),
                                last_scanned_at: 0,
                                scan_status: "idle".to_string(),
                            };
                            self.db.insert_root(&root).unwrap_or(1)
                        }
                    };
                    let file_name = std::path::Path::new(&path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&path)
                        .to_string();
                    let file_ext = std::path::Path::new(&path)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_string();
                    let item = muvrix_core::database::MediaItem {
                        id: 0,
                        root_id,
                        relative_path: file_name.clone(),
                        canonical_path: canon,
                        file_name,
                        file_extension: file_ext,
                        media_type: muvrix_core::database::MediaType::Video,
                        file_size: 0,
                        modified_time: 0,
                        added_time: 0,
                        last_scanned_at: 0,
                        availability_state: muvrix_core::database::AvailabilityState::Available,
                        metadata_status: muvrix_core::database::MetadataStatus::Pending,
                    };
                    self.db.upsert_media_item(&item).unwrap_or(0)
                }
            };
            if media_id > 0 {
                let _ = self.db.insert_bookmark(media_id, timestamp_ms, label, note);
                self.refresh_active_bookmarks();
                self.status_message =
                    format!("Added bookmark '{label}' at {}", format_timestamp(time_pos));
            }
        }
    }

    /// Creates and persists a user bookmark at the current playback position.
    pub fn add_bookmark(&mut self, label: &str, note: Option<&str>) {
        let time_pos = self.controller.time_pos();
        self.add_bookmark_at(time_pos, label, note);
    }

    /// Opens a native file dialog to manually select a synchronized LRC lyrics file.
    pub fn load_lyrics_dialog(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Open Synchronized LRC Lyrics File")
            .add_filter("LRC Lyrics", &["lrc", "txt"])
            .pick_file()
        {
            match LrcFile::from_file(&file) {
                Ok(lrc) => {
                    let title = lrc.title.as_deref().unwrap_or("lyrics");
                    self.status_message = format!("Loaded synchronized lyrics for '{title}'");
                    self.lyrics = Some(lrc);
                }
                Err(e) => {
                    self.status_message = format!("Failed to parse LRC lyrics: {e}");
                }
            }
        }
    }

    pub fn start_library_scan(&mut self, root_id: i64) {
        if self.active_scan_cancel.is_some() {
            return;
        }
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

    pub fn apply_video_enhancements(&mut self) {
        let _ = ShaderManager::apply_adjustments(self.controller.handle(), &self.video_settings);
        // Re-assert the shell-owned output target after the adjustment write.
        // `Auto` in the adjustment settings no longer writes target-trc/prim, but
        // this also covers an explicit non-Auto selection or a reset on an HDR
        // surface: the output surface remains the authority.
        let _ = ShaderManager::apply_output_target(
            self.controller.handle(),
            self.output.native_hdr_available,
            self.output.target_peak_nits,
        );
        let trc = self
            .controller
            .handle()
            .get_property_string("target-trc")
            .unwrap_or_else(|_| "unavailable".to_string());
        let prim = self
            .controller
            .handle()
            .get_property_string("target-prim")
            .unwrap_or_else(|_| "unavailable".to_string());
        log::info!(
            "output colour target: trc={trc} prim={prim} native_hdr={} physical_verified={}",
            self.output.native_hdr_available,
            self.output.physical_verified
        );
        let _ = ShaderManager::apply_anime4k_mode(
            self.controller.handle(),
            self.anime4k_preset,
            self.anime4k_quality,
            None,
        );
        self.save_media_settings();
    }

    pub fn apply_equalizer_settings(&mut self) {
        self.equalizer_settings.sync_gains_to_bands();
        let _ =
            EqualizerManager::apply_to_handle(self.controller.handle(), &self.equalizer_settings);
        self.save_media_settings();
    }

    pub fn save_media_settings(&self) {
        let current_volume = self.controller.volume();
        let settings = muvrix_player::PersistedMediaSettings {
            schema_version: muvrix_player::CURRENT_SETTINGS_VERSION,
            video_adjustments: self.video_settings.clone(),
            anime4k_preset: self.anime4k_preset,
            anime4k_quality: self.anime4k_quality,
            equalizer: self.equalizer_settings.clone(),
            selected_audio_device: self.selected_audio_device.clone(),
            volume: current_volume,
        };
        let _ = settings.save(None);
    }
}

impl MuvrixApp {
    /// A/V timing properties, sampled by the runner's telemetry.
    ///
    /// `avsync` is the engine's own A/V difference in seconds; `display_fps` is
    /// its estimate of the output rate (which we never tell it, because the
    /// compositor does not tell us either). `dropped` counts frames the engine
    /// discarded because they were late.
    pub fn playback_stats(&self) -> PlaybackStats {
        let h = self.controller.handle();
        PlaybackStats {
            avsync: h.get_property::<f64>("avsync").ok(),
            display_fps: h.get_property::<f64>("display-fps").ok(),
            estimated_vf_fps: h.get_property::<f64>("estimated-vf-fps").ok(),
            dropped: h.get_property::<i64>("frame-drop-count").ok(),
            decoder_dropped: h.get_property::<i64>("decoder-frame-drop-count").ok(),
        }
    }

    /// Current video texture and its window-pixel rectangle, for the shell's
    /// composite pass. `None` means the engine has no target yet.
    pub fn video_output(&self) -> Option<(Option<glow::Texture>, [i32; 4])> {
        let engine = self.render_engine.lock().ok()?;
        Some((Some(engine.video_texture()), engine.last_video_rect()))
    }

    /// Cumulative mpv update callbacks observed (WP4 chain counter).
    pub fn update_callback_count(&self) -> u64 {
        self.update_callbacks
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Snapshot of the render engine's counters (WP4). `None` when the engine
    /// cannot be locked (shutdown racing the telemetry tick).
    pub fn render_telemetry(&self) -> Option<crate::render_engine::RenderTelemetry> {
        self.render_engine
            .lock()
            .ok()
            .map(|e| e.telemetry().clone())
    }

    /// Test-only engine pump for the sequential media harness.
    ///
    /// The normal UI path polls the engine and observes the decoder from
    /// `frame_update`. A compositor that withholds paint callbacks must not make
    /// a decoder-policy test unable to observe its own loads, so the test-only
    /// sequence calls this from `about_to_wait`. It does not render or build UI.
    pub fn poll_engine_only(&mut self) {
        self.controller.poll_events();
        self.sync_playback_state();
        self.check_decoder_path();
    }

    /// Current playback state (used by the WP3 presentation state machine).
    pub fn playback_state(&self) -> PlaybackState {
        self.controller.state().clone()
    }

    /// Installs the event-loop wake handle used by the mpv update callback.
    ///
    /// The native runner calls this once its `WakeSignal` (and therefore its
    /// event-loop proxy) exists. The eframe shell never calls it, so its update
    /// callback keeps using egui alone, exactly as before.
    pub fn install_wake(&self, wake: Arc<dyn WakeHandle>) {
        self.wake_bridge.install(wake);
    }

    /// Clears the update-callback wake latch after the event loop drained a
    /// batch. Called from `about_to_wait`; deliberately neither paint-gated nor
    /// dependent on egui.
    pub fn clear_update_wake(&self) {
        self.wake_bridge.clear();
    }

    /// Tells the engine the frame it rendered has been handed to the compositor.
    pub fn report_presented(&self) {
        if let Ok(mut engine) = self.render_engine.lock() {
            engine.report_presented();
        }
    }

    /// Native Wayland presentation owns acknowledgement timing.
    pub fn enable_external_presentation_ack(&self) {
        if let Ok(mut engine) = self.render_engine.lock() {
            engine.set_external_swap_ack(true);
        }
    }

    /// Token of the most recent newly rendered frame, consumed by the runner.
    pub fn take_presentation_token(&self) -> Option<u64> {
        self.render_engine
            .lock()
            .ok()
            .and_then(|mut engine| engine.take_rendered_token())
    }

    /// Acknowledge tokens when the compositor reports actual presentation.
    pub fn report_swap_tokens(&self, tokens: &[u64]) {
        if let Ok(mut engine) = self.render_engine.lock() {
            engine.report_swap_tokens(tokens);
        }
    }

    /// Output capabilities the shell established (diagnostics + HDR decision).
    pub fn output_caps(&self) -> &crate::output::OutputCaps {
        &self.output
    }

    /// One frame: poll the engine, update state, and build the UI.
    ///
    /// Identical for both shells. `eframe::App::update` and the native runner
    /// both call this; only the surrounding window/swap ownership differs.
    pub fn frame_update(&mut self, ctx: &egui::Context) {
        self.update_count = self.update_count.wrapping_add(1);
        // Test-only: an unattended harness launches the window without user
        // interaction, and GNOME will leave it behind other windows, where the
        // compositor throttles it. Going fullscreen on the first frame is the
        // only way a scripted run can guarantee the surface is visible and
        // focused. Production never sets this variable.
        if self.update_count == 1 && std::env::var("MUVRIX_TEST_FULLSCREEN").is_ok() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            log::warn!("TEST-ONLY: requesting fullscreen so the harness has a visible surface");
        }
        self.viewport_flags = ctx.input(|i| {
            let vp = i.viewport();
            (vp.focused, vp.minimized, vp.fullscreen)
        });

        // Poll background scanner channel
        let mut scan_finished = false;
        if let Some(ref rx) = self.active_scan_rx {
            while let Ok(progress) = rx.try_recv() {
                if progress.is_finished {
                    scan_finished = true;
                }
                self.scan_progress = Some(progress);
            }
        }
        if scan_finished {
            self.active_scan_cancel = None;
            self.refresh_library();
        }

        self.sync_playback_state();
        self.check_decoder_path();
        self.flush_telemetry();
        self.update_frame_timing();
        self.handle_keyboard_inputs(ctx);

        // Handle drag and drop media files into the application
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                for file in &i.raw.dropped_files {
                    if let Some(ref path) = file.path {
                        let path_str = path.to_string_lossy().to_string();
                        if self.controller.playlist_count() == 0
                            && *self.controller.state() == PlaybackState::Idle
                        {
                            self.load_file(&path_str);
                        } else {
                            let _ = self.controller.playlist_append(&path_str);
                            self.status_message = format!("Appended to playlist: {path_str}");
                        }
                    }
                }
            }
        });

        // Optional automated window screenshot capture for verification
        if let Ok(capture_path) = std::env::var("MUVRIX_CAPTURE_WINDOW_PNG") {
            self.screenshot_frames += 1;
            let media_ready = self.controller.current_file().is_none()
                || self.controller.duration() > 0.0
                || self.screenshot_frames >= 30;
            let state_ready = *self.controller.state() == PlaybackState::Playing
                || *self.controller.state() == PlaybackState::Idle
                || *self.controller.state() == PlaybackState::Paused;

            if self.screenshot_frames >= 15
                && media_ready
                && state_ready
                && !self.screenshot_requested
            {
                self.screenshot_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            } else if !self.screenshot_requested {
                ctx.request_repaint();
            }

            ctx.input(|i| {
                for event in &i.raw.events {
                    if let egui::Event::Screenshot { image, .. } = event {
                        let pixels: Vec<u8> =
                            image.pixels.iter().flat_map(|p| p.to_array()).collect();
                        if let Err(e) = image::save_buffer(
                            &capture_path,
                            &pixels,
                            image.size[0] as u32,
                            image.size[1] as u32,
                            image::ExtendedColorType::Rgba8,
                        ) {
                            log::error!("Failed to save window screenshot to {capture_path}: {e}");
                        } else {
                            log::info!("Successfully captured window screenshot to {capture_path}");
                            if std::env::var("MUVRIX_CAPTURE_AND_EXIT").is_ok() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                std::process::exit(0);
                            }
                        }
                    }
                }
            });
        }

        // Keep UI animating during active playback
        if *self.controller.state() == PlaybackState::Playing {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        // Apply sleek dark aesthetic theme
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(18, 20, 26);
        ctx.set_visuals(visuals);

        // ---------------------------------------------------------------------
        // 1. Top Identity, Navigation & Player Header Panel
        // ---------------------------------------------------------------------
        egui::TopBottomPanel::top("player_top_bar")
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(3.0);
                let available_w = ui.available_width();

                ui.horizontal(|ui| {
                    // Application branding / identity
                    ui.label(
                        RichText::new("MUVRIX")
                            .strong()
                            .size(13.5)
                            .color(Color32::from_rgb(80, 180, 255)),
                    );

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // Navigation View Tabs (Player, Library, Browser, Playlist, Lyrics, History, Settings)
                    for view in &[
                        AppView::Player,
                        AppView::Library,
                        AppView::Browser,
                        AppView::Playlist,
                        AppView::Lyrics,
                        AppView::Recent,
                        AppView::Settings,
                    ] {
                        let is_active = self.current_view == *view;
                        let text = if available_w <= 960.0 {
                            match *view {
                                AppView::Player => "Player".to_string(),
                                AppView::Library => "Lib".to_string(),
                                AppView::Browser => "Files".to_string(),
                                AppView::Playlist => "Queue".to_string(),
                                AppView::Lyrics => "Lyrics".to_string(),
                                AppView::Recent => "History".to_string(),
                                AppView::Settings => "Settings".to_string(),
                            }
                        } else if *view == AppView::Playlist
                            && self.controller.playlist_count() > 0
                        {
                            format!("Playlist ({})", self.controller.playlist_count())
                        } else {
                            view.label().to_string()
                        };

                        let rich = if is_active {
                            RichText::new(&text)
                                .strong()
                                .color(Color32::from_rgb(95, 195, 255))
                        } else {
                            RichText::new(&text).color(Color32::from_gray(185))
                        };

                        if ui.selectable_label(is_active, rich).clicked() {
                            self.current_view = *view;
                        }
                    }

                    ui.add_space(3.0);
                    ui.separator();
                    ui.add_space(3.0);

                    // User file actions with clean vector icons
                    let (open_text, subs_text) = if available_w > 1050.0 {
                        ("Open File...", "Add Subs...")
                    } else if available_w > 860.0 {
                        ("Open...", "Subs...")
                    } else {
                        ("Open", "Subs")
                    };

                    if icons::icon_text_button(
                        ui,
                        Icon::FolderOpen,
                        open_text,
                        "Open media file from disk (Ctrl+O)",
                    )
                    .clicked()
                    {
                        self.open_file_dialog();
                    }

                    if icons::icon_text_button(
                        ui,
                        Icon::Subtitles,
                        subs_text,
                        "Load external subtitle file (SRT, ASS, VTT)",
                    )
                    .clicked()
                    {
                        self.add_subtitle_dialog();
                    }

                    ui.add_space(3.0);
                    ui.separator();
                    ui.add_space(3.0);

                    // Media file title / label with collision protection
                    let file_label = self
                        .controller
                        .current_file()
                        .map(|path| {
                            std::path::Path::new(path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(path)
                                .to_string()
                        })
                        .unwrap_or_else(|| "No media loaded".to_string());

                    let is_loaded = self.controller.current_file().is_some();
                    let label_color = if is_loaded {
                        Color32::from_gray(230)
                    } else {
                        Color32::from_gray(140)
                    };

                    // Compute dynamic right-side reserved width
                    let (w, h) = self.controller.video_dimensions();
                    let vfmt = self.controller.video_format().to_string();
                    let afmt = self.controller.audio_codec().to_string();
                    let hw = self.controller.hwdec_current().to_string();
                    let show_badge = (w > 0 || !vfmt.is_empty()) && available_w >= 1150.0;

                    let is_player = self.current_view == AppView::Player;
                    let dev_w = 80.0;
                    let help_w = if available_w > 1100.0 { 95.0 } else { 55.0 };
                    let player_extra_w = if is_player { 60.0 } else { 0.0 };
                    let badge_w = if show_badge { 220.0 } else { 0.0 };
                    let total_right_w = dev_w + help_w + player_extra_w + badge_w + 40.0;

                    let remaining_w = ui.available_width();
                    let title_w = (remaining_w - total_right_w).max(40.0);

                    // Allocated middle title with truncation guarantee
                    ui.allocate_ui_with_layout(
                        Vec2::new(title_w, ui.available_height()),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&file_label)
                                        .strong()
                                        .color(label_color),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&file_label);
                        },
                    );

                    // Right-aligned toolbar section
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // In Player view, offer Drawer and PiP toggles
                        if self.current_view == AppView::Player {
                            let drawer_tip = if self.show_playlist_drawer {
                                "Hide Queue Drawer (P)"
                            } else {
                                "Show Queue Drawer (P)"
                            };
                            if icons::icon_button(ui, Icon::Playlist, drawer_tip).clicked() {
                                self.show_playlist_drawer = !self.show_playlist_drawer;
                            }

                            let pip_tip = if self.is_pip {
                                "Exit Compact Mini-Player Mode (I)"
                            } else {
                                "Compact Mini-Player Mode (Always-on-Top, best-effort on Wayland) (I)"
                            };
                            if icons::icon_button(ui, Icon::Pip, pip_tip).clicked() {
                                self.toggle_pip(ctx);
                            }

                            ui.separator();
                        }

                        // Developer & Test Menu (moved out of user toolbar)
                        egui::ComboBox::from_id_salt("dev_menu_combo")
                            .selected_text("Developer")
                            .show_ui(ui, |ui| {
                                let diag_label = if self.show_diagnostics {
                                    "Hide Diagnostics Overlay"
                                } else {
                                    "Show Diagnostics Overlay"
                                };
                                if ui.button(diag_label).clicked() {
                                    self.show_diagnostics = !self.show_diagnostics;
                                }

                                ui.separator();
                                ui.label(
                                    RichText::new("Test Presets")
                                        .size(11.0)
                                        .color(Color32::from_rgb(180, 160, 100)),
                                );

                                if ui.button("H.264 (MP4)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/h264-aac.mp4",
                                        "tests/MuvrixTestMedia/baseline/h264-aac.mp4",
                                    ]);
                                }
                                if ui.button("HEVC (MP4)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/hevc-main-aac.mp4",
                                        "tests/MuvrixTestMedia/codecs/hevc-aac.mp4",
                                    ]);
                                }
                                if ui.button("VP9 (WebM)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/vp9-opus.webm",
                                        "tests/MuvrixTestMedia/codecs/vp9-opus.webm",
                                    ]);
                                }
                                if ui.button("AV1 1080p (WebM)").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/generated/av1-1080p-svtav1-synthetic.webm",
                                        "tests/MuvrixTestMedia/codecs/av1-1080p.webm",
                                        "tests/MuvrixTestMedia/phase-0.4/A-core/av1-4k-cityhall.webm",
                                    ]);
                                }
                                if ui.button("Multi-Audio MOV").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/D-edge-cases/multiple-audio-streams.mov",
                                        "tests/MuvrixTestMedia/tracks/multiple-audio-streams.mov",
                                    ]);
                                }
                                if ui.button("Video-Only MP4").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/D-edge-cases/video-only.mp4",
                                        "tests/MuvrixTestMedia/tracks/video-only.mp4",
                                    ]);
                                }
                                if ui.button("Audio-Only FLAC").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/C-audio/flac-24bit-96khz.flac",
                                        "tests/MuvrixTestMedia/audio/audio.flac",
                                    ]);
                                }
                                if ui.button("Multi-Sub MKV").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/C-subtitles/embedded-srt-ass.mkv",
                                        "tests/MuvrixTestMedia/tracks/synthetic-multi-sub.mkv",
                                    ]);
                                }
                                if ui.button("Surround 7.1 MKV").clicked() {
                                    self.load_preset(&[
                                        "tests/MuvrixTestMedia/phase-0.4/C-audio/surround-7.1-sample.mkv",
                                        "tests/MuvrixTestMedia/tracks/surround-7.1.mkv",
                                        "tests/MuvrixTestMedia/phase-0.4/C-audio/TrueHD Sample 1 (Final Fantasy VII Advent Children).mkv",
                                    ]);
                                }
                            });

                        ui.separator();

                        // Keyboard shortcuts help button
                        let help_text = if available_w > 1100.0 {
                            "Shortcuts (F1)"
                        } else {
                            "Help"
                        };
                        if icons::icon_text_button(
                            ui,
                            Icon::Shortcuts,
                            help_text,
                            "View keyboard shortcuts (F1)",
                        )
                        .clicked()
                        {
                            self.show_shortcuts = !self.show_shortcuts;
                        }

                        // Media metadata badge if active and width allows
                        if show_badge {
                            ui.separator();
                            let v_clean = format_codec_name(&vfmt);
                            let a_clean = format_codec_name(&afmt);
                            let badge = if !a_clean.is_empty() && available_w > 1250.0 {
                                format!("{}×{} · {} · {} · HW: {}", w, h, v_clean, a_clean, hw)
                            } else {
                                format!("{}×{} · {} · HW: {}", w, h, v_clean, hw)
                            };
                            ui.label(
                                RichText::new(badge)
                                    .monospace()
                                    .size(11.0)
                                    .color(Color32::from_rgb(115, 205, 255)),
                            );
                        }
                    });
                });
                ui.add_space(3.0);
            });

        // ---------------------------------------------------------------------
        // Active View Dispatch (Player, Library, Browser, Playlist, Lyrics, History, Settings)
        // ---------------------------------------------------------------------
        if self.current_view != AppView::Player {
            self.render_mini_player_deck(ctx);
            match self.current_view {
                AppView::Library => self.render_library_view(ctx),
                AppView::Browser => self.render_browser_view(ctx),
                AppView::Playlist => self.render_playlist_view(ctx),
                AppView::Lyrics => self.render_lyrics_view(ctx),
                AppView::Recent => self.render_recent_view(ctx),
                AppView::Settings => self.render_settings_view(ctx),
                AppView::Player => unreachable!(),
            }
        } else {
            self.render_playlist_drawer(ctx);

            // -----------------------------------------------------------------
            // Optional Diagnostics Bar (when enabled via Developer menu)
            // -----------------------------------------------------------------
            if self.show_diagnostics {
                egui::TopBottomPanel::top("diagnostics_subpanel")
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.add_space(2.0);
                        ui.horizontal_wrapped(|ui| {
                            icons::icon_label(
                                ui,
                                Icon::Diagnostics,
                                14.0,
                                Color32::from_rgb(255, 180, 50),
                            );
                            ui.label(
                                RichText::new("Developer Diagnostics")
                                    .strong()
                                    .color(Color32::from_rgb(255, 180, 50)),
                            );
                            ui.separator();
                            ui.label(
                                RichText::new(format!(
                                    "GL: {} | Renderer: {} | Vendor: {}",
                                    self.gl_version, self.gpu_renderer, self.gpu_vendor
                                ))
                                .monospace()
                                .color(Color32::from_gray(160)),
                            );

                            // Observed (never requested) decode + presentation state.
                            let decode_line = self
                                .decode_observation
                                .as_ref()
                                .map(|o| o.describe())
                                .unwrap_or_else(|| "Decoder: not yet observed".to_string());
                            ui.label(
                                RichText::new(format!(
                                    "{decode_line} | native display: {}",
                                    self.native_display_kind.unwrap_or("none")
                                ))
                                .monospace()
                                .color(Color32::from_rgb(120, 200, 255)),
                            );
                            if let Some(hdr) = &self.hdr_state {
                                if hdr.source.is_hdr() {
                                    ui.label(
                                        RichText::new(format!(
                                            "HDR: {} | mode {:?} | tone-mapped: {} | HDR output: {}",
                                            hdr.source.label(),
                                            hdr.mode,
                                            hdr.tone_mapping,
                                            hdr.hdr_presented
                                        ))
                                        .monospace()
                                        .color(Color32::from_rgb(255, 200, 120)),
                                    );
                                }
                            }

                            ui.separator();
                            let source_fps = self
                                .controller
                                .container_fps()
                                .or_else(|| self.controller.estimated_vf_fps())
                                .unwrap_or(0.0);
                            let drops = self.controller.frame_drop_count();
                            ui.label(
                                RichText::new(format!(
                                    "UI: {:.1} FPS ({:.1} ms) | Video: {:.1} fps ({} drops)",
                                    self.fps_measured, self.frame_time_ms, source_fps, drops
                                ))
                                .monospace()
                                .color(Color32::from_rgb(0, 255, 128)),
                            );

                            ui.separator();
                            ui.label("Path/URL:");
                            let text_response = ui.add(
                                egui::TextEdit::singleline(&mut self.path_input)
                                    .hint_text("Enter media path or URL...")
                                    .desired_width(180.0),
                            );
                            if (ui.button("Load").clicked()
                                || (text_response.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                                && !self.path_input.trim().is_empty()
                            {
                                let path = self.path_input.clone();
                                self.load_file(&path);
                            }

                            if ui.button("Dismiss [x]").clicked() {
                                self.show_diagnostics = false;
                            }
                        });
                        ui.add_space(2.0);
                    });
            }

            // ---------------------------------------------------------------------
            // 2. Playback Controls Bottom Deck
            // ---------------------------------------------------------------------
            egui::TopBottomPanel::bottom("player_controls")
                .resizable(false)
                .show(ctx, |ui| {
                    ui.add_space(4.0);

                    let deck_width = ui.available_width();

                    // -------------------------------------------------------------
                    // Row 1: Dedicated Timeline Row
                    // -------------------------------------------------------------
                    let time_pos = self.controller.time_pos();
                    let duration = self.controller.duration();

                    ui.horizontal(|ui| {
                        let cur_str = format_timestamp(time_pos);
                        let dur_str = format_timestamp(duration);
                        let remaining_str = format_timestamp((duration - time_pos).max(0.0));

                        // Current position timestamp
                        ui.label(
                            RichText::new(&cur_str)
                                .monospace()
                                .size(12.0)
                                .color(Color32::from_gray(215)),
                        );

                        // Full-width seeker slider with hover timestamp tooltip
                        let max_dur = if duration > 0.0 { duration } else { 1.0 };
                        let mut slider_val = if self.is_seeking {
                            self.seek_target
                        } else {
                            time_pos
                        };

                        let slider_width = (ui.available_width() - 54.0).max(60.0);
                        let slider = egui::Slider::new(&mut slider_val, 0.0..=max_dur)
                            .show_value(false)
                            .trailing_fill(true);

                        let response = ui.add_sized([slider_width, 18.0], slider);

                        // Context-aware hover information: chapter title, bookmark pin details, or seek timestamp
                        let mut custom_hover: Option<String> = None;
                        if let Some(mouse_pos) = response.hover_pos() {
                            if duration > 0.0 {
                                for bm in &self.active_bookmarks {
                                    let frac =
                                        (bm.timestamp_seconds() / duration).clamp(0.0, 1.0) as f32;
                                    let x = response.rect.min.x + frac * response.rect.width();
                                    if (mouse_pos.x - x).abs() <= 7.0 {
                                        let note_str = bm
                                            .note
                                            .as_deref()
                                            .map(|n| format!(" — {}", n))
                                            .unwrap_or_default();
                                        custom_hover = Some(format!(
                                            "🔖 {} ({}){}",
                                            bm.label,
                                            format_timestamp(bm.timestamp_seconds()),
                                            note_str
                                        ));
                                        break;
                                    }
                                }
                                if custom_hover.is_none() {
                                    for ch in self.controller.chapters() {
                                        let frac = (ch.time / duration).clamp(0.0, 1.0) as f32;
                                        let x = response.rect.min.x + frac * response.rect.width();
                                        if (mouse_pos.x - x).abs() <= 7.0 {
                                            custom_hover = Some(format!(
                                                "📑 {} ({})",
                                                ch.title,
                                                format_timestamp(ch.time)
                                            ));
                                            break;
                                        }
                                    }
                                }
                            }
                        }

                        let tooltip = custom_hover
                            .unwrap_or_else(|| format!("Seek to {}", format_timestamp(slider_val)));
                        let response = response.on_hover_text(tooltip);

                        // Draw visual chapter marks on slider track
                        let chapters = self.controller.chapters();
                        if duration > 0.0 && !chapters.is_empty() {
                            let painter = ui.painter();
                            for ch in chapters {
                                let frac = (ch.time / duration).clamp(0.0, 1.0) as f32;
                                let x = response.rect.min.x + frac * response.rect.width();
                                painter.line_segment(
                                    [
                                        Pos2::new(x, response.rect.min.y + 1.0),
                                        Pos2::new(x, response.rect.max.y - 1.0),
                                    ],
                                    Stroke::new(2.0_f32, Color32::from_rgb(56, 189, 248)),
                                );
                                painter.circle_filled(
                                    Pos2::new(x, response.rect.min.y - 1.0),
                                    2.5,
                                    Color32::from_rgb(56, 189, 248),
                                );
                            }
                        }

                        // Draw visual bookmark pins on slider track
                        if duration > 0.0 && !self.active_bookmarks.is_empty() {
                            let painter = ui.painter();
                            for bm in &self.active_bookmarks {
                                let frac =
                                    (bm.timestamp_seconds() / duration).clamp(0.0, 1.0) as f32;
                                let x = response.rect.min.x + frac * response.rect.width();
                                painter.line_segment(
                                    [
                                        Pos2::new(x, response.rect.min.y),
                                        Pos2::new(x, response.rect.max.y),
                                    ],
                                    Stroke::new(2.5_f32, Color32::from_rgb(245, 158, 11)),
                                );
                                let head = vec![
                                    Pos2::new(x, response.rect.min.y - 4.0),
                                    Pos2::new(x + 4.0, response.rect.min.y + 1.0),
                                    Pos2::new(x, response.rect.min.y + 6.0),
                                    Pos2::new(x - 4.0, response.rect.min.y + 1.0),
                                ];
                                painter.add(Shape::convex_polygon(
                                    head,
                                    Color32::from_rgb(245, 158, 11),
                                    Stroke::new(1.0_f32, Color32::from_rgb(180, 83, 9)),
                                ));
                            }
                        }

                        if response.drag_started() {
                            self.is_seeking = true;
                            self.seek_target = slider_val;
                        }
                        if response.dragged() {
                            self.seek_target = slider_val;
                        }
                        if response.drag_stopped() {
                            let _ = self.controller.seek(self.seek_target, SeekMode::Absolute);
                            self.is_seeking = false;
                        }

                        // Duration timestamp
                        ui.label(
                            RichText::new(&dur_str)
                                .monospace()
                                .size(12.0)
                                .color(Color32::from_gray(185)),
                        )
                        .on_hover_text(format!(
                            "Duration: {} (Remaining: -{})",
                            dur_str, remaining_str
                        ));
                    });

                    ui.add_space(3.0);

                    // -------------------------------------------------------------
                    // Row 2: Transport Buttons, Speed, Volume, Tracks, Fullscreen
                    // -------------------------------------------------------------
                    ui.horizontal(|ui| {
                        // Group 1: Transport Controls (Primary Play/Pause, Stop, Seeks)
                        let (play_icon, play_tip) = match self.controller.state() {
                            PlaybackState::Playing => (Icon::Pause, "Pause (Space)"),
                            _ => (Icon::Play, "Play (Space)"),
                        };

                        // Primary Play/Pause button
                        if icons::primary_icon_button(
                            ui,
                            play_icon,
                            Vec2::new(34.0, 26.0),
                            play_tip,
                        )
                        .clicked()
                        {
                            self.toggle_pause();
                        }

                        ui.add_space(2.0);

                        // Stop button
                        if icons::icon_button_sized(
                            ui,
                            Icon::Stop,
                            Vec2::new(26.0, 26.0),
                            "Stop playback",
                        )
                        .clicked()
                        {
                            let _ = self.controller.stop();
                        }

                        ui.add_space(2.0);

                        // Relative seek -5s / +5s
                        if icons::icon_button_sized(
                            ui,
                            Icon::SeekBackward,
                            Vec2::new(28.0, 26.0),
                            "Seek backward 5s (Left Arrow)",
                        )
                        .clicked()
                        {
                            self.seek_relative(-5.0);
                        }

                        if icons::icon_button_sized(
                            ui,
                            Icon::SeekForward,
                            Vec2::new(28.0, 26.0),
                            "Seek forward 5s (Right Arrow)",
                        )
                        .clicked()
                        {
                            self.seek_relative(5.0);
                        }

                        // Chapter navigation if present
                        let chapters = self.controller.chapters().to_vec();
                        if !chapters.is_empty() {
                            ui.add_space(2.0);
                            if icons::icon_button(ui, Icon::SkipPrevious, "Previous Chapter")
                                .clicked()
                            {
                                let _ = self.controller.prev_chapter();
                            }
                            if icons::icon_button(ui, Icon::SkipNext, "Next Chapter").clicked() {
                                let _ = self.controller.next_chapter();
                            }

                            let cur_ch_idx = self.controller.current_chapter().unwrap_or(0);
                            let selected_title = chapters
                                .iter()
                                .find(|c| c.index == cur_ch_idx)
                                .map(|c| {
                                    if deck_width < 960.0 {
                                        format!("Ch {}", c.index + 1)
                                    } else {
                                        format!("Ch {}: {}", c.index + 1, c.title)
                                    }
                                })
                                .unwrap_or_else(|| "Chapters".to_string());

                            egui::ComboBox::from_id_salt("chapters_combo")
                                .selected_text(selected_title)
                                .show_ui(ui, |ui| {
                                    for ch in &chapters {
                                        let label = format!(
                                            "{}: {} ({})",
                                            ch.index + 1,
                                            ch.title,
                                            format_timestamp(ch.time)
                                        );
                                        let is_selected =
                                            Some(ch.index) == self.controller.current_chapter();
                                        if ui.selectable_label(is_selected, label).clicked() {
                                            let _ = self.controller.set_chapter(ch.index);
                                        }
                                    }
                                });
                        }

                        // Bookmark actions
                        if self.controller.current_file().is_some() {
                            ui.add_space(2.0);
                            let bm_btn_text = if deck_width < 960.0 {
                                "🔖"
                            } else {
                                "🔖 Bookmark"
                            };
                            if ui
                                .button(RichText::new(bm_btn_text).size(11.5))
                                .on_hover_text("Add bookmark at current playback position")
                                .clicked()
                            {
                                let label = format!("Bookmark {}", format_timestamp(time_pos));
                                self.add_bookmark(&label, None);
                            }

                            if !self.active_bookmarks.is_empty() {
                                ui.add_space(2.0);
                                egui::ComboBox::from_id_salt("bookmarks_combo")
                                    .selected_text(format!(
                                        "Bookmarks ({})",
                                        self.active_bookmarks.len()
                                    ))
                                    .show_ui(ui, |ui| {
                                        for bm in &self.active_bookmarks {
                                            let label = format!(
                                                "{} ({})",
                                                bm.label,
                                                format_timestamp(bm.timestamp_seconds())
                                            );
                                            if ui.button(label).clicked() {
                                                let _ = self.controller.seek(
                                                    bm.timestamp_seconds(),
                                                    SeekMode::Absolute,
                                                );
                                            }
                                        }
                                    });
                            }
                        }

                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);

                        // Group 2: Volume & Mute Controls
                        let (mute_icon, mute_tip) = if self.controller.is_muted() {
                            (Icon::VolumeMuted, "Unmute Audio (M)")
                        } else if self.controller.volume() < 40.0 {
                            (Icon::VolumeLow, "Mute Audio (M)")
                        } else {
                            (Icon::VolumeHigh, "Mute Audio (M)")
                        };

                        if icons::icon_button_sized(ui, mute_icon, Vec2::new(26.0, 26.0), mute_tip)
                            .clicked()
                        {
                            let _ = self.controller.toggle_mute();
                        }

                        let mut vol = self.controller.volume();
                        let vol_label = format!("{:.0}%", vol);
                        let vol_slider = egui::Slider::new(&mut vol, 0.0..=100.0)
                            .show_value(false)
                            .text(vol_label);
                        let vol_w = if deck_width < 960.0 { 38.0 } else { 65.0 };
                        let vol_res = ui
                            .add_sized([vol_w, 18.0], vol_slider)
                            .on_hover_text(format!("Volume: {:.0}% (Up/Down Arrow)", vol));
                        if vol_res.changed() {
                            let _ = self.controller.set_volume(vol);
                        }

                        ui.add_space(3.0);
                        ui.separator();
                        ui.add_space(3.0);

                        // Group 3: Playback Speed Selector
                        let current_speed = self.controller.speed();
                        let speed_label = if (current_speed - 1.0).abs() < 0.01 {
                            "1.0x".to_string()
                        } else {
                            format!("{:.2}x", current_speed)
                        };

                        egui::ComboBox::from_id_salt("speed_combo")
                            .selected_text(speed_label)
                            .show_ui(ui, |ui| {
                                for &preset in &[0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0] {
                                    let label = format!(
                                        "{preset}x{}",
                                        if preset == 1.0 { " (Normal)" } else { "" }
                                    );
                                    let is_current = (current_speed - preset).abs() < 0.01;
                                    if ui.selectable_label(is_current, label).clicked() {
                                        let _ = self.controller.set_speed(preset);
                                    }
                                }
                            });

                        if deck_width >= 960.0
                            && (current_speed - 1.0).abs() > 0.01
                            && ui
                                .button("1.0x")
                                .on_hover_text("Reset to normal speed (Backspace)")
                                .clicked()
                        {
                            let _ = self.controller.reset_speed();
                        }

                        // Group 4: Right-aligned Controls (Fullscreen, Subs, Audio)
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Fullscreen toggle button
                            let (fs_icon, fs_tip) = if self.is_fullscreen {
                                (Icon::Windowed, "Exit Fullscreen (F / Esc)")
                            } else {
                                (Icon::Fullscreen, "Enter Fullscreen (F / F11)")
                            };
                            if icons::icon_button_sized(ui, fs_icon, Vec2::new(28.0, 26.0), fs_tip)
                                .clicked()
                            {
                                self.toggle_fullscreen(ctx);
                            }

                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(4.0);

                            let sub_tracks: Vec<TrackInfo> = self
                                .controller
                                .tracks_by_type(TrackType::Subtitle)
                                .into_iter()
                                .cloned()
                                .collect();
                            let selected_sub_id = self
                                .controller
                                .selected_track(TrackType::Subtitle)
                                .map(|t| t.id);

                            let audio_tracks: Vec<TrackInfo> = self
                                .controller
                                .tracks_by_type(TrackType::Audio)
                                .into_iter()
                                .cloned()
                                .collect();
                            let selected_audio_id = self
                                .controller
                                .selected_track(TrackType::Audio)
                                .map(|t| t.id);

                            // If width is constrained (<760px), consolidate into a single contextual "Tracks" menu
                            if deck_width < 760.0 {
                                egui::ComboBox::from_id_salt("tracks_combo_compact")
                                    .selected_text("Tracks")
                                    .show_ui(ui, |ui| {
                                        ui.label(
                                            RichText::new("Audio Track")
                                                .strong()
                                                .color(Color32::from_rgb(120, 200, 255)),
                                        );
                                        if ui
                                            .selectable_label(
                                                selected_audio_id.is_none(),
                                                "None (Mute Stream)",
                                            )
                                            .clicked()
                                        {
                                            let _ = self.controller.set_audio_track(None);
                                        }
                                        for track in &audio_tracks {
                                            let label = format_track_label(track);
                                            let is_selected = selected_audio_id == Some(track.id);
                                            if ui.selectable_label(is_selected, label).clicked() {
                                                let _ =
                                                    self.controller.set_audio_track(Some(track.id));
                                            }
                                        }

                                        ui.separator();
                                        ui.label(
                                            RichText::new("Subtitle Track")
                                                .strong()
                                                .color(Color32::from_rgb(120, 200, 255)),
                                        );
                                        if ui
                                            .selectable_label(
                                                selected_sub_id.is_none(),
                                                "Off (Disabled)",
                                            )
                                            .clicked()
                                        {
                                            let _ = self.controller.set_subtitle_track(None);
                                        }
                                        for track in &sub_tracks {
                                            let label = format_track_label(track);
                                            let is_selected = selected_sub_id == Some(track.id);
                                            if ui.selectable_label(is_selected, label).clicked() {
                                                let _ = self
                                                    .controller
                                                    .set_subtitle_track(Some(track.id));
                                            }
                                        }

                                        ui.separator();
                                        if icons::icon_text_button(
                                            ui,
                                            Icon::FolderOpen,
                                            "Add Subtitle File...",
                                            "Load external subtitle file",
                                        )
                                        .clicked()
                                        {
                                            self.add_subtitle_dialog();
                                        }
                                    });
                            } else {
                                // Subtitle track selector
                                let sub_summary = if let Some(id) = selected_sub_id {
                                    if let Some(t) = sub_tracks.iter().find(|t| t.id == id) {
                                        let label = t
                                            .lang
                                            .as_deref()
                                            .unwrap_or(t.title.as_deref().unwrap_or("On"));
                                        if deck_width < 960.0 {
                                            format!("Sub: {label}")
                                        } else {
                                            format!("Subtitles: {label}")
                                        }
                                    } else if deck_width < 960.0 {
                                        "Sub: On".to_string()
                                    } else {
                                        "Subtitles: On".to_string()
                                    }
                                } else if deck_width < 960.0 {
                                    "Sub: Off".to_string()
                                } else {
                                    "Subtitles: Off".to_string()
                                };

                                egui::ComboBox::from_id_salt("sub_combo")
                                    .selected_text(sub_summary)
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(
                                                selected_sub_id.is_none(),
                                                "Off (Disabled)",
                                            )
                                            .clicked()
                                        {
                                            let _ = self.controller.set_subtitle_track(None);
                                        }
                                        for track in &sub_tracks {
                                            let label = format_track_label(track);
                                            let is_selected = selected_sub_id == Some(track.id);
                                            if ui.selectable_label(is_selected, label).clicked() {
                                                let _ = self
                                                    .controller
                                                    .set_subtitle_track(Some(track.id));
                                            }
                                        }

                                        ui.separator();
                                        if icons::icon_text_button(
                                            ui,
                                            Icon::FolderOpen,
                                            "Add Subtitle File...",
                                            "Load external subtitle file",
                                        )
                                        .clicked()
                                        {
                                            self.add_subtitle_dialog();
                                        }

                                        let cur_delay = self.controller.sub_delay();
                                        ui.horizontal(|ui| {
                                            ui.label(format!("Delay: {:.2}s", cur_delay));
                                            if ui.button("-0.25s").clicked() {
                                                let _ = self
                                                    .controller
                                                    .set_subtitle_delay(cur_delay - 0.25);
                                            }
                                            if ui.button("0s").clicked() {
                                                let _ = self.controller.set_subtitle_delay(0.0);
                                            }
                                            if ui.button("+0.25s").clicked() {
                                                let _ = self
                                                    .controller
                                                    .set_subtitle_delay(cur_delay + 0.25);
                                            }
                                        });

                                        let mut vis = self.controller.sub_visibility();
                                        if ui.checkbox(&mut vis, "Subtitle Visible").changed() {
                                            let _ = self.controller.set_subtitle_visibility(vis);
                                        }
                                    });

                                ui.add_space(3.0);

                                // Audio track selector
                                let audio_summary = if let Some(id) = selected_audio_id {
                                    if let Some(t) = audio_tracks.iter().find(|t| t.id == id) {
                                        let label = t
                                            .lang
                                            .as_deref()
                                            .unwrap_or(t.title.as_deref().unwrap_or("On"));
                                        if deck_width < 960.0 {
                                            format!("Aud: {label}")
                                        } else {
                                            format!("Audio: {label}")
                                        }
                                    } else if deck_width < 960.0 {
                                        "Aud: On".to_string()
                                    } else {
                                        "Audio: On".to_string()
                                    }
                                } else if deck_width < 960.0 {
                                    "Aud: None".to_string()
                                } else {
                                    "Audio: None".to_string()
                                };

                                egui::ComboBox::from_id_salt("audio_combo")
                                    .selected_text(audio_summary)
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(
                                                selected_audio_id.is_none(),
                                                "None (Mute Stream)",
                                            )
                                            .clicked()
                                        {
                                            let _ = self.controller.set_audio_track(None);
                                        }
                                        for track in &audio_tracks {
                                            let label = format_track_label(track);
                                            let is_selected = selected_audio_id == Some(track.id);
                                            if ui.selectable_label(is_selected, label).clicked() {
                                                let _ =
                                                    self.controller.set_audio_track(Some(track.id));
                                            }
                                        }
                                    });
                            }
                        });
                    });

                    ui.add_space(3.0);
                });

            // ---------------------------------------------------------------------
            // 3. Central Video Viewport with Interactive Overlay Cards
            // ---------------------------------------------------------------------
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
                .show(ctx, |ui| {
                    let available_size = ui.available_size();
                    if available_size.x <= 1.0 || available_size.y <= 1.0 {
                        return;
                    }

                    // Sense both clicks (play/pause) and double-clicks (fullscreen)
                    let (rect, response) = ui.allocate_exact_size(available_size, Sense::click());

                    if response.double_clicked() {
                        self.toggle_fullscreen(ctx);
                    } else if response.clicked() && *self.controller.state() != PlaybackState::Idle
                    {
                        self.toggle_pause();
                    }

                    // Render video quad via libmpv OpenGL FBO
                    let render_engine = self.render_engine.clone();
                    let callback = egui_glow::CallbackFn::new(move |info, painter| {
                        let mut engine = render_engine.lock().unwrap();
                        let viewport_px = info.viewport_in_pixels();
                        unsafe {
                            engine.paint(
                                painter.gl(),
                                painter,
                                [
                                    viewport_px.left_px,
                                    viewport_px.from_bottom_px,
                                    viewport_px.width_px,
                                    viewport_px.height_px,
                                ],
                            );
                        }
                    });

                    ui.painter().add(egui::PaintCallback {
                        rect,
                        callback: Arc::new(callback),
                    });

                    // Interactive Resume Playback Prompt Banner (Feature Group C)
                    if let Some((_, pos)) = self.pending_resume_prompt {
                        let time_str = format_timestamp(pos);

                        let banner_w = 460.0_f32.min(rect.width() - 32.0);
                        let banner_rect = Rect::from_min_size(
                            Pos2::new(rect.center().x - banner_w * 0.5, rect.min.y + 16.0),
                            Vec2::new(banner_w, 42.0),
                        );

                        ui.painter().rect(
                            banner_rect,
                            6.0_f32,
                            Color32::from_rgba_premultiplied(18, 24, 38, 235),
                            Stroke::new(1.0_f32, Color32::from_rgb(60, 140, 240)),
                            StrokeKind::Inside,
                        );

                        let mut banner_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(banner_rect.shrink(6.0))
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );

                        icons::icon_label(
                            &mut banner_ui,
                            Icon::History,
                            16.0,
                            Color32::from_rgb(85, 180, 255),
                        );
                        banner_ui.add_space(4.0);
                        banner_ui.label(
                            RichText::new(format!("Resume from {time_str}?"))
                                .strong()
                                .size(12.5)
                                .color(Color32::WHITE),
                        );

                        banner_ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui.button("Dismiss [x]").clicked() {
                                    self.pending_resume_prompt = None;
                                }
                                if ui.button("Start 00:00").clicked() {
                                    let _ = self.controller.seek(0.0, SeekMode::Absolute);
                                    self.pending_resume_prompt = None;
                                }
                                if ui
                                    .button(
                                        RichText::new("Resume")
                                            .strong()
                                            .color(Color32::from_rgb(90, 190, 255)),
                                    )
                                    .clicked()
                                {
                                    let _ = self.controller.seek(pos, SeekMode::Absolute);
                                    let _ = self.controller.play();
                                    self.pending_resume_prompt = None;
                                }
                            },
                        );
                    }

                    // Contextual Overlays for player states
                    match self.controller.state() {
                        PlaybackState::Idle => {
                            let center_pos = rect.center();
                            let card_width = 410.0_f32.min(rect.width() - 32.0);
                            let card_height = 205.0_f32.min(rect.height() - 32.0);
                            let card_rect = egui::Rect::from_center_size(
                                center_pos,
                                Vec2::new(card_width, card_height),
                            );

                            // High contrast welcome card container
                            ui.painter().rect(
                                card_rect,
                                8.0_f32,
                                Color32::from_rgba_premultiplied(22, 25, 34, 235),
                                Stroke::new(1.0_f32, Color32::from_rgb(55, 68, 92)),
                                egui::StrokeKind::Inside,
                            );

                            let mut child_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(card_rect.shrink(18.0))
                                    .layout(egui::Layout::top_down(egui::Align::Center)),
                            );
                            child_ui.add_space(6.0);
                            child_ui.label(
                                RichText::new("MUVRIX MEDIA PLAYER")
                                    .strong()
                                    .size(19.0)
                                    .color(Color32::from_rgb(96, 175, 255)),
                            );
                            child_ui.add_space(3.0);
                            child_ui.label(
                                RichText::new(
                                    "Lightweight hardware-accelerated desktop media player",
                                )
                                .size(12.5)
                                .color(Color32::from_gray(190)),
                            );
                            child_ui.add_space(14.0);

                            // Primary action button: Open Media File
                            if icons::primary_icon_button_text(
                                &mut child_ui,
                                Icon::FolderOpen,
                                "Open Media File",
                                "Open video or audio file from disk (Ctrl+O)",
                            )
                            .clicked()
                            {
                                self.open_file_dialog();
                            }

                            child_ui.add_space(10.0);
                            child_ui.label(
                                RichText::new("MP4 · MKV · WebM · AVI · MOV · FLAC · MP3 · AAC")
                                    .size(11.5)
                                    .color(Color32::from_gray(165)),
                            );
                            child_ui.add_space(2.0);
                            child_ui.label(
                                RichText::new("Drag and drop media or press Ctrl+O")
                                    .size(10.5)
                                    .color(Color32::from_gray(135)),
                            );
                        }
                        PlaybackState::Loading | PlaybackState::Buffering => {
                            let center_pos = rect.center();
                            let card_rect =
                                egui::Rect::from_center_size(center_pos, Vec2::new(300.0, 115.0));
                            ui.painter().rect(
                                card_rect,
                                6.0_f32,
                                Color32::from_rgba_premultiplied(18, 20, 28, 225),
                                Stroke::new(1.0_f32, Color32::from_rgb(60, 68, 85)),
                                egui::StrokeKind::Inside,
                            );
                            let mut child_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(card_rect.shrink(12.0))
                                    .layout(egui::Layout::top_down(egui::Align::Center)),
                            );
                            child_ui.add_space(8.0);

                            let is_loading = *self.controller.state() == PlaybackState::Loading;
                            child_ui.label(
                                RichText::new(if is_loading {
                                    "Loading Media..."
                                } else {
                                    "Buffering..."
                                })
                                .strong()
                                .size(15.0)
                                .color(Color32::from_rgb(255, 205, 60)),
                            );
                            child_ui.add_space(6.0);
                            let file_hint = self
                                .controller
                                .current_file()
                                .map(|f| {
                                    std::path::Path::new(f)
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(f)
                                })
                                .unwrap_or("Please wait");
                            child_ui.label(
                                RichText::new(file_hint)
                                    .size(12.0)
                                    .color(Color32::from_gray(200)),
                            );
                        }
                        PlaybackState::Error(err) => {
                            let center_pos = rect.center();
                            let card_rect =
                                egui::Rect::from_center_size(center_pos, Vec2::new(460.0, 200.0));
                            ui.painter().rect(
                                card_rect,
                                8.0_f32,
                                Color32::from_rgba_premultiplied(35, 15, 15, 240),
                                Stroke::new(1.5_f32, Color32::from_rgb(220, 60, 60)),
                                egui::StrokeKind::Inside,
                            );

                            let mut child_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(card_rect.shrink(16.0))
                                    .layout(egui::Layout::top_down(egui::Align::Center)),
                            );
                            child_ui.add_space(4.0);
                            child_ui.horizontal(|ui| {
                                icons::icon_label(
                                    ui,
                                    Icon::Warning,
                                    18.0,
                                    Color32::from_rgb(255, 80, 80),
                                );
                                ui.label(
                                    RichText::new("Playback Error")
                                        .strong()
                                        .size(16.0)
                                        .color(Color32::from_rgb(255, 80, 80)),
                                );
                            });
                            child_ui.add_space(8.0);
                            child_ui.label(
                                RichText::new(err)
                                    .size(12.0)
                                    .color(Color32::from_rgb(240, 190, 190)),
                            );
                            child_ui.add_space(14.0);
                            child_ui.horizontal(|ui| {
                                if ui
                                    .button(RichText::new("Open Another File...").strong())
                                    .clicked()
                                {
                                    self.open_file_dialog();
                                }
                                if ui.button("Dismiss").clicked() {
                                    let _ = self.controller.stop();
                                }
                            });
                        }
                        PlaybackState::Ended => {
                            let center_pos = rect.center();
                            let card_rect =
                                egui::Rect::from_center_size(center_pos, Vec2::new(320.0, 135.0));
                            ui.painter().rect(
                                card_rect,
                                8.0_f32,
                                Color32::from_rgba_premultiplied(20, 22, 30, 230),
                                Stroke::new(1.0_f32, Color32::from_rgb(55, 65, 85)),
                                egui::StrokeKind::Inside,
                            );
                            let mut child_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(card_rect.shrink(14.0))
                                    .layout(egui::Layout::top_down(egui::Align::Center)),
                            );
                            child_ui.add_space(6.0);
                            child_ui.label(
                                RichText::new("Playback Finished")
                                    .strong()
                                    .size(15.0)
                                    .color(Color32::from_gray(230)),
                            );
                            child_ui.add_space(12.0);
                            child_ui.horizontal(|ui| {
                                if icons::icon_text_button(
                                    ui,
                                    Icon::Replay,
                                    "Replay",
                                    "Replay from start",
                                )
                                .clicked()
                                {
                                    let _ = self.controller.seek(0.0, SeekMode::Absolute);
                                    let _ = self.controller.play();
                                }
                                if icons::icon_text_button(
                                    ui,
                                    Icon::FolderOpen,
                                    "Open File...",
                                    "Open another media file",
                                )
                                .clicked()
                                {
                                    self.open_file_dialog();
                                }
                            });
                        }
                        _ => {}
                    }
                });
        }

        // ---------------------------------------------------------------------
        // 4. Modal Shortcuts Dialog
        // ---------------------------------------------------------------------
        if self.show_shortcuts {
            let mut open = true;
            let mut close_requested = false;
            egui::Window::new("Keyboard Shortcuts")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_width(380.0);
                    egui::Grid::new("shortcuts_grid")
                        .striped(true)
                        .spacing([20.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("Space").strong().monospace());
                            ui.label("Play / Pause Toggle");
                            ui.end_row();

                            ui.label(RichText::new("Left / Right").strong().monospace());
                            ui.label("Seek -5s / +5s");
                            ui.end_row();

                            ui.label(RichText::new("Up / Down").strong().monospace());
                            ui.label("Volume +5% / -5%");
                            ui.end_row();

                            ui.label(RichText::new("M").strong().monospace());
                            ui.label("Mute / Unmute Audio");
                            ui.end_row();

                            ui.label(RichText::new("F / F11").strong().monospace());
                            ui.label("Toggle Fullscreen");
                            ui.end_row();

                            ui.label(RichText::new("Esc").strong().monospace());
                            ui.label("Exit Fullscreen / Dismiss");
                            ui.end_row();

                            ui.label(RichText::new("[ / ]").strong().monospace());
                            ui.label("Speed -0.25x / +0.25x");
                            ui.end_row();

                            ui.label(RichText::new("Backspace").strong().monospace());
                            ui.label("Reset Speed to 1.0x");
                            ui.end_row();

                            ui.label(RichText::new("Ctrl+O").strong().monospace());
                            ui.label("Open Media File");
                            ui.end_row();

                            ui.label(RichText::new("P").strong().monospace());
                            ui.label("Toggle Queue Drawer");
                            ui.end_row();

                            ui.label(RichText::new("B").strong().monospace());
                            ui.label("Toggle Media Browser");
                            ui.end_row();

                            ui.label(RichText::new("I").strong().monospace());
                            ui.label("Toggle Compact Mini-Player");
                            ui.end_row();

                            ui.label(RichText::new("Double Click").strong().monospace());
                            ui.label("Toggle Fullscreen");
                            ui.end_row();

                            ui.label(RichText::new("Single Click").strong().monospace());
                            ui.label("Play / Pause (Video)");
                            ui.end_row();
                        });
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                });
            self.show_shortcuts = open && !close_requested;
        }
    }

    /// Releases GL and engine resources. Must run with the context current.
    pub fn shutdown(&mut self, gl: Option<&glow::Context>) {
        log::info!("Initiating safe teardown of MuvrixApp");

        // Persist final playback position in history store
        if let Some(path) = self.controller.current_file() {
            let time_pos = self.controller.time_pos();
            let duration = self.controller.duration();
            if time_pos > 0.0 && duration > 0.0 {
                self.history_store.record_playback(path, time_pos, duration);
                let _ = self.history_store.save();
            }
        }
        self.save_media_settings();
        if let Ok(mut engine) = self.render_engine.lock() {
            if let Some(gl_ctx) = gl {
                unsafe {
                    engine.destroy(gl_ctx);
                }
            } else {
                unsafe {
                    engine.destroy(&self.gl);
                }
            }
        }
        log::info!("MuvrixApp teardown complete");
    }
}

impl eframe::App for MuvrixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_update(ctx);
    }

    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        self.shutdown(gl);
    }
}

/// Formats seconds into standard HH:MM:SS or MM:SS presentation.
pub fn format_timestamp(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "00:00".to_string();
    }
    let total_secs = seconds.floor() as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

/// Calculates normalized progress fraction clamped to [0.0, 1.0].
pub fn calculate_progress(time_pos: f64, duration: f64) -> f32 {
    if duration <= 0.0 || time_pos <= 0.0 || time_pos.is_nan() || duration.is_nan() {
        0.0
    } else {
        ((time_pos / duration) as f32).clamp(0.0, 1.0)
    }
}

/// Formats playback speed factor into a concise display string.
pub fn format_speed(speed: f64) -> String {
    if (speed - 1.0).abs() < 0.001 {
        "1.0x (Normal)".to_string()
    } else {
        format!("{:.2}x", speed)
    }
}

/// Formats volume and mute state for user display.
pub fn format_volume(volume: f64, is_muted: bool) -> String {
    if is_muted {
        "Muted (0%)".to_string()
    } else {
        format!("{:.0}%", volume.clamp(0.0, 100.0))
    }
}

/// Formats a raw codec name into standard user-facing presentation.
pub fn format_codec_name(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "h264" | "avc" | "avc1" => "H.264".to_string(),
        "hevc" | "h265" | "hev1" => "HEVC".to_string(),
        "vp9" => "VP9".to_string(),
        "av1" | "av01" => "AV1".to_string(),
        "vp8" => "VP8".to_string(),
        "aac" | "mp4a" => "AAC".to_string(),
        "opus" => "Opus".to_string(),
        "flac" => "FLAC".to_string(),
        "mp3" => "MP3".to_string(),
        "ac3" => "AC-3".to_string(),
        "eac3" => "E-AC-3".to_string(),
        "truehd" => "TrueHD".to_string(),
        "dts" => "DTS".to_string(),
        "vorbis" => "Vorbis".to_string(),
        other => other.to_string(),
    }
}

/// Formats a media track for user menus with descriptive labels.
pub fn format_track_label(track: &TrackInfo) -> String {
    let mut parts = Vec::new();
    if let Some(title) = &track.title {
        let t = title.trim();
        if !t.is_empty() {
            parts.push(t.to_string());
        }
    }
    if let Some(lang) = &track.lang {
        let l = lang.trim();
        if !l.is_empty() {
            let lower = l.to_lowercase();
            let lang_clean = match lower.as_str() {
                "eng" | "en" => "English",
                "jpn" | "ja" => "Japanese",
                "spa" | "es" => "Spanish",
                "fra" | "fr" => "French",
                "deu" | "ger" | "de" => "German",
                "ita" | "it" => "Italian",
                "por" | "pt" => "Portuguese",
                "rus" | "ru" => "Russian",
                "zho" | "chi" | "zh" => "Chinese",
                "kor" | "ko" => "Korean",
                _ => l,
            };
            if parts.is_empty() || !parts[0].to_lowercase().contains(&lower) {
                parts.push(lang_clean.to_string());
            }
        }
    }
    if let Some(codec) = &track.codec {
        let c = codec.trim();
        if !c.is_empty() {
            parts.push(format!("[{}]", format_codec_name(c)));
        }
    }

    if parts.is_empty() {
        format!("Track {}", track.id)
    } else {
        format!("Track {}: {}", track.id, parts.join(" · "))
    }
}
