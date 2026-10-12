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

/// The texts of the labels in `view` (to print a message window).
pub fn texts(view: &objc2_app_kit::NSView) -> Vec<String> {
    let mut texts = Vec::new();
    for child in &view.subviews() {
        if let Some(field) = child.downcast_ref::<objc2_app_kit::NSTextField>() {
            texts.push(field.stringValue().to_string());
        }
        texts.extend(self::texts(&child));
    }
    texts
}

/// The transcript, comments and related video steps of
/// `APRICOT_MACOS_SELF_TEST_KIND=extras`.
pub fn youtube_extras_steps() -> Vec<(String, Option<Key>)> {
    use crate::keys;
    use objc2_app_kit::NSEventModifierFlags as Flags;
    let mut steps = Vec::new();
    let command_shift = Flags::Command | Flags::Shift;
    steps.push((
        "Command+Shift+T (transcript)".to_owned(),
        Some(Key {
            code: 17,
            text: "T",
            flags: command_shift,
        }),
    ));
    for _ in 0..14 {
        steps.push(("waiting in the transcript".to_owned(), None));
    }
    steps.push((
        "Command+Shift+M (comments)".to_owned(),
        Some(Key {
            code: 46,
            text: "M",
            flags: command_shift,
        }),
    ));
    for _ in 0..16 {
        steps.push(("waiting in the comments".to_owned(), None));
    }
    steps.push((
        "Command+Shift+Page Down (related video)".to_owned(),
        Some(Key {
            code: keys::KEY_PAGE_DOWN,
            text: "\u{f72d}",
            flags: command_shift,
        }),
    ));
    for _ in 0..12 {
        steps.push(("waiting for the related video".to_owned(), None));
    }
    steps
}

/// The steps of `APRICOT_MACOS_SELF_TEST=folder`.
#[allow(clippy::too_many_lines)] // One entry per scripted key.
pub fn folder_steps() -> Vec<(&'static str, Option<Key>)> {
    use crate::keys;
    let command_shift =
        objc2_app_kit::NSEventModifierFlags::Command | objc2_app_kit::NSEventModifierFlags::Shift;
    let with = |code, text, flags| Some(crate::self_test::Key { code, text, flags });
    vec![
        ("loaded", None),
        ("Down", Some(key(keys::KEY_DOWN, "\u{f701}"))),
        ("Enter (play)", Some(key(keys::KEY_RETURN, "\r"))),
        (
            "Escape (back to folder)",
            Some(key(keys::KEY_ESCAPE, "\u{1b}")),
        ),
        (
            "Command+Shift+Q (add to queue)",
            with(12, "Q", command_shift),
        ),
        ("Up", Some(key(keys::KEY_UP, "\u{f700}"))),
        (
            "Command+Shift+Q (add to queue)",
            with(12, "Q", command_shift),
        ),
        (
            "Command+Shift+Q (already added)",
            with(12, "Q", command_shift),
        ),
        (
            "Shift+F10 (context menu: Remove from playback queue)",
            with(
                keys::KEY_F10,
                "\u{f70d}",
                objc2_app_kit::NSEventModifierFlags::Shift,
            ),
        ),
        (
            "Command+Shift+Q (add to queue)",
            with(12, "Q", command_shift),
        ),
        (
            "Command+Option+Q (playback queue)",
            with(
                12,
                "q",
                objc2_app_kit::NSEventModifierFlags::Command
                    | objc2_app_kit::NSEventModifierFlags::Option,
            ),
        ),
        (
            "Command+P (add to a new playlist)",
            with(35, "p", objc2_app_kit::NSEventModifierFlags::Command),
        ),
        (
            "Command+P (already in the playlist)",
            with(35, "p", objc2_app_kit::NSEventModifierFlags::Command),
        ),
        (
            "Command+F (add favorite)",
            with(3, "f", objc2_app_kit::NSEventModifierFlags::Command),
        ),
        (
            "Command+F (already a favorite)",
            with(3, "f", objc2_app_kit::NSEventModifierFlags::Command),
        ),
        (
            "Command+Option+F (favorites)",
            with(
                3,
                "f",
                objc2_app_kit::NSEventModifierFlags::Command
                    | objc2_app_kit::NSEventModifierFlags::Option,
            ),
        ),
        ("Tab (Play)", Some(key(keys::KEY_TAB, "\t"))),
        (
            "Shift+Tab (list)",
            with(
                keys::KEY_TAB,
                "\t",
                objc2_app_kit::NSEventModifierFlags::Shift,
            ),
        ),
        ("Delete (remove)", Some(key(keys::KEY_DELETE, "\u{7f}"))),
        ("Escape (main menu)", Some(key(keys::KEY_ESCAPE, "\u{1b}"))),
        (
            "Command+Option+P (playlists)",
            with(
                35,
                "p",
                objc2_app_kit::NSEventModifierFlags::Command
                    | objc2_app_kit::NSEventModifierFlags::Option,
            ),
        ),
        ("Return (open playlist)", Some(key(keys::KEY_RETURN, "\r"))),
        (
            "Delete (remove from playlist)",
            Some(key(keys::KEY_DELETE, "\u{7f}")),
        ),
        (
            "Shift+Tab (Back)",
            with(
                keys::KEY_TAB,
                "\t",
                objc2_app_kit::NSEventModifierFlags::Shift,
            ),
        ),
        ("Escape (playlists)", Some(key(keys::KEY_ESCAPE, "\u{1b}"))),
        (
            "Delete (remove playlist)",
            Some(key(keys::KEY_DELETE, "\u{7f}")),
        ),
        ("Tab", Some(key(keys::KEY_TAB, "\t"))),
        ("Tab", Some(key(keys::KEY_TAB, "\t"))),
        ("Tab", Some(key(keys::KEY_TAB, "\t"))),
        ("Tab", Some(key(keys::KEY_TAB, "\t"))),
        (
            "Shift+Tab",
            Some(crate::self_test::Key {
                code: keys::KEY_TAB,
                text: "\t",
                flags: objc2_app_kit::NSEventModifierFlags::Shift,
            }),
        ),
        ("Space on Add to queue? no: focus report", None),
        ("Escape (main menu)", Some(key(keys::KEY_ESCAPE, "\u{1b}"))),
    ]
}

