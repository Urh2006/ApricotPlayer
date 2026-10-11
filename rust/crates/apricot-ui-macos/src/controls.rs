//! Native controls with the Windows keyboard contract.
//!
//! macOS leaves buttons and pop-up buttons out of the Tab order unless Full
//! Keyboard Access is on, and its list type-select does not cycle through
//! rows on a repeated letter. These subclasses keep the AppKit controls (so
//! VoiceOver sees real roles) and add the Windows behavior: every control is
//! in the declared Tab order, Return presses the focused button, and a list
//! handles Enter, Shift+F10 and letter navigation like a Windows list box.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::time::Instant;

use apricot_shell::TypeAhead;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSBezelStyle, NSButton, NSControl, NSControlTextEditingDelegate, NSEvent,
    NSEventModifierFlags, NSResponder, NSTableColumn, NSTableView, NSTableViewDataSource,
    NSTableViewDelegate, NSView,
};
use objc2_foundation::{NSIndexSet, NSInteger, NSObjectProtocol, NSPoint, NSRect, NSString};

use crate::keys::{self, Modifiers};

/// The held modifier keys of a key event.
pub fn modifiers(event: &NSEvent) -> Modifiers {
    let flags = event.modifierFlags();
    Modifiers {
        command: flags.contains(NSEventModifierFlags::Command),
        option: flags.contains(NSEventModifierFlags::Option),
        shift: flags.contains(NSEventModifierFlags::Shift),
        control: flags.contains(NSEventModifierFlags::Control),
    }
}

/// Tab and Shift+Tab move along the declared key view loop, as on Windows,
/// whether or not Full Keyboard Access is on. Returns `true` when handled.
pub fn handle_tab(view: &NSView, event: &NSEvent) -> bool {
    if event.keyCode() != keys::KEY_TAB {
        return false;
    }
    let modifiers = modifiers(event);
    if modifiers.command || modifiers.option || modifiers.control {
        return false;
    }
    let Some(window) = view.window() else {
        return false;
    };
    if modifiers.shift {
        window.selectKeyViewPrecedingView(view);
    } else {
        window.selectKeyViewFollowingView(view);
    }
    true
}

/// Declares the Tab order: each view's next key view is the following one,
/// and the last wraps to the first.
pub fn set_tab_order(views: &[&NSView]) {
    for (index, view) in views.iter().enumerate() {
        let next = views[(index + 1) % views.len()];
        // SAFETY: all views belong to the same window.
        unsafe { view.setNextKeyView(Some(next)) };
    }
}

#[derive(Default)]
pub struct ListIvars {
    rows: RefCell<Vec<String>>,
    type_ahead: RefCell<TypeAhead>,
    /// Sent to the target on Enter and on a double click.
    activate: Cell<Option<Sel>>,
}

define_class!(
    /// A single-column list, the counterpart of the Windows list box: rows
    /// are plain text, Enter activates, Shift+F10 and VoiceOver's menu command
    /// open the context menu, letters navigate as in a Windows list box.
    // SAFETY: NSTableView may be subclassed; no Drop impl.
    #[unsafe(super(NSTableView, NSControl, NSView, NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotList"]
    #[ivars = ListIvars]
    pub struct List;

    unsafe impl NSObjectProtocol for List {}

    unsafe impl NSControlTextEditingDelegate for List {}

    unsafe impl NSTableViewDataSource for List {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table: &NSTableView) -> NSInteger {
            NSInteger::try_from(self.ivars().rows.borrow().len()).unwrap_or(0)
        }

        #[unsafe(method_id(tableView:objectValueForTableColumn:row:))]
        fn value(
            &self,
            _table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<AnyObject>> {
            usize::try_from(row)
                .ok()
                .and_then(|row| self.ivars().rows.borrow().get(row).cloned())
                .map(|text| Retained::into_super(Retained::into_super(NSString::from_str(&text))))
        }
    }

    unsafe impl NSTableViewDelegate for List {
        /// Letters are handled in `keyDown:`; AppKit's type select would
        /// search for "pp" on a repeated P.
        #[unsafe(method(tableView:shouldTypeSelectForEvent:withCurrentSearchString:))]
        fn should_type_select(
            &self,
            _table: &NSTableView,
            _event: &NSEvent,
            _search: Option<&NSString>,
        ) -> bool {
            false
        }
    }

    impl List {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if handle_tab(self, event) {
                return;
            }
            let code = event.keyCode();
            let modifiers = modifiers(event);
            let plain = !modifiers.command && !modifiers.option && !modifiers.control;
            if plain && !modifiers.shift && matches!(code, keys::KEY_RETURN | keys::KEY_KEYPAD_ENTER) {
                self.activate();
                return;
            }
            if keys::is_context_menu_key(code, modifiers) {
                self.show_context_menu();
                return;
            }
            if plain && self.type_letter(event) {
                return;
            }
            // Any other key ends the letter search, as in a Windows list box.
            self.ivars().type_ahead.borrow_mut().reset();
            // SAFETY: forwarding the unhandled event to NSTableView.
            unsafe {
                let _: () = msg_send![super(self), keyDown: event];
            }
        }
    }
);

