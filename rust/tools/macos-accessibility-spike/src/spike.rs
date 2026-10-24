//! The AppKit window of the spike. All objects live on the main thread.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::path::PathBuf;

use apricot_playback::{MpvLaunchOptions, MpvRenderHost, MpvVideoMode, PlaybackRuntime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityAnnouncementKey,
    NSAccessibilityAnnouncementRequestedNotification, NSAccessibilityPostNotificationWithUserInfo,
    NSAccessibilityPriorityKey, NSAccessibilityPriorityLevel, NSAlert, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType, NSButton,
    NSControlTextEditingDelegate, NSEvent, NSEventModifierFlags, NSMenu, NSMenuItem, NSPopUpButton,
    NSResponder, NSScrollView, NSSlider, NSTableColumn, NSTableView, NSTableViewDataSource,
    NSTableViewDelegate, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    NSDictionary, NSIndexSet, NSInteger, NSNotification, NSNumber, NSObject, NSObjectProtocol,
    NSPoint, NSRect, NSSize, NSString,
};

/// Rows of the list, shaped like the Apricot main menu.
const ROWS: [&str; 9] = [
    "Search YouTube / SoundCloud",
    "Play file",
    "Favorites",
    "History",
    "Playlists",
    "Podcasts and RSS",
    "Bookmarks",
    "Settings",
    "Exit",
];

const KEY_RETURN: u16 = 36;
const KEY_KEYPAD_ENTER: u16 = 76;
const KEY_ESCAPE: u16 = 53;
const KEY_F10: u16 = 109;
const KEY_TAB: u16 = 48;

/// Tab and Shift+Tab move along the declared key view loop on every control,
/// as on Windows, whether or not Full Keyboard Access is on.
fn handle_tab(view: &NSView, event: &NSEvent) -> bool {
    if event.keyCode() != KEY_TAB {
        return false;
    }
    let Some(window) = view.window() else {
        return false;
    };
    if event.modifierFlags().contains(NSEventModifierFlags::Shift) {
        window.selectKeyViewPrecedingView(view);
    } else {
        window.selectKeyViewFollowingView(view);
    }
    true
}

/// Posts one VoiceOver announcement with high priority, the macOS
/// counterpart of the NVDA controller's `speakText`.
fn announce(text: &str) {
    println!("announce: {text}");
    let app = NSApplication::sharedApplication(MainThreadMarker::new().expect("main thread"));
    let message = NSString::from_str(text);
    let priority = NSNumber::new_isize(NSAccessibilityPriorityLevel::High.0);
    // SAFETY: both keys are AppKit's static user-info keys and the values are
    // the documented types (an `NSString` and an `NSNumber`).
    unsafe {
        let info: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
            &[NSAccessibilityAnnouncementKey, NSAccessibilityPriorityKey],
            &[
                message.as_ref() as &AnyObject,
                priority.as_ref() as &AnyObject,
            ],
        );
        NSAccessibilityPostNotificationWithUserInfo(
            &app,
            NSAccessibilityAnnouncementRequestedNotification,
            Some(&info),
        );
    }
}

#[derive(Default)]
struct SpikeIvars {
    window: RefCell<Option<Retained<NSWindow>>>,
    table: RefCell<Option<Retained<SpikeTable>>>,
    video: RefCell<Option<Retained<NSView>>>,
    runtime: RefCell<Option<PlaybackRuntime>>,
    render_host: RefCell<Option<MpvRenderHost>>,
    media: RefCell<Option<PathBuf>>,
    self_test: std::cell::Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSpikeController"]
    #[ivars = SpikeIvars]
    struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSApplicationDelegate for Controller {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            self.build_window();
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate(&self, _app: &NSApplication) -> bool {
            true
        }
    }