/// The steps of `APRICOT_MACOS_SELF_TEST=podcasts`: adding the feed
/// `APRICOT_MACOS_SELF_TEST_QUERY`, opening it, the played state, playing an
/// episode and going back.
pub fn podcast_steps() -> Vec<(String, Option<Key>)> {
    use crate::keys;
    let command_shift =
        objc2_app_kit::NSEventModifierFlags::Command | objc2_app_kit::NSEventModifierFlags::Shift;
    let named = |name: &str, key: Option<Key>| (name.to_owned(), key);
    let mut steps = vec![
        named("open podcasts", None),
        named("Add feed (prompt)", None),
    ];
    for _ in 0..12 {
        steps.push(named("waiting for the feed", None));
    }
    steps.push(named(
        "Return (open feed)",
        Some(key(keys::KEY_RETURN, "\r")),
    ));
    for _ in 0..4 {
        steps.push(named("waiting for the episodes", None));
    }
    steps.push(named("Down", Some(key(keys::KEY_DOWN, "\u{f701}"))));
    steps.push(named(
        "Command+Shift+X (played)",
        Some(Key {
            code: 7,
            text: "X",
            flags: command_shift,
        }),
    ));
    steps.push(named(
        "Command+Shift+X (unplayed)",
        Some(Key {
            code: 7,
            text: "X",
            flags: command_shift,
        }),
    ));
    steps.push(named("Up", Some(key(keys::KEY_UP, "\u{f700}"))));
    if std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref() == Ok("download") {
        steps.extend(download_steps());
        return steps;
    }
    if std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref() == Ok("playlist_download") {
        steps.extend(playlist_download_steps());
        return steps;
    }
    steps.push(named(
        "Return (play episode)",
        Some(key(keys::KEY_RETURN, "\r")),
    ));
    for _ in 0..12 {
        steps.push(named("waiting for playback", None));
    }
    steps.push(named(
        "Escape (episodes)",
        Some(key(keys::KEY_ESCAPE, "\u{1b}")),
    ));
    steps.push(named(
        "End (more episodes)",
        Some(key(keys::KEY_END, "\u{f72b}")),
    ));
    steps.push(named(
        "Escape (feeds)",
        Some(key(keys::KEY_ESCAPE, "\u{1b}")),
    ));
    steps.push(named(
        "Escape (main menu)",
        Some(key(keys::KEY_ESCAPE, "\u{1b}")),
    ));
    steps
}

