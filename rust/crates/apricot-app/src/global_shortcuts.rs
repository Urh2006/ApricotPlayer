//! 2.1 global shortcuts: playback, volume and recording commands that work
//! while another program is in front. Each command has one chord; an empty
//! chord turns it off.

use std::collections::BTreeMap;

use apricot_core::shortcut::ShortcutChord;
use apricot_storage::SettingsDocument;
use serde::{Deserialize, Serialize};

/// Settings key of [`GlobalShortcutSettings`].
pub const GLOBAL_SHORTCUTS_KEY: &str = "global_shortcuts_v1";

/// The commands that can have a global shortcut, with their Windows
/// defaults. Win with Alt alone is often taken by other programs on these
/// keys; Win, Ctrl and Alt together is free.
pub const GLOBAL_ACTIONS: [(&str, &str); 10] = [
    ("player_play_pause", "Win+Ctrl+Alt+Space"),
    ("player_seek_back", "Win+Ctrl+Alt+Left"),
    ("player_seek_forward", "Win+Ctrl+Alt+Right"),
    ("player_volume_up", "Win+Ctrl+Alt+Up"),
    ("player_volume_down", "Win+Ctrl+Alt+Down"),
    ("player_previous", "Win+Ctrl+Alt+PageUp"),
    ("player_next", "Win+Ctrl+Alt+PageDown"),
    ("recording_start", "Win+Ctrl+Alt+F9"),
    ("recording_pause", "Win+Ctrl+Alt+F10"),
    ("recording_stop", "Win+Ctrl+Alt+F11"),
];

/// A global chord: an application chord plus the Windows key (the Control
/// key on a Mac, where Ctrl means Command).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlobalChord {
    pub system: bool,
    pub chord: ShortcutChord,
}

impl GlobalChord {
    /// "Win+Ctrl+Alt+Space", "Ctrl+Shift+F9". A global chord needs Win, Ctrl
    /// or Alt, so it never takes a plain key from every other program.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut system = false;
        let mut rest = Vec::new();
        for part in text.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "win" | "windows" | "super" | "meta" => system = true,
                "" => return None,
                _ => rest.push(part),
            }
        }
        let chord = ShortcutChord::parse(&rest.join("+"))?;
        (system || chord.control || chord.alt).then_some(Self { system, chord })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct GlobalShortcutSettings {
    pub enabled: bool,
    /// Chords the user changed, by action id; missing ones use the default.
    pub bindings: BTreeMap<String, String>,
}

impl Default for GlobalShortcutSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            bindings: BTreeMap::new(),
        }
    }
}

/// The default of `action`. A Mac has none yet: Control with Option is the
/// VoiceOver modifier there.
#[must_use]
pub fn default_binding(action: &str) -> &'static str {
    if cfg!(target_os = "macos") {
        return "";
    }
    GLOBAL_ACTIONS
        .iter()
        .find(|(id, _)| *id == action)
        .map_or("", |(_, chord)| chord)
}

impl GlobalShortcutSettings {
    #[must_use]
    pub fn from_settings(settings: &SettingsDocument) -> Self {
        settings
            .preserved
            .get(GLOBAL_SHORTCUTS_KEY)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }

    /// The chord text of `action`, empty when it is off.
    #[must_use]
    pub fn binding(&self, action: &str) -> String {
        self.bindings
            .get(action)
            .cloned()
            .unwrap_or_else(|| default_binding(action).to_owned())
    }

    /// The commands with a usable chord, in [`GLOBAL_ACTIONS`] order.
    #[must_use]
    pub fn active(&self) -> Vec<(&'static str, GlobalChord)> {
        if !self.enabled {
            return Vec::new();
        }
        GLOBAL_ACTIONS
            .iter()
            .filter_map(|(action, _)| {
                GlobalChord::parse(&self.binding(action)).map(|chord| (*action, chord))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_need_a_modifier_and_read_the_windows_key() {
        let chord = GlobalChord::parse("Win+Alt+Space").expect("chord");
        assert!(chord.system && chord.chord.alt && !chord.chord.control);
        if !cfg!(target_os = "macos") {
            assert!(GlobalChord::parse(default_binding("player_next")).is_some());
        }
        assert!(GlobalChord::parse("Space").is_none());
        assert!(GlobalChord::parse("Shift+F9").is_none());
        assert!(GlobalChord::parse("Ctrl+Shift+F9").is_some());
        assert!(GlobalChord::parse("Win+").is_none());
    }

    #[test]
    fn defaults_are_on_and_a_cleared_chord_turns_one_off() {
        let mut settings = GlobalShortcutSettings::default();
        let expected = if cfg!(target_os = "macos") { 0 } else { 10 };
        assert_eq!(settings.active().len(), expected);
        settings
            .bindings
            .insert("player_play_pause".to_owned(), String::new());
        assert!(
            settings
                .active()
                .iter()
                .all(|(action, _)| *action != "player_play_pause")
        );
        settings.enabled = false;
        assert!(settings.active().is_empty());
    }
}
