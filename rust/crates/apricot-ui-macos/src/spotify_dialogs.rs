//! The Spotify login dialog (`spotify_login_win32.rs`: the read-only status
//! with focus, Open browser again, Copy login link, Try again and Cancel)
//! and the Spotify settings dialog (`spotify_settings_win32.rs`: quality,
//! normalisation, autoplay, library order, OK and Cancel), modal over the
//! main window.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use apricot_shell::{
    SpotifyLoginAction, SpotifyLoginTexts, SpotifyLoginUpdate, SpotifySettingsTexts,
};
use apricot_spotify::settings::{Autoplay, LibraryOrder, Quality, SpotifySettings};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertSecondButtonReturn, NSApplication, NSBackingStoreType, NSScrollView,
    NSTextField, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::controls::{self, Button, ReadOnlyText};
use crate::form_controls::{CheckBox, PopUp};
use crate::window::MainWindowController;

const MARGIN: f64 = 12.0;
const BUTTON_HEIGHT: f64 = 30.0;

thread_local! {
    /// The open login dialog, for updates from the Spotify events.
    static LOGIN: RefCell<Option<Retained<LoginDialog>>> = const { RefCell::new(None) };
}

pub struct LoginIvars {
    controller: Weak<MainWindowController>,
    window: RefCell<Option<Retained<NSWindow>>>,
    status: RefCell<Option<Retained<ReadOnlyText>>>,
    retry: RefCell<Option<Retained<Button>>>,
    waiting: RefCell<String>,
    succeeded: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSpotifyLogin"]
    #[ivars = LoginIvars]
    struct LoginDialog;

    unsafe impl NSObjectProtocol for LoginDialog {}

    unsafe impl NSWindowDelegate for LoginDialog {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            self.cancel_login();
            false
        }
    }

    impl LoginDialog {
        #[unsafe(method(openBrowser:))]
        fn open_browser(&self, _sender: Option<&AnyObject>) {
            self.action(SpotifyLoginAction::OpenBrowser);
        }

        #[unsafe(method(copyLink:))]
        fn copy_link(&self, _sender: Option<&AnyObject>) {
            self.action(SpotifyLoginAction::CopyLink);
        }

        #[unsafe(method(retry:))]
        fn retry(&self, _sender: Option<&AnyObject>) {
            self.action(SpotifyLoginAction::Retry);
        }

        #[unsafe(method(cancel:))]
        fn cancel(&self, _sender: Option<&AnyObject>) {
            self.cancel_login();
        }
    }
);

impl LoginDialog {
    fn action(&self, action: SpotifyLoginAction) {
        if let Some(controller) = self.ivars().controller.load() {
            controller.with_shell(|shell, ui| shell.spotify_login_action(ui, action));
        }
    }

    /// Windows `cancel`: the late result is stale and the dialog closes.
    fn cancel_login(&self) {
        self.action(SpotifyLoginAction::Cancel);
        NSApplication::sharedApplication(self.mtm()).stopModal();
    }

    /// Windows `update`.
    fn update(&self, update: &SpotifyLoginUpdate) {
        let window = self.ivars().window.borrow().clone();
        match update {
            SpotifyLoginUpdate::Succeeded => {
                self.ivars().succeeded.set(true);
                NSApplication::sharedApplication(self.mtm()).stopModal();
            }
            SpotifyLoginUpdate::Waiting => {
                if let Some(status) = self.ivars().status.borrow().as_ref() {
                    status.set_text(&self.ivars().waiting.borrow());
                }
                if let Some(retry) = self.ivars().retry.borrow().as_ref() {
                    retry.setEnabled(false);
                }
                if let (Some(window), Some(status)) =
                    (window, self.ivars().status.borrow().as_ref())
                {
                    window.makeFirstResponder(Some(status));
                }
            }
            SpotifyLoginUpdate::Failed(message) => {
                if let Some(status) = self.ivars().status.borrow().as_ref() {
                    status.set_text(message);
                }
                if let Some(retry) = self.ivars().retry.borrow().as_ref() {
                    retry.setEnabled(true);
                    if let Some(window) = window {
                        window.makeFirstResponder(Some(retry));
                    }
                }
            }
        }
    }
}

/// Forwards login progress to the open dialog.
pub fn login_update(update: &SpotifyLoginUpdate) {
    let open = LOGIN.with(|open| open.borrow().clone());
    if let Some(dialog) = open {
        dialog.update(update);
    }
}

