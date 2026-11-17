use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

use eframe::egui::{self, RichText};
use muvrix_player::PlaybackState;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::app::MuvrixApp;
use crate::branding::BrandPalette;
use crate::views::AppView;

/// Identifies all configurable shortcut actions across MUVRIX Desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ShortcutAction {
    PlayPause,
    SeekBackward,
    SeekForward,
    VolumeUp,
    VolumeDown,
    FrameStepForward,
    FrameStepBack,
    CycleAbLoop,
    ToggleMute,
    ToggleFullscreen,
    Escape,
    TogglePlayerMore,
    ToggleSecondaryDeck,
    ToggleUrlInput,
    SpeedDecrease,
    SpeedIncrease,
    SpeedReset,
    OpenFile,
    ToggleOsd,
    TogglePlaylist,
    ToggleBrowser,
    TogglePip,
    ShowHelp,
}

impl ShortcutAction {
    pub const ALL: &'static [ShortcutAction] = &[
        ShortcutAction::PlayPause,
        ShortcutAction::SeekBackward,
        ShortcutAction::SeekForward,
        ShortcutAction::VolumeUp,
        ShortcutAction::VolumeDown,
        ShortcutAction::FrameStepForward,
        ShortcutAction::FrameStepBack,
        ShortcutAction::CycleAbLoop,
        ShortcutAction::ToggleMute,
        ShortcutAction::ToggleFullscreen,
        ShortcutAction::Escape,
        ShortcutAction::TogglePlayerMore,
        ShortcutAction::ToggleSecondaryDeck,
        ShortcutAction::ToggleUrlInput,
        ShortcutAction::SpeedDecrease,
        ShortcutAction::SpeedIncrease,
        ShortcutAction::SpeedReset,
        ShortcutAction::OpenFile,
        ShortcutAction::ToggleOsd,
        ShortcutAction::TogglePlaylist,
        ShortcutAction::ToggleBrowser,
        ShortcutAction::TogglePip,
        ShortcutAction::ShowHelp,
    ];

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::PlayPause => "Play / Pause Toggle",
            Self::SeekBackward => "Seek Backward (-5s)",
            Self::SeekForward => "Seek Forward (+5s)",
            Self::VolumeUp => "Volume Increase (+5%)",
            Self::VolumeDown => "Volume Decrease (-5%)",
            Self::FrameStepForward => "Frame Step Forward",
            Self::FrameStepBack => "Frame Step Backward",
            Self::CycleAbLoop => "Cycle A-B Loop (A / B / Clear)",
            Self::ToggleMute => "Mute / Unmute Audio",
            Self::ToggleFullscreen => "Toggle Fullscreen",
            Self::Escape => "Exit Fullscreen / Dismiss Popover",
            Self::TogglePlayerMore => "Toggle Player Overflow Menu",
            Self::ToggleSecondaryDeck => "Toggle Secondary Controls Deck",
            Self::ToggleUrlInput => "Toggle Stream URL Input",
            Self::SpeedDecrease => "Playback Speed Decrease (-0.25x)",
            Self::SpeedIncrease => "Playback Speed Increase (+0.25x)",
            Self::SpeedReset => "Reset Playback Speed (1.0x)",
            Self::OpenFile => "Open Media File",
            Self::ToggleOsd => "Toggle On-Screen Display (OSD)",
            Self::TogglePlaylist => "Toggle Queue / Playlist Drawer",
            Self::ToggleBrowser => "Toggle Media Browser View",
            Self::TogglePip => "Toggle Compact Mini-Player (PiP)",
            Self::ShowHelp => "Toggle Keyboard Shortcuts Help",
        }
    }

    pub fn default_combination(&self) -> KeyCombination {
        match self {
            Self::PlayPause => KeyCombination::new(egui::Key::Space),
            Self::SeekBackward => KeyCombination::new(egui::Key::ArrowLeft),
            Self::SeekForward => KeyCombination::new(egui::Key::ArrowRight),
            Self::VolumeUp => KeyCombination::new(egui::Key::ArrowUp),
            Self::VolumeDown => KeyCombination::new(egui::Key::ArrowDown),
            Self::FrameStepForward => KeyCombination::new(egui::Key::Period),
            Self::FrameStepBack => KeyCombination::new(egui::Key::Comma),
            Self::CycleAbLoop => KeyCombination::new(egui::Key::L),
            Self::ToggleMute => KeyCombination::new(egui::Key::M),
            Self::ToggleFullscreen => KeyCombination::new(egui::Key::F),
            Self::Escape => KeyCombination::new(egui::Key::Escape),
            Self::TogglePlayerMore => KeyCombination::new(egui::Key::M).with_alt(),
            Self::ToggleSecondaryDeck => KeyCombination::new(egui::Key::D).with_ctrl(),
            Self::ToggleUrlInput => KeyCombination::new(egui::Key::U).with_ctrl(),
            Self::SpeedDecrease => KeyCombination::new(egui::Key::OpenBracket),
            Self::SpeedIncrease => KeyCombination::new(egui::Key::CloseBracket),
            Self::SpeedReset => KeyCombination::new(egui::Key::Backspace),
            Self::OpenFile => KeyCombination::new(egui::Key::O).with_ctrl(),
            Self::ToggleOsd => KeyCombination::new(egui::Key::H),
            Self::TogglePlaylist => KeyCombination::new(egui::Key::P),
            Self::ToggleBrowser => KeyCombination::new(egui::Key::B),
            Self::TogglePip => KeyCombination::new(egui::Key::I),
            Self::ShowHelp => KeyCombination::new(egui::Key::F1),
        }
    }

    pub fn default_alternate(&self) -> Option<KeyCombination> {
        match self {
            Self::ToggleFullscreen => Some(KeyCombination::new(egui::Key::F11)),
            Self::ToggleSecondaryDeck => Some(KeyCombination::new(egui::Key::D).with_alt()),
            _ => None,
        }
    }
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
    pub fn matches(&self, input: &egui::InputState) -> bool {
        if !input.key_pressed(self.key) {
            return false;
        }

        let ctrl_match = if self.ctrl {
            input.modifiers.command || input.modifiers.ctrl
        } else {
            !input.modifiers.command && !input.modifiers.ctrl
        };

        let alt_match = if self.alt {
            input.modifiers.alt
        } else {
            !input.modifiers.alt
        };

        let shift_match = if self.shift {
            input.modifiers.shift
        } else {
            !input.modifiers.shift
        };

        ctrl_match && alt_match && shift_match
    }

    /// Formats this combination into a human-readable display string (e.g. `Ctrl+O`, `Alt+M`, `Space`).
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

    /// Parses a string representation like `Ctrl+O` or `Space` into a KeyCombination.
    pub fn parse_str(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        let tokens: Vec<&str> = s.split('+').map(|t| t.trim()).collect();
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut key_opt = None;

        for token in tokens {
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" => ctrl = true,
                "alt" | "opt" | "option" => alt = true,
                "shift" => shift = true,
                other => {
                    if let Some(k) = key_from_name(other) {
                        key_opt = Some(k);
                    }
                }
            }
        }

        key_opt.map(|key| Self {
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
            .ok_or_else(|| serde::de::Error::custom(format!("Invalid key combination string: {s}")))
    }
}

