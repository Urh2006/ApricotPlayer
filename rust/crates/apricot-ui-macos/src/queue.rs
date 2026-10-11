//! The playback queue window, the macOS counterpart of
//! `playback_queue_win32.rs`: instructions, the queue list (focused) and
//! Play, Move up, Move down, Remove, Clear and Back in that Tab order,
//! modal over the main window. Enter and a double click play, Escape and
//! closing go back, Shift+F10 and VoiceOver's menu command open the context
//! menu. The edits are made by `apricot_shell::QueueDialog`.

#![allow(unsafe_code)]

use std::cell::RefCell;

use apricot_shell::{QueueChange, QueueDialog};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSMenu, NSMenuItem, NSScrollView, NSTextField, NSWindow,
    NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::controls::{self, Button, List};
use crate::window::MainWindowController;

/// Windows `CreateWindowExW(..., 760, 520, ...)`.
const WIDTH: f64 = 760.0;
const HEIGHT: f64 = 520.0;
const MARGIN: f64 = 12.0;
const INSTRUCTIONS_HEIGHT: f64 = 34.0;
const BUTTON_HEIGHT: f64 = 34.0;
const GAP: f64 = 6.0;

pub struct DialogIvars {
    dialog: RefCell<Option<QueueDialog>>,
    list: RefCell<Option<Retained<List>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotPlaybackQueueDialog"]
    #[ivars = DialogIvars]
    struct QueueWindow;

    unsafe impl NSObjectProtocol for QueueWindow {}

    unsafe impl NSWindowDelegate for QueueWindow {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    impl QueueWindow {
        #[unsafe(method(closeDialog:))]
        fn close_dialog(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(playItem:))]
        fn play_item(&self, _sender: Option<&AnyObject>) {
            self.edit(QueueDialog::play);
        }

        #[unsafe(method(moveUp:))]
        fn move_up(&self, _sender: Option<&AnyObject>) {
            self.edit(|dialog, selected| dialog.move_selected(selected, false));
        }

        #[unsafe(method(moveDown:))]
        fn move_down(&self, _sender: Option<&AnyObject>) {
            self.edit(|dialog, selected| dialog.move_selected(selected, true));
        }

        #[unsafe(method(removeItem:))]
        fn remove_item(&self, _sender: Option<&AnyObject>) {
            self.edit(QueueDialog::remove);
        }

        #[unsafe(method(clearQueue:))]
        fn clear_queue(&self, _sender: Option<&AnyObject>) {
            self.edit(|dialog, _| dialog.clear());
        }
    }
);

impl QueueWindow {
    fn edit(&self, action: impl FnOnce(&mut QueueDialog, Option<usize>) -> QueueChange) {
        let selected = self
            .ivars()
            .list
            .borrow()
            .as_ref()
            .and_then(|list| list.selected());
        let change = {
            let mut dialog = self.ivars().dialog.borrow_mut();
            let Some(dialog) = dialog.as_mut() else {
                return;
            };
            action(dialog, selected)
        };
        match change {
            QueueChange::Refresh {
                rows,
                selected,
                announcement,
            } => {
                // Focus stays where it is, like Python's buttons.
                if let Some(list) = self.ivars().list.borrow().as_ref() {
                    list.set_rows(rows, Some(selected));
                }
                crate::announce::announce(self.mtm(), &announcement, true);
            }
            QueueChange::Close { announcement } => {
                if let Some(announcement) = announcement {
                    crate::announce::announce(self.mtm(), &announcement, true);
                }
                NSApplication::sharedApplication(self.mtm()).stopModal();
            }
            QueueChange::None => {}
        }
    }
}

fn create_window(mtm: MainThreadMarker, dialog: &QueueWindow, title: &str) -> Retained<NSWindow> {
    // SAFETY: a standard titled window with a buffered backing.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(WIDTH, HEIGHT)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is released by its last owner, `run`.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(title));
    window.setDelegate(Some(ProtocolObject::from_ref(dialog)));
    window
}

