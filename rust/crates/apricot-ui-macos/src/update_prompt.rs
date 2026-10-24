//! "Update available" (Windows `update_win32::show_update_prompt`): the
//! version, "What's new?", the read-only changes (focused, at the top),
//! "Would you like to update now?", Update now and Skip this version.
//! Escape and closing the window skip.

#![allow(unsafe_code)]

use std::cell::Cell;

use apricot_shell::UpdatePrompt;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBackingStoreType, NSScrollView, NSTextField, NSTextView,
    NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString};

use crate::controls::{self, Button};

/// wx `SetMinSize((640, 420))`.
const WIDTH: f64 = 640.0;
const HEIGHT: f64 = 420.0;
const MARGIN: f64 = 12.0;
const BUTTON_WIDTH: f64 = 160.0;
const BUTTON_HEIGHT: f64 = 30.0;

pub struct PromptIvars {
    accepted: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotUpdatePrompt"]
    #[ivars = PromptIvars]
    struct PromptDialog;

    unsafe impl NSObjectProtocol for PromptDialog {}

    unsafe impl NSWindowDelegate for PromptDialog {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    impl PromptDialog {
        #[unsafe(method(updateNow:))]
        fn update_now(&self, _sender: Option<&AnyObject>) {
            self.ivars().accepted.set(true);
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(skip:))]
        fn skip(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }
    }
);

fn label(mtm: MainThreadMarker, content: &NSView, text: &str, top: f64) {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(NSRect::new(
        NSPoint::new(MARGIN, top - 22.0),
        NSSize::new(WIDTH - 2.0 * MARGIN, 22.0),
    ));
    content.addSubview(&label);
}

/// Shows the prompt; `true` for Update now. Focus returns where it was.
pub fn run(mtm: MainThreadMarker, prompt: &UpdatePrompt) -> bool {
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let this: Retained<PromptDialog> = {
        let this = PromptDialog::alloc(mtm).set_ivars(PromptIvars {
            accepted: Cell::new(false),
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
    window.setTitle(&NSString::from_str(&prompt.title));
    window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
    let content = window.contentView().expect("a window has a content view");

    label(mtm, &content, &prompt.version, HEIGHT - MARGIN);
    label(mtm, &content, &prompt.whats_new, HEIGHT - MARGIN - 28.0);
    let buttons_bottom = MARGIN;
    let question_bottom = buttons_bottom + BUTTON_HEIGHT + 10.0;
    let details_bottom = question_bottom + 30.0;
    let details_top = HEIGHT - MARGIN - 56.0;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, details_bottom),
            NSSize::new(WIDTH - 2.0 * MARGIN, details_top - details_bottom),
        ),
    );
    let details = NSTextView::initWithFrame(
        NSTextView::alloc(mtm),
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(WIDTH - 2.0 * MARGIN, details_top - details_bottom),
        ),
    );
    details.setString(&NSString::from_str(&prompt.changelog));
    details.setEditable(false);
    details.setSelectable(true);
    details.setAccessibilityLabel(Some(&NSString::from_str(&prompt.whats_new)));
    scroll.setDocumentView(Some(&details));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);
    label(mtm, &content, &prompt.question, question_bottom + 22.0);

    let update = Button::new(mtm, &prompt.update_now, target, sel!(updateNow:));
    update.setFrame(NSRect::new(
        NSPoint::new(WIDTH - MARGIN - 2.0 * BUTTON_WIDTH - 8.0, buttons_bottom),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    content.addSubview(&update);
    let skip = Button::new(mtm, &prompt.skip, target, sel!(skip:));
    skip.setFrame(NSRect::new(
        NSPoint::new(WIDTH - MARGIN - BUTTON_WIDTH, buttons_bottom),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    // wx `SetEscapeId(wx.ID_NO)`: Escape skips.
    skip.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&skip);

    controls::set_tab_order(&[&*details as &NSView, &update, &skip]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&details));
    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    // wx.CallAfter(safe_set_focus, details): the caret starts at the top.
    window.makeFirstResponder(Some(&details));
    details.setSelectedRange(NSRange::new(0, 0));
    details.scrollRangeToVisible(NSRange::new(0, 0));
    app.runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    this.ivars().accepted.get()
}
