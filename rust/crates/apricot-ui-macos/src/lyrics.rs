//! The lyrics window, the macOS counterpart of `details_win32::show_async`:
//! a read-only text (focused) with Copy and Back and a status line, modal
//! over the main window. Escape and closing go back. The text first says
//! that the lyrics are loading; when they arrive they replace it, get focus
//! and the title (or "No lyrics available") is announced. The timed line at
//! the playback position is highlighted, as on Windows only visually.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ops::Range;

use apricot_shell::LyricsDialog;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackgroundColorAttributeName, NSBackingStoreType, NSColor, NSScrollView,
    NSTextField, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect, NSRunLoopCommonModes, NSSize, NSString,
    NSTimer,
};

use crate::controls::{self, Button, ReadOnlyText};
use crate::window::MainWindowController;

/// Windows `CreateWindowExW(..., 620, 460, ...)`.
const WIDTH: f64 = 620.0;
const HEIGHT: f64 = 460.0;
const MARGIN: f64 = 12.0;
const GAP: f64 = 8.0;
const BUTTON_HEIGHT: f64 = 30.0;
const BUTTON_WIDTH: f64 = 130.0;
const STATUS_HEIGHT: f64 = 24.0;
/// Windows polls the worker every 50 ms and the position every 200 ms.
const POLL_SECONDS: f64 = 0.05;

pub struct DialogIvars {
    dialog: RefCell<LyricsDialog>,
    text: RefCell<Option<Retained<ReadOnlyText>>>,
    status: RefCell<Option<Retained<NSTextField>>>,
    timer: RefCell<Option<Retained<NSTimer>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotLyricsDialog"]
    #[ivars = DialogIvars]
    struct LyricsWindow;

    unsafe impl NSObjectProtocol for LyricsWindow {}

    unsafe impl NSWindowDelegate for LyricsWindow {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    impl LyricsWindow {
        #[unsafe(method(closeDialog:))]
        fn close_dialog(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        /// Windows `copy_details`.
        #[unsafe(method(copyText:))]
        fn copy_text(&self, _sender: Option<&AnyObject>) {
            let message = match self.ivars().dialog.borrow().copy() {
                Ok((text, copied)) => {
                    let pasteboard = objc2_app_kit::NSPasteboard::generalPasteboard();
                    pasteboard.clearContents();
                    // SAFETY: AppKit's static string pasteboard type.
                    // macOS text uses LF line breaks.
                    let text = text.replace("\r\n", "\n");
                    let done = pasteboard.setString_forType(&NSString::from_str(&text), unsafe {
                        objc2_app_kit::NSPasteboardTypeString
                    });
                    done.then_some(copied)
                }
                Err(unavailable) => Some(unavailable),
            };
            if let Some(message) = message {
                self.announce(&message);
            }
        }

        /// Windows timers 1 (`poll_text`) and 2 (`highlight_lyrics`).
        #[unsafe(method(poll:))]
        fn poll(&self, _timer: Option<&AnyObject>) {
            self.poll_dialog();
        }
    }
);

impl LyricsWindow {
    /// Windows `WindowsAnnouncer` on the status line.
    fn announce(&self, message: &str) {
        if let Some(status) = self.ivars().status.borrow().as_ref() {
            status.setStringValue(&NSString::from_str(message));
        }
        crate::announce::announce(self.mtm(), message, true);
    }

    fn poll_dialog(&self) {
        let loaded = self.ivars().dialog.borrow_mut().poll_loaded();
        if let Some(loaded) = loaded
            && let Some(text) = self.ivars().text.borrow().as_ref()
        {
            text.set_text(&loaded.text);
            if let Some(window) = text.window() {
                window.makeFirstResponder(Some(text));
            }
            self.announce(&loaded.announcement);
        }
        let highlight = self.ivars().dialog.borrow_mut().highlight();
        if let Some(highlight) = highlight {
            self.mark(highlight.previous, false);
            self.mark(highlight.current, true);
        }
        if !self.ivars().dialog.borrow().is_live()
            && let Some(timer) = self.ivars().timer.borrow_mut().take()
        {
            timer.invalidate();
        }
    }

