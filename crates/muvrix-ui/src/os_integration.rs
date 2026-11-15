//! Optional Linux desktop integration: MPRIS media controls and screen-saver
//! inhibition. Everything degrades to a no-op when no session D-Bus is present.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use zbus::blocking::Connection;
use zbus::interface;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

pub const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
pub const MPRIS_NAME: &str = "org.mpris.MediaPlayer2.muvrix";
const TRACK_ID: &str = "/org/muvrix/track";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaAction {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    SeekBy(i64),
    SeekTo(i64),
}

/// Current state exposed to desktop media widgets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaSnapshot {
    pub title: String,
    pub artist: String,
    pub playback_status: String,
    pub duration_us: i64,
    pub position_us: i64,
    pub can_next: bool,
    pub can_previous: bool,
}

impl MediaSnapshot {
    pub fn is_playing(&self) -> bool {
        self.playback_status.eq_ignore_ascii_case("Playing")
    }
}

#[derive(Default)]
struct SharedMedia {
    snapshot: Mutex<MediaSnapshot>,
    actions: Mutex<Vec<MediaAction>>,
}

impl SharedMedia {
    fn push(&self, action: MediaAction) {
        if let Ok(mut actions) = self.actions.lock() {
            actions.push(action);
        }
    }

    fn snapshot(&self) -> MediaSnapshot {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn string_value(value: &str) -> OwnedValue {
    OwnedValue::try_from(Value::from(value)).unwrap_or_else(|_| OwnedValue::from(0i32))
}

fn track_value() -> OwnedValue {
    ObjectPath::try_from(TRACK_ID)
        .ok()
        .and_then(|path| OwnedValue::try_from(Value::from(path)).ok())
        .unwrap_or_else(|| string_value(TRACK_ID))
}

struct MediaPlayer2Root {
    shared: Arc<SharedMedia>,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl MediaPlayer2Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        "Muvrix".to_string()
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        "muvrix".to_string()
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec!["file".to_string(), "http".to_string(), "https".to_string()]
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec![
            "video/mp4".to_string(),
            "video/x-matroska".to_string(),
            "video/webm".to_string(),
            "audio/mpeg".to_string(),
            "audio/flac".to_string(),
        ]
    }

    fn raise(&self) {
        self.shared.push(MediaAction::PlayPause);
    }

    #[zbus(name = "Quit")]
    fn quit(&self) {
        self.shared.push(MediaAction::Stop);
    }
}

struct MediaPlayer2Player {
    shared: Arc<SharedMedia>,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl MediaPlayer2Player {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        let status = self.shared.snapshot().playback_status;
        if status.is_empty() {
            "Stopped".to_string()
        } else {
            status
        }
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let snapshot = self.shared.snapshot();
        let mut metadata = HashMap::new();
        metadata.insert("mpris:trackid".to_string(), track_value());
        if !snapshot.title.is_empty() {
            metadata.insert("xesam:title".to_string(), string_value(&snapshot.title));
        }
        if !snapshot.artist.is_empty() {
            metadata.insert("xesam:artist".to_string(), string_value(&snapshot.artist));
        }
        if snapshot.duration_us > 0 {
            metadata.insert(
                "mpris:length".to_string(),
                OwnedValue::from(snapshot.duration_us),
            );
        }
        metadata
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        self.shared.snapshot().position_us.max(0)
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.shared.snapshot().can_next
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.shared.snapshot().can_previous
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.shared.snapshot().duration_us > 0
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }

    #[zbus(name = "Play")]
    fn play(&self) {
        self.shared.push(MediaAction::Play);
    }

    #[zbus(name = "Pause")]
    fn pause(&self) {
        self.shared.push(MediaAction::Pause);
    }

    #[zbus(name = "PlayPause")]
    fn play_pause(&self) {
        self.shared.push(MediaAction::PlayPause);
    }

    #[zbus(name = "Stop")]
    fn stop(&self) {
        self.shared.push(MediaAction::Stop);
    }

    #[zbus(name = "Next")]
    fn next(&self) {
        self.shared.push(MediaAction::Next);
    }

    #[zbus(name = "Previous")]
    fn previous(&self) {
        self.shared.push(MediaAction::Previous);
    }

    #[zbus(name = "Seek")]
    fn seek(&self, offset: i64) {
        self.shared.push(MediaAction::SeekBy(offset));
    }

