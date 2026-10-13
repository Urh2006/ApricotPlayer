//! Canonical shortcut parsing and scope-aware action resolution.

use std::collections::BTreeMap;

use crate::action::{ACTIONS, ActionDefinition, ActionScope};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShortcutKey {
    Character(char),
    Function(u8),
    Enter,
    Space,
    Escape,
    Delete,
    Backspace,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
    Applications,
    LeftBracket,
    RightBracket,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ShortcutChord {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: ShortcutKey,
}

impl ShortcutChord {
    pub const fn new(control: bool, shift: bool, alt: bool, key: ShortcutKey) -> Self {
        Self {
            control,
            shift,
            alt,
            key,
        }
    }

    /// Python `parse_shortcut`: the first alternative of `A | B`.
    pub fn parse(value: &str) -> Option<Self> {
        Self::parse_single(value.split('|').next()?)
    }

    /// Python `event_matches_shortcut`: every alternative of `A | B` matches.
    pub fn matches_configured(self, configured: &str) -> bool {
        configured
            .split('|')
            .filter(|alternative| !alternative.trim().is_empty())
            .any(|alternative| Self::parse_single(alternative) == Some(self))
    }

    fn parse_single(value: &str) -> Option<Self> {
        let primary = value.trim().replace('-', "+");
        let mut control = false;
        let mut shift = false;
        let mut alt = false;
        let mut key_parts = Vec::new();
        for part in primary
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
        {
            match normalize(part).as_str() {
                // Command is the macOS name of the logical primary modifier.
                "ctrl" | "control" | "strg" | "command" | "cmd" => control = true,
                "shift" | "shft" => shift = true,
                "alt" | "option" => alt = true,
                _ => key_parts.push(part),
            }
        }
        if key_parts.is_empty() {
            return None;
        }
        let key = parse_key(&key_parts.join(" "))?;
        Some(Self::new(control, shift, alt, key))
    }

    /// Python `shortcut_is_plain_printable`: a printable character, also with
    /// Shift, is typing in a text field and never runs an action there.
    pub const fn is_plain_text_input(self) -> bool {
        !self.control
            && !self.alt
            && match self.key {
                ShortcutKey::Character(_)
                | ShortcutKey::LeftBracket
                | ShortcutKey::RightBracket => true,
                ShortcutKey::Space => !self.shift,
                _ => false,
            }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShortcutContext {
    pub scope: ActionScope,
    pub accepts_text: bool,
}

impl ShortcutContext {
    pub const fn new(scope: ActionScope, accepts_text: bool) -> Self {
        Self {
            scope,
            accepts_text,
        }
    }
}

pub fn action_for_shortcut(
    shortcuts: &BTreeMap<String, String>,
    chord: ShortcutChord,
    context: ShortcutContext,
) -> Option<&'static ActionDefinition> {
    // The focused player owns its keys, including user-configured collisions.
    // Menus and text fields retain their existing global action priority.
    let global_first = context.scope != ActionScope::Player;
    resolve_pass(shortcuts, chord, context, global_first)
        .or_else(|| resolve_pass(shortcuts, chord, context, !global_first))
}

/// A focused media surface owns its scoped actions before global shortcuts.
/// Callers use this for Spotify rows; ordinary menus keep global priority.
pub fn focused_action_for_shortcut(
    shortcuts: &BTreeMap<String, String>,
    chord: ShortcutChord,
    context: ShortcutContext,
) -> Option<&'static ActionDefinition> {
    resolve_pass(shortcuts, chord, context, false)
        .or_else(|| resolve_pass(shortcuts, chord, context, true))
}

/// Transport shortcuts available while the player is outside its own page.
/// The focused screen keeps its actions, typing and native navigation.
pub fn background_player_action_for_shortcut(
    shortcuts: &BTreeMap<String, String>,
    chord: ShortcutChord,
    context: ShortcutContext,
) -> Option<&'static ActionDefinition> {
    if context.accepts_text
        || !(chord.control || chord.alt || matches!(chord.key, ShortcutKey::Function(_)))
        || action_for_shortcut(shortcuts, chord, context).is_some()
    {
        return None;
    }
    action_for_shortcut(
        shortcuts,
        chord,
        ShortcutContext::new(ActionScope::Player, false),
    )
    .filter(|action| {
        matches!(
            action.id.as_str(),
            "player_previous" | "player_next" | "player_next_related" | "player_fullscreen"
        )
    })
}