/// The download steps of `APRICOT_MACOS_SELF_TEST=podcasts` with
/// `APRICOT_MACOS_SELF_TEST_KIND=download`: queue the selected episode, open
/// the downloads, download it and wait.
fn download_steps() -> Vec<(String, Option<Key>)> {
    use crate::keys;
    use objc2_app_kit::NSEventModifierFlags as Flags;
    let named = |name: &str, key: Option<Key>| (name.to_owned(), key);
    let mut steps = vec![
        named(
            "Shift+A (queue)",
            Some(Key {
                code: 0,
                text: "A",
                flags: Flags::Shift,
            }),
        ),
        named(
            "Command+Option+D (downloads)",
            Some(Key {
                code: 2,
                text: "d",
                flags: Flags::Command | Flags::Option,
            }),
        ),
        named(
            "Shift+Tab",
            Some(Key {
                code: keys::KEY_TAB,
                text: "\t",
                flags: Flags::Shift,
            }),
        ),
        named("Tab (list)", Some(key(keys::KEY_TAB, "\t"))),
        named("Return (download)", Some(key(keys::KEY_RETURN, "\r"))),
    ];
    for _ in 0..40 {
        steps.push(named("waiting for the download", None));
    }
    steps.push(named(
        "Escape (main menu)",
        Some(key(keys::KEY_ESCAPE, "\u{1b}")),
    ));
    steps
}

/// `APRICOT_MACOS_SELF_TEST_KIND=playlist_download`: the episode goes into
/// a new playlist, which is downloaded with the progress window.
fn playlist_download_steps() -> Vec<(String, Option<Key>)> {
    use crate::keys;
    use objc2_app_kit::NSEventModifierFlags as Flags;
    let named = |name: &str, key: Option<Key>| (name.to_owned(), key);
    let shift_tab = || {
        Some(Key {
            code: keys::KEY_TAB,
            text: "\t",
            flags: Flags::Shift,
        })
    };
    let mut steps = vec![
        named(
            "Command+P (add to a new playlist)",
            Some(Key {
                code: 35,
                text: "p",
                flags: Flags::Command,
            }),
        ),
        named(
            "Command+Option+P (playlists)",
            Some(Key {
                code: 35,
                text: "p",
                flags: Flags::Command | Flags::Option,
            }),
        ),
        named("Shift+Tab", shift_tab()),
        named("Shift+Tab (Download playlist)", shift_tab()),
        named("Return (download)", Some(key(keys::KEY_RETURN, "\r"))),
    ];
    for _ in 0..40 {
        steps.push(named("waiting for the download", None));
    }
    steps.push(named(
        "Escape (main menu)",
        Some(key(keys::KEY_ESCAPE, "\u{1b}")),
    ));
    steps
}

/// The keys for the converter of `APRICOT_MACOS_SELF_TEST=converter`: the
/// path, the next format, Replace and Convert.
pub fn converter_plan(file: bool) -> Vec<(&'static str, Key)> {
    use crate::keys;
    let variable = if file {
        "APRICOT_MACOS_SELF_TEST_MEDIA"
    } else {
        "APRICOT_MACOS_SELF_TEST_FOLDER"
    };
    let path = std::env::var(variable).unwrap_or_default();
    let path: &'static str = Box::leak(path.into_boxed_str());
    let mut plan: Vec<(&str, Key)> = path
        .char_indices()
        .map(|(offset, character)| ("type", key(0, &path[offset..offset + character.len_utf8()])))
        .collect();
    let tab = || key(keys::KEY_TAB, "\t");
    plan.extend([
        ("Tab (Browse)", tab()),
        ("Tab (format)", tab()),
        ("Down (next format)", key(keys::KEY_DOWN, "\u{f701}")),
        ("Tab", tab()),
        ("Tab", tab()),
        ("Space (replace)", key(keys::KEY_SPACE, " ")),
        ("Tab", tab()),
        ("Return (convert)", key(keys::KEY_RETURN, "\r")),
    ]);
    plan
}

