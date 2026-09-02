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

    pub fn parse(value: &str) -> Option<Self> {
        let primary = value.split('|').next()?.trim().replace('-', "+");
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
                "ctrl" | "control" | "strg" => control = true,
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

    pub const fn is_plain_text_input(self) -> bool {
        !self.control
            && !self.shift
            && !self.alt
            && matches!(self.key, ShortcutKey::Character(_) | ShortcutKey::Space)
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
    resolve_pass(shortcuts, chord, context, true)
        .or_else(|| resolve_pass(shortcuts, chord, context, false))
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
            .map_or(action.default_windows_shortcut, String::as_str);
        ShortcutChord::parse(configured) == Some(chord)
    })
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

    use crate::action::{ACTIONS, ActionScope};

    use super::{ShortcutChord, ShortcutContext, ShortcutKey, action_for_shortcut};

    #[test]
    fn every_default_shortcut_parses() {
        for action in ACTIONS {
            assert!(
                ShortcutChord::parse(action.default_windows_shortcut).is_some(),
                "{}: {}",
                action.id.as_str(),
                action.default_windows_shortcut
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
    fn global_actions_win_before_route_actions() {
        let shortcuts = BTreeMap::from([
            ("open_search".to_owned(), "Ctrl+F".to_owned()),
            ("add_favorite".to_owned(), "Ctrl+F".to_owned()),
        ]);
        let action = action_for_shortcut(
            &shortcuts,
            ShortcutChord::parse("Ctrl+F").expect("chord"),
            ShortcutContext::new(ActionScope::Player, false),
        )
        .expect("action");
        assert_eq!(action.id.as_str(), "open_search");
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
