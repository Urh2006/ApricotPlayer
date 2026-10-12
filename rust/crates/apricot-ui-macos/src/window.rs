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
    fields: RefCell<Option<crate::screen_fields::ScreenFields>>,
    background_timer: RefCell<Option<Retained<NSTimer>>>,
    scroll: RefCell<Option<Retained<NSScrollView>>>,
    status: RefCell<Option<Retained<NSTextField>>>,
    page: RefCell<Option<PlayerPage>>,
    poll_timer: RefCell<Option<Retained<NSTimer>>>,
    rss_timer: RefCell<Option<Retained<NSTimer>>>,
    subscription_timer: RefCell<Option<Retained<NSTimer>>>,
    app_update_timer: RefCell<Option<Retained<NSTimer>>>,
    /// Windows `tray_icon_added`: the menu bar item.
    status_item: RefCell<Option<crate::status_item::StatusItem>>,
    download_progress: RefCell<Option<Retained<crate::download_progress::ProgressWindow>>>,
    /// The key code of a held player key (Windows `ControlledRepeatState`).
    held_key: Cell<Option<u16>>,
    /// The modifiers held at the last key event, to see which one went up.
    held_modifiers: Cell<keys::Modifiers>,
    /// Windows `WindowLifecycle::Exiting`: Exit quits even with close to tray.
    exiting: Cell<bool>,
    /// The next step of the development self-test.
    #[cfg_attr(not(debug_assertions), allow(dead_code))] // Development self-tests.
    self_test_step: Cell<usize>,
    /// Activation requests waiting for the shell.
    activations: RefCell<Vec<apricot_app::ActivationRequest>>,
    /// The bookmarks window waiting to open after the current event.
    pending_bookmarks: RefCell<Option<apricot_shell::BookmarkDialog>>,
    /// The lyrics window waiting to open after the current event.
    pending_lyrics: RefCell<Option<apricot_shell::LyricsDialog>>,
    /// The playback queue window waiting to open after the current event.
    pending_queue: RefCell<Option<apricot_shell::QueueDialog>>,
    pending_spotify_dialog: RefCell<Option<apricot_shell::SpotifyListDialog>>,
    /// The transcript window waiting to open after the current event.
    pending_transcript: RefCell<Option<apricot_shell::TranscriptDialog>>,
    pending_converter: RefCell<Option<apricot_shell::ConverterDialog>>,
    pending_spotify_login: RefCell<Option<apricot_shell::SpotifyLoginTexts>>,
    pending_action_finder: RefCell<Option<apricot_app::ActionFinderModel>>,
    conversion_progress: RefCell<Option<Retained<crate::converter::ConversionProgress>>>,
    /// The comments window waiting to open after the current event.
    pending_comments: RefCell<Option<apricot_shell::CommentsDialog>>,
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
                // Windows `hide_to_tray` with `announce_tray_state`.
                self.with_shell(|shell, ui| shell.announce_background_state(ui));
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

        #[unsafe(method(openBookmarks:))]
        fn open_bookmarks(&self, _sender: Option<&AnyObject>) {
            let dialog = self.ivars().pending_bookmarks.borrow_mut().take();
            if let Some(dialog) = dialog {
                crate::bookmarks::run(self, dialog);
            }
        }

        #[unsafe(method(openLyrics:))]
        fn open_lyrics(&self, _sender: Option<&AnyObject>) {
            let dialog = self.ivars().pending_lyrics.borrow_mut().take();
            if let Some(dialog) = dialog {
                crate::lyrics::run(self, dialog);
            }
        }

        #[unsafe(method(openSpotifyDialog:))]
        fn open_spotify_dialog(&self, _sender: Option<&AnyObject>) {
            let dialog = self.ivars().pending_spotify_dialog.borrow_mut().take();
            if let Some(dialog) = dialog {
                crate::spotify_panels::run(self, &dialog);
            }
        }

        #[unsafe(method(openPlaybackQueue:))]
        fn open_playback_queue(&self, _sender: Option<&AnyObject>) {
            let dialog = self.ivars().pending_queue.borrow_mut().take();
            if let Some(dialog) = dialog {
                crate::queue::run(self, dialog);
            }
        }

        /// Shift+F10, the context menu key and VoiceOver's menu command on
        /// the list (Windows `WM_CONTEXTMENU`).
        #[unsafe(method(contextMenu:))]
        fn context_menu(&self, _sender: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.activate_action(ui, "context_menu"));
        }

        #[unsafe(method(openTranscript:))]
        fn open_transcript(&self, _sender: Option<&AnyObject>) {
            let dialog = self.ivars().pending_transcript.borrow_mut().take();
            if let Some(dialog) = dialog {
                crate::transcript::run(self, dialog);
            }
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            crate::settings::run(self);
        }

        #[unsafe(method(openActionFinder:))]
        fn open_action_finder(&self, _sender: Option<&AnyObject>) {
            let model = self.ivars().pending_action_finder.borrow_mut().take();
            if let Some(model) = model {
                crate::action_finder::run(self, model);
            }
        }

        #[unsafe(method(openSpotifyLogin:))]
        fn open_spotify_login(&self, _sender: Option<&AnyObject>) {
            let texts = self.ivars().pending_spotify_login.borrow_mut().take();
            if let Some(texts) = texts {
                crate::spotify_dialogs::run_login(self, &texts);
            }
        }

        #[unsafe(method(openConverter:))]
        fn open_converter(&self, _sender: Option<&AnyObject>) {
            let dialog = self.ivars().pending_converter.borrow_mut().take();
            if let Some(dialog) = dialog {
                crate::converter::run(self, dialog);
            }
        }

        #[unsafe(method(openComments:))]
        fn open_comments(&self, _sender: Option<&AnyObject>) {
            let dialog = self.ivars().pending_comments.borrow_mut().take();
            if let Some(dialog) = dialog {
                crate::comments::run(self, dialog);
            }
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
        /// Return in a screen's text field.
        #[unsafe(method(screenField:))]
        fn screen_field(&self, sender: Option<&AnyObject>) {
            if let Some(id) = self.field_id(sender) {
                self.with_shell(|shell, ui| shell.screen_text_submitted(ui, id));
            }
        }

        /// A screen's drop-down list changed (Windows `CBN_SELCHANGE`).
        #[unsafe(method(screenChoice:))]
        fn screen_choice(&self, sender: Option<&AnyObject>) {
            if let Some(id) = self.field_id(sender) {
                self.with_shell(|shell, ui| shell.screen_choice_changed(ui, id));
            }
        }

        /// The list's selected row changed (Windows `LBN_SELCHANGE`).
        #[unsafe(method(listSelectionChanged:))]
        fn list_selection_changed(&self, _sender: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.list_selection_changed(ui));
        }

        /// Command+A on the screen's list: a Spotify list marks every song.
        #[unsafe(method(listSelectAll:))]
        fn list_select_all(&self, _sender: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.activate_action(ui, "spotify_mark_all"));
        }

        /// End, or Down at the last row (Windows `rss_list_subclass_proc`).
        #[unsafe(method(listEndReached:))]
        fn list_end_reached(&self, _sender: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.list_end_reached(ui));
        }

        /// Windows `RSS_REFRESH_TIMER_ID`.
        /// Windows `APP_UPDATE_TIMER_ID`.
        #[unsafe(method(appUpdateTimer:))]
        fn app_update_timer(&self, _timer: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.app_update_timer_fired(ui));
        }

        /// Windows `YTDLP_STARTUP_TIMER_ID`.
        #[unsafe(method(startupYtdlpCheck:))]
        fn startup_ytdlp_check(&self, _sender: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.startup_ytdlp_update_check(ui));
        }

        /// Windows `APP_UPDATE_STARTUP_TIMER_ID`.
        #[unsafe(method(startupAppCheck:))]
        fn startup_app_check(&self, _sender: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.startup_app_update_check(ui));
        }

        /// Windows `SUBSCRIPTION_TIMER_ID`.
        #[unsafe(method(subscriptionTimer:))]
        fn subscription_timer(&self, _timer: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.check_subscriptions_if_due(ui));
        }

        #[unsafe(method(rssTimer:))]
        fn rss_timer(&self, _timer: Option<&AnyObject>) {
            self.with_shell(|shell, ui| shell.refresh_all_rss_feeds_background(ui));
        }

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
            fields: RefCell::new(None),
            background_timer: RefCell::new(None),
            scroll: RefCell::new(None),
            status: RefCell::new(None),
            page: RefCell::new(None),
            poll_timer: RefCell::new(None),
            rss_timer: RefCell::new(None),
            subscription_timer: RefCell::new(None),
            app_update_timer: RefCell::new(None),
            status_item: RefCell::new(None),
            download_progress: RefCell::new(None),
            held_key: Cell::new(None),
            held_modifiers: Cell::new(keys::Modifiers::default()),
            exiting: Cell::new(false),
            self_test_step: Cell::new(0),
            activations: RefCell::new(Vec::new()),
            pending_bookmarks: RefCell::new(None),
            pending_lyrics: RefCell::new(None),
            pending_queue: RefCell::new(None),
            pending_spotify_dialog: RefCell::new(None),
            pending_transcript: RefCell::new(None),
            pending_converter: RefCell::new(None),
            pending_spotify_login: RefCell::new(None),
            pending_action_finder: RefCell::new(None),
            conversion_progress: RefCell::new(None),
            pending_comments: RefCell::new(None),
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
        if keys::is_context_menu_key(event.keyCode(), modifiers) {
            self.with_shell(|shell, ui| shell.activate_action(ui, "context_menu"));
            return true;
        }
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

    /// The id of the screen field that sent an action.
    fn field_id(&self, sender: Option<&AnyObject>) -> Option<&'static str> {
        let tag = sender
            .and_then(|sender| sender.downcast_ref::<objc2_app_kit::NSControl>())
            .map(objc2_app_kit::NSControl::tag)?;
        self.ivars()
            .fields
            .borrow()
            .as_ref()
            .and_then(|fields| fields.id_for_tag(tag))
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
        list.set_context_menu_action(sel!(contextMenu:));
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

        list.set_selection_action(sel!(listSelectionChanged:));
        list.set_end_action(sel!(listEndReached:));
        list.set_select_all_action(sel!(listSelectAll:));
        let fields = crate::screen_fields::ScreenFields::new(
            mtm,
            self.as_ref(),
            sel!(screenField:),
            sel!(screenChoice:),
        );
        fields.view().setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
        );
        content.addSubview(fields.view());
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
        *self.ivars().fields.borrow_mut() = Some(fields);
        *self.ivars().scroll.borrow_mut() = Some(scroll);
        *self.ivars().status.borrow_mut() = Some(status);
        *self.ivars().page.borrow_mut() = Some(page);
        *self.ivars().window.borrow_mut() = Some(window.clone());

        // Python `setup_taskbar_icon`: the item stays for the whole session.
        *self.ivars().status_item.borrow_mut() = Some(crate::status_item::install(self));
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

    pub(crate) fn window(&self) -> Option<Retained<MainWindow>> {
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
        if let Some(fields) = self.ivars().fields.borrow().as_ref() {
            fields.set_hidden(player);
        }
        if let Some(page) = self.ivars().page.borrow().as_ref() {
            page.set_visible(player);
        }
    }

    /// Windows `layout_controls_state` for a list screen: the buttons above
    /// the status line, the list above them.
    fn layout_list_screen(&self, button_count: usize, field_count: usize) {
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
        let fields_height = crate::screen_fields::ScreenFields::height_for(field_count);
        if let Some(fields) = self.ivars().fields.borrow().as_ref() {
            fields.view().setFrame(NSRect::new(
                NSPoint::new(MARGIN, bounds.size.height - MARGIN - fields_height),
                NSSize::new(width, fields_height),
            ));
        }
        let list_top = bounds.size.height - MARGIN - fields_height;
        scroll.setFrame(NSRect::new(
            NSPoint::new(MARGIN, list_bottom),
            NSSize::new(width, (list_top - list_bottom).max(60.0)),
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

    /// Development builds with `APRICOT_MACOS_SELF_TEST=search`: the search
    /// screen, a real yt-dlp search for `APRICOT_MACOS_SELF_TEST_QUERY`,
    /// playing the second result and going back.
    #[cfg(debug_assertions)]
    pub fn run_search_self_test(&self) {
        self.ivars().self_test_step.set(300);
        // SAFETY: `playerSelfTestStep:` exists on this controller.
        unsafe {
            let _: () = msg_send![
                self,
                performSelector: sel!(playerSelfTestStep:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 1.0_f64
            ];
        }
    }

    /// Development builds with `APRICOT_MACOS_SELF_TEST=trending`: the
    /// Trending chart, playing from it and back, then the direct link
    /// `APRICOT_MACOS_SELF_TEST_QUERY`.
    #[cfg(debug_assertions)]
    pub fn run_podcast_self_test(&self) {
        self.ivars().self_test_step.set(500);
        // SAFETY: `playerSelfTestStep:` exists on this controller.
        unsafe {
            let _: () = msg_send![
                self,
                performSelector: sel!(playerSelfTestStep:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 1.0_f64
            ];
        }
    }

    /// Development builds with `APRICOT_MACOS_SELF_TEST=trending`.
    #[cfg(debug_assertions)]
    pub fn run_trending_self_test(&self) {
        self.ivars().self_test_step.set(400);
        // SAFETY: `playerSelfTestStep:` exists on this controller.
        unsafe {
            let _: () = msg_send![
                self,
                performSelector: sel!(playerSelfTestStep:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 1.0_f64
            ];
        }
    }

    #[cfg(debug_assertions)]
    fn trending_self_test_step(&self, index: usize) -> bool {
        use crate::self_test::{Key, focus, focused_value, key, send};
        use objc2_app_kit::NSEventModifierFlags as Flags;
        let Some(window) = self.window() else {
            return false;
        };
        let link = std::env::var("APRICOT_MACOS_SELF_TEST_QUERY")
            .unwrap_or_else(|_| "youtu.be/jfKfPfyJRdk".to_owned());
        let link: &'static str = Box::leak(link.into_boxed_str());
        let escape = || Some(key(keys::KEY_ESCAPE, "\u{1b}"));
        // Without a YouTube Data API key the default category has no public
        // chart: the message is answered with Return and the main menu shows.
        let mut steps: Vec<(String, Option<Key>)> = vec![
            ("open Trending".to_owned(), None),
            ("after the message".to_owned(), None),
        ];
        steps.push((
            "Command+Option+L (direct link)".to_owned(),
            Some(Key {
                code: 37,
                text: "l",
                flags: Flags::Command | Flags::Option,
            }),
        ));
        for (offset, character) in link.char_indices() {
            let text = &link[offset..offset + character.len_utf8()];
            steps.push((format!("type {text:?}"), Some(key(0, text))));
        }
        steps.push((
            "Return (play)".to_owned(),
            Some(key(keys::KEY_RETURN, "\r")),
        ));
        for _ in 0..10 {
            steps.push(("waiting for playback".to_owned(), None));
        }
        if std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref() == Ok("extras") {
            steps.extend(crate::self_test::youtube_extras_steps());
        }
        steps.push(("Escape (direct link)".to_owned(), escape()));
        steps.push(("Tab".to_owned(), Some(key(keys::KEY_TAB, "\t"))));
        steps.push(("Escape (main menu)".to_owned(), escape()));
        let Some((name, key)) = steps.get(index) else {
            return false;
        };
        if index == 0 {
            self.schedule_modal_self_test_keys();
            self.run_action("open_trending");
        }
        if name.starts_with("Command+Shift+T") {
            self.schedule_modal_self_test_keys_after(9.0);
        }
        if name.starts_with("Command+Shift+M") {
            self.schedule_modal_self_test_keys_after(11.0);
        }
        if name.starts_with("Return (play)") || name.starts_with("waiting for playback") {
            // A failed playback shows a message; it is printed and answered.
            self.schedule_modal_self_test_keys();
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
        let row = self.list().and_then(|list| list.selected());
        let quiet =
            name.starts_with("type ") || (name.starts_with("waiting") && !index.is_multiple_of(4));
        if !quiet {
            println!(
                "trending-test {name}: focus={} value={:?} row={row:?} title={:?} status={status:?} frames={}",
                focus(&window),
                focused_value(&window),
                window.title().to_string(),
                crate::video::frames_drawn()
            );
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        true
    }

    #[cfg(debug_assertions)]
    fn converter_self_test_step(&self, index: usize) -> bool {
        use crate::self_test::focus;
        let Some(window) = self.window() else {
            return false;
        };
        if index > 48 {
            return false;
        }
        let name = match index {
            0 => {
                self.schedule_modal_self_test_keys_after(1.0);
                self.run_action("file_converter");
                "open the file converter"
            }
            18 => {
                self.schedule_modal_self_test_keys_after(1.0);
                self.run_action("folder_converter");
                "open the folder converter"
            }
            37 => {
                if let Some(clip) = std::env::var_os("APRICOT_MACOS_SELF_TEST_CLIP") {
                    self.open_file(clip.into());
                }
                "open the clip source"
            }
            39 => {
                self.run_action("player_marker_start");
                "clip start marker"
            }
            41 => {
                self.run_action("player_marker_end");
                "clip end marker"
            }
            42 => {
                self.run_action("download_audio");
                "export the clip as audio"
            }
            _ => {
                // A message at the end is printed and answered.
                self.schedule_modal_self_test_keys_after(0.5);
                "waiting for the conversion"
            }
        };
        let status = self
            .ivars()
            .status
            .borrow()
            .as_ref()
            .map(|status| status.stringValue().to_string())
            .unwrap_or_default();
        let windows: Vec<String> = NSApplication::sharedApplication(self.mtm())
            .windows()
            .iter()
            .filter(|window| window.isVisible())
            .map(|window| window.title().to_string())
            .collect();
        println!(
            "converter-test {name}: focus={} windows={windows:?} status={status:?}",
            focus(&window)
        );
        let _ = std::io::Write::flush(&mut std::io::stdout());
        true
    }

    #[cfg(debug_assertions)]
    #[allow(clippy::too_many_lines)] // The development self-test plans.
    fn settings_self_test_step(&self, index: usize) -> bool {
        use crate::self_test::focus;
        let Some(window) = self.window() else {
            return false;
        };
        let name = match index {
            0 if std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref() == Ok("spotify") => {
                self.run_action("open_spotify");
                "open the Spotify hub"
            }
            1 if std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref() == Ok("spotify") => {
                self.schedule_modal_self_test_keys_after(1.0);
                self.run_action("open_selected");
                "Return on the first hub row"
            }
            0 if std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref()
                == Ok("spotify_queue") =>
            {
                self.schedule_modal_self_test_keys_after(1.0);
                self.with_shell(|shell, ui| shell.spotify_queue_dialog_self_test(ui));
                "Spotify queue dialog"
            }
            index if std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref() == Ok("updates") => {
                match index {
                    0 => {
                        self.with_shell(|shell, ui| shell.manual_ytdlp_update_check(ui));
                        "Check yt-dlp updates"
                    }
                    41 => {
                        self.with_shell(|shell, ui| {
                            shell.start_app_update_check(ui, true, true, false);
                        });
                        "Check app updates"
                    }
                    1..=40 | 42..=50 => "waiting",
                    _ => return false,
                }
            }
            index
                if std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref() == Ok("background") =>
            {
                let status_menu = |entry: Option<isize>| {
                    self.ivars()
                        .status_item
                        .borrow()
                        .as_ref()
                        .map(|item| item.self_test(entry))
                        .unwrap_or_default()
                };
                match index {
                    0 => {
                        crate::self_test::send(
                            &window,
                            &crate::self_test::Key {
                                code: 13,
                                text: "w",
                                flags: objc2_app_kit::NSEventModifierFlags::Command,
                            },
                        );
                        "Command+W"
                    }
                    1 => {
                        println!(
                            "settings-test window visible={} menu={:?}",
                            window.isVisible(),
                            status_menu(None)
                        );
                        "menu bar item"
                    }
                    2 => {
                        status_menu(Some(0));
                        "Show from the menu bar"
                    }
                    3 => {
                        println!("settings-test window visible={}", window.isVisible());
                        "shown"
                    }
                    _ => return false,
                }
            }
            index
                if std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref()
                    == Ok("subscriptions") =>
            {
                match index {
                    0 => {
                        self.run_action("open_subscriptions");
                        "open the subscriptions"
                    }
                    1 => {
                        self.with_shell(|shell, ui| {
                            shell.activate_screen_button(ui, "subscription_check");
                        });
                        "Check now"
                    }
                    2..=40 => "waiting",
                    41 => {
                        self.run_action("new_subscription_videos");
                        "open the notification center"
                    }
                    _ => return false,
                }
            }
            0 if std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref() == Ok("audiovault") => {
                self.schedule_modal_self_test_keys_after(1.0);
                self.run_action("open_audiovault");
                "open AudioVault"
            }
            0 => {
                self.schedule_modal_self_test_keys_after(1.0);
                self.run_action("open_action_finder");
                "open the Action Finder"
            }
            3 => {
                self.schedule_modal_self_test_keys_after(1.5);
                self.run_action("open_settings");
                "open the settings"
            }
            1 | 2 | 4..=8 => "waiting",
            _ => return false,
        };
        let status = self
            .ivars()
            .status
            .borrow()
            .as_ref()
            .map(|status| status.stringValue().to_string())
            .unwrap_or_default();
        println!(
            "settings-test {name}: focus={} status={status:?}",
            focus(&window)
        );
        let _ = std::io::Write::flush(&mut std::io::stdout());
        true
    }

    #[cfg(debug_assertions)]
    pub fn run_settings_self_test(&self) {
        self.ivars().self_test_step.set(700);
        // SAFETY: `playerSelfTestStep:` exists on this controller.
        unsafe {
            let _: () = msg_send![
                self,
                performSelector: sel!(playerSelfTestStep:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 1.0_f64
            ];
        }
    }

    #[cfg(debug_assertions)]
    pub fn run_converter_self_test(&self) {
        self.ivars().self_test_step.set(600);
        // SAFETY: `playerSelfTestStep:` exists on this controller.
        unsafe {
            let _: () = msg_send![
                self,
                performSelector: sel!(playerSelfTestStep:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 1.0_f64
            ];
        }
    }

    #[cfg(debug_assertions)]
    fn podcast_self_test_step(&self, index: usize) -> bool {
        use crate::self_test::{focus, focused_value, send};
        let Some(window) = self.window() else {
            return false;
        };
        let steps = crate::self_test::podcast_steps();
        let Some((name, key)) = steps.get(index) else {
            return false;
        };
        if index == 0 {
            self.run_action("open_podcasts_rss");
        }
        if name.starts_with("Add feed") {
            self.schedule_modal_self_test_keys();
            self.with_shell(|shell, ui| shell.activate_screen_button(ui, "rss_add"));
        }
        if name.starts_with("waiting") || name.starts_with("Return") {
            // A failure shows a message; it is printed and answered.
            self.schedule_modal_self_test_keys_after(2.0);
        }
        if name.starts_with("Command+P") {
            self.schedule_modal_self_test_keys();
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
        let row = self.list().and_then(|list| list.selected());
        let count = self.list().map_or(0, |list| list.numberOfRows());
        if !name.starts_with("waiting") || index.is_multiple_of(4) {
            let value: String = focused_value(&window).chars().take(160).collect();
            let app = NSApplication::sharedApplication(self.mtm());
            let key_window = app
                .keyWindow()
                .map(|key| key.title().to_string())
                .unwrap_or_default();
            let visible: Vec<String> = app
                .windows()
                .iter()
                .filter(|window| window.isVisible())
                .map(|window| {
                    let texts = window
                        .contentView()
                        .map(|view| crate::self_test::texts(&view))
                        .unwrap_or_default();
                    format!("{} {texts:?}", window.title())
                })
                .collect();
            let key_window = format!("{key_window} active={} windows={visible:?}", app.isActive());
            println!(
                "podcast-test {name}: focus={} value={value:?} row={row:?} rows={count} title={:?} key={key_window:?} status={status:?}",
                focus(&window),
                window.title().to_string(),
            );
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        true
    }

    #[cfg(debug_assertions)]
    fn search_self_test_step(&self, index: usize) -> bool {
        use crate::self_test::{Key, focus, focused_value, key, send};
        use objc2_app_kit::NSEventModifierFlags as Flags;
        let Some(window) = self.window() else {
            return false;
        };
        let query =
            std::env::var("APRICOT_MACOS_SELF_TEST_QUERY").unwrap_or_else(|_| "apricot".to_owned());
        let query: &'static str = Box::leak(query.into_boxed_str());
        let mut steps: Vec<(String, Option<Key>)> = vec![
            ("opened".to_owned(), None),
            (
                "Command+Option+Y (search)".to_owned(),
                Some(Key {
                    code: 16,
                    text: "y",
                    flags: Flags::Command | Flags::Option,
                }),
            ),
        ];
        for (offset, _) in query.char_indices() {
            let text =
                &query[offset..offset + query[offset..].chars().next().map_or(1, char::len_utf8)];
            steps.push((format!("type {text:?}"), Some(key(0, text))));
        }
        let tab = || Some(key(keys::KEY_TAB, "\t"));
        let down = || Some(key(keys::KEY_DOWN, "\u{f701}"));
        let enter = || Some(key(keys::KEY_RETURN, "\r"));
        let playlists = std::env::var("APRICOT_MACOS_SELF_TEST_KIND").as_deref() == Ok("playlist");
        if playlists {
            // Type: All, Video, Playlist; then the Search button.
            steps.push(("Tab (provider)".to_owned(), tab()));
            steps.push(("Tab (type)".to_owned(), tab()));
            steps.push(("Down (video)".to_owned(), down()));
            steps.push(("Down (playlist)".to_owned(), down()));
            steps.push(("Tab (Search button)".to_owned(), tab()));
            steps.push(("Return (search)".to_owned(), enter()));
        } else {
            steps.push(("Return (search)".to_owned(), enter()));
        }
        for _ in 0..14 {
            steps.push(("waiting for results".to_owned(), None));
        }
        if playlists {
            steps.push(("Return (open playlist)".to_owned(), enter()));
            for _ in 0..12 {
                steps.push(("waiting for the playlist".to_owned(), None));
            }
        }
        steps.push(("Down".to_owned(), down()));
        steps.push(("Return (play)".to_owned(), enter()));
        for _ in 0..12 {
            steps.push(("waiting for playback".to_owned(), None));
        }
        steps.push((
            "Escape (list)".to_owned(),
            Some(key(keys::KEY_ESCAPE, "\u{1b}")),
        ));
        if playlists {
            steps.push((
                "Escape (results)".to_owned(),
                Some(key(keys::KEY_ESCAPE, "\u{1b}")),
            ));
        }
        steps.push((
            "Shift+Tab".to_owned(),
            Some(Key {
                code: keys::KEY_TAB,
                text: "\t",
                flags: Flags::Shift,
            }),
        ));
        steps.push((
            "Escape (main menu)".to_owned(),
            Some(key(keys::KEY_ESCAPE, "\u{1b}")),
        ));
        let Some((name, key)) = steps.get(index) else {
            return false;
        };
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
        let row = self.list().and_then(|list| list.selected());
        println!(
            "search-test {name}: focus={} value={:?} row={row:?} title={:?} status={status:?} frames={}",
            focus(&window),
            focused_value(&window),
            window.title().to_string(),
            crate::video::frames_drawn()
        );
        true
    }

    #[cfg(debug_assertions)]
    fn tools_self_test_step(&self, index: usize) -> bool {
        use crate::self_test::{focus, send};
        let Some(window) = self.window() else {
            return false;
        };
        let steps = crate::self_test::tools_steps();
        let Some((name, key, modal)) = steps.get(index) else {
            return false;
        };
        if *modal {
            self.schedule_modal_self_test_keys();
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

    /// The next modal window is answered from the modal run loop.
    #[cfg(debug_assertions)]
    fn schedule_modal_self_test_keys(&self) {
        self.schedule_modal_self_test_keys_after(0.6);
    }

    /// [`Self::schedule_modal_self_test_keys`] after `seconds`.
    #[cfg(debug_assertions)]
    fn schedule_modal_self_test_keys_after(&self, seconds: f64) {
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
                afterDelay: seconds,
                inModes: &*modes
            ];
        }
    }

    /// Answers the modal window a tools step opened: the chapter list,
    /// the bookmark name prompt or the bookmarks window.
    #[cfg(debug_assertions)]
    #[allow(clippy::too_many_lines)] // One plan per scripted window.
    fn modal_self_test_keys(&self) {
        use crate::self_test::{Key, focus, key, send};
        let Some(window) = NSApplication::sharedApplication(self.mtm()).modalWindow() else {
            println!("tools-test modal: no modal window");
            return;
        };
        if let Some(alert) = window
            .contentView()
            .map(|view| crate::self_test::texts(&view))
        {
            println!("tools-test modal texts: {alert:?}");
        }
        let print = |name: &str| {
            let row = window
                .firstResponder()
                .and_then(|responder| Retained::downcast::<List>(responder).ok())
                .and_then(|list| list.selected_text())
                .unwrap_or_default();
            let row: String = row.chars().take(120).collect();
            println!(
                "tools-test   {name}: window={:?} focus={} row={row:?}",
                window.title().to_string(),
                focus(&window)
            );
            let _ = std::io::Write::flush(&mut std::io::stdout());
        };
        let escape = key(keys::KEY_ESCAPE, "\u{1b}");
        let plan: Vec<(&str, Key)> = match window.title().to_string().as_str() {
            "Chapters" => vec![
                ("Down", key(keys::KEY_DOWN, "\u{f701}")),
                ("Return", key(keys::KEY_RETURN, "\r")),
            ],
            "Transcript" => vec![
                ("Tab (list)", key(keys::KEY_TAB, "\t")),
                ("Down", key(keys::KEY_DOWN, "\u{f701}")),
                ("Down", key(keys::KEY_DOWN, "\u{f701}")),
                ("Return (play the line)", key(keys::KEY_RETURN, "\r")),
                ("Escape", escape),
            ],
            "Comments" => vec![
                ("Tab (sort)", key(keys::KEY_TAB, "\t")),
                ("Tab (list)", key(keys::KEY_TAB, "\t")),
                ("Down", key(keys::KEY_DOWN, "\u{f701}")),
                ("Tab (Open)", key(keys::KEY_TAB, "\t")),
                ("Escape", escape),
            ],
            "Lyrics" => vec![
                ("Tab", key(keys::KEY_TAB, "\t")),
                ("Return (copy)", key(keys::KEY_RETURN, "\r")),
                ("Escape", escape),
            ],
            "Playback queue" => vec![
                ("Down", key(keys::KEY_DOWN, "\u{f701}")),
                ("Tab", key(keys::KEY_TAB, "\t")),
                ("Tab", key(keys::KEY_TAB, "\t")),
                ("Return (move up)", key(keys::KEY_RETURN, "\r")),
                ("Escape", escape),
            ],
            "Action finder" => crate::self_test::action_finder_plan(),
            "Spotify login" => vec![
                ("Tab (Open browser)", key(keys::KEY_TAB, "\t")),
                ("Tab (Copy link)", key(keys::KEY_TAB, "\t")),
                (
                    "Tab (Cancel, Try again is disabled)",
                    key(keys::KEY_TAB, "\t"),
                ),
                ("Tab (status)", key(keys::KEY_TAB, "\t")),
                ("Escape", escape),
            ],
            "Log in to AudioVault" => vec![
                ("Tab (password)", key(keys::KEY_TAB, "\t")),
                ("Tab (Register)", key(keys::KEY_TAB, "\t")),
                ("Tab (OK)", key(keys::KEY_TAB, "\t")),
                ("Escape", escape),
            ],
            "Settings" => crate::self_test::settings_plan(),
            "Spotify queue" => crate::self_test::spotify_queue_plan(),
            title @ ("File converter" | "Folder converter") => {
                crate::self_test::converter_plan(title == "File converter")
            }
            "Add feed" => {
                let url = std::env::var("APRICOT_MACOS_SELF_TEST_QUERY")
                    .unwrap_or_else(|_| "feeds.npr.org/510289/podcast.xml".to_owned());
                let url: &'static str = Box::leak(url.into_boxed_str());
                let mut plan: Vec<(&str, Key)> = url
                    .char_indices()
                    .map(|(offset, character)| {
                        ("type", key(0, &url[offset..offset + character.len_utf8()]))
                    })
                    .collect();
                plan.push(("Return", key(keys::KEY_RETURN, "\r")));
                plan
            }
            "Create playlist" => vec![
                ("type M", key(0, "M")),
                ("type i", key(0, "i")),
                ("type x", key(0, "x")),
                ("Return", key(keys::KEY_RETURN, "\r")),
            ],
            "Bookmarks" => vec![
                ("Down", key(keys::KEY_DOWN, "\u{f701}")),
                ("Tab", key(keys::KEY_TAB, "\t")),
                ("Tab", key(keys::KEY_TAB, "\t")),
                ("Escape", escape),
            ],
            // The first name prompt is accepted, the second cancelled.
            _ if self.ivars().self_test_step.get() == 231
                || self.ivars().self_test_step.get() >= 400 =>
            {
                vec![("Return", key(keys::KEY_RETURN, "\r"))]
            }
            _ => vec![("Escape", escape)],
        };
        print("opened");
        for (name, key) in plan {
            if name.ends_with("(capture)") {
                // A conflict shows a message, answered from its own loop.
                self.schedule_modal_self_test_keys_after(1.0);
            }
            send(&window, &key);
            print(name);
        }
    }

    #[cfg(debug_assertions)]
    fn folder_self_test_step(&self, index: usize) -> bool {
        use crate::self_test::{focus, send};
        let Some(window) = self.window() else {
            return false;
        };
        let steps = crate::self_test::folder_steps();
        let Some((name, key)) = steps.get(index) else {
            return false;
        };
        if name.starts_with("Command+Option+Q") || name.starts_with("Command+P (add to a new") {
            self.schedule_modal_self_test_keys();
        }
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
    #[allow(clippy::too_many_lines)] // The scripted player keys.
    fn player_self_test_step(&self) {
        use crate::self_test::{Key, focus, key, send};
        let step = self.ivars().self_test_step.get();
        if step >= 100 {
            let more = if step >= 700 {
                self.settings_self_test_step(step - 700)
            } else if step >= 600 {
                self.converter_self_test_step(step - 600)
            } else if step >= 500 {
                self.podcast_self_test_step(step - 500)
            } else if step >= 400 {
                self.trending_self_test_step(step - 400)
            } else if step >= 300 {
                self.search_self_test_step(step - 300)
            } else if step >= 200 {
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

/// Python `DEFAULT_REACHED_SOUND`: `assets/default_reached.wav` in the
/// bundle's `Contents/Resources` (data may not sit beside signed code), next
/// to the executable, and in a development build the repository's copy.
fn default_reached_sound_path() -> Option<PathBuf> {
    let executable_directory = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let bundled = [
        executable_directory.join("../Resources/assets"),
        executable_directory.join("assets"),
    ]
    .into_iter()
    .map(|directory| directory.join("default_reached.wav"))
    .find(|path| path.is_file());
    if bundled.is_some() {
        return bundled;
    }
    let repository =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../assets/default_reached.wav");
    (cfg!(debug_assertions) && repository.is_file()).then_some(repository)
}

/// Windows timers keep firing while a dialog runs its modal loop; AppKit
/// timers only do so in the common run loop modes.
/// Limits an open or save panel to `extensions`.
fn set_allowed_extensions(panel: &objc2_app_kit::NSSavePanel, extensions: &[&str]) {
    let extensions: Vec<Retained<NSString>> = extensions
        .iter()
        .map(|extension| NSString::from_str(extension))
        .collect();
    let extensions = NSArray::from_retained_slice(&extensions);
    // SAFETY: `setAllowedFileTypes:` takes an array of extensions; it is
    // deprecated in favor of content types but keeps the Windows filter.
    unsafe {
        let _: () = msg_send![panel, setAllowedFileTypes: &*extensions];
    }
}

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
        use crate::screen_fields::{FieldControl, FieldSpec};
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
                _ => None,
            })
            .collect();
        let fields: Vec<FieldSpec<'_>> = screen
            .controls
            .iter()
            .filter_map(|control| match control {
                ScreenControl::TextField { id, label, value } => Some(FieldSpec::Text {
                    id,
                    label,
                    value: value.as_deref(),
                }),
                ScreenControl::Choice {
                    id,
                    label,
                    options,
                    selected,
                } => Some(FieldSpec::Choice {
                    id,
                    label,
                    options,
                    selected: *selected,
                }),
                _ => None,
            })
            .collect();
        self.controller
            .layout_list_screen(buttons.len(), fields.len());
        // A screen without a list (the direct link) hides it, as Windows
        // `set_view_visibility` does.
        let has_list = screen.controls.contains(&ScreenControl::List);
        if let Some(scroll) = self.controller.ivars().scroll.borrow().as_ref() {
            scroll.setHidden(!has_list);
        }
        if let Some(screen_fields) = self.controller.ivars().fields.borrow_mut().as_mut() {
            screen_fields.sync(&fields);
        }
        if let Some(screen_buttons) = self.controller.ivars().buttons.borrow_mut().as_mut() {
            screen_buttons.sync(&buttons);
        }
        {
            let screen_buttons = self.controller.ivars().buttons.borrow();
            let screen_fields = self.controller.ivars().fields.borrow();
            let views: Vec<&NSView> = screen
                .controls
                .iter()
                .filter_map(|control| match control {
                    ScreenControl::List => Some(&**list as &NSView),
                    ScreenControl::Button { id, .. } => screen_buttons
                        .as_ref()
                        .and_then(|buttons| buttons.button(id))
                        .map(|button| button as &NSView),
                    ScreenControl::TextField { id, .. } | ScreenControl::Choice { id, .. } => {
                        screen_fields
                            .as_ref()
                            .and_then(|fields| fields.control(id))
                            .map(FieldControl::view)
                    }
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

    fn screen_text(&self, id: &str) -> String {
        self.controller
            .ivars()
            .fields
            .borrow()
            .as_ref()
            .map(|fields| fields.text(id))
            .unwrap_or_default()
    }

    fn screen_choice(&self, id: &str) -> Option<usize> {
        self.controller
            .ivars()
            .fields
            .borrow()
            .as_ref()
            .and_then(|fields| fields.choice(id))
    }

    fn set_screen_choices(&mut self, id: &str, options: &[String], selected: usize) {
        if let Some(fields) = self.controller.ivars().fields.borrow().as_ref() {
            fields.set_choices(id, options, selected);
        }
    }

    fn focus_screen_control(&mut self, id: &str) {
        let Some(window) = self.controller.window() else {
            return;
        };
        let fields = self.controller.ivars().fields.borrow();
        if let Some(control) = fields.as_ref().and_then(|fields| fields.control(id)) {
            window.makeFirstResponder(Some(control.view()));
        }
    }

    fn set_list_rows(&mut self, rows: Vec<String>, selected: usize) {
        if let Some(list) = self.controller.list() {
            list.set_rows(rows, Some(selected));
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

    fn choose_open_file(&mut self, title: &str, extensions: &[&str]) -> Option<PathBuf> {
        let panel = NSOpenPanel::openPanel(self.controller.mtm());
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);
        set_allowed_extensions(&panel, extensions);
        if panel.runModal() != NSModalResponseOK {
            return None;
        }
        panel
            .URL()
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
    }

    fn choose_import_source(&mut self, title: &str) -> Option<PathBuf> {
        let panel = NSOpenPanel::openPanel(self.controller.mtm());
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(true);
        panel.setAllowsMultipleSelection(false);
        set_allowed_extensions(&panel, &["zip"]);
        if panel.runModal() != NSModalResponseOK {
            return None;
        }
        panel
            .URL()
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
    }

    fn choose_save_file(
        &mut self,
        title: &str,
        directory: Option<&std::path::Path>,
        default_name: &str,
        extensions: &[&str],
    ) -> Option<PathBuf> {
        let panel = objc2_app_kit::NSSavePanel::savePanel(self.controller.mtm());
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setNameFieldStringValue(&NSString::from_str(default_name));
        if let Some(directory) = directory {
            panel.setDirectoryURL(Some(&objc2_foundation::NSURL::fileURLWithPath(
                &NSString::from_str(&directory.to_string_lossy()),
            )));
        }
        set_allowed_extensions(&panel, extensions);
        if panel.runModal() != NSModalResponseOK {
            return None;
        }
        panel
            .URL()
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
    }

    fn choose_download_folder(
        &mut self,
        title: &str,
        initial: &std::path::Path,
    ) -> Option<PathBuf> {
        let panel = NSOpenPanel::openPanel(self.controller.mtm());
        panel.setTitle(Some(&NSString::from_str(title)));
        panel.setMessage(Some(&NSString::from_str(title)));
        panel.setCanChooseFiles(false);
        panel.setCanChooseDirectories(true);
        panel.setCanCreateDirectories(true);
        panel.setAllowsMultipleSelection(false);
        panel.setDirectoryURL(Some(&objc2_foundation::NSURL::fileURLWithPath(
            &NSString::from_str(&initial.to_string_lossy()),
        )));
        if panel.runModal() != NSModalResponseOK {
            return None;
        }
        panel
            .URL()
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
    }

    fn ask_yes_no(&mut self, message: &str, yes: &str, no: &str) -> bool {
        let window = self.controller.window();
        let previous = window.as_ref().and_then(|window| window.firstResponder());
        let alert = NSAlert::new(self.controller.mtm());
        alert.setMessageText(&NSString::from_str(message));
        alert.addButtonWithTitle(&NSString::from_str(yes));
        alert.addButtonWithTitle(&NSString::from_str(no));
        let answer = alert.runModal();
        if let (Some(window), Some(previous)) = (window, previous) {
            window.makeFirstResponder(Some(&previous));
        }
        answer == objc2_app_kit::NSAlertFirstButtonReturn
    }

    fn show_download_progress(&mut self, progress: &apricot_shell::DownloadProgress) {
        let existing = self.controller.ivars().download_progress.borrow().clone();
        if let Some(window) = existing {
            window.set_progress(progress.percent, &progress.message);
            window.show();
            return;
        }
        let window = crate::download_progress::ProgressWindow::new(self.controller, progress);
        *self.controller.ivars().download_progress.borrow_mut() = Some(window);
    }

    fn update_download_progress(&mut self, percent: u32, message: &str) {
        if let Some(window) = self.controller.ivars().download_progress.borrow().as_ref() {
            window.set_progress(percent, message);
        }
    }

    fn close_download_progress(&mut self) {
        let window = self
            .controller
            .ivars()
            .download_progress
            .borrow_mut()
            .take();
        if let Some(window) = window {
            window.destroy();
        }
    }

    fn audiovault_login(
        &mut self,
        login: &apricot_shell::AudiovaultLogin,
    ) -> Option<(String, String)> {
        crate::audiovault_login::show(self.controller.mtm(), login)
    }

    fn focus_list(&mut self) {
        if let (Some(window), Some(list)) = (self.controller.window(), self.controller.list()) {
            let focused = window.firstResponder().is_some_and(|responder| {
                std::ptr::eq(
                    Retained::as_ptr(&responder).cast::<u8>(),
                    Retained::as_ptr(&list).cast::<u8>(),
                )
            });
            if !focused {
                window.makeFirstResponder(Some(&list));
            }
        }
    }

    fn settings_cookie_source(&mut self, path: &str, browser_index: usize) {
        crate::settings::cookie_source_exported(path, browser_index);
    }

    fn open_spotify_login(&mut self, texts: &apricot_shell::SpotifyLoginTexts) {
        *self.controller.ivars().pending_spotify_login.borrow_mut() = Some(texts.clone());
        // SAFETY: `openSpotifyLogin:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openSpotifyLogin:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn spotify_login_update(&mut self, update: &apricot_shell::SpotifyLoginUpdate) {
        crate::spotify_dialogs::login_update(update);
    }

    fn spotify_settings(
        &mut self,
        texts: &apricot_shell::SpotifySettingsTexts,
        current: apricot_spotify::settings::SpotifySettings,
    ) -> Option<apricot_spotify::settings::SpotifySettings> {
        crate::spotify_dialogs::run_settings(self.controller.mtm(), texts, current)
    }

    fn open_spotify_dialog(&mut self, dialog: apricot_shell::SpotifyListDialog) {
        *self.controller.ivars().pending_spotify_dialog.borrow_mut() = Some(dialog);
        // SAFETY: `openSpotifyDialog:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openSpotifyDialog:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn spotify_dialog_update(
        &mut self,
        rows: Vec<apricot_shell::SpotifyDialogRow>,
        selected: Option<usize>,
    ) {
        crate::spotify_panels::update(rows, selected);
    }

    fn open_settings(&mut self) {
        // SAFETY: `openSettings:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openSettings:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn open_action_finder(&mut self, model: apricot_app::ActionFinderModel) {
        *self.controller.ivars().pending_action_finder.borrow_mut() = Some(model);
        // SAFETY: `openActionFinder:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openActionFinder:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn open_converter(&mut self, dialog: apricot_shell::ConverterDialog) {
        *self.controller.ivars().pending_converter.borrow_mut() = Some(dialog);
        // SAFETY: `openConverter:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openConverter:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn show_conversion_progress(&mut self, title: &str, message: &str, total: usize) {
        let progress =
            crate::converter::ConversionProgress::new(self.controller.mtm(), title, message, total);
        let previous = self
            .controller
            .ivars()
            .conversion_progress
            .borrow_mut()
            .replace(progress);
        if let Some(previous) = previous {
            previous.destroy();
        }
    }

    fn update_conversion_progress(&mut self, converted: usize, message: &str) {
        if let Some(progress) = self
            .controller
            .ivars()
            .conversion_progress
            .borrow()
            .as_ref()
        {
            progress.update(converted, message);
        }
    }

    fn close_conversion_progress(&mut self) {
        let progress = self
            .controller
            .ivars()
            .conversion_progress
            .borrow_mut()
            .take();
        if let Some(progress) = progress {
            progress.destroy();
        }
    }

    fn set_list_row(&mut self, index: usize, label: &str) {
        if let Some(list) = self.controller.list() {
            list.set_row(index, label);
        }
    }

    fn app_is_active(&self) -> bool {
        NSApplication::sharedApplication(self.controller.mtm()).isActive()
    }

    fn set_rss_timer(&mut self, interval_seconds: Option<f64>) {
        let mut timer = self.controller.ivars().rss_timer.borrow_mut();
        if let Some(timer) = timer.take() {
            timer.invalidate();
        }
        let Some(interval) = interval_seconds else {
            return;
        };
        // SAFETY: the controller implements `rssTimer:` and outlives the
        // timer, which is invalidated when it is replaced.
        let rss = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                interval,
                self.controller.as_ref(),
                sel!(rssTimer:),
                None,
                true,
            )
        };
        run_in_modal_loops(&rss);
        *timer = Some(rss);
    }

    fn set_subscription_timer(&mut self, interval_seconds: Option<f64>) {
        let mut timer = self.controller.ivars().subscription_timer.borrow_mut();
        if let Some(timer) = timer.take() {
            timer.invalidate();
        }
        let Some(interval) = interval_seconds else {
            return;
        };
        // SAFETY: the controller implements `subscriptionTimer:` and outlives
        // the timer, which is invalidated when it is replaced.
        let subscriptions = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                interval,
                self.controller.as_ref(),
                sel!(subscriptionTimer:),
                None,
                true,
            )
        };
        run_in_modal_loops(&subscriptions);
        *timer = Some(subscriptions);
    }

    fn set_app_update_timer(&mut self, interval_seconds: Option<f64>) {
        let mut timer = self.controller.ivars().app_update_timer.borrow_mut();
        if let Some(timer) = timer.take() {
            timer.invalidate();
        }
        let Some(interval) = interval_seconds else {
            return;
        };
        // SAFETY: the controller implements `appUpdateTimer:` and outlives
        // the timer, which is invalidated when it is replaced.
        let updates = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                interval,
                self.controller.as_ref(),
                sel!(appUpdateTimer:),
                None,
                true,
            )
        };
        run_in_modal_loops(&updates);
        *timer = Some(updates);
    }

    fn schedule_startup_update_checks(&mut self, ytdlp: Option<f64>, app: Option<f64>) {
        for (delay, selector) in [
            (ytdlp, sel!(startupYtdlpCheck:)),
            (app, sel!(startupAppCheck:)),
        ] {
            if let Some(delay) = delay {
                // SAFETY: both selectors exist on the controller.
                unsafe {
                    let _: () = msg_send![
                        self.controller,
                        performSelector: selector,
                        withObject: Option::<&AnyObject>::None,
                        afterDelay: delay
                    ];
                }
            }
        }
    }

    fn update_prompt(&mut self, prompt: &apricot_shell::UpdatePrompt) -> bool {
        crate::update_prompt::run(self.controller.mtm(), prompt)
    }

    fn show_system_notification(&mut self, title: &str, message: &str) {
        crate::notifications::notify(self.controller.mtm(), title, message);
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

    fn open_bookmarks(&mut self, dialog: apricot_shell::BookmarkDialog) {
        *self.controller.ivars().pending_bookmarks.borrow_mut() = Some(dialog);
        // SAFETY: `openBookmarks:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openBookmarks:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn open_lyrics(&mut self, dialog: apricot_shell::LyricsDialog) {
        *self.controller.ivars().pending_lyrics.borrow_mut() = Some(dialog);
        // SAFETY: `openLyrics:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openLyrics:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn open_transcript(&mut self, dialog: apricot_shell::TranscriptDialog) {
        *self.controller.ivars().pending_transcript.borrow_mut() = Some(dialog);
        // SAFETY: `openTranscript:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openTranscript:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn open_comments(&mut self, dialog: apricot_shell::CommentsDialog) {
        *self.controller.ivars().pending_comments.borrow_mut() = Some(dialog);
        // SAFETY: `openComments:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openComments:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn open_playback_queue(&mut self, dialog: apricot_shell::QueueDialog) {
        *self.controller.ivars().pending_queue.borrow_mut() = Some(dialog);
        // SAFETY: `openPlaybackQueue:` exists on the controller.
        unsafe {
            let _: () = msg_send![
                self.controller,
                performSelector: sel!(openPlaybackQueue:),
                withObject: Option::<&AnyObject>::None,
                afterDelay: 0.0_f64
            ];
        }
    }

    fn show_context_menu(
        &mut self,
        entries: &[apricot_app::context_menu::ContextMenuEntry],
    ) -> Option<apricot_app::context_menu::ContextCommand> {
        let window = self.controller.window()?;
        crate::context_menu::show(&window, entries)
    }

    fn prompt_text(
        &mut self,
        title: &str,
        prompt: &str,
        initial: &str,
        ok: &str,
        cancel: &str,
    ) -> Option<String> {
        crate::language::prompt_text(self.controller.mtm(), title, prompt, initial, ok, cancel)
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
