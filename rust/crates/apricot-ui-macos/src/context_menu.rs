//! Context menus of the main window (Windows `TrackPopupMenu` with the
//! entries of `apricot_app::context_menu`): commands, submenus and greyed
//! entries, shown at the focused control. Choosing an entry returns its
//! command; focus stays on the control.

#![allow(unsafe_code)]

use std::cell::Cell;

use apricot_app::context_menu::{ContextCommand, ContextMenuEntry};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSMenu, NSMenuItem, NSView, NSWindow};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSString};

use crate::controls::List;

#[derive(Default)]
pub struct TargetIvars {
    chosen: Cell<Option<isize>>,
}

define_class!(
    /// Receives the chosen menu item.
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotContextMenuTarget"]
    #[ivars = TargetIvars]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(choose:))]
        fn choose(&self, sender: Option<&NSMenuItem>) {
            self.ivars().chosen.set(sender.map(NSMenuItem::tag));
        }
    }
);

/// Windows `append_context_model_entries`.
fn append(
    mtm: MainThreadMarker,
    menu: &NSMenu,
    entries: &[ContextMenuEntry],
    target: &MenuTarget,
    commands: &mut Vec<ContextCommand>,
) {
    for entry in entries {
        // The shortcut after the tab reads as part of the item, as in Python.
        let title = NSString::from_str(&entry.label().replace('\t', "  "));
        let action = matches!(entry, ContextMenuEntry::Command { .. }).then_some(sel!(choose:));
        // SAFETY: `initWithTitle:action:keyEquivalent:` of NSMenuItem.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &title,
                action,
                &NSString::from_str(""),
            )
        };
        match entry {
            ContextMenuEntry::Command { command, .. } => {
                item.setTag(isize::try_from(commands.len()).unwrap_or(isize::MAX));
                commands.push(*command);
                // SAFETY: the target lives until the menu closed.
                unsafe { item.setTarget(Some(target)) };
            }
            ContextMenuEntry::Submenu { label, entries } => {
                let submenu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(label));
                submenu.setAutoenablesItems(false);
                append(mtm, &submenu, entries, target, commands);
                item.setSubmenu(Some(&submenu));
            }
            ContextMenuEntry::Disabled { .. } => item.setEnabled(false),
        }
        menu.addItem(&item);
    }
}

/// Shows `entries` at the focused control of `window`.
pub fn show(window: &NSWindow, entries: &[ContextMenuEntry]) -> Option<ContextCommand> {
    let mtm = MainThreadMarker::from(window);
    let target: Retained<MenuTarget> = {
        let this = MenuTarget::alloc(mtm).set_ivars(TargetIvars::default());
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let mut commands = Vec::new();
    append(mtm, &menu, entries, &target, &mut commands);
    let focused = window
        .firstResponder()
        .and_then(|responder| Retained::downcast::<NSView>(responder).ok());
    let view: Retained<NSView> = focused.or_else(|| window.contentView())?;
    // Windows `context_menu_point`: at the selected row, or the control.
    let point = view.downcast_ref::<List>().map_or_else(
        || NSPoint::new(16.0, 16.0),
        |list| {
            let rect = list.rectOfRow(list.selectedRow().max(0));
            NSPoint::new(rect.origin.x + 20.0, rect.origin.y + rect.size.height)
        },
    );
    #[cfg(debug_assertions)]
    if let Ok(choice) = std::env::var("APRICOT_MACOS_SELF_TEST_MENU_CHOICE") {
        // Development self-test: the menu is printed and an entry chosen by
        // the start of its label, as a click on it would.
        let labels: Vec<&str> = entries.iter().map(ContextMenuEntry::label).collect();
        println!("context menu: {labels:?}");
        return entries.iter().find_map(|entry| match entry {
            ContextMenuEntry::Command { label, command } if label.starts_with(&choice) => {
                Some(*command)
            }
            _ => None,
        });
    }
    menu.popUpMenuPositioningItem_atLocation_inView(None, point, Some(&view));
    let _: &AnyObject = target.as_ref();
    target
        .ivars()
        .chosen
        .get()
        .and_then(|tag| usize::try_from(tag).ok())
        .and_then(|index| commands.get(index).copied())
}