/// Returns a human-friendly string name for an `egui::Key`.
pub fn key_name(key: egui::Key) -> &'static str {
    match key {
        egui::Key::Space => "Space",
        egui::Key::ArrowLeft => "Left",
        egui::Key::ArrowRight => "Right",
        egui::Key::ArrowUp => "Up",
        egui::Key::ArrowDown => "Down",
        egui::Key::Period => ".",
        egui::Key::Comma => ",",
        egui::Key::OpenBracket => "[",
        egui::Key::CloseBracket => "]",
        egui::Key::Backspace => "Backspace",
        egui::Key::Escape => "Esc",
        egui::Key::Enter => "Enter",
        egui::Key::Tab => "Tab",
        egui::Key::Delete => "Delete",
        egui::Key::F1 => "F1",
        egui::Key::F2 => "F2",
        egui::Key::F3 => "F3",
        egui::Key::F4 => "F4",
        egui::Key::F5 => "F5",
        egui::Key::F6 => "F6",
        egui::Key::F7 => "F7",
        egui::Key::F8 => "F8",
        egui::Key::F9 => "F9",
        egui::Key::F10 => "F10",
        egui::Key::F11 => "F11",
        egui::Key::F12 => "F12",
        egui::Key::A => "A",
        egui::Key::B => "B",
        egui::Key::C => "C",
        egui::Key::D => "D",
        egui::Key::E => "E",
        egui::Key::F => "F",
        egui::Key::G => "G",
        egui::Key::H => "H",
        egui::Key::I => "I",
        egui::Key::J => "J",
        egui::Key::K => "K",
        egui::Key::L => "L",
        egui::Key::M => "M",
        egui::Key::N => "N",
        egui::Key::O => "O",
        egui::Key::P => "P",
        egui::Key::Q => "Q",
        egui::Key::R => "R",
        egui::Key::S => "S",
        egui::Key::T => "T",
        egui::Key::U => "U",
        egui::Key::V => "V",
        egui::Key::W => "W",
        egui::Key::X => "X",
        egui::Key::Y => "Y",
        egui::Key::Z => "Z",
        egui::Key::Num0 => "0",
        egui::Key::Num1 => "1",
        egui::Key::Num2 => "2",
        egui::Key::Num3 => "3",
        egui::Key::Num4 => "4",
        egui::Key::Num5 => "5",
        egui::Key::Num6 => "6",
        egui::Key::Num7 => "7",
        egui::Key::Num8 => "8",
        egui::Key::Num9 => "9",
        _ => "Unknown",
    }
}

