//! The macOS application: its delegate, the menu bar and the run loop.

#![allow(unsafe_code)]

use std::cell::RefCell;

use apricot_app::Application;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSMenu, NSMenuItem,
};
use objc2_foundation::{NSArray, NSNotification, NSObject, NSObjectProtocol, NSString, NSURL};

use crate::window::MainWindowController;

pub struct DelegateIvars {
    application: RefCell<Option<Application>>,
    title: String,
    start_hidden: bool,
    prompt_language: bool,
    controller: RefCell<Option<Retained<MainWindowController>>>,
    /// Files opened from Finder before the window exists.
    pending_files: RefCell<Vec<std::path::PathBuf>>,
    /// Requests of second launches (Windows `WM_COPYDATA`).
    activation_listener: RefCell<Option<apricot_platform::activation::ActivationListener>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotApplicationDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            let mtm = MainThreadMarker::from(self);
            let Some(mut application) = self.ivars().application.borrow_mut().take() else {
                return;
            };
            if self.ivars().prompt_language {
                let selected = crate::language::choose(mtm, &application.settings().language);
                if let Err(error) = application.complete_initial_language(selected) {
                    eprintln!("The language choice was not saved: {error}");
                }
            }
            let catalog = apricot_app::embedded_catalog(&application.settings().language);
            NSApplication::sharedApplication(mtm).setMainMenu(Some(&menu_bar(mtm, self, &catalog)));
            let controller = MainWindowController::new(mtm, application);
            controller.show(&self.ivars().title, self.ivars().start_hidden);
            #[cfg(debug_assertions)]
            match std::env::var("APRICOT_MACOS_SELF_TEST").as_deref() {
                Ok("player") => {
                    if let Some(media) = std::env::var_os("APRICOT_MACOS_SELF_TEST_MEDIA") {
                        controller.run_player_self_test(media.into());
                    }
                }
                Ok("tools") => {
                    if let Some(media) = std::env::var_os("APRICOT_MACOS_SELF_TEST_MEDIA") {
                        controller.run_player_self_test(media.into());
                        controller.start_tools_self_test();
                    }
                }
                Ok("search") => controller.run_search_self_test(),
                Ok("trending") => controller.run_trending_self_test(),
                Ok("folder") => {
                    if let Some(media) = std::env::var_os("APRICOT_MACOS_SELF_TEST_MEDIA") {
                        controller.run_folder_self_test(media.into());
                    }
                }
                Ok(_) => controller.run_self_test(),
                Err(_) => {}
            }
            let pending = std::mem::take(&mut *self.ivars().pending_files.borrow_mut());
            for path in pending {
                controller.open_file(path);
            }
            self.listen_for_activations(mtm, &controller);
            *self.ivars().controller.borrow_mut() = Some(controller);
        }

        /// Files opened from Finder or dropped on the Dock icon (Windows
        /// `ActivationRequest::OpenFile`).
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            let paths: Vec<_> = urls
                .iter()
                .filter_map(|url| url.path())
                .map(|path| std::path::PathBuf::from(path.to_string()))
                .collect();
            let controller = self.ivars().controller.borrow().clone();
            match controller {
                Some(controller) => {
                    for path in paths {
                        controller.open_file(path);
                    }
                }
                None => self.ivars().pending_files.borrow_mut().extend(paths),
            }
        }

        /// The Dock icon of a running application with a hidden window.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, has_visible_windows: bool) -> bool {
            if !has_visible_windows
                && let Some(controller) = self.ivars().controller.borrow().as_ref()
            {
                controller.restore();
            }
            true
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            if let Some(controller) = self.ivars().controller.borrow().as_ref() {
                controller.mark_exiting();
                controller.shutdown();
            }
            NSApplicationTerminateReply::TerminateNow
        }
    }

    impl Delegate {
        /// Settings in the application menu (Command+comma), the macOS place
        /// of the main menu's Settings row.
        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            if let Some(controller) = self.ivars().controller.borrow().as_ref() {
                controller.run_action("open_settings");
            }
        }
    }
);