    unsafe impl NSTableViewDataSource for Controller {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table: &NSTableView) -> NSInteger {
            ROWS.len().try_into().unwrap_or(0)
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
                .and_then(|row| ROWS.get(row))
                .map(|text| Retained::into_super(Retained::into_super(NSString::from_str(text))))
        }
    }

    unsafe impl NSControlTextEditingDelegate for Controller {}

    unsafe impl NSTableViewDelegate for Controller {}

    impl Controller {
        #[unsafe(method(openRow:))]
        fn open_row(&self, _sender: Option<&AnyObject>) {
            self.activate_selected_row();
        }

        #[unsafe(method(announceTest:))]
        fn announce_test(&self, _sender: Option<&AnyObject>) {
            announce("Announcement test. This text was spoken without moving focus.");
        }

        #[unsafe(method(playVideo:))]
        fn play_video(&self, _sender: Option<&AnyObject>) {
            self.start_video();
        }

        #[unsafe(method(contextChoice:))]
        fn context_choice(&self, sender: Option<&AnyObject>) {
            let title = sender
                .and_then(|sender| sender.downcast_ref::<NSMenuItem>())
                .map(|item| item.title().to_string())
                .unwrap_or_default();
            announce(&format!("Context menu: {title}"));
        }

        #[unsafe(method(checkboxChanged:))]
        fn checkbox_changed(&self, _sender: Option<&AnyObject>) {}

        #[unsafe(method(sliderChanged:))]
        fn slider_changed(&self, _sender: Option<&AnyObject>) {}

        #[unsafe(method(selfTestStart:))]
        fn self_test_start(&self, _sender: Option<&AnyObject>) {
            self.report_accessibility();
            self.start_video();
            // SAFETY: `selfTestFinish:` exists on this object.
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelector: sel!(selfTestFinish:),
                    withObject: Option::<&AnyObject>::None,
                    afterDelay: 4.0_f64
                ];
            }
        }

        /// Drains libmpv events every 50 ms while a video is loaded, as the
        /// application's playback timer does.
        #[unsafe(method(pollPlayer:))]
        fn poll_player(&self, _sender: Option<&AnyObject>) {
            if let Some(runtime) = self.ivars().runtime.borrow().as_ref() {
                loop {
                    match runtime.poll_update() {
                        Ok(Some(update)) => println!("mpv event: {:?}", update.event),
                        Ok(None) => break,
                        Err(error) => {
                            println!("mpv poll error: {error}");
                            break;
                        }
                    }
                }
            } else {
                return;
            }
            // SAFETY: `pollPlayer:` exists on this object.
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelector: sel!(pollPlayer:),
                    withObject: Option::<&AnyObject>::None,
                    afterDelay: 0.05_f64
                ];
            }
        }

        #[unsafe(method(selfTestFinish:))]
        fn self_test_finish(&self, _sender: Option<&AnyObject>) {
            if let Some(runtime) = self.ivars().runtime.borrow().as_ref() {
                while let Ok(Some(update)) = runtime.poll_update() {
                    println!("mpv event: {:?}", update.event);
                }
            }
            let app = NSApplication::sharedApplication(self.mtm());
            for window in &app.windows() {
                println!(
                    "window: {} visible={} frame={:?}",
                    window.title(),
                    window.isVisible(),
                    window.frame()
                );
            }
            println!("video frames drawn: {}", crate::video::frames_drawn());
            app.terminate(None);
        }
    }
);

impl Controller {
    fn new(mtm: MainThreadMarker, media: Option<PathBuf>, self_test: bool) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SpikeIvars {
            media: RefCell::new(media),
            self_test: std::cell::Cell::new(self_test),
            ..SpikeIvars::default()
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    }

    fn mtm(&self) -> MainThreadMarker {
        MainThreadMarker::from(self)
    }

    #[allow(clippy::unnecessary_wraps)] // AppKit's `setTarget:` takes an optional target.
    fn target(&self) -> Option<&AnyObject> {
        Some(self.as_ref())
    }

