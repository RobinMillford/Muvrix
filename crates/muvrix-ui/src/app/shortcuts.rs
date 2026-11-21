//! Keyboard shortcut registry with VLC/mpv/MUVRIX profiles.
//!
//! The registry maps stable [`PlayerAction`] identifiers to key combinations.
//! Four built-in profiles resolve the places where VLC and mpv disagree:
//!
//! * `Classic`  — VLC-compatible defaults (the shipped default).
//! * `Advanced` — mpv-style defaults for advanced playback and video control.
//! * `Hybrid`   — VLC keys win conflicts; mpv actions receive alternatives.
//! * `Legacy`   — the bindings MUVRIX shipped before the profile system.
//!
//! Every profile is editable, and edits are stored per profile so switching
//! profiles and restarting the app both preserve customisations.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Instant;

use eframe::egui::{self, RichText};
use muvrix_player::{PlaybackState, TrackType};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::app::actions::{PlayerAction, ShortcutCategory};
use crate::app::MuvrixApp;
use crate::branding::BrandPalette;
use crate::views::AppView;

/// Which default catalogue the registry is currently resolving.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutProfile {
    /// MUVRIX defaults, built from the VLC key map. Shipped as the default.
    #[default]
    Classic,
    /// mpv-inspired defaults for advanced playback and video control.
    Advanced,
    /// VLC defaults first, mpv-only actions on alternative combinations.
    Hybrid,
    /// The original MUVRIX bindings.
    Legacy,
    /// A blank slate: starts from the MUVRIX defaults and keeps every edit.
    Custom,
}

impl ShortcutProfile {
    pub const ALL: &'static [ShortcutProfile] = &[
        ShortcutProfile::Classic,
        ShortcutProfile::Advanced,
        ShortcutProfile::Hybrid,
        ShortcutProfile::Legacy,
        ShortcutProfile::Custom,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Classic => "MUVRIX Default (VLC)",
            Self::Advanced => "mpv-inspired",
            Self::Hybrid => "MUVRIX Hybrid (VLC + mpv)",
            Self::Legacy => "Legacy MUVRIX",
            Self::Custom => "Custom",
        }
    }

    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Advanced => "Advanced",
            Self::Hybrid => "Hybrid",
            Self::Legacy => "Legacy",
            Self::Custom => "Custom",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Classic => {
                "Familiar VLC shortcuts for playback, seeking, audio, subtitles and video."
            }
            Self::Advanced => {
                "mpv-style bindings for frame stepping, pan/zoom, statistics and advanced control."
            }
            Self::Hybrid => {
                "VLC bindings take priority; mpv-only actions move to alternative combinations."
            }
            Self::Legacy => "The original MUVRIX bindings, preserved for existing users.",
            Self::Custom => "Your own table. Starts from the MUVRIX defaults and keeps every edit.",
        }
    }

    pub(crate) fn source_badge(&self) -> &'static str {
        match self {
            Self::Classic | Self::Hybrid | Self::Legacy => "VLC",
            Self::Advanced => "mpv",
            Self::Custom => "Custom",
        }
    }
}

/// Which slot of an action a conflicting collision occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingSlot {
    Primary,
    Alternate,
}

/// A duplicate binding between two actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyConflict {
    pub action_a: PlayerAction,
    pub slot_a: BindingSlot,
    pub action_b: PlayerAction,
    pub slot_b: BindingSlot,
    pub combination: KeyCombination,
}

/// Result of attempting to bind a combination to an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindOutcome {
    Bound,
    Conflict(Vec<(PlayerAction, BindingSlot)>),
}

/// A conflict awaiting the user's replace/swap/cancel decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingShortcutConflict {
    pub action: PlayerAction,
    pub combination: KeyCombination,
    pub conflicts: Vec<(PlayerAction, BindingSlot)>,
}

/// A specific physical key combined with modifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyCombination {
    pub key: egui::Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyCombination {
    pub fn new(key: egui::Key) -> Self {
        Self {
            key,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }

    pub fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Evaluates if this combination matches the active egui input state.
    ///
    /// Only fresh (non-repeat) presses match. Discrete actions such as
    /// play/pause must never fire twice because a key was held down; use
    /// [`Self::matches_with_repeat`] for continuous actions like seeking.
    pub fn matches(&self, input: &egui::InputState) -> bool {
        self.matches_with_repeat(input, false)
    }

    /// Evaluates this combination, optionally accepting auto-repeat presses.
    ///
    /// Modifiers are taken from the key event itself rather than from the
    /// frame's aggregate [`egui::InputState::modifiers`]. The aggregate is the
    /// state at the *end* of the frame, so a fast press-then-release of a
    /// modifier inside one frame made `Ctrl+Arrow` miss its own press.
    pub fn matches_with_repeat(&self, input: &egui::InputState, allow_repeat: bool) -> bool {
        input.events.iter().any(|event| {
            let egui::Event::Key {
                key,
                pressed: true,
                repeat,
                modifiers,
                ..
            } = event
            else {
                return false;
            };
            *key == self.key && (allow_repeat || !*repeat) && self.modifiers_match(*modifiers)
        })
    }

    /// True when the modifier flags of a key event satisfy this combination.
    pub fn modifiers_match(&self, modifiers: egui::Modifiers) -> bool {
        let ctrl_match = if self.ctrl {
            modifiers.command || modifiers.ctrl
        } else {
            !modifiers.command && !modifiers.ctrl
        };

        let alt_match = if self.alt {
            modifiers.alt
        } else {
            !modifiers.alt
        };

        // Keys that only exist as shifted symbols on a US layout report
        // `shift = true` from the platform layer. Binding them without an
        // explicit Shift must still match.
        let shift_match = if self.shift {
            modifiers.shift
        } else {
            !modifiers.shift || shift_is_implicit(self.key)
        };

        ctrl_match && alt_match && shift_match
    }

    /// Formats this combination into a human-readable display string.
    pub fn display_string(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(key_name(self.key));
        parts.join("+")
    }

    /// Parses a string representation like `Ctrl+O` or `Space`.
    ///
    /// Splitting happens from the right so symbol keys that are themselves a
    /// plus sign (`Ctrl++`) round-trip correctly.
    pub fn parse_str(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        let (mods, key) = if let Some(rest) = s.strip_suffix("++") {
            (rest, key_from_name("+")?)
        } else if let Some(idx) = s.rfind('+') {
            let tail = s[idx + 1..].trim();
            let key = if tail.is_empty() {
                key_from_name("+")?
            } else {
                key_from_name(tail)?
            };
            (&s[..idx], key)
        } else {
            ("", key_from_name(s)?)
        };

        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        for token in mods.split('+').filter(|t| !t.trim().is_empty()) {
            match token.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" => ctrl = true,
                "alt" | "opt" | "option" => alt = true,
                "shift" => shift = true,
                _ => return None,
            }
        }

        Some(Self {
            key,
            ctrl,
            alt,
            shift,
        })
    }
}

impl fmt::Display for KeyCombination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_string())
    }
}

impl Serialize for KeyCombination {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.display_string())
    }
}

impl<'de> Deserialize<'de> for KeyCombination {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        KeyCombination::parse_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("Invalid key combination: {s}")))
    }
}

