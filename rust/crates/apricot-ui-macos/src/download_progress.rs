//! The download progress window, the macOS counterpart of
//! `download_progress_win32.rs`: a modeless window with the message, a
//! progress bar, Hide (focused, the default button) and Details. Hide and
//! closing only hide it; Details hides it and opens the download screen.

#![allow(unsafe_code)]

use std::cell::RefCell;

use apricot_shell::DownloadProgress;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSProgressIndicator, NSProgressIndicatorStyle, NSTextField,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::controls::{self, Button};
use crate::window::MainWindowController;

/// Windows `CreateWindowExW(..., 560, 190, ...)`.
const WIDTH: f64 = 560.0;
const HEIGHT: f64 = 190.0;
const MARGIN: f64 = 12.0;
const BUTTON_WIDTH: f64 = 130.0;
const BUTTON_HEIGHT: f64 = 30.0;

pub struct ProgressIvars {
    controller: Weak<MainWindowController>,
    window: RefCell<Option<Retained<NSWindow>>>,
    message: RefCell<Option<Retained<NSTextField>>>,
    bar: RefCell<Option<Retained<NSProgressIndicator>>>,
    hide: RefCell<Option<Retained<Button>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotDownloadProgress"]
    #[ivars = ProgressIvars]
    pub struct ProgressWindow;

    unsafe impl NSObjectProtocol for ProgressWindow {}

    unsafe impl NSWindowDelegate for ProgressWindow {
        /// Windows `WM_CLOSE` only hides the window.
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, sender: &NSWindow) -> bool {
            sender.orderOut(None);
            false
        }
    }

    impl ProgressWindow {
        #[unsafe(method(hideWindow:))]
        fn hide_window(&self, _sender: Option<&AnyObject>) {
            self.hide();
        }

        #[unsafe(method(showDetails:))]
        fn show_details(&self, _sender: Option<&AnyObject>) {
            self.hide();
            if let Some(controller) = self.ivars().controller.load() {
                controller.with_shell(|shell, ui| shell.download_progress_details(ui));
            }
        }
    }
);

impl ProgressWindow {
    /// Windows `DownloadProgressWindow::create`.
    pub fn new(controller: &MainWindowController, progress: &DownloadProgress) -> Retained<Self> {
        let mtm = MainThreadMarker::from(controller);
        let this: Retained<Self> = {
            let this = Self::alloc(mtm).set_ivars(ProgressIvars {
                controller: Weak::from(controller),
                window: RefCell::new(None),
                message: RefCell::new(None),
                bar: RefCell::new(None),
                hide: RefCell::new(None),
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
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: the window is kept by this object.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(&progress.title));
        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        let content = window.contentView().expect("a window has a content view");
        let message = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
        message.setFrame(NSRect::new(
            NSPoint::new(MARGIN, HEIGHT - MARGIN - 60.0),
            NSSize::new(WIDTH - 2.0 * MARGIN, 60.0),
        ));
        content.addSubview(&message);
        let bar = NSProgressIndicator::new(mtm);
        bar.setStyle(NSProgressIndicatorStyle::Bar);
        bar.setIndeterminate(false);
        bar.setMinValue(0.0);
        bar.setMaxValue(100.0);
        bar.setFrame(NSRect::new(
            NSPoint::new(MARGIN, MARGIN + BUTTON_HEIGHT + 16.0),
            NSSize::new(WIDTH - 2.0 * MARGIN, 20.0),
        ));
        content.addSubview(&bar);
        let hide = Button::new(mtm, &progress.hide, target, sel!(hideWindow:));
        hide.setFrame(NSRect::new(
            NSPoint::new(MARGIN, MARGIN),
            NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
        ));
        // Windows `BS_DEFPUSHBUTTON`.
        hide.setKeyEquivalent(&NSString::from_str("\r"));
        content.addSubview(&hide);
        let details = Button::new(mtm, &progress.details, target, sel!(showDetails:));
        details.setFrame(NSRect::new(
            NSPoint::new(MARGIN + BUTTON_WIDTH + 8.0, MARGIN),
            NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
        ));
        content.addSubview(&details);
        controls::set_tab_order(&[&hide, &details]);
        window.setAutorecalculatesKeyViewLoop(false);
        window.setInitialFirstResponder(Some(&hide));
        window.center();
        *this.ivars().window.borrow_mut() = Some(window);
        *this.ivars().message.borrow_mut() = Some(message);
        *this.ivars().bar.borrow_mut() = Some(bar);
        *this.ivars().hide.borrow_mut() = Some(hide);
        this.set_progress(progress.percent, &progress.message);
        this.show();
        this
    }

    /// Windows `DownloadProgressWindow::set_progress`.
    pub fn set_progress(&self, percent: u32, message: &str) {
        if let Some(label) = self.ivars().message.borrow().as_ref() {
            label.setStringValue(&NSString::from_str(message));
        }
        if let Some(bar) = self.ivars().bar.borrow().as_ref() {
            bar.setDoubleValue(f64::from(percent.min(100)));
        }
    }

    /// Windows `DownloadProgressWindow::show`: in front, focus on Hide.
    pub fn show(&self) {
        let window = self.ivars().window.borrow();
        let Some(window) = window.as_ref() else {
            return;
        };
        NSApplication::sharedApplication(self.mtm()).activate();
        window.makeKeyAndOrderFront(None);
        if let Some(hide) = self.ivars().hide.borrow().as_ref() {
            window.makeFirstResponder(Some(hide));
        }
    }

    fn hide(&self) {
        let window = self.ivars().window.borrow();
        if let Some(window) = window.as_ref() {
            window.orderOut(None);
        }
        // Focus returns to the main window, as hiding a Windows window
        // activates its owner.
        if let Some(controller) = self.ivars().controller.load()
            && let Some(main) = controller.window()
        {
            main.makeKeyAndOrderFront(None);
        }
    }

    /// Windows `DownloadProgressWindow::destroy`.
    pub fn destroy(&self) {
        if let Some(window) = self.ivars().window.borrow_mut().take() {
            let was_key = window.isKeyWindow();
            window.setDelegate(None);
            window.orderOut(None);
            if was_key
                && let Some(controller) = self.ivars().controller.load()
                && let Some(main) = controller.window()
            {
                main.makeKeyAndOrderFront(None);
            }
        }
    }
}