impl List {
    pub fn new(mtm: MainThreadMarker, label: &str) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ListIvars::default());
        // SAFETY: `initWithFrame:` of NSTableView.
        let list: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        let column = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str("text"),
        );
        column.setEditable(false);
        column.setResizingMask(objc2_app_kit::NSTableColumnResizingOptions::AutoresizingMask);
        list.addTableColumn(&column);
        list.setHeaderView(None);
        list.setAllowsEmptySelection(false);
        list.setAllowsMultipleSelection(false);
        list.setColumnAutoresizingStyle(
            objc2_app_kit::NSTableViewColumnAutoresizingStyle::FirstColumnOnlyAutoresizingStyle,
        );
        // SAFETY: the list is its own data source and delegate and lives as
        // long as itself.
        unsafe {
            list.setDataSource(Some(ProtocolObject::from_ref(&*list)));
            list.setDelegate(Some(ProtocolObject::from_ref(&*list)));
        }
        list.set_label(label);
        list
    }

    /// The accessible name, Windows `set_control_name`.
    pub fn set_label(&self, label: &str) {
        self.setAccessibilityLabel(Some(&NSString::from_str(label)));
    }

    /// Replaces the rows and selects `selected` (clamped). Tabs in a label
    /// (the shortcut column of the main menu) read as two spaces.
    pub fn set_rows(&self, rows: Vec<String>, selected: Option<usize>) {
        let rows = rows
            .into_iter()
            .map(|row| row.replace('\t', "  "))
            .collect::<Vec<_>>();
        let count = rows.len();
        *self.ivars().rows.borrow_mut() = rows;
        self.ivars().type_ahead.borrow_mut().reset();
        self.reloadData();
        if let Some(selected) = selected.filter(|_| count > 0) {
            self.select(selected.min(count - 1));
        }
    }

    pub fn select(&self, row: usize) {
        let Ok(index) = NSInteger::try_from(row) else {
            return;
        };
        self.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
        self.scrollRowToVisible(index);
    }

    pub fn selected(&self) -> Option<usize> {
        usize::try_from(self.selectedRow()).ok()
    }

    /// The selector sent to the target on Enter and on a double click.
    pub fn set_activate_action(&self, target: &AnyObject, action: Sel) {
        self.ivars().activate.set(Some(action));
        // SAFETY: the target outlives the list (it is the window controller).
        unsafe {
            self.setTarget(Some(target));
            self.setDoubleAction(Some(action));
        }
    }

    fn activate(&self) {
        if let Some(action) = self.ivars().activate.get() {
            // SAFETY: the action exists on the target set with it.
            unsafe {
                let _: bool = msg_send![self, sendAction: action, to: self.target().as_deref()];
            }
        }
    }

    fn show_context_menu(&self) {
        let Some(menu) = self.menu() else {
            return;
        };
        let row = self.selectedRow().max(0);
        let rect = self.rectOfRow(row);
        let point = NSPoint::new(rect.origin.x + 20.0, rect.origin.y + rect.size.height);
        menu.popUpMenuPositioningItem_atLocation_inView(None, point, Some(self));
    }

    /// Windows list box letter navigation; `true` when the key was a letter.
    fn type_letter(&self, event: &NSEvent) -> bool {
        let Some(characters) = event.characters() else {
            return false;
        };
        let characters = characters.to_string();
        let mut letters = characters.chars();
        let (Some(character), None) = (letters.next(), letters.next()) else {
            return false;
        };
        if !character.is_alphanumeric() {
            return false;
        }
        let target = {
            let rows = self.ivars().rows.borrow();
            self.ivars().type_ahead.borrow_mut().type_character(
                &rows,
                self.selected(),
                character,
                Instant::now(),
            )
        };
        if let Some(row) = target {
            self.select(row);
        }
        true
    }
}

define_class!(
    /// A push button that is always in the Tab order, as Windows buttons
    /// are, and that also presses on Return.
    // SAFETY: NSButton may be subclassed; no Drop impl.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotButton"]
    pub struct Button;

    impl Button {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeKeyView))]
        fn can_become_key_view(&self) -> bool {
            true
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if handle_tab(self, event) {
                return;
            }
            let modifiers = modifiers(event);
            if matches!(event.keyCode(), keys::KEY_RETURN | keys::KEY_KEYPAD_ENTER)
                && !modifiers.command
                && !modifiers.option
                && !modifiers.control
            {
                // SAFETY: a click with no sender.
                unsafe { self.performClick(None) };
            } else {
                // SAFETY: forwarding the unhandled event to NSButton.
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }
    }
);

impl Button {
    pub fn new(
        mtm: MainThreadMarker,
        title: &str,
        target: &AnyObject,
        action: Sel,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` of NSButton; the target outlives the button.
        let button: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        button.setTitle(&NSString::from_str(title));
        button.setBezelStyle(NSBezelStyle::Push);
        // SAFETY: as above.
        unsafe {
            button.setTarget(Some(target));
            button.setAction(Some(action));
        }
        button
    }
}

define_class!(
    /// A read-only multi-line text (the player details) whose Tab moves
    /// focus on like every other control (Windows `handle_details_tab_message`).
    // SAFETY: NSTextView may be subclassed; no Drop impl.
    #[unsafe(super(
        objc2_app_kit::NSTextView,
        objc2_app_kit::NSText,
        NSView,
        NSResponder,
        objc2_foundation::NSObject
    ))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotReadOnlyText"]
    pub struct ReadOnlyText;

    impl ReadOnlyText {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !handle_tab(self, event) {
                // SAFETY: forwarding the unhandled event to NSTextView.
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }
    }
);

impl ReadOnlyText {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` of NSTextView.
        let text: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        text.setEditable(false);
        text.setSelectable(true);
        text.setRichText(false);
        text
    }

    /// Replaces the text and puts the caret at the start, unless it is
    /// unchanged (Windows `set_details_text` keeps the reading position).
    pub fn set_text(&self, value: &str) {
        if self.string().to_string() == value {
            return;
        }
        self.setString(&NSString::from_str(value));
        self.setSelectedRange(objc2_foundation::NSRange::new(0, 0));
    }

    pub fn set_label(&self, label: &str) {
        self.setAccessibilityLabel(Some(&NSString::from_str(label)));
    }
}