/// Returns a human-friendly string name for an `egui::Key`.
pub fn key_name(key: egui::Key) -> &'static str {
    use egui::Key as K;
    match key {
        K::Space => "Space",
        K::ArrowLeft => "Left",
        K::ArrowRight => "Right",
        K::ArrowUp => "Up",
        K::ArrowDown => "Down",
        K::Period => ".",
        K::Comma => ",",
        K::OpenBracket => "[",
        K::CloseBracket => "]",
        K::OpenCurlyBracket => "{",
        K::CloseCurlyBracket => "}",
        K::Backspace => "Backspace",
        K::Escape => "Esc",
        K::Enter => "Enter",
        K::Tab => "Tab",
        K::Delete => "Delete",
        K::Insert => "Insert",
        K::Home => "Home",
        K::End => "End",
        K::PageUp => "PageUp",
        K::PageDown => "PageDown",
        K::Minus => "−",
        K::Plus => "+",
        K::Equals => "=",
        K::Slash => "/",
        K::Backslash => "\\",
        K::Semicolon => ";",
        K::Colon => ":",
        K::Quote => "'",
        K::Pipe => "|",
        K::Questionmark => "?",
        K::Backtick => "`",
        K::F1 => "F1",
        K::F2 => "F2",
        K::F3 => "F3",
        K::F4 => "F4",
        K::F5 => "F5",
        K::F6 => "F6",
        K::F7 => "F7",
        K::F8 => "F8",
        K::F9 => "F9",
        K::F10 => "F10",
        K::F11 => "F11",
        K::F12 => "F12",
        K::A => "A",
        K::B => "B",
        K::C => "C",
        K::D => "D",
        K::E => "E",
        K::F => "F",
        K::G => "G",
        K::H => "H",
        K::I => "I",
        K::J => "J",
        K::K => "K",
        K::L => "L",
        K::M => "M",
        K::N => "N",
        K::O => "O",
        K::P => "P",
        K::Q => "Q",
        K::R => "R",
        K::S => "S",
        K::T => "T",
        K::U => "U",
        K::V => "V",
        K::W => "W",
        K::X => "X",
        K::Y => "Y",
        K::Z => "Z",
        K::Num0 => "0",
        K::Num1 => "1",
        K::Num2 => "2",
        K::Num3 => "3",
        K::Num4 => "4",
        K::Num5 => "5",
        K::Num6 => "6",
        K::Num7 => "7",
        K::Num8 => "8",
        K::Num9 => "9",
        _ => "Unknown",
    }
}

/// Parses an `egui::Key` from its string name.
pub fn key_from_name(s: &str) -> Option<egui::Key> {
    use egui::Key as K;
    match s.trim().to_ascii_lowercase().as_str() {
        "space" => Some(K::Space),
        "left" | "arrowleft" => Some(K::ArrowLeft),
        "right" | "arrowright" => Some(K::ArrowRight),
        "up" | "arrowup" => Some(K::ArrowUp),
        "down" | "arrowdown" => Some(K::ArrowDown),
        "." | "period" | "dot" => Some(K::Period),
        "," | "comma" => Some(K::Comma),
        "[" | "openbracket" => Some(K::OpenBracket),
        "]" | "closebracket" => Some(K::CloseBracket),
        "{" | "opencurlybracket" => Some(K::OpenCurlyBracket),
        "}" | "closecurlybracket" => Some(K::CloseCurlyBracket),
        "backspace" => Some(K::Backspace),
        "esc" | "escape" => Some(K::Escape),
        "enter" | "return" => Some(K::Enter),
        "tab" => Some(K::Tab),
        "delete" | "del" => Some(K::Delete),
        "insert" => Some(K::Insert),
        "home" => Some(K::Home),
        "end" => Some(K::End),
        "pageup" | "pgup" => Some(K::PageUp),
        "pagedown" | "pgdown" => Some(K::PageDown),
        "-" | "minus" | "−" => Some(K::Minus),
        "+" | "plus" => Some(K::Plus),
        "=" | "equals" => Some(K::Equals),
        "/" | "slash" => Some(K::Slash),
        "\\" | "backslash" => Some(K::Backslash),
        ";" | "semicolon" => Some(K::Semicolon),
        ":" | "colon" => Some(K::Colon),
        "'" | "quote" => Some(K::Quote),
        "|" | "pipe" => Some(K::Pipe),
        "?" | "questionmark" => Some(K::Questionmark),
        "`" | "backtick" | "grave" => Some(K::Backtick),
        "f1" => Some(K::F1),
        "f2" => Some(K::F2),
        "f3" => Some(K::F3),
        "f4" => Some(K::F4),
        "f5" => Some(K::F5),
        "f6" => Some(K::F6),
        "f7" => Some(K::F7),
        "f8" => Some(K::F8),
        "f9" => Some(K::F9),
        "f10" => Some(K::F10),
        "f11" => Some(K::F11),
        "f12" => Some(K::F12),
        "0" => Some(K::Num0),
        "1" => Some(K::Num1),
        "2" => Some(K::Num2),
        "3" => Some(K::Num3),
        "4" => Some(K::Num4),
        "5" => Some(K::Num5),
        "6" => Some(K::Num6),
        "7" => Some(K::Num7),
        "8" => Some(K::Num8),
        "9" => Some(K::Num9),
        other if other.len() == 1 => {
            let c = other.chars().next()?;
            letter_or_digit_key(c)
        }
        _ => None,
    }
}

/// Keys whose character requires Shift on a standard layout.
fn shift_is_implicit(key: egui::Key) -> bool {
    use egui::Key as K;
    matches!(
        key,
        K::Plus | K::Questionmark | K::Colon | K::Pipe | K::Exclamationmark
    )
}

