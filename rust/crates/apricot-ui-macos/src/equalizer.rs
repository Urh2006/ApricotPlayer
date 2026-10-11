//! The player equalizer window (F4), the macOS counterpart of
//! `equalizer_win32.rs`: the preset and dB range choices, the custom preset
//! name, a labelled slider per band and the eleven buttons, in that Tab
//! order, modal over the main window. The decisions are made by
//! `apricot_shell::equalizer_dialog`; this file only shows them.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::path::PathBuf;

use apricot_shell::{
    ChoiceDialog, EQUALIZER_BUTTONS, EqualizerButton, EqualizerLabels, EqualizerView, Shell,
    ShellUi,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAlert, NSAlertFirstButtonReturn, NSApplication, NSBackingStoreType,
    NSModalResponseOK, NSOpenPanel, NSResponder, NSSavePanel, NSScreen, NSScrollView, NSTextField,
    NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSRunLoopCommonModes, NSSize, NSString,
};

use crate::controls::{self, Button};
use crate::form_controls::{PopUp, Slider};
use crate::window::MainWindowController;

/// Windows `CreateWindowExW(..., 760, height, ...)`.
const WINDOW_WIDTH: f64 = 760.0;
const MARGIN: f64 = 10.0;
const LABEL_HEIGHT: f64 = 18.0;
const BUTTON_WIDTH: f64 = 230.0;
const BUTTON_HEIGHT: f64 = 28.0;

define_class!(
    /// The scrolling content of the dialog, laid out from the top as on
    /// Windows.
    // SAFETY: NSView may be subclassed; no Drop impl.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotFlippedView"]
    struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

struct Controls {
    document: Retained<FlippedView>,
    preset_label: Retained<NSTextField>,
    preset: Retained<PopUp>,
    range_label: Retained<NSTextField>,
    range: Retained<PopUp>,
    name_label: Retained<NSTextField>,
    name: Retained<NSTextField>,
    band_labels: Vec<Retained<NSTextField>>,
    sliders: Vec<Retained<Slider>>,
    buttons: Vec<(EqualizerButton, Retained<Button>)>,
}

pub struct DialogIvars {
    controller: Retained<MainWindowController>,
    /// The next step of the development self-test.
    self_test: std::cell::Cell<usize>,
    window: RefCell<Option<Retained<NSWindow>>>,
    controls: RefCell<Option<Controls>>,
}

define_class!(
    /// Owns the equalizer window and forwards its controls to the shell.
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotEqualizerDialog"]
    #[ivars = DialogIvars]
    struct EqualizerDialog;

    unsafe impl NSObjectProtocol for EqualizerDialog {}

    /// Closing the window cancels (Windows `WM_CLOSE`).
    unsafe impl NSWindowDelegate for EqualizerDialog {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            stop_modal(self.mtm());
            false
        }
    }

    impl EqualizerDialog {
        /// Windows `CBN_SELCHANGE` of the preset combo box.
        #[unsafe(method(presetChanged:))]
        fn preset_changed(&self, _sender: Option<&AnyObject>) {
            self.with_view(|shell, _, view| shell.equalizer_preset_changed(view));
        }

        /// Windows `CBN_SELCHANGE` of the dB range combo box.
        #[unsafe(method(rangeChanged:))]
        fn range_changed(&self, _sender: Option<&AnyObject>) {
            self.with_view(|shell, _, view| shell.equalizer_range_changed(view));
        }

        /// Windows `WM_HSCROLL`.
        #[unsafe(method(sliderChanged:))]
        fn slider_changed(&self, sender: Option<&AnyObject>) {
            let Some(slider) = sender.and_then(|sender| sender.downcast_ref::<Slider>()) else {
                return;
            };
            let (Ok(index), tenths) = (usize::try_from(slider.tag()), slider.position()) else {
                return;
            };
            self.with_view(|shell, _, view| shell.equalizer_slider_changed(view, index, tenths));
        }

        /// Windows `BN_CLICKED`.
        #[unsafe(method(dialogButton:))]
        fn dialog_button(&self, sender: Option<&AnyObject>) {
            let button = sender
                .and_then(|sender| sender.downcast_ref::<objc2_app_kit::NSControl>())
                .and_then(|control| usize::try_from(control.tag()).ok())
                .and_then(|index| EQUALIZER_BUTTONS.get(index))
                .map(|(button, _)| *button);
            if let Some(button) = button {
                self.with_view(|shell, ui, view| shell.equalizer_button(ui, view, button));
            }
        }

        /// Development builds: one step of the scripted equalizer check.
        #[unsafe(method(selfTestStep:))]
        fn self_test_step_selector(&self, _sender: Option<&AnyObject>) {
            #[cfg(debug_assertions)]
            self.self_test_step();
        }

        /// Windows `APPLY_TIMER_ID`.
        #[unsafe(method(applyNow:))]
        fn apply_now(&self, _sender: Option<&AnyObject>) {
            let applied = self
                .ivars()
                .controller
                .with_shell(|shell, ui| shell.equalizer_apply_now(ui));
            if applied.is_none() {
                // A prompt of the dialog holds the shell; try again after it.
                self.schedule(apricot_app::equalizer::APPLY_DELAY_MS);
            }
        }
    }
);