/// The Action Finder keys of `APRICOT_MACOS_SELF_TEST=settings`: search
/// "favor" and run the first match.
pub fn action_finder_plan() -> Vec<(&'static str, Key)> {
    use crate::keys;
    vec![
        ("type f", key(0, "f")),
        ("type a", key(0, "a")),
        ("type v", key(0, "v")),
        ("type o", key(0, "o")),
        ("type r", key(0, "r")),
        ("Down (list)", key(keys::KEY_DOWN, "\u{f701}")),
        ("Return (run)", key(keys::KEY_RETURN, "\r")),
    ]
}

/// `APRICOT_MACOS_SELF_TEST=settings` with `KIND=spotify_queue`: rows, the
/// buttons in Tab order, the context menu (Remove, which fails without a
/// session and says the queue changed) and Escape.
pub fn spotify_queue_plan() -> Vec<(&'static str, Key)> {
    use crate::keys;
    let tab = || key(keys::KEY_TAB, "\t");
    vec![
        ("Down", key(keys::KEY_DOWN, "\u{f701}")),
        ("Up", key(keys::KEY_UP, "\u{f700}")),
        ("Tab (Play)", tab()),
        ("Tab (Move up)", tab()),
        ("Tab (Move down)", tab()),
        ("Tab (Remove)", tab()),
        ("Tab (Clear)", tab()),
        ("Tab (Back)", tab()),
        ("Tab (list)", tab()),
        (
            "Shift+F10 (menu, Remove)",
            Key {
                code: keys::KEY_F10,
                text: "\u{f70d}",
                flags: objc2_app_kit::NSEventModifierFlags::Shift,
            },
        ),
        ("Escape", key(keys::KEY_ESCAPE, "\u{1b}")),
    ]
}

/// The Settings keys of `APRICOT_MACOS_SELF_TEST=settings`: the second
/// section, Tab into it, two controls, Shift+Tab and Back.
pub fn settings_plan() -> Vec<(&'static str, Key)> {
    use crate::keys;
    let tab = || key(keys::KEY_TAB, "\t");
    vec![
        ("Down (section)", key(keys::KEY_DOWN, "\u{f701}")),
        ("Tab (into the section)", tab()),
        ("Tab", tab()),
        ("Tab", tab()),
        (
            "Shift+Tab",
            Key {
                code: keys::KEY_TAB,
                text: "\t",
                flags: objc2_app_kit::NSEventModifierFlags::Shift,
            },
        ),
        (
            "Shift+Tab",
            Key {
                code: keys::KEY_TAB,
                text: "\t",
                flags: objc2_app_kit::NSEventModifierFlags::Shift,
            },
        ),
        (
            "Shift+Tab (sections)",
            Key {
                code: keys::KEY_TAB,
                text: "\t",
                flags: objc2_app_kit::NSEventModifierFlags::Shift,
            },
        ),
        ("End (Shortcuts)", key(keys::KEY_END, "\u{f72b}")),
        ("Tab (actions)", tab()),
        ("Down (next action)", key(keys::KEY_DOWN, "\u{f701}")),
        ("Tab (capture)", tab()),
        (
            "Command+Shift+J (capture)",
            Key {
                code: 38,
                text: "j",
                flags: objc2_app_kit::NSEventModifierFlags::Command
                    | objc2_app_kit::NSEventModifierFlags::Shift,
            },
        ),
        (
            "Shift+Tab",
            Key {
                code: keys::KEY_TAB,
                text: "\t",
                flags: objc2_app_kit::NSEventModifierFlags::Shift,
            },
        ),
        ("Escape (Back)", key(keys::KEY_ESCAPE, "\u{1b}")),
    ]
}
