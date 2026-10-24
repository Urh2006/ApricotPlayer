//! The `AudioVault` login dialog, the macOS counterpart of
//! `audiovault_login_win32.rs` (Python `show_audiovault_login`): Email
//! (focused), Password, Register, then OK and Cancel. Return in either field
//! is OK, Escape is Cancel and Register opens the registration page.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use apricot_shell::AudiovaultLogin;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBackingStoreType, NSSecureTextField, NSTextField, NSWindow,
    NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::controls::{self, Button};

const WIDTH: f64 = 460.0;
const HEIGHT: f64 = 230.0;
const MARGIN: f64 = 12.0;
const LABEL_HEIGHT: f64 = 20.0;
const FIELD_HEIGHT: f64 = 26.0;
const BUTTON_WIDTH: f64 = 120.0;
const BUTTON_HEIGHT: f64 = 30.0;

pub struct LoginIvars {
    email: RefCell<Option<Retained<NSTextField>>>,
    password: RefCell<Option<Retained<NSSecureTextField>>>,
    register_url: Cell<&'static str>,
    result: RefCell<Option<(String, String)>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotAudiovaultLogin"]
    #[ivars = LoginIvars]
    struct LoginDialog;

    unsafe impl NSObjectProtocol for LoginDialog {}

    unsafe impl NSWindowDelegate for LoginDialog {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    impl LoginDialog {
        #[unsafe(method(accept:))]
        fn accept(&self, _sender: Option<&AnyObject>) {
            let email = self
                .ivars()
                .email
                .borrow()
                .as_ref()
                .map(|field| field.stringValue().to_string())
                .unwrap_or_default();
            let password = self
                .ivars()
                .password
                .borrow()
                .as_ref()
                .map(|field| field.stringValue().to_string())
                .unwrap_or_default();
            *self.ivars().result.borrow_mut() = Some((email, password));
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(cancel:))]
        fn cancel(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        /// Python `open_audiovault_registration`.
        #[unsafe(method(register:))]
        fn register(&self, _sender: Option<&AnyObject>) {
            let _ = apricot_platform::shell::open_url(self.ivars().register_url.get());
        }
    }
);

/// Windows `audiovault_login_win32::show`: the address and password, or
/// `None` when cancelled; focus returns where it was.
#[allow(clippy::too_many_lines)] // The dialog's controls in their Tab order.
pub fn show(mtm: MainThreadMarker, login: &AudiovaultLogin) -> Option<(String, String)> {
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let this: Retained<LoginDialog> = {
        let this = LoginDialog::alloc(mtm).set_ivars(LoginIvars {
            email: RefCell::new(None),
            password: RefCell::new(None),
            register_url: Cell::new(login.register_url),
            result: RefCell::new(None),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let target: &AnyObject = this.as_ref();
    // SAFETY: a standard titled window with a buffered backing.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(300.0, 200.0), NSSize::new(WIDTH, HEIGHT)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is released by its last owner, this function.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(&login.title));
    window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
    let content = window.contentView().expect("a window has a content view");
    let mut y = HEIGHT - MARGIN - LABEL_HEIGHT;
    let label = |text: &str, y: f64| {
        let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        label.setFrame(NSRect::new(
            NSPoint::new(MARGIN, y),
            NSSize::new(WIDTH - 2.0 * MARGIN, LABEL_HEIGHT),
        ));
        content.addSubview(&label);
    };
    label(&login.email_label, y);
    y -= FIELD_HEIGHT + 2.0;
    let email = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, y),
            NSSize::new(WIDTH - 2.0 * MARGIN, FIELD_HEIGHT),
        ),
    );
    email.setStringValue(&NSString::from_str(&login.email));
    email.setAccessibilityLabel(Some(&NSString::from_str(&login.email_label)));
    content.addSubview(&email);
    y -= LABEL_HEIGHT + 8.0;
    label(&login.password_label, y);
    y -= FIELD_HEIGHT + 2.0;
    let password = NSSecureTextField::initWithFrame(
        NSSecureTextField::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, y),
            NSSize::new(WIDTH - 2.0 * MARGIN, FIELD_HEIGHT),
        ),
    );
    password.setAccessibilityLabel(Some(&NSString::from_str(&login.password_label)));
    content.addSubview(&password);
    // wx: Enter in either field ends the dialog with OK.
    // SAFETY: the dialog outlives the fields.
    unsafe {
        email.setTarget(Some(target));
        email.setAction(Some(sel!(accept:)));
        password.setTarget(Some(target));
        password.setAction(Some(sel!(accept:)));
    }
    let register = Button::new(mtm, &login.register, target, sel!(register:));
    register.setFrame(NSRect::new(
        NSPoint::new(MARGIN, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    content.addSubview(&register);
    let ok = Button::new(mtm, &login.ok, target, sel!(accept:));
    ok.setFrame(NSRect::new(
        NSPoint::new(WIDTH - MARGIN - 2.0 * BUTTON_WIDTH - 8.0, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    content.addSubview(&ok);
    let cancel = Button::new(mtm, &login.cancel, target, sel!(cancel:));
    cancel.setFrame(NSRect::new(
        NSPoint::new(WIDTH - MARGIN - BUTTON_WIDTH, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&cancel);
    controls::set_tab_order(&[&email, &password, &register, &ok, &cancel]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&email));
    *this.ivars().email.borrow_mut() = Some(email.clone());
    *this.ivars().password.borrow_mut() = Some(password);
    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&email));
    app.runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    this.ivars().result.borrow_mut().take()
}