fn stop_modal(mtm: MainThreadMarker) {
    NSApplication::sharedApplication(mtm).stopModal();
}

impl EqualizerDialog {
    fn with_view(&self, action: impl FnOnce(&mut Shell, &mut dyn ShellUi, &mut dyn EqualizerView)) {
        self.ivars().controller.with_shell(|shell, ui| {
            let mut view = MacEqualizerView { dialog: self };
            action(shell, ui, &mut view);
        });
    }

    fn schedule(&self, delay_ms: u32) {
        self.cancel();
        self.perform_in_modal_loop(sel!(applyNow:), delay_ms);
    }

    /// `selector` after `delay_ms`, also while the dialog runs modally.
    fn perform_in_modal_loop(&self, selector: objc2::runtime::Sel, delay_ms: u32) {
        let modes = NSArray::from_slice(&[
            // SAFETY: AppKit's static run loop mode; common modes include the
            // modal panel mode, so the timer runs while the dialog is open.
            unsafe { NSRunLoopCommonModes },
        ]);
        // SAFETY: the selector exists on the dialog, which outlives the
        // request (pending ones are cancelled when the dialog ends).
        unsafe {
            let _: () = msg_send![
                self,
                performSelector: selector,
                withObject: Option::<&AnyObject>::None,
                afterDelay: f64::from(delay_ms) / 1000.0,
                inModes: &*modes
            ];
        }
    }

    fn cancel(&self) {
        // SAFETY: cancels the pending `applyNow:` of this dialog.
        unsafe {
            let _: () = msg_send![
                objc2::class!(NSObject),
                cancelPreviousPerformRequestsWithTarget: self,
                selector: sel!(applyNow:),
                object: Option::<&AnyObject>::None
            ];
        }
    }

    /// Development builds with `APRICOT_MACOS_SELF_TEST=player`: real key
    /// presses in the dialog, printing the focused control after each.
    #[cfg(debug_assertions)]
    fn self_test_step(&self) {
        use crate::keys;
        use crate::self_test::{Key, focus, key, send};
        let Some(window) = self.ivars().window.borrow().clone() else {
            return;
        };
        let shift_tab = Key {
            code: keys::KEY_TAB,
            text: "\t",
            flags: objc2_app_kit::NSEventModifierFlags::Shift,
        };
        let steps: [(&str, Option<Key>); 10] = [
            ("opened", None),
            ("Down (next preset)", Some(key(keys::KEY_DOWN, "\u{f701}"))),
            ("Up (previous preset)", Some(key(keys::KEY_UP, "\u{f700}"))),
            ("Tab (range)", Some(key(keys::KEY_TAB, "\t"))),
            ("Tab (first band)", Some(key(keys::KEY_TAB, "\t"))),
            ("Up (band up)", Some(key(keys::KEY_UP, "\u{f700}"))),
            (
                "Page Up (band up more)",
                Some(key(keys::KEY_PAGE_UP, "\u{f72c}")),
            ),
            ("Tab (second band)", Some(key(keys::KEY_TAB, "\t"))),
            ("Shift+Tab (first band)", Some(shift_tab)),
            ("Escape (cancel)", Some(key(keys::KEY_ESCAPE, "\u{1b}"))),
        ];
        let index = self.ivars().self_test.get();
        let Some((name, key)) = steps.get(index) else {
            return;
        };
        if let Some(key) = key {
            send(&window, key);
        }
        println!(
            "equalizer-test {name}: focus={} value={:?} open={}",
            focus(&window),
            crate::self_test::focused_value(&window),
            window.isVisible()
        );
        self.ivars().self_test.set(index + 1);
        if index + 1 < steps.len() {
            self.perform_in_modal_loop(sel!(selfTestStep:), 500);
        }
    }