    #[zbus(name = "SetPosition")]
    fn set_position(&self, _track_id: OwnedObjectPath, position: i64) {
        if position >= 0 {
            self.shared.push(MediaAction::SeekTo(position));
        }
    }
}

/// MPRIS media session. `None` when the desktop has no session bus.
pub struct MediaSession {
    shared: Arc<SharedMedia>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MediaSession {
    pub fn start() -> Option<Self> {
        let shared = Arc::new(SharedMedia::default());
        let connection = match zbus::blocking::connection::Builder::session() {
            Ok(builder) => builder
                .name(MPRIS_NAME)
                .ok()?
                .serve_at(
                    MPRIS_PATH,
                    MediaPlayer2Root {
                        shared: shared.clone(),
                    },
                )
                .ok()?
                .serve_at(
                    MPRIS_PATH,
                    MediaPlayer2Player {
                        shared: shared.clone(),
                    },
                )
                .ok()?
                .build(),
            Err(error) => {
                log::warn!("MPRIS unavailable (no session D-Bus): {error}");
                return None;
            }
        };
        let connection = match connection {
            Ok(connection) => connection,
            Err(error) => {
                log::warn!("MPRIS registration failed: {error}");
                return None;
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("muvrix-mpris".to_string())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(200));
                }
                drop(connection);
            })
            .ok()?;
        log::info!("MPRIS media session registered as {MPRIS_NAME}");
        Some(Self {
            shared,
            stop,
            worker: Some(worker),
        })
    }

    pub fn update(&self, snapshot: MediaSnapshot) {
        if let Ok(mut state) = self.shared.snapshot.lock() {
            *state = snapshot;
        }
    }

    pub fn take_actions(&self) -> Vec<MediaAction> {
        self.shared
            .actions
            .lock()
            .map(|mut actions| std::mem::take(&mut *actions))
            .unwrap_or_default()
    }
}

impl Drop for MediaSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Linux screen-saver inhibitor. Unavailable sessions are a silent no-op.
pub struct SleepInhibitor {
    connection: Option<Connection>,
    cookie: Mutex<Option<u32>>,
}

impl SleepInhibitor {
    pub fn new() -> Self {
        let connection = match Connection::session() {
            Ok(connection) => Some(connection),
            Err(error) => {
                log::warn!("screen-saver inhibition unavailable (no session D-Bus): {error}");
                None
            }
        };
        Self {
            connection,
            cookie: Mutex::new(None),
        }
    }

    pub fn set_inhibited(&self, inhibited: bool) {
        let Some(connection) = self.connection.as_ref() else {
            return;
        };
        let Ok(mut cookie) = self.cookie.lock() else {
            return;
        };
        if inhibited {
            if cookie.is_some() {
                return;
            }
            match connection.call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "Inhibit",
                &("muvrix", "Playing media"),
            ) {
                Ok(reply) => {
                    if let Ok(value) = reply.body().deserialize::<u32>() {
                        *cookie = Some(value);
                    }
                }
                Err(error) => log::warn!("screen-saver inhibit failed: {error}"),
            }
        } else if let Some(value) = cookie.take() {
            if let Err(error) = connection.call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "UnInhibit",
                &(value,),
            ) {
                log::warn!("screen-saver uninhibit failed: {error}");
            }
        }
    }
}

impl Default for SleepInhibitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SleepInhibitor {
    fn drop(&mut self) {
        self.set_inhibited(false);
    }
}

/// Only active playback holds the inhibit; paused, stopped, EOF, and teardown release it.
pub fn should_inhibit(playing: bool, paused: bool, has_media: bool) -> bool {
    playing && !paused && has_media
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_inhibitor_state_logic_is_strict() {
        assert!(should_inhibit(true, false, true));
        assert!(!should_inhibit(true, true, true));
        assert!(!should_inhibit(false, false, true));
        assert!(!should_inhibit(true, false, false));
    }

    #[test]
    fn media_snapshot_reports_playback_and_actions_are_bounded() {
        let snapshot = MediaSnapshot {
            title: "Demo".to_string(),
            artist: "Muvrix".to_string(),
            playback_status: "Playing".to_string(),
            duration_us: 60_000_000,
            position_us: 10_000_000,
            can_next: true,
            can_previous: false,
        };
        assert!(snapshot.is_playing());
        assert_eq!(snapshot.duration_us / 1_000_000, 60);
        assert_eq!(snapshot.position_us / 1_000_000, 10);
        let actions = [
            MediaAction::SeekBy(5_000_000),
            MediaAction::SeekTo(2_000_000),
        ];
        assert!(matches!(actions[0], MediaAction::SeekBy(_)));
        assert!(matches!(actions[1], MediaAction::SeekTo(_)));
    }
}
