//! The Settings window, the macOS counterpart of `settings_win32.rs`: Back,
//! Save (the default button) and Restore to defaults, the section list and
//! the scrolling controls of the selected section, in that Tab order, modal
//! over the main window. Arrowing through the sections applies the visible
//! controls once and renders the new section 140 ms after the last move;
//! Tab or Return in the list renders it at once and moves into it. Global
//! shortcuts close the window and run in the main window. The decisions are
//! made by the shell's `settings_*` methods.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use apricot_app::equalizer::EqualizerGains;
use apricot_app::{SettingsCommand, SettingsControl, ShortcutActionItem};
use apricot_core::action::{ActionScope, RepeatPolicy};
use apricot_core::shortcut::{ShortcutContext, action_for_shortcut};
use apricot_core::{SettingId, SettingsSection};
use apricot_shell::{
    ChoiceDialog, EqualizerView, SettingsBinding, SettingsInput, Shell, ShellUi, ShortcutCapture,
    settings_binding, shortcut_action_label,
};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAlert, NSAlertFirstButtonReturn, NSApplication, NSBackingStoreType,
    NSControlTextEditingDelegate, NSEvent, NSEventType, NSModalResponseOK, NSOpenPanel,
    NSResponder, NSSavePanel, NSScrollView, NSSecureTextField, NSTextField, NSTextFieldDelegate,
    NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSNotification, NSObject, NSObjectNSDelayedPerforming, NSObjectProtocol, NSPoint,
    NSRect, NSRunLoopCommonModes, NSSize, NSString, NSTimer, NSURL,
};

use crate::controls::{self, Button, List};
use crate::form_controls::{CheckBox, PopUp, Slider};
use crate::keys;
use crate::window::MainWindowController;

const WIDTH: f64 = 1_000.0;
const HEIGHT: f64 = 720.0;
const MARGIN: f64 = 12.0;
const BUTTON_HEIGHT: f64 = 32.0;
const SECTION_WIDTH: f64 = 220.0;
/// Windows `control_row_height`.
const ROW: f64 = 34.0;
const LIST_ROW: f64 = 268.0;
/// Python waits 140 ms after the last arrow key in the section list.
const SECTION_RENDER_DELAY: f64 = 0.14;

define_class!(
    // SAFETY: NSView may be subclassed; no Drop impl.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSettingsContent"]
    struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

define_class!(
    /// The shortcut capture field: every key but Tab is recorded (Windows
    /// `settings_control_proc` with `DLGC_WANTMESSAGE`).
    // SAFETY: NSTextField may be subclassed; no Drop impl.
    #[unsafe(super(NSTextField, objc2_app_kit::NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotShortcutField"]
    struct ShortcutField;

    impl ShortcutField {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeKeyView))]
        fn can_become_key_view(&self) -> bool {
            true
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            controls::scroll_into_view(self);
            // SAFETY: forwarding to NSTextField.
            unsafe { msg_send![super(self), becomeFirstResponder] }
        }

        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            // Command chords are captured too, not menu or button keys.
            let focused = self.window().is_some_and(|window| {
                window.firstResponder().is_some_and(|responder| {
                    std::ptr::eq(
                        Retained::as_ptr(&responder).cast::<u8>(),
                        std::ptr::from_ref(self).cast::<u8>(),
                    )
                })
            });
            let capture = focused && event.keyCode() != keys::KEY_TAB;
            if capture {
                self.capture(event);
            }
            capture
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if event.keyCode() == keys::KEY_TAB {
                controls::handle_tab(self, event);
                return;
            }
            self.capture(event);
        }
    }
);

impl ShortcutField {
    fn new(mtm: MainThreadMarker, label: &str, value: &str) -> Retained<Self> {
        // SAFETY: `initWithFrame:` of NSTextField.
        let field: Retained<Self> =
            unsafe { msg_send![Self::alloc(mtm), initWithFrame: NSRect::ZERO] };
        field.setEditable(false);
        field.setSelectable(false);
        field.setStringValue(&NSString::from_str(
            &apricot_core::shortcut::display_shortcut(value),
        ));
        field.setAccessibilityLabel(Some(&NSString::from_str(label)));
        // SAFETY: AppKit's static role name.
        field.setAccessibilityRole(Some(unsafe { objc2_app_kit::NSAccessibilityTextFieldRole }));
        field
    }

    fn capture(&self, event: &NSEvent) {
        let Some(characters) = event.charactersIgnoringModifiers() else {
            return;
        };
        let modifiers = controls::modifiers(event);
        let Some(chord) = keys::chord(event.keyCode(), &characters.to_string(), modifiers) else {
            return;
        };
        let target = self.target();
        if let (Some(target), Some(action)) = (target, self.action()) {
            CAPTURED.with(|captured| captured.set(Some(chord)));
            // SAFETY: the action exists on the target (the settings window).
            unsafe {
                let _: bool = msg_send![self, sendAction: action, to: &*target];
            }
        }
    }
}

thread_local! {
    /// The open Settings window, for results that arrive while it is shown.
    static OPEN: RefCell<Option<Retained<SettingsWindow>>> = const { RefCell::new(None) };
    /// The chord the capture field just reported.
    static CAPTURED: Cell<Option<apricot_core::shortcut::ShortcutChord>> = const { Cell::new(None) };
}

/// One control of the section (Windows `BoundControl`).
struct Bound {
    label: Option<Retained<NSTextField>>,
    view: Retained<NSView>,
    kind: ControlKind,
    binding: SettingsBinding,
}

enum ControlKind {
    Field(Retained<NSTextField>),
    Choice(Retained<PopUp>),
    Check(Retained<CheckBox>),
    Slider(Retained<Slider>),
    List(Retained<List>, Retained<NSScrollView>),
    Capture(Retained<ShortcutField>),
    Button(Retained<Button>),
}

pub struct SettingsIvars {
    controller: Weak<MainWindowController>,
    window: RefCell<Option<Retained<NSWindow>>>,
    back: RefCell<Option<Retained<Button>>>,
    save: RefCell<Option<Retained<Button>>>,
    reset_all: RefCell<Option<Retained<Button>>>,
    sections: RefCell<Option<Retained<List>>>,
    scroll: RefCell<Option<Retained<NSScrollView>>>,
    document: RefCell<Option<Retained<FlippedView>>>,
    controls: RefCell<Vec<Bound>>,
    selected_section: Cell<SettingsSection>,
    pending_section: Cell<Option<SettingsSection>>,
    displayed_language: RefCell<String>,
    deferred_action: Cell<Option<&'static str>>,
    audio_devices: RefCell<Option<Receiver<Vec<apricot_playback::AudioOutputDevice>>>>,
    timer: RefCell<Option<Retained<NSTimer>>>,
    /// A key reached the window since it opened.
    user_moved: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSettingsDialog"]
    #[ivars = SettingsIvars]
    struct SettingsWindow;

    unsafe impl NSObjectProtocol for SettingsWindow {}

    unsafe impl NSWindowDelegate for SettingsWindow {
        /// Windows `WM_CLOSE`: Back without saving.
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    unsafe impl NSControlTextEditingDelegate for SettingsWindow {
        /// Windows `EN_KILLFOCUS`: a text field applies its value.
        #[unsafe(method(controlTextDidEndEditing:))]
        fn control_text_did_end_editing(&self, notification: &NSNotification) {
            if let Some(object) = notification.object() {
                self.control_changed(Retained::as_ptr(&object).cast());
            }
        }
    }

    unsafe impl NSTextFieldDelegate for SettingsWindow {}

    impl SettingsWindow {
        #[unsafe(method(back:))]
        fn back(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(save:))]
        fn save(&self, _sender: Option<&AnyObject>) {
            self.save_settings();
        }

        #[unsafe(method(restoreDefaults:))]
        fn restore_defaults(&self, _sender: Option<&AnyObject>) {
            self.restore_all();
        }

        /// Windows `LBN_SELCHANGE` of the section list.
        #[unsafe(method(sectionChanged:))]
        fn section_changed(&self, _sender: Option<&AnyObject>) {
            self.schedule_section_render();
        }

        /// Tab or Return in the section list.
        #[unsafe(method(sectionCommit:))]
        fn section_commit(&self, _sender: Option<&AnyObject>) {
            self.flush_section_render();
            self.focus_first_control();
        }

