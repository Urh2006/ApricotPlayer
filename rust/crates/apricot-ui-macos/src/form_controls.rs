//! Form controls of the dialogs with the Windows keyboard contract: a
//! pop-up button that behaves like a drop-down combo box and a slider that
//! behaves like a trackbar. Both are always in the Tab order, Return presses
//! the default button and Escape the cancel button, as `IsDialogMessageW`
//! sends `IDOK` and `IDCANCEL`.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::time::Instant;

use apricot_shell::TypeAhead;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityPostNotification, NSAccessibilityValueChangedNotification,
    NSButton, NSControl, NSEvent, NSPopUpButton, NSResponder, NSSlider, NSView,
};
use objc2_foundation::{NSInteger, NSRect, NSString};

use crate::controls::{handle_tab, modifiers, scroll_into_view};
use crate::keys;

/// Return and Escape go to the window's default and cancel buttons.
fn dialog_key(view: &NSView, event: &NSEvent) -> bool {
    let modifiers = modifiers(event);
    if modifiers.command || modifiers.option || modifiers.control {
        return false;
    }
    if !matches!(
        event.keyCode(),
        keys::KEY_RETURN | keys::KEY_KEYPAD_ENTER | keys::KEY_ESCAPE
    ) {
        return false;
    }
    view.window()
        .is_some_and(|window| window.performKeyEquivalent(event))
}

fn send_action(control: &NSControl) {
    if let Some(action) = control.action() {
        // SAFETY: the action exists on the target it was set with.
        unsafe {
            let _: bool = msg_send![control, sendAction: action, to: control.target().as_deref()];
        }
    }
}

/// Speaks the new value of a focused control.
pub fn notify_value_changed(view: &NSView) {
    // SAFETY: AppKit's static notification name on a live view.
    unsafe { NSAccessibilityPostNotification(view, NSAccessibilityValueChangedNotification) };
}

#[derive(Default)]
pub struct PopUpIvars {
    type_ahead: RefCell<TypeAhead>,
}

define_class!(
    /// A drop-down list (Windows `CBS_DROPDOWNLIST`): the arrow keys, Home,
    /// End and letters change the selection at once and send the action
    /// (`CBN_SELCHANGE`); Space opens the list.
    // SAFETY: NSPopUpButton may be subclassed; no Drop impl.
    #[unsafe(super(NSPopUpButton, NSButton, NSControl, NSView, NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotPopUp"]
    #[ivars = PopUpIvars]
    pub struct PopUp;

    impl PopUp {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeKeyView))]
        fn can_become_key_view(&self) -> bool {
            self.isEnabled() && !self.isHiddenOrHasHiddenAncestor()
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            scroll_into_view(self);
            // SAFETY: forwarding to NSPopUpButton.
            unsafe { msg_send![super(self), becomeFirstResponder] }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !(handle_tab(self, event) || dialog_key(self, event) || self.navigate(event)) {
                self.ivars().type_ahead.borrow_mut().reset();
                // SAFETY: forwarding the unhandled event to NSPopUpButton.
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }
    }
);

impl PopUp {
    pub fn new(
        mtm: MainThreadMarker,
        label: &str,
        target: &AnyObject,
        action: Sel,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PopUpIvars::default());
        // SAFETY: the designated initializer of NSPopUpButton.
        let popup: Retained<Self> =
            unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO, pullsDown: false] };
        popup.setAccessibilityLabel(Some(&NSString::from_str(label)));
        // SAFETY: the target outlives the pop-up (it owns the dialog).
        unsafe {
            popup.setTarget(Some(target));
            popup.setAction(Some(action));
        }
        popup
    }

    /// Replaces the items and selects `selected` without sending the action
    /// (Windows `CB_RESETCONTENT`, `CB_ADDSTRING` and `CB_SETCURSEL`).
    pub fn set_items(&self, items: &[String], selected: usize) {
        self.removeAllItems();
        for item in items {
            // `addItemWithTitle:` merges equal titles; the menu keeps them.
            let menu_item = objc2_app_kit::NSMenuItem::new(self.mtm());
            menu_item.setTitle(&NSString::from_str(item));
            if let Some(menu) = self.menu() {
                menu.addItem(&menu_item);
            }
        }
        self.select(selected);
        self.ivars().type_ahead.borrow_mut().reset();
    }

    pub fn select(&self, index: usize) {
        if let Ok(index) = NSInteger::try_from(index)
            && index < self.numberOfItems()
        {
            self.selectItemAtIndex(index);
        }
    }

    pub fn selected(&self) -> Option<usize> {
        usize::try_from(self.indexOfSelectedItem()).ok()
    }

    fn titles(&self) -> Vec<String> {
        (0..self.numberOfItems())
            .map(|index| self.itemTitleAtIndex(index).to_string())
            .collect()
    }

    /// The Windows combo box keys; `true` when handled.
    fn navigate(&self, event: &NSEvent) -> bool {
        let modifiers = modifiers(event);
        if modifiers.command || modifiers.option || modifiers.control {
            return false;
        }
        let count = usize::try_from(self.numberOfItems()).unwrap_or_default();
        if count == 0 {
            return false;
        }
        let current = self.selected().unwrap_or_default();
        let target = match event.keyCode() {
            keys::KEY_UP | keys::KEY_LEFT => Some(current.saturating_sub(1)),
            keys::KEY_DOWN | keys::KEY_RIGHT => Some((current + 1).min(count - 1)),
            keys::KEY_HOME | keys::KEY_PAGE_UP => Some(0),
            keys::KEY_END | keys::KEY_PAGE_DOWN => Some(count - 1),
            _ => {
                let Some(character) = single_letter(event) else {
                    return false;
                };
                let titles = self.titles();
                let found = self.ivars().type_ahead.borrow_mut().type_character(
                    &titles,
                    self.selected(),
                    character,
                    Instant::now(),
                );
                // A letter without a match is still consumed, as in Windows.
                found.or(Some(current))
            }
        };
        if let Some(target) = target
            && target != current
        {
            self.select(target);
            notify_value_changed(self);
            send_action(self);
        }
        true
    }
}