fn letter_or_digit_key(c: char) -> Option<egui::Key> {
    use egui::Key as K;
    Some(match c.to_ascii_lowercase() {
        'a' => K::A,
        'b' => K::B,
        'c' => K::C,
        'd' => K::D,
        'e' => K::E,
        'f' => K::F,
        'g' => K::G,
        'h' => K::H,
        'i' => K::I,
        'j' => K::J,
        'k' => K::K,
        'l' => K::L,
        'm' => K::M,
        'n' => K::N,
        'o' => K::O,
        'p' => K::P,
        'q' => K::Q,
        'r' => K::R,
        's' => K::S,
        't' => K::T,
        'u' => K::U,
        'v' => K::V,
        'w' => K::W,
        'x' => K::X,
        'y' => K::Y,
        'z' => K::Z,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Built-in default catalogues
// ---------------------------------------------------------------------------

#[derive(Default)]
struct BindingSet {
    primary: BTreeMap<PlayerAction, KeyCombination>,
    alternate: BTreeMap<PlayerAction, KeyCombination>,
}

impl BindingSet {
    fn used(&self, combo: KeyCombination, ignore: PlayerAction) -> bool {
        self.primary
            .iter()
            .any(|(a, c)| *a != ignore && *c == combo)
            || self
                .alternate
                .iter()
                .any(|(a, c)| *a != ignore && *c == combo)
    }

    fn add(&mut self, action: PlayerAction, combo: KeyCombination) {
        self.primary.insert(action, combo);
    }

    fn add_alt(&mut self, action: PlayerAction, combo: KeyCombination) {
        if !self.used(combo, action) {
            self.alternate.insert(action, combo);
        }
    }

    /// Adds a binding only when the action and key are both free. Actions that
    /// already have a primary receive this combination as an alternative.
    fn try_add(&mut self, action: PlayerAction, combo: KeyCombination) -> bool {
        if self.used(combo, action) || self.primary.get(&action) == Some(&combo) {
            return false;
        }
        if self.primary.contains_key(&action) {
            if self.alternate.contains_key(&action) {
                return false;
            }
            self.alternate.insert(action, combo);
            return true;
        }
        self.primary.insert(action, combo);
        true
    }
}

fn combo(key: egui::Key) -> KeyCombination {
    KeyCombination::new(key)
}

fn classic_bindings() -> BindingSet {
    use egui::Key as K;
    use PlayerAction as A;
    let mut b = BindingSet::default();

    // Playback
    b.add(A::PlayPause, combo(K::Space));
    b.add(A::Stop, combo(K::S));
    b.add(A::NextMedia, combo(K::N));
    b.add(A::PreviousMedia, combo(K::P));
    b.add(A::ToggleRepeatMode, combo(K::L));
    b.add(A::ToggleAbLoop, combo(K::L).with_shift());
    b.add(A::FrameStepForward, combo(K::E));
    b.add(A::SpeedIncrease, combo(K::Plus));
    b.add(A::SpeedDecrease, combo(K::Minus));
    b.add(A::SpeedReset, combo(K::Equals));
    b.add_alt(A::SpeedIncrease, combo(K::CloseBracket));
    b.add_alt(A::SpeedDecrease, combo(K::OpenBracket));

    // Seeking: the VLC granularity ladder, anchored on the bare arrows.
    //
    // Bare Left/Right is the combination every VLC user reaches for first, and
    // leaving it unbound (as it was) is why "5-second seeking does not work".
    // The magnitudes are user-tunable, so the ladder is described by step
    // class rather than by a literal duration.
    b.add(A::SeekBackwardShort, combo(K::ArrowLeft));
    b.add(A::SeekForwardShort, combo(K::ArrowRight));
    b.add(A::SeekBackwardVeryShort, combo(K::ArrowLeft).with_shift());
    b.add(A::SeekForwardVeryShort, combo(K::ArrowRight).with_shift());
    b.add(A::SeekBackwardMedium, combo(K::ArrowLeft).with_alt());
    b.add(A::SeekForwardMedium, combo(K::ArrowRight).with_alt());
    b.add(A::SeekBackwardLong, combo(K::ArrowLeft).with_ctrl());
    b.add(A::SeekForwardLong, combo(K::ArrowRight).with_ctrl());
    b.add(A::SeekBackwardTenMinutes, combo(K::PageUp).with_shift());
    b.add(A::SeekForwardTenMinutes, combo(K::PageDown).with_shift());
    b.add(A::SeekToBeginning, combo(K::Home));

    // Volume: VLC's own Ctrl+Up/Down, with a coarse rung on Ctrl+Shift.
    //
    // Bare Up/Down is deliberately left unbound here. VLC does not bind it, and
    // claiming it silently shadowed mpv's 60-second seek: in a profile that is
    // described as mpv-inspired, Up moved the volume instead of the
    // timeline. Each profile now keeps its own semantics.
    b.add(A::VolumeUp, combo(K::ArrowUp).with_ctrl());
    b.add(A::VolumeDown, combo(K::ArrowDown).with_ctrl());
    b.add(A::VolumeUpLarge, combo(K::ArrowUp).with_ctrl().with_shift());
    b.add(
        A::VolumeDownLarge,
        combo(K::ArrowDown).with_ctrl().with_shift(),
    );
    b.add(A::ToggleMute, combo(K::M));
    b.add(A::Unmute, combo(K::M).with_ctrl());
    b.add(A::RestoreVolume, combo(K::M).with_alt());

    // Audio
    b.add(A::CycleAudioTrack, combo(K::B));
    b.add(A::CycleAudioDevice, combo(K::A).with_shift());
    b.add(A::AudioDelayIncrease, combo(K::K));
    b.add(A::AudioDelayDecrease, combo(K::J));

    // Subtitles
    b.add(A::CycleSubtitleTrack, combo(K::V));
    b.add(A::CycleSubtitleTrackReverse, combo(K::V).with_alt());
    b.add(A::ToggleSubtitles, combo(K::V).with_shift());
    b.add(A::CycleSecondarySubtitle, combo(K::V).with_ctrl());
    b.add(A::SubtitleDelayIncrease, combo(K::H));
    b.add(A::SubtitleDelayDecrease, combo(K::G));
    b.add(A::AddSubtitleFile, combo(K::S).with_ctrl().with_alt());

    // Video
    b.add(A::CycleAspectRatio, combo(K::A));
    b.add(A::CycleCropPreset, combo(K::C));
    b.add(A::CycleScalingMode, combo(K::O));
    b.add(A::ToggleDeinterlace, combo(K::D));
    b.add(A::ZoomIn, combo(K::Z));
    b.add(A::ZoomOut, combo(K::Z).with_shift());
    b.add(A::Screenshot, combo(K::S).with_shift());

    // Playlist
    b.add(A::ToggleShuffle, combo(K::R));
    b.add(A::NextChapter, combo(K::PageDown).with_ctrl());
    b.add(A::PreviousChapter, combo(K::PageUp).with_ctrl());
    b.add(A::AddBookmark, combo(K::B).with_ctrl().with_shift());
    b.add(A::ClearPlaylist, combo(K::W).with_ctrl().with_shift());

    // Navigation / window
    b.add(A::ToggleFullscreen, combo(K::F));
    b.add_alt(A::ToggleFullscreen, combo(K::F11));
    b.add(A::ExitFullscreen, combo(K::Escape));
    b.add(A::ToggleCinema, combo(K::Enter).with_shift());
    b.add(A::ToggleControls, combo(K::I));
    b.add(A::ToggleStatistics, combo(K::I).with_ctrl());
    b.add(A::ExportDiagnostics, combo(K::I).with_ctrl().with_shift());
    // VLC's `T`: print the position, do not change it.
    b.add(A::ShowPlaybackPosition, combo(K::T));
    b.add(A::ToggleAlwaysOnTop, combo(K::T).with_ctrl());
    b.add(A::ToggleMiniPlayer, combo(K::P).with_alt());
    b.add(A::TogglePlaylistDrawer, combo(K::L).with_ctrl());
    b.add(A::ToggleBrowserView, combo(K::B).with_ctrl());
    b.add(A::OpenFile, combo(K::O).with_ctrl());

    // Advanced
    b.add(A::ShowShortcutHelp, combo(K::F1));
    b.add_alt(A::ShowShortcutHelp, combo(K::Questionmark));
    b.add(A::ReloadMedia, combo(K::R).with_ctrl());
    b
}

fn advanced_bindings() -> BindingSet {
    use egui::Key as K;
    use PlayerAction as A;
    let mut b = BindingSet::default();

    // Playback
    b.add(A::PlayPause, combo(K::Space));
    b.add_alt(A::PlayPause, combo(K::P));
    b.add(A::FrameStepForward, combo(K::Period));
    b.add(A::FrameStepBack, combo(K::Comma));
    b.add(A::SpeedDecrease, combo(K::OpenBracket));
    b.add(A::SpeedIncrease, combo(K::CloseBracket));
    b.add(A::SpeedHalf, combo(K::OpenCurlyBracket));
    b.add(A::SpeedDouble, combo(K::CloseCurlyBracket));
    b.add(A::SpeedReset, combo(K::Backspace));
    b.add(A::ToggleAbLoop, combo(K::L));

    // Seeking
    b.add(A::SeekBackwardShort, combo(K::ArrowLeft));
    b.add(A::SeekForwardShort, combo(K::ArrowRight));
    b.add(A::SeekBackwardLong, combo(K::ArrowDown));
    b.add(A::SeekForwardLong, combo(K::ArrowUp));
    b.add(A::SeekBackwardVeryShort, combo(K::ArrowLeft).with_shift());
    b.add(A::SeekForwardVeryShort, combo(K::ArrowRight).with_shift());
    // Upstream: `Shift+UP/DOWN no-osd seek ±5 exact`. The same 5-second rung as
    // the bare arrows, but exact, so it is an alternate of the same action.
    b.add_alt(A::SeekForwardShort, combo(K::ArrowUp).with_shift());
    b.add_alt(A::SeekBackwardShort, combo(K::ArrowDown).with_shift());
    // Upstream: `Shift+PGUP seek 600`, `Shift+PGDWN seek -600`.
    b.add(A::SeekForwardTenMinutes, combo(K::PageUp).with_shift());
    b.add(A::SeekBackwardTenMinutes, combo(K::PageDown).with_shift());
    b.add(A::SeekToBeginning, combo(K::Home));
    b.add(A::ShowPlaybackPosition, combo(K::O));
    b.add_alt(A::ShowPlaybackPosition, combo(K::P).with_shift());

    // Volume (mpv 9/0, with slash as the mpv-style alternate). Ctrl+Up/Down
    // is added as a second, familiar route because mpv reserves the bare
    // arrows for 60-second seeking; without it the volume keys look broken to
    // anyone who did not memorise mpv's number keys.
    b.add(A::VolumeDown, combo(K::Num9));
    b.add(A::VolumeUp, combo(K::Num0));
    b.add_alt(A::VolumeDown, combo(K::Slash));
    b.add(A::VolumeUpSmall, combo(K::ArrowUp).with_ctrl());
    b.add(A::VolumeDownSmall, combo(K::ArrowDown).with_ctrl());
    b.add(A::ToggleMute, combo(K::M));

    // Audio
    b.add(A::CycleAudioTrack, combo(K::A).with_shift());
    b.add(A::AudioDelayIncrease, combo(K::Plus).with_ctrl());
    b.add(A::AudioDelayDecrease, combo(K::Minus).with_ctrl());

    // Subtitles
    b.add(A::ToggleSubtitles, combo(K::V));
    b.add(A::CycleSubtitleTrack, combo(K::J));
    b.add(A::CycleSubtitleTrackReverse, combo(K::J).with_shift());
    b.add(A::CycleSecondarySubtitle, combo(K::V).with_alt());
    b.add(A::SubtitleDelayIncrease, combo(K::Z));
    b.add(A::SubtitleDelayDecrease, combo(K::Z).with_shift());
    // Upstream: `r add sub-pos -1` (up), `R add sub-pos +1` (down).
    b.add(A::SubtitleMoveUp, combo(K::R));
    b.add(A::SubtitleMoveDown, combo(K::R).with_shift());
    // Upstream: `G add sub-scale 0.1` (larger), `F add sub-scale -0.1`
    // (smaller). Uppercase F is the shift-modified form; lowercase f is
    // fullscreen, so the two never collide.
    b.add(A::SubtitleFontIncrease, combo(K::G));
    b.add(A::SubtitleFontDecrease, combo(K::F).with_shift());
    b.add(A::SubtitleFontReset, combo(K::G).with_ctrl());

    // Video
    b.add(A::CycleAspectRatio, combo(K::A));
    b.add(A::ToggleDeband, combo(K::B));
    b.add(A::ToggleDeinterlace, combo(K::D));
    b.add(A::CycleScalingMode, combo(K::W));
    b.add(A::PanLeft, combo(K::ArrowLeft).with_alt());
    b.add(A::PanRight, combo(K::ArrowRight).with_alt());
    b.add(A::PanUp, combo(K::ArrowUp).with_alt());
    b.add(A::PanDown, combo(K::ArrowDown).with_alt());
    b.add(A::ZoomIn, combo(K::Plus).with_alt());
    b.add(A::ZoomOut, combo(K::Minus).with_alt());
    b.add(A::ResetVideoTransform, combo(K::Backspace).with_alt());
    b.add(A::ToggleHardwareDecoding, combo(K::H).with_ctrl());
    b.add(A::Screenshot, combo(K::S));
    b.add(A::ScreenshotWithoutSubtitles, combo(K::S).with_shift());

    // Playlist / chapters
    b.add(A::NextMedia, combo(K::Enter));
    // Upstream: `PGUP add chapter 1` advances, `PGDWN add chapter -1` goes back.
    b.add(A::NextChapter, combo(K::PageUp));
    b.add(A::PreviousChapter, combo(K::PageDown));
    b.add(A::TogglePlaylistDrawer, combo(K::F8));

    // Navigation / window
    b.add(A::ToggleFullscreen, combo(K::F));
    b.add(A::ExitFullscreen, combo(K::Escape));
    b.add(A::ToggleControls, combo(K::Delete));
    b.add(A::ToggleStatistics, combo(K::I));
    b.add(A::ToggleAlwaysOnTop, combo(K::T));
    b.add(A::OpenFile, combo(K::O).with_ctrl());

    // Advanced
    b.add(A::ShowShortcutHelp, combo(K::Questionmark));
    b.add_alt(A::ShowShortcutHelp, combo(K::F1));
    b.add(A::ReloadMedia, combo(K::R).with_ctrl());
    b
}

fn hybrid_bindings() -> BindingSet {
    use egui::Key as K;
    use PlayerAction as A;
    // VLC compatibility is the priority: start with the Classic table.
    let mut b = classic_bindings();

    // mpv-only actions receive free keys or proposed alternatives.
    b.try_add(A::SeekBackwardShort, combo(K::ArrowLeft));
    b.try_add(A::SeekForwardShort, combo(K::ArrowRight));
    b.try_add(A::SeekBackwardLong, combo(K::ArrowDown));
    b.try_add(A::SeekForwardLong, combo(K::ArrowUp));
    b.try_add(A::FrameStepBack, combo(K::Comma));
    b.try_add(A::SpeedHalf, combo(K::OpenCurlyBracket));
    b.try_add(A::SpeedDouble, combo(K::CloseCurlyBracket));
    b.try_add(A::ToggleDeband, combo(K::B).with_ctrl().with_alt());
    b.try_add(A::CycleAudioTrack, combo(K::A).with_ctrl());
    b.try_add(A::CycleSubtitleTrack, combo(K::J).with_ctrl());
    b.try_add(A::ToggleHardwareDecoding, combo(K::H).with_ctrl());
    b.try_add(A::ZoomIn, combo(K::Plus).with_alt());
    b.try_add(A::ZoomOut, combo(K::Minus).with_alt());
    b.try_add(A::ResetVideoTransform, combo(K::Backspace).with_alt());
    b.try_add(A::PanLeft, combo(K::ArrowLeft).with_alt());
    b.try_add(A::PanRight, combo(K::ArrowRight).with_alt());
    b.try_add(A::PanUp, combo(K::ArrowUp).with_alt());
    b.try_add(A::PanDown, combo(K::ArrowDown).with_alt());
    b.try_add(A::ToggleStatistics, combo(K::I).with_ctrl());
    b.try_add(A::ToggleCinema, combo(K::Enter).with_shift());
    b.try_add(A::ExportDiagnostics, combo(K::I).with_ctrl().with_shift());
    b.try_add(A::ToggleControls, combo(K::Delete));
    b.try_add(A::SeekForwardShort, combo(K::ArrowUp).with_shift());
    b.try_add(A::SeekBackwardShort, combo(K::ArrowDown).with_shift());
    b.try_add(A::NextChapter, combo(K::PageUp));
    b.try_add(A::PreviousChapter, combo(K::PageDown));
    b.try_add(A::ShowPlaybackPosition, combo(K::O));
    b.try_add(A::SubtitleMoveUp, combo(K::R));
    b.try_add(A::SubtitleMoveDown, combo(K::R).with_shift());
    b.try_add(A::TogglePlaylistDrawer, combo(K::F8));
    b.add_alt(A::NextMedia, combo(K::Enter));
    b.add_alt(A::VolumeDown, combo(K::Num9));
    b.add_alt(A::VolumeUp, combo(K::Num0));
    b.add_alt(A::ShowShortcutHelp, combo(K::Questionmark));
    b.add_alt(A::ToggleFullscreen, combo(K::F11));
    b
}

fn legacy_bindings() -> BindingSet {
    use egui::Key as K;
    use PlayerAction as A;
    let mut b = BindingSet::default();

    b.add(A::PlayPause, combo(K::Space));
    b.add_alt(A::PlayPause, combo(K::Enter));
    b.add(A::SeekBackwardShort, combo(K::ArrowLeft));
    b.add(A::SeekForwardShort, combo(K::ArrowRight));
    b.add(A::VolumeUp, combo(K::ArrowUp));
    b.add(A::VolumeDown, combo(K::ArrowDown));
    b.add(A::FrameStepForward, combo(K::Period));
    b.add(A::FrameStepBack, combo(K::Comma));
    b.add(A::ToggleAbLoop, combo(K::L));
    b.add(A::ToggleMute, combo(K::M));
    b.add(A::ToggleFullscreen, combo(K::F));
    b.add_alt(A::ToggleFullscreen, combo(K::F11));
    b.add(A::ExitFullscreen, combo(K::Escape));
    b.add(A::ToggleCinema, combo(K::Enter).with_shift());
    b.add(A::ToggleControls, combo(K::H));
    b.add(A::ToggleStatistics, combo(K::M).with_alt());
    b.add(A::ExportDiagnostics, combo(K::I).with_ctrl().with_shift());
    b.add(A::TogglePlaylistDrawer, combo(K::P));
    b.add(A::ToggleBrowserView, combo(K::B));
    b.add(A::ToggleMiniPlayer, combo(K::I));
    b.add(A::SpeedDecrease, combo(K::OpenBracket));
    b.add(A::SpeedIncrease, combo(K::CloseBracket));
    b.add(A::SpeedReset, combo(K::Backspace));
    b.add(A::OpenFile, combo(K::O).with_ctrl());
    b.add(A::ShowShortcutHelp, combo(K::F1));
    b.add_alt(A::ShowShortcutHelp, combo(K::Questionmark));
    b
}

type BindingTables = (
    BTreeMap<PlayerAction, KeyCombination>,
    BTreeMap<PlayerAction, KeyCombination>,
);

fn defaults_for(profile: ShortcutProfile) -> BindingTables {
    let set = match profile {
        ShortcutProfile::Classic => classic_bindings(),
        ShortcutProfile::Advanced => advanced_bindings(),
        ShortcutProfile::Hybrid => hybrid_bindings(),
        ShortcutProfile::Legacy => legacy_bindings(),
        // A custom table starts from the shipped defaults; every edit is then
        // recorded as an override for this profile only.
        ShortcutProfile::Custom => classic_bindings(),
    };
    (set.primary, set.alternate)
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// User-tunable magnitudes used by the continuous shortcut actions.
///
/// These live next to the bindings so that one JSON document captures both
/// halves of the keyboard configuration, and so that changing a value takes
/// effect on the very next key press.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShortcutTuning {
    /// Distance moved by the "small" seek actions, in seconds.
    pub seek_small_secs: f64,
    /// Distance moved by the "medium" seek actions, in seconds.
    pub seek_medium_secs: f64,
    /// Distance moved by the "large" seek actions, in seconds.
    pub seek_large_secs: f64,
    /// Percentage points added or removed by the volume actions.
    pub volume_step: f64,
    /// Playback-rate change applied by the speed actions.
    pub speed_step: f64,
    /// Milliseconds added or removed by the audio-delay actions.
    pub audio_delay_step_ms: f64,
    /// Milliseconds added or removed by the subtitle-delay actions.
    pub subtitle_delay_step_ms: f64,
}

impl Default for ShortcutTuning {
    fn default() -> Self {
        Self {
            seek_small_secs: 5.0,
            seek_medium_secs: 10.0,
            seek_large_secs: 60.0,
            volume_step: 5.0,
            speed_step: 0.25,
            audio_delay_step_ms: 50.0,
            subtitle_delay_step_ms: 50.0,
        }
    }
}

impl ShortcutTuning {
    /// Clamps every value to a range the engine can actually honour.
    ///
    /// A zero or negative volume step would make the volume keys do nothing,
    /// and an unbounded seek could jump past the end of the file, so both are
    /// clamped here rather than at each call site.
    pub fn sanitized(self) -> Self {
        Self {
            seek_small_secs: self.seek_small_secs.clamp(0.5, 600.0),
            seek_medium_secs: self.seek_medium_secs.clamp(0.5, 1800.0),
            seek_large_secs: self.seek_large_secs.clamp(1.0, 3600.0),
            volume_step: self.volume_step.clamp(0.5, 25.0),
            speed_step: self.speed_step.clamp(0.05, 1.0),
            audio_delay_step_ms: self.audio_delay_step_ms.clamp(1.0, 1000.0),
            subtitle_delay_step_ms: self.subtitle_delay_step_ms.clamp(1.0, 1000.0),
        }
    }
}

/// Configurable keybinding registry with profiles, conflict detection,
/// per-profile overrides, JSON persistence and import/export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyBindingMap {
    #[serde(default)]
    pub profile: ShortcutProfile,
    pub bindings: BTreeMap<PlayerAction, KeyCombination>,
    #[serde(default)]
    pub alternates: BTreeMap<PlayerAction, KeyCombination>,
    /// Per-profile user edits layered over the profile defaults.
    #[serde(default)]
    pub overrides: BTreeMap<ShortcutProfile, BTreeMap<PlayerAction, KeyCombination>>,
    /// Actions the user switched off without deleting their assignment.
    #[serde(default)]
    pub disabled: BTreeSet<PlayerAction>,
    /// Magnitudes used by seeking, volume, speed and delay actions.
    #[serde(default)]
    pub tuning: ShortcutTuning,
}

impl Default for KeyBindingMap {
    fn default() -> Self {
        Self::for_profile(ShortcutProfile::Classic)
    }
}

impl KeyBindingMap {
    pub fn for_profile(profile: ShortcutProfile) -> Self {
        let (bindings, alternates) = defaults_for(profile);
        Self {
            profile,
            bindings,
            alternates,
            overrides: BTreeMap::new(),
            disabled: BTreeSet::new(),
            tuning: ShortcutTuning::default(),
        }
    }

    /// Switches profile, preserving edits made in the previous profile.
    pub fn set_profile(&mut self, profile: ShortcutProfile) {
        if self.profile == profile {
            return;
        }
        let (mut bindings, mut alternates) = defaults_for(profile);
        if let Some(edits) = self.overrides.get(&profile) {
            for (action, combo) in edits {
                bindings.insert(*action, *combo);
            }
            // A customised primary may collide with a default alternate;
            // drop the stale alternate rather than silently double-fire.
            alternates.retain(|action, combo| {
                let taken = bindings.iter().any(|(a, c)| a != action && c == combo);
                !taken
            });
        }
        self.profile = profile;
        self.bindings = bindings;
        self.alternates = alternates;
    }

    /// Checks whether an action's bound combination was triggered this frame.
    ///
    /// Disabled actions never match, and continuous actions (seeking, volume,
    /// zoom, delays, speed) accept auto-repeat while discrete ones do not.
    pub fn matches(&self, action: PlayerAction, input: &egui::InputState) -> bool {
        self.triggered_combo(action, input).is_some()
    }

    /// Returns the combination that fired for `action` this frame, if any.
    pub fn triggered_combo(
        &self,
        action: PlayerAction,
        input: &egui::InputState,
    ) -> Option<KeyCombination> {
        if self.disabled.contains(&action) {
            return None;
        }
        let allow_repeat = action.allows_key_repeat();
        if let Some(combo) = self.bindings.get(&action) {
            if combo.matches_with_repeat(input, allow_repeat) {
                return Some(*combo);
            }
        }
        if let Some(alt) = self.alternates.get(&action) {
            if alt.matches_with_repeat(input, allow_repeat) {
                return Some(*alt);
            }
        }
        None
    }

    /// Resolves every action triggered this frame, de-duplicating by
    /// combination so a stray conflict can never fire two actions.
    pub fn pressed_actions(
        &self,
        actions: &[PlayerAction],
        input: &egui::InputState,
    ) -> Vec<PlayerAction> {
        let mut fired: Vec<PlayerAction> = Vec::new();
        let mut seen: Vec<KeyCombination> = Vec::new();
        for &action in actions {
            let Some(combo) = self.triggered_combo(action, input) else {
                continue;
            };
            if seen.contains(&combo) {
                continue;
            }
            seen.push(combo);
            fired.push(action);
        }
        fired
    }

    /// True when the user switched this action off in the shortcut manager.
    pub fn is_enabled(&self, action: PlayerAction) -> bool {
        !self.disabled.contains(&action)
    }

    /// Enables or disables an action without touching its assignment.
    pub fn set_enabled(&mut self, action: PlayerAction, enabled: bool) {
        if enabled {
            self.disabled.remove(&action);
        } else {
            self.disabled.insert(action);
        }
    }

    pub fn get(&self, action: PlayerAction) -> Option<&KeyCombination> {
        self.bindings.get(&action)
    }

    pub fn get_alternate(&self, action: PlayerAction) -> Option<&KeyCombination> {
        self.alternates.get(&action)
    }

    /// Human-readable shortcut hint for tooltips (`""` when unbound).
    pub fn hint(&self, action: PlayerAction) -> String {
        self.get(action)
            .map(|c| c.display_string())
            .unwrap_or_default()
    }

    /// Returns every action/slot currently using `combination`.
    pub fn conflicting_slots(
        &self,
        action: PlayerAction,
        combination: KeyCombination,
    ) -> Vec<(PlayerAction, BindingSlot)> {
        let mut out = Vec::new();
        for (other, combo) in &self.bindings {
            if *other != action && *combo == combination {
                out.push((*other, BindingSlot::Primary));
            }
        }
        for (other, combo) in &self.alternates {
            if *other != action && *combo == combination {
                out.push((*other, BindingSlot::Alternate));
            }
        }
        out
    }

    /// Attempts to bind a combination, reporting conflicts instead of
    /// silently overwriting another action.
    pub fn try_bind(&self, action: PlayerAction, combination: KeyCombination) -> BindOutcome {
        let conflicts = self.conflicting_slots(action, combination);
        if conflicts.is_empty() {
            BindOutcome::Bound
        } else {
            BindOutcome::Conflict(conflicts)
        }
    }

    /// Binds a combination, clearing any conflicting assignment.
    pub fn bind_replace(&mut self, action: PlayerAction, combination: KeyCombination) {
        let conflicts = self.conflicting_slots(action, combination);
        for (other, slot) in conflicts {
            match slot {
                BindingSlot::Primary => {
                    self.bindings.remove(&other);
                }
                BindingSlot::Alternate => {
                    self.alternates.remove(&other);
                }
            }
        }
        self.set_binding(action, combination);
    }

    /// Moves the conflicting action onto this action's previous combination.
    pub fn bind_swap(&mut self, action: PlayerAction, combination: KeyCombination) {
        let previous = self.bindings.get(&action).copied();
        let conflicts = self.conflicting_slots(action, combination);
        if let (Some(previous), Some((other, slot))) = (previous, conflicts.first().copied()) {
            match slot {
                BindingSlot::Primary => {
                    self.bindings.insert(other, previous);
                }
                BindingSlot::Alternate => {
                    self.alternates.insert(other, previous);
                }
            }
        }
        self.bind_replace(action, combination);
    }

    /// True when this action's primary binding was customised in this profile.
    pub fn is_custom(&self, action: PlayerAction) -> bool {
        self.overrides
            .get(&self.profile)
            .is_some_and(|edits| edits.contains_key(&action))
    }

    /// Records a primary binding as a per-profile override.
    pub fn set_binding(&mut self, action: PlayerAction, combination: KeyCombination) {
        self.bindings.insert(action, combination);
        self.alternates.remove(&action);
        self.overrides
            .entry(self.profile)
            .or_default()
            .insert(action, combination);
    }

    pub fn reset_to_defaults(&mut self) {
        self.overrides.clear();
        *self = Self::for_profile(self.profile);
    }

    pub fn reset_action(&mut self, action: PlayerAction) {
        let (defaults, alternates) = defaults_for(self.profile);
        if let Some(combo) = defaults.get(&action) {
            self.bindings.insert(action, *combo);
        } else {
            self.bindings.remove(&action);
        }
        if let Some(alt) = alternates.get(&action) {
            self.alternates.insert(action, *alt);
        } else {
            self.alternates.remove(&action);
        }
        if let Some(edits) = self.overrides.get_mut(&self.profile) {
            edits.remove(&action);
        }
    }

    /// Identifies all conflicting bindings across primary and alternate slots.
    pub fn find_conflicts(&self) -> Vec<KeyConflict> {
        let mut list: Vec<(PlayerAction, KeyCombination, BindingSlot)> = Vec::new();
        list.extend(
            self.bindings
                .iter()
                .map(|(a, c)| (*a, *c, BindingSlot::Primary)),
        );
        list.extend(
            self.alternates
                .iter()
                .map(|(a, c)| (*a, *c, BindingSlot::Alternate)),
        );

        let mut conflicts = Vec::new();
        for i in 0..list.len() {
            for j in (i + 1)..list.len() {
                let (action_a, combo_a, slot_a) = list[i];
                let (action_b, combo_b, slot_b) = list[j];
                if combo_a == combo_b {
                    conflicts.push(KeyConflict {
                        action_a,
                        slot_a,
                        action_b,
                        slot_b,
                        combination: combo_a,
                    });
                }
            }
        }
        conflicts
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json_str)
    }

    pub fn config_path() -> std::path::PathBuf {
        crate::config_editor::config_dir().join("shortcuts.json")
    }

    pub fn load_from_disk() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(map) = Self::from_json(&content) {
                    return map;
                }
            }
        }
        Self::default()
    }

    pub fn save_to_disk(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = self
            .to_json()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Exports the active profile + bindings to an arbitrary JSON file.
    pub fn export_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        let json = self
            .to_json()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Imports a JSON configuration. Invalid files leave the registry untouched.
    pub fn import_from(&mut self, path: &std::path::Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        let imported = Self::from_json(&content)
            .map_err(|e| format!("Invalid shortcut configuration: {e}"))?;
        *self = imported;
        Ok(())
    }
}