        #[unsafe(method(focusSections:))]
        fn focus_sections(&self, _sender: Option<&AnyObject>) {
            self.focus_section_list();
        }

        /// The start of the modal session can put focus on Back, the first
        /// key view; the section list gets it back unless the user moved.
        #[unsafe(method(focusSectionsIfUnmoved:))]
        fn focus_sections_if_unmoved(&self, _sender: Option<&AnyObject>) {
            let back = self.ivars().back.borrow().clone();
            let on_back = self.window().is_some_and(|window| {
                window.firstResponder().is_some_and(|responder| {
                    back.as_ref().is_some_and(|back| {
                        std::ptr::eq(
                            Retained::as_ptr(&responder).cast::<u8>(),
                            Retained::as_ptr(back).cast::<u8>(),
                        )
                    })
                })
            });
            if on_back && !self.ivars().user_moved.get() {
                self.focus_section_list();
            }
        }

        #[unsafe(method(renderSection:))]
        fn render_section(&self, _sender: Option<&AnyObject>) {
            self.flush_section_render();
        }

        /// A check box, choice, slider or button of the section.
        #[unsafe(method(controlChanged:))]
        fn control_changed_action(&self, sender: Option<&AnyObject>) {
            if let Some(sender) = sender {
                self.control_changed(std::ptr::from_ref(sender).cast());
            }
        }

        #[unsafe(method(shortcutSelected:))]
        fn shortcut_selected(&self, _sender: Option<&AnyObject>) {
            self.sync_shortcut_selection();
        }

        #[unsafe(method(shortcutCaptured:))]
        fn shortcut_captured(&self, _sender: Option<&AnyObject>) {
            if let Some(chord) = CAPTURED.with(Cell::take) {
                self.capture_shortcut(chord);
            }
        }

        /// Windows `focus_adjacent_menu_checkbox`.
        #[unsafe(method(menuArrow:))]
        fn menu_arrow(&self, sender: Option<&AnyObject>) {
            if let Some(sender) = sender {
                self.focus_adjacent_menu_checkbox(std::ptr::from_ref(sender).cast());
            }
        }

        /// Windows timers 2 (audio devices) and 3 (equalizer apply).
        #[unsafe(method(pollDevices:))]
        fn poll_devices(&self, _timer: Option<&AnyObject>) {
            self.finish_audio_device_refresh();
        }

        #[unsafe(method(applyEqualizer:))]
        fn apply_equalizer(&self, _sender: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.settings_apply_equalizer(ui, true));
        }
    }
);

impl SettingsWindow {
    fn focus_section_list(&self) {
        let sections = self.ivars().sections.borrow().clone();
        if let (Some(sections), Some(window)) = (sections, self.window()) {
            window.makeFirstResponder(Some(&sections));
        }
    }

    fn with_shell<R>(&self, body: impl FnOnce(&mut Shell, &mut dyn ShellUi) -> R) -> Option<R> {
        self.ivars().controller.load()?.with_shell(body)
    }

    fn mtm(&self) -> MainThreadMarker {
        MainThreadMarker::from(self)
    }