    #[allow(clippy::too_many_lines)]
    fn build_window(&self) {
        let mtm = self.mtm();
        let app = NSApplication::sharedApplication(mtm);
        app.setMainMenu(Some(&main_menu(mtm)));

        let frame = NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(720.0, 560.0));
        // SAFETY: a standard titled, closable window with a buffered backing.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: the controller outlives the window (application lifetime).
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str("ApricotPlayer accessibility spike"));
        let content = window.contentView().expect("content view");

        // The list, like the Apricot main menu list box.
        let table = SpikeTable::new(
            mtm,
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(320.0, 240.0)),
        );
        let column = NSTableColumn::initWithIdentifier(
            NSTableColumn::alloc(mtm),
            &NSString::from_str("title"),
        );
        column.setWidth(300.0);
        column.setEditable(false);
        table.addTableColumn(&column);
        table.setHeaderView(None);
        table.setAllowsTypeSelect(true);
        // SAFETY: the controller implements both protocols and outlives the table.
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
            table.setTarget(self.target());
            table.setDoubleAction(Some(sel!(openRow:)));
        }
        table.setAccessibilityLabel(Some(&NSString::from_str("Main menu")));
        // SAFETY: the menu's target (the controller) outlives the table.
        unsafe { table.setMenu(Some(&context_menu(mtm, self))) };
        table.reloadData();
        table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(0), false);
        let scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(mtm),
            NSRect::new(NSPoint::new(20.0, 280.0), NSSize::new(320.0, 240.0)),
        );
        scroll.setDocumentView(Some(&table));
        scroll.setHasVerticalScroller(true);
        content.addSubview(&scroll);

        // Buttons.
        let open = tab_button(mtm, "Open", self.target(), sel!(openRow:));
        open.setFrame(NSRect::new(
            NSPoint::new(360.0, 480.0),
            NSSize::new(160.0, 32.0),
        ));
        content.addSubview(&open);
        let speak = tab_button(mtm, "Announce", self.target(), sel!(announceTest:));
        speak.setFrame(NSRect::new(
            NSPoint::new(360.0, 440.0),
            NSSize::new(160.0, 32.0),
        ));
        content.addSubview(&speak);
        let play = tab_button(mtm, "Play test video", self.target(), sel!(playVideo:));
        play.setFrame(NSRect::new(
            NSPoint::new(360.0, 400.0),
            NSSize::new(160.0, 32.0),
        ));
        content.addSubview(&play);

        // A labelled search field.
        let search_label = NSTextField::labelWithString(&NSString::from_str("Search:"), mtm);
        search_label.setFrame(NSRect::new(
            NSPoint::new(360.0, 360.0),
            NSSize::new(80.0, 22.0),
        ));
        content.addSubview(&search_label);
        let search = NSTextField::textFieldWithString(&NSString::new(), mtm);
        search.setFrame(NSRect::new(
            NSPoint::new(440.0, 360.0),
            NSSize::new(240.0, 22.0),
        ));
        search.setAccessibilityLabel(Some(&NSString::from_str("Search")));
        content.addSubview(&search);

        // A provider choice, like the Windows combo box.
        let provider = TabPopUp::new(
            mtm,
            NSRect::new(NSPoint::new(360.0, 320.0), NSSize::new(200.0, 28.0)),
        );
        provider.addItemWithTitle(&NSString::from_str("YouTube"));
        provider.addItemWithTitle(&NSString::from_str("SoundCloud"));
        provider.setAccessibilityLabel(Some(&NSString::from_str("Provider")));
        content.addSubview(&provider);

        // A check box.
        let autoplay = tab_checkbox(mtm, "Autoplay next", self.target(), sel!(checkboxChanged:));
        autoplay.setFrame(NSRect::new(
            NSPoint::new(360.0, 285.0),
            NSSize::new(240.0, 24.0),
        ));
        content.addSubview(&autoplay);

        // A slider with a spoken percentage.
        let volume = TabSlider::new(mtm, self.target(), sel!(sliderChanged:));
        volume.setFrame(NSRect::new(
            NSPoint::new(20.0, 240.0),
            NSSize::new(320.0, 24.0),
        ));
        volume.setAccessibilityLabel(Some(&NSString::from_str("Volume")));
        content.addSubview(&volume);

        // The video surface for libmpv's render API.
        let (video, host) = crate::video::video_view(
            mtm,
            NSRect::new(NSPoint::new(20.0, 20.0), NSSize::new(320.0, 180.0)),
        );
        content.addSubview(&video);
        *self.ivars().render_host.borrow_mut() = Some(host);

        // Tab order, declared explicitly as on Windows.
        // SAFETY: all views are subviews of the same window.
        unsafe {
            table.setNextKeyView(Some(&open));
            open.setNextKeyView(Some(&speak));
            speak.setNextKeyView(Some(&play));
            play.setNextKeyView(Some(&search));
            search.setNextKeyView(Some(&provider));
            provider.setNextKeyView(Some(&autoplay));
            autoplay.setNextKeyView(Some(&volume));
            volume.setNextKeyView(Some(&table));
        }
        window.setAutorecalculatesKeyViewLoop(false);
        window.setInitialFirstResponder(Some(&table));
        window.makeKeyAndOrderFront(None);
        window.makeFirstResponder(Some(&table));
        app.activate();

        *self.ivars().window.borrow_mut() = Some(window);
        *self.ivars().table.borrow_mut() = Some(table);
        *self.ivars().video.borrow_mut() = Some(video);
        if self.ivars().self_test.get() {
            // SAFETY: `selfTestStart:` exists on this object.
            unsafe {
                let _: () = msg_send![
                    self,
                    performSelector: sel!(selfTestStart:),
                    withObject: Option::<&AnyObject>::None,
                    afterDelay: 1.0_f64
                ];
            }
        }
    }

    /// Prints what AppKit exposes to VoiceOver for each control, and the Tab
    /// order as `selectNextKeyView:` walks it.
    fn report_accessibility(&self) {
        let Some(window) = self.ivars().window.borrow().clone() else {
            return;
        };
        let Some(content) = window.contentView() else {
            return;
        };
        for view in &content.subviews() {
            print_element("control", &view);
        }
        for (label, shift) in [(false, false); 10]
            .into_iter()
            .enumerate()
            .map(|(index, _)| (index, false))
            .chain((0..3).map(|index| (index, true)))
        {
            let responder = window.firstResponder();
            let mut name = responder
                .as_ref()
                .map(|responder| element_summary(responder))
                .unwrap_or_default();
            // While a text field edits, the field editor is the responder.
            if name.starts_with("NSTextView") {
                name = format!("field editor of the search field: {name}");
            }
            println!(
                "{} {label}: {name}",
                if shift { "shift+tab" } else { "tab" }
            );
            send_tab(&window, shift);
        }
    }

    /// Enter in the list: one action, then a modal alert, then focus back on
    /// the list (the Windows `show_message_box` contract).
    fn activate_selected_row(&self) {
        let Some(table) = self.ivars().table.borrow().clone() else {
            return;
        };
        let row = usize::try_from(table.selectedRow()).ok();
        let title = row
            .and_then(|row| ROWS.get(row))
            .copied()
            .unwrap_or("nothing");
        let window = self.ivars().window.borrow().clone();
        let previous = window.as_ref().and_then(|window| window.firstResponder());
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str(&format!("You opened {title}.")));
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        alert.runModal();
        if let (Some(window), Some(previous)) = (window, previous) {
            window.makeFirstResponder(Some(&previous));
        }
    }

    fn start_video(&self) {
        if self.ivars().runtime.borrow().is_some() {
            announce("Video is already playing.");
            return;
        }
        let Some(media) = self.ivars().media.borrow().clone() else {
            announce("Start the spike with a video file path to test video.");
            return;
        };
        let Some(host) = self.ivars().render_host.borrow().clone() else {
            return;
        };
        let mut options = MpvLaunchOptions::new(PathBuf::from("/opt/homebrew/bin/mpv"));
        options.log_file = std::env::var_os("APRICOT_SPIKE_MPV_LOG").map(PathBuf::from);
        options.library = Some(PathBuf::from("/opt/homebrew/lib/libmpv.2.dylib"));
        options.video_mode = MpvVideoMode::RenderApi(host.id());
        let result = PlaybackRuntime::spawn()
            .map_err(|error| error.to_string())
            .and_then(|runtime| {
                runtime
                    .start(1, options, local_item(&media))
                    .map_err(|error| error.to_string())?;
                Ok(runtime)
            });
        match result {
            Ok(runtime) => {
                *self.ivars().runtime.borrow_mut() = Some(runtime);
                announce("Playing the test video.");
                // SAFETY: `pollPlayer:` exists on this object.
                unsafe {
                    let _: () = msg_send![
                        self,
                        performSelector: sel!(pollPlayer:),
                        withObject: Option::<&AnyObject>::None,
                        afterDelay: 0.05_f64
                    ];
                }
            }
            Err(error) => announce(&format!("Video failed: {error}")),
        }
    }
}

