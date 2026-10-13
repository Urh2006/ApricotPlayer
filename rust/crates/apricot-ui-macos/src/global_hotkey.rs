//! Carbon hot-key registration receives only the configured chord; it does
//! not monitor other applications' input or require Accessibility permission.
#![allow(unsafe_code)]
use apricot_core::shortcut::{ShortcutChord, ShortcutKey};
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel};
use std::{ffi::c_void, ptr};

type Ref = *mut c_void;
#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}
#[repr(C)]
#[derive(Default)]
struct HotKeyId {
    signature: u32,
    id: u32,
}
const SIGNATURE: u32 = u32::from_be_bytes(*b"Apr8");
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> Ref;
    fn InstallEventHandler(
        target: Ref,
        callback: unsafe extern "C" fn(Ref, Ref, Ref) -> i32,
        count: u32,
        events: *const EventType,
        data: Ref,
        handler: *mut Ref,
    ) -> i32;
    fn RemoveEventHandler(handler: Ref) -> i32;
    fn RegisterEventHotKey(
        key: u32,
        modifiers: u32,
        id: HotKeyId,
        target: Ref,
        options: u32,
        hotkey: *mut Ref,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey: Ref) -> i32;
    fn GetEventParameter(
        event: Ref,
        name: u32,
        kind: u32,
        actual: *mut u32,
        size: u32,
        actual_size: *mut u32,
        data: Ref,
    ) -> i32;
}
unsafe extern "C" fn pressed(_: Ref, event: Ref, data: Ref) -> i32 {
    let mut id = HotKeyId::default();
    // SAFETY: Carbon provides a live event; the output is an eight-byte HotKeyId.
    let status = unsafe {
        GetEventParameter(
            event,
            u32::from_be_bytes(*b"----"),
            u32::from_be_bytes(*b"hkid"),
            ptr::null_mut(),
            8,
            ptr::null_mut(),
            (&raw mut id).cast(),
        )
    };
    if status != 0 || id.signature != SIGNATURE || id.id != 1 {
        return -9874;
    }
    // SAFETY: registration is owned by the controller and removed before its
    // destruction. Carbon dispatches the callback on the application thread.
    // Schedule the Objective-C action so Rust never unwinds through Carbon.
    unsafe {
        let target = &*data.cast::<AnyObject>();
        let _: () = msg_send![target, performSelector: sel!(globalPlayPause:), withObject: Option::<&AnyObject>::None, afterDelay: 0.0_f64];
    }
    0
}

pub struct GlobalHotKey {
    handler: Ref,
    hotkey: Ref,
}
impl GlobalHotKey {
    pub fn install(target: &AnyObject, chord: ShortcutChord) -> Result<Self, String> {
        let (key, modifiers) =
            carbon_chord(chord).ok_or_else(|| "unsupported global play/pause chord".to_owned())?;
        let event = EventType {
            class: u32::from_be_bytes(*b"keyb"),
            kind: 5,
        };
        let mut registration = Self {
            handler: ptr::null_mut(),
            hotkey: ptr::null_mut(),
        };
        // SAFETY: pointer lifetimes are tied to the controller's registration;
        // Carbon copies the event spec and returns owned registration handles.
        unsafe {
            let event_target = GetApplicationEventTarget();
            let status = InstallEventHandler(
                event_target,
                pressed,
                1,
                &raw const event,
                std::ptr::from_ref(target).cast_mut().cast(),
                &raw mut registration.handler,
            );
            if status != 0 {
                return Err(format!("global play/pause handler: {status}"));
            }
            // Exclusive registration refuses an existing owner's chord.
            let status = RegisterEventHotKey(
                key,
                modifiers,
                HotKeyId {
                    signature: SIGNATURE,
                    id: 1,
                },
                event_target,
                1,
                &raw mut registration.hotkey,
            );
            if status != 0 {
                return Err(format!("global play/pause registration: {status}"));
            }
        }
        Ok(registration)
    }
}
impl Drop for GlobalHotKey {
    fn drop(&mut self) {
        // SAFETY: each non-null handle belongs to this registration alone.
        unsafe {
            if !self.hotkey.is_null() {
                UnregisterEventHotKey(self.hotkey);
            }
            if !self.handler.is_null() {
                RemoveEventHandler(self.handler);
            }
        }
    }
}

pub fn carbon_chord(chord: ShortcutChord) -> Option<(u32, u32)> {
    if !(chord.control || chord.alt) {
        return None;
    }
    let key = match chord.key {
        ShortcutKey::Space => 49,
        // Letter hotkeys require translating the active keyboard layout.
        // Refuse them rather than silently registering the wrong physical key.
        ShortcutKey::Function(number) => {
            let codes = [
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
            codes.iter().find(|(_, function)| *function == number)?.0
        }
        ShortcutKey::Enter => 36,
        ShortcutKey::Escape => 53,
        ShortcutKey::Left => 123,
        ShortcutKey::Right => 124,
        ShortcutKey::Down => 125,
        ShortcutKey::Up => 126,
        ShortcutKey::Home => 115,
        ShortcutKey::End => 119,
        ShortcutKey::PageUp => 116,
        ShortcutKey::PageDown => 121,
        _ => return None,
    };
    let modifiers =
        u32::from(chord.control) * 256 + u32::from(chord.shift) * 512 + u32::from(chord.alt) * 2048;
    Some((key, modifiers))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_and_custom_chords_map_to_carbon_modifiers() {
        assert_eq!(
            carbon_chord(ShortcutChord::parse("Ctrl+Shift+Space").unwrap()),
            Some((49, 768))
        );
        assert_eq!(
            carbon_chord(ShortcutChord::parse("Command+Option+PageUp").unwrap()),
            Some((116, 2304))
        );
        assert_eq!(carbon_chord(ShortcutChord::parse("Space").unwrap()), None);
    }
}
