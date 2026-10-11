//! Modal list choices: the one-time language choice before the main window
//! (`first_run_language_win32.rs`) and the list picker of
//! `playlist_dialog_win32.rs` (output devices and other choices). A window
//! with the prompt, the list (focused, the initial row selected), OK and
//! Cancel. Enter and a double click accept, Escape and closing cancel, and
//! focus returns to the control that had it.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use apricot_core::locale::LANGUAGES;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBackingStoreType, NSModalResponseCancel, NSModalResponseOK,
    NSScrollView, NSTextField, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::controls::{self, Button, List};

const MARGIN: f64 = 12.0;
const PROMPT_HEIGHT: f64 = 28.0;
const BUTTON_HEIGHT: f64 = 34.0;
const BUTTON_WIDTH: f64 = 110.0;

#[derive(Default)]
pub struct DialogIvars {
    list: RefCell<Option<Retained<List>>>,
    selected: Cell<Option<usize>>,
    field: RefCell<Option<Retained<NSTextField>>>,
    text: RefCell<Option<String>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotChoiceDialog"]
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
            let text = self
                .ivars()
                .field
                .borrow()
                .as_ref()
                .map(|field| field.stringValue().to_string());
            *self.ivars().text.borrow_mut() = text;
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

/// One list choice.
pub struct ChoiceRequest<'a> {
    pub title: &'a str,
    pub prompt: &'a str,
    /// The list's accessible name (Windows uses the prompt).
    pub list_name: &'a str,
    pub choices: &'a [String],
    pub initial: usize,
    pub ok: &'a str,
    pub cancel: &'a str,
    pub width: f64,
    pub height: f64,
}

/// Shows the choice modally; `None` when cancelled or closed.
pub fn choose_from_list(mtm: MainThreadMarker, request: &ChoiceRequest<'_>) -> Option<usize> {
    let (width, height) = (request.width, request.height);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
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
            NSRect::new(NSPoint::new(300.0, 200.0), NSSize::new(width, height)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is released by its last owner, this function.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(request.title));
    window.setDelegate(Some(ProtocolObject::from_ref(&*dialog)));
    let content = window.contentView().expect("a window has a content view");

    let prompt = NSTextField::labelWithString(&NSString::from_str(request.prompt), mtm);
    prompt.setFrame(NSRect::new(
        NSPoint::new(MARGIN, height - MARGIN - PROMPT_HEIGHT),
        NSSize::new(width - 2.0 * MARGIN, PROMPT_HEIGHT),
    ));
    content.addSubview(&prompt);

    let list = List::new(mtm, request.list_name);
    list.set_rows(request.choices.to_vec(), Some(request.initial));
    list.set_activate_action(target, sel!(accept:));
    let list_bottom = MARGIN * 2.0 + BUTTON_HEIGHT;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, list_bottom),
            NSSize::new(
                width - 2.0 * MARGIN,
                height - MARGIN - PROMPT_HEIGHT - list_bottom,
            ),
        ),
    );
    scroll.setDocumentView(Some(&list));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);

    let ok = Button::new(mtm, request.ok, target, sel!(accept:));
    ok.setFrame(NSRect::new(
        NSPoint::new(width - MARGIN - BUTTON_WIDTH * 2.0 - 8.0, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    content.addSubview(&ok);
    let cancel = Button::new(mtm, request.cancel, target, sel!(cancel:));
    cancel.setFrame(NSRect::new(
        NSPoint::new(width - MARGIN - BUTTON_WIDTH, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    // Escape presses Cancel, as IDCANCEL does on Windows.
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&cancel);

    controls::set_tab_order(&[&list, &ok, &cancel]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&list));
    *dialog.ivars().list.borrow_mut() = Some(list.clone());

    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&list));
    let response = app.runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    if response != NSModalResponseOK {
        return None;
    }
    dialog.ivars().selected.get()
}

/// Windows `prompt_name`: the prompt, a text field (focused) with OK and
/// Cancel. Return accepts, Escape and closing cancel; `None` when cancelled.
/// The text is returned as typed.
pub fn prompt_text(
    mtm: MainThreadMarker,
    title: &str,
    prompt: &str,
    ok: &str,
    cancel: &str,
) -> Option<String> {
    let (width, height) = (520.0, 170.0);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
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
            NSRect::new(NSPoint::new(300.0, 200.0), NSSize::new(width, height)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is released by its last owner, this function.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(title));
    window.setDelegate(Some(ProtocolObject::from_ref(&*dialog)));
    let content = window.contentView().expect("a window has a content view");

    let label = NSTextField::labelWithString(&NSString::from_str(prompt), mtm);
    label.setFrame(NSRect::new(
        NSPoint::new(MARGIN, height - MARGIN - PROMPT_HEIGHT),
        NSSize::new(width - 2.0 * MARGIN, PROMPT_HEIGHT),
    ));
    content.addSubview(&label);
    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, height - MARGIN - PROMPT_HEIGHT - 30.0),
            NSSize::new(width - 2.0 * MARGIN, 26.0),
        ),
    );
    field.setAccessibilityLabel(Some(&NSString::from_str(prompt)));
    content.addSubview(&field);

    let ok = Button::new(mtm, ok, target, sel!(accept:));
    ok.setFrame(NSRect::new(
        NSPoint::new(width - MARGIN - BUTTON_WIDTH * 2.0 - 8.0, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    // Return in the field presses OK, as the Windows edit subclass does.
    ok.setKeyEquivalent(&NSString::from_str("\r"));
    content.addSubview(&ok);
    let cancel = Button::new(mtm, cancel, target, sel!(cancel:));
    cancel.setFrame(NSRect::new(
        NSPoint::new(width - MARGIN - BUTTON_WIDTH, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&cancel);

    controls::set_tab_order(&[&field, &ok, &cancel]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&field));
    *dialog.ivars().field.borrow_mut() = Some(field.clone());

    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&field));
    let response = app.runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    if response != NSModalResponseOK {
        return None;
    }
    dialog.ivars().text.borrow_mut().take()
}

/// Shows the language choice and returns the chosen language code, or
/// `None` when it was cancelled or closed (the current language stays).
pub fn choose(mtm: MainThreadMarker, current_language: &str) -> Option<&'static str> {
    let choices: Vec<String> = LANGUAGES
        .iter()
        .map(|language| language.name.to_owned())
        .collect();
    let initial = LANGUAGES
        .iter()
        .position(|language| language.code == current_language)
        .unwrap_or(0);
    choose_from_list(
        mtm,
        &ChoiceRequest {
            title: "Language",
            prompt: "Choose the language for ApricotPlayer.",
            list_name: "Language",
            choices: &choices,
            initial,
            ok: "OK",
            cancel: "Cancel",
            width: 520.0,
            height: 560.0,
        },
    )
    .and_then(|index| LANGUAGES.get(index))
    .map(|language| language.code)
}
