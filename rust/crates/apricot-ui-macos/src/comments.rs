//! The comments window, the macOS counterpart of `comments_win32.rs`: the
//! search field (focused), the sort choice, the comment list and Open,
//! Copy, Copy visible, Author channel, Load more and Back, modal over the
//! main window; and the read-only comment details with Back. Enter and a
//! double click on the list open a comment, Escape and closing go back,
//! Shift+F10 and VoiceOver's menu command open the context menu. The
//! decisions are made by `apricot_shell::CommentsDialog`.

#![allow(unsafe_code)]

use std::cell::RefCell;

use apricot_app::comments::{CommentsListView, LoadFinished};
use apricot_shell::{CommentsCommand, CommentsDialog, CommentsEffect};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBackingStoreType, NSMenu, NSMenuItem, NSScrollView,
    NSTextField, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSRunLoopCommonModes, NSSize,
    NSString, NSTimer,
};

use crate::controls::{self, Button, List, ReadOnlyText};
use crate::form_controls::PopUp;
use crate::window::MainWindowController;

/// Windows `CreateWindowExW(..., 860, 560, ...)`.
const WIDTH: f64 = 860.0;
const HEIGHT: f64 = 560.0;
const MARGIN: f64 = 8.0;
const BUTTON_HEIGHT: f64 = 30.0;
const POLL_SECONDS: f64 = 0.05;

struct Controls {
    search: Retained<NSTextField>,
    list: Retained<List>,
    /// Open, Copy, Copy visible, Author channel, Load more.
    buttons: [Retained<Button>; 5],
}

pub struct DialogIvars {
    controller: objc2::rc::Retained<MainWindowController>,
    dialog: RefCell<Option<CommentsDialog>>,
    controls: RefCell<Option<Controls>>,
    timer: RefCell<Option<Retained<NSTimer>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotCommentsDialog"]
    #[ivars = DialogIvars]
    struct CommentsWindow;

    unsafe impl NSObjectProtocol for CommentsWindow {}

    unsafe impl NSWindowDelegate for CommentsWindow {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    impl CommentsWindow {
        #[unsafe(method(closeDialog:))]
        fn close_dialog(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(openComment:))]
        fn open_comment(&self, _sender: Option<&AnyObject>) {
            self.command(CommentsCommand::Open);
        }

        #[unsafe(method(copyComment:))]
        fn copy_comment(&self, _sender: Option<&AnyObject>) {
            self.command(CommentsCommand::Copy);
        }

        #[unsafe(method(copyVisible:))]
        fn copy_visible(&self, _sender: Option<&AnyObject>) {
            self.command(CommentsCommand::CopyVisible);
        }

        #[unsafe(method(openAuthor:))]
        fn open_author(&self, _sender: Option<&AnyObject>) {
            self.command(CommentsCommand::Author);
        }

        #[unsafe(method(loadMore:))]
        fn load_more(&self, _sender: Option<&AnyObject>) {
            self.command(CommentsCommand::More);
        }

        /// Windows `CBN_SELCHANGE` of the sort choice.
        #[unsafe(method(sortChanged:))]
        fn sort_changed(&self, sender: Option<&AnyObject>) {
            let index = sender
                .and_then(|sender| sender.downcast_ref::<PopUp>())
                .and_then(PopUp::selected)
                .unwrap_or_default();
            let view = self
                .ivars()
                .dialog
                .borrow_mut()
                .as_mut()
                .map(|dialog| dialog.set_sort(index));
            if let Some(view) = view {
                self.apply_view(&view);
            }
        }

        /// Windows `EN_CHANGE` of the search field.
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, _notification: &NSNotification) {
            let query = self
                .ivars()
                .controls
                .borrow()
                .as_ref()
                .map(|controls| controls.search.stringValue().to_string())
                .unwrap_or_default();
            let view = self
                .ivars()
                .dialog
                .borrow_mut()
                .as_mut()
                .map(|dialog| dialog.set_query(&query));
            if let Some(view) = view {
                self.apply_view(&view);
            }
        }

