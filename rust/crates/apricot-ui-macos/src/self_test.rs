//! A scripted keyboard check of the main window for development builds:
//! `APRICOT_MACOS_SELF_TEST=1 apricot-player` sends real key presses to the
//! window, prints what the user would get (focused control, selected row,
//! status line) and quits. It covers what can be checked without VoiceOver.

#![allow(unsafe_code)]

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSWindow};
use objc2_foundation::{NSPoint, NSString};

/// One scripted key press: key code, typed text and modifiers.
#[derive(Clone, Copy)]
pub struct Key {
    pub code: u16,
    pub text: &'static str,
    pub flags: NSEventModifierFlags,
}

pub const fn key(code: u16, text: &'static str) -> Key {
    Key {
        code,
        text,
        flags: NSEventModifierFlags::empty(),
    }
}

pub fn send(window: &NSWindow, key: &Key) {
    let characters = NSString::from_str(key.text);
    for kind in [NSEventType::KeyDown, NSEventType::KeyUp] {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind,
            NSPoint::new(0.0, 0.0),
            key.flags,
            0.0,
            window.windowNumber(),
            None,
            &characters,
            &characters,
            false,
            key.code,
        );
        if let Some(event) = event {
            NSApplication::sharedApplication(MainThreadMarker::from(window)).sendEvent(&event);
        }
    }
}

/// The class and accessibility label of the window's focused control.
pub fn focus(window: &NSWindow) -> String {
    let Some(responder) = window.firstResponder() else {
        return "nothing".to_owned();
    };
    let responder: Retained<AnyObject> = Retained::into_super(Retained::into_super(responder));
    // SAFETY: every responder in the window answers these accessibility
    // getters (NSAccessibility protocol of NSView).
    unsafe {
        let label: Option<Retained<NSString>> = msg_send![&*responder, accessibilityLabel];
        format!(
            "{} {:?}",
            responder.class().name().to_string_lossy(),
            label.map(|label| label.to_string()).unwrap_or_default()
        )
    }
}
