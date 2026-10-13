//! Carbon hot-key registration receives only the configured chord; it does
//! not monitor other applications' input or require Accessibility permission.
#![allow(unsafe_code)]
use apricot_core::shortcut::{ShortcutChord, ShortcutKey};
use objc2::runtime::AnyObject;
use objc2::{msg_send, sel};
use std::{cell::Cell, ffi::c_void, ptr};

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
struct CallbackState {
    target: *const AnyObject,
    down: Cell<bool>,
}
const SIGNATURE: u32 = u32::from_be_bytes(*b"Apr8");
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> Ref;
    fn GetEventKind(event: Ref) -> u32;
    #[cfg(debug_assertions)]
    fn CreateEvent(
        allocator: Ref,
        class: u32,
        kind: u32,
        time: f64,
        attributes: u32,
        event: *mut Ref,
    ) -> i32;
    #[cfg(debug_assertions)]
    fn SetEventParameter(event: Ref, name: u32, kind: u32, size: u32, data: *const c_void) -> i32;
    #[cfg(debug_assertions)]
    fn SendEventToEventTarget(event: Ref, target: Ref) -> i32;
    #[cfg(debug_assertions)]
    fn ReleaseEvent(event: Ref);
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
    // SAFETY: the boxed state remains at a stable address until the handler
    // is removed, and Carbon dispatches these events on the UI thread.
    let state = unsafe { &*data.cast::<CallbackState>() };
    match unsafe { GetEventKind(event) } {
        6 => {
            state.down.set(false);
            return 0;
        }
        5 if state.down.replace(true) => return 0,
        5 => {}
        _ => return -9874,
    }
    // SAFETY: registration is owned by the controller and removed before its
    // destruction. Carbon dispatches the callback on the application thread.
    // Schedule the Objective-C action so Rust never unwinds through Carbon.
    unsafe {
        let target = &*state.target;
        let _: () = msg_send![target, performSelector: sel!(globalPlayPause:), withObject: Option::<&AnyObject>::None, afterDelay: 0.0_f64];
    }
    0
}

pub struct GlobalHotKey {
    handler: Ref,
    hotkey: Ref,
    callback: Box<CallbackState>,
}
impl GlobalHotKey {
    pub fn install(target: &AnyObject, chord: ShortcutChord) -> Result<Self, String> {
        let (key, modifiers) =
            carbon_chord(chord).ok_or_else(|| "unsupported global play/pause chord".to_owned())?;
        let events = [
            EventType {
                class: u32::from_be_bytes(*b"keyb"),
                kind: 5,
            },
            EventType {
                class: u32::from_be_bytes(*b"keyb"),
                kind: 6,
            },
        ];
        let mut registration = Self {
            handler: ptr::null_mut(),
            hotkey: ptr::null_mut(),
            callback: Box::new(CallbackState {
                target: std::ptr::from_ref(target),
                down: Cell::new(false),
            }),
        };
        // SAFETY: pointer lifetimes are tied to the controller's registration;
        // Carbon copies the event spec and returns owned registration handles.
        unsafe {
            let event_target = GetApplicationEventTarget();
            let status = InstallEventHandler(
                event_target,
                pressed,
                2,
                events.as_ptr(),
                std::ptr::from_ref(registration.callback.as_ref())
                    .cast_mut()
                    .cast(),
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
    let key = match chord.key {
        ShortcutKey::Space => 49,
        ShortcutKey::Character(character) => crate::keyboard_layout::virtual_key(character)?,
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
        ShortcutKey::Delete | ShortcutKey::Backspace => 51,
        ShortcutKey::Insert => 114,
        ShortcutKey::LeftBracket => 33,
        ShortcutKey::RightBracket => 30,
        ShortcutKey::Left => 123,
        ShortcutKey::Right => 124,
        ShortcutKey::Down => 125,
        ShortcutKey::Up => 126,
        ShortcutKey::Home => 115,
        ShortcutKey::End => 119,
        ShortcutKey::PageUp => 116,
        ShortcutKey::PageDown => 121,
        ShortcutKey::Applications => return None,
    };
    let modifiers =
        u32::from(chord.control) * 256 + u32::from(chord.shift) * 512 + u32::from(chord.alt) * 2048;
    Some((key, modifiers))
}

/// Internal regression input for the isolated native probe, not keyboard injection.
#[cfg(debug_assertions)]
pub fn audit_event(kind: u32) -> Result<(), String> {
    let mut event = ptr::null_mut();
    let id = HotKeyId {
        signature: SIGNATURE,
        id: 1,
    };
    // SAFETY: the native probe calls on the UI thread and releases its event.
    unsafe {
        let status = CreateEvent(
            ptr::null_mut(),
            u32::from_be_bytes(*b"keyb"),
            kind,
            0.0,
            0,
            &raw mut event,
        );
        if status != 0 {
            return Err(format!("create test event: {status}"));
        }
        let mut status = SetEventParameter(
            event,
            u32::from_be_bytes(*b"----"),
            u32::from_be_bytes(*b"hkid"),
            8,
            std::ptr::from_ref(&id).cast(),
        );
        if status == 0 {
            status = SendEventToEventTarget(event, GetApplicationEventTarget());
        }
        ReleaseEvent(event);
        if status != 0 {
            return Err(format!("dispatch test event: {status}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_function_key_without_primary_modifier_is_supported() {
        assert_eq!(
            carbon_chord(ShortcutChord::parse("F5").unwrap()),
            Some((96, 0))
        );
        assert_eq!(
            carbon_chord(ShortcutChord::parse("Shift+F5").unwrap()),
            Some((96, 512))
        );
    }

    #[test]
    fn configured_character_hotkey_has_a_native_registration_mapping() {
        assert!(carbon_chord(ShortcutChord::parse("Ctrl+Shift+K").unwrap()).is_some());
    }

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
        assert_eq!(
            carbon_chord(ShortcutChord::parse("Space").unwrap()),
            Some((49, 0))
        );
    }
}