fn standard_window(
    mtm: MainThreadMarker,
    title: &str,
    width: f64,
    height: f64,
) -> Retained<NSWindow> {
    // SAFETY: a standard titled window with a buffered backing.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(300.0, 200.0), NSSize::new(width, height)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is released by its last owner, the caller.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(title));
    window
}

/// Windows `spotify_login_win32::show`; the outcome goes to
/// `Shell::spotify_login_closed`.
pub fn run_login(controller: &MainWindowController, texts: &SpotifyLoginTexts) {
    let mtm = MainThreadMarker::from(controller);
    let (width, height) = (560.0, 240.0);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let this: Retained<LoginDialog> = {
        let this = LoginDialog::alloc(mtm).set_ivars(LoginIvars {
            controller: Weak::from(controller),
            window: RefCell::new(None),
            status: RefCell::new(None),
            retry: RefCell::new(None),
            waiting: RefCell::new(texts.waiting.clone()),
            succeeded: Cell::new(false),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let target: &AnyObject = this.as_ref();
    let window = standard_window(mtm, &texts.title, width, height);
    window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
    let content = window.contentView().expect("a window has a content view");
    let text_height = height - 3.0 * MARGIN - BUTTON_HEIGHT;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, 2.0 * MARGIN + BUTTON_HEIGHT),
            NSSize::new(width - 2.0 * MARGIN, text_height),
        ),
    );
    let status = ReadOnlyText::new(mtm);
    status.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(width - 2.0 * MARGIN, text_height),
    ));
    status.set_label(&texts.status_name);
    status.set_text(&texts.waiting);
    scroll.setDocumentView(Some(&status));
    content.addSubview(&scroll);
    let mut x = MARGIN;
    let mut button = |label: &str, action| {
        let button = Button::new(mtm, label, target, action);
        let button_width = 130.0;
        button.setFrame(NSRect::new(
            NSPoint::new(x, MARGIN),
            NSSize::new(button_width, BUTTON_HEIGHT),
        ));
        x += button_width + 6.0;
        content.addSubview(&button);
        button
    };
    let open = button(&texts.open_browser, sel!(openBrowser:));
    let copy = button(&texts.copy_link, sel!(copyLink:));
    let retry = button(&texts.retry, sel!(retry:));
    let cancel = button(&texts.cancel, sel!(cancel:));
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    // Try again only after a failure.
    retry.setEnabled(false);
    controls::set_tab_order(&[&*status as &NSView, &open, &copy, &retry, &cancel]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&status));
    *this.ivars().window.borrow_mut() = Some(window.clone());
    *this.ivars().status.borrow_mut() = Some(status.clone());
    *this.ivars().retry.borrow_mut() = Some(retry);
    LOGIN.with(|open| *open.borrow_mut() = Some(this.clone()));
    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&status));
    app.runModalForWindow(&window);
    LOGIN.with(|open| *open.borrow_mut() = None);
    window.setDelegate(None);
    window.orderOut(None);
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    let succeeded = this.ivars().succeeded.get();
    controller.with_shell(|shell, ui| shell.spotify_login_closed(ui, succeeded));
}

pub struct SettingsIvars {
    accepted: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSpotifySettings"]
    #[ivars = SettingsIvars]
    struct SettingsDialog;

    unsafe impl NSObjectProtocol for SettingsDialog {}

    unsafe impl NSWindowDelegate for SettingsDialog {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    impl SettingsDialog {
        #[unsafe(method(accept:))]
        fn accept(&self, _sender: Option<&AnyObject>) {
            self.ivars().accepted.set(true);
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(cancel:))]
        fn cancel(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(changed:))]
        fn changed(&self, _sender: Option<&AnyObject>) {}
    }
);

/// No is the default choice; an explicit Yes is required for opt-in.
pub(crate) fn confirm_history(
    mtm: MainThreadMarker,
    message: &str,
    yes: &str,
    no: &str,
    cancel: &str,
) -> bool {
    let app = NSApplication::sharedApplication(mtm);
    let window = app.keyWindow();
    let previous = window.as_ref().and_then(|window| window.firstResponder());
    let alert = history_alert(mtm, message, yes, no, cancel);
    let answer = alert.runModal();
    if let (Some(window), Some(previous)) = (window, previous) {
        window.makeKeyAndOrderFront(None);
        window.makeFirstResponder(Some(&previous));
    }
    answer == NSAlertSecondButtonReturn
}

