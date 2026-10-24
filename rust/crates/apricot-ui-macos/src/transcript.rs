//! The transcript window, the macOS counterpart of `transcript_win32.rs`:
//! the search field (focused), the cue list, Play, Copy line, Copy
//! transcript, Copy timestamp link and Back, modal over the main window.
//! Typing filters the cues, Enter in the field or the list and a double
//! click play the selected cue, Escape and closing go back. The decisions
//! are made by `apricot_shell::TranscriptDialog`.

#![allow(unsafe_code)]

use std::cell::RefCell;

use apricot_shell::{TranscriptButton, TranscriptDialog};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBackingStoreType, NSScrollView, NSTextField, NSView,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSRunLoopCommonModes, NSSize,
    NSString, NSTimer,
};

use crate::controls::{self, Button, List};
use crate::window::MainWindowController;

/// Windows `CreateWindowExW(..., 760, 560, ...)`.
const WIDTH: f64 = 760.0;
const HEIGHT: f64 = 560.0;
const MARGIN: f64 = 12.0;
const BUTTON_HEIGHT: f64 = 30.0;
/// Windows polls the worker every 50 ms.
const POLL_SECONDS: f64 = 0.05;

struct Controls {
    search: Retained<NSTextField>,
    list: Retained<List>,
    play: Retained<Button>,
    copy_line: Retained<Button>,
    copy_all: Retained<Button>,
    copy_timestamp: Retained<Button>,
}

pub struct DialogIvars {
    controller: Retained<MainWindowController>,
    dialog: RefCell<Option<TranscriptDialog>>,
    controls: RefCell<Option<Controls>>,
    timer: RefCell<Option<Retained<NSTimer>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotTranscriptDialog"]
    #[ivars = DialogIvars]
    struct TranscriptWindow;

    unsafe impl NSObjectProtocol for TranscriptWindow {}

    unsafe impl NSWindowDelegate for TranscriptWindow {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            NSApplication::sharedApplication(self.mtm()).stopModal();
            false
        }
    }

    impl TranscriptWindow {
        #[unsafe(method(closeDialog:))]
        fn close_dialog(&self, _sender: Option<&AnyObject>) {
            NSApplication::sharedApplication(self.mtm()).stopModal();
        }

        #[unsafe(method(playLine:))]
        fn play_line(&self, _sender: Option<&AnyObject>) {
            self.press(TranscriptButton::Play);
        }

        #[unsafe(method(copyLine:))]
        fn copy_line(&self, _sender: Option<&AnyObject>) {
            self.press(TranscriptButton::CopyLine);
        }

        #[unsafe(method(copyAll:))]
        fn copy_all(&self, _sender: Option<&AnyObject>) {
            self.press(TranscriptButton::CopyAll);
        }

        #[unsafe(method(copyTimestamp:))]
        fn copy_timestamp(&self, _sender: Option<&AnyObject>) {
            self.press(TranscriptButton::CopyTimestamp);
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
            if let Some(dialog) = self.ivars().dialog.borrow_mut().as_mut() {
                dialog.set_query(&query);
            }
            self.refresh();
        }

        /// Windows `WM_TIMER`.
        #[unsafe(method(poll:))]
        fn poll(&self, _timer: Option<&AnyObject>) {
            let announcement = self
                .ivars()
                .dialog
                .borrow_mut()
                .as_mut()
                .and_then(TranscriptDialog::poll);
            if let Some(announcement) = announcement {
                if let Some(timer) = self.ivars().timer.borrow_mut().take() {
                    timer.invalidate();
                }
                self.refresh();
                crate::announce::announce(self.mtm(), &announcement, true);
            }
        }
    }
);

impl TranscriptWindow {
    /// Windows `refresh`: the rows with the first selected and the
    /// buttons' enabled states.
    fn refresh(&self) {
        let dialog = self.ivars().dialog.borrow();
        let controls = self.ivars().controls.borrow();
        let (Some(dialog), Some(controls)) = (dialog.as_ref(), controls.as_ref()) else {
            return;
        };
        let rows = dialog.rows();
        controls.list.set_rows(rows.rows, Some(0));
        controls.play.setEnabled(rows.line_buttons);
        controls.copy_line.setEnabled(rows.line_buttons);
        controls.copy_all.setEnabled(rows.copy_all);
        controls.copy_timestamp.setEnabled(rows.copy_timestamp);
    }

