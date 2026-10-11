//! macOS key presses as Apricot shortcut chords.
//!
//! Shortcuts are stored with the Windows names. Command is the logical
//! primary modifier (`Ctrl`), Option is `Alt`, Shift stays Shift. The
//! Control key has no Windows counterpart and is left to the system, so a
//! press with Control is no Apricot shortcut. Special keys come from the
//! layout-independent key code, letters and digits from the typed character.

use apricot_core::shortcut::{ShortcutChord, ShortcutKey};

/// The modifier keys held during a key press.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // One flag per physical modifier key.
pub struct Modifiers {
    pub command: bool,
    pub option: bool,
    pub shift: bool,
    pub control: bool,
}

pub const KEY_RETURN: u16 = 36;
pub const KEY_TAB: u16 = 48;
pub const KEY_SPACE: u16 = 49;
pub const KEY_DELETE: u16 = 51;
pub const KEY_ESCAPE: u16 = 53;
pub const KEY_KEYPAD_ENTER: u16 = 76;
pub const KEY_F10: u16 = 109;
pub const KEY_HELP_INSERT: u16 = 114;
pub const KEY_HOME: u16 = 115;
pub const KEY_PAGE_UP: u16 = 116;
pub const KEY_FORWARD_DELETE: u16 = 117;
pub const KEY_END: u16 = 119;
pub const KEY_PAGE_DOWN: u16 = 121;
pub const KEY_LEFT: u16 = 123;
pub const KEY_RIGHT: u16 = 124;
pub const KEY_DOWN: u16 = 125;
pub const KEY_UP: u16 = 126;
pub const KEY_LEFT_BRACKET: u16 = 33;
pub const KEY_RIGHT_BRACKET: u16 = 30;

/// The function keys F1 to F20 by their macOS key codes.
const FUNCTION_KEYS: [(u16, u8); 20] = [
    (122, 1),
    (120, 2),
    (99, 3),
    (118, 4),
    (96, 5),
    (97, 6),
    (98, 7),
    (100, 8),
    (101, 9),
    (109, 10),
    (103, 11),
    (111, 12),
    (105, 13),
    (107, 14),
    (113, 15),
    (106, 16),
    (64, 17),
    (79, 18),
    (80, 19),
    (90, 20),
];

/// The chord of one key press, or `None` for keys Apricot never binds.
/// `characters` is the text of the key without modifiers
/// (`charactersIgnoringModifiers`).
#[must_use]
pub fn chord(key_code: u16, characters: &str, modifiers: Modifiers) -> Option<ShortcutChord> {
    if modifiers.control {
        return None;
    }
    let key = special_key(key_code).or_else(|| {
        let mut letters = characters.chars();
        let character = letters.next()?;
        (letters.next().is_none() && character.is_ascii_alphanumeric())
            .then(|| ShortcutKey::Character(character.to_ascii_uppercase()))
    })?;
    Some(ShortcutChord::new(
        modifiers.command,
        modifiers.shift,
        modifiers.option,
        key,
    ))
}

fn special_key(key_code: u16) -> Option<ShortcutKey> {
    if let Some((_, number)) = FUNCTION_KEYS.iter().find(|(code, _)| *code == key_code) {
        return Some(ShortcutKey::Function(*number));
    }
    Some(match key_code {
        KEY_RETURN | KEY_KEYPAD_ENTER => ShortcutKey::Enter,
        KEY_SPACE => ShortcutKey::Space,
        KEY_ESCAPE => ShortcutKey::Escape,
        // The Mac's Delete key does what Windows Delete does in lists
        // (remove the item); text fields keep it, as shortcuts that are not
        // global stay with a focused field.
        KEY_FORWARD_DELETE | KEY_DELETE => ShortcutKey::Delete,
        KEY_HELP_INSERT => ShortcutKey::Insert,
        KEY_HOME => ShortcutKey::Home,
        KEY_END => ShortcutKey::End,
        KEY_PAGE_UP => ShortcutKey::PageUp,
        KEY_PAGE_DOWN => ShortcutKey::PageDown,
        KEY_LEFT => ShortcutKey::Left,
        KEY_RIGHT => ShortcutKey::Right,
        KEY_UP => ShortcutKey::Up,
        KEY_DOWN => ShortcutKey::Down,
        KEY_LEFT_BRACKET => ShortcutKey::LeftBracket,
        KEY_RIGHT_BRACKET => ShortcutKey::RightBracket,
        _ => return None,
    })
}

/// Shift+F10 opens the context menu, as the Applications key does on
/// Windows; Macs have no Applications key.
#[must_use]
pub const fn is_context_menu_key(key_code: u16, modifiers: Modifiers) -> bool {
    key_code == KEY_F10
        && modifiers.shift
        && !modifiers.command
        && !modifiers.option
        && !modifiers.control
}

#[cfg(test)]
mod tests {
    use apricot_core::shortcut::{ShortcutChord, ShortcutKey};

    use super::{KEY_ESCAPE, KEY_F10, KEY_RETURN, Modifiers, chord, is_context_menu_key};

    #[test]
    fn command_is_the_primary_modifier_and_option_is_alt() {
        let modifiers = Modifiers {
            command: true,
            option: true,
            ..Modifiers::default()
        };
        assert_eq!(
            chord(16, "y", modifiers),
            ShortcutChord::parse("Ctrl+Alt+Y")
        );
        assert_eq!(
            chord(
                3,
                "f",
                Modifiers {
                    command: true,
                    ..Modifiers::default()
                }
            ),
            ShortcutChord::parse("Ctrl+F")
        );
        assert_eq!(
            chord(KEY_RETURN, "\r", Modifiers::default()),
            Some(ShortcutChord::new(false, false, false, ShortcutKey::Enter))
        );
        assert_eq!(
            chord(KEY_ESCAPE, "\u{1b}", Modifiers::default()),
            ShortcutChord::parse("Escape")
        );
        assert_eq!(
            chord(KEY_F10, "\u{f70d}", Modifiers::default()),
            ShortcutChord::parse("F10")
        );
    }

    #[test]
    fn the_control_key_is_left_to_the_system() {
        let modifiers = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        assert_eq!(chord(3, "f", modifiers), None);
    }

    #[test]
    fn shift_f10_is_the_context_menu() {
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        assert!(is_context_menu_key(KEY_F10, shift));
        assert!(!is_context_menu_key(KEY_F10, Modifiers::default()));
    }
}