    fn window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().window.borrow().clone()
    }

    /// Windows `WindowsAnnouncer::announce`.
    fn announce(&self, message: &str, interrupt: bool) {
        crate::announce::announce(self.mtm(), message, interrupt);
    }

    /// Windows `show_error` and `show_message`; focus returns to the
    /// control that had it.
    fn message(&self, title: Option<&str>, text: &str) {
        let window = self.window();
        let previous = window.as_ref().and_then(|window| window.firstResponder());
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(text));
        if let Some(title) = title {
            alert.window().setTitle(&NSString::from_str(title));
        }
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        alert.runModal();
        if let Some(window) = window {
            window.makeKeyAndOrderFront(None);
            if let Some(previous) = previous {
                window.makeFirstResponder(Some(&previous));
            }
        }
    }

    fn show_error(&self, text: &str) {
        self.message(None, text);
    }

    /// The input a control shows.
    fn input(bound: &Bound) -> SettingsInput {
        match &bound.kind {
            ControlKind::Field(field) => SettingsInput::Text(field.stringValue().to_string()),
            ControlKind::Choice(popup) => SettingsInput::Selected(popup.selected()),
            ControlKind::Check(check) => SettingsInput::Checked(check.checked()),
            ControlKind::Slider(slider) => SettingsInput::Position(i64::from(slider.position())),
            ControlKind::Capture(field) => SettingsInput::Text(field.stringValue().to_string()),
            ControlKind::List(..) | ControlKind::Button(_) => SettingsInput::None,
        }
    }

    /// Windows `visible_equalizer_gains`.
    fn visible_gains(&self) -> EqualizerGains {
        self.ivars()
            .controls
            .borrow()
            .iter()
            .filter_map(|bound| match (&bound.binding, &bound.kind) {
                (SettingsBinding::EqualizerBand { band_id, .. }, ControlKind::Slider(slider)) => {
                    Some((
                        (*band_id).to_owned(),
                        apricot_app::equalizer::gain_from_slider(slider.position()),
                    ))
                }
                _ => None,
            })
            .collect()
    }

    /// Windows `sync_bound_control` for one control.
    fn sync(&self, index: usize) -> Result<(), String> {
        let gains = self.visible_gains();
        let (binding, input) = {
            let controls = self.ivars().controls.borrow();
            let Some(bound) = controls.get(index) else {
                return Ok(());
            };
            let input = match (&bound.binding, &bound.kind) {
                // The capture field shows the macOS spelling; the stored
                // shortcut is already saved when it was captured.
                (SettingsBinding::ShortcutCapture(_), _) => return Ok(()),
                _ => Self::input(bound),
            };
            (bound.binding.clone(), input)
        };
        self.with_shell(|shell, _| shell.settings_apply(&binding, &input, &gains))
            .unwrap_or(Ok(()))
    }

    /// Windows `sync_all_controls`.
    fn sync_all(&self) -> Result<(), String> {
        let count = self.ivars().controls.borrow().len();
        for index in 0..count {
            self.sync(index)?;
        }
        Ok(())
    }

    fn index_of(&self, view: *const NSView) -> Option<usize> {
        self.ivars()
            .controls
            .borrow()
            .iter()
            .position(|bound| std::ptr::eq(Retained::as_ptr(&bound.view), view))
    }

    /// Windows `activate_dynamic_control` and `handle_slider_change`.
    fn control_changed(&self, view: *const NSView) {
        let Some(index) = self.index_of(view) else {
            return;
        };
        let binding = self.ivars().controls.borrow()[index].binding.clone();
        if let SettingsBinding::Command(command) = binding {
            self.handle_command(command);
            return;
        }
        if matches!(binding, SettingsBinding::ShortcutActionList(_)) {
            self.sync_shortcut_selection();
            return;
        }
        if let Err(error) = self.sync(index) {
            self.show_error(&error);
            return;
        }
        self.update_slider_name(index);
        let gains = self.visible_gains();
        if let SettingsBinding::EqualizerBand { preset_id, .. } = &binding {
            let preview = self
                .with_shell(|shell, _| shell.settings_equalizer_preview(preset_id, &gains))
                .unwrap_or(false);
            if preview {
                self.schedule_equalizer_apply();
            }
            return;
        }
        self.with_shell(|shell, ui| shell.settings_equalizer_changed(ui, &binding, &gains));
        if let SettingsBinding::EqualizerPresetName(preset_id) = &binding {
            self.rename_preset_choice(preset_id);
        }
        if let Some(setting) = binding.dependent_setting() {
            self.render_controls();
            self.focus_setting(setting);
        }
    }

    /// Windows `update_slider_accessible_name`.
    fn update_slider_name(&self, index: usize) {
        let controls = self.ivars().controls.borrow();
        let Some(bound) = controls.get(index) else {
            return;
        };
        let ControlKind::Slider(slider) = &bound.kind else {
            return;
        };
        let position = slider.position();
        match &bound.binding {
            SettingsBinding::IntegerSlider { label, unit, .. } => {
                let value = if unit.is_empty() {
                    position.to_string()
                } else {
                    format!("{position} {unit}")
                };
                slider.set_value(position, label, &value, true);
            }
            SettingsBinding::EqualizerBand { label, .. } => {
                slider.set_value(
                    position,
                    label,
                    &apricot_app::equalizer::slider_value_text(position),
                    true,
                );
            }
            _ => {}
        }
    }

    /// Windows `equalizer_setting_changed` for a new preset name: the
    /// preset choice shows it at once.
    fn rename_preset_choice(&self, preset_id: &str) {
        let label = self
            .with_shell(|shell, _| {
                shell
                    .application
                    .equalizer_settings()
                    .custom_name(preset_id)
            })
            .unwrap_or_default();
        for bound in self.ivars().controls.borrow().iter() {
            if let (
                SettingsBinding::Choice {
                    setting: SettingId::GlobalEqualizerPreset,
                    values,
                    ..
                },
                ControlKind::Choice(popup),
            ) = (&bound.binding, &bound.kind)
                && let Some(index) = values.iter().position(|value| value == preset_id)
                && let Some(item) = popup.itemAtIndex(isize::try_from(index).unwrap_or_default())
            {
                item.setTitle(&NSString::from_str(&label));
            }
        }
    }

    fn schedule_equalizer_apply(&self) {
        // SAFETY: `applyEqualizer:` exists on this object.
        unsafe {
            NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(
                self.as_ref(),
                sel!(applyEqualizer:),
                None,
            );
            let modes = NSArray::from_slice(&[NSRunLoopCommonModes]);
            let _: () = msg_send![
                self,
                performSelector: sel!(applyEqualizer:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: f64::from(apricot_app::equalizer::APPLY_DELAY_MS) / 1_000.0,
                inModes: &*modes
            ];
        }
    }

    /// Windows `sync_shortcut_selection`: the capture field follows the
    /// selected action.
    fn sync_shortcut_selection(&self) {
        let mut controls = self.ivars().controls.borrow_mut();
        let selected = controls
            .iter()
            .find_map(|bound| match (&bound.binding, &bound.kind) {
                (SettingsBinding::ShortcutActionList(actions), ControlKind::List(list, _)) => list
                    .selected()
                    .and_then(|index| actions.get(index).cloned()),
                _ => None,
            });
        let Some(action) = selected else {
            return;
        };
        for bound in controls.iter_mut() {
            if let (SettingsBinding::ShortcutCapture(action_id), ControlKind::Capture(field)) =
                (&mut bound.binding, &bound.kind)
            {
                action_id.clone_from(&action.action_id);
                field.setStringValue(&NSString::from_str(
                    &apricot_core::shortcut::display_shortcut(&action.shortcut),
                ));
                break;
            }
        }
    }

    /// Windows `capture_shortcut`.
    fn capture_shortcut(&self, chord: apricot_core::shortcut::ShortcutChord) {
        let (action_id, actions) = {
            let controls = self.ivars().controls.borrow();
            let action_id = controls.iter().find_map(|bound| match &bound.binding {
                SettingsBinding::ShortcutCapture(action_id) => Some(action_id.clone()),
                _ => None,
            });
            let actions: Vec<ShortcutActionItem> = controls
                .iter()
                .find_map(|bound| match &bound.binding {
                    SettingsBinding::ShortcutActionList(actions) => Some(actions.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            (action_id, actions)
        };
        let Some(action_id) = action_id else {
            return;
        };
        let name = |id: &str| {
            actions
                .iter()
                .find(|action| action.action_id == id)
                .map_or_else(|| id.to_owned(), |action| action.label.clone())
        };
        let outcome = self
            .with_shell(|shell, _| shell.settings_capture_shortcut(&action_id, chord, &name))
            .unwrap_or(ShortcutCapture::Ignored);
        match outcome {
            ShortcutCapture::Ignored => {}
            ShortcutCapture::Captured { shortcut, message } => {
                self.show_captured(&action_id, &shortcut);
                self.announce(&message, true);
            }
            ShortcutCapture::Conflict { title, message } => {
                self.message(Some(&title), &message);
                self.announce(&message, true);
            }
            ShortcutCapture::Failed(error) => self.show_error(&error),
        }
    }

    /// Windows `update_shortcut_action_list` and the capture field text.
    fn show_captured(&self, action_id: &str, shortcut: &str) {
        let mut controls = self.ivars().controls.borrow_mut();
        for bound in controls.iter_mut() {
            match (&mut bound.binding, &bound.kind) {
                (SettingsBinding::ShortcutCapture(_), ControlKind::Capture(field)) => {
                    field.setStringValue(&NSString::from_str(
                        &apricot_core::shortcut::display_shortcut(shortcut),
                    ));
                }
                (SettingsBinding::ShortcutActionList(actions), ControlKind::List(list, _)) => {
                    if let Some(index) = actions
                        .iter()
                        .position(|action| action.action_id == action_id)
                    {
                        shortcut.clone_into(&mut actions[index].shortcut);
                        list.set_row(index, &shortcut_action_label(&actions[index]));
                    }
                }
                _ => {}
            }
        }
    }

    /// Windows `focus_adjacent_menu_checkbox`.
    fn focus_adjacent_menu_checkbox(&self, current: *const NSView) {
        let code = NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .filter(|event| event.r#type() == NSEventType::KeyDown)
            .map(|event| event.keyCode());
        let controls = self.ivars().controls.borrow();
        let menu: Vec<&Bound> = controls
            .iter()
            .filter(|bound| matches!(bound.binding, SettingsBinding::MenuItem(_)))
            .collect();
        let Some(index) = menu
            .iter()
            .position(|bound| std::ptr::eq(Retained::as_ptr(&bound.view), current))
        else {
            return;
        };
        let last = menu.len().saturating_sub(1);
        let target = match code {
            Some(keys::KEY_HOME) => 0,
            Some(keys::KEY_END) => last,
            Some(keys::KEY_UP) => index.saturating_sub(1),
            _ => (index + 1).min(last),
        };
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(&menu[target].view));
        }
    }

    /// Windows `schedule_section_render`.
    fn schedule_section_render(&self) {
        let Some(section) = self
            .ivars()
            .sections
            .borrow()
            .as_ref()
            .and_then(|list| list.selected())
            .and_then(|index| SettingsSection::ALL.get(index).copied())
        else {
            return;
        };
        if self.ivars().pending_section.get().is_none() {
            if section == self.ivars().selected_section.get() {
                return;
            }
            if let Err(error) = self.sync_all() {
                self.show_error(&error);
                return;
            }
        }
        self.ivars().pending_section.set(Some(section));
        // SAFETY: `renderSection:` exists on this object.
        unsafe {
            NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(
                self.as_ref(),
                sel!(renderSection:),
                None,
            );
            let modes = NSArray::from_slice(&[NSRunLoopCommonModes]);
            let _: () = msg_send![
                self,
                performSelector: sel!(renderSection:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: SECTION_RENDER_DELAY,
                inModes: &*modes
            ];
        }
    }

    /// Windows `flush_section_render`.
    fn flush_section_render(&self) {
        // SAFETY: `renderSection:` exists on this object.
        unsafe {
            NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(
                self.as_ref(),
                sel!(renderSection:),
                None,
            );
        }
        let Some(section) = self.ivars().pending_section.take() else {
            return;
        };
        self.ivars().selected_section.set(section);
        self.render_controls();
        if let Some(scroll) = self.ivars().scroll.borrow().as_ref() {
            scroll.contentView().scrollToPoint(NSPoint::new(0.0, 0.0));
        }
    }

    fn focus_first_control(&self) {
        let first = self
            .ivars()
            .controls
            .borrow()
            .first()
            .map(|bound| bound.view.clone());
        if let (Some(first), Some(window)) = (first, self.window()) {
            window.makeFirstResponder(Some(&first));
        }
    }

    fn focus_setting(&self, setting: SettingId) {
        let view = self
            .ivars()
            .controls
            .borrow()
            .iter()
            .find(|bound| bound.binding.setting() == Some(setting))
            .map(|bound| bound.view.clone());
        if let (Some(view), Some(window)) = (view, self.window()) {
            window.makeFirstResponder(Some(&view));
        }
    }

    fn focus_command(&self, command: SettingsCommand) {
        let view = self
            .ivars()
            .controls
            .borrow()
            .iter()
            .find(|bound| bound.binding == SettingsBinding::Command(command))
            .map(|bound| bound.view.clone());
        if let (Some(view), Some(window)) = (view, self.window()) {
            window.makeFirstResponder(Some(&view));
        }
    }

    /// Windows `render_controls`.
    fn render_controls(&self) {
        let section = self.ivars().selected_section.get();
        let Some(model) = self.with_shell(|shell, _| shell.settings_screen(section)) else {
            return;
        };
        let Some(document) = self.ivars().document.borrow().clone() else {
            return;
        };
        for bound in self.ivars().controls.borrow_mut().drain(..) {
            if let Some(label) = bound.label {
                label.removeFromSuperview();
            }
            match bound.kind {
                ControlKind::List(_, scroll) => scroll.removeFromSuperview(),
                _ => bound.view.removeFromSuperview(),
            }
        }
        let mtm = self.mtm();
        let controls: Vec<Bound> = model
            .controls
            .into_iter()
            .map(|control| self.create_control(mtm, &document, control))
            .collect();
        *self.ivars().controls.borrow_mut() = controls;
        self.layout();
        self.update_tab_order();
        if section == SettingsSection::Playback && self.ivars().audio_devices.borrow().is_none() {
            let probe = self
                .with_shell(|shell, _| shell.settings_audio_device_probe())
                .flatten();
            if let Some(probe) = probe {
                *self.ivars().audio_devices.borrow_mut() = Some(probe);
                self.start_device_timer();
            }
        }
    }

    fn start_device_timer(&self) {
        // SAFETY: the dialog implements `pollDevices:` and invalidates the
        // timer when the probe answered or the window closed.
        let timer = unsafe {
            NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                0.1,
                self,
                sel!(pollDevices:),
                None,
                true,
            )
        };
        // SAFETY: AppKit's static run loop mode, so the timer runs modally.
        unsafe {
            objc2_foundation::NSRunLoop::currentRunLoop()
                .addTimer_forMode(&timer, NSRunLoopCommonModes);
        }
        if let Some(previous) = self.ivars().timer.borrow_mut().replace(timer) {
            previous.invalidate();
        }
    }

    /// Windows `finish_audio_device_refresh`: the device choices change in
    /// place without moving focus.
    fn finish_audio_device_refresh(&self) {
        let devices = {
            let pending = self.ivars().audio_devices.borrow();
            let Some(pending) = pending.as_ref() else {
                return;
            };
            match pending.try_recv() {
                Ok(devices) => devices,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
            }
        };
        *self.ivars().audio_devices.borrow_mut() = None;
        if let Some(timer) = self.ivars().timer.borrow_mut().take() {
            timer.invalidate();
        }
        let showing = self.ivars().selected_section.get() == SettingsSection::Playback
            && self.ivars().pending_section.get().is_none();
        let mut controls = self.ivars().controls.borrow_mut();
        let device = controls.iter_mut().find(|bound| {
            matches!(
                bound.binding,
                SettingsBinding::Choice {
                    setting: SettingId::AudioOutputDevice,
                    ..
                }
            )
        });
        let selected = device
            .as_ref()
            .and_then(|bound| match (&bound.binding, &bound.kind) {
                (SettingsBinding::Choice { values, .. }, ControlKind::Choice(popup)) => popup
                    .selected()
                    .and_then(|index| values.get(index).cloned()),
                _ => None,
            });
        let Some((options, position)) =
            self.with_shell(|shell, _| shell.settings_audio_devices(&devices, selected.as_deref()))
        else {
            return;
        };
        if !showing {
            return;
        }
        if let Some(bound) = device
            && let (SettingsBinding::Choice { values, .. }, ControlKind::Choice(popup)) =
                (&mut bound.binding, &bound.kind)
        {
            let labels: Vec<String> = options.iter().map(|option| option.label.clone()).collect();
            popup.set_items(&labels, position);
            *values = options.into_iter().map(|option| option.value).collect();
        }
    }

    #[allow(clippy::too_many_lines)] // One arm per model control, as on Windows.
    fn create_control(
        &self,
        mtm: MainThreadMarker,
        document: &NSView,
        control: SettingsControl,
    ) -> Bound {
        let binding = settings_binding(&control);
        let target: &AnyObject = self.as_ref();
        let label_view = |text: &str| {
            let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
            document.addSubview(&label);
            label
        };
        let text_field = |value: &str, name: &str, secret: bool, read_only: bool| {
            let field: Retained<NSTextField> = if secret {
                Retained::into_super(NSSecureTextField::initWithFrame(
                    NSSecureTextField::alloc(mtm),
                    NSRect::ZERO,
                ))
            } else {
                NSTextField::initWithFrame(NSTextField::alloc(mtm), NSRect::ZERO)
            };
            field.setStringValue(&NSString::from_str(value));
            field.setAccessibilityLabel(Some(&NSString::from_str(name)));
            if read_only {
                field.setEditable(false);
                field.setSelectable(true);
            } else {
                // SAFETY: the dialog outlives the field; the delegate is weak.
                unsafe { field.setDelegate(Some(ProtocolObject::from_ref(self))) };
            }
            document.addSubview(&field);
            field
        };
        let popup = |name: &str, labels: &[String], selected: usize| {
            let popup = PopUp::new(mtm, name, target, sel!(controlChanged:));
            popup.set_items(labels, selected);
            document.addSubview(&popup);
            popup
        };
        let check = |name: &str, checked: bool| {
            let check = CheckBox::new(mtm, name, target, sel!(controlChanged:));
            check.set_checked(checked);
            document.addSubview(&check);
            check
        };
        let (label, kind) = match control {
            SettingsControl::ReadOnlyText { label, value, .. } => (
                Some(label_view(&label)),
                ControlKind::Field(text_field(&value, &label, false, true)),
            ),
            SettingsControl::Text {
                label,
                value,
                secret,
                ..
            } => (
                Some(label_view(&label)),
                ControlKind::Field(text_field(&value, &label, secret, false)),
            ),
            SettingsControl::Integer { label, value, .. } => (
                Some(label_view(&label)),
                ControlKind::Field(text_field(&value.to_string(), &label, false, false)),
            ),
            SettingsControl::Choice {
                label,
                value,
                options,
                ..
            }
            | SettingsControl::EqualizerDevicePresetChoice {
                label,
                value,
                options,
                ..
            } => {
                let selected = options
                    .iter()
                    .position(|option| option.value == value)
                    .unwrap_or_default();
                let labels: Vec<String> = options.into_iter().map(|option| option.label).collect();
                (
                    Some(label_view(&label)),
                    ControlKind::Choice(popup(&label, &labels, selected)),
                )
            }
            SettingsControl::SpotifyChoice {
                label,
                selected,
                options,
                ..
            } => (
                Some(label_view(&label)),
                ControlKind::Choice(popup(&label, &options, selected)),
            ),
            SettingsControl::Checkbox { label, checked, .. }
            | SettingsControl::SpotifyNormalisation { label, checked } => {
                (None, ControlKind::Check(check(&label, checked)))
            }
            SettingsControl::MenuItemCheckbox { label, checked, .. } => {
                let check = check(&label, checked);
                check.set_arrow_action(sel!(menuArrow:));
                (None, ControlKind::Check(check))
            }
            SettingsControl::IntegerSlider {
                label,
                value,
                minimum,
                maximum,
                unit,
                ..
            } => {
                let page = ((maximum - minimum) / 5).max(1);
                let slider = Slider::new(
                    mtm,
                    target,
                    sel!(controlChanged:),
                    1,
                    i32::try_from(page).unwrap_or(1),
                );
                slider.set_bounds(minimum, maximum);
                let position = i32::try_from(value).unwrap_or_default();
                let text = if unit.is_empty() {
                    value.to_string()
                } else {
                    format!("{value} {unit}")
                };
                slider.set_value(position, &label, &text, false);
                document.addSubview(&slider);
                (Some(label_view(&label)), ControlKind::Slider(slider))
            }
            SettingsControl::EqualizerBandSlider {
                label,
                value_db,
                minimum_db,
                maximum_db,
                ..
            } => {
                let slider = Slider::new(
                    mtm,
                    target,
                    sel!(controlChanged:),
                    apricot_app::equalizer::SLIDER_LINE_STEP,
                    apricot_app::equalizer::SLIDER_PAGE_STEP,
                );
                slider.set_bounds(minimum_db * 10, maximum_db * 10);
                let tenths = apricot_app::equalizer::slider_from_gain(value_db, maximum_db);
                slider.set_value(
                    tenths,
                    &label,
                    &apricot_app::equalizer::slider_value_text(tenths),
                    false,
                );
                document.addSubview(&slider);
                (Some(label_view(&label)), ControlKind::Slider(slider))
            }
            SettingsControl::EqualizerPresetName { label, value, .. } => (
                Some(label_view(&label)),
                ControlKind::Field(text_field(&value, &label, false, false)),
            ),
            SettingsControl::ShortcutActionList { label, actions } => {
                let list = List::new(mtm, &label);
                // SAFETY: the dialog outlives the list.
                unsafe { list.setTarget(Some(target)) };
                list.set_selection_action(sel!(shortcutSelected:));
                let rows = actions.iter().map(shortcut_action_label).collect();
                list.set_rows(rows, (!actions.is_empty()).then_some(0));
                let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), NSRect::ZERO);
                scroll.setDocumentView(Some(&list));
                scroll.setHasVerticalScroller(true);
                document.addSubview(&scroll);
                (Some(label_view(&label)), ControlKind::List(list, scroll))
            }
            SettingsControl::ShortcutCapture { label, value, .. } => {
                let field = ShortcutField::new(mtm, &label, &value);
                // SAFETY: the dialog outlives the field.
                unsafe {
                    field.setTarget(Some(target));
                    field.setAction(Some(sel!(shortcutCaptured:)));
                }
                document.addSubview(&field);
                (Some(label_view(&label)), ControlKind::Capture(field))
            }
            SettingsControl::Command { label, .. } => {
                let button = Button::new(mtm, &label, target, sel!(controlChanged:));
                document.addSubview(&button);
                (None, ControlKind::Button(button))
            }
        };
        let view: Retained<NSView> = match &kind {
            ControlKind::Field(field) => {
                let view: &NSView = field;
                view.retain()
            }
            ControlKind::Choice(popup) => {
                let view: &NSView = popup;
                view.retain()
            }
            ControlKind::Check(check) => {
                let view: &NSView = check;
                view.retain()
            }
            ControlKind::Slider(slider) => {
                let view: &NSView = slider;
                view.retain()
            }
            ControlKind::List(list, _) => {
                let view: &NSView = list;
                view.retain()
            }
            ControlKind::Capture(field) => {
                let view: &NSView = field;
                view.retain()
            }
            ControlKind::Button(button) => {
                let view: &NSView = button;
                view.retain()
            }
        };
        Bound {
            label,
            view,
            kind,
            binding,
        }
    }

    /// Windows `layout` of the section: a label column and a control
    /// column, rows of 34 points, the shortcut list taller.
    fn layout(&self) {
        let Some(document) = self.ivars().document.borrow().clone() else {
            return;
        };
        let width = self
            .ivars()
            .scroll
            .borrow()
            .as_ref()
            .map_or(WIDTH - SECTION_WIDTH - 3.0 * MARGIN, |scroll| {
                scroll.contentSize().width
            });
        let label_width = (width / 2.0).min(330.0);
        let mut y = 0.0;
        for bound in self.ivars().controls.borrow().iter() {
            let (height, row) = match bound.kind {
                ControlKind::List(..) => (260.0, LIST_ROW),
                _ => (30.0, ROW),
            };
            let frame = |x: f64, w: f64| NSRect::new(NSPoint::new(x, y), NSSize::new(w, height));
            let control_frame = if let Some(label) = &bound.label {
                label.setFrame(NSRect::new(
                    NSPoint::new(0.0, y + 5.0),
                    NSSize::new(label_width, 24.0),
                ));
                frame(label_width + 8.0, width - label_width - 8.0)
            } else {
                frame(0.0, width)
            };
            match &bound.kind {
                ControlKind::List(_, scroll) => scroll.setFrame(control_frame),
                _ => bound.view.setFrame(control_frame),
            }
            y += row;
        }
        document.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width, y.max(1.0)),
        ));
    }

    /// Back, Save, Restore to defaults, the section list, then the
    /// section's controls; Shift+Tab from the first control returns to the
    /// section list.
    fn update_tab_order(&self) {
        let (Some(back), Some(save), Some(reset_all), Some(sections)) = (
            self.ivars().back.borrow().clone(),
            self.ivars().save.borrow().clone(),
            self.ivars().reset_all.borrow().clone(),
            self.ivars().sections.borrow().clone(),
        ) else {
            return;
        };
        let controls = self.ivars().controls.borrow();
        let mut views: Vec<&NSView> = vec![&back, &save, &reset_all, &sections];
        views.extend(controls.iter().map(|bound| &*bound.view));
        controls::set_tab_order(&views);
    }

    /// Windows `handle_settings_command`.
    #[allow(clippy::too_many_lines)] // One arm per command, as on Windows.
    fn handle_command(&self, command: SettingsCommand) {
        match command {
            SettingsCommand::ResetSection => self.reset_section(),
            SettingsCommand::BrowseDownloadFolder => self.browse_download_folder(),
            SettingsCommand::ResetEqualizerPreset => self.reset_equalizer_preset(),
            SettingsCommand::AddEqualizerProfile => {
                let gains = self.visible_gains();
                let mut view = PromptView { dialog: self };
                let added = self
                    .with_shell(|shell, ui| {
                        shell.settings_add_equalizer_profile(ui, &mut view, &gains)
                    })
                    .unwrap_or(false);
                if added {
                    self.rerender_and_focus(SettingId::GlobalEqualizerPreset);
                }
            }
            SettingsCommand::ImportEqualizerProfile => {
                let mut view = PromptView { dialog: self };
                let imported = self
                    .with_shell(|shell, ui| shell.settings_import_equalizer_profile(ui, &mut view))
                    .unwrap_or(false);
                if imported {
                    self.rerender_and_focus(SettingId::GlobalEqualizerPreset);
                }
            }
            SettingsCommand::ExportEqualizerProfile => {
                let gains = self.visible_gains();
                let mut view = PromptView { dialog: self };
                self.with_shell(|shell, ui| {
                    shell.settings_export_equalizer_profile(ui, &mut view, &gains);
                });
                self.focus_command(command);
            }
            SettingsCommand::DeleteEqualizerProfile => {
                let mut view = PromptView { dialog: self };
                let deleted = self
                    .with_shell(|shell, ui| shell.settings_delete_equalizer_profile(ui, &mut view))
                    .unwrap_or(false);
                if deleted {
                    self.rerender_and_focus(SettingId::GlobalEqualizerPreset);
                }
            }
            SettingsCommand::ChooseCookiesFile => self.choose_cookies_file(),
            SettingsCommand::AudiovaultLogin => {
                if let Err(error) = self.sync_all() {
                    self.show_error(&error);
                    return;
                }
                self.with_shell(|shell, ui| shell.audiovault_settings_login(ui));
            }
            SettingsCommand::AudiovaultLogout => {
                self.with_shell(|shell, ui| shell.audiovault_settings_logout(ui));
            }
            SettingsCommand::AudiovaultRegister => {
                let _ = apricot_platform::shell::open_url(
                    apricot_platform::audiovault::AUDIOVAULT_REGISTER_URL,
                );
            }
            SettingsCommand::OpenYoutubeLoginProfile => {
                if let Err(error) = self.sync_all() {
                    self.show_error(&error);
                    return;
                }
                if let Some(result) =
                    self.with_shell(|shell, _| shell.settings_open_youtube_login())
                {
                    self.show_cookie_result(result);
                }
            }
            SettingsCommand::ExportBrowserCookies => self.export_browser_cookies(),
            SettingsCommand::SetDefaultPlayer => self.set_default_player(),
            // Python checks with the saved settings, without applying the
            // visible controls; the result is spoken while they stay open.
            SettingsCommand::CheckSubscriptions => {
                self.with_shell(|shell, ui| shell.check_subscriptions(ui, true));
            }
            SettingsCommand::ObtainYoutubeApiKey => {
                if let Err(error) = self.sync_all() {
                    self.show_error(&error);
                    return;
                }
                match self.with_shell(|shell, _| shell.settings_open_api_key_page()) {
                    Some(Ok(message)) => self.announce(&message, true),
                    Some(Err(error)) => self.show_error(&error),
                    None => {}
                }
            }
            _ => {
                // The update checks arrive with unit M16.
                let label = self
                    .ivars()
                    .controls
                    .borrow()
                    .iter()
                    .find(|bound| bound.binding == SettingsBinding::Command(command))
                    .and_then(|bound| match &bound.kind {
                        ControlKind::Button(button) => Some(button.title().to_string()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let message = self
                    .with_shell(|shell, _| shell.settings_command_unavailable(&label))
                    .unwrap_or_default();
                self.announce(&message, true);
            }
        }
    }

    /// Windows `set_default_player`.
    fn set_default_player(&self) {
        let Some(catalog) = self.with_shell(|shell, _| shell.catalog()) else {
            return;
        };
        match crate::default_player::set() {
            Ok(()) => self.announce(catalog.text("default_player_settings_opened"), true),
            Err(error) => self.show_error(
                &catalog
                    .text("default_player_settings_failed")
                    .replace("{error}", &error),
            ),
        }
    }

    /// Shows what a cookie command did.
    fn show_cookie_result(&self, result: apricot_shell::CookieCommandResult) {
        use apricot_shell::CookieCommandResult as R;
        match result {
            R::None => {}
            R::Announce(text) => self.announce(&text, true),
            R::Message {
                title,
                text,
                announce,
            } => {
                self.message(title.as_deref(), &text);
                if let Some(announce) = announce {
                    self.announce(&announce, true);
                }
            }
            R::Source {
                path,
                browser_index,
                announce,
                warning,
            } => {
                self.show_cookie_source(&path, browser_index);
                for (index, text) in announce.iter().enumerate() {
                    self.announce(text, index == 0);
                }
                if let Some(warning) = warning {
                    self.message(None, &warning);
                }
            }
        }
    }

    /// Windows `show_cookie_source`: the cookies field and the browser
    /// choice after an import or an export; an import also puts the profile
    /// back on Auto.
    fn show_cookie_source(&self, path: &str, browser_index: usize) {
        for bound in self.ivars().controls.borrow().iter() {
            match (&bound.binding, &bound.kind) {
                (SettingsBinding::Text(SettingId::CookiesFile), ControlKind::Field(field)) => {
                    field.setStringValue(&NSString::from_str(path));
                }
                (
                    SettingsBinding::Choice {
                        setting: SettingId::CookiesFromBrowser,
                        ..
                    },
                    ControlKind::Choice(popup),
                ) => popup.select(browser_index),
                (
                    SettingsBinding::Choice {
                        setting: SettingId::CookiesBrowserProfile,
                        ..
                    },
                    ControlKind::Choice(popup),
                ) if browser_index == 0 => popup.select(0),
                _ => {}
            }
        }
    }

    /// Windows `choose_cookies_file`.
    fn choose_cookies_file(&self) {
        let title = self
            .with_shell(|shell, _| shell.text("choose_cookies_file"))
            .unwrap_or_default();
        let panel = NSOpenPanel::openPanel(self.mtm());
        panel.setTitle(Some(&NSString::from_str(&title)));
        panel.setMessage(Some(&NSString::from_str(&title)));
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        let chosen = self.run_panel(&panel);
        if let Some(path) = chosen
            && let Some(result) =
                self.with_shell(|shell, _| shell.settings_import_cookies_file(&path))
        {
            self.show_cookie_result(result);
        }
        self.focus_command(SettingsCommand::ChooseCookiesFile);
    }

    /// Windows `export_browser_cookies`.
    fn export_browser_cookies(&self) {
        if let Err(error) = self.sync_all() {
            self.show_error(&error);
            return;
        }
        let Some(start) = self.with_shell(|shell, _| shell.settings_cookie_export_browser()) else {
            return;
        };
        let (browser, prompt) = match start {
            Ok(start) => start,
            Err(result) => {
                self.show_cookie_result(result);
                return;
            }
        };
        let mut close = false;
        if let Some((title, message)) = prompt {
            let (yes, no) = self
                .with_shell(|shell, _| (shell.text("yes"), shell.text("no")))
                .unwrap_or_default();
            let window = self.window();
            let previous = window.as_ref().and_then(|window| window.firstResponder());
            let alert = NSAlert::new(self.mtm());
            alert.setMessageText(&NSString::from_str(&message));
            alert.window().setTitle(&NSString::from_str(&title));
            alert.addButtonWithTitle(&NSString::from_str(&yes));
            alert
                .addButtonWithTitle(&NSString::from_str(&no))
                .setKeyEquivalent(&NSString::from_str("\u{1b}"));
            let answer = alert.runModal();
            if let (Some(window), Some(previous)) = (window, previous) {
                window.makeKeyAndOrderFront(None);
                window.makeFirstResponder(Some(&previous));
            }
            if answer != NSAlertFirstButtonReturn {
                return;
            }
            close = true;
        }
        let announcements = self
            .with_shell(|shell, ui| shell.settings_start_cookie_export(ui, browser, close))
            .unwrap_or_default();
        for text in announcements {
            self.announce(&text, true);
        }
    }

    /// Windows `rerender_and_focus`.
    fn rerender_and_focus(&self, setting: SettingId) {
        self.render_controls();
        self.focus_setting(setting);
    }

    /// Windows `reset_equalizer_preset`.
    fn reset_equalizer_preset(&self) {
        let result = self.with_shell(|shell, ui| shell.settings_reset_equalizer_preset(ui));
        let (gains, range, message) = match result {
            Some(Ok(result)) => result,
            Some(Err(error)) => {
                self.show_error(&error);
                return;
            }
            None => return,
        };
        for bound in self.ivars().controls.borrow().iter() {
            if let (
                SettingsBinding::EqualizerBand { band_id, label, .. },
                ControlKind::Slider(slider),
            ) = (&bound.binding, &bound.kind)
            {
                let tenths = apricot_app::equalizer::slider_from_gain(
                    gains.get(*band_id).copied().unwrap_or_default(),
                    range,
                );
                slider.set_value(
                    tenths,
                    label,
                    &apricot_app::equalizer::slider_value_text(tenths),
                    false,
                );
            }
        }
        self.announce(&message, false);
    }

    /// Windows `reset_section`.
    fn reset_section(&self) {
        let section = self.ivars().selected_section.get();
        match self.with_shell(|shell, _| shell.settings_reset_section(section)) {
            Some(Ok(message)) => {
                self.announce(&message, true);
                self.render_controls();
                self.focus_first_control();
            }
            Some(Err(error)) => self.show_error(&error),
            None => {}
        }
    }

    /// Windows `browse_download_folder`: the chosen folder is saved at once.
    fn browse_download_folder(&self) {
        let (title, initial) = self
            .with_shell(|shell, _| {
                (
                    shell.text("choose_download_folder"),
                    shell.application.settings().download_folder.clone(),
                )
            })
            .unwrap_or_default();
        let panel = NSOpenPanel::openPanel(self.mtm());
        panel.setTitle(Some(&NSString::from_str(&title)));
        panel.setMessage(Some(&NSString::from_str(&title)));
        panel.setCanChooseFiles(false);
        panel.setCanChooseDirectories(true);
        panel.setCanCreateDirectories(true);
        if !initial.trim().is_empty() {
            panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(&initial))));
        }
        let chosen = self.run_panel(&panel);
        if let Some(folder) = chosen {
            let folder = folder.to_string_lossy().into_owned();
            if let Some(Err(error)) =
                self.with_shell(|shell, _| shell.settings_set_download_folder(&folder))
            {
                self.show_error(&error);
            }
            for bound in self.ivars().controls.borrow().iter() {
                if let (
                    SettingsBinding::Text(SettingId::DownloadFolder),
                    ControlKind::Field(field),
                ) = (&bound.binding, &bound.kind)
                {
                    field.setStringValue(&NSString::from_str(&folder));
                }
            }
        }
        self.focus_command(SettingsCommand::BrowseDownloadFolder);
    }

    fn run_panel(&self, panel: &NSSavePanel) -> Option<PathBuf> {
        let window = self.window();
        let previous = window.as_ref().and_then(|window| window.firstResponder());
        let response = panel.runModal();
        if let Some(window) = window {
            window.makeKeyAndOrderFront(None);
            if let Some(previous) = previous {
                window.makeFirstResponder(Some(&previous));
            }
        }
        if response != NSModalResponseOK {
            return None;
        }
        panel
            .URL()
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
    }

    /// Windows `save_settings`.
    fn save_settings(&self) {
        if let Err(error) = self.sync_all() {
            self.show_error(&error);
            return;
        }
        let language = self.ivars().displayed_language.borrow().clone();
        match self.with_shell(|shell, _| shell.settings_save(&language)) {
            Some(Ok((message, language_changed))) => {
                self.announce(&message, true);
                if language_changed {
                    self.rebuild_screen();
                }
            }
            Some(Err(error)) => self.show_error(&error),
            None => {}
        }
    }

    /// Windows `restore_defaults`.
    fn restore_all(&self) {
        match self.with_shell(|shell, _| shell.settings_restore_defaults()) {
            Some(Ok(message)) => {
                self.announce(&message, true);
                self.rebuild_screen();
            }
            Some(Err(error)) => self.show_error(&error),
            None => {}
        }
    }

    /// Windows `rebuild_screen`: every text in the new language, focus on
    /// the section list.
    fn rebuild_screen(&self) {
        // SAFETY: `renderSection:` exists on this object.
        unsafe {
            NSObject::cancelPreviousPerformRequestsWithTarget_selector_object(
                self.as_ref(),
                sel!(renderSection:),
                None,
            );
        }
        self.ivars().pending_section.set(None);
        let section = self.ivars().selected_section.get();
        let Some((model, language, back, save, reset)) = self.with_shell(|shell, _| {
            (
                shell.settings_screen(section),
                shell.application.settings().language.clone(),
                shell.text("back"),
                shell.text("save"),
                shell.text("restore_defaults"),
            )
        }) else {
            return;
        };
        if let Some(window) = self.window() {
            window.setTitle(&NSString::from_str(&model.title));
        }
        for (button, label) in [
            (&self.ivars().back, back),
            (&self.ivars().save, save),
            (&self.ivars().reset_all, reset),
        ] {
            if let Some(button) = button.borrow().as_ref() {
                button.setTitle(&NSString::from_str(&label));
            }
        }
        if let Some(list) = self.ivars().sections.borrow().as_ref() {
            list.set_label(&model.section_list_name);
            let selected = SettingsSection::ALL
                .iter()
                .position(|candidate| *candidate == section)
                .unwrap_or_default();
            list.set_rows(
                model.sections.into_iter().map(|item| item.label).collect(),
                Some(selected),
            );
        }
        *self.ivars().displayed_language.borrow_mut() = language;
        self.render_controls();
        let sections = self.ivars().sections.borrow().clone();
        if let (Some(sections), Some(window)) = (sections, self.window()) {
            window.makeFirstResponder(Some(&sections));
        }
    }

    /// Windows `handle_shortcut_message`: a global shortcut closes the
    /// window and runs in the main window; the Action Finder opens over it.
    fn handle_shortcut(&self, event: &NSEvent) -> bool {
        self.ivars().user_moved.set(true);
        let Some(window) = self.window() else {
            return false;
        };
        let capture_focused = window.firstResponder().is_some_and(|responder| {
            self.ivars().controls.borrow().iter().any(|bound| {
                matches!(bound.kind, ControlKind::Capture(_))
                    && std::ptr::eq(
                        Retained::as_ptr(&responder).cast::<u8>(),
                        Retained::as_ptr(&bound.view).cast::<u8>(),
                    )
            })
        });
        if capture_focused {
            return false;
        }
        let Some(characters) = event.charactersIgnoringModifiers() else {
            return false;
        };
        let Some(chord) = keys::chord(
            event.keyCode(),
            &characters.to_string(),
            controls::modifiers(event),
        ) else {
            return false;
        };
        let action = self.with_shell(|shell, _| {
            action_for_shortcut(
                &shell.application.settings().keyboard_shortcuts,
                chord,
                ShortcutContext::new(ActionScope::Dialog, true),
            )
            .filter(|action| action.scopes.contains(&ActionScope::Global))
            .map(|action| (action.id.as_str(), action.repeat))
        });
        let Some(Some((action_id, repeat))) = action else {
            return false;
        };
        if event.isARepeat() && repeat == RepeatPolicy::None {
            return true;
        }
        match action_id {
            "open_action_finder" => self.show_action_finder(),
            "open_settings" => {
                let sections = self.ivars().sections.borrow().clone();
                if let Some(sections) = sections {
                    window.makeFirstResponder(Some(&sections));
                }
            }
            action_id => self.defer_global_action(action_id),
        }
        true
    }

    /// Windows `show_action_finder` of the Settings window.
    fn show_action_finder(&self) {
        let Some(model) = self.with_shell(|shell, _| shell.application.action_finder_model())
        else {
            return;
        };
        match crate::action_finder::show(self.mtm(), model) {
            Some("open_settings" | "open_action_finder") | None => {
                let sections = self.ivars().sections.borrow().clone();
                if let (Some(sections), Some(window)) = (sections, self.window()) {
                    window.makeFirstResponder(Some(&sections));
                }
            }
            Some(action_id) => self.defer_global_action(action_id),
        }
    }

    fn defer_global_action(&self, action_id: &'static str) {
        self.ivars().deferred_action.set(Some(action_id));
        NSApplication::sharedApplication(self.mtm()).stopModal();
    }
}