fn single_letter(event: &NSEvent) -> Option<char> {
    let characters = event.characters()?.to_string();
    let mut letters = characters.chars();
    let (Some(character), None) = (letters.next(), letters.next()) else {
        return None;
    };
    character.is_alphanumeric().then_some(character)
}

pub struct SliderIvars {
    line_step: Cell<i32>,
    page_step: Cell<i32>,
}

define_class!(
    /// A trackbar: the arrow keys move by the line step, Page Up and Page
    /// Down by the page step, Home and End to the ends, each change sends
    /// the action (`WM_HSCROLL`). Up and Right raise the value as macOS
    /// sliders do. VoiceOver's increment and decrement move by the line step.
    // SAFETY: NSSlider may be subclassed; no Drop impl.
    #[unsafe(super(NSSlider, NSControl, NSView, NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSlider"]
    #[ivars = SliderIvars]
    pub struct Slider;

    impl Slider {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeKeyView))]
        fn can_become_key_view(&self) -> bool {
            self.isEnabled() && !self.isHiddenOrHasHiddenAncestor()
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            scroll_into_view(self);
            // SAFETY: forwarding to NSSlider.
            unsafe { msg_send![super(self), becomeFirstResponder] }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !(handle_tab(self, event) || dialog_key(self, event) || self.slider_key(event)) {
                // SAFETY: forwarding the unhandled event to NSSlider.
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }

        #[unsafe(method(accessibilityPerformIncrement))]
        fn accessibility_increment(&self) -> bool {
            self.step(self.ivars().line_step.get());
            true
        }

        #[unsafe(method(accessibilityPerformDecrement))]
        fn accessibility_decrement(&self) -> bool {
            self.step(-self.ivars().line_step.get());
            true
        }
    }
);

impl Slider {
    pub fn new(
        mtm: MainThreadMarker,
        target: &AnyObject,
        action: Sel,
        line_step: i32,
        page_step: i32,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SliderIvars {
            line_step: Cell::new(line_step),
            page_step: Cell::new(page_step),
        });
        // SAFETY: `initWithFrame:` of NSSlider.
        let slider: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        // SAFETY: the target outlives the slider (it owns the dialog).
        unsafe {
            slider.setTarget(Some(target));
            slider.setAction(Some(action));
        }
        slider.setContinuous(true);
        slider
    }

    /// Windows `TBM_SETRANGEMIN`/`TBM_SETRANGEMAX`.
    pub fn set_range(&self, limit: i32) {
        self.setMinValue(f64::from(-limit));
        self.setMaxValue(f64::from(limit));
    }

    pub fn position(&self) -> i32 {
        i32::try_from(self.integerValue()).unwrap_or_default()
    }

    /// The position, its accessible name and value text; `notify` speaks
    /// the new value (Windows `annotate_slider`).
    pub fn set_value(&self, tenths: i32, label: &str, value: &str, notify: bool) {
        self.setIntegerValue(NSInteger::try_from(tenths).unwrap_or_default());
        self.setAccessibilityLabel(Some(&NSString::from_str(label)));
        self.setAccessibilityValueDescription(Some(&NSString::from_str(value)));
        if notify {
            notify_value_changed(self);
        }
    }

    fn step(&self, delta: i32) {
        self.move_to(self.position().saturating_add(delta));
    }

    fn move_to(&self, position: i32) {
        // The range is whole tenths, so the float limits are exact.
        #[allow(clippy::cast_possible_truncation)]
        let (low, high) = (self.minValue() as i32, self.maxValue() as i32);
        let position = position.clamp(low, high);
        if position != self.position() {
            self.setIntegerValue(NSInteger::try_from(position).unwrap_or_default());
            send_action(self);
        }
    }

    fn slider_key(&self, event: &NSEvent) -> bool {
        let modifiers = modifiers(event);
        if modifiers.command || modifiers.option || modifiers.control || modifiers.shift {
            return false;
        }
        let (line, page) = (self.ivars().line_step.get(), self.ivars().page_step.get());
        match event.keyCode() {
            keys::KEY_UP | keys::KEY_RIGHT => self.step(line),
            keys::KEY_DOWN | keys::KEY_LEFT => self.step(-line),
            keys::KEY_PAGE_UP => self.step(page),
            keys::KEY_PAGE_DOWN => self.step(-page),
            keys::KEY_HOME => self.move_to(i32::MIN),
            keys::KEY_END => self.move_to(i32::MAX),
            _ => return false,
        }
        true
    }
}