impl Delegate {
    /// Requests of second launches reach the controller on the main thread.
    fn listen_for_activations(
        &self,
        mtm: MainThreadMarker,
        controller: &Retained<MainWindowController>,
    ) {
        use apricot_platform::activation::{ActivationMessage, listen};
        let controller =
            std::sync::Arc::new(dispatch2::MainThreadBound::new(controller.clone(), mtm));
        let listener = listen(apricot_platform::BUILD_IDENTITY, move |message| {
            let controller = std::sync::Arc::clone(&controller);
            dispatch2::DispatchQueue::main().exec_async(move || {
                let Some(mtm) = MainThreadMarker::new() else {
                    return;
                };
                let request = match message {
                    ActivationMessage::Show => apricot_app::ActivationRequest::Show,
                    ActivationMessage::OpenSettings => apricot_app::ActivationRequest::OpenSettings,
                    ActivationMessage::OpenFile(path) => {
                        apricot_app::ActivationRequest::OpenFile(path)
                    }
                };
                controller.get(mtm).enqueue_activation(request);
            });
        });
        match listener {
            Ok(listener) => *self.ivars().activation_listener.borrow_mut() = Some(listener),
            Err(error) => eprintln!("A second launch cannot reach this instance: {error}"),
        }
    }

    fn new(
        mtm: MainThreadMarker,
        application: Application,
        title: String,
        start_hidden: bool,
        prompt_language: bool,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars {
            application: RefCell::new(Some(application)),
            title,
            start_hidden,
            prompt_language,
            controller: RefCell::new(None),
            pending_files: RefCell::new(Vec::new()),
            activation_listener: RefCell::new(None),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    }
}

fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    key: &str,
) -> Retained<NSMenuItem> {
    // SAFETY: every action used here is implemented by its target or by the
    // responder chain (standard AppKit actions).
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    }
}

/// The macOS menu bar: the application menu (Settings, Hide, Quit) and the
/// Edit menu that text fields need for copy and paste.
fn menu_bar(
    mtm: MainThreadMarker,
    delegate: &Delegate,
    catalog: &apricot_core::TranslationCatalog,
) -> Retained<NSMenu> {
    let product = apricot_platform::product_name();
    let bar = NSMenu::new(mtm);

    let app_menu = NSMenu::new(mtm);
    let settings = menu_item(
        mtm,
        &format!("{}…", catalog.text("settings")),
        Some(sel!(openSettings:)),
        ",",
    );
    // SAFETY: the delegate lives as long as the application.
    unsafe { settings.setTarget(Some(delegate.as_ref())) };
    app_menu.addItem(&settings);
    app_menu.addItem(&NSMenuItem::separatorItem(mtm));
    app_menu.addItem(&menu_item(
        mtm,
        &format!("Hide {product}"),
        Some(sel!(hide:)),
        "h",
    ));
    app_menu.addItem(&menu_item(
        mtm,
        &format!("Quit {product}"),
        Some(sel!(terminate:)),
        "q",
    ));
    let app_item = NSMenuItem::new(mtm);
    app_item.setSubmenu(Some(&app_menu));
    bar.addItem(&app_item);

    let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));
    for (title, action, key) in [
        ("Cut", sel!(cut:), "x"),
        ("Copy", sel!(copy:), "c"),
        ("Paste", sel!(paste:), "v"),
        ("Select All", sel!(selectAll:), "a"),
    ] {
        edit_menu.addItem(&menu_item(mtm, title, Some(action), key));
    }
    let edit_item = NSMenuItem::new(mtm);
    edit_item.setSubmenu(Some(&edit_menu));
    bar.addItem(&edit_item);
    bar
}

pub fn run(
    application: Application,
    version: &str,
    start_hidden: bool,
    prompt_language: bool,
) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("the macOS UI must run on the main thread")?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let title = format!("{} {version}", apricot_platform::product_name());
    let delegate = Delegate::new(mtm, application, title, start_hidden, prompt_language);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    drop(delegate);
    Ok(())
}

/// Windows `show_already_open` for a second launch.
pub fn show_already_open(message: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    app.activate();
    let alert = objc2_app_kit::NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(message));
    alert.addButtonWithTitle(&NSString::from_str("OK"));
    alert.runModal();
}