/// The settings side of the equalizer profile prompts (Windows
/// `SettingsEqualizerHost`); only the prompts are used.
struct PromptView<'a> {
    dialog: &'a SettingsWindow,
}

impl EqualizerView for PromptView<'_> {
    fn set_presets(&mut self, _labels: &[String], _selected: usize) {}
    fn selected_preset(&self) -> Option<usize> {
        None
    }
    fn set_range(&mut self, _index: usize) {}
    fn selected_range(&self) -> Option<usize> {
        None
    }
    fn set_name(&mut self, _text: &str) {}
    fn name(&self) -> String {
        String::new()
    }
    fn set_name_visible(&mut self, _visible: bool) {}
    fn set_slider(
        &mut self,
        _index: usize,
        _limit: i32,
        _tenths: i32,
        _label: &str,
        _value: &str,
        _notify: bool,
    ) {
    }
    fn schedule_apply(&mut self, _delay_ms: u32) {}
    fn cancel_apply(&mut self) {}
    fn close(&mut self) {}

    fn message(&mut self, title: &str, message: &str, buttons: &[&str]) -> usize {
        let window = self.dialog.window();
        let previous = window.as_ref().and_then(|window| window.firstResponder());
        let alert = NSAlert::new(self.dialog.mtm());
        alert.setMessageText(&NSString::from_str(message));
        alert.window().setTitle(&NSString::from_str(title));
        for (index, label) in buttons.iter().enumerate() {
            let button = alert.addButtonWithTitle(&NSString::from_str(label));
            if index == 1 {
                button.setKeyEquivalent(&NSString::from_str("\u{1b}"));
            }
        }
        let response = alert.runModal();
        if let (Some(window), Some(previous)) = (window, previous) {
            window.makeKeyAndOrderFront(None);
            window.makeFirstResponder(Some(&previous));
        }
        usize::try_from(response - NSAlertFirstButtonReturn).unwrap_or(usize::MAX)
    }

    fn prompt_name(&mut self, title: &str, prompt: &str, ok: &str, cancel: &str) -> Option<String> {
        crate::language::prompt_text(self.dialog.mtm(), title, prompt, "", ok, cancel)
    }

    fn choose(&mut self, dialog: &ChoiceDialog) -> Option<usize> {
        crate::language::choose_from_list(
            self.dialog.mtm(),
            &crate::language::ChoiceRequest {
                title: &dialog.title,
                prompt: &dialog.prompt,
                list_name: &dialog.list_name,
                choices: &dialog.choices,
                initial: dialog.initial,
                ok: &dialog.ok,
                cancel: &dialog.cancel,
                width: 540.0,
                height: 470.0,
            },
        )
    }

    fn choose_json_file(&mut self, title: &str, _type_label: &str) -> Option<PathBuf> {
        let panel = NSOpenPanel::openPanel(self.dialog.mtm());
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        allow_json(&panel);
        self.dialog.run_panel(&panel)
    }

    fn save_json_file(
        &mut self,
        title: &str,
        file_name: &str,
        _type_label: &str,
    ) -> Option<PathBuf> {
        let panel = NSSavePanel::savePanel(self.dialog.mtm());
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setNameFieldStringValue(&NSString::from_str(file_name));
        allow_json(&panel);
        self.dialog.run_panel(&panel)
    }
}

