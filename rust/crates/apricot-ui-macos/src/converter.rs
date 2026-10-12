//! The file and folder converter, the macOS counterpart of
//! `converter_win32.rs`: the path field with Browse, the format choice, the
//! audio-to-video options (Add image, Dark background, the image path with
//! Choose image), Create new and Replace, Convert and Back, in that Tab
//! order, modal over the main window. Hidden rows are removed from the
//! layout, as `dialog.Fit()` does. The decisions are made by
//! `apricot_shell::ConverterDialog`. Also the folder conversion progress
//! window, which has no buttons and cannot be closed.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::Instant;

use apricot_shell::{ConverterDialog, ConverterStart, ConverterView, Shell};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAlert, NSApplication, NSBackingStoreType, NSControlTextEditingDelegate,
    NSModalResponseOK, NSOpenPanel, NSProgressIndicator, NSProgressIndicatorStyle, NSResponder,
    NSSavePanel, NSTextField, NSTextFieldDelegate, NSView, NSWindow, NSWindowDelegate,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSRunLoopCommonModes,
    NSSize, NSString, NSTimer, NSURL,
};

use crate::controls::{self, Button};
use crate::form_controls::{CheckBox, PopUp};
use crate::window::MainWindowController;

/// Windows `layout`: a 640 pixel two-column form.
const WIDTH: f64 = 640.0;
const MARGIN: f64 = 12.0;
const GAP: f64 = 6.0;
const ROW: f64 = 26.0;
const LABEL_WIDTH: f64 = 200.0;
const BUTTON_WIDTH: f64 = 130.0;

define_class!(
    // SAFETY: NSView may be subclassed; no Drop impl.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotConverterContent"]
    struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

struct Controls {
    path_label: Retained<NSTextField>,
    path: Retained<NSTextField>,
    browse: Retained<Button>,
    target_label: Retained<NSTextField>,
    target: Retained<PopUp>,
    options_label: Retained<NSTextField>,
    add_image: Retained<CheckBox>,
    dark: Retained<CheckBox>,
    image_label: Retained<NSTextField>,
    image: Retained<NSTextField>,
    choose_image: Retained<Button>,
    output_label: Retained<NSTextField>,
    create_new: Retained<CheckBox>,
    replace: Retained<CheckBox>,
    convert: Retained<Button>,
    back: Retained<Button>,
}

pub struct DialogIvars {
    dialog: RefCell<ConverterDialog>,
    window: RefCell<Option<Retained<NSWindow>>>,
    controls: RefCell<Option<Controls>>,
    result: RefCell<Option<ConverterStart>>,
    controller: Weak<MainWindowController>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotConverterDialog"]
    #[ivars = DialogIvars]
    struct ConverterWindow;

    unsafe impl NSObjectProtocol for ConverterWindow {}

    unsafe impl NSWindowDelegate for ConverterWindow {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    unsafe impl NSControlTextEditingDelegate for ConverterWindow {
        /// Windows `EN_CHANGE` of the path field.
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            let is_path = notification.object().is_some_and(|object| {
                self.ivars()
                    .controls
                    .borrow()
                    .as_ref()
                    .is_some_and(|controls| {
                        std::ptr::eq(
                            Retained::as_ptr(&object).cast::<u8>(),
                            Retained::as_ptr(&controls.path).cast::<u8>(),
                        )
                    })
            });
            if is_path {
                self.update_formats();
            }
        }
    }

    unsafe impl NSTextFieldDelegate for ConverterWindow {}

    impl ConverterWindow {
        #[unsafe(method(back:))]
        fn back(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(targetChanged:))]
        fn target_changed(&self, _sender: Option<&AnyObject>) {
            self.update_options();
        }

        #[unsafe(method(browse:))]
        fn browse(&self, _sender: Option<&AnyObject>) {
            self.browse_path();
        }

        #[unsafe(method(chooseImage:))]
        fn choose_image(&self, _sender: Option<&AnyObject>) {
            self.browse_image();
        }

        /// Python `on_add_image` and `on_dark`: one of the two stays checked.
        #[unsafe(method(imageOption:))]
        fn image_option(&self, sender: Option<&AnyObject>) {
            self.pair_toggled(sender, true);
            self.update_options();
        }

        /// Python `on_create_new` and `on_replace`.
        #[unsafe(method(outputOption:))]
        fn output_option(&self, sender: Option<&AnyObject>) {
            self.pair_toggled(sender, false);
        }

        #[unsafe(method(convert:))]
        fn convert(&self, _sender: Option<&AnyObject>) {
            self.run_convert();
        }
    }
);

