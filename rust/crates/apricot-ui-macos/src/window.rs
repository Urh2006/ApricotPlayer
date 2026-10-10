//! The main window: the main menu list, the Open button and the status line,
//! laid out and focused as `create_controls` and `layout_controls_state` do
//! it on Windows.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use apricot_app::{Application, MainMenuModel};
use apricot_shell::{KeyRoute, Shell, ShellUi};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSEvent, NSEventType,
    NSResponder, NSScrollView, NSTextField, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::controls::{self, Button, List};
use crate::keys;

/// Windows `CreateWindowExW(..., 760, 600, ...)`.
const WINDOW_WIDTH: f64 = 760.0;
const WINDOW_HEIGHT: f64 = 600.0;
const MARGIN: f64 = 12.0;
const BUTTON_HEIGHT: f64 = 32.0;
const STATUS_HEIGHT: f64 = 20.0;

pub struct ControllerIvars {
    shell: RefCell<Option<Shell>>,
    window: RefCell<Option<Retained<MainWindow>>>,
    list: RefCell<Option<Retained<List>>>,
    open: RefCell<Option<Retained<Button>>>,
    status: RefCell<Option<Retained<NSTextField>>>,
    /// Windows `WindowLifecycle::Exiting`: Exit quits even with close to tray.
    exiting: Cell<bool>,
}

define_class!(
    /// Owns the shell and the controls of the main window.
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotMainWindowController"]
    #[ivars = ControllerIvars]
    pub struct MainWindowController;

    unsafe impl NSObjectProtocol for MainWindowController {}

    unsafe impl NSWindowDelegate for MainWindowController {
        /// Windows `WM_CLOSE`: with "close to tray" the window hides and the
        /// application keeps running; otherwise the application quits.
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            let close_to_background = !self.ivars().exiting.get()
                && self.with_shell(|shell, _| shell.application.settings().close_to_tray)
                    .unwrap_or(false);
            if close_to_background {
                if let Some(window) = self.ivars().window.borrow().as_ref() {
                    window.orderOut(None);
                }
            } else {
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            }
            false
        }
    }

    impl MainWindowController {
        /// The Open button and Enter in the list (Windows `ID_OPEN`).
        #[unsafe(method(openSelected:))]
        fn open_selected(&self, _sender: Option<&AnyObject>) {
            let row = self.list().and_then(|list| list.selected());
            if let Some(row) = row {
                self.with_shell(|shell, ui| shell.activate_main_menu_row(ui, row));
            }
        }

        /// Called by [`MainWindow`] for every key press before the focused
        /// control sees it; `true` when the key was handled.
        #[unsafe(method(handleKeyEvent:))]
        fn handle_key_event(&self, event: &NSEvent) -> bool {
            self.route_key_event(event)
        }
    }
);

impl MainWindowController {
    pub fn new(mtm: MainThreadMarker, application: Application) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ControllerIvars {
            shell: RefCell::new(Some(Shell::new(application))),
            window: RefCell::new(None),
            list: RefCell::new(None),
            open: RefCell::new(None),
            status: RefCell::new(None),
            exiting: Cell::new(false),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    }

    fn route_key_event(&self, event: &NSEvent) -> bool {
        let modifiers = controls::modifiers(event);
        let characters = event
            .charactersIgnoringModifiers()
            .map(|text| text.to_string())
            .unwrap_or_default();
        let Some(chord) = keys::chord(event.keyCode(), &characters, modifiers) else {
            return false;
        };
        let repeat = event.isARepeat();
        let route = self
            .with_shell(|shell, _| shell.route_key(chord, repeat))
            .unwrap_or(KeyRoute::Unhandled);
        match route {
            KeyRoute::Unhandled => false,
            KeyRoute::Consumed => true,
            KeyRoute::Run(action) => {
                self.with_shell(|shell, ui| shell.activate_action(ui, action));
                true
            }
        }
    }

    fn mtm(&self) -> MainThreadMarker {
        MainThreadMarker::from(self)
    }

    fn list(&self) -> Option<Retained<List>> {
        self.ivars().list.borrow().clone()
    }