fn allow_json(panel: &NSSavePanel) {
    let types = NSArray::from_retained_slice(&[NSString::from_str("json")]);
    // SAFETY: `setAllowedFileTypes:` takes an array of extensions.
    unsafe {
        let _: () = msg_send![panel, setAllowedFileTypes: &*types];
    }
}

define_class!(
    /// The settings window: key presses reach Windows
    /// `handle_shortcut_message` before the focused control.
    // SAFETY: NSWindow may be subclassed; no Drop impl.
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSettingsNSWindow"]
    #[ivars = Cell<*const SettingsWindow>]
    struct SettingsNSWindow;

    impl SettingsNSWindow {
        #[unsafe(method(sendEvent:))]
        fn send_event(&self, event: &NSEvent) {
            if event.r#type() == NSEventType::KeyDown {
                let dialog = self.ivars().get();
                // SAFETY: the dialog outlives the modal loop of this window
                // and clears the pointer before it is released.
                if let Some(dialog) = unsafe { dialog.as_ref() }
                    && dialog.handle_shortcut(event)
                {
                    return;
                }
            }
            // SAFETY: forwarding to NSWindow.
            unsafe {
                let _: () = msg_send![super(self), sendEvent: event];
            }
        }
    }
);

/// Windows `open_settings`: the modal window; then
/// `Shell::settings_closed` with a deferred global action.
#[allow(clippy::too_many_lines)] // The window's fixed controls in their Tab order.
pub fn run(controller: &MainWindowController) {
    let mtm = MainThreadMarker::from(controller);
    let app = NSApplication::sharedApplication(mtm);
    let Some((model, language, back_label, save_label, reset_label)) =
        controller.with_shell(|shell, _| {
            (
                shell.settings_screen(SettingsSection::General),
                shell.application.settings().language.clone(),
                shell.text("back"),
                shell.text("save"),
                shell.text("restore_defaults"),
            )
        })
    else {
        return;
    };
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let this: Retained<SettingsWindow> = {
        let this = SettingsWindow::alloc(mtm).set_ivars(SettingsIvars {
            controller: Weak::from(controller),
            window: RefCell::new(None),
            back: RefCell::new(None),
            save: RefCell::new(None),
            reset_all: RefCell::new(None),
            sections: RefCell::new(None),
            scroll: RefCell::new(None),
            document: RefCell::new(None),
            controls: RefCell::new(Vec::new()),
            selected_section: Cell::new(SettingsSection::General),
            pending_section: Cell::new(None),
            displayed_language: RefCell::new(language),
            deferred_action: Cell::new(None),
            audio_devices: RefCell::new(None),
            timer: RefCell::new(None),
            user_moved: Cell::new(false),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let target: &AnyObject = this.as_ref();
    // SAFETY: a titled, resizable window with a buffered backing.
    let window: Retained<SettingsNSWindow> = unsafe {
        let window = SettingsNSWindow::alloc(mtm).set_ivars(Cell::new(Retained::as_ptr(&this)));
        msg_send![
            super(window),
            initWithContentRect: NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(WIDTH, HEIGHT)),
            styleMask: NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Resizable,
            backing: NSBackingStoreType::Buffered,
            defer: false
        ]
    };
    let window: Retained<NSWindow> = Retained::into_super(window);
    // SAFETY: the window is released by its last owner, this function.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(&model.title));
    window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
    let content = window.contentView().expect("a window has a content view");
    let top = HEIGHT - MARGIN - BUTTON_HEIGHT;
    // Python adds the button row before the section list, so Back, Save and
    // Restore to defaults come first in the Tab order.
    let back = Button::new(mtm, &back_label, target, sel!(back:));
    back.setFrame(NSRect::new(
        NSPoint::new(MARGIN, top),
        NSSize::new(150.0, BUTTON_HEIGHT),
    ));
    // Windows `IDCANCEL` is Back.
    back.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    let save = Button::new(mtm, &save_label, target, sel!(save:));
    save.setFrame(NSRect::new(
        NSPoint::new(172.0, top),
        NSSize::new(120.0, BUTTON_HEIGHT),
    ));
    // Windows `BS_DEFPUSHBUTTON`: Return saves.
    save.setKeyEquivalent(&NSString::from_str("\r"));
    let reset_all = Button::new(mtm, &reset_label, target, sel!(restoreDefaults:));
    reset_all.setFrame(NSRect::new(
        NSPoint::new(302.0, top),
        NSSize::new(190.0, BUTTON_HEIGHT),
    ));
    for button in [&back, &save, &reset_all] {
        button.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewMinYMargin);
        content.addSubview(button);
    }
    let body_height = top - 2.0 * MARGIN;
    let sections = List::new(mtm, &model.section_list_name);
    // SAFETY: the dialog outlives the list.
    unsafe { sections.setTarget(Some(target)) };
    sections.set_selection_action(sel!(sectionChanged:));
    sections.set_activate_action(target, sel!(sectionCommit:));
    sections.set_tab_action(sel!(sectionCommit:));
    sections.set_rows(
        model
            .sections
            .iter()
            .map(|item| item.label.clone())
            .collect(),
        Some(0),
    );
    let section_scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, MARGIN),
            NSSize::new(SECTION_WIDTH, body_height),
        ),
    );
    section_scroll.setDocumentView(Some(&sections));
    section_scroll.setHasVerticalScroller(true);
    section_scroll.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable);
    content.addSubview(&section_scroll);
    let control_x = MARGIN * 2.0 + SECTION_WIDTH;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(control_x, MARGIN),
            NSSize::new(WIDTH - control_x - MARGIN, body_height),
        ),
    );
    scroll.setHasVerticalScroller(true);
    scroll.setAutoresizingMask(
        objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
            | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    // SAFETY: `initWithFrame:` of NSView.
    let document: Retained<FlippedView> =
        unsafe { msg_send![FlippedView::alloc(mtm), initWithFrame: NSRect::ZERO] };
    scroll.setDocumentView(Some(&document));
    content.addSubview(&scroll);
    *this.ivars().window.borrow_mut() = Some(window.clone());
    *this.ivars().back.borrow_mut() = Some(back);
    *this.ivars().save.borrow_mut() = Some(save);
    *this.ivars().reset_all.borrow_mut() = Some(reset_all);
    *this.ivars().sections.borrow_mut() = Some(sections.clone());
    *this.ivars().scroll.borrow_mut() = Some(scroll);
    *this.ivars().document.borrow_mut() = Some(document);
    this.render_controls();
    OPEN.with(|open| *open.borrow_mut() = Some(this.clone()));
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&sections));
    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&sections));
    // The modal session makes the window key again, which would put focus
    // on Back; the section list gets it once the session runs.
    // SAFETY: `focusSections:` exists on the dialog.
    unsafe {
        let run_modes = NSArray::from_slice(&[NSRunLoopCommonModes]);
        let _: () = msg_send![
            &*this,
            performSelector: sel!(focusSections:),
            withObject: Option::<&AnyObject>::None,
            afterDelay: 0.0_f64,
            inModes: &*run_modes
        ];
        let _: () = msg_send![
            &*this,
            performSelector: sel!(focusSectionsIfUnmoved:),
            withObject: Option::<&AnyObject>::None,
            afterDelay: 0.3_f64,
            inModes: &*run_modes
        ];
    }
    app.runModalForWindow(&window);
    // SAFETY: `renderSection:` and `applyEqualizer:` exist on this object.
    unsafe {
        NSObject::cancelPreviousPerformRequestsWithTarget(this.as_ref());
    }
    if let Some(timer) = this.ivars().timer.borrow_mut().take() {
        timer.invalidate();
    }
    if let Some(settings_window) = window.downcast_ref::<SettingsNSWindow>() {
        settings_window.ivars().set(std::ptr::null());
    }
    window.setDelegate(None);
    window.orderOut(None);
    OPEN.with(|open| *open.borrow_mut() = None);
    let deferred = this.ivars().deferred_action.get();
    this.ivars().controls.borrow_mut().clear();
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    controller.with_shell(|shell, ui| shell.settings_closed(ui, deferred));
}

/// Windows `browser_cookies_exported` for an open Settings window.
pub fn cookie_source_exported(path: &str, browser_index: usize) {
    let open = OPEN.with(|open| open.borrow().clone());
    if let Some(dialog) = open {
        dialog.show_cookie_source(path, browser_index);
    }
}