impl ConverterWindow {
    fn with_controls<R>(&self, body: impl FnOnce(&Controls) -> R) -> Option<R> {
        self.ivars().controls.borrow().as_ref().map(body)
    }

    fn path_text(&self) -> String {
        self.with_controls(|controls| controls.path.stringValue().to_string())
            .unwrap_or_default()
    }

    /// The two check boxes of a pair: unchecking one checks the other,
    /// checking one unchecks the other.
    fn pair_toggled(&self, sender: Option<&AnyObject>, image_pair: bool) {
        self.with_controls(|controls| {
            let (first, second) = if image_pair {
                (&controls.add_image, &controls.dark)
            } else {
                (&controls.create_new, &controls.replace)
            };
            let sender_is_first = sender.is_some_and(|sender| {
                std::ptr::eq(
                    std::ptr::from_ref(sender).cast::<u8>(),
                    Retained::as_ptr(first).cast::<u8>(),
                )
            });
            let (changed, other) = if sender_is_first {
                (first, second)
            } else {
                (second, first)
            };
            if changed.checked() {
                other.set_checked(false);
            } else if !other.checked() {
                other.set_checked(true);
            }
        });
    }

    /// Windows `update_formats`.
    fn update_formats(&self) {
        let path = self.path_text();
        let selected = self
            .with_controls(|controls| controls.target.selected())
            .flatten();
        let update = self
            .ivars()
            .dialog
            .borrow_mut()
            .update_formats(&path, selected);
        if let Some((labels, selection)) = update {
            self.with_controls(|controls| controls.target.set_items(&labels, selection));
        }
        self.update_options();
    }

    /// Windows `update_audio_video_controls` and `layout`.
    fn update_options(&self) {
        let path = self.path_text();
        self.with_controls(|controls| {
            let options = self.ivars().dialog.borrow().options(
                &path,
                controls.target.selected(),
                controls.add_image.checked(),
                controls.dark.checked(),
            );
            if options.check_dark {
                controls.dark.set_checked(true);
            }
            let image = options.audio_video && controls.add_image.checked();
            for view in [
                &*controls.options_label as &NSView,
                &controls.add_image,
                &controls.dark,
            ] {
                view.setHidden(!options.audio_video);
            }
            for view in [
                &*controls.image_label as &NSView,
                &controls.image,
                &controls.choose_image,
            ] {
                view.setHidden(!image);
            }
        });
        self.layout();
    }

    /// Windows `layout`, resizing the window around the shown rows.
    fn layout(&self) {
        let window = self.ivars().window.borrow();
        let Some(window) = window.as_ref() else {
            return;
        };
        let height = self.with_controls(place_controls).unwrap_or(300.0);
        let mut frame = window.frame();
        let content = window.contentRectForFrameRect(frame);
        let extra = frame.size.height - content.size.height;
        let top = frame.origin.y + frame.size.height;
        frame.size.height = height + extra;
        frame.origin.y = top - frame.size.height;
        window.setFrame_display(frame, true);
    }

    /// Windows `browse_path`.
    fn browse_path(&self) {
        let dialog = self.ivars().dialog.borrow();
        let panel = NSOpenPanel::openPanel(self.mtm());
        let title = dialog.text(if dialog.folder_mode {
            "browse_folder"
        } else {
            "browse_file"
        });
        panel.setTitle(Some(&NSString::from_str(&title)));
        panel.setMessage(Some(&NSString::from_str(&title)));
        panel.setAllowsMultipleSelection(false);
        if dialog.folder_mode {
            panel.setCanChooseFiles(false);
            panel.setCanChooseDirectories(true);
        } else {
            panel.setCanChooseFiles(true);
            panel.setCanChooseDirectories(false);
            set_extensions(&panel, &dialog.input_extensions());
        }
        drop(dialog);
        let chosen = run_panel(self.window().as_deref(), &panel);
        if let Some(path) = chosen {
            self.with_controls(|controls| {
                controls
                    .path
                    .setStringValue(&NSString::from_str(&path.to_string_lossy()));
            });
        }
        self.update_formats();
    }

