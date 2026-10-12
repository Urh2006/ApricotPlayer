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

/// Scrolls a focused control into view inside a scrolling dialog.
pub fn scroll_into_view(view: &NSView) {
    view.scrollRectToVisible(view.bounds());
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
    /// Sent to the target on Delete (Windows `VK_DELETE` in a list).
    delete: Cell<Option<Sel>>,
    /// Sent to the target for the context menu instead of showing `menu`.
    context: Cell<Option<Sel>>,
    /// Sent to the target when the selected row changes
    /// (Windows `LBN_SELCHANGE`).
    selection: Cell<Option<Sel>>,
    /// Sent to the target on End, or Down at the last row, before the list
    /// moves (Windows `rss_list_subclass_proc`).
    end: Cell<Option<Sel>>,
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
        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, _notification: &objc2_foundation::NSNotification) {
            if let Some(action) = self.ivars().selection.get() {
                self.send(action);
            }
        }

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
        /// VoiceOver's "show menu" (VO+Shift+M).
        #[unsafe(method(accessibilityPerformShowMenu))]
        fn accessibility_show_menu(&self) -> bool {
            self.show_context_menu();
            true
        }

        /// A right click selects the row under the pointer and opens the
        /// shell's menu when the list has a context action.
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, event: &NSEvent) -> Option<Retained<objc2_app_kit::NSMenu>> {
            if self.ivars().context.get().is_some() {
                let point = self.convertPoint_fromView(event.locationInWindow(), None);
                if let Ok(row) = usize::try_from(self.rowAtPoint(point)) {
                    self.select(row);
                }
                self.show_context_menu();
                None
            } else {
                self.menu()
            }
        }

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
            if plain
                && !modifiers.shift
                && matches!(code, keys::KEY_DELETE | keys::KEY_FORWARD_DELETE)
                && let Some(action) = self.ivars().delete.get()
            {
                self.send(action);
                return;
            }
            if keys::is_context_menu_key(code, modifiers) {
                self.show_context_menu();
                return;
            }
            if plain
                && let Some(action) = self.ivars().end.get()
                && let Ok(count) = usize::try_from(self.numberOfRows())
                && count > 0
                && (code == keys::KEY_END
                    || (code == keys::KEY_DOWN && self.selected() == Some(count - 1)))
            {
                self.send(action);
            }
            if plain && !modifiers.shift && self.move_by_page_key(code) {
                self.ivars().type_ahead.borrow_mut().reset();
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

    /// Replaces one row, keeping the selection.
    pub fn set_row(&self, index: usize, label: &str) {
        {
            let mut rows = self.ivars().rows.borrow_mut();
            let Some(row) = rows.get_mut(index) else {
                return;
            };
            *row = label.replace('\t', "  ");
        }
        self.reloadDataForRowIndexes_columnIndexes(
            &NSIndexSet::indexSetWithIndex(index),
            &NSIndexSet::indexSetWithIndex(0),
        );
    }

    pub fn select(&self, row: usize) {
        let Ok(index) = NSInteger::try_from(row) else {
            return;
        };
        self.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
        self.scrollRowToVisible(index);
    }

    /// The text of the selected row.
    pub fn selected_text(&self) -> Option<String> {
        let index = self.selected()?;
        self.ivars().rows.borrow().get(index).cloned()
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

    /// The selector sent to the target when the selection changes.
    pub fn set_selection_action(&self, action: Sel) {
        self.ivars().selection.set(Some(action));
    }

    /// Home, End, Page Up and Page Down move the selection as in a Windows
    /// list box (`NSTableView` only scrolls); `true` when `code` was one.
    fn move_by_page_key(&self, code: u16) -> bool {
        if !matches!(
            code,
            keys::KEY_HOME | keys::KEY_END | keys::KEY_PAGE_UP | keys::KEY_PAGE_DOWN
        ) {
            return false;
        }
        let Ok(count) = usize::try_from(self.numberOfRows()) else {
            return true;
        };
        if count == 0 {
            return true;
        }
        let row_height = self.rowHeight() + self.intercellSpacing().height;
        let visible = self.visibleRect().size.height;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let page = if row_height > 0.0 {
            ((visible / row_height).floor() as usize)
                .saturating_sub(1)
                .max(1)
        } else {
            1
        };
        let current = self.selected().unwrap_or_default();
        let target = match code {
            keys::KEY_HOME => 0,
            keys::KEY_END => count - 1,
            keys::KEY_PAGE_UP => current.saturating_sub(page),
            _ => (current + page).min(count - 1),
        };
        if Some(target) != self.selected() {
            self.select(target);
        }
        true
    }

    /// The selector sent to the target on End or Down at the last row.
    pub fn set_end_action(&self, action: Sel) {
        self.ivars().end.set(Some(action));
    }

    /// The selector sent to the target on Delete.
    pub fn set_delete_action(&self, action: Sel) {
        self.ivars().delete.set(Some(action));
    }

    fn activate(&self) {
        if let Some(action) = self.ivars().activate.get() {
            self.send(action);
        }
    }

    fn send(&self, action: Sel) {
        // SAFETY: the action exists on the target set with it.
        unsafe {
            let _: bool = msg_send![self, sendAction: action, to: self.target().as_deref()];
        }
    }

    /// The selector sent to the target for the context menu (Shift+F10,
    /// VoiceOver's menu command and a right click).
    pub fn set_context_menu_action(&self, action: Sel) {
        self.ivars().context.set(Some(action));
    }

    fn show_context_menu(&self) {
        if let Some(action) = self.ivars().context.get() {
            self.send(action);
            return;
        }
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

        /// Disabled and hidden buttons are skipped, as `WS_TABSTOP` controls
        /// are on Windows.
        #[unsafe(method(canBecomeKeyView))]
        fn can_become_key_view(&self) -> bool {
            self.isEnabled() && !self.isHiddenOrHasHiddenAncestor()
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            scroll_into_view(self);
            // SAFETY: forwarding to NSButton.
            unsafe { msg_send![super(self), becomeFirstResponder] }
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