    /// Runs `action` with the shell and this window as its UI. Returns `None`
    /// when the shell is already in use (a nested call during a modal alert).
    fn with_shell<R>(&self, action: impl FnOnce(&mut Shell, &mut dyn ShellUi) -> R) -> Option<R> {
        let mut shell = self.ivars().shell.try_borrow_mut().ok()?;
        let shell = shell.as_mut()?;
        let mut ui = MacUi { controller: self };
        Some(action(shell, &mut ui))
    }

    /// Windows `run_win32`: creates the window and its controls and shows
    /// the main menu with focus on the list.
    pub fn show(&self, title: &str, start_hidden: bool) {
        let mtm = self.mtm();
        let frame = NSRect::new(
            NSPoint::new(200.0, 200.0),
            NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT),
        );
        let window = MainWindow::new(mtm, frame, self);
        window.setTitle(&NSString::from_str(title));
        window.setDelegate(Some(ProtocolObject::from_ref(self)));
        let content = window.contentView().expect("a window has a content view");
        let bounds = content.bounds();

        let catalog = self
            .with_shell(|shell, _| shell.catalog())
            .expect("the shell is free while the window is built");
        let list = List::new(mtm, catalog.text("main_menu"));
        list.set_activate_action(self.as_ref(), sel!(openSelected:));
        let list_top = bounds.size.height - MARGIN;
        let list_bottom = MARGIN * 3.0 + BUTTON_HEIGHT + STATUS_HEIGHT;
        let scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(mtm),
            NSRect::new(
                NSPoint::new(MARGIN, list_bottom),
                NSSize::new(bounds.size.width - 2.0 * MARGIN, list_top - list_bottom),
            ),
        );
        scroll.setDocumentView(Some(&list));
        scroll.setHasVerticalScroller(true);
        scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.addSubview(&scroll);

        let open = Button::new(
            mtm,
            catalog.text("open"),
            self.as_ref(),
            sel!(openSelected:),
        );
        open.setFrame(NSRect::new(
            NSPoint::new(MARGIN, MARGIN * 2.0 + STATUS_HEIGHT),
            NSSize::new(160.0, BUTTON_HEIGHT),
        ));
        open.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMaxYMargin);
        content.addSubview(&open);

        let status = NSTextField::labelWithString(&NSString::from_str(catalog.text("ready")), mtm);
        status.setFrame(NSRect::new(
            NSPoint::new(MARGIN, MARGIN),
            NSSize::new(bounds.size.width - 2.0 * MARGIN, STATUS_HEIGHT),
        ));
        status.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        content.addSubview(&status);

        controls::set_tab_order(&[&list, &open]);
        window.setAutorecalculatesKeyViewLoop(false);
        window.setInitialFirstResponder(Some(&list));

        *self.ivars().list.borrow_mut() = Some(list.clone());
        *self.ivars().open.borrow_mut() = Some(open);
        *self.ivars().status.borrow_mut() = Some(status);
        *self.ivars().window.borrow_mut() = Some(window.clone());

        self.with_shell(|shell, ui| shell.start(ui));
        if !start_hidden {
            window.makeKeyAndOrderFront(None);
            window.makeFirstResponder(Some(&list));
            NSApplication::sharedApplication(mtm).activate();
        }
    }

    /// Development builds: the scripted keyboard check of
    /// [`crate::self_test`], when `APRICOT_MACOS_SELF_TEST` is set.
    #[cfg(debug_assertions)]
    pub fn run_self_test(&self) {
        use crate::self_test::{focus, key, send};
        let Some(window) = self.ivars().window.borrow().clone() else {
            return;
        };
        window.makeKeyAndOrderFront(None);
        let report = |step: &str| {
            let row = self.list().and_then(|list| list.selected());
            let label = row
                .and_then(|row| {
                    self.ivars()
                        .shell
                        .try_borrow()
                        .ok()?
                        .as_ref()?
                        .main_menu()
                        .items
                        .get(row)
                        .map(|item| item.label.clone())
                })
                .unwrap_or_default();
            let status = self
                .ivars()
                .status
                .borrow()
                .as_ref()
                .map(|status| status.stringValue().to_string())
                .unwrap_or_default();
            println!(
                "self-test {step}: focus={} row={row:?} {label:?} status={status:?}",
                focus(&window)
            );
        };
        report("start");
        for (step, code, text) in [
            ("P", 35, "p"),
            ("P again", 35, "p"),
            ("P third", 35, "p"),
            ("down", keys::KEY_DOWN, "\u{f701}"),
            ("H", 4, "h"),
        ] {
            send(&window, &key(code, text));
            report(step);
        }
        send(&window, &key(keys::KEY_RETURN, "\r"));
        report("Enter on a row");
        send(&window, &key(keys::KEY_TAB, "\t"));
        report("Tab");
        send(&window, &key(keys::KEY_TAB, "\t"));
        report("Tab again");
        send(
            &window,
            &crate::self_test::Key {
                code: 16,
                text: "y",
                flags: objc2_app_kit::NSEventModifierFlags::Command
                    | objc2_app_kit::NSEventModifierFlags::Option,
            },
        );
        report("Command+Option+Y");
        send(&window, &key(keys::KEY_ESCAPE, "\u{1b}"));
        report("Escape");
        NSApplication::sharedApplication(self.mtm()).terminate(None);
    }

    /// The Dock icon was clicked or the app was reopened (Windows
    /// `restore_from_tray`).
    pub fn restore(&self) {
        if let Some(window) = self.ivars().window.borrow().as_ref() {
            window.makeKeyAndOrderFront(None);
        }
    }

    /// An action from the menu bar.
    pub fn run_action(&self, action: &str) {
        self.with_shell(|shell, ui| shell.activate_action(ui, action));
    }

    /// Quits through Exit or Quit, never hiding to the background.
    pub fn mark_exiting(&self) {
        self.ivars().exiting.set(true);
    }
}

