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

/// The accessibility value of the window's focused control: its value
/// description when it has one (sliders), else its value.
pub fn focused_value(window: &NSWindow) -> String {
    let Some(responder) = window.firstResponder() else {
        return String::new();
    };
    let responder: Retained<AnyObject> = Retained::into_super(Retained::into_super(responder));
    // SAFETY: NSView answers these NSAccessibility getters.
    unsafe {
        let description: Option<Retained<NSString>> =
            msg_send![&*responder, accessibilityValueDescription];
        if let Some(description) = description {
            return description.to_string();
        }
        if let Some(popup) = responder.downcast_ref::<objc2_app_kit::NSPopUpButton>() {
            return popup
                .titleOfSelectedItem()
                .map(|title| title.to_string())
                .unwrap_or_default();
        }
        let value: Option<Retained<AnyObject>> = msg_send![&*responder, accessibilityValue];
        value
            .map(|value| {
                let text: Retained<NSString> = msg_send![&*value, description];
                text.to_string()
            })
            .unwrap_or_default()
    }
}

/// The steps of `APRICOT_MACOS_SELF_TEST=tools`: name, key and whether the
/// key opens a modal window that `modalSelfTestKeys:` answers.
#[allow(clippy::too_many_lines)] // One line per scripted key.
pub fn tools_steps() -> Vec<(&'static str, Option<Key>, bool)> {
    use crate::keys;
    use objc2_app_kit::NSEventModifierFlags as Flags;
    let with = |code, text, flags| Some(Key { code, text, flags });
    let plain = |code, text| Some(key(code, text));
    let command_shift = Flags::Command | Flags::Shift;
    vec![
        ("opened", None, false),
        (
            "Option+Right (next chapter)",
            with(keys::KEY_RIGHT, "\u{f703}", Flags::Option),
            false,
        ),
        (
            "Option+Right (next chapter)",
            with(keys::KEY_RIGHT, "\u{f703}", Flags::Option),
            false,
        ),
        (
            "Option+Left (previous chapter)",
            with(keys::KEY_LEFT, "\u{f702}", Flags::Option),
            false,
        ),
        (
            "Command+Shift+G (ReplayGain)",
            with(5, "G", command_shift),
            false,
        ),
        ("B (BPM)", plain(11, "b"), false),
        ("waiting for BPM", None, false),
        ("waiting for BPM", None, false),
        ("L (copy path)", plain(37, "l"), false),
        (
            "Command+Shift+L (timestamp link)",
            with(37, "L", command_shift),
            false,
        ),
        (
            "Command+D (stream URL)",
            with(2, "d", Flags::Command),
            false,
        ),
        (
            "Command+Shift+C (chapter list)",
            with(8, "C", command_shift),
            true,
        ),
        ("after the chapter list", None, false),
        ("[ (clip start)", plain(keys::KEY_LEFT_BRACKET, "["), false),
        (
            "Option+Left (previous chapter)",
            with(keys::KEY_LEFT, "\u{f702}", Flags::Option),
            false,
        ),
        (
            "] (clip end before start)",
            plain(keys::KEY_RIGHT_BRACKET, "]"),
            false,
        ),
        ("P (preview)", plain(35, "p"), false),
        (
            "[ (clear clip start)",
            plain(keys::KEY_LEFT_BRACKET, "["),
            false,
        ),
        ("[ (clip start)", plain(keys::KEY_LEFT_BRACKET, "["), false),
        (
            "Option+Right (next chapter)",
            with(keys::KEY_RIGHT, "\u{f703}", Flags::Option),
            false,
        ),
        ("] (clip end)", plain(keys::KEY_RIGHT_BRACKET, "]"), false),
        (
            "Option+Left (previous chapter)",
            with(keys::KEY_LEFT, "\u{f702}", Flags::Option),
            false,
        ),
        ("P (preview)", plain(35, "p"), false),
        ("waiting for the preview", None, false),
        ("E (edit mode)", plain(14, "e"), false),
        ("D (speed up)", plain(2, "d"), false),
        ("Command+S (save copy)", with(1, "s", Flags::Command), false),
        ("waiting for the save", None, false),
        ("waiting for the save", None, false),
        ("waiting for the save", None, false),
        ("E (edit mode off)", plain(14, "e"), false),
        (
            "Command+Shift+B (add bookmark)",
            with(11, "B", command_shift),
            true,
        ),
        ("after the name prompt", None, false),
        (
            "Command+Shift+B (add bookmark, Escape)",
            with(11, "B", command_shift),
            true,
        ),
        ("after the cancelled prompt", None, false),
        (
            "Command+Shift+K (bookmarks)",
            with(40, "K", command_shift),
            true,
        ),
        ("after the bookmarks", None, false),
        (
            "Command+Shift+Y (lyrics)",
            with(16, "Y", command_shift),
            true,
        ),
        ("after the lyrics", None, false),
    ]
}
