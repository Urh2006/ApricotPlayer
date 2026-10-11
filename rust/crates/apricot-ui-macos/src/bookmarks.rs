//! The bookmarks window, the macOS counterpart of `bookmark_dialog_win32.rs`:
//! the list (focused) with Add, Play, Rename, Delete, Copy timestamp link
//! and Back in that Tab order, modal over the main window. Enter and a
//! double click play, Delete deletes, Escape and closing go back, Shift+F10
//! and VoiceOver's menu command open the context menu. The requests are
//! answered by `apricot_shell::Shell::bookmark_request`.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};

use apricot_shell::{BookmarkDialog, BookmarkEntry, BookmarkRequest, BookmarkResponse};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSMenu, NSMenuItem, NSScrollView, NSWindow,
    NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::controls::{self, Button, List};
use crate::window::MainWindowController;

/// Windows `CreateWindowExW(..., 760, 520, ...)`.
const WIDTH: f64 = 760.0;
const HEIGHT: f64 = 520.0;
const MARGIN: f64 = 12.0;
const BUTTON_HEIGHT: f64 = 32.0;
const BUTTON_WIDTH: f64 = 112.0;

struct Controls {
    list: Retained<List>,
    add: Retained<Button>,
    play: Retained<Button>,
    rename: Retained<Button>,
    delete: Retained<Button>,
    copy: Retained<Button>,
}

pub struct DialogIvars {
    controller: Retained<MainWindowController>,
    dialog: BookmarkDialog,
    entries: RefCell<Vec<BookmarkEntry>>,
    controls: RefCell<Option<Controls>>,
    /// The handler runs at most once at a time (Windows takes it out of the
    /// state while it runs).
    busy: Cell<bool>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotBookmarksDialog"]
    #[ivars = DialogIvars]
    struct BookmarksDialog;

    unsafe impl NSObjectProtocol for BookmarksDialog {}

    unsafe impl NSWindowDelegate for BookmarksDialog {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    impl BookmarksDialog {
        #[unsafe(method(closeDialog:))]
        fn close_dialog(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(addBookmark:))]
        fn add_bookmark(&self, _sender: Option<&AnyObject>) {
            self.dispatch(BookmarkRequest::Add);
        }

        #[unsafe(method(playBookmark:))]
        fn play_bookmark(&self, _sender: Option<&AnyObject>) {
            self.dispatch_selected(BookmarkRequest::Play);
        }

        #[unsafe(method(renameBookmark:))]
        fn rename_bookmark(&self, _sender: Option<&AnyObject>) {
            self.dispatch_selected(BookmarkRequest::Rename);
        }

        #[unsafe(method(deleteBookmark:))]
        fn delete_bookmark(&self, _sender: Option<&AnyObject>) {
            self.dispatch_selected(BookmarkRequest::Delete);
        }

        #[unsafe(method(copyBookmark:))]
        fn copy_bookmark(&self, _sender: Option<&AnyObject>) {
            self.dispatch_selected(BookmarkRequest::Copy);
        }
    }
);

impl BookmarksDialog {
    /// Windows `dispatch_selected`: nothing happens on the empty row.
    fn dispatch_selected(&self, request: fn(String) -> BookmarkRequest) {
        let id = {
            let controls = self.ivars().controls.borrow();
            let entries = self.ivars().entries.borrow();
            controls
                .as_ref()
                .and_then(|controls| controls.list.selected())
                .and_then(|index| entries.get(index))
                .map(|entry| entry.id.clone())
        };
        if let Some(id) = id {
            self.dispatch(request(id));
        }
    }

    /// Windows `dispatch_action`.
    fn dispatch(&self, request: BookmarkRequest) {
        if self.ivars().busy.replace(true) {
            return;
        }
        let current_only = self.ivars().dialog.current_only;
        let response = self
            .ivars()
            .controller
            .with_shell(|shell, ui| shell.bookmark_request(ui, current_only, request));
        self.ivars().busy.set(false);
        match response {
            Some(BookmarkResponse::Refresh {
                entries,
                selected_id,
            }) => {
                *self.ivars().entries.borrow_mut() = entries;
                self.refresh(selected_id.as_deref());
                if let Some(controls) = self.ivars().controls.borrow().as_ref()
                    && let Some(window) = controls.list.window()
                {
                    window.makeFirstResponder(Some(&controls.list));
                }
            }
            Some(BookmarkResponse::Close) => {
                NSApplication::sharedApplication(self.mtm()).stopModal();
            }
            Some(BookmarkResponse::KeepOpen) | None => {}
        }
    }

