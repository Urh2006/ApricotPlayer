//! The main window: the main menu list with the Open button, the player
//! page and the status line, laid out and focused as `create_controls`,
//! `layout_controls_state` and `refresh_player` do it on Windows.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use std::path::PathBuf;

use apricot_app::{Application, PlayerScreenModel};
use apricot_playback::MpvVideoMode;
use apricot_shell::{FocusKind, KeyRoute, PlayerFocus, ScreenControl, Shell, ShellUi};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSEvent, NSEventType,
    NSModalResponseOK, NSOpenPanel, NSResponder, NSScrollView, NSSound, NSTextField, NSView,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSTimer,
};

use crate::controls::{self, List};
use crate::keys;
use crate::player_page::PlayerPage;
use crate::screen_buttons::ScreenButtons;

/// Windows `PLAYBACK_TIMER_INTERVAL_MS`.
const PLAYBACK_POLL_SECONDS: f64 = 0.025;

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
    buttons: RefCell<Option<ScreenButtons>>,
    background_timer: RefCell<Option<Retained<NSTimer>>>,
    scroll: RefCell<Option<Retained<NSScrollView>>>,
    status: RefCell<Option<Retained<NSTextField>>>,
    page: RefCell<Option<PlayerPage>>,
    poll_timer: RefCell<Option<Retained<NSTimer>>>,
    /// The key code of a held player key (Windows `ControlledRepeatState`).
    held_key: Cell<Option<u16>>,
    /// The modifiers held at the last key event, to see which one went up.
    held_modifiers: Cell<keys::Modifiers>,
    /// Windows `WindowLifecycle::Exiting`: Exit quits even with close to tray.
    exiting: Cell<bool>,
    /// The next step of the development self-test.
    self_test_step: Cell<usize>,
    /// Activation requests waiting for the shell.
    activations: RefCell<Vec<apricot_app::ActivationRequest>>,
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

        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            if let Some(page) = self.ivars().page.borrow().as_ref() {
                page.layout();
            }
        }
    }

    impl MainWindowController {
        /// A button or check box of the player page (Windows
        /// `activation_for_command`).
        #[unsafe(method(playerControl:))]
        fn player_control(&self, sender: Option<&AnyObject>) {
            let tag = sender
                .and_then(|sender| sender.downcast_ref::<objc2_app_kit::NSControl>())
                .map(objc2_app_kit::NSControl::tag);
            let action = tag.and_then(|tag| {
                self.ivars()
                    .page
                    .borrow()
                    .as_ref()
                    .and_then(|page| page.activation(tag))
            });
            if let Some(action) = action {
                self.with_shell(|shell, ui| shell.activate_player_control(ui, action));
            }
        }

        /// Windows `PLAYBACK_TIMER_ID`.
        #[unsafe(method(pollPlayback:))]
        fn poll_playback(&self, _timer: Option<&AnyObject>) {
            if self.with_shell(|shell, ui| shell.poll_playback(ui)) == Some(false) {
                self.stop_poll_timer();
            }
        }

        /// Windows `CONTROLLED_REPEAT_TIMER_ID`.
        #[unsafe(method(keyRepeatTick:))]
        fn key_repeat_tick(&self, _sender: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.key_repeat_tick(ui));
        }

        /// Opens the equalizer after the event that asked for it, so the
        /// shell is free while the dialog runs.
        #[unsafe(method(openEqualizer:))]
        fn open_equalizer(&self, _sender: Option<&AnyObject>) {
            crate::equalizer::run(self);
        }

        #[unsafe(method(drainActivations:))]
        fn drain_activations_selector(&self, _sender: Option<&AnyObject>) {
            self.drain_activations();
        }

        #[unsafe(method(modalSelfTestKeys:))]
        fn modal_self_test_keys_selector(&self, _sender: Option<&AnyObject>) {
            #[cfg(debug_assertions)]
            self.modal_self_test_keys();
        }

        #[unsafe(method(playerSelfTestStep:))]
        fn player_self_test_step_selector(&self, _sender: Option<&AnyObject>) {
            #[cfg(debug_assertions)]
            self.player_self_test_step();
        }

        /// Called by [`MainWindow`] for key releases and modifier changes.
        #[unsafe(method(handleKeyRelease:))]
        fn handle_key_release(&self, event: &NSEvent) {
            self.key_release(event);
        }
        /// Enter or a double click in the list (Windows `ID_OPEN`).
        #[unsafe(method(openSelected:))]
        fn open_selected(&self, _sender: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.activate_screen_button(ui, "open"));
        }

        /// A button of a list screen.
        #[unsafe(method(screenButton:))]
        fn screen_button(&self, sender: Option<&AnyObject>) {
            let tag = sender
                .and_then(|sender| sender.downcast_ref::<objc2_app_kit::NSControl>())
                .map(objc2_app_kit::NSControl::tag);
            let id = tag.and_then(|tag| {
                self.ivars()
                    .buttons
                    .borrow()
                    .as_ref()
                    .and_then(|buttons| buttons.id_for_tag(tag))
            });
            if let Some(id) = id {
                self.with_shell(|shell, ui| shell.activate_screen_button(ui, id));
            }
        }

        /// Windows `LOCAL_FOLDER_TIMER_ID` and the other work timers.
        #[unsafe(method(pollBackground:))]
        fn poll_background(&self, _timer: Option<&AnyObject>) {
            if self.with_shell(|shell, ui| shell.poll_background(ui)) == Some(false)
                && let Some(timer) = self.ivars().background_timer.borrow_mut().take()
            {
                timer.invalidate();
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
            buttons: RefCell::new(None),
            background_timer: RefCell::new(None),
            scroll: RefCell::new(None),
            status: RefCell::new(None),
            page: RefCell::new(None),
            poll_timer: RefCell::new(None),
            held_key: Cell::new(None),
            held_modifiers: Cell::new(keys::Modifiers::default()),
            exiting: Cell::new(false),
            self_test_step: Cell::new(0),
            activations: RefCell::new(Vec::new()),
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
        let focus = self.focus_kind();
        let route = self
            .with_shell(|shell, _| shell.route_key(chord, repeat, focus))
            .unwrap_or(KeyRoute::Unhandled);
        match route {
            KeyRoute::Unhandled => false,
            KeyRoute::Consumed => true,
            KeyRoute::Run(action) => {
                self.with_shell(|shell, ui| shell.activate_action(ui, action));
                true
            }
            KeyRoute::Hold(action, chord) => {
                self.ivars().held_key.set(Some(event.keyCode()));
                self.ivars().held_modifiers.set(modifiers);
                self.with_shell(|shell, ui| shell.start_key_repeat(ui, action, chord));
                true
            }
        }
    }

    /// Windows `handle_controlled_repeat_release`: the held key or one of
    /// its modifiers went up.
    fn key_release(&self, event: &NSEvent) {
        let Some(held) = self.ivars().held_key.get() else {
            return;
        };
        let now = controls::modifiers(event);
        let before = self.ivars().held_modifiers.get();
        self.ivars().held_modifiers.set(now);
        let main_key = event.r#type() == NSEventType::KeyUp && event.keyCode() == held;
        let released = self.with_shell(|shell, ui| {
            shell.key_released(
                ui,
                apricot_shell::KeyRelease {
                    main_key,
                    primary: before.command && !now.command,
                    shift: before.shift && !now.shift,
                    alt: before.option && !now.option,
                },
            );
            !shell.is_repeating()
        });
        if released == Some(true) {
            self.ivars().held_key.set(None);
        }
    }

    /// What has focus, for the shell's key routing.
    fn focus_kind(&self) -> FocusKind {
        let Some(responder) = self
            .ivars()
            .window
            .borrow()
            .as_ref()
            .and_then(|window| window.firstResponder())
        else {
            return FocusKind::Other;
        };
        let responder: &AnyObject = responder.as_ref();
        if let Some(list) = self.list()
            && std::ptr::eq(responder, std::ptr::from_ref::<AnyObject>(list.as_ref()))
        {
            return FocusKind::List;
        }
        let on_screen_button = self
            .ivars()
            .buttons
            .borrow()
            .as_ref()
            .is_some_and(|buttons| {
                responder
                    .downcast_ref::<crate::controls::Button>()
                    .is_some_and(|button| {
                        buttons
                            .id_for_tag(button.tag())
                            .and_then(|id| buttons.button(id))
                            .is_some_and(|shown| std::ptr::eq(shown, button))
                    })
            });
        if on_screen_button {
            return FocusKind::Button;
        }
        let page = self.ivars().page.borrow();
        let Some(page) = page.as_ref() else {
            return FocusKind::Other;
        };
        match page.control_id(responder) {
            Some("video_host") => FocusKind::PlayerSurface,
            Some("details_text") => FocusKind::DetailsText,
            Some(id) if page.is_checkbox(id) => FocusKind::PlayerCheckbox,
            Some(_) => FocusKind::PlayerButton,
            None => FocusKind::Other,
        }
    }

    fn stop_poll_timer(&self) {
        if let Some(timer) = self.ivars().poll_timer.borrow_mut().take() {
            timer.invalidate();
        }
    }

    /// Saves the player position before the application ends.
    pub fn shutdown(&self) {
        self.with_shell(|shell, ui| shell.shutdown(ui));
    }

    fn mtm(&self) -> MainThreadMarker {
        MainThreadMarker::from(self)
    }

    fn list(&self) -> Option<Retained<List>> {
        self.ivars().list.borrow().clone()
    }

    /// Runs `action` with the shell and this window as its UI. Returns `None`
    /// when the shell is already in use (a nested call during a modal alert).
    pub(crate) fn with_shell<R>(
        &self,
        action: impl FnOnce(&mut Shell, &mut dyn ShellUi) -> R,
    ) -> Option<R> {
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

        let buttons = ScreenButtons::new(mtm, self.as_ref(), sel!(screenButton:));
        buttons.view().setFrame(NSRect::new(
            NSPoint::new(MARGIN, MARGIN * 2.0 + STATUS_HEIGHT),
            NSSize::new(bounds.size.width - 2.0 * MARGIN, BUTTON_HEIGHT),
        ));
        buttons.view().setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        content.addSubview(buttons.view());

        let status = NSTextField::labelWithString(&NSString::from_str(catalog.text("ready")), mtm);
        status.setFrame(NSRect::new(
            NSPoint::new(MARGIN, MARGIN),
            NSSize::new(bounds.size.width - 2.0 * MARGIN, STATUS_HEIGHT),
        ));
        status.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        content.addSubview(&status);

        let page = PlayerPage::new(mtm, self.as_ref(), sel!(playerControl:));
        page.view().setFrame(NSRect::new(
            NSPoint::new(0.0, MARGIN * 2.0 + STATUS_HEIGHT),
            NSSize::new(
                bounds.size.width,
                bounds.size.height - MARGIN * 2.0 - STATUS_HEIGHT,
            ),
        ));
        page.view().setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.addSubview(page.view());

        window.setAutorecalculatesKeyViewLoop(false);
        window.setInitialFirstResponder(Some(&list));

        *self.ivars().list.borrow_mut() = Some(list.clone());
        *self.ivars().buttons.borrow_mut() = Some(buttons);
        *self.ivars().scroll.borrow_mut() = Some(scroll);
        *self.ivars().status.borrow_mut() = Some(status);
        *self.ivars().page.borrow_mut() = Some(page);
        *self.ivars().window.borrow_mut() = Some(window.clone());

        self.with_shell(|shell, ui| shell.start(ui));
        if !start_hidden {
            window.makeKeyAndOrderFront(None);
            window.makeFirstResponder(Some(&list));
            NSApplication::sharedApplication(mtm).activate();
        }
        if !start_hidden {
            self.with_shell(|shell, ui| shell.schedule_saved_audio_device_check(ui));
        }
        // Windows `process_pending_activations` after the window is shown.
        self.with_shell(|shell, ui| shell.process_activations(ui));
    }

    /// A file opened from Finder or a second launch (Windows `WM_COPYDATA`).
    pub fn open_file(&self, path: PathBuf) {
        self.enqueue_activation(apricot_app::ActivationRequest::OpenFile(path));
    }

    /// A request of Finder, the Dock or a second launch. While a modal
    /// window keeps the shell busy it waits, as Windows defers
    /// `WM_PROCESS_ACTIVATION` while `modal_open` is set.
    pub fn enqueue_activation(&self, request: apricot_app::ActivationRequest) {
        self.ivars().activations.borrow_mut().push(request);
        self.drain_activations();
    }

    fn drain_activations(&self) {
        let requests = std::mem::take(&mut *self.ivars().activations.borrow_mut());
        if requests.is_empty() {
            return;
        }
        let handled = self.with_shell(|shell, ui| {
            for request in requests.iter().cloned() {
                shell.application.enqueue_activation(request);
            }
            shell.process_activations(ui);
        });
        if handled.is_none() {
            self.ivars().activations.borrow_mut().splice(0..0, requests);
            // SAFETY: `drainActivations:` exists on this controller; the
            // delay runs in the default run loop mode, after a modal window.
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelector: sel!(drainActivations:),
                    withObject: Option::<&AnyObject>::None,
                    afterDelay: 0.2_f64
                ];
            }
        }
    }

    fn window(&self) -> Option<Retained<MainWindow>> {
        self.ivars().window.borrow().clone()
    }

    /// Shows the list screen controls or the player page.
    fn show_screen(&self, player: bool) {
        if let Some(scroll) = self.ivars().scroll.borrow().as_ref() {
            scroll.setHidden(player);
        }
        if let Some(buttons) = self.ivars().buttons.borrow().as_ref() {
            buttons.set_hidden(player);
        }
        if let Some(page) = self.ivars().page.borrow().as_ref() {
            page.set_visible(player);
        }
    }

    /// Windows `layout_controls_state` for a list screen: the buttons above
    /// the status line, the list above them.
    fn layout_list_screen(&self, button_count: usize) {
        let (Some(window), Some(scroll)) = (self.window(), self.ivars().scroll.borrow().clone())
        else {
            return;
        };
        let Some(content) = window.contentView() else {
            return;
        };
        let bounds = content.bounds();
        let width = bounds.size.width - 2.0 * MARGIN;
        let buttons_height = ScreenButtons::height_for(button_count, width);
        if let Some(buttons) = self.ivars().buttons.borrow().as_ref() {
            buttons.view().setFrame(NSRect::new(
                NSPoint::new(MARGIN, MARGIN * 2.0 + STATUS_HEIGHT),
                NSSize::new(width, buttons_height),
            ));
        }
        let list_bottom = MARGIN * 3.0 + STATUS_HEIGHT + buttons_height;
        scroll.setFrame(NSRect::new(
            NSPoint::new(MARGIN, list_bottom),
            NSSize::new(width, bounds.size.height - MARGIN - list_bottom),
        ));
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

    /// Development builds: a scripted player check when
    /// `APRICOT_MACOS_SELF_TEST=player` and `APRICOT_MACOS_SELF_TEST_MEDIA`
    /// name a media file. Steps run 0.7 s apart so libmpv can answer.
    #[cfg(debug_assertions)]
    pub fn run_player_self_test(&self, media: PathBuf) {
        self.open_file(media);
        self.ivars().self_test_step.set(0);
        // SAFETY: `playerSelfTestStep:` exists on this controller.
        unsafe {
            let _: () = msg_send![
                self,
                performSelector: sel!(playerSelfTestStep:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 1.5_f64
            ];
        }
    }

    /// Development builds: a scripted folder check when
    /// `APRICOT_MACOS_SELF_TEST=folder` and `APRICOT_MACOS_SELF_TEST_MEDIA`
    /// name a folder with media files.
    #[cfg(debug_assertions)]
    pub fn run_folder_self_test(&self, folder: PathBuf) {
        self.with_shell(|shell, ui| shell.start_local_folder_scan(ui, folder));
        self.ivars().self_test_step.set(100);
        // SAFETY: `playerSelfTestStep:` exists on this controller.
        unsafe {
            let _: () = msg_send![
                self,
                performSelector: sel!(playerSelfTestStep:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 1.5_f64
            ];
        }
    }

    /// Development builds with `APRICOT_MACOS_SELF_TEST=tools`: chapters,
    /// replay gain, BPM and the copy actions on a file with chapters.
    #[cfg(debug_assertions)]
    pub fn start_tools_self_test(&self) {
        self.ivars().self_test_step.set(200);
    }

    #[cfg(debug_assertions)]
    fn tools_self_test_step(&self, index: usize) -> bool {
        use crate::self_test::{Key, focus, key, send};
        use objc2_app_kit::NSEventModifierFlags as Flags;
        let Some(window) = self.window() else {
            return false;
        };
        let with = |code, text, flags| Key { code, text, flags };
        let steps: [(&str, Option<Key>); 13] = [
            ("opened", None),
            (
                "Option+Right (next chapter)",
                Some(with(keys::KEY_RIGHT, "\u{f703}", Flags::Option)),
            ),
            (
                "Option+Right (next chapter)",
                Some(with(keys::KEY_RIGHT, "\u{f703}", Flags::Option)),
            ),
            (
                "Option+Left (previous chapter)",
                Some(with(keys::KEY_LEFT, "\u{f702}", Flags::Option)),
            ),
            (
                "Command+Shift+G (ReplayGain)",
                Some(with(5, "G", Flags::Command | Flags::Shift)),
            ),
            ("B (BPM)", Some(key(11, "b"))),
            ("waiting for BPM", None),
            ("waiting for BPM", None),
            ("L (copy path)", Some(key(37, "l"))),
            (
                "Command+Shift+L (timestamp link)",
                Some(with(37, "L", Flags::Command | Flags::Shift)),
            ),
            ("Command+D (stream URL)", Some(with(2, "d", Flags::Command))),
            (
                "Command+Shift+C (chapters, Down, Return)",
                Some(with(8, "C", Flags::Command | Flags::Shift)),
            ),
            ("after the chapter list", None),
        ];
        let Some((name, key)) = steps.get(index) else {
            return false;
        };
        if index == 11 {
            // The list is modal; answer it from the modal run loop.
            let modes = NSArray::from_slice(&[
                // SAFETY: AppKit's static run loop mode name.
                unsafe { objc2_foundation::NSRunLoopCommonModes },
            ]);
            // SAFETY: `modalSelfTestKeys:` exists on this controller.
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelector: sel!(modalSelfTestKeys:),
                    withObject: Option::<&AnyObject>::None,
                    afterDelay: 0.6_f64,
                    inModes: &*modes
                ];
            }
        }
        if let Some(key) = key {
            send(&window, key);
        }
        let status = self
            .ivars()
            .status
            .borrow()
            .as_ref()
            .map(|status| status.stringValue().to_string())
            .unwrap_or_default();
        println!(
            "tools-test {name}: focus={} status={status:?}",
            focus(&window)
        );
        true
    }

    #[cfg(debug_assertions)]
    fn modal_self_test_keys(&self) {
        use crate::self_test::{focus, key, send};
        let Some(window) = NSApplication::sharedApplication(self.mtm()).modalWindow() else {
            println!("tools-test chapter list: no modal window");
            return;
        };
        println!(
            "tools-test chapter list: focus={} title={:?}",
            focus(&window),
            window.title().to_string()
        );
        send(&window, &key(keys::KEY_DOWN, "\u{f701}"));
        println!("tools-test chapter list Down: focus={}", focus(&window));
        send(&window, &key(keys::KEY_RETURN, "\r"));
    }

    #[cfg(debug_assertions)]
    fn folder_self_test_step(&self, index: usize) -> bool {
        use crate::self_test::{focus, key, send};
        let Some(window) = self.window() else {
            return false;
        };
        let steps: [(&str, Option<crate::self_test::Key>); 11] = [
            ("loaded", None),
            ("Down", Some(key(keys::KEY_DOWN, "\u{f701}"))),
            ("Enter (play)", Some(key(keys::KEY_RETURN, "\r"))),
            (
                "Escape (back to folder)",
                Some(key(keys::KEY_ESCAPE, "\u{1b}")),
            ),
            ("Tab", Some(key(keys::KEY_TAB, "\t"))),
            ("Tab", Some(key(keys::KEY_TAB, "\t"))),
            ("Tab", Some(key(keys::KEY_TAB, "\t"))),
            ("Tab", Some(key(keys::KEY_TAB, "\t"))),
            (
                "Shift+Tab",
                Some(crate::self_test::Key {
                    code: keys::KEY_TAB,
                    text: "\t",
                    flags: objc2_app_kit::NSEventModifierFlags::Shift,
                }),
            ),
            ("Space on Add to queue? no: focus report", None),
            ("Escape (main menu)", Some(key(keys::KEY_ESCAPE, "\u{1b}"))),
        ];
        let Some((name, key)) = steps.get(index) else {
            return false;
        };
        if let Some(key) = key {
            send(&window, key);
        }
        let row = self.list().and_then(|list| list.selected());
        let status = self
            .ivars()
            .status
            .borrow()
            .as_ref()
            .map(|status| status.stringValue().to_string())
            .unwrap_or_default();
        println!(
            "folder-test {name}: focus={} row={row:?} title={:?} status={status:?}",
            focus(&window),
            window.title().to_string()
        );
        true
    }

    #[cfg(debug_assertions)]
    fn player_self_test_step(&self) {
        use crate::self_test::{Key, focus, key, send};
        let step = self.ivars().self_test_step.get();
        if step >= 100 {
            let more = if step >= 200 {
                self.tools_self_test_step(step - 200)
            } else {
                self.folder_self_test_step(step - 100)
            };
            if more {
                self.ivars().self_test_step.set(step + 1);
                // SAFETY: `playerSelfTestStep:` exists on this controller.
                unsafe {
                    let _: () = msg_send![
                        self,
                        performSelector: sel!(playerSelfTestStep:),
                        withObject: Option::<&AnyObject>::None,
                        afterDelay: 0.8_f64
                    ];
                }
            } else {
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            }
            return;
        }
        let Some(window) = self.window() else {
            return;
        };
        let command = objc2_app_kit::NSEventModifierFlags::Command;
        let steps: [(&str, Option<Key>); 20] = [
            ("opened", None),
            ("T (time)", Some(key(17, "t"))),
            ("Space (pause)", Some(key(keys::KEY_SPACE, " "))),
            ("Space (play)", Some(key(keys::KEY_SPACE, " "))),
            ("D (speed up)", Some(key(2, "d"))),
            (
                "Command+0 (reset)",
                Some(Key {
                    code: 29,
                    text: "0",
                    flags: command,
                }),
            ),
            ("R (repeat)", Some(key(15, "r"))),
            ("Right (seek)", Some(key(keys::KEY_RIGHT, "\u{f703}"))),
            ("V (volume)", Some(key(9, "v"))),
            ("Down (volume)", Some(key(keys::KEY_DOWN, "\u{f701}"))),
            ("V (volume)", Some(key(9, "v"))),
            ("F7 (details)", Some(key(98, "\u{f70a}"))),
            ("Down in details", Some(key(keys::KEY_DOWN, "\u{f701}"))),
            (
                "Escape (hide details)",
                Some(key(keys::KEY_ESCAPE, "\u{1b}")),
            ),
            ("F11 (full screen)", Some(key(103, "\u{f70e}"))),
            (
                "Escape (leave full screen)",
                Some(key(keys::KEY_ESCAPE, "\u{1b}")),
            ),
            ("F4 (equalizer)", Some(key(118, "\u{f707}"))),
            ("after the equalizer", None),
            ("Tab", Some(key(keys::KEY_TAB, "\t"))),
            ("Escape", Some(key(keys::KEY_ESCAPE, "\u{1b}"))),
        ];
        let index = self.ivars().self_test_step.get();
        if let Some((name, key)) = steps.get(index) {
            if let Some(key) = key {
                send(&window, key);
            }
            let status = self
                .ivars()
                .status
                .borrow()
                .as_ref()
                .map(|status| status.stringValue().to_string())
                .unwrap_or_default();
            println!(
                "player-test {name}: focus={} title={:?} status={status:?} frames={}",
                focus(&window),
                window.title().to_string(),
                crate::video::frames_drawn()
            );
            self.ivars().self_test_step.set(index + 1);
            // SAFETY: as in `run_player_self_test`.
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelector: sel!(playerSelfTestStep:),
                    withObject: Option::<&AnyObject>::None,
                    afterDelay: 0.7_f64
                ];
            }
        } else {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }

    /// The Dock icon was clicked or the app was reopened (Windows
    /// `restore_from_tray`).
    pub fn restore(&self) {
        if let Some(window) = self.ivars().window.borrow().as_ref() {
            window.makeKeyAndOrderFront(None);
        }
        NSApplication::sharedApplication(self.mtm()).activate();
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

/// Python `DEFAULT_REACHED_SOUND`: `assets/default_reached.wav` next to the
/// executable, in a development build the repository's copy.
fn default_reached_sound_path() -> Option<PathBuf> {
    let bundled = std::env::current_exe()
        .ok()?
        .parent()?
        .join("assets")
        .join("default_reached.wav");
    if bundled.is_file() {
        return Some(bundled);
    }
    let repository =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../assets/default_reached.wav");
    (cfg!(debug_assertions) && repository.is_file()).then_some(repository)
}

/// Windows timers keep firing while a dialog runs its modal loop; AppKit
/// timers only do so in the common run loop modes.
fn run_in_modal_loops(timer: &NSTimer) {
    // SAFETY: AppKit's static run loop mode name.
    unsafe {
        objc2_foundation::NSRunLoop::currentRunLoop()
            .addTimer_forMode(timer, objc2_foundation::NSRunLoopCommonModes);
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

    fn show_list_screen(&mut self, screen: &apricot_shell::ListScreen) {
        self.controller.show_screen(false);
        let Some(list) = self.controller.list() else {
            return;
        };
        list.set_label(&screen.list_name);
        list.set_rows(screen.rows.clone(), screen.selected);
        let buttons: Vec<(&'static str, &str)> = screen
            .controls
            .iter()
            .filter_map(|control| match control {
                ScreenControl::Button { id, label } => Some((*id, label.as_str())),
                ScreenControl::List => None,
            })
            .collect();
        self.controller.layout_list_screen(buttons.len());
        if let Some(screen_buttons) = self.controller.ivars().buttons.borrow_mut().as_mut() {
            screen_buttons.sync(&buttons);
        }
        {
            let screen_buttons = self.controller.ivars().buttons.borrow();
            let views: Vec<&NSView> = screen
                .controls
                .iter()
                .filter_map(|control| match control {
                    ScreenControl::List => Some(&**list as &NSView),
                    ScreenControl::Button { id, .. } => screen_buttons
                        .as_ref()
                        .and_then(|buttons| buttons.button(id))
                        .map(|button| button as &NSView),
                })
                .collect();
            controls::set_tab_order(&views);
        }
        if screen.focus_list
            && let Some(window) = self.controller.window()
        {
            window.makeFirstResponder(Some(&list));
        }
    }

    fn choose_folder(&mut self, title: &str) -> Option<PathBuf> {
        let panel = NSOpenPanel::openPanel(self.controller.mtm());
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setCanChooseFiles(false);
        panel.setCanChooseDirectories(true);
        panel.setAllowsMultipleSelection(false);
        if panel.runModal() != NSModalResponseOK {
            return None;
        }
        panel
            .URL()
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
    }

    fn set_background_polling(&mut self, active: bool) {
        let mut timer = self.controller.ivars().background_timer.borrow_mut();
        if !active {
            if let Some(timer) = timer.take() {
                timer.invalidate();
            }
            return;
        }
        if timer.is_some() {
            return;
        }
        // SAFETY: the controller implements `pollBackground:` and outlives
        // the timer, which is invalidated when polling stops.
        let background = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                0.05,
                self.controller.as_ref(),
                sel!(pollBackground:),
                None,
                true,
            )
        };
        run_in_modal_loops(&background);
        *timer = Some(background);
    }

    fn selected_row(&self) -> Option<usize> {
        self.controller.list().and_then(|list| list.selected())
    }

    fn show_error(&mut self, message: &str) {
        let window = self.controller.window();
        let previous = window.as_ref().and_then(|window| window.firstResponder());
        let alert = NSAlert::new(self.controller.mtm());
        alert.setMessageText(&NSString::from_str(message));
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        alert.runModal();
        // Windows `show_message_box`: focus returns to the control that had it.
        if let (Some(window), Some(previous)) = (window, previous) {
            window.makeFirstResponder(Some(&previous));
        }
    }

    fn show_player(&mut self, model: &PlayerScreenModel, focus: PlayerFocus) {
        let previous = (focus == PlayerFocus::Preserve)
            .then(|| self.focused_player_control())
            .flatten();
        {
            let mut page = self.controller.ivars().page.borrow_mut();
            let Some(page) = page.as_mut() else {
                return;
            };
            page.sync(model);
        }
        self.controller.show_screen(true);
        let page = self.controller.ivars().page.borrow();
        let Some(page) = page.as_ref() else {
            return;
        };
        let target = match focus {
            PlayerFocus::Keep => None,
            PlayerFocus::Initial => Some(page.initial_focus()),
            PlayerFocus::Preserve => previous.and_then(|id| page.view_for(id)),
        };
        if let (Some(target), Some(window)) = (target, self.controller.window()) {
            window.makeFirstResponder(Some(target));
        }
    }

    fn show_player_details(&mut self, labels: &apricot_shell::DetailsLabels, text: &str) {
        let mut page = self.controller.ivars().page.borrow_mut();
        let Some(page) = page.as_mut() else {
            return;
        };
        page.show_details(labels, text);
        if let Some(window) = self.controller.window() {
            window.makeFirstResponder(Some(page.details_text()));
        }
    }

    fn hide_player_details(&mut self) {
        let mut page = self.controller.ivars().page.borrow_mut();
        let Some(page) = page.as_mut() else {
            return;
        };
        let had_focus = self
            .controller
            .window()
            .and_then(|window| window.firstResponder())
            .is_some_and(|responder| page.control_id(responder.as_ref()).is_some());
        page.hide_details();
        if had_focus
            && let (Some(window), Some(surface)) =
                (self.controller.window(), page.view_for("video_host"))
        {
            window.makeFirstResponder(Some(surface));
        }
    }

    fn set_player_details_text(&mut self, text: &str) {
        if let Some(page) = self.controller.ivars().page.borrow().as_ref() {
            page.set_details_text(text);
        }
    }

    fn focus_player_control(&mut self, id: &str) {
        let page = self.controller.ivars().page.borrow();
        if let (Some(window), Some(view)) = (
            self.controller.window(),
            page.as_ref().and_then(|page| page.view_for(id)),
        ) {
            window.makeFirstResponder(Some(view));
        }
    }

    fn set_window_fullscreen(&mut self, fullscreen: bool) {
        let Some(window) = self.controller.window() else {
            return;
        };
        let current = window.styleMask().contains(NSWindowStyleMask::FullScreen);
        if current != fullscreen {
            window.toggleFullScreen(None);
        }
    }

    fn choose(&mut self, dialog: &apricot_shell::ChoiceDialog) -> Option<usize> {
        crate::language::choose_from_list(
            self.controller.mtm(),
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

    fn open_equalizer(&mut self) {
        // SAFETY: `openEqualizer:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openEqualizer:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn copy_text(&mut self, text: &str) -> bool {
        let pasteboard = objc2_app_kit::NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        // SAFETY: `NSPasteboardTypeString` is AppKit's static string type.
        pasteboard.setString_forType(&NSString::from_str(text), unsafe {
            objc2_app_kit::NSPasteboardTypeString
        })
    }

    fn set_window_title(&mut self, title: &str) {
        if let Some(window) = self.controller.window() {
            window.setTitle(&NSString::from_str(title));
        }
    }

    fn focused_player_control(&self) -> Option<&'static str> {
        let responder = self.controller.window()?.firstResponder()?;
        let page = self.controller.ivars().page.borrow();
        page.as_ref()?.control_id(responder.as_ref())
    }

    fn focus_primary_control(&mut self) {
        let Some(window) = self.controller.window() else {
            return;
        };
        let player = self
            .controller
            .ivars()
            .page
            .borrow()
            .as_ref()
            .is_some_and(|page| !page.view().isHidden());
        if player {
            let page = self.controller.ivars().page.borrow();
            if let Some(page) = page.as_ref() {
                window.makeFirstResponder(Some(page.initial_focus()));
            }
        } else if let Some(list) = self.controller.list() {
            window.makeFirstResponder(Some(&list));
        }
    }

    fn video_mode(&self) -> MpvVideoMode {
        self.controller
            .ivars()
            .page
            .borrow()
            .as_ref()
            .map_or(MpvVideoMode::AudioOnly, |page| {
                MpvVideoMode::RenderApi(page.render_host().id())
            })
    }

    fn set_playback_polling(&mut self, active: bool) {
        if !active {
            self.controller.stop_poll_timer();
            return;
        }
        if self.controller.ivars().poll_timer.borrow().is_some() {
            return;
        }
        // SAFETY: the controller implements `pollPlayback:` and outlives
        // the timer, which is invalidated when polling stops.
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                PLAYBACK_POLL_SECONDS,
                self.controller.as_ref(),
                sel!(pollPlayback:),
                None,
                true,
            )
        };
        run_in_modal_loops(&timer);
        *self.controller.ivars().poll_timer.borrow_mut() = Some(timer);
    }

    fn schedule_key_repeat(&mut self, delay_ms: u32) {
        self.cancel_key_repeat();
        // SAFETY: `keyRepeatTick:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(keyRepeatTick:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: f64::from(delay_ms) / 1000.0
            ];
        }
    }

    fn cancel_key_repeat(&mut self) {
        // SAFETY: cancels the pending `keyRepeatTick:` of this controller.
        unsafe {
            let _: () = msg_send![
                objc2::class!(NSObject),
                cancelPreviousPerformRequestsWithTarget: self.controller,
                selector: sel!(keyRepeatTick:),
                object: Option::<&AnyObject>::None
            ];
        }
    }

    fn play_default_reached_sound(&mut self) {
        let played = default_reached_sound_path().is_some_and(|path| {
            NSSound::initWithContentsOfFile_byReference(
                <NSSound as objc2::AnyThread>::alloc(),
                &NSString::from_str(&path.to_string_lossy()),
                true,
            )
            .is_some_and(|sound| sound.play())
        });
        if !played {
            objc2_app_kit::NSBeep();
        }
    }

    fn choose_media_file(&mut self, title: &str) -> Result<Option<PathBuf>, String> {
        let mtm = self.controller.mtm();
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);
        let extensions: Vec<Retained<NSString>> = apricot_platform::MEDIA_EXTENSIONS
            .iter()
            .map(|extension| NSString::from_str(extension))
            .collect();
        let extensions = NSArray::from_retained_slice(&extensions);
        // SAFETY: `setAllowedFileTypes:` takes an array of extensions; it is
        // deprecated in favor of content types but keeps the Windows list.
        unsafe {
            let _: () = msg_send![&panel, setAllowedFileTypes: &*extensions];
        }
        if panel.runModal() != NSModalResponseOK {
            return Ok(None);
        }
        Ok(panel
            .URL()
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string())))
    }

    fn restore(&mut self) {
        self.controller.restore();
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
            let kind = event.r#type();
            if kind == NSEventType::KeyDown && !controller.is_null() {
                // SAFETY: the controller outlives the window; see `new`.
                let handled: bool =
                    unsafe { msg_send![&*controller, handleKeyEvent: event] };
                if handled {
                    return;
                }
            }
            if matches!(kind, NSEventType::KeyUp | NSEventType::FlagsChanged)
                && !controller.is_null()
            {
                // SAFETY: as above.
                unsafe {
                    let _: () = msg_send![&*controller, handleKeyRelease: event];
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