fn history_alert(
    mtm: MainThreadMarker,
    message: &str,
    yes: &str,
    no: &str,
    cancel: &str,
) -> Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(message));
    alert.addButtonWithTitle(&NSString::from_str(no));
    alert
        .addButtonWithTitle(&NSString::from_str(yes))
        .setKeyEquivalent(&NSString::from_str(""));
    alert
        .addButtonWithTitle(&NSString::from_str(cancel))
        .setKeyEquivalent(&NSString::from_str("\u{1b}"));
    alert
}

/// Native QA of the warning only; it never changes settings or reports audio.
#[cfg(debug_assertions)]
pub(crate) fn audit_history_prompt(mtm: MainThreadMarker) -> Result<(), String> {
    use objc2_app_kit::{NSAlertFirstButtonReturn, NSAlertThirdButtonReturn};
    use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};
    for (index, expected) in [
        (0, NSAlertFirstButtonReturn),
        (1, NSAlertSecondButtonReturn),
        (2, NSAlertThirdButtonReturn),
    ] {
        let alert = history_alert(
            mtm,
            "Experimental Spotify account-risk warning (isolated QA)",
            "Yes",
            "No",
            "Cancel",
        );
        let buttons = alert.buttons();
        let labels: Vec<_> = buttons
            .iter()
            .map(|button| button.title().to_string())
            .collect();
        if labels != ["No", "Yes", "Cancel"] {
            return Err(format!("Unexpected warning labels: {labels:?}"));
        }
        if alert
            .window()
            .defaultButtonCell()
            .is_none_or(|cell| cell.title().to_string() != "No")
        {
            return Err("No must be the default warning choice".into());
        }
        let button = buttons.objectAtIndex(index);
        // SAFETY: This is an owned NSButton with the documented performClick:
        // action. The timer passes its own object as the optional sender.
        let timer = unsafe {
            NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                0.05,
                &button,
                sel!(performClick:),
                None,
                false,
            )
        };
        // SAFETY: An owned main-thread timer targets the owned alert button;
        // the system common-mode constant is valid and the timer is invalidated below.
        unsafe {
            NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
        }
        let answer = alert.runModal();
        timer.invalidate();
        if answer != expected {
            return Err(format!("Unexpected warning response {answer}"));
        }
    }
    println!(
        "native-probe PASS: experimental warning No default, Yes and Cancel; no settings/account changes"
    );
    Ok(())
}

