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
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSString};

use crate::window::MainWindowController;

pub struct DelegateIvars {
    application: RefCell<Option<Application>>,
    title: String,
    start_hidden: bool,
    prompt_language: bool,
    controller: RefCell<Option<Retained<MainWindowController>>>,
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
            if std::env::var_os("APRICOT_MACOS_SELF_TEST").is_some() {
                controller.run_self_test();
            }
            *self.ivars().controller.borrow_mut() = Some(controller);
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
