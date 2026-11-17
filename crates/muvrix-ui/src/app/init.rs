use std::collections::VecDeque;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use glow::HasContext as _;
use muvrix_core::browser::MediaBrowser;
use muvrix_core::database::{DatabaseManager, NetworkConnection};
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
            show_diagnostics: std::env::var("MUVRIX_SHOW_DIAGNOSTICS").is_ok(),
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
            playlist_drag_source_idx: None,
            playlist_drag_target_idx: None,
            playlist_use_smart_shuffle: true,
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
            thumbnail_textures: crate::thumbnail_cache::LruTextureCache::default(),
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
            custom_shader_path_input: String::new(),
            custom_shader_error: None,
            tone_mapping_hover_nits: None,
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

                if let Ok(ebu_str) = std::env::var("MUVRIX_EBU_R128") {
                    let active = ebu_str == "1" || ebu_str.eq_ignore_ascii_case("true");
                    if active {
                        eq.set_normalization_mode(muvrix_player::NormalizationMode::EbuR128);
                    }
                }

                if let Ok(i_str) = std::env::var("MUVRIX_EBU_R128_I") {
                    if let Ok(val) = i_str.parse::<f64>() {
                        eq.ebu_r128.integrated_loudness = val;
                    }
                }

                if let Ok(tp_str) = std::env::var("MUVRIX_EBU_R128_TP") {
                    if let Ok(val) = tp_str.parse::<f64>() {
                        eq.ebu_r128.true_peak = val;
                    }
                }

                if let Ok(lra_str) = std::env::var("MUVRIX_EBU_R128_LRA") {
                    if let Ok(val) = lra_str.parse::<f64>() {
                        eq.ebu_r128.loudness_range = val;
                    }
                }

                if let Ok(autoeq_path) = std::env::var("MUVRIX_AUTOEQ_PROFILE") {
                    if let Ok(content) = std::fs::read_to_string(&autoeq_path) {
                        let name = std::path::Path::new(&autoeq_path)
                            .file_stem()
                            .and_then(|s| s.to_str());
                        if let Ok(profile) = muvrix_player::AutoEqProfile::parse(&content, name) {
                            eq.apply_autoeq_profile(profile);
                        }
                    }
                }

                if let Ok(vis_str) = std::env::var("MUVRIX_SPECTRUM_VISUALIZER") {
                    let active = vis_str == "1" || vis_str.eq_ignore_ascii_case("true");
                    eq.spectrum_visualizer_enabled = active;
                } else {
                    eq.spectrum_visualizer_enabled = true;
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
            connection_pool: Arc::new(muvrix_core::network::ConnectionPool::new()),
            add_network_open: std::env::var("MUVRIX_OPEN_NETWORK_FORM").is_ok(),
            network_name_input: if let Ok(name) = std::env::var("MUVRIX_NETWORK_NAME") {
                name
            } else if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network") {
                "Demo WebDAV Share (Example)".to_string()
            } else {
                String::new()
            },
            network_host_input: if let Ok(host) = std::env::var("MUVRIX_NETWORK_HOST") {
                host
            } else if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network") {
                "dav.example.com".to_string()
            } else {
                String::new()
            },
            network_port_input: if let Ok(port_str) = std::env::var("MUVRIX_NETWORK_PORT") {
                port_str.parse().unwrap_or(8443)
            } else {
                8443
            },
            network_path_input: if let Ok(path) = std::env::var("MUVRIX_NETWORK_PATH") {
                path
            } else {
                "/remote.php/webdav/Media/".to_string()
            },
            network_user_input: if let Ok(user) = std::env::var("MUVRIX_NETWORK_USER") {
                user
            } else if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network") {
                "media_streamer".to_string()
            } else {
                String::new()
            },
            network_pass_input: if let Ok(pass) = std::env::var("MUVRIX_NETWORK_PASS") {
                pass
            } else if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network") {
                "VaultSuperSecretPassword123!".to_string()
            } else {
                String::new()
            },
            network_https_input: std::env::var("MUVRIX_NETWORK_HTTPS").as_deref() != Ok("0"),
            network_protocol_input: if let Ok(proto) = std::env::var("MUVRIX_NETWORK_PROTOCOL") {
                proto
            } else {
                "webdav".to_string()
            },
            network_nfs_version: if std::env::var("MUVRIX_NFS_VERSION").as_deref() == Ok("v3") {
                muvrix_core::network::NfsVersion::V3
            } else {
                muvrix_core::network::NfsVersion::V4
            },
            network_status_msg: if let Ok(msg) = std::env::var("MUVRIX_NETWORK_STATUS") {
                Some(msg)
            } else if std::env::var("MUVRIX_SETTINGS_TAB").as_deref() == Ok("network") {
                Some(
                    "Credentials are held in application memory for this session and are not persisted by Muvrix."
                        .to_string(),
                )
            } else {
                None
            },
            network_host_key_input: String::new(),
            stream_buffer_mb: persisted.stream_buffer_mb,
            stream_back_buffer_mb: persisted.stream_back_buffer_mb,
            stream_readahead_secs: persisted.stream_readahead_secs,
            stream_max_retries: persisted.stream_max_retries,
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
            export_playlist_format: crate::app::PlaylistExportFormat::default(),

            // Phase E3: Library Intelligence & Smart Collections
            selected_smart_collection: None,
            smart_collections: Vec::new(),
            smart_collection_dialog_open: std::env::var("MUVRIX_OPEN_SMART_COLLECTION_BUILDER")
                .is_ok(),
            editing_smart_collection: None,
            smart_collection_name_input: if std::env::var("MUVRIX_OPEN_SMART_COLLECTION_BUILDER")
                .is_ok()
            {
                "4K Sci-Fi Masterpieces".to_string()
            } else {
                String::new()
            },
            smart_collection_desc_input: if std::env::var("MUVRIX_OPEN_SMART_COLLECTION_BUILDER")
                .is_ok()
            {
                "High bitrate 4K releases matching genre Sci-Fi".to_string()
            } else {
                String::new()
            },
            smart_collection_icon_input: "🚀".to_string(),
            smart_collection_rules: if std::env::var("MUVRIX_OPEN_SMART_COLLECTION_BUILDER").is_ok()
            {
                vec![
                    muvrix_core::database::SmartRule::Text {
                        field: muvrix_core::database::RuleField::Genre,
                        operator: muvrix_core::database::TextOperator::Contains,
                        value: "Sci-Fi".to_string(),
                    },
                    muvrix_core::database::SmartRule::Numeric {
                        field: muvrix_core::database::RuleField::ReleaseYear,
                        operator: muvrix_core::database::NumericOperator::GreaterThan,
                        value: 2020.0,
                        value_to: None,
                    },
                ]
            } else {
                Vec::new()
            },
            smart_collection_combination_mode: muvrix_core::database::RuleCombinationMode::And,
            smart_collection_preview_count: if std::env::var("MUVRIX_OPEN_SMART_COLLECTION_BUILDER")
                .is_ok()
            {
                Some(14)
            } else {
                None
            },
            smart_collection_error: None,

            // Phase E3: Batch Metadata Editor
            batch_metadata_dialog_open: std::env::var("MUVRIX_OPEN_BATCH_EDITOR").is_ok(),
            batch_metadata_summary: if std::env::var("MUVRIX_OPEN_BATCH_EDITOR").is_ok() {
                Some(muvrix_core::database::SelectedMetadataSummary {
                    count: 3,
                    title: muvrix_core::database::FieldMixedState::Mixed,
                    artist: muvrix_core::database::FieldMixedState::Uniform(Some(
                        "Christopher Nolan".to_string(),
                    )),
                    album: muvrix_core::database::FieldMixedState::Mixed,
                    album_artist: muvrix_core::database::FieldMixedState::Uniform(Some(
                        "Syncopy Films".to_string(),
                    )),
                    genre: muvrix_core::database::FieldMixedState::Uniform(Some(
                        "Sci-Fi / Thriller".to_string(),
                    )),
                    release_year: muvrix_core::database::FieldMixedState::Mixed,
                    track_number: muvrix_core::database::FieldMixedState::Mixed,
                    disc_number: muvrix_core::database::FieldMixedState::Uniform(Some(1)),
                })
            } else {
                None
            },
            batch_metadata_request: muvrix_core::database::BatchMetadataRequest::default(),
            batch_edit_title_action: 0,
            batch_title_input: String::new(),
            batch_edit_artist_action: 0,
            batch_artist_input: "Christopher Nolan".to_string(),
            batch_edit_album_action: 0,
            batch_album_input: String::new(),
            batch_edit_album_artist_action: 0,
            batch_album_artist_input: "Syncopy Films".to_string(),
            batch_edit_genre_action: 0,
            batch_genre_input: "Sci-Fi / Thriller".to_string(),
            batch_edit_year_action: 0,
            batch_year_input: String::new(),
            batch_edit_track_action: 0,
            batch_track_input: String::new(),
            batch_edit_disc_action: 0,
            batch_disc_input: "1".to_string(),
            batch_edit_status: None,

            // Phase E3: Integrity Scanner & Relinking
            integrity_dialog_open: std::env::var("MUVRIX_OPEN_INTEGRITY_LOCATOR").is_ok()
                || std::env::var("MUVRIX_OPEN_CATALOG_CLEANUP").is_ok(),
            integrity_scan_progress: if std::env::var("MUVRIX_OPEN_INTEGRITY_LOCATOR").is_ok()
                || std::env::var("MUVRIX_OPEN_CATALOG_CLEANUP").is_ok()
            {
                Some(muvrix_core::database::IntegrityScanProgress {
                    total_examined: 142,
                    total_items: 142,
                    available_count: 138,
                    missing_count: 3,
                    inaccessible_count: 0,
                    permission_denied_count: 0,
                    network_unavailable_count: 1,
                    is_completed: true,
                    last_scan_timestamp: 1700000000,
                })
            } else {
                None
            },
            integrity_scan_cancel: None,
            missing_media_items: if std::env::var("MUVRIX_OPEN_INTEGRITY_LOCATOR").is_ok()
                || std::env::var("MUVRIX_OPEN_CATALOG_CLEANUP").is_ok()
            {
                vec![
                    muvrix_core::database::LibraryItemView {
                        item: muvrix_core::database::MediaItem {
                            id: 101,
                            root_id: 1,
                            relative_path: "Interstellar.mp4".to_string(),
                            canonical_path: "/mnt/storage/movies/Interstellar.mp4".to_string(),
                            file_name: "Interstellar.mp4".to_string(),
                            file_extension: "mp4".to_string(),
                            media_type: muvrix_core::database::MediaType::Video,
                            file_size: 12_450_892_100,
                            modified_time: 1700000000,
                            added_time: 1700000000,
                            last_scanned_at: 1700000000,
                            availability_state:
                                muvrix_core::database::AvailabilityState::Unavailable,
                            metadata_status: muvrix_core::database::MetadataStatus::Extracted,
                        },
                        metadata: Some(muvrix_core::database::MediaMetadata {
                            media_id: 101,
                            title: Some("Interstellar (2014)".to_string()),
                            artist: Some("Christopher Nolan".to_string()),
                            genre: Some("Sci-Fi".to_string()),
                            release_year: Some(2014),
                            duration_ms: 169 * 60 * 1000,
                            overall_bitrate: 15_000_000,
                            ..Default::default()
                        }),
                        is_favourite: true,
                        is_watched: true,
                        thumbnail_path: None,
                        tags: Vec::new(),
                    },
                    muvrix_core::database::LibraryItemView {
                        item: muvrix_core::database::MediaItem {
                            id: 104,
                            root_id: 1,
                            relative_path: "Dune.Part.Two.mkv".to_string(),
                            canonical_path: "/mnt/storage/movies/Dune.Part.Two.mkv".to_string(),
                            file_name: "Dune.Part.Two.mkv".to_string(),
                            file_extension: "mkv".to_string(),
                            media_type: muvrix_core::database::MediaType::Video,
                            file_size: 18_120_400_000,
                            modified_time: 1700000000,
                            added_time: 1700000000,
                            last_scanned_at: 1700000000,
                            availability_state:
                                muvrix_core::database::AvailabilityState::Unavailable,
                            metadata_status: muvrix_core::database::MetadataStatus::Extracted,
                        },
                        metadata: Some(muvrix_core::database::MediaMetadata {
                            media_id: 104,
                            title: Some("Dune: Part Two (2024)".to_string()),
                            artist: Some("Denis Villeneuve".to_string()),
                            genre: Some("Sci-Fi / Adventure".to_string()),
                            release_year: Some(2024),
                            duration_ms: 166 * 60 * 1000,
                            overall_bitrate: 22_000_000,
                            ..Default::default()
                        }),
                        is_favourite: false,
                        is_watched: false,
                        thumbnail_path: None,
                        tags: Vec::new(),
                    },
                    muvrix_core::database::LibraryItemView {
                        item: muvrix_core::database::MediaItem {
                            id: 112,
                            root_id: 1,
                            relative_path: "Blade.Runner.2049.mkv".to_string(),
                            canonical_path: "/mnt/storage/movies/Blade.Runner.2049.mkv".to_string(),
                            file_name: "Blade.Runner.2049.mkv".to_string(),
                            file_extension: "mkv".to_string(),
                            media_type: muvrix_core::database::MediaType::Video,
                            file_size: 14_800_000_000,
                            modified_time: 1700000000,
                            added_time: 1700000000,
                            last_scanned_at: 1700000000,
                            availability_state:
                                muvrix_core::database::AvailabilityState::Unavailable,
                            metadata_status: muvrix_core::database::MetadataStatus::Extracted,
                        },
                        metadata: Some(muvrix_core::database::MediaMetadata {
                            media_id: 112,
                            title: Some("Blade Runner 2049".to_string()),
                            artist: Some("Denis Villeneuve".to_string()),
                            genre: Some("Cyberpunk / Sci-Fi".to_string()),
                            release_year: Some(2017),
                            duration_ms: 164 * 60 * 1000,
                            overall_bitrate: 18_000_000,
                            ..Default::default()
                        }),
                        is_favourite: true,
                        is_watched: true,
                        thumbnail_path: None,
                        tags: Vec::new(),
                    },
                ]
            } else {
                Vec::new()
            },
            selected_missing_item: if std::env::var("MUVRIX_OPEN_INTEGRITY_LOCATOR").is_ok() {
                Some(muvrix_core::database::LibraryItemView {
                    item: muvrix_core::database::MediaItem {
                        id: 101,
                        root_id: 1,
                        relative_path: "Interstellar.mp4".to_string(),
                        canonical_path: "/mnt/storage/movies/Interstellar.mp4".to_string(),
                        file_name: "Interstellar.mp4".to_string(),
                        file_extension: "mp4".to_string(),
                        media_type: muvrix_core::database::MediaType::Video,
                        file_size: 12_450_892_100,
                        modified_time: 1700000000,
                        added_time: 1700000000,
                        last_scanned_at: 1700000000,
                        availability_state: muvrix_core::database::AvailabilityState::Unavailable,
                        metadata_status: muvrix_core::database::MetadataStatus::Extracted,
                    },
                    metadata: Some(muvrix_core::database::MediaMetadata {
                        media_id: 101,
                        title: Some("Interstellar (2014)".to_string()),
                        artist: Some("Christopher Nolan".to_string()),
                        genre: Some("Sci-Fi".to_string()),
                        release_year: Some(2014),
                        duration_ms: 169 * 60 * 1000,
                        overall_bitrate: 15_000_000,
                        ..Default::default()
                    }),
                    is_favourite: true,
                    is_watched: true,
                    thumbnail_path: None,
                    tags: Vec::new(),
                })
            } else {
                None
            },
            relink_candidates: if std::env::var("MUVRIX_OPEN_INTEGRITY_LOCATOR").is_ok() {
                vec![
                    muvrix_core::database::RelinkCandidate {
                        path: std::path::PathBuf::from(
                            "/mnt/storage/archive/Interstellar (2014) Remaster.mp4",
                        ),
                        file_name: "Interstellar (2014) Remaster.mp4".to_string(),
                        file_size: 12_450_892_100,
                        confidence: 0.96,
                        match_reasons: vec![
                            "Exact file size match (12.45 GB)".to_string(),
                            "Filename contains title 'Interstellar'".to_string(),
                            "Matching video container .mp4".to_string(),
                        ],
                    },
                    muvrix_core::database::RelinkCandidate {
                        path: std::path::PathBuf::from("/mnt/backup/movies/Interstellar.mp4"),
                        file_name: "Interstellar.mp4".to_string(),
                        file_size: 12_450_892_100,
                        confidence: 0.99,
                        match_reasons: vec![
                            "Exact filename match".to_string(),
                            "Exact file size match (12.45 GB)".to_string(),
                        ],
                    },
                ]
            } else {
                Vec::new()
            },
            relink_search_dir_input: "/mnt/storage/archive".to_string(),
            cleanup_preview: if std::env::var("MUVRIX_OPEN_CATALOG_CLEANUP").is_ok() {
                Some(muvrix_core::database::CleanupPreview {
                    confirmed_missing_ids: vec![101, 104, 112],
                    duplicate_ids: vec![205, 206],
                    suspected_duplicates: Vec::new(),
                    orphaned_metadata_ids: vec![301],
                })
            } else {
                None
            },
            cleanup_confirm_open: false,
            integrity_status_msg: None,

            // Phase E4: Adaptive Player HUD, Three-Tier Collapsible Decks & Overflow
            player_secondary_deck_expanded: std::env::var("MUVRIX_OPEN_PLAYER_DECK").is_ok(),
            player_secondary_deck_collapsed_wide: false,
            player_more_overflow_open: std::env::var("MUVRIX_OPEN_PLAYER_MORE").is_ok(),
            player_empty_url_input_open: std::env::var("MUVRIX_OPEN_PLAYER_URL_INPUT").is_ok(),
            seeded_ab_loop_applied_file: None,

            // Phase E5: Audio DSP Expansion, EBU R128 & 32-Band Spectrum Visualizer
            spectrum_visualizer: muvrix_player::SpectrumVisualizer::new(),
            autoeq_status_msg: std::env::var("MUVRIX_AUTOEQ_STATUS")
                .ok()
                .map(|s| (s, false)),
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
        let _ = app.controller.configure_stream_buffering(
            (persisted.stream_buffer_mb as u64) * 1024 * 1024,
            (persisted.stream_back_buffer_mb as u64) * 1024 * 1024,
            persisted.stream_readahead_secs as f64,
        );

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
        } else if app.controller.playlist_count() == 0
            && std::env::var("MUVRIX_NO_RESUME_QUEUE").is_err()
        {
            if let Ok(active_entries) = app.db.load_active_queue() {
                if !active_entries.is_empty() {
                    let mut resume_cursor = 0;
                    for (i, entry) in active_entries.iter().enumerate() {
                        let _ = app.controller.playlist_append(&entry.path);
                        if entry.is_current {
                            resume_cursor = i;
                        }
                    }
                    if resume_cursor > 0 {
                        let _ = app.controller.playlist_play_index(resume_cursor);
                    }
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
            let parts: Vec<&str> = ab_str.split(&[',', ';'][..]).collect();
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
        if std::env::var("MUVRIX_SEED_SMART_COLLECTIONS").is_ok() {
            let sc1 = muvrix_core::database::SmartCollection {
                id: 0,
                name: "4K Sci-Fi".to_string(),
                description: "4K resolution sci-fi films".to_string(),
                icon: Some("🚀".to_string()),
                rules: muvrix_core::database::RuleDefinition {
                    mode: muvrix_core::database::RuleCombinationMode::And,
                    rules: vec![muvrix_core::database::SmartRule::Text {
                        field: muvrix_core::database::RuleField::Genre,
                        operator: muvrix_core::database::TextOperator::Contains,
                        value: "Sci-Fi".to_string(),
                    }],
                },
                combination_mode: muvrix_core::database::RuleCombinationMode::And,
                sort_field: Some("title".to_string()),
                sort_direction: Some("asc".to_string()),
                display_order: 1,
                created_at: 1000,
                updated_at: 1000,
            };
            let sc2 = muvrix_core::database::SmartCollection {
                id: 0,
                name: "Lossless Audio".to_string(),
                description: "FLAC audio tracks".to_string(),
                icon: Some("🎵".to_string()),
                rules: muvrix_core::database::RuleDefinition {
                    mode: muvrix_core::database::RuleCombinationMode::And,
                    rules: vec![muvrix_core::database::SmartRule::Text {
                        field: muvrix_core::database::RuleField::Format,
                        operator: muvrix_core::database::TextOperator::Equals,
                        value: "flac".to_string(),
                    }],
                },
                combination_mode: muvrix_core::database::RuleCombinationMode::And,
                sort_field: Some("title".to_string()),
                sort_direction: Some("asc".to_string()),
                display_order: 2,
                created_at: 1000,
                updated_at: 1000,
            };
            let _ = app.db.create_smart_collection(&sc1);
            let _ = app.db.create_smart_collection(&sc2);
            app.refresh_library();
        }
        if std::env::var("MUVRIX_SELECT_MULTI_BATCH").is_ok()
            || std::env::var("MUVRIX_OPEN_BATCH_EDITOR").is_ok()
        {
            app.library_multi_select = true;
            let ids: Vec<i64> = app
                .library_items
                .iter()
                .map(|item| item.item.id)
                .take(3)
                .collect();
            app.library_selection.select_all(ids);
            if std::env::var("MUVRIX_OPEN_BATCH_EDITOR").is_ok() {
                app.open_batch_metadata_editor();
                if let Ok(val) = std::env::var("MUVRIX_BATCH_SET_GENRE") {
                    app.batch_edit_genre_action = 1;
                    app.batch_genre_input = val;
                }
            }
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
        if let Ok(src_str) = std::env::var("MUVRIX_PLAYLIST_DRAG_SOURCE") {
            if let Ok(src) = src_str.parse::<usize>() {
                app.playlist_drag_source_idx = Some(src);
            }
        }
        if let Ok(tgt_str) = std::env::var("MUVRIX_PLAYLIST_DRAG_TARGET") {
            if let Ok(tgt) = tgt_str.parse::<usize>() {
                app.playlist_drag_target_idx = Some(tgt);
            }
        }
        if std::env::var("MUVRIX_SMART_SHUFFLE").is_ok() {
            app.playlist_use_smart_shuffle = true;
            app.playlist_status =
                Some("Smart shuffled queue with artist & album separation".to_string());
        }
        if let Ok(offset_str) = std::env::var("MUVRIX_CAROUSEL_OFFSET") {
            if let Ok(offset) = offset_str.parse::<f32>() {
                app.home_carousel_scroll_offset = offset;
            }
        }

        if let Ok(recon_str) = std::env::var("MUVRIX_SIMULATE_STREAM_RECONNECT") {
            let telemetry = muvrix_player::StreamTelemetry::new();
            let attempt = recon_str.parse::<u32>().unwrap_or(2);
            *telemetry.state.write().unwrap() = muvrix_player::RecoveryState::Reconnecting {
                attempt,
                max_attempts: 5,
            };
            telemetry
                .reconnect_count
                .store(attempt as u64, std::sync::atomic::Ordering::Relaxed);
            telemetry
                .total_bytes_read
                .store(14_250_880, std::sync::atomic::Ordering::Relaxed);
            app.controller.set_stream_telemetry(Some(telemetry));
        } else if std::env::var("MUVRIX_SIMULATE_STREAM_STREAMING").is_ok() {
            let telemetry = muvrix_player::StreamTelemetry::new();
            *telemetry.state.write().unwrap() = muvrix_player::RecoveryState::Connected;
            telemetry
                .total_bytes_read
                .store(48_920_412, std::sync::atomic::Ordering::Relaxed);
            app.controller.set_stream_telemetry(Some(telemetry));
        }

        // Phase E1 resilience simulation hooks for visual QA
        if let Ok(status) = std::env::var("MUVRIX_SIMULATE_AUDIO_HOTPLUG") {
            match status.as_str() {
                "lost" => {
                    let _ = app
                        .controller
                        .set_audio_device("USB DAC Studio Monitor Pro");
                    let _ = app
                        .controller
                        .audio()
                        .on_device_list_changed(app.controller.handle());
                    app.status_message =
                        "Audio device 'USB DAC Studio Monitor Pro' disconnected; rerouted to system default"
                            .to_string();
                }
                "recovered" => {
                    app.status_message =
                        "Preferred audio device 'USB DAC Studio Monitor Pro' reconnected; restored route"
                            .to_string();
                }
                _ => {}
            }
        }
        if let Ok(mode) = std::env::var("MUVRIX_SIMULATE_WATCHDOG_STATUS") {
            match mode.as_str() {
                "fallback" => {
                    let reason = muvrix_player::DecoderFallbackReason::StallDetected {
                        stall_duration_secs: 3.2,
                        dropped_frames: 12,
                    };
                    let _ = app
                        .controller
                        .video()
                        .trigger_decoder_fallback(app.controller.handle(), reason);
                }
                "exhausted" => {
                    let _ = app.controller.video().trigger_decoder_fallback(
                        app.controller.handle(),
                        muvrix_player::DecoderFallbackReason::PersistentErrors { error_count: 5 },
                    );
                    let _ = app.controller.video().trigger_decoder_fallback(
                        app.controller.handle(),
                        muvrix_player::DecoderFallbackReason::PersistentErrors { error_count: 5 },
                    );
                }
                _ => {}
            }
        }
        if let Ok(lyrics_mode) = std::env::var("MUVRIX_SEED_LYRICS") {
            match lyrics_mode.as_str() {
                "synced" => {
                    let synced_content = r#"
[ti:Quantum Echoes]
[ar:Muvrix Soundlab]
[al:Resilience & Purity]
[00:00.00]Synthesizing harmonious soundscapes
[00:04.50]Crystal clear frequencies across the spectrum
[00:09.00]Zero latency timeline synchronization
[00:14.20]Harmonics align in pristine acoustic balance
"#;
                    if let Ok(lrc) = muvrix_core::lyrics::LrcFile::parse(synced_content) {
                        app.lyrics = Some(lrc);
                    }
                }
                "embedded" => {
                    let embedded_content = r#"
[ti:Midnight Drive]
[ar:Solar Pulse]
[al:Embedded Tape]
Verse 1:
Rolling down the neon highway with nowhere to be.
Subwoofers humming a deep low-frequency pulse.

Chorus:
Pure embedded typography without timestamps.
Authentic acoustic performance preserved.
Japanese translation: 星空のワルツが流れる
"#;
                    if let Ok(lrc) = muvrix_core::lyrics::LrcFile::from_unsynced_text(
                        embedded_content,
                        muvrix_core::lyrics::LyricSource::EmbeddedUnsynced,
                    ) {
                        app.lyrics = Some(lrc);
                    }
                }
                path if std::path::Path::new(path).is_file() => {
                    if let Ok(lrc) = muvrix_core::lyrics::LrcFile::from_file_auto(path) {
                        app.lyrics = Some(lrc);
                    }
                }
                _ => {}
            }
        }

        // Phase E6 Network protocol and media server seeding hooks for visual QA
        if std::env::var("MUVRIX_SEED_E6_NETWORK").is_ok() {
            let nfs_conn = NetworkConnection {
                id: 0,
                name: "TrueNAS NFS Share (4K Cinema)".to_string(),
                protocol: "nfs".to_string(),
                host: "nas.home.local".to_string(),
                port: 2049,
                remote_path: "/volume1/video".to_string(),
                auth_type: "anonymous".to_string(),
                credential_ref: None,
                is_anonymous: true,
                auto_connect: false,
                last_connected_at: 1700000000,
            };
            let jf_conn = NetworkConnection {
                id: 0,
                name: "Jellyfin Home Server".to_string(),
                protocol: "jellyfin".to_string(),
                host: "jellyfin.local".to_string(),
                port: 8096,
                remote_path: "/".to_string(),
                auth_type: "token".to_string(),
                credential_ref: Some("muvrix:jellyfin:jellyfin.local:8096:user".to_string()),
                is_anonymous: false,
                auto_connect: false,
                last_connected_at: 1700001000,
            };
            let plex_conn = NetworkConnection {
                id: 0,
                name: "Plex Media Server (Remote)".to_string(),
                protocol: "plex".to_string(),
                host: "plex.cloud.tv".to_string(),
                port: 32400,
                remote_path: "/library/sections/1".to_string(),
                auth_type: "token".to_string(),
                credential_ref: Some("muvrix:plex:plex.cloud.tv:32400:owner".to_string()),
                is_anonymous: false,
                auto_connect: false,
                last_connected_at: 1700002000,
            };

            // Deduplication guard: only insert if connection does not already exist
            let existing_conns = app.db.list_connections().unwrap_or_default();
            if !existing_conns
                .iter()
                .any(|c| c.protocol.eq_ignore_ascii_case("nfs") && c.host == nfs_conn.host)
            {
                let _ = app.db.insert_connection(&nfs_conn);
            }
            if !existing_conns
                .iter()
                .any(|c| c.protocol.eq_ignore_ascii_case("jellyfin") && c.host == jf_conn.host)
            {
                let _ = app.db.insert_connection(&jf_conn);
            }
            if !existing_conns
                .iter()
                .any(|c| c.protocol.eq_ignore_ascii_case("plex") && c.host == plex_conn.host)
            {
                let _ = app.db.insert_connection(&plex_conn);
            }

            if let Ok(browse_mode) = std::env::var("MUVRIX_SEED_REMOTE_BROWSER") {
                let current_conns = app.db.list_connections().unwrap_or_default();
                match browse_mode.as_str() {
                    "jellyfin" => {
                        let target_id = current_conns
                            .iter()
                            .find(|c| c.protocol.eq_ignore_ascii_case("jellyfin"))
                            .map(|c| c.id);
                        app.browsing_conn_id = target_id;
                        app.browsing_path = "/items/jf_movies".to_string();
                        app.browsing_entries = vec![
                            muvrix_core::network::RemoteFileEntry {
                                name: "Big Buck Bunny (2008).mp4".to_string(),
                                remote_path: "/items/101".to_string(),
                                is_dir: false,
                                size_bytes: 158293740,
                                modified_timestamp: 1700000000,
                            },
                            muvrix_core::network::RemoteFileEntry {
                                name: "Sita Sings the Blues (2008).mkv".to_string(),
                                remote_path: "/items/102".to_string(),
                                is_dir: false,
                                size_bytes: 850400000,
                                modified_timestamp: 1700000000,
                            },
                            muvrix_core::network::RemoteFileEntry {
                                name: "Animation Series".to_string(),
                                remote_path: "/items/103".to_string(),
                                is_dir: true,
                                size_bytes: 0,
                                modified_timestamp: 1700000000,
                            },
                        ];
                    }
                    "plex" => {
                        let target_id = current_conns
                            .iter()
                            .find(|c| c.protocol.eq_ignore_ascii_case("plex"))
                            .map(|c| c.id);
                        app.browsing_conn_id = target_id;
                        app.browsing_path = "/library/sections/1".to_string();
                        app.browsing_entries = vec![
                            muvrix_core::network::RemoteFileEntry {
                                name: "Elephants Dream (2006).mkv".to_string(),
                                remote_path: "/library/parts/4001/file.mkv".to_string(),
                                is_dir: false,
                                size_bytes: 367001600,
                                modified_timestamp: 1700000000,
                            },
                            muvrix_core::network::RemoteFileEntry {
                                name: "Cosmos Laundromat (2015).mp4".to_string(),
                                remote_path: "/library/parts/4002/file.mp4".to_string(),
                                is_dir: false,
                                size_bytes: 720000000,
                                modified_timestamp: 1700000000,
                            },
                        ];
                    }
                    _ => {}
                }
            }

            // Populate connection pool telemetry with realistic WAN keep-alive stats
            let _ = app
                .connection_pool
                .acquire("jellyfin", "jellyfin.local", 8096);
            let p1 = app.connection_pool.acquire("plex", "plex.cloud.tv", 32400);
            app.connection_pool.record_rtt(
                "plex",
                "plex.cloud.tv",
                32400,
                std::time::Duration::from_millis(224),
            );
            app.connection_pool.release(p1);
            let p2 = app.connection_pool.acquire("plex", "plex.cloud.tv", 32400);
            app.connection_pool.release(p2);
        }

        Ok(app)
    }
}