/// Windows `spotify_settings_win32::show`: the chosen settings, `None`
/// when cancelled; focus returns where it was.
#[allow(clippy::too_many_lines)] // The dialog's controls in their Tab order.
pub fn run_settings(
    mtm: MainThreadMarker,
    texts: &SpotifySettingsTexts,
    current: SpotifySettings,
) -> Option<SpotifySettings> {
    let (width, height) = (460.0, 322.0);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let this: Retained<SettingsDialog> = {
        let this = SettingsDialog::alloc(mtm).set_ivars(SettingsIvars {
            accepted: Cell::new(false),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let target: &AnyObject = this.as_ref();
    let window = standard_window(mtm, &texts.title, width, height);
    window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
    let content = window.contentView().expect("a window has a content view");
    let label_width = 180.0;
    let field_x = MARGIN + label_width + 8.0;
    let field_width = width - field_x - MARGIN;
    let mut y = height - MARGIN - 28.0;
    let mut row = |label: &str, options: &[String], selected: usize| {
        let text = NSTextField::labelWithString(&NSString::from_str(label), mtm);
        text.setFrame(NSRect::new(
            NSPoint::new(MARGIN, y + 4.0),
            NSSize::new(label_width, 22.0),
        ));
        content.addSubview(&text);
        let popup = PopUp::new(mtm, label, target, sel!(changed:));
        popup.set_items(options, selected);
        popup.setFrame(NSRect::new(
            NSPoint::new(field_x, y),
            NSSize::new(field_width, 28.0),
        ));
        content.addSubview(&popup);
        y -= 36.0;
        popup
    };
    let quality = row(
        &texts.quality,
        &texts.qualities,
        Quality::ALL
            .iter()
            .position(|quality| *quality == current.quality)
            .unwrap_or(2),
    );
    let normalisation = CheckBox::new(mtm, &texts.normalisation, target, sel!(changed:));
    normalisation.set_checked(current.normalisation);
    normalisation.setFrame(NSRect::new(
        NSPoint::new(field_x, y),
        NSSize::new(field_width, 28.0),
    ));
    content.addSubview(&normalisation);
    y -= 36.0;
    let mut row = |label: &str, options: &[String], selected: usize| {
        let text = NSTextField::labelWithString(&NSString::from_str(label), mtm);
        text.setFrame(NSRect::new(
            NSPoint::new(MARGIN, y + 4.0),
            NSSize::new(label_width, 22.0),
        ));
        content.addSubview(&text);
        let popup = PopUp::new(mtm, label, target, sel!(changed:));
        popup.set_items(options, selected);
        popup.setFrame(NSRect::new(
            NSPoint::new(field_x, y),
            NSSize::new(field_width, 28.0),
        ));
        content.addSubview(&popup);
        y -= 36.0;
        popup
    };
    let autoplay = row(
        &texts.autoplay,
        &texts.autoplays,
        Autoplay::ALL
            .iter()
            .position(|autoplay| *autoplay == current.autoplay)
            .unwrap_or(0),
    );
    let order = row(
        &texts.order,
        &texts.orders,
        LibraryOrder::ALL
            .iter()
            .position(|order| *order == current.library_order)
            .unwrap_or(0),
    );
    let history = CheckBox::new(mtm, &texts.listening_history, target, sel!(changed:));
    history.set_checked(current.listening_history);
    history.setFrame(NSRect::new(
        NSPoint::new(MARGIN, y),
        NSSize::new(width - 2.0 * MARGIN, 28.0),
    ));
    content.addSubview(&history);
    let key_cache = CheckBox::new(mtm, &texts.key_cache, target, sel!(changed:));
    key_cache.set_checked(current.key_cache);
    key_cache.setFrame(NSRect::new(
        NSPoint::new(MARGIN, y - 36.0),
        NSSize::new(width - 2.0 * MARGIN, 28.0),
    ));
    content.addSubview(&key_cache);
    let ok = Button::new(mtm, &texts.ok, target, sel!(accept:));
    ok.setFrame(NSRect::new(
        NSPoint::new(width - MARGIN - 2.0 * 110.0 - 8.0, MARGIN),
        NSSize::new(110.0, BUTTON_HEIGHT),
    ));
    // Windows `BS_DEFPUSHBUTTON`.
    ok.setKeyEquivalent(&NSString::from_str("\r"));
    content.addSubview(&ok);
    let cancel = Button::new(mtm, &texts.cancel, target, sel!(cancel:));
    cancel.setFrame(NSRect::new(
        NSPoint::new(width - MARGIN - 110.0, MARGIN),
        NSSize::new(110.0, BUTTON_HEIGHT),
    ));
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&cancel);
    controls::set_tab_order(&[
        &*quality as &NSView,
        &normalisation,
        &autoplay,
        &order,
        &history,
        &key_cache,
        &ok,
        &cancel,
    ]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&quality));
    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&quality));
    loop {
        app.runModalForWindow(&window);
        if !this.ivars().accepted.get()
            || current.listening_history
            || !history.checked()
            || confirm_history(
                mtm,
                &texts.experimental_warning,
                &texts.yes,
                &texts.no,
                &texts.cancel,
            )
        {
            break;
        }
        history.set_checked(false);
        this.ivars().accepted.set(false);
        window.makeFirstResponder(Some(&history));
    }
    window.setDelegate(None);
    window.orderOut(None);
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    if !this.ivars().accepted.get() {
        return None;
    }
    let mut chosen = current;
    chosen.quality = quality
        .selected()
        .and_then(|index| Quality::ALL.get(index).copied())
        .unwrap_or(current.quality);
    chosen.normalisation = normalisation.checked();
    chosen.listening_history = history.checked();
    chosen.key_cache = key_cache.checked();
    chosen.autoplay = autoplay
        .selected()
        .and_then(|index| Autoplay::ALL.get(index).copied())
        .unwrap_or(current.autoplay);
    chosen.library_order = order
        .selected()
        .and_then(|index| LibraryOrder::ALL.get(index).copied())
        .unwrap_or(current.library_order);
    Some(chosen)
}