/// Parses an `egui::Key` from its string name.
pub fn key_from_name(s: &str) -> Option<egui::Key> {
    match s.trim().to_ascii_lowercase().as_str() {
        "space" => Some(egui::Key::Space),
        "left" | "arrowleft" => Some(egui::Key::ArrowLeft),
        "right" | "arrowright" => Some(egui::Key::ArrowRight),
        "up" | "arrowup" => Some(egui::Key::ArrowUp),
        "down" | "arrowdown" => Some(egui::Key::ArrowDown),
        "." | "period" | "dot" => Some(egui::Key::Period),
        "," | "comma" => Some(egui::Key::Comma),
        "[" | "openbracket" => Some(egui::Key::OpenBracket),
        "]" | "closebracket" => Some(egui::Key::CloseBracket),
        "backspace" => Some(egui::Key::Backspace),
        "esc" | "escape" => Some(egui::Key::Escape),
        "enter" | "return" => Some(egui::Key::Enter),
        "tab" => Some(egui::Key::Tab),
        "delete" | "del" => Some(egui::Key::Delete),
        "f1" => Some(egui::Key::F1),
        "f2" => Some(egui::Key::F2),
        "f3" => Some(egui::Key::F3),
        "f4" => Some(egui::Key::F4),
        "f5" => Some(egui::Key::F5),
        "f6" => Some(egui::Key::F6),
        "f7" => Some(egui::Key::F7),
        "f8" => Some(egui::Key::F8),
        "f9" => Some(egui::Key::F9),
        "f10" => Some(egui::Key::F10),
        "f11" => Some(egui::Key::F11),
        "f12" => Some(egui::Key::F12),
        "a" => Some(egui::Key::A),
        "b" => Some(egui::Key::B),
        "c" => Some(egui::Key::C),
        "d" => Some(egui::Key::D),
        "e" => Some(egui::Key::E),
        "f" => Some(egui::Key::F),
        "g" => Some(egui::Key::G),
        "h" => Some(egui::Key::H),
        "i" => Some(egui::Key::I),
        "j" => Some(egui::Key::J),
        "k" => Some(egui::Key::K),
        "l" => Some(egui::Key::L),
        "m" => Some(egui::Key::M),
        "n" => Some(egui::Key::N),
        "o" => Some(egui::Key::O),
        "p" => Some(egui::Key::P),
        "q" => Some(egui::Key::Q),
        "r" => Some(egui::Key::R),
        "s" => Some(egui::Key::S),
        "t" => Some(egui::Key::T),
        "u" => Some(egui::Key::U),
        "v" => Some(egui::Key::V),
        "w" => Some(egui::Key::W),
        "x" => Some(egui::Key::X),
        "y" => Some(egui::Key::Y),
        "z" => Some(egui::Key::Z),
        "0" => Some(egui::Key::Num0),
        "1" => Some(egui::Key::Num1),
        "2" => Some(egui::Key::Num2),
        "3" => Some(egui::Key::Num3),
        "4" => Some(egui::Key::Num4),
        "5" => Some(egui::Key::Num5),
        "6" => Some(egui::Key::Num6),
        "7" => Some(egui::Key::Num7),
        "8" => Some(egui::Key::Num8),
        "9" => Some(egui::Key::Num9),
        _ => None,
    }
}