    /// Windows `command`.
    fn press(&self, button: TranscriptButton) {
        let row = self
            .ivars()
            .controls
            .borrow()
            .as_ref()
            .and_then(|controls| controls.list.selected());
        let announcement = {
            let dialog = self.ivars().dialog.borrow();
            let Some(dialog) = dialog.as_ref() else {
                return;
            };
            self.ivars()
                .controller
                .with_shell(|shell, ui| shell.transcript_button(ui, dialog, button, row))
                .flatten()
        };
        if let Some(announcement) = announcement {
            crate::announce::announce(self.mtm(), &announcement, true);
        }
    }
}

fn create_window(
    mtm: MainThreadMarker,
    dialog: &TranscriptWindow,
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

/// Windows `layout`: the field on top, the list, the five buttons.
fn create_controls(
    mtm: MainThreadMarker,
    content: &NSView,
    target: &AnyObject,
    dialog: &TranscriptDialog,
) -> (Controls, Retained<Button>) {
    let width = WIDTH - 2.0 * MARGIN;
    let search = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, HEIGHT - MARGIN - 28.0),
            NSSize::new(width, 28.0),
        ),
    );
    search.setAccessibilityLabel(Some(&NSString::from_str(
        &dialog.label("transcript_search"),
    )));
    // SAFETY: the dialog outlives the field; Return plays the cue.
    unsafe {
        search.setTarget(Some(target));
        search.setAction(Some(sel!(playLine:)));
        let _: () = msg_send![&search, setDelegate: target];
    }
    content.addSubview(&search);
    let list = List::new(mtm, &dialog.label("transcript"));
    list.set_activate_action(target, sel!(playLine:));
    let list_bottom = MARGIN * 2.0 + BUTTON_HEIGHT;
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(
            NSPoint::new(MARGIN, list_bottom),
            NSSize::new(width, HEIGHT - MARGIN * 2.0 - 28.0 - list_bottom),
        ),
    );
    scroll.setDocumentView(Some(&list));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);
    let specs: [(&str, Sel); 5] = [
        ("play", sel!(playLine:)),
        ("copy_transcript_line", sel!(copyLine:)),
        ("copy_transcript", sel!(copyAll:)),
        ("copy_timestamp_link", sel!(copyTimestamp:)),
        ("back", sel!(closeDialog:)),
    ];
    let button_width = (width - 32.0) / 5.0;
    let buttons: Vec<Retained<Button>> = specs
        .iter()
        .enumerate()
        .map(|(index, (key, action))| {
            let button = Button::new(mtm, &dialog.label(key), target, *action);
            #[allow(clippy::cast_precision_loss)]
            let left = MARGIN + index as f64 * (button_width + 8.0);
            button.setFrame(NSRect::new(
                NSPoint::new(left, MARGIN),
                NSSize::new(button_width, BUTTON_HEIGHT),
            ));
            content.addSubview(&button);
            button
        })
        .collect();
    let [play, copy_line, copy_all, copy_timestamp, back] =
        <[Retained<Button>; 5]>::try_from(buttons)
            .unwrap_or_else(|_| unreachable!("five buttons were created"));
    // Windows closes the window on Escape before the dialog manager.
    back.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    controls::set_tab_order(&[
        &*search,
        &*list,
        &*play,
        &*copy_line,
        &*copy_all,
        &*copy_timestamp,
        &*back,
    ]);
    (
        Controls {
            search,
            list,
            play,
            copy_line,
            copy_all,
            copy_timestamp,
        },
        back,
    )
}

/// Windows `show_player_transcript` and `transcript_win32::show`.
pub fn run(controller: &MainWindowController, dialog: TranscriptDialog) {
    let mtm = MainThreadMarker::from(controller);
    let app = NSApplication::sharedApplication(mtm);
    let previous_window = app.keyWindow();
    let previous_focus = previous_window
        .as_ref()
        .and_then(|window| window.firstResponder());
    let title = dialog.label("transcript");
    let this: Retained<TranscriptWindow> = {
        let this = TranscriptWindow::alloc(mtm).set_ivars(DialogIvars {
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
    let (controls, _back) = create_controls(mtm, &content, this.as_ref(), &dialog);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&controls.search));
    let initial_focus = controls.search.clone();
    *this.ivars().controls.borrow_mut() = Some(controls);
    *this.ivars().dialog.borrow_mut() = Some(dialog);
    this.refresh();

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
    let dialog = this.ivars().dialog.borrow_mut().take();
    if let Some(dialog) = dialog {
        controller.with_shell(|shell, ui| shell.transcript_closed(ui, dialog));
    }
}
