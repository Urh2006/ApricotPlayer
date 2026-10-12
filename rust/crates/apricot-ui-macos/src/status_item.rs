//! The menu bar item, the macOS place of the Windows tray icon
//! (`add_tray_icon`, `show_tray_menu`, `handle_tray_command`): it stays for
//! the whole session and offers Show, Settings, Check subscriptions and
//! Exit. VoiceOver reaches it with VO+M twice.

#![allow(unsafe_code)]

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSImage, NSMenu, NSMenuDelegate, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

use crate::window::MainWindowController;

pub struct StatusIvars {
    controller: Weak<MainWindowController>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotStatusMenu"]
    #[ivars = StatusIvars]
    pub struct StatusMenu;

    unsafe impl NSObjectProtocol for StatusMenu {}

    unsafe impl NSMenuDelegate for StatusMenu {
        /// The entries in the current language, rebuilt on every opening as
        /// Windows builds the tray menu.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            self.fill(menu);
        }
    }

    impl StatusMenu {
        #[unsafe(method(trayShow:))]
        fn tray_show(&self, _sender: Option<&AnyObject>) {
            if let Some(controller) = self.ivars().controller.load() {
                controller.restore();
            }
        }

        #[unsafe(method(traySettings:))]
        fn tray_settings(&self, _sender: Option<&AnyObject>) {
            if let Some(controller) = self.ivars().controller.load() {
                controller.restore();
                controller.run_action("open_settings");
            }
        }

        #[unsafe(method(trayCheckSubscriptions:))]
        fn tray_check_subscriptions(&self, _sender: Option<&AnyObject>) {
            if let Some(controller) = self.ivars().controller.load() {
                controller.with_shell(|shell, ui| shell.check_subscriptions(ui, true));
            }
        }

        #[unsafe(method(trayExit:))]
        fn tray_exit(&self, _sender: Option<&AnyObject>) {
            if let Some(controller) = self.ivars().controller.load() {
                controller.mark_exiting();
            }
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl StatusMenu {
    fn fill(&self, menu: &NSMenu) {
        let mtm = self.mtm();
        let catalog = self
            .ivars()
            .controller
            .load()
            .and_then(|controller| controller.with_shell(|shell, _| shell.catalog()));
        let Some(catalog) = catalog else {
            return;
        };
        menu.removeAllItems();
        let entries: [(&str, Sel); 4] = [
            ("tray_show", sel!(trayShow:)),
            ("tray_settings", sel!(traySettings:)),
            ("tray_check_subscriptions", sel!(trayCheckSubscriptions:)),
            ("tray_exit", sel!(trayExit:)),
        ];
        for (key, action) in entries {
            if key == "tray_exit" {
                menu.addItem(&NSMenuItem::separatorItem(mtm));
            }
            // SAFETY: `initWithTitle:action:keyEquivalent:` of NSMenuItem;
            // this object implements every action and lives as long as the
            // status item.
            let item = unsafe {
                let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(catalog.text(key)),
                    Some(action),
                    &NSString::from_str(""),
                );
                item.setTarget(Some(self.as_ref()));
                item
            };
            menu.addItem(&item);
        }
    }
}

/// The menu bar item with its menu; both live until the application ends.
pub struct StatusItem {
    item: Retained<NSStatusItem>,
    menu: Retained<StatusMenu>,
}

impl StatusItem {
    /// Development self-test: the entry titles, and runs entry `index`.
    #[cfg(debug_assertions)]
    pub fn self_test(&self, index: Option<isize>) -> Vec<String> {
        let mtm = MainThreadMarker::from(&*self.menu);
        let Some(menu) = self.item.menu(mtm) else {
            return Vec::new();
        };
        if let Some(index) = index {
            menu.performActionForItemAtIndex(index);
        }
        menu.itemArray()
            .iter()
            .map(|item| item.title().to_string())
            .collect()
    }
}

/// Windows `add_tray_icon`.
pub fn install(controller: &MainWindowController) -> StatusItem {
    let mtm = MainThreadMarker::from(controller);
    let target: Retained<StatusMenu> = {
        let this = StatusMenu::alloc(mtm).set_ivars(StatusIvars {
            controller: Weak::from(controller),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let product = NSString::from_str(apricot_platform::product_name());
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        // The tray tooltip and VoiceOver's name of the item.
        match NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("play.circle"),
            Some(&product),
        ) {
            Some(image) => button.setImage(Some(&image)),
            None => button.setTitle(&product),
        }
        button.setToolTip(Some(&product));
    }
    let menu = NSMenu::new(mtm);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*target)));
    target.fill(&menu);
    item.setMenu(Some(&menu));
    StatusItem {
        item,
        menu: target,
    }
}