/// Windows `show_context_menu` of the queue window: Play, Move up, Move
/// down, Remove and Clear.
fn queue_menu(
    mtm: MainThreadMarker,
    target: &AnyObject,
    items: [(&str, Sel); 5],
) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    for (title, action) in items {
        // SAFETY: `initWithTitle:action:keyEquivalent:` of NSMenuItem.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(""),
            )
        };
        // SAFETY: the dialog outlives the menu (it is the list's menu).
        unsafe { item.setTarget(Some(target)) };
        menu.addItem(&item);
    }
    menu
}

/// The six buttons in a row at the bottom (Windows `layout`).
fn create_buttons(
    mtm: MainThreadMarker,
    content: &objc2_app_kit::NSView,
    target: &AnyObject,
    texts: &[String; 6],
    actions: [Sel; 6],
) -> Vec<Retained<Button>> {
    let button_width = (WIDTH - 2.0 * MARGIN - GAP * 5.0) / 6.0;
    texts
        .iter()
        .zip(actions)
        .enumerate()
        .map(|(index, (text, action))| {
            let button = Button::new(mtm, text, target, action);
            #[allow(clippy::cast_precision_loss)]
            let left = MARGIN + index as f64 * (button_width + GAP);
            button.setFrame(NSRect::new(
                NSPoint::new(left, MARGIN),
                NSSize::new(button_width, BUTTON_HEIGHT),
            ));
            content.addSubview(&button);
            button
        })
        .collect()
}

/// Windows `show_playback_queue` and `playback_queue_win32::show`.
pub fn run(controller: &MainWindowController, dialog: QueueDialog) {
    let mtm = MainThreadMarker::from(controller);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let rows = dialog.rows();
    let texts = [
        dialog.play.clone(),
        dialog.move_up.clone(),
        dialog.move_down.clone(),
        dialog.remove.clone(),
        dialog.clear.clone(),
        dialog.back.clone(),
    ];
    let (title, instructions) = (dialog.title.clone(), dialog.instructions.clone());
    let this: Retained<QueueWindow> = {
        let this = QueueWindow::alloc(mtm).set_ivars(DialogIvars {
            dialog: RefCell::new(Some(dialog)),
            list: RefCell::new(None),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let target: &AnyObject = this.as_ref();
    let window = create_window(mtm, &this, &title);
    let content = window.contentView().expect("a window has a content view");

    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(&instructions), mtm);
    label.setFrame(NSRect::new(
        NSPoint::new(MARGIN, HEIGHT - MARGIN - INSTRUCTIONS_HEIGHT),
        NSSize::new(WIDTH - 2.0 * MARGIN, INSTRUCTIONS_HEIGHT),
    ));
    content.addSubview(&label);
    let list = List::new(mtm, &title);
    list.set_rows(rows, Some(0));
    list.set_activate_action(target, sel!(playItem:));
    let list_bottom = MARGIN * 2.0 + BUTTON_HEIGHT;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, list_bottom),
            NSSize::new(
                WIDTH - 2.0 * MARGIN,
                HEIGHT - MARGIN - INSTRUCTIONS_HEIGHT - list_bottom,
            ),
        ),
    );
    scroll.setDocumentView(Some(&list));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);

    let actions = [
        sel!(playItem:),
        sel!(moveUp:),
        sel!(moveDown:),
        sel!(removeItem:),
        sel!(clearQueue:),
        sel!(closeDialog:),
    ];
    let buttons = create_buttons(mtm, &content, target, &texts, actions);
    if let Some(back) = buttons.last() {
        // `IDCANCEL`: Escape goes back.
        back.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    }
    let menu = queue_menu(
        mtm,
        target,
        [
            (&texts[0], actions[0]),
            (&texts[1], actions[1]),
            (&texts[2], actions[2]),
            (&texts[3], actions[3]),
            (&texts[4], actions[4]),
        ],
    );
    // SAFETY: the list retains its menu.
    unsafe { list.setMenu(Some(&menu)) };
    let mut order: Vec<&objc2_app_kit::NSView> = vec![&list];
    order.extend(
        buttons
            .iter()
            .map(|button| -> &objc2_app_kit::NSView { button }),
    );
    controls::set_tab_order(&order);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&list));
    *this.ivars().list.borrow_mut() = Some(list.clone());

    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&list));
    app.runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    let dialog = this.ivars().dialog.borrow_mut().take();
    if let Some(dialog) = dialog {
        controller.with_shell(|shell, ui| shell.playback_queue_closed(ui, dialog));
    }
}