impl MuvrixApp {
    /// Dispatches configured shortcuts for this frame.
    ///
    /// Text entry always wins: when an editable widget has keyboard focus the
    /// registry is skipped entirely, so typing in a search box or file field
    /// is never intercepted.
    pub(crate) fn handle_keyboard_inputs(&mut self, ctx: &egui::Context) {
        // Shortcut recording mode owns the keyboard until a key or Esc arrives.
        if let Some(target_action) = self.recording_shortcut {
            let mut captured = None;
            ctx.input(|i| {
                for event in &i.events {
                    if let egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } = event
                    {
                        if *key == egui::Key::Escape {
                            captured = Some(None);
                        } else {
                            captured = Some(Some(KeyCombination {
                                key: *key,
                                ctrl: modifiers.command || modifiers.ctrl,
                                alt: modifiers.alt,
                                shift: modifiers.shift,
                            }));
                        }
                        break;
                    }
                }
            });

            if let Some(res) = captured {
                if let Some(combo) = res {
                    match self.keybindings.try_bind(target_action, combo) {
                        BindOutcome::Bound => {
                            self.keybindings.set_binding(target_action, combo);
                            let _ = self.keybindings.save_to_disk();
                            self.pending_shortcut_conflict = None;
                        }
                        BindOutcome::Conflict(conflicts) => {
                            self.pending_shortcut_conflict = Some(PendingShortcutConflict {
                                action: target_action,
                                combination: combo,
                                conflicts,
                            });
                        }
                    }
                }
                self.recording_shortcut = None;
                return;
            }
            return;
        }