        /// Windows `LBN_SELCHANGE`: Author channel follows the comment.
        #[unsafe(method(selectionChanged:))]
        fn selection_changed(&self, _sender: Option<&AnyObject>) {
            let controls = self.ivars().controls.borrow();
            let Some(controls) = controls.as_ref() else {
                return;
            };
            let enabled = self
                .ivars()
                .dialog
                .borrow()
                .as_ref()
                .is_some_and(|dialog| dialog.author_enabled(controls.list.selected()));
            controls.buttons[3].setEnabled(enabled);
        }

        /// Python `show_comments_context_menu`.
        #[unsafe(method(contextMenu:))]
        fn context_menu(&self, _sender: Option<&AnyObject>) {
            self.show_context_menu();
        }

        /// Windows `WM_TIMER`: Python `finish_load`.
        #[unsafe(method(poll:))]
        fn poll(&self, _timer: Option<&AnyObject>) {
            let finished = self
                .ivars()
                .dialog
                .borrow_mut()
                .as_mut()
                .and_then(CommentsDialog::poll);
            match finished {
                Some(LoadFinished::Failed {
                    label,
                    announcement,
                }) => {
                    if let Some(controls) = self.ivars().controls.borrow().as_ref() {
                        controls.list.set_rows(vec![label], Some(0));
                        controls.buttons[4].setEnabled(false);
                    }
                    crate::announce::announce(self.mtm(), &announcement, true);
                }
                Some(LoadFinished::Loaded { view, announcement }) => {
                    self.apply_view(&view);
                    crate::announce::announce(self.mtm(), &announcement, true);
                }
                None => {}
            }
        }
    }
);

impl CommentsWindow {
    /// Python `refresh_comments` applied to the controls.
    fn apply_view(&self, view: &CommentsListView) {
        let controls = self.ivars().controls.borrow();
        let Some(controls) = controls.as_ref() else {
            return;
        };
        controls
            .list
            .set_rows(view.labels.clone(), Some(view.selection));
        for button in &controls.buttons[..3] {
            button.setEnabled(view.has_comments);
        }
        controls.buttons[3].setEnabled(view.author_enabled);
        controls.buttons[4].setEnabled(view.more_enabled);
    }

    /// Windows `command`.
    fn command(&self, command: CommentsCommand) {
        let row = self
            .ivars()
            .controls
            .borrow()
            .as_ref()
            .and_then(|controls| controls.list.selected());
        let effect = {
            let mut dialog = self.ivars().dialog.borrow_mut();
            let Some(dialog) = dialog.as_mut() else {
                return;
            };
            self.ivars()
                .controller
                .with_shell(|shell, ui| shell.comments_command(ui, dialog, command, row))
                .unwrap_or(CommentsEffect::None)
        };
        match effect {
            CommentsEffect::None => {}
            CommentsEffect::Announce(text) => crate::announce::announce(self.mtm(), &text, true),
            CommentsEffect::Loading(label) => {
                if let Some(controls) = self.ivars().controls.borrow().as_ref() {
                    controls.list.set_rows(vec![label], Some(0));
                    controls.buttons[4].setEnabled(false);
                }
            }
            CommentsEffect::Details { title, back, text } => {
                show_details(self.mtm(), &title, &back, &text);
            }
        }
        if command == CommentsCommand::More
            && let Some(controls) = self.ivars().controls.borrow().as_ref()
        {
            // Windows `load_more` disables Load more while a page loads.
            let more = self
                .ivars()
                .dialog
                .borrow()
                .as_ref()
                .is_some_and(CommentsDialog::more_enabled);
            controls.buttons[4].setEnabled(more);
        }
    }