fn resolve_pass(
    shortcuts: &BTreeMap<String, String>,
    chord: ShortcutChord,
    context: ShortcutContext,
    global_pass: bool,
) -> Option<&'static ActionDefinition> {
    ACTIONS.iter().find(|action| {
        let is_global = action.scopes.contains(&ActionScope::Global);
        if is_global != global_pass
            || (!is_global && !action.scopes.contains(&context.scope))
            || (context.accepts_text && chord.is_plain_text_input())
        {
            return false;
        }
        let configured = shortcuts
            .get(action.id.as_str())
            .map_or(action.default_shortcut(), String::as_str);
        chord.matches_configured(configured)
    })
}

/// How a stored shortcut is shown to the user. Shortcuts are stored with the
/// Windows names; on macOS the logical primary modifier `Ctrl` is Command and
/// `Alt` is Option (`docs/MACOS_PORT_PLAN.md`, keyboard shortcut parity).
pub fn display_shortcut(stored: &str) -> String {
    if cfg!(target_os = "macos") {
        macos_shortcut_display(stored)
    } else {
        stored.to_owned()
    }
}

/// The macOS spelling of a stored shortcut: `Ctrl+Alt+Y` reads
/// `Command+Option+Y`; other keys and the `A | B` layout stay as stored.
pub fn macos_shortcut_display(stored: &str) -> String {
    stored
        .split('|')
        .map(|alternative| {
            alternative
                .split('+')
                .map(|part| {
                    let trimmed = part.trim();
                    let name = match normalize(trimmed).as_str() {
                        "ctrl" | "control" | "strg" => "Command",
                        "alt" | "option" => "Option",
                        _ => return part.to_owned(),
                    };
                    part.replacen(trimmed, name, 1)
                })
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn parse_key(value: &str) -> Option<ShortcutKey> {
    let normalized = normalize(value);
    let key = match normalized.as_str() {
        "enter" | "return" => ShortcutKey::Enter,
        "space" | "spacebar" => ShortcutKey::Space,
        "escape" | "esc" => ShortcutKey::Escape,
        "delete" | "del" => ShortcutKey::Delete,
        "backspace" | "back" => ShortcutKey::Backspace,
        "insert" | "ins" => ShortcutKey::Insert,
        "home" => ShortcutKey::Home,
        "end" => ShortcutKey::End,
        "pageup" => ShortcutKey::PageUp,
        "pagedown" => ShortcutKey::PageDown,
        "left" | "leftarrow" => ShortcutKey::Left,
        "right" | "rightarrow" => ShortcutKey::Right,
        "up" | "uparrow" => ShortcutKey::Up,
        "down" | "downarrow" => ShortcutKey::Down,
        "applications" | "application" | "apps" | "menu" | "contextmenu" => {
            ShortcutKey::Applications
        }
        "[" | "leftbracket" | "openbracket" | "physicalleftbracket" => ShortcutKey::LeftBracket,
        "]" | "rightbracket" | "closebracket" | "physicalrightbracket" => ShortcutKey::RightBracket,
        _ => {
            if let Some(number) = normalized.strip_prefix('f')
                && let Ok(number) = number.parse::<u8>()
                && (1..=24).contains(&number)
            {
                return Some(ShortcutKey::Function(number));
            }
            let mut characters = normalized.chars();
            let character = characters.next()?;
            if characters.next().is_none() && character.is_ascii_alphanumeric() {
                ShortcutKey::Character(character.to_ascii_uppercase())
            } else {
                return None;
            }
        }
    };
    Some(key)
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && *character != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    #[test]
    fn macos_display_names_the_primary_modifier_command_and_alt_option() {
        use super::macos_shortcut_display;
        assert_eq!(macos_shortcut_display("Ctrl+Alt+Y"), "Command+Option+Y");
        assert_eq!(macos_shortcut_display("Ctrl+Shift+F"), "Command+Shift+F");
        assert_eq!(macos_shortcut_display("F8"), "F8");
        assert_eq!(
            macos_shortcut_display("Ctrl+Up | Alt+Up"),
            "Command+Up | Option+Up"
        );
        assert_eq!(
            super::ShortcutChord::parse("Command+Option+Y"),
            super::ShortcutChord::parse("Ctrl+Alt+Y")
        );
    }

    use crate::action::{ACTIONS, ActionScope};

    use super::{
        ShortcutChord, ShortcutContext, ShortcutKey, action_for_shortcut,
        background_player_action_for_shortcut, focused_action_for_shortcut,
    };

    #[test]
    fn focused_spotify_rows_like_the_selection_despite_a_global_collision() {
        let shortcuts = BTreeMap::from([(
            "new_subscription_videos".to_owned(),
            "Ctrl+Shift+I".to_owned(),
        )]);
        let chord = ShortcutChord::parse("Ctrl+Shift+I").unwrap();
        let context = ShortcutContext::new(ActionScope::List, false);
        assert_eq!(
            focused_action_for_shortcut(&shortcuts, chord, context)
                .unwrap()
                .id
                .as_str(),
            "spotify_toggle_saved"
        );
        assert_eq!(
            action_for_shortcut(&shortcuts, chord, context)
                .unwrap()
                .id
                .as_str(),
            "new_subscription_videos"
        );
    }

    #[test]
    fn background_transport_works_in_menus_and_results() {
        for (key, expected) in [
            ("Ctrl+PageUp", "player_previous"),
            ("Ctrl+PageDown", "player_next"),
        ] {
            let action = background_player_action_for_shortcut(
                &BTreeMap::new(),
                ShortcutChord::parse(key).unwrap(),
                ShortcutContext::new(ActionScope::List, false),
            )
            .expect("background transport");
            assert_eq!(action.id.as_str(), expected);
        }
    }

    #[test]
    fn background_transport_preserves_typing_navigation_and_route_actions() {
        let context = ShortcutContext::new(ActionScope::List, false);
        for key in ["T", "Shift+T", "PageUp", "Down", "Space"] {
            let shortcuts = BTreeMap::from([("player_previous".to_owned(), key.to_owned())]);
            assert!(
                background_player_action_for_shortcut(
                    &shortcuts,
                    ShortcutChord::parse(key).unwrap(),
                    context
                )
                .is_none(),
                "{key}"
            );
        }
        let shortcuts = BTreeMap::from([("open_search".to_owned(), "Ctrl+PageDown".to_owned())]);
        assert!(
            background_player_action_for_shortcut(
                &shortcuts,
                ShortcutChord::parse("Ctrl+PageDown").unwrap(),
                context
            )
            .is_none()
        );
        assert!(
            background_player_action_for_shortcut(
                &BTreeMap::new(),
                ShortcutChord::parse("Ctrl+PageDown").unwrap(),
                ShortcutContext::new(ActionScope::Dialog, true)
            )
            .is_none()
        );
    }

    #[test]
    fn every_default_shortcut_parses() {
        for action in ACTIONS {
            assert!(
                ShortcutChord::parse(action.default_shortcut()).is_some(),
                "{}: {}",
                action.id.as_str(),
                action.default_shortcut()
            );
        }
    }

    #[test]
    fn aliases_and_case_have_one_canonical_chord() {
        let expected = ShortcutChord::new(true, true, false, ShortcutKey::PageDown);
        assert_eq!(
            ShortcutChord::parse("Control+Shft+Page Down"),
            Some(expected)
        );
        assert_eq!(ShortcutChord::parse("ctrl-shift-pagedown"), Some(expected));
        assert_eq!(
            ShortcutChord::parse("Ctrl+Shift+PageDown | F9"),
            Some(expected)
        );
    }

    #[test]
    fn every_alternative_of_an_imported_shortcut_runs_the_action() {
        let shortcuts = BTreeMap::from([("open_search".to_owned(), "Ctrl+F8 | F9".to_owned())]);
        for key in ["Ctrl+F8", "F9"] {
            let action = action_for_shortcut(
                &shortcuts,
                ShortcutChord::parse(key).expect("chord"),
                ShortcutContext::new(ActionScope::List, false),
            )
            .expect("action");
            assert_eq!(action.id.as_str(), "open_search", "{key}");
        }
    }

    #[test]
    fn shifted_letters_are_typing_in_text_fields() {
        let shortcuts = BTreeMap::from([("open_search".to_owned(), "Shift+Y".to_owned())]);
        let chord = ShortcutChord::parse("Shift+Y").expect("chord");
        assert!(chord.is_plain_text_input());
        assert!(
            action_for_shortcut(
                &shortcuts,
                chord,
                ShortcutContext::new(ActionScope::List, true)
            )
            .is_none()
        );
        assert!(
            action_for_shortcut(
                &shortcuts,
                chord,
                ShortcutContext::new(ActionScope::List, false)
            )
            .is_some()
        );
        assert!(
            !ShortcutChord::parse("Shift+Space")
                .unwrap()
                .is_plain_text_input()
        );
        assert!(
            !ShortcutChord::parse("Ctrl+Y")
                .unwrap()
                .is_plain_text_input()
        );
    }

    #[test]
    fn global_actions_win_before_route_actions() {
        let shortcuts = BTreeMap::from([
            ("open_search".to_owned(), "Ctrl+F".to_owned()),
            ("add_favorite".to_owned(), "Ctrl+F".to_owned()),
        ]);
        let action = action_for_shortcut(
            &shortcuts,
            ShortcutChord::parse("Ctrl+F").expect("chord"),
            ShortcutContext::new(ActionScope::List, false),
        )
        .expect("action");
        assert_eq!(action.id.as_str(), "open_search");
    }

    #[test]
    fn player_zone_prioritizes_all_player_actions_over_global_collisions() {
        for player_action in ACTIONS.iter().filter(|action| {
            !action.scopes.contains(&ActionScope::Global)
                && action.scopes.contains(&ActionScope::Player)
        }) {
            let key = player_action.default_shortcut();
            let shortcuts =
                BTreeMap::from([("new_subscription_videos".to_owned(), key.to_owned())]);
            let action = action_for_shortcut(
                &shortcuts,
                ShortcutChord::parse(key).unwrap(),
                ShortcutContext::new(ActionScope::Player, false),
            )
            .unwrap();
            assert_eq!(action.id, player_action.id, "player zone: {key}");
            assert_eq!(
                action_for_shortcut(
                    &shortcuts,
                    ShortcutChord::parse(key).unwrap(),
                    ShortcutContext::new(ActionScope::List, false)
                )
                .unwrap()
                .id
                .as_str(),
                "new_subscription_videos",
                "menu/list retains global routing: {key}"
            );
        }
    }

    #[test]
    fn player_keys_do_not_leak_into_lists_or_text_fields() {
        let shortcuts = BTreeMap::new();
        let volume = ShortcutChord::parse("V").expect("volume chord");
        assert!(
            action_for_shortcut(
                &shortcuts,
                volume,
                ShortcutContext::new(ActionScope::List, false)
            )
            .is_none()
        );
        assert!(
            action_for_shortcut(
                &shortcuts,
                volume,
                ShortcutContext::new(ActionScope::Player, true)
            )
            .is_none()
        );
        assert_eq!(
            action_for_shortcut(
                &shortcuts,
                volume,
                ShortcutContext::new(ActionScope::Player, false)
            )
            .expect("player action")
            .id
            .as_str(),
            "player_volume_status"
        );
    }
}
