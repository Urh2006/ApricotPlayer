//! The Spotify queue and device dialogs, the macOS counterparts of
//! `spotify_queue_win32.rs` and `spotify_devices_win32.rs`: instructions,
//! the list (focused) and the buttons with Back last in that Tab order,
//! modal over the main window. Enter and a double click run the first
//! button, Delete removes in the queue, Escape and closing go back,
//! Shift+F10 and VoiceOver's menu command open the menu of the selected
//! row. The shell keeps the rows and makes the edits; updates arrive while
//! the dialog is open.

#![allow(unsafe_code)]

use std::cell::RefCell;

use apricot_shell::{SpotifyDialogAction, SpotifyDialogKind, SpotifyDialogRow, SpotifyListDialog};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSScrollView, NSTextField, NSView, NSWindow,
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

thread_local! {
    /// The open dialog, for updates from the Spotify events.
    static OPEN: RefCell<Option<Retained<SpotifyDialog>>> = const { RefCell::new(None) };
}

pub struct DialogIvars {
    controller: Weak<MainWindowController>,
    list: RefCell<Option<Retained<List>>>,
    window: RefCell<Option<Retained<NSWindow>>>,
    keys: RefCell<Vec<Option<String>>>,
    /// The first button: what Enter does.
    primary: SpotifyDialogAction,
    kind: SpotifyDialogKind,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotSpotifyListDialog"]
    #[ivars = DialogIvars]
    struct SpotifyDialog;

    unsafe impl NSObjectProtocol for SpotifyDialog {}

    unsafe impl NSWindowDelegate for SpotifyDialog {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    impl SpotifyDialog {
        #[unsafe(method(closeDialog:))]
        fn close_dialog(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(primary:))]
        fn primary(&self, _sender: Option<&AnyObject>) {
            self.run(self.ivars().primary);
        }

        #[unsafe(method(play:))]
        fn play(&self, _sender: Option<&AnyObject>) {
            self.run(SpotifyDialogAction::Play);
        }

        #[unsafe(method(moveUp:))]
        fn move_up(&self, _sender: Option<&AnyObject>) {
            self.run(SpotifyDialogAction::MoveUp);
        }

        #[unsafe(method(moveDown:))]
        fn move_down(&self, _sender: Option<&AnyObject>) {
            self.run(SpotifyDialogAction::MoveDown);
        }

        #[unsafe(method(remove:))]
        fn remove(&self, _sender: Option<&AnyObject>) {
            if self.ivars().kind == SpotifyDialogKind::Queue {
                self.run(SpotifyDialogAction::Remove);
            }
        }

        #[unsafe(method(clear:))]
        fn clear(&self, _sender: Option<&AnyObject>) {
            self.run(SpotifyDialogAction::Clear);
        }

        #[unsafe(method(transfer:))]
        fn transfer(&self, _sender: Option<&AnyObject>) {
            self.run(SpotifyDialogAction::Transfer);
        }

        #[unsafe(method(contextMenu:))]
        fn context_menu(&self, _sender: Option<&AnyObject>) {
            let selected = self.selected();
            let Some(controller) = self.ivars().controller.load() else {
                return;
            };
            let entries = controller
                .with_shell(|shell, _| shell.spotify_dialog_menu(selected))
                .unwrap_or_default();
            if entries.is_empty() {
                return;
            }
            let window = self.ivars().window.borrow().clone();
            let Some(window) = window else {
                return;
            };
            if let Some(action) = crate::context_menu::show(&window, &entries)
                .and_then(SpotifyDialogAction::from_command)
            {
                self.run(action);
            }
        }
    }
);

impl SpotifyDialog {
    fn selected(&self) -> Option<usize> {
        self.ivars()
            .list
            .borrow()
            .as_ref()
            .and_then(|list| list.selected())
    }

    fn run(&self, action: SpotifyDialogAction) {
        let selected = self.selected();
        let Some(controller) = self.ivars().controller.load() else {
            return;
        };
        let close = controller
            .with_shell(|shell, ui| shell.spotify_dialog_action(ui, action, selected))
            .unwrap_or(false);
        if close {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }
    }