/// Configurable keybinding map with conflict detection, JSON persistence, and reset capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBindingMap {
    pub bindings: BTreeMap<ShortcutAction, KeyCombination>,
    #[serde(default)]
    pub alternates: BTreeMap<ShortcutAction, KeyCombination>,
}

impl Default for KeyBindingMap {
    fn default() -> Self {
        let mut bindings = BTreeMap::new();
        let mut alternates = BTreeMap::new();

        for &action in ShortcutAction::ALL {
            bindings.insert(action, action.default_combination());
            if let Some(alt) = action.default_alternate() {
                alternates.insert(action, alt);
            }
        }

        Self {
            bindings,
            alternates,
        }
    }
}

impl KeyBindingMap {
    /// Checks whether an action's bound combination was triggered in this input frame.
    pub fn matches(&self, action: ShortcutAction, input: &egui::InputState) -> bool {
        if let Some(combo) = self.bindings.get(&action) {
            if combo.matches(input) {
                return true;
            }
        }
        if let Some(alt) = self.alternates.get(&action) {
            if alt.matches(input) {
                return true;
            }
        }
        false
    }

    /// Gets the primary binding for an action.
    pub fn get(&self, action: ShortcutAction) -> Option<&KeyCombination> {
        self.bindings.get(&action)
    }

    /// Rebinds an action to a new combination.
    pub fn set_binding(&mut self, action: ShortcutAction, combination: KeyCombination) {
        self.bindings.insert(action, combination);
    }

    /// Resets all bindings and alternates to their factory defaults.
    pub fn reset_to_defaults(&mut self) {
        *self = Self::default();
    }

    /// Resets a single action to its default combination.
    pub fn reset_action(&mut self, action: ShortcutAction) {
        self.bindings.insert(action, action.default_combination());
        if let Some(alt) = action.default_alternate() {
            self.alternates.insert(action, alt);
        } else {
            self.alternates.remove(&action);
        }
    }

    /// Identifies all conflicting pairs of actions sharing the exact same key combination.
    pub fn find_conflicts(&self) -> Vec<(ShortcutAction, ShortcutAction, KeyCombination)> {
        let mut conflicts = Vec::new();
        let list: Vec<(ShortcutAction, KeyCombination)> =
            self.bindings.iter().map(|(a, c)| (*a, *c)).collect();

        for i in 0..list.len() {
            for j in (i + 1)..list.len() {
                let (act_a, combo_a) = list[i];
                let (act_b, combo_b) = list[j];
                if combo_a == combo_b {
                    conflicts.push((act_a, act_b, combo_a));
                }
            }
        }

        conflicts
    }

    /// Serializes keybindings map to formatted JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserializes keybindings map from JSON string.
    pub fn from_json(json_str: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json_str)
    }

    /// Configuration file path for persisting user custom keybindings.
    pub fn config_path() -> std::path::PathBuf {
        crate::config_editor::config_dir().join("shortcuts.json")
    }

    /// Loads custom keybindings from disk, or falls back to defaults.
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

    /// Saves keybindings map to disk in the Muvrix config directory.
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
}