/// Sends a real Tab (or Shift+Tab) key press to the window, as the keyboard
/// would, so the key view loop and the field editor take part.
fn send_tab(window: &NSWindow, shift: bool) {
    let flags = if shift {
        NSEventModifierFlags::Shift
    } else {
        NSEventModifierFlags::empty()
    };
    let characters = NSString::from_str("\t");
    for kind in [
        objc2_app_kit::NSEventType::KeyDown,
        objc2_app_kit::NSEventType::KeyUp,
    ] {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind,
            NSPoint::new(0.0, 0.0),
            flags,
            0.0,
            window.windowNumber(),
            None,
            &characters,
            &characters,
            false,
            48,
        );
        if let Some(event) = event {
            window.sendEvent(&event);
        }
    }
}

fn element_summary(element: &AnyObject) -> String {
    // SAFETY: every AppKit view and accessibility element answers these.
    unsafe {
        let class = element.class().name().to_string_lossy().into_owned();
        let role: Option<Retained<NSString>> = msg_send![element, accessibilityRole];
        let label: Option<Retained<NSString>> = msg_send![element, accessibilityLabel];
        let title: Option<Retained<NSString>> = msg_send![element, accessibilityTitle];
        let value: Option<Retained<AnyObject>> = msg_send![element, accessibilityValue];
        let value_description: Option<Retained<NSString>> =
            msg_send![element, accessibilityValueDescription];
        let value = value.map(|value| {
            let description: Retained<NSString> = msg_send![&*value, description];
            description.to_string()
        });
        format!(
            "{class} role={} label={:?} title={:?} value={:?} value_description={:?}",
            role.map(|role| role.to_string()).unwrap_or_default(),
            label.map(|label| label.to_string()),
            title.map(|title| title.to_string()),
            value,
            value_description.map(|text| text.to_string()),
        )
    }
}

