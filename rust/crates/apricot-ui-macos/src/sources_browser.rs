//! Native source browser. VoiceOver and keyboard context actions share one menu.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.
#![allow(unsafe_code)]

use crate::controls::{self, Button, List};
use apricot_shell::sources::{SourceBrowser, SourceSelection};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSEvent, NSMenu, NSMenuItem, NSModalResponseCancel,
    NSModalResponseOK, NSResponder, NSScrollView, NSTextField, NSWindow, NSWindowDelegate,
    NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;

#[derive(Default)]
pub struct DialogIvars {
    model: RefCell<Option<SourceBrowser>>,
    list: RefCell<Option<Retained<List>>>,
    result: RefCell<Option<SourceSelection>>,
}
define_class!(
    // SAFETY: NSObject supports subclassing and the modal loop owns the target.
    #[unsafe(super(NSObject))]
    #[thread_kind=MainThreadOnly]
    #[name="ApricotSourcesTarget"]
    #[ivars=DialogIvars]
    struct Dialog;
    unsafe impl NSObjectProtocol for Dialog {}
    unsafe impl NSWindowDelegate for Dialog {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self,_:&NSWindow)->bool {NSApplication::sharedApplication(self.mtm()).stopModalWithCode(NSModalResponseCancel);false}
    }
    impl Dialog {
        #[unsafe(method(open:))]
        fn open(&self,_:Option<&AnyObject>){let id=self.ivars().model.borrow().as_ref().and_then(|model|model.actions.first().map(|action|action.id.to_owned()));if let Some(id)=id{self.finish(id)}}
        #[unsafe(method(back:))]
        fn back(&self,_:Option<&AnyObject>){NSApplication::sharedApplication(self.mtm()).stopModalWithCode(NSModalResponseCancel);}
        #[unsafe(method(actions:))]
        fn actions(&self,_:Option<&AnyObject>){self.menu();}
        #[unsafe(method(chosen:))]
        fn chosen(&self,item:Option<&NSMenuItem>){let id=item.and_then(|item|usize::try_from(item.tag()).ok()).and_then(|index|self.ivars().model.borrow().as_ref().and_then(|model|model.actions.get(index).map(|action|action.id.to_owned())));if let Some(id)=id{self.finish(id)}}
    }
);

impl Dialog {
    fn finish(&self, action: String) {
        let index = self
            .ivars()
            .list
            .borrow()
            .as_ref()
            .and_then(|list| list.selected())
            .unwrap_or_default();
        *self.ivars().result.borrow_mut() = Some(SourceSelection { index, action });
        NSApplication::sharedApplication(self.mtm()).stopModalWithCode(NSModalResponseOK);
    }
    fn shortcut(&self, event: &NSEvent) -> bool {
        let Some(characters) = event.charactersIgnoringModifiers() else {
            return false;
        };
        let Some(chord) = crate::keys::chord(
            event.keyCode(),
            &characters.to_string(),
            controls::modifiers(event),
        ) else {
            return false;
        };
        let action = self.ivars().model.borrow().as_ref().and_then(|model| {
            model
                .actions
                .iter()
                .find(|action| {
                    !action.shortcut.is_empty() && chord.matches_configured(&action.shortcut)
                })
                .map(|action| action.id.to_owned())
        });
        if let Some(action) = action {
            self.finish(action);
            true
        } else {
            false
        }
    }
    fn menu(&self) {
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let Some(model) = self.ivars().model.borrow().clone() else {
            return;
        };
        for (index, action) in model.actions.iter().enumerate() {
            let label = if action.shortcut.is_empty() {
                action.label.clone()
            } else {
                format!(
                    "{}  {}",
                    action.label,
                    apricot_core::shortcut::display_shortcut(&action.shortcut)
                )
            };
            // SAFETY: the menu and target remain live until the native popup exits.
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(&label),
                    Some(sel!(chosen:)),
                    &NSString::from_str(""),
                )
            };
            item.setTag(index as isize);
            unsafe { item.setTarget(Some(self)) };
            menu.addItem(&item);
        }
        if let Some(list) = self.ivars().list.borrow().as_ref() {
            let rect = list.rectOfRow(list.selectedRow().max(0));
            menu.popUpMenuPositioningItem_atLocation_inView(
                None,
                NSPoint::new(16.0, rect.origin.y + rect.size.height),
                Some(&**list),
            );
        }
    }
}