impl MuvrixApp {
    /// Checks `ctx.wants_keyboard_input()` to ensure shortcuts do not trigger
    /// while the user is actively typing in a text field, then dispatches configured actions.
    pub(crate) fn handle_keyboard_inputs(&mut self, ctx: &egui::Context) {
        // Do NOT intercept shortcuts if user is typing into an active text field!
        if ctx.wants_keyboard_input() {
            return;
        }

        // If recording a shortcut in Settings view, capture key press directly
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
                        // Escape cancels recording without rebind
                        if *key == egui::Key::Escape {
                            captured = Some(None);
                        } else {
                            let combo = KeyCombination {
                                key: *key,
                                ctrl: modifiers.command || modifiers.ctrl,
                                alt: modifiers.alt,
                                shift: modifiers.shift,
                            };
                            captured = Some(Some(combo));
                        }
                        break;
                    }
                }
            });

            if let Some(res) = captured {
                if let Some(combo) = res {
                    self.keybindings.set_binding(target_action, combo);
                    let _ = self.keybindings.save_to_disk();
                }
                self.recording_shortcut = None;
                return;
            }
        }

        // 1. Play / Pause
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::PlayPause, i)) {
            self.toggle_pause();
        }

        // 2. Relative Seek (-5s / +5s)
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::SeekBackward, i)) {
            let _ = self.controller.seek_relative(-5.0);
        }
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::SeekForward, i)) {
            let _ = self.controller.seek_relative(5.0);
        }

        // 3. Volume Adjustments (+5% / -5%)
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::VolumeUp, i)) {
            let _ = self.controller.adjust_volume(5.0);
        }
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::VolumeDown, i)) {
            let _ = self.controller.adjust_volume(-5.0);
        }

        // 4. Frame Stepping (. / ,)
        if ctx.input(|i| {
            self.keybindings
                .matches(ShortcutAction::FrameStepForward, i)
        }) {
            self.frame_step();
        }
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::FrameStepBack, i)) {
            self.frame_back_step();
        }

        // 5. A-B Loop
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::CycleAbLoop, i)) {
            self.cycle_ab_loop();
        }

        // 6. Mute Toggle
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::ToggleMute, i)) {
            let _ = self.controller.toggle_mute();
        }

        // 7. Fullscreen Toggle
        if ctx.input(|i| {
            self.keybindings
                .matches(ShortcutAction::ToggleFullscreen, i)
        }) {
            self.toggle_fullscreen(ctx);
        }

        // 8. Escape: Exit Fullscreen or dismiss modals
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::Escape, i)) {
            if self.player_more_overflow_open {
                self.player_more_overflow_open = false;
            } else if self.player_empty_url_input_open {
                self.player_empty_url_input_open = false;
            } else if self.show_shortcuts {
                self.show_shortcuts = false;
            } else if self.is_fullscreen {
                self.exit_fullscreen(ctx);
            }
        }

        // 9. Overflow Menu Toggle
        if ctx.input(|i| {
            self.keybindings
                .matches(ShortcutAction::TogglePlayerMore, i)
        }) {
            self.player_more_overflow_open = !self.player_more_overflow_open;
        }

        // 10. Secondary Controls Deck Toggle
        if ctx.input(|i| {
            self.keybindings
                .matches(ShortcutAction::ToggleSecondaryDeck, i)
        }) {
            let screen_w = ctx.screen_rect().width();
            if screen_w >= 1300.0 {
                self.player_secondary_deck_collapsed_wide =
                    !self.player_secondary_deck_collapsed_wide;
            } else {
                self.player_secondary_deck_expanded = !self.player_secondary_deck_expanded;
            }
        }

        // 11. URL Streaming Input Toggle
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::ToggleUrlInput, i)) {
            self.player_empty_url_input_open = !self.player_empty_url_input_open;
        }

        // 12. Speed Adjustment (-0.25x / +0.25x / Reset)
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::SpeedDecrease, i)) {
            let _ = self.controller.adjust_speed(-0.25);
        }
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::SpeedIncrease, i)) {
            let _ = self.controller.adjust_speed(0.25);
        }
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::SpeedReset, i)) {
            let _ = self.controller.reset_speed();
        }

        // 13. Open File Dialog
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::OpenFile, i)) {
            self.open_file_dialog();
        }

        // 14. Toggle UI overlay / OSD
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::ToggleOsd, i)) {
            self.ui_overlay_visible = !self.ui_overlay_visible;
            self.osd_manually_hidden = !self.ui_overlay_visible;
            self.last_mouse_activity = Instant::now();
        }

        // 15. Toggle Playlist Drawer
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::TogglePlaylist, i)) {
            self.show_playlist_drawer = !self.show_playlist_drawer;
        }

        // 16. Toggle Media Browser View
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::ToggleBrowser, i)) {
            self.current_view = if self.current_view == AppView::Browser {
                AppView::Player
            } else {
                AppView::Browser
            };
        }

        // 17. Toggle Picture-in-Picture
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::TogglePip, i)) {
            self.toggle_pip(ctx);
        }

        // 18. Toggle Shortcuts Help Dialog
        if ctx.input(|i| self.keybindings.matches(ShortcutAction::ShowHelp, i)) {
            self.show_shortcuts = !self.show_shortcuts;
        }
    }

    /// Automatically hides on-screen playback controls during uninterrupted playback.
    pub(crate) fn update_osd_autohide(&mut self, ctx: &egui::Context) {
        let modal_active = self.show_shortcuts
            || self.show_diagnostics
            || self.show_playlist_drawer
            || self.add_root_dialog_open
            || self.pending_resume_prompt.is_some()
            || self.crash_notice.is_some()
            || self.player_more_overflow_open;

        if modal_active {
            self.last_mouse_activity = Instant::now();
            self.ui_overlay_visible = true;
            return;
        }

        if self.current_view != AppView::Player {
            self.ui_overlay_visible = true;
            return;
        }

        if std::env::var("MUVRIX_FORCE_OSD")
            .map(|v| v == "0")
            .unwrap_or(false)
        {
            self.ui_overlay_visible = false;
            return;
        }
        if std::env::var("MUVRIX_FORCE_OSD")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            self.ui_overlay_visible = true;
            return;
        }

        let is_playing =
            *self.controller.state() == PlaybackState::Playing && !self.controller.is_paused();

        let current_mouse_pos = ctx.input(|i| i.pointer.hover_pos());
        let mouse_moved = match (self.last_mouse_pos, current_mouse_pos) {
            (Some(last), Some(cur)) => last.distance(cur) > 2.0,
            (None, Some(_)) => true,
            _ => false,
        };
        let mouse_active = ctx
            .input(|i| i.pointer.any_down() || i.pointer.any_pressed() || i.pointer.any_released());

        let screen_h = ctx.screen_rect().height();
        let cursor_over_controls = current_mouse_pos
            .is_some_and(|pos| pos.y <= 52.0 || pos.y >= (screen_h - 110.0).max(0.0));

        if mouse_moved || mouse_active {
            self.last_mouse_activity = Instant::now();
            self.last_mouse_pos = current_mouse_pos;
            self.ui_overlay_visible = true;
            self.osd_manually_hidden = false;
        } else if cursor_over_controls && self.ui_overlay_visible {
            self.last_mouse_activity = Instant::now();
        }

        if is_playing {
            if !self.osd_manually_hidden && !cursor_over_controls {
                let elapsed = self.last_mouse_activity.elapsed();
                if elapsed >= self.osd_autohide_delay {
                    self.ui_overlay_visible = false;
                } else {
                    let remaining = self.osd_autohide_delay.saturating_sub(elapsed);
                    ctx.request_repaint_after(remaining);
                }
            }
        } else if !self.osd_manually_hidden {
            self.ui_overlay_visible = true;
        }
    }

    /// Renders the keyboard shortcuts modal populated directly from the active `KeyBindingMap`.
    pub(crate) fn render_shortcuts_modal(&mut self, ctx: &egui::Context) {
        if self.show_shortcuts {
            let mut open = true;
            let mut close_requested = false;
            let mut configure_clicked = false;

            egui::Window::new("Keyboard Shortcuts")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.set_width(420.0);
                    egui::ScrollArea::vertical()
                        .max_height(480.0)
                        .show(ui, |ui| {
                            egui::Grid::new("shortcuts_grid")
                                .striped(true)
                                .spacing([20.0, 6.0])
                                .show(ui, |ui| {
                                    for &action in ShortcutAction::ALL {
                                        let primary = self
                                            .keybindings
                                            .get(action)
                                            .map(|c| c.display_string())
                                            .unwrap_or_else(|| "-".to_string());
                                        let alt_str = self
                                            .keybindings
                                            .alternates
                                            .get(&action)
                                            .map(|a| format!(" / {}", a.display_string()))
                                            .unwrap_or_default();

                                        ui.label(
                                            RichText::new(format!("{primary}{alt_str}"))
                                                .strong()
                                                .monospace()
                                                .color(BrandPalette::ELECTRIC_LIME),
                                        );
                                        ui.label(action.display_name());
                                        ui.end_row();
                                    }

                                    ui.label(RichText::new("Double Click").strong().monospace());
                                    ui.label("Toggle Fullscreen");
                                    ui.end_row();

                                    ui.label(RichText::new("Single Click").strong().monospace());
                                    ui.label("Play / Pause (Video)");
                                    ui.end_row();
                                });
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Customize Shortcuts in Settings...").clicked() {
                            configure_clicked = true;
                        }
                        if ui.button("Close").clicked() {
                            close_requested = true;
                        }
                    });
                });

            if configure_clicked {
                self.show_shortcuts = false;
                self.current_view = AppView::Settings;
                self.settings_tab = crate::views::SettingsTab::Shortcuts;
            } else {
                self.show_shortcuts = open && !close_requested;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_custom_keybinding_dispatch() {
        let mut keybindings = KeyBindingMap::default();

        // 1. Verify default PlayPause is Space
        let play_pause_combo = keybindings.get(ShortcutAction::PlayPause).unwrap();
        assert_eq!(play_pause_combo.key, egui::Key::Space);
        assert!(!play_pause_combo.ctrl);
        assert_eq!(play_pause_combo.display_string(), "Space");

        // 2. Rebind PlayPause to 'K'
        keybindings.set_binding(ShortcutAction::PlayPause, KeyCombination::new(egui::Key::K));
        let updated = keybindings.get(ShortcutAction::PlayPause).unwrap();
        assert_eq!(updated.key, egui::Key::K);
        assert_eq!(updated.display_string(), "K");

        // 3. Create a deliberate conflict: bind VolumeUp also to 'K'
        keybindings.set_binding(ShortcutAction::VolumeUp, KeyCombination::new(egui::Key::K));
        let conflicts = keybindings.find_conflicts();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].0, ShortcutAction::PlayPause);
        assert_eq!(conflicts[0].1, ShortcutAction::VolumeUp);
        assert_eq!(conflicts[0].2.key, egui::Key::K);

        // 4. Reset to defaults clears conflict
        keybindings.reset_to_defaults();
        assert!(keybindings.find_conflicts().is_empty());
        assert_eq!(
            keybindings.get(ShortcutAction::PlayPause).unwrap().key,
            egui::Key::Space
        );
        assert_eq!(
            keybindings.get(ShortcutAction::VolumeUp).unwrap().key,
            egui::Key::ArrowUp
        );

        // 5. Test JSON roundtrip serialization
        let json = keybindings.to_json().expect("Serialization failed");
        let deserialized = KeyBindingMap::from_json(&json).expect("Deserialization failed");
        assert_eq!(keybindings, deserialized);
    }
}
