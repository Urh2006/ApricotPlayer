//! System-wide hotkeys. Windows registers them on the main window, which
//! then receives `WM_HOTKEY` with the hotkey's id.

#![cfg_attr(windows, allow(unsafe_code))]

use apricot_core::shortcut::ShortcutKey;

/// The modifiers and key of one hotkey; `system` is the Windows key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // One flag per modifier key.
pub struct Hotkey {
    pub system: bool,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: ShortcutKey,
}

/// The Windows virtual key of `key`.
#[must_use]
pub fn virtual_key(key: ShortcutKey) -> Option<u32> {
    Some(match key {
        ShortcutKey::Character(character) if character.is_ascii_alphanumeric() => {
            u32::from(character.to_ascii_uppercase())
        }
        ShortcutKey::Function(number @ 1..=24) => 0x70 + u32::from(number) - 1,
        ShortcutKey::Enter => 0x0D,
        ShortcutKey::Space => 0x20,
        ShortcutKey::Escape => 0x1B,
        ShortcutKey::Delete => 0x2E,
        ShortcutKey::Backspace => 0x08,
        ShortcutKey::Insert => 0x2D,
        ShortcutKey::Home => 0x24,
        ShortcutKey::End => 0x23,
        ShortcutKey::PageUp => 0x21,
        ShortcutKey::PageDown => 0x22,
        ShortcutKey::Left => 0x25,
        ShortcutKey::Right => 0x27,
        ShortcutKey::Up => 0x26,
        ShortcutKey::Down => 0x28,
        ShortcutKey::Applications => 0x5D,
        ShortcutKey::LeftBracket => 0xDB,
        ShortcutKey::RightBracket => 0xDD,
        _ => return None,
    })
}

/// Registers `hotkey` for `window` (an `HWND`) under `id`.
///
/// # Errors
///
/// Returns the system's message when another program already uses it.
#[cfg(windows)]
pub fn register(window: isize, id: i32, hotkey: Hotkey) -> Result<(), String> {
    use windows::Win32::{
        Foundation::HWND,
        UI::Input::KeyboardAndMouse::{
            HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
            RegisterHotKey,
        },
    };
    let key = virtual_key(hotkey.key).ok_or_else(|| "unsupported key".to_owned())?;
    let mut modifiers = HOT_KEY_MODIFIERS(MOD_NOREPEAT.0);
    for (on, flag) in [
        (hotkey.system, MOD_WIN),
        (hotkey.control, MOD_CONTROL),
        (hotkey.alt, MOD_ALT),
        (hotkey.shift, MOD_SHIFT),
    ] {
        if on {
            modifiers |= flag;
        }
    }
    // SAFETY: `window` is the live main window of this thread.
    unsafe { RegisterHotKey(Some(HWND(window as *mut _)), id, modifiers, key) }
        .map_err(|error| error.message())
}

/// Releases the hotkey `id` of `window`.
#[cfg(windows)]
pub fn unregister(window: isize, id: i32) {
    use windows::Win32::{Foundation::HWND, UI::Input::KeyboardAndMouse::UnregisterHotKey};
    // SAFETY: `window` is the live main window of this thread.
    let _ = unsafe { UnregisterHotKey(Some(HWND(window as *mut _)), id) };
}

/// Registers `hotkey` for `window` under `id`.
///
/// # Errors
///
/// Other systems register global hotkeys in their own frontends.
#[cfg(not(windows))]
pub fn register(_window: isize, _id: i32, _hotkey: Hotkey) -> Result<(), String> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported).to_string())
}

/// Releases the hotkey `id` of `window`.
#[cfg(not(windows))]
pub fn unregister(_window: isize, _id: i32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_windows_virtual_keys() {
        assert_eq!(virtual_key(ShortcutKey::Space), Some(0x20));
        assert_eq!(virtual_key(ShortcutKey::Function(9)), Some(0x78));
        assert_eq!(virtual_key(ShortcutKey::Character('a')), Some(0x41));
        assert_eq!(virtual_key(ShortcutKey::PageDown), Some(0x22));
    }
}