    /// Windows `browse_image`.
    fn browse_image(&self) {
        let dialog = self.ivars().dialog.borrow();
        let panel = NSOpenPanel::openPanel(self.mtm());
        let title = dialog.text("select_image_file");
        panel.setTitle(Some(&NSString::from_str(&title)));
        panel.setMessage(Some(&NSString::from_str(&title)));
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        set_extensions(&panel, &dialog.image_extensions());
        drop(dialog);
        if let Some(path) = run_panel(self.window().as_deref(), &panel) {
            self.with_controls(|controls| {
                controls
                    .image
                    .setStringValue(&NSString::from_str(&path.to_string_lossy()));
            });
        }
    }

    fn window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().window.borrow().clone()
    }

    /// Windows `convert`.
    fn run_convert(&self) {
        let Some((path, selected, image, replace)) = self.with_controls(|controls| {
            let image = (!controls.add_image.isHidden() && controls.add_image.checked())
                .then(|| controls.image.stringValue().to_string());
            (
                controls.path.stringValue().to_string(),
                controls.target.selected(),
                image,
                controls.replace.checked(),
            )
        }) else {
            return;
        };
        let mut view = PanelView {
            window: self.window(),
            controller: self.ivars().controller.clone(),
        };
        let start = self.ivars().dialog.borrow().convert(
            &mut view,
            &path,
            selected,
            image.as_deref(),
            replace,
        );
        if let Some(start) = start {
            *self.ivars().result.borrow_mut() = Some(start);
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }
    }
}

/// The converter's own dialogs (Windows `warn` and the file dialogs).
struct PanelView {
    window: Option<Retained<NSWindow>>,
    controller: Weak<MainWindowController>,
}

impl ConverterView for PanelView {
    fn warn(&mut self, text: &str) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let previous = window.firstResponder();
        let alert = NSAlert::new(MainThreadMarker::from(&**window));
        alert.setMessageText(&NSString::from_str(text));
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        alert.runModal();
        window.makeKeyAndOrderFront(None);
        if let Some(previous) = previous {
            window.makeFirstResponder(Some(&previous));
        }
    }

    fn choose_folder(&mut self, title: &str, initial: &Path) -> Option<PathBuf> {
        let window = self.window.as_ref()?;
        let panel = NSOpenPanel::openPanel(MainThreadMarker::from(&**window));
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setCanChooseFiles(false);
        panel.setCanChooseDirectories(true);
        panel.setCanCreateDirectories(true);
        panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(
            &initial.to_string_lossy(),
        ))));
        run_panel(self.window.as_deref(), &panel)
    }

    fn choose_save_file(
        &mut self,
        title: &str,
        directory: &Path,
        name: &str,
        extensions: &[String],
    ) -> Option<PathBuf> {
        let window = self.window.as_ref()?;
        let panel = NSSavePanel::savePanel(MainThreadMarker::from(&**window));
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setNameFieldStringValue(&NSString::from_str(name));
        if !directory.as_os_str().is_empty() {
            panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(
                &directory.to_string_lossy(),
            ))));
        }
        set_extensions(&panel, extensions);
        run_panel(self.window.as_deref(), &panel)
    }

    fn announce(&mut self, text: &str) {
        if let Some(controller) = self.controller.load() {
            controller.with_shell(|_, ui| Shell::converter_announce(ui, text));
        }
    }
}