    /// Windows `refresh_entries`: the empty text as the only row with the
    /// item buttons disabled, or the bookmarks with `selected_id` selected.
    fn refresh(&self, selected_id: Option<&str>) {
        let controls = self.ivars().controls.borrow();
        let Some(controls) = controls.as_ref() else {
            return;
        };
        let entries = self.ivars().entries.borrow();
        let has_entries = !entries.is_empty();
        if has_entries {
            let selected = selected_id
                .and_then(|id| entries.iter().position(|entry| entry.id == id))
                .unwrap_or(0);
            controls.list.set_rows(
                entries.iter().map(|entry| entry.label.clone()).collect(),
                Some(selected),
            );
        } else {
            controls
                .list
                .set_rows(vec![self.ivars().dialog.empty.clone()], Some(0));
        }
        for button in [
            &controls.play,
            &controls.rename,
            &controls.delete,
            &controls.copy,
        ] {
            button.setEnabled(has_entries);
        }
        self.update_menu(controls);
    }

    /// Windows `show_context_menu`: Play, Add, Rename, Delete and Copy with
    /// the disabled buttons greyed out.
    fn update_menu(&self, controls: &Controls) {
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let items: [(&Button, Sel); 5] = [
            (&controls.play, sel!(playBookmark:)),
            (&controls.add, sel!(addBookmark:)),
            (&controls.rename, sel!(renameBookmark:)),
            (&controls.delete, sel!(deleteBookmark:)),
            (&controls.copy, sel!(copyBookmark:)),
        ];
        for (button, action) in items {
            // SAFETY: `initWithTitle:action:keyEquivalent:` of NSMenuItem.
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &button.title(),
                    Some(action),
                    &NSString::from_str(""),
                )
            };
            // SAFETY: the dialog outlives the menu (it is the list's menu).
            unsafe { item.setTarget(Some(self)) };
            item.setEnabled(button.isEnabled());
            menu.addItem(&item);
        }
        // SAFETY: the menu is retained by the list.
        unsafe { controls.list.setMenu(Some(&menu)) };
    }
}

fn create_window(
    mtm: MainThreadMarker,
    dialog: &BookmarksDialog,
    title: &str,
) -> Retained<NSWindow> {
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

/// Windows `show_bookmarks_dialog` and `bookmark_dialog_win32::show`.
pub fn run(controller: &MainWindowController, dialog: BookmarkDialog) {
    let mtm = MainThreadMarker::from(controller);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let return_on_close = dialog.return_on_close;
    let this: Retained<BookmarksDialog> = {
        let entries = dialog.entries.clone();
        let this = BookmarksDialog::alloc(mtm).set_ivars(DialogIvars {
            controller: controller.retain(),
            dialog,
            entries: RefCell::new(entries),
            controls: RefCell::new(None),
            busy: Cell::new(false),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let labels = &this.ivars().dialog;
    let target: &AnyObject = this.as_ref();
    let window = create_window(mtm, &this, &labels.title);
    let content = window.contentView().expect("a window has a content view");

    let list = List::new(mtm, &labels.list_name);
    list.set_activate_action(target, sel!(playBookmark:));
    list.set_delete_action(sel!(deleteBookmark:));
    let list_bottom = MARGIN * 2.0 + BUTTON_HEIGHT;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, list_bottom),
            NSSize::new(WIDTH - 2.0 * MARGIN, HEIGHT - MARGIN - list_bottom),
        ),
    );
    scroll.setDocumentView(Some(&list));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);

    let buttons: Vec<Retained<Button>> = [
        (&labels.add, sel!(addBookmark:)),
        (&labels.play, sel!(playBookmark:)),
        (&labels.rename, sel!(renameBookmark:)),
        (&labels.delete, sel!(deleteBookmark:)),
        (&labels.copy, sel!(copyBookmark:)),
        (&labels.close, sel!(closeDialog:)),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (label, action))| {
        let button = Button::new(mtm, label, target, action);
        #[allow(clippy::cast_precision_loss)]
        let left = MARGIN + index as f64 * (BUTTON_WIDTH + 6.0);
        button.setFrame(NSRect::new(
            NSPoint::new(left, MARGIN),
            NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
        ));
        content.addSubview(&button);
        button
    })
    .collect();
    let [add, play, rename, delete, copy, close] = <[Retained<Button>; 6]>::try_from(buttons)
        .unwrap_or_else(|_| unreachable!("six buttons were created"));
    // `IDCANCEL`: Escape goes back.
    close.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    add.setEnabled(labels.can_add);
    controls::set_tab_order(&[&list, &add, &play, &rename, &delete, &copy, &close]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&list));
    *this.ivars().controls.borrow_mut() = Some(Controls {
        list: list.clone(),
        add,
        play,
        rename,
        delete,
        copy,
    });
    this.refresh(None);

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
    this.ivars().controls.borrow_mut().take();
    controller.with_shell(|shell, ui| shell.bookmarks_closed(ui, return_on_close));
}
