//! Win32 key messages projected into the platform-neutral shortcut model.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use apricot_core::shortcut::{ShortcutChord, ShortcutKey};
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT},
    WindowsAndMessaging::{MSG, WM_KEYDOWN, WM_SYSKEYDOWN},
};

const VK_RETURN: usize = 0x0D;
const VK_SPACE: usize = 0x20;
const VK_ESCAPE: usize = 0x1B;
const VK_DELETE: usize = 0x2E;
const VK_BACK: usize = 0x08;
const VK_INSERT: usize = 0x2D;
const VK_HOME: usize = 0x24;
const VK_END: usize = 0x23;
const VK_PAGE_UP: usize = 0x21;
const VK_PAGE_DOWN: usize = 0x22;
const VK_LEFT: usize = 0x25;
const VK_UP: usize = 0x26;
const VK_RIGHT: usize = 0x27;
const VK_DOWN: usize = 0x28;
const VK_APPLICATIONS: usize = 0x5D;
const VK_LEFT_BRACKET: usize = 0xDB;
const VK_RIGHT_BRACKET: usize = 0xDD;
const VK_F1: usize = 0x70;
const VK_F24: usize = 0x87;

pub unsafe fn chord_from_message(message: &MSG) -> Option<ShortcutChord> {
    if !matches!(message.message, WM_KEYDOWN | WM_SYSKEYDOWN) {
        return None;
    }
    let key = key_from_virtual_code(message.wParam.0)?;
    Some(ShortcutChord::new(
        key_is_down(VK_CONTROL),
        key_is_down(VK_SHIFT),
        key_is_down(VK_MENU),
        key,
    ))
}

pub const fn is_repeat(message: &MSG) -> bool {
    message.lParam.0 & (1_isize << 30) != 0
}

fn key_is_down(key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(i32::from(key.0)).is_negative() }
}

fn key_from_virtual_code(key: usize) -> Option<ShortcutKey> {
    let key = match key {
        VK_RETURN => ShortcutKey::Enter,
        VK_SPACE => ShortcutKey::Space,
        VK_ESCAPE => ShortcutKey::Escape,
        VK_DELETE => ShortcutKey::Delete,
        VK_BACK => ShortcutKey::Backspace,
        VK_INSERT => ShortcutKey::Insert,
        VK_HOME => ShortcutKey::Home,
        VK_END => ShortcutKey::End,
        VK_PAGE_UP => ShortcutKey::PageUp,
        VK_PAGE_DOWN => ShortcutKey::PageDown,
        VK_LEFT => ShortcutKey::Left,
        VK_RIGHT => ShortcutKey::Right,
        VK_UP => ShortcutKey::Up,
        VK_DOWN => ShortcutKey::Down,
        VK_APPLICATIONS => ShortcutKey::Applications,
        VK_LEFT_BRACKET => ShortcutKey::LeftBracket,
        VK_RIGHT_BRACKET => ShortcutKey::RightBracket,
        VK_F1..=VK_F24 => ShortcutKey::Function(u8::try_from(key - VK_F1 + 1).ok()?),
        0x30..=0x39 | 0x41..=0x5A => ShortcutKey::Character(char::from(u8::try_from(key).ok()?)),
        _ => return None,
    };
    Some(key)
}

#[cfg(test)]
mod tests {
    use apricot_core::shortcut::ShortcutKey;

    use super::{VK_F1, VK_F24, key_from_virtual_code};

    #[test]
    fn virtual_key_projection_covers_shortcut_registry_shapes() {
        assert_eq!(
            key_from_virtual_code(0x41),
            Some(ShortcutKey::Character('A'))
        );
        assert_eq!(key_from_virtual_code(VK_F1), Some(ShortcutKey::Function(1)));
        assert_eq!(
            key_from_virtual_code(VK_F24),
            Some(ShortcutKey::Function(24))
        );
        assert_eq!(key_from_virtual_code(0xDB), Some(ShortcutKey::LeftBracket));
        assert_eq!(key_from_virtual_code(0xFF), None);
    }
}