    /// Windows `layout`: label above control, the name only for a custom
    /// preset, then the buttons in rows.
    fn layout(&self) {
        let controls = self.ivars().controls.borrow();
        let Some(controls) = controls.as_ref() else {
            return;
        };
        let inner = WINDOW_WIDTH - MARGIN * 2.0;
        let mut top = MARGIN;
        let mut place = |label: &NSView, control: &NSView, height: f64| {
            label.setFrame(NSRect::new(
                NSPoint::new(MARGIN, top),
                NSSize::new(inner, LABEL_HEIGHT),
            ));
            control.setFrame(NSRect::new(
                NSPoint::new(MARGIN, top + LABEL_HEIGHT),
                NSSize::new(inner, height),
            ));
            top += LABEL_HEIGHT + height + 6.0;
        };
        place(&controls.preset_label, &controls.preset, 26.0);
        place(&controls.range_label, &controls.range, 26.0);
        if !controls.name.isHidden() {
            place(&controls.name_label, &controls.name, 24.0);
        }
        for (label, slider) in controls.band_labels.iter().zip(&controls.sliders) {
            place(label, slider, 28.0);
        }
        // Whole columns of a positive width, so the conversion is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let columns = ((inner / (BUTTON_WIDTH + 8.0)).floor() as usize).max(1);
        for (index, (_, button)) in controls.buttons.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let (column, row) = ((index % columns) as f64, (index / columns) as f64);
            button.setFrame(NSRect::new(
                NSPoint::new(
                    MARGIN + column * (BUTTON_WIDTH + 8.0),
                    top + row * (BUTTON_HEIGHT + 6.0),
                ),
                NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
            ));
        }
        #[allow(clippy::cast_precision_loss)]
        let rows = controls.buttons.len().div_ceil(columns) as f64;
        let height = top + rows * (BUTTON_HEIGHT + 6.0) + MARGIN;
        controls
            .document
            .setFrameSize(NSSize::new(WINDOW_WIDTH, height));
    }
}

/// The shell's view of the equalizer window.
struct MacEqualizerView<'a> {
    dialog: &'a EqualizerDialog,
}

impl MacEqualizerView<'_> {
    fn controls<R>(&self, action: impl FnOnce(&Controls) -> R) -> Option<R> {
        self.dialog.ivars().controls.borrow().as_ref().map(action)
    }

    fn mtm(&self) -> MainThreadMarker {
        self.dialog.mtm()
    }
}

impl EqualizerView for MacEqualizerView<'_> {
    fn set_presets(&mut self, labels: &[String], selected: usize) {
        self.controls(|controls| controls.preset.set_items(labels, selected));
    }

    fn selected_preset(&self) -> Option<usize> {
        self.controls(|controls| controls.preset.selected())
            .flatten()
    }

    fn set_range(&mut self, index: usize) {
        self.controls(|controls| controls.range.select(index));
    }

    fn selected_range(&self) -> Option<usize> {
        self.controls(|controls| controls.range.selected())
            .flatten()
    }

    fn set_name(&mut self, text: &str) {
        self.controls(|controls| controls.name.setStringValue(&NSString::from_str(text)));
    }

    fn name(&self) -> String {
        self.controls(|controls| controls.name.stringValue().to_string())
            .unwrap_or_default()
    }

    fn set_name_visible(&mut self, visible: bool) {
        let changed = self
            .controls(|controls| {
                let changed = controls.name.isHidden() == visible;
                controls.name_label.setHidden(!visible);
                controls.name.setHidden(!visible);
                if let Some((_, delete)) = controls
                    .buttons
                    .iter()
                    .find(|(button, _)| *button == EqualizerButton::DeleteProfile)
                {
                    delete.setEnabled(visible);
                }
                changed
            })
            .unwrap_or_default();
        if changed {
            self.dialog.layout();
        }
    }

    fn set_slider(
        &mut self,
        index: usize,
        limit: i32,
        tenths: i32,
        label: &str,
        value: &str,
        notify: bool,
    ) {
        self.controls(|controls| {
            if let Some(slider) = controls.sliders.get(index) {
                slider.set_range(limit);
                slider.set_value(tenths, label, value, notify);
            }
        });
    }

    fn schedule_apply(&mut self, delay_ms: u32) {
        self.dialog.schedule(delay_ms);
    }

    fn cancel_apply(&mut self) {
        self.dialog.cancel();
    }

    fn close(&mut self) {
        stop_modal(self.mtm());
    }

    fn message(&mut self, title: &str, message: &str, buttons: &[&str]) -> usize {
        let window = self.dialog.ivars().window.borrow().clone();
        let previous = window.as_ref().and_then(|window| window.firstResponder());
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(message));
        alert.window().setTitle(&NSString::from_str(title));
        for (index, label) in buttons.iter().enumerate() {
            let button = alert.addButtonWithTitle(&NSString::from_str(label));
            if index == 1 {
                // Escape answers No.
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
        crate::language::prompt_text(self.mtm(), title, prompt, "", ok, cancel)
    }

    fn choose(&mut self, dialog: &ChoiceDialog) -> Option<usize> {
        crate::language::choose_from_list(
            self.mtm(),
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
        let panel = NSOpenPanel::openPanel(self.mtm());
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);
        allow_json(&panel);
        run_panel(&panel, self.dialog)
    }

    fn save_json_file(
        &mut self,
        title: &str,
        file_name: &str,
        _type_label: &str,
    ) -> Option<PathBuf> {
        let panel = NSSavePanel::savePanel(self.mtm());
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setNameFieldStringValue(&NSString::from_str(file_name));
        allow_json(&panel);
        run_panel(&panel, self.dialog)
    }
}

