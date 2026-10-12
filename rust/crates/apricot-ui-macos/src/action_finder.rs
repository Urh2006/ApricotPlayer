//! The Action Finder, the macOS counterpart of `action_finder_win32.rs`:
//! the search field (focused), the matching actions, Open (the default
//! button) and Cancel, modal over the main window. Typing filters the list,
//! Up and Down in the field go to the list, Return runs the selected action
//! and Escape closes.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use apricot_app::ActionFinderModel;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBackingStoreType, NSControl, NSControlTextEditingDelegate,
    NSScrollView, NSTextField, NSTextFieldDelegate, NSTextView, NSWindow, NSWindowDelegate,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

use crate::controls::{self, Button, List};
use crate::window::MainWindowController;

/// Windows `CreateWindowExW(..., 620, 500, ...)`.
const WIDTH: f64 = 620.0;
const HEIGHT: f64 = 500.0;
const MARGIN: f64 = 12.0;
const LABEL_HEIGHT: f64 = 22.0;
const QUERY_HEIGHT: f64 = 30.0;
const BUTTON_HEIGHT: f64 = 34.0;
const BUTTON_WIDTH: f64 = 110.0;

pub struct FinderIvars {
    model: ActionFinderModel,
    filtered: RefCell<Vec<usize>>,
    query: RefCell<Option<Retained<NSTextField>>>,
    list: RefCell<Option<Retained<List>>>,
    result: Cell<Option<&'static str>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotActionFinder"]
    #[ivars = FinderIvars]
    struct ActionFinder;

    unsafe impl NSObjectProtocol for ActionFinder {}

    unsafe impl NSWindowDelegate for ActionFinder {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    unsafe impl NSControlTextEditingDelegate for ActionFinder {
        /// Windows `EN_CHANGE`.
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _notification: &NSNotification) {
            self.refresh_results();
        }

        /// Return runs the action, Escape closes, Up and Down go to the list
        /// (Windows `control_proc` of the field).
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, _control: &NSControl, _view: &NSTextView, command: Sel) -> bool {
            if command == sel!(insertNewline:) {
                self.activate();
                true
            } else if command == sel!(cancelOperation:) {
                NSApplication::sharedApplication(self.mtm()).stopModal();
                true
            } else if command == sel!(moveUp:) || command == sel!(moveDown:) {
                if let Some(list) = self.ivars().list.borrow().as_ref()
                    && let Some(window) = list.window()
                {
                    window.makeFirstResponder(Some(list));
                }
                true
            } else {
                false
            }
        }
    }

    unsafe impl NSTextFieldDelegate for ActionFinder {}

    impl ActionFinder {
        #[unsafe(method(open:))]
        fn open(&self, _sender: Option<&AnyObject>) {
            self.activate();
        }

        #[unsafe(method(cancel:))]
        fn cancel(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }
    }
);

impl ActionFinder {
    /// Windows `refresh_results`: the matches, or the "no results" row,
    /// with the first one selected.
    fn refresh_results(&self) {
        let query = self
            .ivars()
            .query
            .borrow()
            .as_ref()
            .map(|field| field.stringValue().to_string())
            .unwrap_or_default();
        let model = &self.ivars().model;
        let filtered = model.filtered_indices(&query);
        let rows = if filtered.is_empty() {
            vec![model.no_results_label.clone()]
        } else {
            filtered
                .iter()
                .map(|index| model.items[*index].label.clone())
                .collect()
        };
        *self.ivars().filtered.borrow_mut() = filtered;
        if let Some(list) = self.ivars().list.borrow().as_ref() {
            list.set_rows(rows, Some(0));
        }
    }

    /// Windows `activate`.
    fn activate(&self) {
        let selected = self
            .ivars()
            .list
            .borrow()
            .as_ref()
            .and_then(|list| list.selected());
        let Some(index) = selected.and_then(|row| self.ivars().filtered.borrow().get(row).copied())
        else {
            return;
        };
        self.ivars()
            .result
            .set(Some(self.ivars().model.items[index].action_id));
        NSApplication::sharedApplication(self.mtm()).stopModal();
    }
}

/// Windows `show_action_finder`: the chosen action goes to
/// `Shell::action_finder_closed`.
pub fn run(controller: &MainWindowController, model: ActionFinderModel) {
    let mtm = MainThreadMarker::from(controller);
    let action = show(mtm, model);
    controller.with_shell(|shell, ui| shell.action_finder_closed(ui, action));
}

/// The modal Action Finder over the key window; focus returns where it was.
pub fn show(mtm: MainThreadMarker, model: ActionFinderModel) -> Option<&'static str> {
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let title = model.title.clone();
    let query_label_text = model.query_label.clone();
    let results_name = model.results_name.clone();
    let (open_label, cancel_label) = (model.open_label.clone(), model.cancel_label.clone());
    let this: Retained<ActionFinder> = {
        let this = ActionFinder::alloc(mtm).set_ivars(FinderIvars {
            model,
            filtered: RefCell::new(Vec::new()),
            query: RefCell::new(None),
            list: RefCell::new(None),
            result: Cell::new(None),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let target: &AnyObject = this.as_ref();
    // SAFETY: a standard titled window with a buffered backing.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(WIDTH, HEIGHT)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is released by its last owner, this function.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(&title));
    window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
    let content = window.contentView().expect("a window has a content view");
    let top = HEIGHT - MARGIN;
    let query_label = NSTextField::labelWithString(&NSString::from_str(&query_label_text), mtm);
    query_label.setFrame(NSRect::new(
        NSPoint::new(MARGIN, top - LABEL_HEIGHT),
        NSSize::new(WIDTH - 2.0 * MARGIN, LABEL_HEIGHT),
    ));
    content.addSubview(&query_label);
    let query = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, top - LABEL_HEIGHT - 4.0 - QUERY_HEIGHT),
            NSSize::new(WIDTH - 2.0 * MARGIN, QUERY_HEIGHT),
        ),
    );
    query.setAccessibilityLabel(Some(&NSString::from_str(&query_label_text)));
    // SAFETY: the dialog outlives the field; the delegate is weak.
    unsafe { query.setDelegate(Some(ProtocolObject::from_ref(&*this))) };
    content.addSubview(&query);
    let list_top = top - LABEL_HEIGHT - 4.0 - QUERY_HEIGHT - MARGIN;
    let list_bottom = MARGIN * 2.0 + BUTTON_HEIGHT;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, list_bottom),
            NSSize::new(WIDTH - 2.0 * MARGIN, list_top - list_bottom),
        ),
    );
    let list = List::new(mtm, &results_name);
    list.set_activate_action(target, sel!(open:));
    scroll.setDocumentView(Some(&list));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);
    let open = Button::new(mtm, &open_label, target, sel!(open:));
    open.setFrame(NSRect::new(
        NSPoint::new(WIDTH - MARGIN - 2.0 * BUTTON_WIDTH - 8.0, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    content.addSubview(&open);
    let cancel = Button::new(mtm, &cancel_label, target, sel!(cancel:));
    cancel.setFrame(NSRect::new(
        NSPoint::new(WIDTH - MARGIN - BUTTON_WIDTH, MARGIN),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    // Windows `IDCANCEL` and the list's Escape.
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&cancel);
    controls::set_tab_order(&[&query, &list, &open, &cancel]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&query));
    *this.ivars().query.borrow_mut() = Some(query.clone());
    *this.ivars().list.borrow_mut() = Some(list);
    this.refresh_results();
    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&query));
    app.runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    let result = this.ivars().result.get();
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    result
}