/// Runs an open or save panel; focus returns to the converter control
/// that had it.
fn run_panel(window: Option<&NSWindow>, panel: &NSSavePanel) -> Option<PathBuf> {
    let previous = window.and_then(NSWindow::firstResponder);
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

fn set_extensions(panel: &NSSavePanel, extensions: &[String]) {
    if extensions.is_empty() {
        return;
    }
    let extensions: Vec<Retained<NSString>> = extensions
        .iter()
        .map(|extension| NSString::from_str(extension))
        .collect();
    let extensions = NSArray::from_retained_slice(&extensions);
    // SAFETY: `setAllowedFileTypes:` takes an array of extensions.
    unsafe {
        let _: () = msg_send![panel, setAllowedFileTypes: &*extensions];
    }
}

/// Windows `layout`; returns the content height.
fn place_controls(controls: &Controls) -> f64 {
    let field_x = MARGIN + LABEL_WIDTH + GAP;
    let field_width = WIDTH - field_x - MARGIN;
    let place = |view: &NSView, x: f64, y: f64, width: f64, height: f64| {
        view.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)));
    };
    let mut y = MARGIN;
    place(&controls.path_label, MARGIN, y + 4.0, LABEL_WIDTH, ROW);
    place(
        &controls.path,
        field_x,
        y,
        field_width - BUTTON_WIDTH - GAP,
        ROW,
    );
    place(
        &controls.browse,
        field_x + field_width - BUTTON_WIDTH,
        y,
        BUTTON_WIDTH,
        ROW,
    );
    y += ROW + GAP;
    place(&controls.target_label, MARGIN, y + 4.0, LABEL_WIDTH, ROW);
    place(&controls.target, field_x, y, field_width, ROW);
    y += ROW + GAP;
    if !controls.add_image.isHidden() {
        place(&controls.options_label, MARGIN, y + 4.0, LABEL_WIDTH, ROW);
        place(&controls.add_image, field_x, y, field_width, ROW);
        y += ROW + GAP;
        place(&controls.dark, field_x, y, field_width, ROW);
        y += ROW + GAP;
    }
    if !controls.image.isHidden() {
        place(&controls.image_label, MARGIN, y + 4.0, LABEL_WIDTH, ROW);
        place(
            &controls.image,
            field_x,
            y,
            field_width - BUTTON_WIDTH - GAP,
            ROW,
        );
        place(
            &controls.choose_image,
            field_x + field_width - BUTTON_WIDTH,
            y,
            BUTTON_WIDTH,
            ROW,
        );
        y += ROW + GAP;
    }
    place(&controls.output_label, MARGIN, y + 4.0, LABEL_WIDTH, ROW);
    place(&controls.create_new, field_x, y, field_width, ROW);
    y += ROW + 3.0;
    place(&controls.replace, field_x, y, field_width, ROW);
    y += ROW + MARGIN;
    let buttons_x = WIDTH - MARGIN - BUTTON_WIDTH * 2.0 - GAP;
    place(&controls.convert, buttons_x, y, BUTTON_WIDTH, ROW + 4.0);
    place(
        &controls.back,
        buttons_x + BUTTON_WIDTH + GAP,
        y,
        BUTTON_WIDTH,
        ROW + 4.0,
    );
    y + ROW + 4.0 + MARGIN
}

fn label(mtm: MainThreadMarker, parent: &NSView, text: &str) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    parent.addSubview(&label);
    label
}

fn text_field(mtm: MainThreadMarker, parent: &NSView, name: &str) -> Retained<NSTextField> {
    let field = NSTextField::initWithFrame(NSTextField::alloc(mtm), NSRect::ZERO);
    // Python `SetName` on the path and image fields.
    field.setAccessibilityLabel(Some(&NSString::from_str(name)));
    parent.addSubview(&field);
    field
}

#[allow(clippy::too_many_lines)] // One block per control, as on Windows.
fn create_controls(mtm: MainThreadMarker, this: &ConverterWindow, content: &NSView) -> Controls {
    let target: &AnyObject = this.as_ref();
    let dialog = this.ivars().dialog.borrow();
    let (browse_label, create_label, replace_label) = dialog.mode_labels();
    let path_text = dialog.path_label();
    let path_label = label(mtm, content, &path_text);
    let path = text_field(mtm, content, &path_text);
    // SAFETY: the dialog outlives the field; the delegate is weak.
    unsafe { path.setDelegate(Some(ProtocolObject::from_ref(this))) };
    let browse = Button::new(mtm, &browse_label, target, sel!(browse:));
    content.addSubview(&browse);
    let convert_to = dialog.text("convert_to");
    let target_choice = PopUp::new(mtm, &convert_to, target, sel!(targetChanged:));
    content.addSubview(&target_choice);
    let target_label = label(mtm, content, &convert_to);
    let options_label = label(
        mtm,
        content,
        &dialog.text("converter_audio_to_video_options"),
    );
    let add_image = CheckBox::new(mtm, &dialog.text("add_image"), target, sel!(imageOption:));
    content.addSubview(&add_image);
    let dark = CheckBox::new(
        mtm,
        &dialog.text("dark_background"),
        target,
        sel!(imageOption:),
    );
    content.addSubview(&dark);
    let image_text = dialog.text("image_path");
    let image_label = label(mtm, content, &image_text);
    let image = text_field(mtm, content, &image_text);
    let choose_image = Button::new(
        mtm,
        &dialog.text("choose_image"),
        target,
        sel!(chooseImage:),
    );
    content.addSubview(&choose_image);
    let output_label = label(mtm, content, &dialog.text("output_format"));
    let create_new = CheckBox::new(mtm, &create_label, target, sel!(outputOption:));
    content.addSubview(&create_new);
    let replace = CheckBox::new(mtm, &replace_label, target, sel!(outputOption:));
    content.addSubview(&replace);
    let convert = Button::new(mtm, &dialog.text("convert"), target, sel!(convert:));
    content.addSubview(&convert);
    let back = Button::new(mtm, &dialog.text("back"), target, sel!(back:));
    // Windows `IDCANCEL` on Escape.
    back.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&back);
    // Python: "Create a new file" starts checked.
    create_new.set_checked(true);
    // Windows creation order, which is the Tab order.
    controls::set_tab_order(&[
        &path,
        &browse,
        &target_choice,
        &add_image,
        &dark,
        &image,
        &choose_image,
        &create_new,
        &replace,
        &convert,
        &back,
    ]);
    Controls {
        path_label,
        path,
        browse,
        target_label,
        target: target_choice,
        options_label,
        add_image,
        dark,
        image_label,
        image,
        choose_image,
        output_label,
        create_new,
        replace,
        convert,
        back,
    }
}