fn allow_json(panel: &NSSavePanel) {
    let types = NSArray::from_retained_slice(&[NSString::from_str("json")]);
    // SAFETY: `setAllowedFileTypes:` takes an array of extensions (Windows
    // `*.json` filter); it is deprecated in favor of content types.
    unsafe {
        let _: () = msg_send![panel, setAllowedFileTypes: &*types];
    }
}

fn run_panel(panel: &NSSavePanel, dialog: &EqualizerDialog) -> Option<PathBuf> {
    let window = dialog.ivars().window.borrow().clone();
    let previous = window.as_ref().and_then(|window| window.firstResponder());
    let response = panel.runModal();
    if let (Some(window), Some(previous)) = (window, previous) {
        window.makeKeyAndOrderFront(None);
        window.makeFirstResponder(Some(&previous));
    }
    (response == NSModalResponseOK)
        .then(|| panel.URL())
        .flatten()
        .and_then(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
}

fn label(mtm: MainThreadMarker, document: &NSView, text: &str) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    document.addSubview(&label);
    label
}

/// Windows `create_controls`: the controls in Tab order inside `document`.
fn create_controls(
    mtm: MainThreadMarker,
    target: &AnyObject,
    document: Retained<FlippedView>,
    labels: &EqualizerLabels,
) -> Controls {
    let preset_label = label(mtm, &document, &labels.preset);
    let preset = PopUp::new(mtm, &labels.preset, target, sel!(presetChanged:));
    document.addSubview(&preset);
    let range_label = label(mtm, &document, &labels.range);
    let range = PopUp::new(mtm, &labels.range, target, sel!(rangeChanged:));
    let range_options: Vec<String> = apricot_app::equalizer::RANGE_OPTIONS
        .iter()
        .map(|option| (*option).to_owned())
        .collect();
    range.set_items(&range_options, 1);
    document.addSubview(&range);
    let name_label = label(mtm, &document, &labels.name);
    let name = NSTextField::initWithFrame(NSTextField::alloc(mtm), NSRect::ZERO);
    name.setAccessibilityLabel(Some(&NSString::from_str(&labels.name)));
    // Return in the name presses OK (`IDOK`): tag 0 is OK in
    // `EQUALIZER_BUTTONS`.
    // SAFETY: the dialog outlives the field.
    unsafe {
        name.setTarget(Some(target));
        name.setAction(Some(sel!(dialogButton:)));
    }
    name.setTag(0);
    document.addSubview(&name);
    let mut band_labels = Vec::with_capacity(labels.bands.len());
    let mut sliders = Vec::with_capacity(labels.bands.len());
    for (index, text) in labels.bands.iter().enumerate() {
        band_labels.push(label(mtm, &document, text));
        let slider = Slider::new(
            mtm,
            target,
            sel!(sliderChanged:),
            apricot_app::equalizer::SLIDER_LINE_STEP,
            apricot_app::equalizer::SLIDER_PAGE_STEP,
        );
        slider.setTag(isize::try_from(index).unwrap_or_default());
        slider.setAccessibilityLabel(Some(&NSString::from_str(text)));
        document.addSubview(&slider);
        sliders.push(slider);
    }
    let mut buttons = Vec::with_capacity(EQUALIZER_BUTTONS.len());
    for (index, (&(button, _), text)) in EQUALIZER_BUTTONS.iter().zip(&labels.buttons).enumerate() {
        let control = Button::new(mtm, text, target, sel!(dialogButton:));
        control.setTag(isize::try_from(index).unwrap_or_default());
        match button {
            // `BS_DEFPUSHBUTTON` and `IDOK`: Return presses OK.
            EqualizerButton::Ok => control.setKeyEquivalent(&NSString::from_str("\r")),
            // `IDCANCEL`: Escape presses Cancel.
            EqualizerButton::Cancel => control.setKeyEquivalent(&NSString::from_str("\u{1b}")),
            _ => {}
        }
        document.addSubview(&control);
        buttons.push((button, control));
    }
    let mut order: Vec<&NSView> = vec![&preset, &range, &name];
    order.extend(sliders.iter().map(|slider| -> &NSView { slider }));
    order.extend(buttons.iter().map(|(_, button)| -> &NSView { button }));
    controls::set_tab_order(&order);
    Controls {
        document,
        preset_label,
        preset,
        range_label,
        range,
        name_label,
        name,
        band_labels,
        sliders,
        buttons,
    }
}