        // Escape dismisses transient UI before it reaches the binding table.
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            let dismissed = self.dismiss_transient_ui();
            if dismissed {
                return;
            }
        }

        // Text entry always wins, but only while a text widget really owns the
        // keyboard. `Context::wants_keyboard_input` is `focused().is_some()`:
        // it is true for *any* focused widget, and egui keeps that focus when
        // the user clicks a non-focusable widget or leaves the view that owned
        // the focused field. Relying on it naively is what made every shortcut
        // stop responding for the rest of the session after one click in a
        // search box. Stale focus is dropped first, so the check below only
        // suppresses shortcuts while the user is genuinely typing.
        self.release_stale_keyboard_focus(ctx);
        if self.text_entry_owns_keyboard(ctx) {
            return;
        }

        let actions = self
            .keybindings
            .pressed_actions(PlayerAction::ALL, &ctx.input(|i| i.clone()));

        for action in actions {
            self.execute_player_action(action, ctx);
        }
    }

    /// Test-only: injects scheduled key presses as genuine `egui::Event::Key`
    /// events, then reports what the engine actually did.
    ///
    /// This exists because "the binding appears in Settings" is not evidence
    /// that a key works. Presses go through the same router, the same action
    /// layer and the same libmpv controller calls as a physical key, so a log
    /// line that says `CHANGED` is end-to-end proof for the pipeline.
    ///
    /// Inert unless `MUVRIX_TEST_KEYS` is set, e.g.
    /// `MUVRIX_TEST_KEYS="2:Space,4:ArrowRight,6:Up,8:Shift+Up"`.
    pub(crate) fn drive_key_script(&mut self, ctx: &egui::Context) {
        if let Some((spec, before)) = self.test_keys_pending.take() {
            let after = self.key_state_probe();
            log::warn!(
                "TEST-ONLY key '{spec}' {} — before[{before}] after[{after}]",
                if before == after {
                    "UNCHANGED"
                } else {
                    "CHANGED"
                }
            );
        }

        let Some(script) = self.test_keys.clone() else {
            return;
        };
        let elapsed = self.session_start.elapsed().as_secs_f64();
        while self.test_keys_next < script.len() {
            let (due, combo) = script[self.test_keys_next];
            if elapsed < due {
                break;
            }
            let before = self.key_state_probe();
            let modifiers = egui::Modifiers {
                ctrl: combo.ctrl,
                alt: combo.alt,
                shift: combo.shift,
                mac_cmd: false,
                command: combo.ctrl,
            };
            ctx.input_mut(|input| {
                input.events.push(egui::Event::Key {
                    key: combo.key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                });
                input.events.push(egui::Event::Key {
                    key: combo.key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers,
                });
            });
            log::warn!("TEST-ONLY key inject: {}", combo.display_string());
            self.test_keys_pending = Some((combo.display_string(), before));
            self.test_keys_next += 1;
            ctx.request_repaint();
        }
    }

    /// Compact snapshot of the engine state a shortcut is supposed to change.
    ///
    /// Every value is read back from libmpv through the controller, so a
    /// shortcut that only updates a local variable would show up as UNCHANGED
    /// here rather than passing silently.
    fn key_state_probe(&self) -> String {
        let (pan_x, pan_y) = self.controller.video_pan();
        let styling = self.controller.subtitle_styling();
        let audio = self
            .controller
            .selected_track(TrackType::Audio)
            .map(|track| track.id);
        let subtitle = self
            .controller
            .selected_track(TrackType::Subtitle)
            .map(|track| track.id);
        let audio_count = self.controller.tracks_by_type(TrackType::Audio).len();
        let subtitle_count = self.controller.tracks_by_type(TrackType::Subtitle).len();
        let chapter = self.controller.current_chapter();
        let chapter_count = self.controller.chapter_count();
        format!(
            "{:?} vol={:.0} muted={} pos={:.3} speed={:.2} aspect={} zoom={:.2} pan=({pan_x:.2},{pan_y:.2}) crop={} scale={:?} sub_delay={:.2} sub_vis={} font={} sub_pos={} audio_delay={:?} overlay={} fullscreen={} audio={audio:?}/{audio_count} sub={subtitle:?}/{subtitle_count} chapter={chapter:?}/{chapter_count} deint={} deband={} sp={:.2} ontop={} help={} dur={:.1} file={:?} ab={:?} osd={:?}",
            self.controller.state(),
            self.controller.volume(),
            self.controller.is_muted(),
            self.controller.time_pos(),
            self.controller.speed(),
            self.controller.aspect_override(),
            self.controller.video_zoom(),
            self.controller.video_crop(),
            self.controller.scaling_mode(),
            self.controller.sub_delay(),
            self.controller.sub_visibility(),
            styling.font_size,
            styling.pos,
            self.controller.audio_delay(),
            self.ui_overlay_visible,
            self.is_fullscreen,
            self.controller.deinterlace(),
            self.controller.deband_enabled(),
            self.controller.speed(),
            self.always_on_top,
            self.show_shortcuts,
            self.controller.duration(),
            self.controller
                .current_file()
                .and_then(|f| std::path::Path::new(f).file_name())
                .and_then(|n| n.to_str()),
            self.controller.ab_loop_state(),
            self.osd_notice
                .as_ref()
                .map(|(text, _)| text.as_str()),
        )
    }

    /// Drops keyboard focus that egui left behind on a widget the user is no
    /// longer interacting with.
    ///
    /// Two cases matter for shortcut reliability: a click that lands outside
    /// the focused widget, and a focus that belongs to a view which is no
    /// longer on screen. In both the focused widget can never surrender the
    /// focus itself, so the shortcut router would stay disabled forever.
    fn release_stale_keyboard_focus(&mut self, ctx: &egui::Context) {
        let view_changed = self
            .last_shortcut_view
            .is_some_and(|previous| previous != self.current_view);
        self.last_shortcut_view = Some(self.current_view);

        let Some(focused) = ctx.memory(|memory| memory.focused()) else {
            return;
        };
        if view_changed {
            ctx.memory_mut(|memory| memory.stop_text_input());
            return;
        }

        let press = ctx.input(|i| {
            if i.pointer.any_pressed() {
                i.pointer.interact_pos()
            } else {
                None
            }
        });
        let Some(press) = press else {
            return;
        };

        // Keep focus when the press was inside the focused widget (clicking
        // into a text field must keep working). A widget that is not drawn
        // this frame has no response at all, which means the focus is stale.
        let inside = ctx
            .read_response(focused)
            .is_some_and(|response| response.rect.expand(2.0).contains(press));
        if !inside {
            ctx.memory_mut(|memory| memory.stop_text_input());
        }
    }

    /// True when a text-entry surface currently owns the keyboard.
    fn text_entry_owns_keyboard(&self, ctx: &egui::Context) -> bool {
        // The URL prompt is opened from a shortcut and does not necessarily
        // have focus on its very first frame, so it is guarded explicitly.
        // Every other text surface is covered by the focus check below.
        if self.player_empty_url_input_open {
            return true;
        }
        ctx.wants_keyboard_input()
    }

    fn dismiss_transient_ui(&mut self) -> bool {
        if self.pending_shortcut_conflict.is_some() {
            self.pending_shortcut_conflict = None;
            return true;
        }
        if self.recording_shortcut.is_some() {
            self.recording_shortcut = None;
            return true;
        }
        if self.player_view_menu_open {
            self.player_view_menu_open = false;
            return true;
        }
        if self.player_more_overflow_open {
            self.player_more_overflow_open = false;
            return true;
        }
        if self.player_empty_url_input_open {
            self.player_empty_url_input_open = false;
            return true;
        }
        if self.show_shortcuts {
            self.show_shortcuts = false;
            return true;
        }
        false
    }

    /// Drives the single overlay-visibility state machine for the player HUD.
    ///
    /// One target state is computed per frame from the *actual* HUD rectangles
    /// captured during the previous frame, not from guessed screen bands, then
    /// a short fade is applied. That removes the old failure modes: controls
    /// vanishing while the pointer sat on them, popovers auto-hiding, and the
    /// window resizing during hover transitions.
    pub(crate) fn update_osd_autohide(&mut self, ctx: &egui::Context) {
        let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.1);
        let on_player = self.current_view == AppView::Player;

        let modal_active = self.show_shortcuts
            || self.show_playback_stats
            || self.sync_panel_open
            || self.show_playlist_drawer
            || self.add_root_dialog_open
            || self.pending_resume_prompt.is_some()
            || self.crash_notice.is_some()
            || self.player_more_overflow_open
            || self.player_view_menu_open
            || self.shortcut_help_open();

        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let pointer_active = ctx
            .input(|i| i.pointer.any_down() || i.pointer.any_pressed() || i.pointer.any_released());
        let moved = match (self.last_mouse_pos, pointer) {
            (Some(last), Some(cur)) => last.distance(cur) > 1.5,
            (None, Some(_)) => true,
            _ => false,
        };

        // A popover or deck that owns the pointer keeps the HUD alive even if
        // the pointer sits still for minutes.
        let over_hud = pointer.is_some_and(|p| {
            self.deck_rect.is_some_and(|r| r.expand(8.0).contains(p))
                || self
                    .top_overlay_rect
                    .is_some_and(|r| r.expand(8.0).contains(p))
        });

        let playing =
            *self.controller.state() == PlaybackState::Playing && !self.controller.is_paused();

        if moved || pointer_active || over_hud {
            self.last_mouse_activity = Instant::now();
            self.last_mouse_pos = pointer;
            // Any deliberate input revives a manually hidden HUD.
            self.osd_manually_hidden = false;
        }

        let forced = std::env::var("MUVRIX_FORCE_OSD").ok();
        let visible = match forced.as_deref() {
            Some("0") => false,
            Some("1") => true,
            _ if !on_player => true,
            _ if self.osd_manually_hidden => false,
            _ if modal_active || over_hud => true,
            // Paused playback keeps controls reachable; this is the documented
            // default and can be turned off by the user hiding them manually.
            _ if !playing => true,
            _ => self.last_mouse_activity.elapsed() < self.osd_autohide_delay,
        };

        if visible {
            if let Some(remaining) = self
                .osd_autohide_delay
                .checked_sub(self.last_mouse_activity.elapsed())
            {
                ctx.request_repaint_after(remaining);
            }
        }

        self.ui_overlay_visible = visible;
        let target = if visible { 1.0 } else { 0.0 };
        let step = dt / 0.16;
        let next = if (target - self.overlay_alpha).abs() <= step {
            target
        } else if target > self.overlay_alpha {
            self.overlay_alpha + step
        } else {
            self.overlay_alpha - step
        };
        if (next - self.overlay_alpha).abs() > f32::EPSILON {
            self.overlay_alpha = next;
            ctx.request_repaint();
        }

        // Immersive playback hides the cursor together with the HUD and brings
        // it back on the first mouse movement.
        let immersive = (self.is_fullscreen || self.is_cinema_mode) && on_player;
        let want_hidden = immersive && self.overlay_alpha <= 0.01 && playing;
        if want_hidden != self.cursor_hidden {
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(!want_hidden));
            self.cursor_hidden = want_hidden;
        }
    }

    /// True when the keyboard help overlay is on screen.
    pub(crate) fn shortcut_help_open(&self) -> bool {
        self.show_shortcuts
    }

    /// Renders the searchable shortcut help overlay from the active registry.
    pub(crate) fn render_shortcuts_modal(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts {
            return;
        }

        let mut open = true;
        let mut close_clicked = false;
        let mut configure_clicked = false;
        let mut category = self.shortcut_category;
        let query = self.shortcut_help_search.to_ascii_lowercase();

        egui::Window::new("Keyboard Shortcuts")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.keybindings.profile.label())
                            .strong()
                            .color(BrandPalette::ELECTRIC_LIME),
                    );
                    ui.label(
                        RichText::new(self.keybindings.profile.description())
                            .size(11.5)
                            .color(BrandPalette::SECONDARY_TEXT),
                    );
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.shortcut_help_search)
                            .hint_text("Search actions or keys…")
                            .desired_width(240.0),
                    );
                    ui.label(
                        RichText::new(format!("{} actions", PlayerAction::ALL.len()))
                            .size(11.0)
                            .color(BrandPalette::MUTED_TEXT),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .selectable_label(category.is_none(), "All")
                        .clicked()
                    {
                        category = None;
                    }
                    for cat in ShortcutCategory::ALL {
                        if ui
                            .selectable_label(category == Some(*cat), cat.label())
                            .clicked()
                        {
                            category = Some(*cat);
                        }
                    }
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .max_height(460.0)
                    .show(ui, |ui| {
                        let mut any = false;
                        for cat in ShortcutCategory::ALL {
                            if category.is_some() && category != Some(*cat) {
                                continue;
                            }
                            let mut rows: Vec<(String, String, PlayerAction)> = Vec::new();
                            for &action in PlayerAction::ALL {
                                if action.category() != *cat {
                                    continue;
                                }
                                let primary = self.keybindings.hint(action);
                                let alt = self
                                    .keybindings
                                    .get_alternate(action)
                                    .map(|c| c.display_string())
                                    .unwrap_or_default();
                                let label = action.display_name().to_string();
                                let hay = format!("{label} {primary} {alt}").to_ascii_lowercase();
                                if !query.is_empty() && !hay.contains(&query) {
                                    continue;
                                }
                                if primary.is_empty() && alt.is_empty() {
                                    continue;
                                }
                                rows.push((label, primary, action));
                            }
                            if rows.is_empty() {
                                continue;
                            }
                            any = true;
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(cat.label())
                                    .strong()
                                    .size(12.0)
                                    .color(BrandPalette::ELECTRIC_LIME),
                            );
                            egui::Grid::new(format!("help_grid_{}", cat.label()))
                                .num_columns(2)
                                .spacing([16.0, 6.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    for (label, primary, action) in rows {
                                        let mut keys = primary;
                                        if let Some(alt) = self.keybindings.get_alternate(action) {
                                            if !alt.display_string().is_empty() {
                                                keys = format!("{keys}  /  {}", alt.display_string());
                                            }
                                        }
                                        ui.label(
                                            RichText::new(keys)
                                                .monospace()
                                                .strong()
                                                .color(BrandPalette::PRIMARY_TEXT),
                                        );
                                        ui.label(
                                            RichText::new(label)
                                                .size(12.0)
                                                .color(BrandPalette::SECONDARY_TEXT),
                                        );
                                        ui.end_row();
                                    }
                                });
                        }
                        if !any {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("No shortcuts match this search.")
                                    .color(BrandPalette::MUTED_TEXT),
                            );
                        }
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(
                                "Mouse: double-click video toggles fullscreen · right-click opens the player menu · wheel changes volume",
                            )
                            .size(11.0)
                            .color(BrandPalette::MUTED_TEXT),
                        );
                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Customize Shortcuts…").clicked() {
                        configure_clicked = true;
                    }
                    if ui.button("Close").clicked() {
                        close_clicked = true;
                    }
                });
            });

        self.shortcut_category = category;
        if close_clicked {
            open = false;
        }
        if configure_clicked {
            self.show_shortcuts = false;
            self.current_view = AppView::Settings;
            self.settings_tab = crate::views::SettingsTab::Shortcuts;
        } else if !open {
            self.show_shortcuts = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_profile_has_no_conflicts() {
        for profile in ShortcutProfile::ALL {
            let map = KeyBindingMap::for_profile(*profile);
            let conflicts = map.find_conflicts();
            assert!(
                conflicts.is_empty(),
                "{} has conflicts: {conflicts:?}",
                profile.label()
            );
        }
    }

    #[test]
    fn profiles_preserve_common_shortcuts() {
        for profile in ShortcutProfile::ALL {
            let map = KeyBindingMap::for_profile(*profile);
            assert_eq!(
                map.get(PlayerAction::PlayPause).unwrap().key,
                egui::Key::Space,
                "{} should keep Space for play/pause",
                profile.label()
            );
            assert_eq!(
                map.get(PlayerAction::ToggleFullscreen).unwrap().key,
                egui::Key::F
            );
        }
    }

    #[test]
    fn vlc_and_mpv_conflicts_are_resolved_by_priority() {
        // S: VLC Stop and mpv Screenshot must never share a slot.
        let classic = KeyBindingMap::for_profile(ShortcutProfile::Classic);
        assert_eq!(classic.get(PlayerAction::Stop).unwrap().key, egui::Key::S);
        assert!(classic.get(PlayerAction::Screenshot).unwrap().shift);

        // In the Advanced profile the same key is the mpv screenshot.
        let advanced = KeyBindingMap::for_profile(ShortcutProfile::Advanced);
        assert_eq!(
            advanced.get(PlayerAction::Screenshot).unwrap().key,
            egui::Key::S
        );

        // Hybrid keeps VLC's Stop on S.
        let hybrid = KeyBindingMap::for_profile(ShortcutProfile::Hybrid);
        assert_eq!(hybrid.get(PlayerAction::Stop).unwrap().key, egui::Key::S);
    }

    #[test]
    fn hybrid_uses_alternatives_for_mpv_only_actions() {
        let hybrid = KeyBindingMap::for_profile(ShortcutProfile::Hybrid);
        assert!(hybrid.get(PlayerAction::CycleAspectRatio).is_some());
        assert!(hybrid.get(PlayerAction::ToggleDeband).is_some());
        assert_eq!(
            hybrid.get(PlayerAction::CycleAudioDevice).unwrap(),
            &KeyCombination::new(egui::Key::A).with_shift(),
            "VLC priority keeps Shift+A on audio-device cycling"
        );
    }

    #[test]
    fn binding_a_conflict_reports_it_instead_of_overwriting() {
        let map = KeyBindingMap::for_profile(ShortcutProfile::Classic);
        let outcome = map.try_bind(PlayerAction::VolumeUp, KeyCombination::new(egui::Key::M));
        match outcome {
            BindOutcome::Conflict(list) => {
                assert!(list.iter().any(|(a, _)| *a == PlayerAction::ToggleMute));
            }
            BindOutcome::Bound => panic!("expected a conflict"),
        }
    }

    #[test]
    fn replace_and_swap_resolve_conflicts() {
        let mut map = KeyBindingMap::for_profile(ShortcutProfile::Classic);
        map.bind_replace(PlayerAction::VolumeUp, KeyCombination::new(egui::Key::M));
        assert_eq!(map.get(PlayerAction::VolumeUp).unwrap().key, egui::Key::M);
        assert!(map.get(PlayerAction::ToggleMute).is_none());

        let mut map = KeyBindingMap::for_profile(ShortcutProfile::Classic);
        map.bind_swap(PlayerAction::VolumeUp, KeyCombination::new(egui::Key::M));
        assert_eq!(
            map.get(PlayerAction::ToggleMute).unwrap().key,
            egui::Key::ArrowUp,
            "ToggleMute should inherit VolumeUp's previous binding"
        );
    }

    #[test]
    fn switch_profile_preserves_custom_overrides() {
        let mut map = KeyBindingMap::for_profile(ShortcutProfile::Classic);
        map.set_binding(PlayerAction::ToggleMute, KeyCombination::new(egui::Key::Q));
        map.set_profile(ShortcutProfile::Advanced);
        map.set_profile(ShortcutProfile::Classic);
        assert_eq!(map.get(PlayerAction::ToggleMute).unwrap().key, egui::Key::Q);
    }

    #[test]
    fn json_roundtrip_includes_profile_and_overrides() {
        let mut map = KeyBindingMap::for_profile(ShortcutProfile::Hybrid);
        map.set_binding(PlayerAction::ZoomIn, KeyCombination::new(egui::Key::X));
        let json = map.to_json().expect("serialize");
        let back = KeyBindingMap::from_json(&json).expect("deserialize");
        assert_eq!(map, back);
        assert_eq!(back.profile, ShortcutProfile::Hybrid);
    }

    #[test]
    fn reset_restores_profile_defaults() {
        let mut map = KeyBindingMap::for_profile(ShortcutProfile::Classic);
        map.set_binding(PlayerAction::ToggleMute, KeyCombination::new(egui::Key::Q));
        map.reset_to_defaults();
        assert_eq!(map.get(PlayerAction::ToggleMute).unwrap().key, egui::Key::M);
        assert!(map.overrides.is_empty());
    }

    #[test]
    fn key_strings_roundtrip() {
        for text in [
            "Space",
            "Ctrl+O",
            "Alt+Shift+V",
            "Ctrl+Alt+Left",
            "PageUp",
            "−",
            "?",
        ] {
            let combo =
                KeyCombination::parse_str(text).unwrap_or_else(|| panic!("failed to parse {text}"));
            assert_eq!(combo.display_string(), text);
        }
    }
}