/// Windows `show_converter`: the modal converter; the chosen conversion
/// goes to [`Shell::converter_closed`].
pub fn run(controller: &MainWindowController, dialog: ConverterDialog) {
    let mtm = MainThreadMarker::from(controller);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let title = dialog.title();
    let this: Retained<ConverterWindow> = {
        let this = ConverterWindow::alloc(mtm).set_ivars(DialogIvars {
            dialog: RefCell::new(dialog),
            window: RefCell::new(None),
            controls: RefCell::new(None),
            result: RefCell::new(None),
            controller: Weak::from(controller),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    // SAFETY: a standard titled window with a buffered backing.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(WIDTH, 360.0)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is released by its last owner, this function.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(&title));
    window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
    // SAFETY: `initWithFrame:` of NSView.
    let content: Retained<FlippedView> = unsafe {
        msg_send![
            FlippedView::alloc(mtm),
            initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, 360.0))
        ]
    };
    window.setContentView(Some(&content));
    let controls = create_controls(mtm, &this, &content);
    let path = controls.path.clone();
    *this.ivars().controls.borrow_mut() = Some(controls);
    *this.ivars().window.borrow_mut() = Some(window.clone());
    this.update_formats();
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&path));
    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    // wx.Dialog focuses its first control, the path field.
    window.makeFirstResponder(Some(&path));
    app.runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    let result = this.ivars().result.borrow_mut().take();
    *this.ivars().controls.borrow_mut() = None;
    *this.ivars().window.borrow_mut() = None;
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    controller.with_shell(|shell, ui| shell.converter_closed(ui, result));
}

pub struct ProgressIvars {
    window: RefCell<Option<Retained<NSWindow>>>,
    message: RefCell<Option<Retained<NSTextField>>>,
    bar: RefCell<Option<Retained<NSProgressIndicator>>>,
    times: RefCell<Option<Retained<NSTextField>>>,
    timer: RefCell<Option<Retained<NSTimer>>>,
    total: Cell<usize>,
    converted: Cell<usize>,
    started: Cell<Option<Instant>>,
    previous_window: RefCell<Option<Retained<NSWindow>>>,
    previous_focus: RefCell<Option<Retained<NSResponder>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotConversionProgress"]
    #[ivars = ProgressIvars]
    pub struct ConversionProgress;

    unsafe impl NSObjectProtocol for ConversionProgress {}

    unsafe impl NSWindowDelegate for ConversionProgress {
        /// Python's dialog has no abort button and cannot be closed.
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            false
        }
    }

    impl ConversionProgress {
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: Option<&AnyObject>) {
            self.update_times();
        }
    }
);