/// The dialog window with a scrolling, top-down document view.
fn create_window(
    mtm: MainThreadMarker,
    dialog: &EqualizerDialog,
    title: &str,
) -> (Retained<NSWindow>, Retained<FlippedView>) {
    let screen_height =
        NSScreen::mainScreen(mtm).map_or(800.0, |screen| screen.visibleFrame().size.height);
    let height = (screen_height - 20.0).clamp(520.0, 1020.0);
    // SAFETY: a standard titled window with a buffered backing.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(
                NSPoint::new(200.0, 100.0),
                NSSize::new(WINDOW_WIDTH, height),
            ),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is released by its last owner, `run`.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(title));
    window.setDelegate(Some(ProtocolObject::from_ref(dialog)));
    let content = window.contentView().expect("a window has a content view");
    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), content.bounds());
    scroll.setHasVerticalScroller(true);
    scroll.setAutoresizingMask(
        objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
            | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    let document: Retained<FlippedView> = {
        let this = FlippedView::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` of NSView.
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    };
    scroll.setDocumentView(Some(&document));
    content.addSubview(&scroll);
    (window, document)
}

/// Windows `show_player_equalizer` and `equalizer_win32::show`: builds the
/// window, runs it modally and calls the shell's `finish` after it closed,
/// with focus back on the control that had it.
pub fn run(controller: &MainWindowController) {
    let mtm = MainThreadMarker::from(controller);
    let Some(labels) = controller.with_shell(|shell, _| shell.equalizer_labels()) else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());

    let dialog: Retained<EqualizerDialog> = {
        let this = EqualizerDialog::alloc(mtm).set_ivars(DialogIvars {
            controller: controller.retain(),
            self_test: std::cell::Cell::new(0),
            window: RefCell::new(None),
            controls: RefCell::new(None),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let (window, document) = create_window(mtm, &dialog, &labels.title);
    let controls = create_controls(mtm, dialog.as_ref(), document, &labels);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&controls.preset));
    let initial_focus = controls.preset.clone();
    *dialog.ivars().controls.borrow_mut() = Some(controls);
    *dialog.ivars().window.borrow_mut() = Some(window.clone());

    dialog.with_view(|shell, _, view| shell.equalizer_begin(view));
    dialog.layout();
    if let Some(controls) = dialog.ivars().controls.borrow().as_ref() {
        controls.document.scrollPoint(NSPoint::new(0.0, 0.0));
    }

    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&initial_focus));
    if cfg!(debug_assertions) && std::env::var("APRICOT_MACOS_SELF_TEST").as_deref() == Ok("player")
    {
        dialog.perform_in_modal_loop(sel!(selfTestStep:), 500);
    }
    app.runModalForWindow(&window);
    dialog.cancel();
    // SAFETY: cancels every pending request of the dialog.
    unsafe {
        let _: () = msg_send![
            objc2::class!(NSObject),
            cancelPreviousPerformRequestsWithTarget: &*dialog
        ];
    }
    window.setDelegate(None);
    window.orderOut(None);
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    controller.with_shell(|shell, ui| shell.equalizer_finish(ui));
    dialog.ivars().controls.borrow_mut().take();
    dialog.ivars().window.borrow_mut().take();
}
