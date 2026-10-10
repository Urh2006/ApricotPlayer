//! The one-time language choice before the main window
//! (`first_run_language_win32.rs`): a window "Language" with the prompt, the
//! language list (focused, current language selected), OK and Cancel. Enter
//! and a double click accept, Escape cancels.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use apricot_core::locale::LANGUAGES;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSModalResponseCancel, NSModalResponseOK, NSScrollView,
    NSTextField, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::controls::{self, Button, List};

const WIDTH: f64 = 520.0;
const HEIGHT: f64 = 560.0;
const MARGIN: f64 = 12.0;
const PROMPT_HEIGHT: f64 = 28.0;
const BUTTON_HEIGHT: f64 = 34.0;
const BUTTON_WIDTH: f64 = 110.0;

#[derive(Default)]
pub struct DialogIvars {
    list: RefCell<Option<Retained<List>>>,
    selected: Cell<Option<usize>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotLanguageDialog"]
    #[ivars = DialogIvars]
    struct Dialog;

    unsafe impl NSObjectProtocol for Dialog {}

    /// Closing the window cancels, like Windows `WM_CLOSE`.
    unsafe impl NSWindowDelegate for Dialog {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            self.finish(None, NSModalResponseCancel);
            false
        }
    }

    impl Dialog {
        #[unsafe(method(accept:))]
        fn accept(&self, _sender: Option<&AnyObject>) {
            let selected = self.ivars().list.borrow().as_ref().and_then(|list| list.selected());
            self.finish(selected, NSModalResponseOK);
        }

        #[unsafe(method(cancel:))]
        fn cancel(&self, _sender: Option<&AnyObject>) {
            self.finish(None, NSModalResponseCancel);
        }
    }
);

impl Dialog {
    fn finish(&self, selected: Option<usize>, response: objc2_app_kit::NSModalResponse) {
        self.ivars().selected.set(selected);
        NSApplication::sharedApplication(MainThreadMarker::from(self)).stopModalWithCode(response);
    }
}

/// Shows the language choice and returns the chosen language code, or
/// `None` when it was cancelled or closed (the current language stays).
pub fn choose(mtm: MainThreadMarker, current_language: &str) -> Option<&'static str> {
    let dialog: Retained<Dialog> = {
        let this = Dialog::alloc(mtm).set_ivars(DialogIvars::default());
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let target: &AnyObject = dialog.as_ref();
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
    window.setTitle(&NSString::from_str("Language"));
    window.setDelegate(Some(ProtocolObject::from_ref(&*dialog)));
    let content = window.contentView().expect("a window has a content view");

    let prompt = NSTextField::labelWithString(
        &NSString::from_str("Choose the language for ApricotPlayer."),
        mtm,
    );
    prompt.setFrame(NSRect::new(
        NSPoint::new(MARGIN, HEIGHT - MARGIN - PROMPT_HEIGHT),
        NSSize::new(WIDTH - 2.0 * MARGIN, PROMPT_HEIGHT),
    ));
    content.addSubview(&prompt);

    let list = List::new(mtm, "Language");
    let selected = LANGUAGES
        .iter()
        .position(|language| language.code == current_language)
        .unwrap_or(0);
    list.set_rows(
        LANGUAGES
            .iter()
            .map(|language| language.name.to_owned())
            .collect(),
        Some(selected),
    );
    list.set_activate_action(target, sel!(accept:));
    let list_bottom = MARGIN * 2.0 + BUTTON_HEIGHT;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, list_bottom),
            NSSize::new(
                WIDTH - 2.0 * MARGIN,
                HEIGHT - MARGIN - PROMPT_HEIGHT - list_bottom,
            ),
        ),
    );
    scroll.setDocumentView(Some(&list));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);

    let ok = Button::new(mtm, "OK", target, sel!(accept:));
    ok.setFrame(NSRect::new(
        NSPoint::new(WIDTH - MARGIN - BUTTON_WIDTH * 2.0 - 8.0, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    content.addSubview(&ok);
    let cancel = Button::new(mtm, "Cancel", target, sel!(cancel:));
    cancel.setFrame(NSRect::new(
        NSPoint::new(WIDTH - MARGIN - BUTTON_WIDTH, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    // Escape presses Cancel, as IDCANCEL does on Windows.
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&cancel);

    controls::set_tab_order(&[&list, &ok, &cancel]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&list));
    *dialog.ivars().list.borrow_mut() = Some(list.clone());

    let app = NSApplication::sharedApplication(mtm);
    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&list));
    let response = app.runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    if response != NSModalResponseOK {
        return None;
    }
    dialog
        .ivars()
        .selected
        .get()
        .and_then(|index| LANGUAGES.get(index))
        .map(|language| language.code)
}