    /// Windows `update`: the same key stays selected, or the same position
    /// when it is gone; an explicit selection wins.
    fn update(&self, rows: Vec<SpotifyDialogRow>, selected: Option<usize>) {
        let Some(list) = self.ivars().list.borrow().clone() else {
            return;
        };
        let index = list.selected().unwrap_or(0);
        let selection = selected.unwrap_or_else(|| {
            let key = self.ivars().keys.borrow().get(index).cloned().flatten();
            key.and_then(|key| {
                rows.iter()
                    .position(|row| row.key.as_deref() == Some(key.as_str()))
            })
            .unwrap_or(index)
        });
        let selection = selection.min(rows.len().saturating_sub(1));
        *self.ivars().keys.borrow_mut() = rows.iter().map(|row| row.key.clone()).collect();
        list.set_rows(
            rows.into_iter().map(|row| row.label).collect(),
            Some(selection),
        );
    }
}

/// New rows for the open dialog (`ShellUi::spotify_dialog_update`).
pub fn update(rows: Vec<SpotifyDialogRow>, selected: Option<usize>) {
    let dialog = OPEN.with(|open| open.borrow().clone());
    if let Some(dialog) = dialog {
        dialog.update(rows, selected);
    }
}

fn action_selector(action: SpotifyDialogAction) -> Sel {
    match action {
        SpotifyDialogAction::Play => sel!(play:),
        SpotifyDialogAction::MoveUp => sel!(moveUp:),
        SpotifyDialogAction::MoveDown => sel!(moveDown:),
        SpotifyDialogAction::Remove => sel!(remove:),
        SpotifyDialogAction::Clear => sel!(clear:),
        SpotifyDialogAction::Transfer => sel!(transfer:),
    }
}

/// The buttons in a row at the bottom (Windows `layout`), Back last.
fn create_buttons(
    mtm: MainThreadMarker,
    content: &NSView,
    target: &AnyObject,
    dialog: &SpotifyListDialog,
) -> Vec<Retained<Button>> {
    let mut buttons: Vec<(&str, Sel)> = dialog
        .buttons
        .iter()
        .map(|(action, label)| (label.as_str(), action_selector(*action)))
        .collect();
    buttons.push((&dialog.back, sel!(closeDialog:)));
    #[allow(clippy::cast_precision_loss)]
    let count = buttons.len() as f64;
    let button_width = ((WIDTH - 2.0 * MARGIN - GAP * (count - 1.0)) / count).min(180.0);
    buttons
        .into_iter()
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

/// Windows `show_queue` and `show_devices` with their dialogs.
pub fn run(controller: &MainWindowController, dialog: &SpotifyListDialog) {
    let mtm = MainThreadMarker::from(controller);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let primary = dialog
        .buttons
        .first()
        .map_or(SpotifyDialogAction::Play, |(action, _)| *action);
    let this: Retained<SpotifyDialog> = {
        let this = SpotifyDialog::alloc(mtm).set_ivars(DialogIvars {
            controller: Weak::from(controller),
            list: RefCell::new(None),
            window: RefCell::new(None),
            keys: RefCell::new(dialog.rows.iter().map(|row| row.key.clone()).collect()),
            primary,
            kind: dialog.kind,
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
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is released by its last owner, this function.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(&dialog.title));
    window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
    let content = window.contentView().expect("a window has a content view");

    let label =
        NSTextField::wrappingLabelWithString(&NSString::from_str(&dialog.instructions), mtm);
    label.setFrame(NSRect::new(
        NSPoint::new(MARGIN, HEIGHT - MARGIN - INSTRUCTIONS_HEIGHT),
        NSSize::new(WIDTH - 2.0 * MARGIN, INSTRUCTIONS_HEIGHT),
    ));
    content.addSubview(&label);
    let list = List::new(mtm, &dialog.title);
    list.set_rows(
        dialog.rows.iter().map(|row| row.label.clone()).collect(),
        Some(dialog.selected),
    );
    list.set_activate_action(target, sel!(primary:));
    list.set_delete_action(sel!(remove:));
    list.set_context_menu_action(sel!(contextMenu:));
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

    let buttons = create_buttons(mtm, &content, target, dialog);
    if let Some(back) = buttons.last() {
        // `IDCANCEL`: Escape goes back.
        back.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    }
    let mut order: Vec<&NSView> = vec![&list];
    order.extend(buttons.iter().map(|button| -> &NSView { button }));
    controls::set_tab_order(&order);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&list));
    *this.ivars().list.borrow_mut() = Some(list.clone());
    *this.ivars().window.borrow_mut() = Some(window.clone());
    OPEN.with(|open| *open.borrow_mut() = Some(this.clone()));

    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&list));
    app.runModalForWindow(&window);
    OPEN.with(|open| open.borrow_mut().take());
    window.setDelegate(None);
    window.orderOut(None);
    *this.ivars().window.borrow_mut() = None;
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    controller.with_shell(|shell, ui| shell.spotify_dialog_closed(ui));
}