fn clock(seconds: u64) -> String {
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

impl ConversionProgress {
    /// Windows `ConversionProgressWindow::create`.
    pub fn new(mtm: MainThreadMarker, title: &str, message: &str, total: usize) -> Retained<Self> {
        let app = NSApplication::sharedApplication(mtm);
        let previous_window = app.keyWindow();
        let previous_focus = previous_window
            .as_ref()
            .and_then(|window| window.firstResponder());
        let this: Retained<Self> = {
            let this = Self::alloc(mtm).set_ivars(ProgressIvars {
                window: RefCell::new(None),
                message: RefCell::new(None),
                bar: RefCell::new(None),
                times: RefCell::new(None),
                timer: RefCell::new(None),
                total: Cell::new(total.max(1)),
                converted: Cell::new(0),
                started: Cell::new(Some(Instant::now())),
                previous_window: RefCell::new(previous_window),
                previous_focus: RefCell::new(previous_focus),
            });
            // SAFETY: `init` of NSObject.
            unsafe { msg_send![super(this), init] }
        };
        // SAFETY: a titled window with a buffered backing.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(460.0, 170.0)),
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: the window is kept by this object.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        let content = window.contentView().expect("a window has a content view");
        let message_label = NSTextField::wrappingLabelWithString(&NSString::from_str(message), mtm);
        message_label.setFrame(NSRect::new(
            NSPoint::new(12.0, 106.0),
            NSSize::new(436.0, 52.0),
        ));
        content.addSubview(&message_label);
        let bar = NSProgressIndicator::new(mtm);
        bar.setStyle(NSProgressIndicatorStyle::Bar);
        bar.setIndeterminate(false);
        bar.setMinValue(0.0);
        #[allow(clippy::cast_precision_loss)]
        bar.setMaxValue(total.max(1) as f64);
        bar.setFrame(NSRect::new(
            NSPoint::new(12.0, 78.0),
            NSSize::new(436.0, 20.0),
        ));
        content.addSubview(&bar);
        let times = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
        times.setFrame(NSRect::new(
            NSPoint::new(12.0, 12.0),
            NSSize::new(436.0, 58.0),
        ));
        content.addSubview(&times);
        *this.ivars().window.borrow_mut() = Some(window.clone());
        *this.ivars().message.borrow_mut() = Some(message_label);
        *this.ivars().bar.borrow_mut() = Some(bar);
        *this.ivars().times.borrow_mut() = Some(times);
        this.update_times();
        // SAFETY: the object implements `tick:` and invalidates the timer
        // when it is destroyed.
        let tick_timer = unsafe {
            NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                1.0,
                &this,
                sel!(tick:),
                None,
                true,
            )
        };
        // SAFETY: AppKit's static run loop mode, so the timer runs modally.
        unsafe {
            objc2_foundation::NSRunLoop::currentRunLoop()
                .addTimer_forMode(&tick_timer, NSRunLoopCommonModes);
        }
        *this.ivars().timer.borrow_mut() = Some(tick_timer);
        window.center();
        app.activate();
        window.makeKeyAndOrderFront(None);
        this
    }

    /// Windows `ConversionProgressWindow::update`.
    pub fn update(&self, converted: usize, message: &str) {
        self.ivars()
            .converted
            .set(converted.min(self.ivars().total.get()));
        if let Some(label) = self.ivars().message.borrow().as_ref() {
            label.setStringValue(&NSString::from_str(message));
        }
        if let Some(bar) = self.ivars().bar.borrow().as_ref() {
            #[allow(clippy::cast_precision_loss)]
            bar.setDoubleValue(self.ivars().converted.get() as f64);
        }
        self.update_times();
    }

    /// wx shows elapsed, estimated and remaining time under the gauge.
    fn update_times(&self) {
        let elapsed = self
            .ivars()
            .started
            .get()
            .map_or(0, |started| started.elapsed().as_secs());
        let converted = self.ivars().converted.get();
        let total = self.ivars().total.get();
        let (estimated, remaining) = if converted == 0 {
            ("Unknown".to_owned(), "Unknown".to_owned())
        } else {
            let estimated =
                elapsed * u64::try_from(total).unwrap_or(1) / u64::try_from(converted).unwrap_or(1);
            (clock(estimated), clock(estimated.saturating_sub(elapsed)))
        };
        let text = format!(
            "Elapsed time: {}\nEstimated time: {estimated}\nRemaining time: {remaining}",
            clock(elapsed)
        );
        if let Some(times) = self.ivars().times.borrow().as_ref() {
            times.setStringValue(&NSString::from_str(&text));
        }
    }

    /// Windows `ConversionProgressWindow::destroy`: focus goes back to where
    /// it was when the window still had it.
    pub fn destroy(&self) {
        if let Some(timer) = self.ivars().timer.borrow_mut().take() {
            timer.invalidate();
        }
        let Some(window) = self.ivars().window.borrow_mut().take() else {
            return;
        };
        let active = window.isKeyWindow();
        window.setDelegate(None);
        window.orderOut(None);
        if active && let Some(previous) = self.ivars().previous_window.borrow_mut().take() {
            previous.makeKeyAndOrderFront(None);
            if let Some(focus) = self.ivars().previous_focus.borrow_mut().take() {
                previous.makeFirstResponder(Some(&focus));
            }
        }
    }
}
