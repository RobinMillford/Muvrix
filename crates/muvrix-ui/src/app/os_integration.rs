use std::sync::Arc;
use std::time::{Duration, Instant};

use muvrix_player::equalizer::EqualizerManager;
use muvrix_player::{classify_source, plan_presentation, DecodePath, Event, PlaybackState};

use crate::app::{format_timestamp, MuvrixApp, PlaybackStats};
use crate::os_integration::{should_inhibit, MediaSession, MediaSnapshot};
use crate::wake::WakeHandle;

impl MuvrixApp {
    pub(crate) fn sync_desktop_integration(&mut self) {
        let actions = self
            .media_session
            .as_ref()
            .map(MediaSession::take_actions)
            .unwrap_or_default();
        for action in actions {
            self.apply_media_action(action);
        }

        if let Some(session) = &self.media_session {
            let title = self
                .controller
                .current_file()
                .and_then(|path| std::path::Path::new(path).file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("No media")
                .to_string();
            let status = match self.controller.state() {
                PlaybackState::Playing | PlaybackState::Buffering => "Playing",
                PlaybackState::Paused => "Paused",
                _ => "Stopped",
            };
            let playlist_pos = self.controller.playlist_pos();
            let playlist_count = self.controller.playlist_count();
            session.update(MediaSnapshot {
                title,
                artist: String::new(),
                playback_status: status.to_string(),
                duration_us: (self.controller.duration() * 1_000_000.0) as i64,
                position_us: (self.controller.time_pos() * 1_000_000.0) as i64,
                can_next: playlist_pos
                    .map(|pos| pos + 1 < playlist_count)
                    .unwrap_or(false),
                can_previous: playlist_pos.map(|pos| pos > 0).unwrap_or(false),
            });
        }

        let should_inhibit_now = should_inhibit(
            matches!(
                self.controller.state(),
                PlaybackState::Playing | PlaybackState::Buffering
            ),
            self.controller.is_paused(),
            self.controller.current_file().is_some(),
        );
        if should_inhibit_now != self.sleep_inhibited {
            self.sleep_inhibitor.set_inhibited(should_inhibit_now);
            self.sleep_inhibited = should_inhibit_now;
        }
    }

    ///
    /// The request (`hwdec`) is never treated as evidence: only `hwdec-current`
    /// plus the decoded frame's pixel format decide the reported path.
    pub(crate) fn check_decoder_path(&mut self) {
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
    pub(crate) fn flush_telemetry(&mut self) {
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
    pub(crate) fn sync_playback_state(&mut self) {
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

    /// Measures actual frame presentation rate and frame delta times.
    pub(crate) fn update_frame_timing(&mut self) {
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
}
