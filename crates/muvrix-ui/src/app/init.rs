use std::collections::VecDeque;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use glow::HasContext as _;
use muvrix_core::browser::MediaBrowser;
use muvrix_core::database::DatabaseManager;
use muvrix_core::history::HistoryStore;
use muvrix_core::network::CredentialVault;
use muvrix_core::thumbnails::ThumbnailCache;
use muvrix_player::equalizer::{AudioDeviceInfo, EqualizerPreset, EqualizerSettings};
use muvrix_player::shader::VideoAdjustmentSettings;
use muvrix_player::{
    DecodePolicy, MpvHandle, MpvRenderContext, MpvStreamBridge, PlaybackController,
};

use crate::app::{LibrarySelection, MuvrixApp, SleepTimer};
use crate::config_editor::ConfigEditorFile;
use crate::gl_loader;
use crate::os_integration::{MediaSession, SleepInhibitor};
use crate::render_engine::RenderEngine;
use crate::views::AppView;
use crate::views::SettingsTab;
use crate::wake::WakeBridge;
use muvrix_core::database::{LibraryFilter, LibrarySortField, SortDirection};
use muvrix_player::shader::Anime4KPreset;

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
        let startup_ctx = egui_ctx.clone();

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
            Ok("player") => AppView::Player,
            Ok("library") => AppView::Library,
            Ok("browser") => AppView::Browser,
            Ok("playlist") => AppView::Playlist,
            Ok("lyrics") => AppView::Lyrics,
            Ok("history" | "recent") => AppView::Recent,
            Ok("settings") => AppView::Settings,
            _ => AppView::Home,
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
            stepping_preset: muvrix_player::SteppingPreset::default(),
            jog_drag_accumulator: 0.0,
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
            active_styling: persisted.subtitle_styling.clone(),
            theme_mode: persisted.theme_mode,
            accent_color: persisted.accent_color,
            audio_channels: persisted.audio_channels,
            volume_normalization: if let Ok(norm) = std::env::var("MUVRIX_VOLUME_NORMALIZATION") {
                norm == "1" || norm.eq_ignore_ascii_case("true")
            } else {
                persisted.volume_normalization
            },
            audio_pitch_correction: persisted.audio_pitch_correction,
            preferred_audio_languages: persisted.preferred_audio_languages.clone(),
            default_audio_delay_ms: persisted.default_audio_delay_ms,
            background_audio: persisted.background_audio,
            verbose_logging: persisted.verbose_logging,
            advanced_status: None,
            ui_overlay_visible: true,
            osd_manually_hidden: false,
            last_mouse_activity: Instant::now(),
            last_mouse_pos: None,
            osd_autohide_delay: Duration::from_secs(3),
            sleep_timer: SleepTimer::Off,
            sleep_deadline: None,
            settings_search: String::new(),
            config_editor_file: ConfigEditorFile::MpvConf,
            config_editor_text: String::new(),
            config_editor_loaded: false,
            config_editor_status: String::new(),
            selected_persistent_playlist: None,
            playlist_name_input: String::new(),
            playlist_import_input: String::new(),
            playlist_status: None,
            media_session: MediaSession::start(),
            sleep_inhibitor: SleepInhibitor::new(),
            sleep_inhibited: false,
            crash_notice: crate::crash::pending_report(),
            library_selection: LibrarySelection::default(),
            library_multi_select: false,
            library_batch_confirm_delete: false,
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
            logo_texture: None,
            active_scan_cancel: None,
            active_scan_rx: None,
            scan_progress: None,
            active_enrichment_cancel: None,
            active_enrichment_rx: None,
            enrichment_progress: None,
            enrichment_pending: false,
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
            equalizer_settings: {
                let mut eq = if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("equalizer")
                    || std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("audio")
                {
                    EqualizerSettings {
                        is_enabled: true,
                        ..Default::default()
                    }
                } else {
                    persisted.equalizer
                };

                if let Ok(preset_str) = std::env::var("MUVRIX_EQ_PRESET") {
                    eq.is_enabled = true;
                    match preset_str.to_lowercase().as_str() {
                        "flat" => eq.apply_preset(EqualizerPreset::Flat),
                        "bass_boost" | "bass" => eq.apply_preset(EqualizerPreset::BassBoost),
                        "bass_reduction" => eq.apply_preset(EqualizerPreset::BassReduction),
                        "treble_boost" | "treble" => eq.apply_preset(EqualizerPreset::TrebleBoost),
                        "treble_reduction" => eq.apply_preset(EqualizerPreset::TrebleReduction),
                        "vocal_clarity" | "vocal" => eq.apply_preset(EqualizerPreset::VocalClarity),
                        "rock" => eq.apply_preset(EqualizerPreset::Rock),
                        "classical" => eq.apply_preset(EqualizerPreset::Classical),
                        "electronic" => eq.apply_preset(EqualizerPreset::Electronic),
                        "cinema" => eq.apply_preset(EqualizerPreset::Cinema),
                        _ => {}
                    }
                }

                if let Ok(gains_str) = std::env::var("MUVRIX_EQ_GAINS") {
                    eq.is_enabled = true;
                    let parts: Vec<f64> = gains_str
                        .split(',')
                        .filter_map(|s| s.trim().parse::<f64>().ok())
                        .collect();
                    for (i, &g) in parts.iter().take(10).enumerate() {
                        eq.set_band_gain(i, g);
                    }
                }

                if let Ok(preamp_str) = std::env::var("MUVRIX_EQ_PREAMP") {
                    if let Ok(p) = preamp_str.parse::<f64>() {
                        eq.preamp_db = p;
                    }
                }

                if let Ok(bs2b_str) = std::env::var("MUVRIX_BS2B") {
                    match bs2b_str.to_lowercase().as_str() {
                        "off" | "0" | "false" => eq.crossfeed_enabled = false,
                        "default" => {
                            eq.crossfeed_enabled = true;
                            eq.crossfeed_preset = muvrix_player::Bs2bPreset::Default;
                        }
                        "cmoy" | "chu_moy" => {
                            eq.crossfeed_enabled = true;
                            eq.crossfeed_preset = muvrix_player::Bs2bPreset::ChuMoy;
                        }
                        "jmeier" | "jan_meier" => {
                            eq.crossfeed_enabled = true;
                            eq.crossfeed_preset = muvrix_player::Bs2bPreset::JanMeier;
                        }
                        _ => {}
                    }
                }

                if let Ok(norm_str) = std::env::var("MUVRIX_VOLUME_NORMALIZATION") {
                    let active = norm_str == "1" || norm_str.eq_ignore_ascii_case("true");
                    eq.dynaudnorm = active;
                }

                eq
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
                Ok("audio" | "equalizer") => SettingsTab::Audio,
                Ok("enhancement" | "video") => SettingsTab::VideoEnhancement,
                Ok("subtitles" | "subs") => SettingsTab::Subtitles,
                Ok("network" | "webdav") => SettingsTab::NetworkMedia,
                Ok("appearance" | "theme") => SettingsTab::Appearance,
                Ok("config") => SettingsTab::ConfigEditor,
                Ok("advanced" | "preferences" | "privacy" | "window") => SettingsTab::Advanced,
                Ok("about") => SettingsTab::About,
                _ => SettingsTab::Audio,
            },
            lyrics: None,
            lyrics_auto_scroll: true,
            active_bookmarks: Vec::new(),
            editing_bookmark_id: None,
            editing_bookmark_label: String::new(),
            editing_bookmark_note: String::new(),
            // Phase C1
            library_context_item: None,
            show_library_context_menu: false,
            // Phase C2
            browser_is_grid: false,
            browser_context_entry: None,
            show_browser_context_menu: false,
            // Phase C3
            playlist_rename_id: None,
            playlist_rename_input: String::new(),
            playlist_confirm_delete: None,
            playlist_confirm_clear: None,
            // Phase C5
            global_search_query: String::new(),
            global_search_active: false,
            global_search_videos: Vec::new(),
            global_search_audio: Vec::new(),
            global_search_playlists: Vec::new(),
            global_search_last_query: String::new(),
            home_recently_added: Vec::new(),
            home_favorites: Vec::new(),
            home_longest: Vec::new(),
            home_largest: Vec::new(),
            home_sections_loaded: false,
            // Phase C Final Fix
            home_carousel_scroll_offset: 0.0,
            show_export_playlist_modal: false,
            export_playlist_path_input: String::new(),
            export_playlist_status: None,
            export_playlist_target_id: None,
        };

        app.refresh_library();
        let _ = app
            .controller
            .set_subtitle_styling(app.active_styling.clone());
        if app.db.count_pending_media_items().unwrap_or(0) > 0 {
            app.start_library_enrichment(&startup_ctx);
        }
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
        if let Ok(ab_str) = std::env::var("MUVRIX_SEED_AB_LOOP") {
            let parts: Vec<&str> = ab_str.split(';').collect();
            if parts.len() == 2 {
                if let (Ok(a), Ok(b)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                    let _ = app.controller.set_ab_loop(a, b);
                }
            } else if parts.len() == 1 {
                if let Ok(a) = parts[0].parse::<f64>() {
                    let _ = app.controller.set_ab_loop_a(a);
                }
            }
        }
        if let Ok(preset_str) = std::env::var("MUVRIX_STEPPING_PRESET") {
            app.stepping_preset = match preset_str.to_lowercase().as_str() {
                "film24" | "24" => muvrix_player::SteppingPreset::Film24,
                "ntsc30" | "30" => muvrix_player::SteppingPreset::Ntsc30,
                "highrate60" | "60" => muvrix_player::SteppingPreset::HighRate60,
                _ => muvrix_player::SteppingPreset::Frame,
            };
        }
        if let Ok(sub_id) = std::env::var("MUVRIX_SELECT_SUBTITLE") {
            if let Ok(id) = sub_id.parse::<i64>() {
                let _ = app.controller.set_subtitle_track(Some(id));
            }
        }
        if let Ok(sec_id) = std::env::var("MUVRIX_SELECT_SECONDARY_SUBTITLE") {
            if let Ok(id) = sec_id.parse::<i64>() {
                let _ = app.controller.set_secondary_sub_track(Some(id));
            }
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

        // Phase C diagnostic, test seeding, and visual QA hooks
        if std::env::var("MUVRIX_SEED_LIBRARY").is_ok() {
            let root = muvrix_core::database::LibraryRoot {
                id: 0,
                display_name: "Demo Media".to_string(),
                root_path: "/media/demo".to_string(),
                source_type: "local".to_string(),
                is_recursive: true,
                is_enabled: true,
                exclusion_patterns: Vec::new(),
                last_scanned_at: 1731670000,
                scan_status: "idle".to_string(),
            };
            let root_id = app.db.insert_root(&root).unwrap_or(1);
            let sample_items = [
                (
                    "Cosmos.Laundromat.2015.4K.mkv",
                    "mkv",
                    muvrix_core::database::MediaType::Video,
                    1_420_000_000,
                    "Cosmos Laundromat (First Cycle)",
                    "H.264",
                    3840,
                    2160,
                    730_000,
                    true,
                ),
                (
                    "Big.Buck.Bunny.1080p.mp4",
                    "mp4",
                    muvrix_core::database::MediaType::Video,
                    650_000_000,
                    "Big Buck Bunny",
                    "H.264",
                    1920,
                    1080,
                    596_000,
                    true,
                ),
                (
                    "Sintel.2010.1080p.mkv",
                    "mkv",
                    muvrix_core::database::MediaType::Video,
                    820_000_000,
                    "Sintel",
                    "HEVC",
                    1920,
                    1080,
                    888_000,
                    false,
                ),
                (
                    "Tears.of.Steel.4K.mkv",
                    "mkv",
                    muvrix_core::database::MediaType::Video,
                    2_150_000_000,
                    "Tears of Steel",
                    "VP9",
                    3840,
                    2160,
                    734_000,
                    false,
                ),
                (
                    "Elephants.Dream.720p.avi",
                    "avi",
                    muvrix_core::database::MediaType::Video,
                    320_000_000,
                    "Elephants Dream",
                    "MPEG-4",
                    1280,
                    720,
                    653_000,
                    false,
                ),
                (
                    "Solar.Wind.Symphony.flac",
                    "flac",
                    muvrix_core::database::MediaType::Audio,
                    45_000_000,
                    "Solar Wind Symphony",
                    "FLAC",
                    0,
                    0,
                    312_000,
                    true,
                ),
                (
                    "Cinematic.Soundtrack.mp3",
                    "mp3",
                    muvrix_core::database::MediaType::Audio,
                    12_500_000,
                    "Cinematic Ambient Theme",
                    "MP3",
                    0,
                    0,
                    245_000,
                    false,
                ),
            ];
            for (fname, ext, mtype, fsize, title, codec, w, h, dur, fav) in sample_items {
                let item = muvrix_core::database::MediaItem {
                    id: 0,
                    root_id,
                    relative_path: fname.to_string(),
                    canonical_path: format!("/media/demo/{fname}"),
                    file_name: fname.to_string(),
                    file_extension: ext.to_string(),
                    media_type: mtype,
                    file_size: fsize,
                    modified_time: 1731670000,
                    added_time: 1731670000,
                    last_scanned_at: 1731670000,
                    availability_state: muvrix_core::database::AvailabilityState::Available,
                    metadata_status: muvrix_core::database::MetadataStatus::Extracted,
                };
                if let Ok(id) = app.db.upsert_media_item(&item) {
                    let meta = muvrix_core::database::MediaMetadata {
                        media_id: id,
                        title: Some(title.to_string()),
                        artist: if mtype == muvrix_core::database::MediaType::Audio {
                            Some("Muvrix Soundworks".to_string())
                        } else {
                            None
                        },
                        album: None,
                        genre: Some("Cinema".to_string()),
                        release_year: Some(2024),
                        duration_ms: dur,
                        width: w,
                        height: h,
                        video_codec: if mtype == muvrix_core::database::MediaType::Video {
                            Some(codec.to_string())
                        } else {
                            None
                        },
                        audio_codec: Some("AAC".to_string()),
                        audio_channels: 2,
                        sample_rate: 48000,
                        overall_bitrate: 8_000_000,
                        ..Default::default()
                    };
                    let _ = app.db.upsert_metadata(&meta);
                    if fav {
                        let _ = app.db.set_favourite(id, true);
                    }
                }
            }
            app.refresh_library();
            app.load_home_sections(true);
        }

        if let Ok(lib_mode) = std::env::var("MUVRIX_LIBRARY_VIEW") {
            app.library_is_grid = lib_mode != "list";
        }
        if let Ok(search) = std::env::var("MUVRIX_LIBRARY_SEARCH") {
            app.library_filter.search_query = Some(search);
            app.refresh_library();
        }
        if let Ok(filter) = std::env::var("MUVRIX_LIBRARY_FILTER") {
            match filter.as_str() {
                "video" => {
                    app.library_filter.media_type = Some(muvrix_core::database::MediaType::Video);
                }
                "audio" => {
                    app.library_filter.media_type = Some(muvrix_core::database::MediaType::Audio);
                }
                "fav" => {
                    app.library_filter.favourites_only = true;
                }
                "4k" => {
                    app.library_filter.resolution_tier = Some("4K".to_string());
                }
                "1080p" => {
                    app.library_filter.resolution_tier = Some("1080p".to_string());
                }
                _ => {}
            }
            app.refresh_library();
        }
        if let Ok(q) = std::env::var("MUVRIX_GLOBAL_SEARCH") {
            app.settings_search = q.clone();
            app.global_search_query = q.clone();
            app.global_search_active = true;
            app.run_global_search(&q);
        }
        if let Ok(grid) = std::env::var("MUVRIX_BROWSER_GRID") {
            app.browser_is_grid = grid == "1" || grid == "true";
        }
        if let Ok(dir) = std::env::var("MUVRIX_BROWSER_DIR") {
            app.media_browser.navigate_to(std::path::PathBuf::from(dir));
        }
        if std::env::var("MUVRIX_OPEN_DETAILS_DRAWER").is_ok() {
            if let Some(item) = app.library_items.first().cloned() {
                app.selected_library_item = Some(item);
            }
        }
        if let Ok(err) = std::env::var("MUVRIX_SHOW_ERROR") {
            app.status_message = format!("Error: {err}");
            app.media_browser.error = Some(err);
        }
        if std::env::var("MUVRIX_SIMULATE_SCAN").is_ok() {
            app.scan_progress = Some(muvrix_core::scanner::ScanProgress {
                root_id: 1,
                discovered_count: 42,
                indexed_count: 38,
                skipped_count: 3,
                missing_count: 1,
                error_count: 0,
                current_path: Some("/media/movies/Inception.2010.4K.mkv".to_string()),
                is_finished: false,
                is_cancelled: false,
            });
        }
        if let Ok(export_dest) = std::env::var("MUVRIX_EXPORT_PLAYLIST_PATH") {
            let entries = app.controller.playlist();
            let m3u_entries: Vec<muvrix_core::playlist::M3uEntry> = entries
                .iter()
                .map(|e| muvrix_core::playlist::M3uEntry {
                    path: e.filename.clone(),
                    title: e.title.clone(),
                    duration_ms: 0,
                    logo: None,
                    group: None,
                })
                .collect();
            if let Err(e) = muvrix_core::playlist::export_m3u(&m3u_entries, &export_dest) {
                log::error!("Automated MUVRIX_EXPORT_PLAYLIST_PATH failed: {e}");
            } else {
                log::info!(
                    "Automated MUVRIX_EXPORT_PLAYLIST_PATH successfully wrote to {export_dest}"
                );
                app.export_playlist_status = Some(format!(
                    "Exported {} entries to {}",
                    m3u_entries.len(),
                    export_dest
                ));
            }
        }
        if std::env::var("MUVRIX_OPEN_EXPORT_MODAL").is_ok() {
            app.show_export_playlist_modal = true;
            app.export_playlist_path_input = "exported_queue.m3u8".to_string();
            app.export_playlist_status = Some("Ready to export".to_string());
        }
        if let Ok(offset_str) = std::env::var("MUVRIX_CAROUSEL_OFFSET") {
            if let Ok(offset) = offset_str.parse::<f32>() {
                app.home_carousel_scroll_offset = offset;
            }
        }

        Ok(app)
    }
}