/// The shell's view of the macOS main window.
struct MacUi<'a> {
    controller: &'a MainWindowController,
}

impl ShellUi for MacUi<'_> {
    fn set_status(&mut self, message: &str, announce: bool) {
        if let Some(status) = self.controller.ivars().status.borrow().as_ref() {
            status.setStringValue(&NSString::from_str(message));
        }
        if announce {
            crate::announce::announce(self.controller.mtm(), message, false);
        }
    }

    fn show_main_menu(
        &mut self,
        model: &MainMenuModel,
        open_label: &str,
        selected: usize,
        focus: bool,
    ) {
        let Some(list) = self.controller.list() else {
            return;
        };
        list.set_label(&model.accessible_name);
        list.set_rows(
            model.items.iter().map(|item| item.label.clone()).collect(),
            Some(selected),
        );
        if let Some(open) = self.controller.ivars().open.borrow().as_ref() {
            open.setTitle(&NSString::from_str(open_label));
        }
        if focus && let Some(window) = self.controller.ivars().window.borrow().as_ref() {
            window.makeFirstResponder(Some(&list));
        }
    }

    fn selected_row(&self) -> Option<usize> {
        self.controller.list().and_then(|list| list.selected())
    }

    fn exit(&mut self) {
        self.controller.mark_exiting();
        NSApplication::sharedApplication(self.controller.mtm()).terminate(None);
    }
}

pub struct MainWindowIvars {
    controller: Cell<*const MainWindowController>,
}

define_class!(
    /// The main window. Key presses go to the controller before the focused
    /// control, as the Windows message loop calls `handle_shortcut_message`
    /// before `IsDialogMessageW`.
    // SAFETY: NSWindow may be subclassed; no Drop impl.
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotMainWindow"]
    #[ivars = MainWindowIvars]
    pub struct MainWindow;

    impl MainWindow {
        #[unsafe(method(sendEvent:))]
        fn send_event(&self, event: &NSEvent) {
            let controller = self.ivars().controller.get();
            if event.r#type() == NSEventType::KeyDown && !controller.is_null() {
                // SAFETY: the controller outlives the window; see `new`.
                let handled: bool =
                    unsafe { msg_send![&*controller, handleKeyEvent: event] };
                if handled {
                    return;
                }
            }
            // SAFETY: forwarding the event to NSWindow.
            unsafe {
                let _: () = msg_send![super(self), sendEvent: event];
            }
        }
    }
);

impl MainWindow {
    fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        controller: &MainWindowController,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MainWindowIvars {
            controller: Cell::new(std::ptr::from_ref(controller)),
        });
        // SAFETY: the designated initializer of NSWindow.
        let window: Retained<Self> = unsafe {
            msg_send![
                super(this),
                initWithContentRect: frame,
                styleMask: NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };
        // SAFETY: the controller keeps the window alive.
        unsafe { window.setReleasedWhenClosed(false) };
        window
    }
}
