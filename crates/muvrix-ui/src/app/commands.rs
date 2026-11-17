use std::time::{Duration, Instant};

use eframe::egui::{self, Vec2};
use muvrix_core::lyrics::{resolve_lyrics, LrcFile, LyricSource};
use muvrix_player::equalizer::EqualizerManager;
use muvrix_player::shader::ShaderManager;
use muvrix_player::{AbLoopState, MpvexCommand, PlaybackState, RepeatMode, SeekMode};

use crate::app::{
    format_timestamp, mini_player_window_state, sleep_deadline_for, MuvrixApp, SleepTimer,
};
use crate::config_editor::{self, ConfigEditorFile};
use crate::os_integration::MediaAction;
use crate::views::AppView;

impl MuvrixApp {
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

                        // Auto-detect lyrics following 4-tier precedence (external / embedded)
                        self.reload_lyrics_for_current_media();

                        // Refresh active media bookmarks
                        self.refresh_active_bookmarks();
                        self.current_view = AppView::Player;
                        self.sync_audio_preferences();
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

    /// Loads a media file and immediately resumes playback at the designated timestamp.
    pub fn load_file_and_resume(&mut self, raw_path: &str, resume_pos: f64) {
        self.load_file(raw_path);
        if resume_pos > 0.0 {
            let _ = self.controller.seek(resume_pos, SeekMode::Absolute);
            self.pending_resume_prompt = None;
            let _ = self.controller.play();
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
    pub(crate) fn reset_decoder_for_new_media(&mut self) {
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

    pub fn frame_step(&mut self) {
        let _ = self.controller.frame_step();
    }

    pub fn frame_back_step(&mut self) {
        let _ = self.controller.frame_back_step();
    }

    pub fn cycle_ab_loop(&mut self) {
        match self.controller.cycle_ab_loop() {
            Ok(()) => {
                self.status_message = match self.controller.ab_loop_state() {
                    AbLoopState::None => "A-B loop cleared".to_string(),
                    AbLoopState::PointA(a) => {
                        format!("A-B loop: A set at {}", format_timestamp(a))
                    }
                    AbLoopState::Loop { a, b } => format!(
                        "A-B loop: {} → {}",
                        format_timestamp(a),
                        format_timestamp(b)
                    ),
                };
            }
            Err(e) => self.status_message = format!("A-B loop: {e}"),
        }
    }

    pub fn cycle_repeat_mode(&mut self) {
        let next = match self.controller.repeat_mode() {
            RepeatMode::Off => RepeatMode::One,
            RepeatMode::One => RepeatMode::All,
            RepeatMode::All => RepeatMode::Off,
        };
        if let Err(e) = self.controller.set_repeat_mode(next) {
            self.status_message = format!("Repeat mode: {e}");
        }
    }

    pub fn set_panscan(&mut self, value: f64) {
        let _ = self.controller.set_panscan(value);
    }

    pub fn set_sleep_timer(&mut self, timer: SleepTimer) {
        self.sleep_timer = timer;
        self.sleep_deadline = sleep_deadline_for(timer, Instant::now());
        self.status_message = format!("Sleep timer: {}", timer.label());
    }

    pub fn sleep_remaining(&self) -> Option<Duration> {
        self.sleep_deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub(crate) fn tick_sleep_timer(&mut self) {
        match self.sleep_timer {
            SleepTimer::Off => {}
            SleepTimer::Minutes(_) => {
                if self
                    .sleep_deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
                {
                    let _ = self.controller.pause();
                    self.sleep_timer = SleepTimer::Off;
                    self.sleep_deadline = None;
                    self.status_message = "Sleep timer expired; playback paused".to_string();
                }
            }
            SleepTimer::EndOfFile => {
                if *self.controller.state() == PlaybackState::Ended {
                    let _ = self.controller.pause();
                    self.sleep_timer = SleepTimer::Off;
                    self.status_message = "Sleep timer: end of file reached".to_string();
                }
            }
        }
    }

    pub(crate) fn apply_media_action(&mut self, action: MediaAction) {
        match action {
            MediaAction::Play => {
                let _ = self.controller.play();
            }
            MediaAction::Pause => {
                let _ = self.controller.pause();
            }
            MediaAction::PlayPause => {
                let _ = self.controller.toggle_pause();
            }
            MediaAction::Stop => {
                let _ = self.controller.stop();
            }
            MediaAction::Next => {
                let _ = self.controller.playlist_next();
            }
            MediaAction::Previous => {
                let _ = self.controller.playlist_prev();
            }
            MediaAction::SeekBy(offset_us) => {
                let _ = self
                    .controller
                    .seek_relative(offset_us as f64 / 1_000_000.0);
            }
            MediaAction::SeekTo(position_us) => {
                let mut seconds = (position_us.max(0) as f64) / 1_000_000.0;
                let duration = self.controller.duration();
                if duration > 0.0 {
                    seconds = seconds.min(duration);
                }
                let _ = self.controller.seek(seconds, SeekMode::Absolute);
            }
            MediaAction::SetVolume(vol) => {
                let _ = self.controller.set_volume(vol * 100.0);
            }
        }
    }

    pub(crate) fn dispatch_mpvex_commands(&mut self) {
        for command in self.controller.drain_mpvex_commands() {
            match command {
                MpvexCommand::ShowText(text) => self.status_message = text,
                MpvexCommand::ToggleUi => {
                    self.ui_overlay_visible = !self.ui_overlay_visible;
                }
                MpvexCommand::ShowPanel(name) => {
                    self.current_view = match name.to_ascii_lowercase().as_str() {
                        "player" => AppView::Player,
                        "library" => AppView::Library,
                        "browser" | "files" => AppView::Browser,
                        "playlist" | "queue" => AppView::Playlist,
                        "lyrics" => AppView::Lyrics,
                        "history" | "recent" => AppView::Recent,
                        "settings" => AppView::Settings,
                        _ => self.current_view,
                    };
                }
                MpvexCommand::SeekBy(delta) => {
                    let _ = self.controller.seek(delta, SeekMode::Relative);
                }
                MpvexCommand::SeekTo(target) => {
                    let _ = self.controller.seek(target, SeekMode::Absolute);
                }
            }
        }
    }

    pub(crate) fn ensure_config_editor_loaded(&mut self) {
        if self.config_editor_loaded {
            return;
        }
        match config_editor::load(self.config_editor_file) {
            Ok(text) => {
                self.config_editor_text = text;
                self.config_editor_status = format!("Loaded {}", self.config_editor_file.label());
            }
            Err(e) => self.config_editor_status = format!("Load failed: {e}"),
        }
        self.config_editor_loaded = true;
    }

    pub(crate) fn save_config_editor(&mut self) {
        match config_editor::save(self.config_editor_file, &self.config_editor_text) {
            Ok(path) => {
                self.config_editor_status = format!("Saved {}", path.display());
                if self.config_editor_file == ConfigEditorFile::MpvConf {
                    let _ = self
                        .controller
                        .handle()
                        .command(&["load-config-file", path.to_string_lossy().as_ref()]);
                }
            }
            Err(e) => self.config_editor_status = format!("Save failed: {e}"),
        }
    }

    pub fn add_to_selected_playlist(
        &mut self,
        path: String,
        title: Option<String>,
        duration_ms: u64,
    ) {
        let Some(playlist_id) = self.selected_persistent_playlist else {
            self.playlist_status = Some("Select or create a saved playlist first".to_string());
            return;
        };
        match self
            .db
            .add_playlist_entry(playlist_id, &path, title.as_deref(), duration_ms)
        {
            Ok(_) => self.playlist_status = Some(format!("Added to saved playlist: {path}")),
            Err(e) => self.playlist_status = Some(format!("Playlist add failed: {e}")),
        }
    }

    pub fn play_saved_playlist(&mut self, playlist_id: i64) -> Result<(), String> {
        let entries = self
            .db
            .list_playlist_entries(playlist_id)
            .map_err(|e| e.to_string())?;
        if entries.is_empty() {
            return Err("Playlist is empty".to_string());
        }
        let _ = self.controller.playlist_clear();
        for entry in &entries {
            self.controller
                .playlist_append(&entry.path)
                .map_err(|e| e.to_string())?;
        }
        self.controller
            .playlist_play_index(0)
            .map_err(|e| e.to_string())?;
        self.sync_active_queue_to_db();
        self.current_view = AppView::Player;
        Ok(())
    }

    /// Persists current playback queue and cursor into SQLite.
    pub fn sync_active_queue_to_db(&self) {
        let items = self.controller.playlist();
        let cur_pos = self.controller.playlist_pos();
        let active_entries: Vec<muvrix_core::database::ActiveQueueEntry> = items
            .iter()
            .enumerate()
            .map(|(idx, item)| muvrix_core::database::ActiveQueueEntry {
                position: idx,
                path: item.filename.clone(),
                title: item.title.clone(),
                duration_ms: 0,
                is_current: cur_pos == Some(idx) || item.is_current || item.is_playing,
            })
            .collect();
        let _ = self.db.save_active_queue(&active_entries);
    }

    /// Clears the active playback queue in SQLite.
    pub fn clear_active_queue_in_db(&self) {
        let _ = self.db.clear_active_queue();
    }

    /// Shuffles the active queue. Uses smart shuffle with artist/album separation if enabled.
    pub fn execute_shuffle(&mut self) {
        if self.playlist_use_smart_shuffle {
            let playlist = self.controller.playlist().to_vec();
            if playlist.is_empty() {
                return;
            }
            let tracks: Vec<muvrix_player::shuffle::ShuffleTrackInfo> = playlist
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let mut artist = None;
                    let mut album = None;
                    let mut genre = None;
                    if let Ok(Some(lib_item)) =
                        self.db.get_media_item_by_canonical_path(&item.filename)
                    {
                        if let Ok(Some(meta)) = self.db.get_metadata(lib_item.id) {
                            artist = meta.artist;
                            album = meta.album;
                            genre = meta.genre;
                        }
                    }
                    muvrix_player::shuffle::ShuffleTrackInfo {
                        index: idx,
                        title: item.title.clone(),
                        artist,
                        album,
                        genre,
                    }
                })
                .collect();
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(42);
            let _ = self.controller.playlist_smart_shuffle(&tracks, seed);
            self.playlist_status = Some("Smart shuffled queue with artist separation".to_string());
        } else {
            let _ = self.controller.playlist_shuffle();
            self.playlist_status = Some("Shuffled queue".to_string());
        }
        self.sync_active_queue_to_db();
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

    /// Toggles compact mini-player floating mode (resizes main window to 480x270 and requests AlwaysOnTop).
    pub fn toggle_pip(&mut self, ctx: &egui::Context) {
        self.is_pip = !self.is_pip;
        let (decorations, level) = mini_player_window_state(self.is_pip);
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(decorations));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
        if self.is_pip {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(480.0, 270.0)));
            self.status_message =
                "Compact Mini-Player mode enabled (Always on Top is best-effort on Wayland)"
                    .to_string();
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(1280.0, 720.0)));
            self.status_message = "Exited Compact Mini-Player mode".to_string();
        }
    }

    /// Reloads lyrics for the currently active media file using the 4-tier precedence:
    /// 1. External synchronized LRC
    /// 2. Embedded synchronized lyrics
    /// 3. External unsynchronized text
    /// 4. Embedded unsynchronized text
    pub fn reload_lyrics_for_current_media(&mut self) {
        let current_path = self.controller.current_file().map(std::path::PathBuf::from);
        let embedded = self.controller.embedded_lyrics();
        let media_title = self.controller.media_title();

        if let Some(ref path) = current_path {
            if let Some(mut lrc) = resolve_lyrics(path, embedded.as_deref()) {
                if lrc.title.is_none() {
                    lrc.title = media_title.clone();
                }
                log::info!(
                    "Resolved lyrics for {:?}: source={:?}, synced={}",
                    path,
                    lrc.source,
                    lrc.is_synchronized
                );
                self.lyrics = Some(lrc);
                return;
            }
        } else if let Some(ref embedded_text) = embedded {
            if let Ok(mut lrc) = LrcFile::parse_auto(embedded_text, LyricSource::EmbeddedSynced) {
                if lrc.title.is_none() {
                    lrc.title = media_title;
                }
                self.lyrics = Some(lrc);
                return;
            }
        }

        self.lyrics = None;
    }

    /// Opens a native file dialog to manually select a synchronized LRC or plain text lyrics file.
    pub fn load_lyrics_dialog(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Open Synchronized or Plain Lyrics File")
            .add_filter("Lyrics Files", &["lrc", "txt"])
            .pick_file()
        {
            match LrcFile::from_file_auto(&file) {
                Ok(lrc) => {
                    let title = lrc.title.as_deref().unwrap_or("lyrics");
                    self.status_message = format!(
                        "Loaded lyrics for '{title}' ({})",
                        lrc.source.display_badge()
                    );
                    self.lyrics = Some(lrc);
                }
                Err(e) => {
                    self.status_message = format!("Failed to parse lyrics: {e}");
                }
            }
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
        let _ = ShaderManager::apply_shader_pipeline(
            self.controller.handle(),
            self.anime4k_preset,
            self.anime4k_quality,
            None,
            &self.video_settings.custom_shaders,
        );
        self.save_media_settings();
    }

    pub fn apply_equalizer_settings(&mut self) {
        self.equalizer_settings.sync_gains_to_bands();
        let needs_pan = self.audio_channels.needs_pan_filter();
        let eq = self.equalizer_settings.clone();
        let _ = EqualizerManager::apply_unified_to_handle(self.controller.handle(), &eq, needs_pan);
        self.volume_normalization =
            eq.effective_normalization_mode() == muvrix_player::NormalizationMode::Dynamic;
        self.save_media_settings();
    }

    /// Opens the native file picker to select and import an AutoEq profile (.txt or .json).
    pub fn import_autoeq_dialog(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Import AutoEq Profile")
            .add_filter("AutoEq Presets (*.txt, *.json)", &["txt", "json"])
            .pick_file()
        {
            self.load_autoeq_from_path(&file);
        }
    }

    /// Loads, parses, validates and applies an AutoEq profile from disk.
    pub fn load_autoeq_from_path(&mut self, path: &std::path::Path) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                let err_msg = format!("Failed to read AutoEq file: {e}");
                self.autoeq_status_msg = Some((err_msg.clone(), true));
                self.status_message = err_msg;
                return;
            }
        };

        let file_stem = path.file_stem().and_then(|s| s.to_str());
        match muvrix_player::AutoEqProfile::parse(&content, file_stem) {
            Ok(profile) => {
                let name = profile.name.clone();
                let filter_count = profile.filters.len();
                let preamp = profile.preamp_db;
                self.equalizer_settings.apply_autoeq_profile(profile);
                self.apply_equalizer_settings();
                let success_msg = format!(
                    "Loaded AutoEq profile '{}' ({} filters, Preamp: {:.1} dB)",
                    name, filter_count, preamp
                );
                self.autoeq_status_msg = Some((success_msg.clone(), false));
                self.status_message = success_msg;
            }
            Err(e) => {
                let err_msg = format!("AutoEq parse error: {e}");
                self.autoeq_status_msg = Some((err_msg.clone(), true));
                self.status_message = err_msg;
            }
        }
    }

    /// Opens a native file picker to export the active equalizer configuration as AutoEq.
    pub fn export_autoeq_dialog(&mut self) {
        let default_name = self
            .equalizer_settings
            .autoeq_profile
            .as_ref()
            .map(|p| format!("{}.txt", p.name.replace(' ', "_")))
            .unwrap_or_else(|| "muvrix-eq-profile.txt".to_string());

        if let Some(file) = rfd::FileDialog::new()
            .set_title("Export Equalizer Preset")
            .set_file_name(&default_name)
            .add_filter("AutoEq Text Preset (*.txt)", &["txt"])
            .add_filter("AutoEq JSON Preset (*.json)", &["json"])
            .save_file()
        {
            self.save_autoeq_to_path(&file);
        }
    }

    /// Serializes and writes the active equalizer configuration to the target path.
    pub fn save_autoeq_to_path(&mut self, path: &std::path::Path) {
        let is_json = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.eq_ignore_ascii_case("json"))
            .unwrap_or(false);

        let profile = if let Some(ref p) = self.equalizer_settings.autoeq_profile {
            p.clone()
        } else {
            // Build profile from 10 bands
            let mut filters = Vec::new();
            for band in &self.equalizer_settings.bands {
                if let Ok(f) = muvrix_player::AutoEqFilter::new_with_enabled(
                    muvrix_player::AutoEqFilterType::Peaking,
                    band.frequency_hz,
                    band.gain_db,
                    band.q_factor,
                    band.enabled,
                ) {
                    filters.push(f);
                }
            }
            match muvrix_player::AutoEqProfile::new(
                "Muvrix Custom Preset".to_string(),
                self.equalizer_settings.preamp_db,
                filters,
            ) {
                Ok(p) => p,
                Err(e) => {
                    let err = format!("Cannot export preset: {e}");
                    self.autoeq_status_msg = Some((err.clone(), true));
                    self.status_message = err;
                    return;
                }
            }
        };

        let result = if is_json {
            profile.export_json().map_err(|e| e.to_string())
        } else {
            Ok(profile.export_text())
        };

        match result {
            Ok(text) => match std::fs::write(path, text) {
                Ok(_) => {
                    let msg = format!("Exported equalizer profile to {}", path.display());
                    self.autoeq_status_msg = Some((msg.clone(), false));
                    self.status_message = msg;
                }
                Err(e) => {
                    let err = format!("Failed to write export file: {e}");
                    self.autoeq_status_msg = Some((err.clone(), true));
                    self.status_message = err;
                }
            },
            Err(e) => {
                let err = format!("Failed to serialize profile: {e}");
                self.autoeq_status_msg = Some((err.clone(), true));
                self.status_message = err;
            }
        }
    }

    /// Clears any active AutoEq profile, reverting to the 10-band equalizer.
    pub fn clear_autoeq_profile(&mut self) {
        self.equalizer_settings.clear_autoeq_profile();
        self.autoeq_status_msg = None;
        self.apply_equalizer_settings();
        self.status_message =
            "Cleared AutoEq profile; reverted to standard 10-band equalizer.".to_string();
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
            subtitle_styling: self.active_styling.clone(),
            theme_mode: self.theme_mode,
            accent_color: self.accent_color,
            audio_channels: self.audio_channels,
            volume_normalization: self.volume_normalization,
            audio_pitch_correction: self.audio_pitch_correction,
            preferred_audio_languages: self.preferred_audio_languages.clone(),
            default_audio_delay_ms: self.default_audio_delay_ms,
            volume_boost_cap: self.equalizer_settings.volume_boost as i32,
            background_audio: self.background_audio,
            verbose_logging: self.verbose_logging,
            stream_buffer_mb: self.stream_buffer_mb,
            stream_back_buffer_mb: self.stream_back_buffer_mb,
            stream_readahead_secs: self.stream_readahead_secs,
            stream_max_retries: self.stream_max_retries,
        };
        let _ = settings.save(None);
    }

    /// Applies demuxer buffer sizing and stream read-ahead settings to libmpv and saves preferences.
    pub fn apply_stream_buffering_settings(&mut self) {
        let max_bytes = (self.stream_buffer_mb as u64) * 1024 * 1024;
        let max_back_bytes = (self.stream_back_buffer_mb as u64) * 1024 * 1024;
        let readahead_secs = self.stream_readahead_secs as f64;
        let _ =
            self.controller
                .configure_stream_buffering(max_bytes, max_back_bytes, readahead_secs);
        self.save_media_settings();
    }

    /// Synchronizes audio channels, volume normalization filter, pitch correction, and languages with libmpv.
    pub fn sync_audio_preferences(&self) {
        let _ = self.controller.set_audio_channels(self.audio_channels);
        let _ = self
            .controller
            .set_audio_pitch_correction(self.audio_pitch_correction);
        let _ = self
            .controller
            .set_audio_languages(&self.preferred_audio_languages);
        let _ = self
            .controller
            .set_audio_delay(self.default_audio_delay_ms as f64 / 1000.0);
        let needs_pan = self.audio_channels.needs_pan_filter();
        let mut eq = self.equalizer_settings.clone();
        if self.volume_normalization {
            eq.dynaudnorm = true;
        }
        let _ = EqualizerManager::apply_unified_to_handle(self.controller.handle(), &eq, needs_pan);
    }

    /// Exports current application configuration to a user-selected XML backup file.
    pub fn export_settings_to_xml(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Export Muvrix Settings Backup")
            .set_file_name("muvrix-settings-backup.xml")
            .add_filter("XML Backup", &["xml"])
            .save_file()
        {
            let backup = muvrix_core::backup::BackupSettings {
                schema_version: muvrix_core::backup::CURRENT_BACKUP_SCHEMA,
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                created_at_unix: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                audio_channels: format!("{:?}", self.audio_channels).to_lowercase(),
                volume_normalization: self.volume_normalization,
                audio_pitch_correction: self.audio_pitch_correction,
                preferred_audio_languages: self.preferred_audio_languages.clone(),
                default_audio_delay_ms: self.default_audio_delay_ms,
                volume_boost_cap: self.equalizer_settings.volume_boost as i32,
                volume: self.controller.volume(),
                theme_mode: format!("{:?}", self.theme_mode).to_lowercase(),
                accent_color: format!("{:?}", self.accent_color).to_lowercase(),
                sub_font: self.active_styling.font.clone(),
                sub_font_size: self.active_styling.font_size as u32,
                sub_color: self.active_styling.color.clone(),
                sub_border_color: self.active_styling.border_color.clone(),
                sub_border_size: self.active_styling.border_size,
                sub_shadow_offset: self.active_styling.sub_shadow_offset,
                sub_back_color: self.active_styling.sub_back_color.clone(),
                sub_margin_y: self.active_styling.margin_y as u32,
                sub_pos: self.active_styling.pos as u32,
                sub_speed: self.active_styling.sub_speed,
                sub_justify: format!("{:?}", self.active_styling.sub_justify).to_lowercase(),
                ass_override: self.active_styling.ass_override,
                anime4k_preset: format!("{:?}", self.anime4k_preset).to_lowercase(),
                anime4k_quality: format!("{:?}", self.anime4k_quality).to_lowercase(),
                tone_mapping: format!("{:?}", self.video_settings.tone_mapping).to_lowercase(),
                deband: self.video_settings.deband,
                history_enabled: self.history_store.enabled,
                auto_resume_enabled: self.history_store.auto_resume_enabled,
                verbose_logging: self.verbose_logging,
                background_audio: self.background_audio,
            };

            let xml = backup.to_xml();
            match std::fs::write(&file, xml.as_bytes()) {
                Ok(()) => {
                    self.advanced_status =
                        Some(format!("Exported XML backup to {}", file.display()));
                }
                Err(e) => {
                    self.advanced_status = Some(format!("Failed to export XML: {e}"));
                }
            }
        }
    }

    /// Imports application settings from a user-selected XML backup file.
    pub fn import_settings_from_xml(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Import Muvrix Settings Backup")
            .add_filter("XML Backup", &["xml"])
            .pick_file()
        {
            match std::fs::read_to_string(&file) {
                Ok(content) => match muvrix_core::backup::BackupSettings::from_xml(&content) {
                    Ok(backup) => {
                        self.volume_normalization = backup.volume_normalization;
                        self.audio_pitch_correction = backup.audio_pitch_correction;
                        self.preferred_audio_languages = backup.preferred_audio_languages;
                        self.default_audio_delay_ms = backup.default_audio_delay_ms;
                        self.background_audio = backup.background_audio;
                        self.verbose_logging = backup.verbose_logging;
                        self.history_store.set_enabled(backup.history_enabled);
                        self.history_store
                            .set_auto_resume(backup.auto_resume_enabled);
                        let _ = self.history_store.save();

                        self.sync_audio_preferences();
                        self.save_media_settings();
                        self.advanced_status = Some(format!(
                            "Imported settings from {} (schema v{})",
                            file.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("backup"),
                            backup.schema_version
                        ));
                    }
                    Err(e) => {
                        self.advanced_status = Some(format!("Invalid XML backup: {e}"));
                    }
                },
                Err(e) => {
                    self.advanced_status = Some(format!("Failed to read backup file: {e}"));
                }
            }
        }
    }

    /// Clears generated thumbnail images and in-memory textures.
    pub fn clear_thumbnail_cache(&mut self) {
        self.thumbnail_textures.clear();
        let cache_dir = muvrix_core::thumbnails::ThumbnailCache::default_cache_dir();
        if cache_dir.exists() {
            let _ = std::fs::remove_dir_all(&cache_dir);
            let _ = std::fs::create_dir_all(&cache_dir);
        }
        self.advanced_status = Some("Thumbnail cache cleared".to_string());
    }

    /// Flushes and resets subtitle font cache directory in MPV.
    pub fn clear_font_cache(&mut self) {
        self.active_styling.sub_fonts_dir.clear();
        let _ = self
            .controller
            .handle()
            .set_property_string("sub-fonts-dir", "");
        self.save_media_settings();
        self.advanced_status = Some("Subtitle font cache reset".to_string());
    }

    /// Reloads persistent media settings from disk and synchronizes MPV.
    pub fn clear_config_cache(&mut self) {
        let loaded = muvrix_player::PersistedMediaSettings::load(None);
        self.theme_mode = loaded.theme_mode;
        self.accent_color = loaded.accent_color;
        self.audio_channels = loaded.audio_channels;
        self.volume_normalization = loaded.volume_normalization;
        self.audio_pitch_correction = loaded.audio_pitch_correction;
        self.preferred_audio_languages = loaded.preferred_audio_languages;
        self.default_audio_delay_ms = loaded.default_audio_delay_ms;
        self.background_audio = loaded.background_audio;
        self.verbose_logging = loaded.verbose_logging;
        self.sync_audio_preferences();
        self.advanced_status = Some("Reloaded settings and flushed config cache".to_string());
    }

    /// Collects and dumps diagnostic system, engine, and playback telemetry to a file.
    pub fn dump_diagnostic_logs(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title("Save Muvrix Diagnostic Report")
            .set_file_name("muvrix-diagnostics.txt")
            .add_filter("Text Report", &["txt"])
            .save_file()
        {
            let mut report = String::new();
            report.push_str(
                "======================================================================\n",
            );
            report.push_str(
                "                   MUVRIX DIAGNOSTIC SYSTEM DUMP                     \n",
            );
            report.push_str(
                "======================================================================\n\n",
            );
            report.push_str(&format!(
                "Application Version: {}\n",
                env!("CARGO_PKG_VERSION")
            ));
            report.push_str(&format!(
                "Target OS / Arch:    {} {}\n",
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
            report.push_str(&format!(
                "Desktop Session:     {}\n",
                std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".to_string())
            ));
            report.push_str(&format!(
                "Wayland Display:     {}\n",
                std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "none".to_string())
            ));
            report.push_str(&format!(
                "X11 Display:         {}\n",
                std::env::var("DISPLAY").unwrap_or_else(|_| "none".to_string())
            ));
            report.push_str(&format!(
                "Engine Core:         libmpv {}\n",
                self.media_capabilities.libmpv_version
            ));
            report.push_str(&format!(
                "libplacebo Active:   {}\n",
                self.media_capabilities.libplacebo_available
            ));
            report.push_str(&format!(
                "Decoder Status:      {:?}\n",
                self.decode_observation
            ));
            report.push_str(&format!(
                "Output Color HDR:    native_hdr_available={}\n",
                self.output.native_hdr_available
            ));
            report.push_str(&format!(
                "Playback State:      {:?}\n",
                self.controller.state()
            ));
            report.push_str(&format!(
                "Current Media:       {:?}\n",
                self.controller.current_file()
            ));
            report.push_str(&format!(
                "A/V Position / Dur:  {:.2}s / {:.2}s\n",
                self.controller.time_pos(),
                self.controller.duration()
            ));
            report.push_str(&format!(
                "Volume / Boost Max:  {:.1}% / {:.1}%\n",
                self.controller.volume(),
                self.controller.volume_boost_max()
            ));
            report.push_str(&format!(
                "Audio Channels:      {:?} (option: {})\n",
                self.audio_channels,
                self.audio_channels.mpv_channels_option()
            ));
            report.push_str(&format!(
                "Volume Normalization:{}\n",
                self.volume_normalization
            ));
            report.push_str(&format!(
                "Audio Pitch Correct: {}\n",
                self.audio_pitch_correction
            ));
            report.push_str(&format!(
                "Preferred Languages: {}\n",
                self.preferred_audio_languages
            ));
            report.push_str(&format!(
                "Equalizer Enabled:   {}\n",
                self.equalizer_settings.is_enabled
            ));
            report.push_str(&format!(
                "History Recording:   {}\n",
                self.history_store.enabled
            ));
            report.push_str(&format!(
                "Auto Resume Active:  {}\n",
                self.history_store.auto_resume_enabled
            ));
            report.push_str(&format!(
                "Total Indexed Media: {}\n",
                self.library_total_count
            ));

            match std::fs::write(&file, report.as_bytes()) {
                Ok(()) => {
                    self.advanced_status =
                        Some(format!("Saved diagnostic report to {}", file.display()));
                }
                Err(e) => {
                    self.advanced_status = Some(format!("Failed to save diagnostics: {e}"));
                }
            }
        }
    }
}

impl MuvrixApp {}