    /// Python `show_comments_context_menu`: Open, Copy, Copy visible,
    /// Author channel, a separator and Load more, greyed when unavailable.
    fn show_context_menu(&self) {
        let mtm = self.mtm();
        let Some(list) = self
            .ivars()
            .controls
            .borrow()
            .as_ref()
            .map(|controls| controls.list.clone())
        else {
            return;
        };
        // The entries are read first: a chosen item borrows the dialog.
        let entries: Vec<(String, Sel, bool)> = {
            let dialog = self.ivars().dialog.borrow();
            let Some(dialog) = dialog.as_ref() else {
                return;
            };
            let has_comments = dialog.has_comments();
            vec![
                (
                    dialog.text("open_comment"),
                    sel!(openComment:),
                    has_comments,
                ),
                (
                    dialog.text("copy_comment"),
                    sel!(copyComment:),
                    has_comments,
                ),
                (
                    dialog.text("copy_visible_comments"),
                    sel!(copyVisible:),
                    has_comments,
                ),
                (
                    dialog.text("open_comment_author_channel"),
                    sel!(openAuthor:),
                    dialog.author_enabled(list.selected()),
                ),
                (
                    dialog.text("load_more_comments"),
                    sel!(loadMore:),
                    dialog.more_enabled(),
                ),
            ]
        };
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        for (index, (title, action, enabled)) in entries.into_iter().enumerate() {
            if index == 4 {
                menu.addItem(&NSMenuItem::separatorItem(mtm));
            }
            // SAFETY: `initWithTitle:action:keyEquivalent:` of NSMenuItem.
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(&title),
                    Some(action),
                    &NSString::from_str(""),
                )
            };
            // SAFETY: the dialog outlives the menu.
            unsafe { item.setTarget(Some(self)) };
            item.setEnabled(enabled);
            menu.addItem(&item);
        }
        let row = list.selectedRow().max(0);
        let rect = list.rectOfRow(row);
        let point = NSPoint::new(rect.origin.x + 16.0, rect.origin.y + rect.size.height);
        menu.popUpMenuPositioningItem_atLocation_inView(None, point, Some(&list));
    }
}

/// Python `show_comment_details`: the read-only text (focused) with Back;
/// Escape and closing go back.
fn show_details(mtm: MainThreadMarker, title: &str, back: &str, text: &str) {
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let (width, height) = (640.0, 460.0);
    // SAFETY: a standard titled window with a buffered backing.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(240.0, 240.0), NSSize::new(width, height)),
            NSWindowStyleMask::Titled,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: the window is released by its last owner, this function.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(title));
    let content = window.contentView().expect("a window has a content view");
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(12.0, 54.0),
            NSSize::new(width - 24.0, height - 66.0),
        ),
    );
    let body = ReadOnlyText::new(mtm);
    body.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(width - 24.0, height - 66.0),
    ));
    body.set_label(title);
    body.set_text(text);
    scroll.setDocumentView(Some(&body));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);
    let stopper = crate::language::ModalStopper::new(mtm);
    let back_button = Button::new(mtm, back, stopper.target(), sel!(stop:));
    back_button.setFrame(NSRect::new(
        NSPoint::new(12.0, 12.0),
        NSSize::new(130.0, 30.0),
    ));
    back_button.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&back_button);
    controls::set_tab_order(&[&*body, &*back_button]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&body));
    app.runModalForWindow(&window);
    window.orderOut(None);
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
}