fn print_element(kind: &str, element: &AnyObject) {
    println!("{kind}: {}", element_summary(element));
}

fn local_item(path: &std::path::Path) -> apricot_core::MediaItem {
    apricot_core::MediaItem {
        id: apricot_core::MediaId("spike".to_owned()),
        source: apricot_core::MediaSource::Local,
        kind: apricot_core::MediaKind::Video,
        title: "Spike video".to_owned(),
        url: None,
        stream_url: None,
        external_audio_url: None,
        local_path: Some(path.to_string_lossy().into_owned()),
        channel: String::new(),
        duration_seconds: None,
        metadata: std::collections::BTreeMap::new(),
    }
}

define_class!(
    /// The list: Enter opens the row, Shift+F10 opens the context menu at the
    /// selected row, Escape is reported.
    // SAFETY: NSTableView may be subclassed; no Drop impl.
    #[unsafe(super(NSTableView, objc2_app_kit::NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSpikeTable"]
    struct SpikeTable;

    impl SpikeTable {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let code = event.keyCode();
            let shift = event.modifierFlags().contains(NSEventModifierFlags::Shift);
            if handle_tab(self, event) {
            } else if code == KEY_RETURN || code == KEY_KEYPAD_ENTER {
                // SAFETY: the action selector exists on the controller target.
                unsafe {
                    let _: () = msg_send![self, sendAction: sel!(openRow:), to: self.target().as_deref()];
                }
            } else if code == KEY_F10 && shift {
                self.show_context_menu();
            } else if code == KEY_ESCAPE {
                announce("Escape pressed.");
            } else {
                // SAFETY: forwarding the unhandled event to NSTableView.
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }
    }
);