    /// Marks or unmarks one line by its background, keeping the caret.
    fn mark(&self, range: Option<Range<usize>>, active: bool) {
        let Some(range) = range else {
            return;
        };
        let text = self.ivars().text.borrow();
        let Some(text) = text.as_ref() else {
            return;
        };
        // SAFETY: a text view always has a text storage.
        let Some(storage) = (unsafe { text.textStorage() }) else {
            return;
        };
        let range = NSRange::new(range.start, range.end.saturating_sub(range.start));
        if range.end() > storage.length() {
            return;
        }
        // SAFETY: AppKit's static attribute name on a range inside the text.
        unsafe {
            if active {
                storage.addAttribute_value_range(
                    NSBackgroundColorAttributeName,
                    &NSColor::selectedTextBackgroundColor(),
                    range,
                );
                text.scrollRangeToVisible(range);
            } else {
                storage.removeAttribute_range(NSBackgroundColorAttributeName, range);
            }
        }
    }
}

fn create_window(mtm: MainThreadMarker, dialog: &LyricsWindow, title: &str) -> Retained<NSWindow> {
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

/// Windows `show_player_lyrics` and `details_win32::show_async`.
pub fn run(controller: &MainWindowController, dialog: LyricsDialog) {
    let mtm = MainThreadMarker::from(controller);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let (title, copy_label, back_label, loading) = (
        dialog.title.clone(),
        dialog.copy.clone(),
        dialog.back.clone(),
        dialog.loading.clone(),
    );
    let this: Retained<LyricsWindow> = {
        let this = LyricsWindow::alloc(mtm).set_ivars(DialogIvars {
            dialog: RefCell::new(dialog),
            text: RefCell::new(None),
            status: RefCell::new(None),
            timer: RefCell::new(None),
        });
        // SAFETY: `init` of NSObject.
        unsafe { msg_send![super(this), init] }
    };
    let target: &AnyObject = this.as_ref();
    let window = create_window(mtm, &this, &title);
    let content = window.contentView().expect("a window has a content view");

    let text_height = HEIGHT - MARGIN * 2.0 - GAP * 2.0 - BUTTON_HEIGHT - STATUS_HEIGHT;
    let button_y = MARGIN + STATUS_HEIGHT + GAP;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, button_y + BUTTON_HEIGHT + GAP),
            NSSize::new(WIDTH - 2.0 * MARGIN, text_height),
        ),
    );
    let text = ReadOnlyText::new(mtm);
    text.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(WIDTH - 2.0 * MARGIN, text_height),
    ));
    text.set_label(&title);
    text.set_text(&loading);
    scroll.setDocumentView(Some(&text));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);
    let copy = Button::new(mtm, &copy_label, target, sel!(copyText:));
    copy.setFrame(NSRect::new(
        NSPoint::new(MARGIN, button_y),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    content.addSubview(&copy);
    let back = Button::new(mtm, &back_label, target, sel!(closeDialog:));
    back.setFrame(NSRect::new(
        NSPoint::new(MARGIN + BUTTON_WIDTH + GAP, button_y),
        NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
    ));
    // Windows closes the window on Escape before the dialog manager.
    back.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    content.addSubview(&back);
    let status = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    status.setFrame(NSRect::new(
        NSPoint::new(MARGIN, MARGIN),
        NSSize::new(WIDTH - 2.0 * MARGIN, STATUS_HEIGHT),
    ));
    content.addSubview(&status);
    controls::set_tab_order(&[&*text, &copy, &back]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&text));
    *this.ivars().text.borrow_mut() = Some(text.clone());
    *this.ivars().status.borrow_mut() = Some(status);

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
    window.makeFirstResponder(Some(&text));
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
    controller.with_shell(|shell, ui| shell.lyrics_closed(ui));
}