#[derive(Default)]
pub struct WindowIvars {
    target: RefCell<Option<Weak<Dialog>>>,
}
define_class!(
    // SAFETY: NSWindow supports subclassing; the target is held weakly.
    #[unsafe(super(NSWindow,NSResponder,NSObject))]
    #[thread_kind=MainThreadOnly]
    #[name="ApricotSourcesWindow"]
    #[ivars=WindowIvars]
    struct SourcesWindow;
    impl SourcesWindow {
        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self,event:&NSEvent)->bool {
            if self.ivars().target.borrow().as_ref().and_then(Weak::load).is_some_and(|target|target.shortcut(event)){return true}
            // SAFETY: forwarding the same event to NSWindow.
            unsafe{msg_send![super(self),performKeyEquivalent:event]}
        }
    }
);

pub(crate) fn show(mtm: MainThreadMarker, model: &SourceBrowser) -> Option<SourceSelection> {
    let app = NSApplication::sharedApplication(mtm);
    let previous = app.keyWindow();
    let previous_focus = previous.as_ref().and_then(|window| window.firstResponder());
    let target: Retained<Dialog> = {
        let this = Dialog::alloc(mtm).set_ivars(DialogIvars::default());
        unsafe { msg_send![super(this), init] }
    };
    *target.ivars().model.borrow_mut() = Some(model.clone());
    let this = SourcesWindow::alloc(mtm).set_ivars(WindowIvars::default());
    // SAFETY: standard native window initialization on AppKit's main thread.
    let window: Retained<SourcesWindow> = unsafe {
        msg_send![super(this),initWithContentRect:NSRect::new(NSPoint::new(100.0,100.0),NSSize::new(820.0,560.0)),styleMask:NSWindowStyleMask::Titled|NSWindowStyleMask::Closable,backing:NSBackingStoreType::Buffered,defer:false]
    };
    *window.ivars().target.borrow_mut() = Some(Weak::from(&*target));
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str(&model.title));
    window.setDelegate(Some(ProtocolObject::from_ref(&*target)));
    let content = window.contentView()?;
    let prompt = NSTextField::labelWithString(&NSString::from_str(&model.prompt), mtm);
    prompt.setFrame(NSRect::new(
        NSPoint::new(12.0, 510.0),
        NSSize::new(796.0, 38.0),
    ));
    content.addSubview(&prompt);
    let list = List::new(mtm, &model.title);
    list.set_rows(
        model.rows.clone(),
        (!model.rows.is_empty()).then_some(model.selected.min(model.rows.len().saturating_sub(1))),
    );
    list.set_activate_action(&target, sel!(open:));
    list.set_context_menu_action(sel!(actions:));
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(NSPoint::new(12.0, 60.0), NSSize::new(796.0, 440.0)),
    );
    scroll.setDocumentView(Some(&list));
    scroll.setHasVerticalScroller(true);
    content.addSubview(&scroll);
    *target.ivars().list.borrow_mut() = Some(list.clone());
    let open = Button::new(
        mtm,
        model.actions.first().map_or("Open", |action| &action.label),
        &target,
        sel!(open:),
    );
    let actions = Button::new(mtm, "Actions", &target, sel!(actions:));
    let back = Button::new(mtm, "Back", &target, sel!(back:));
    back.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    for (index, button) in [&open, &actions, &back].into_iter().enumerate() {
        button.setFrame(NSRect::new(
            NSPoint::new(12.0 + index as f64 * 180.0, 12.0),
            NSSize::new(168.0, 34.0),
        ));
        content.addSubview(&**button);
    }
    controls::set_tab_order(&[&list, &open, &actions, &back]);
    window.setAutorecalculatesKeyViewLoop(false);
    window.setInitialFirstResponder(Some(&list));
    window.center();
    window.makeKeyAndOrderFront(None);
    window.makeFirstResponder(Some(&list));
    let response = app.runModalForWindow(&window);
    window.setDelegate(None);
    window.orderOut(None);
    if let Some(previous) = previous {
        previous.makeKeyAndOrderFront(None);
        if let Some(focus) = previous_focus {
            previous.makeFirstResponder(Some(&focus));
        }
    }
    if response != NSModalResponseOK {
        return None;
    }
    let result = target.ivars().result.borrow_mut().take();
    result
}