impl SpikeTable {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` of NSTableView.
        unsafe { msg_send![super(this), initWithFrame: frame] }
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
}

define_class!(
    /// A push button that is always in the Tab order, as Windows buttons are,
    /// and that also presses on Return.
    // SAFETY: NSButton may be subclassed; no Drop impl.
    #[unsafe(super(NSButton, objc2_app_kit::NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSpikeButton"]
    struct TabButton;

    impl TabButton {
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
            } else if matches!(event.keyCode(), KEY_RETURN | KEY_KEYPAD_ENTER) {
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

fn tab_button(
    mtm: MainThreadMarker,
    title: &str,
    target: Option<&AnyObject>,
    action: Sel,
) -> Retained<TabButton> {
    let this = TabButton::alloc(mtm).set_ivars(());
    // SAFETY: `initWithFrame:` of NSButton; the target outlives the button.
    let button: Retained<TabButton> =
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
    button.setTitle(&NSString::from_str(title));
    button.setBezelStyle(objc2_app_kit::NSBezelStyle::Push);
    // SAFETY: as above.
    unsafe {
        button.setTarget(target);
        button.setAction(Some(action));
    }
    button
}

fn tab_checkbox(
    mtm: MainThreadMarker,
    title: &str,
    target: Option<&AnyObject>,
    action: Sel,
) -> Retained<TabButton> {
    let button = tab_button(mtm, title, target, action);
    button.setButtonType(objc2_app_kit::NSButtonType::Switch);
    button
}

define_class!(
    /// A pop-up button that is always in the Tab order.
    // SAFETY: NSPopUpButton may be subclassed; no Drop impl.
    #[unsafe(super(NSPopUpButton, NSButton, objc2_app_kit::NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSpikePopUp"]
    struct TabPopUp;

    impl TabPopUp {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !handle_tab(self, event) {
                // SAFETY: forwarding the unhandled event to the superclass.
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeKeyView))]
        fn can_become_key_view(&self) -> bool {
            true
        }
    }
);

impl TabPopUp {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:pullsDown:` of NSPopUpButton.
        unsafe { msg_send![super(this), initWithFrame: frame, pullsDown: false] }
    }
}

define_class!(
    /// A slider that is always in the Tab order and speaks its value as a
    /// percentage.
    // SAFETY: NSSlider may be subclassed; no Drop impl.
    #[unsafe(super(NSSlider, objc2_app_kit::NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSpikeSlider"]
    struct TabSlider;

    impl TabSlider {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !handle_tab(self, event) {
                // SAFETY: forwarding the unhandled event to the superclass.
                unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                }
            }
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeKeyView))]
        fn can_become_key_view(&self) -> bool {
            true
        }

        #[unsafe(method_id(accessibilityValueDescription))]
        fn value_description(&self) -> Option<Retained<NSString>> {
            Some(NSString::from_str(&format!("{:.0} percent", self.doubleValue())))
        }
    }
);

impl TabSlider {
    fn new(mtm: MainThreadMarker, target: Option<&AnyObject>, action: Sel) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: `initWithFrame:` of NSSlider; the target outlives the slider.
        let slider: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        slider.setMinValue(0.0);
        slider.setMaxValue(100.0);
        slider.setDoubleValue(50.0);
        // SAFETY: as above.
        unsafe {
            slider.setTarget(target);
            slider.setAction(Some(action));
        }
        slider
    }
}

fn main_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let bar = NSMenu::new(mtm);
    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);
    // SAFETY: `terminate:` is NSApplication's standard action.
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Quit ApricotPlayer spike"),
            Some(sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    app_menu.addItem(&quit);
    app_item.setSubmenu(Some(&app_menu));
    bar.addItem(&app_item);
    bar
}

fn context_menu(mtm: MainThreadMarker, controller: &Controller) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    for title in ["Play", "Add to favorites", "Copy link"] {
        // SAFETY: the controller implements `contextChoice:` and outlives the menu.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(sel!(contextChoice:)),
                &NSString::new(),
            )
        };
        // SAFETY: as above.
        unsafe { item.setTarget(Some(controller.as_ref())) };
        menu.addItem(&item);
    }
    menu
}

pub fn run() {
    let mtm = MainThreadMarker::new().expect("the spike must start on the main thread");
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let self_test = arguments.iter().any(|argument| argument == "--self-test");
    let media = arguments
        .iter()
        .find(|argument| !argument.to_string_lossy().starts_with("--"))
        .map(PathBuf::from);
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let controller = Controller::new(mtm, media, self_test);
    app.setDelegate(Some(ProtocolObject::from_ref(&*controller)));
    app.run();
}