fn create_window(
    mtm: MainThreadMarker,
    dialog: &CommentsWindow,
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

/// Windows `create_controls` and `layout`.
fn create_controls(
    mtm: MainThreadMarker,
    content: &NSView,
    target: &AnyObject,
    dialog: &CommentsDialog,
) -> Controls {
    let width = WIDTH - 2.0 * MARGIN;
    let sort_width = 200.0_f64.min(width / 3.0);
    let top = HEIGHT - MARGIN - 26.0;
    let search = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, top),
            NSSize::new(width - sort_width - 8.0, 26.0),
        ),
    );
    search.setAccessibilityLabel(Some(&NSString::from_str(&dialog.text("search_comments"))));
    // SAFETY: the dialog outlives the field and receives its text changes.
    unsafe {
        let _: () = msg_send![&search, setDelegate: target];
    }
    content.addSubview(&search);
    let sort = PopUp::new(
        mtm,
        &dialog.text("comments_sort"),
        target,
        sel!(sortChanged:),
    );
    sort.set_items(&dialog.sort_labels(), 0);
    sort.setFrame(NSRect::new(
        NSPoint::new(MARGIN + width - sort_width, top),
        NSSize::new(sort_width, 26.0),
    ));
    content.addSubview(&sort);
    let list = List::new(mtm, &dialog.text("comments"));
    list.set_rows(vec![dialog.text("comments_loading")], Some(0));
    list.set_activate_action(target, sel!(openComment:));
    list.set_selection_action(sel!(selectionChanged:));
    list.set_context_menu_action(sel!(contextMenu:));
    let list_bottom = MARGIN * 2.0 + BUTTON_HEIGHT;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, list_bottom),
            NSSize::new(width, top - 8.0 - list_bottom),
        ),
    );
    scroll.setDocumentView(Some(&list));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);
    let specs: [(&str, Sel); 6] = [
        ("open_comment", sel!(openComment:)),
        ("copy_comment", sel!(copyComment:)),
        ("copy_visible_comments", sel!(copyVisible:)),
        ("open_comment_author_channel", sel!(openAuthor:)),
        ("load_more_comments", sel!(loadMore:)),
        ("back", sel!(closeDialog:)),
    ];
    let button_width = (width - 40.0) / 6.0;
    let buttons: Vec<Retained<Button>> = specs
        .iter()
        .enumerate()
        .map(|(index, (key, action))| {
            let button = Button::new(mtm, &dialog.text(key), target, *action);
            #[allow(clippy::cast_precision_loss)]
            let left = MARGIN + index as f64 * (button_width + 8.0);
            button.setFrame(NSRect::new(
                NSPoint::new(left, MARGIN),
                NSSize::new(button_width, BUTTON_HEIGHT),
            ));
            // Python: every button but Back starts disabled.
            button.setEnabled(index == 5);
            content.addSubview(&button);
            button
        })
        .collect();
    let [open, copy, copy_visible, author, more, back] = <[Retained<Button>; 6]>::try_from(buttons)
        .unwrap_or_else(|_| unreachable!("six buttons were created"));
    // Python's Back button is `wx.ID_CANCEL`.
    back.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    controls::set_tab_order(&[
        &*search,
        &*sort,
        &*list,
        &*open,
        &*copy,
        &*copy_visible,
        &*author,
        &*more,
        &*back,
    ]);
    // The window keeps Back and the sort choice alive through its views.
    let _ = (sort, back);
    Controls {
        search,
        list,
        buttons: [open, copy, copy_visible, author, more],
    }
}

/// Windows `show_player_comments` and `comments_win32::show`.
pub fn run(controller: &MainWindowController, mut dialog: CommentsDialog) {
    let mtm = MainThreadMarker::from(controller);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let title = dialog.text("comments");
    // Python: the list says "Loading comments..." and `load_more()` starts
    // the first page.
    let _ = dialog.load_more();
    let this: Retained<CommentsWindow> = {
        let this = CommentsWindow::alloc(mtm).set_ivars(DialogIvars {
            controller: objc2::Message::retain(controller),
            dialog: RefCell::new(None),
            controls: RefCell::new(None),
            timer: RefCell::new(None),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let window = create_window(mtm, &this, &title);
    let content = window.contentView().expect("a window has a content view");
    let controls = create_controls(mtm, &content, this.as_ref(), &dialog);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&controls.search));
    let initial_focus = controls.search.clone();
    *this.ivars().controls.borrow_mut() = Some(controls);
    *this.ivars().dialog.borrow_mut() = Some(dialog);

    // SAFETY: the dialog implements `poll:` and outlives the timer, which is
    // invalidated when the window closes.
    let timer = unsafe {
        NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
            POLL_SECONDS,
            &this,
            sel!(poll:),
            None,
            true,
        )
    };
    // SAFETY: AppKit's static run loop mode, so the timer runs modally.
    unsafe {
        objc2_foundation::NSRunLoop::currentRunLoop()
            .addTimer_forMode(&timer, NSRunLoopCommonModes);
    }
    *this.ivars().timer.borrow_mut() = Some(timer);

    app.activate();
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&initial_focus));
    app.runModalForWindow(&window);
    if let Some(timer) = this.ivars().timer.borrow_mut().take() {
        timer.invalidate();
    }
    window.setDelegate(None);
    window.orderOut(None);
    if let Some(previous) = previous_window {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    this.ivars().controls.borrow_mut().take();
    this.ivars().dialog.borrow_mut().take();
    controller.with_shell(|shell, ui| shell.comments_closed(ui));
}
