//! Native AppKit rendering of typed feature forms.
#![allow(unsafe_code)]

use crate::controls::{self, ReadOnlyText};
use crate::form_controls::{CheckBox, PopUp};
use apricot_shell::{FeatureForm, FormFieldKind, FormValue, FormValues};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAlert, NSAlertFirstButtonReturn, NSControlTextEditingDelegate, NSResponder,
    NSScrollView, NSSecureTextField, NSTextField, NSTextFieldDelegate, NSView,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use std::{cell::RefCell, ffi::c_void};

#[derive(Clone, Copy)]
struct Preview {
    data: *mut c_void,
    invoke: unsafe fn(*mut c_void, &FormValues),
}
struct PreviewState {
    form: FeatureForm,
    controls: Vec<Control>,
    callback: Preview,
}
#[derive(Default)]
struct TargetIvars {
    preview: RefCell<Option<PreviewState>>,
}

define_class!(
    // SAFETY: NSObject is subclassable; this target outlives every control.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotFeatureFormTarget"]
    #[ivars = TargetIvars]
    struct Target;
    unsafe impl NSObjectProtocol for Target {}
    unsafe impl NSControlTextEditingDelegate for Target {
        #[unsafe(method(controlTextDidChange:))]
        fn text_changed(&self, _notification: &NSNotification) { self.preview(); }
    }
    unsafe impl NSTextFieldDelegate for Target {}
    impl Target {
        #[unsafe(method(changed:))]
        fn changed(&self, _sender: Option<&AnyObject>) { self.preview(); }
    }
);

define_class!(
    // SAFETY: NSView is subclassable; no additional ownership requirements.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ApricotFeatureFormContent"]
    struct Content;
    impl Content {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }
    }
);

#[derive(Clone)]
enum Control {
    Text(Retained<NSTextField>),
    Choice(Retained<PopUp>),
    Check(Retained<CheckBox>),
    ReadOnly(Retained<ReadOnlyText>),
}

impl Control {
    fn view(&self) -> &NSView {
        match self {
            Self::Text(field) => field,
            Self::ReadOnly(text) => text,
            Self::Choice(choice) => choice,
            Self::Check(check) => check,
        }
    }
}

impl Target {
    fn preview(&self) {
        let update = {
            let preview = self.ivars().preview.borrow();
            preview.as_ref().and_then(|state| {
                state
                    .form
                    .validate(&read_values(&state.form, &state.controls))
                    .ok()
                    .map(|values| (state.callback, values))
            })
        };
        if let Some((callback, values)) = update {
            // SAFETY: the synchronous form loop owns the callback until the preview is cleared.
            unsafe { (callback.invoke)(callback.data, &values) };
        }
    }
}

pub(crate) fn show(mtm: MainThreadMarker, form: &FeatureForm) -> Option<FormValues> {
    show_inner(mtm, form, None)
}

pub(crate) fn show_preview(
    mtm: MainThreadMarker,
    form: &FeatureForm,
    preview: &mut dyn FnMut(&FormValues),
) -> Option<FormValues> {
    let mut callback = preview;
    let handle = Preview {
        data: std::ptr::from_mut(&mut callback).cast(),
        invoke: invoke_preview,
    };
    show_inner(mtm, form, Some(handle))
}

unsafe fn invoke_preview(data: *mut c_void, values: &FormValues) {
    // SAFETY: show_preview retains this callback stack slot for the entire modal loop.
    let callback = unsafe { &mut *data.cast::<&mut dyn FnMut(&FormValues)>() };
    callback(values);
}

fn read_values(form: &FeatureForm, bound: &[Control]) -> FormValues {
    form.fields
        .iter()
        .zip(bound)
        .filter_map(|(field, control)| {
            let value = match control {
                Control::Text(text) => FormValue::Text(text.stringValue().to_string()),
                Control::Choice(choice) => {
                    FormValue::Choice(choice.selected().unwrap_or(usize::MAX))
                }
                Control::Check(check) => FormValue::Checkbox(check.checked()),
                Control::ReadOnly(_) => return None,
            };
            Some((field.id.clone(), value))
        })
        .collect()
}

#[allow(clippy::too_many_lines, clippy::cast_precision_loss)] // One block per field kind; few rows.
fn show_inner(
    mtm: MainThreadMarker,
    form: &FeatureForm,
    preview: Option<Preview>,
) -> Option<FormValues> {
    // SAFETY: NSObject's designated initialization on the main thread.
    let this = Target::alloc(mtm).set_ivars(TargetIvars::default());
    let target: Retained<Target> = unsafe { msg_send![super(this), init] };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&form.title));
    let save = alert.addButtonWithTitle(&NSString::from_str(&form.submit));
    let cancel = alert.addButtonWithTitle(&NSString::from_str(&form.cancel));
    cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
    let height = (form.fields.len().max(1) as f64 * 42.0 + 12.0).max(60.0);
    // SAFETY: NSView's designated initialization on the main thread.
    let content: Retained<Content> =
        unsafe { msg_send![Content::alloc(mtm), initWithFrame: NSRect::ZERO] };
    content.setFrame(NSRect::new(NSPoint::ZERO, NSSize::new(680.0, height)));
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(NSPoint::ZERO, NSSize::new(700.0, height.min(480.0))),
    );
    scroll.setDocumentView(Some(&content));
    scroll.setHasVerticalScroller(true);
    let mut bound = Vec::new();
    for (index, field) in form.fields.iter().enumerate() {
        let y = 8.0 + index as f64 * 42.0;
        if !matches!(field.kind, FormFieldKind::Checkbox { .. }) {
            let label = NSTextField::labelWithString(&NSString::from_str(&field.label), mtm);
            label.setFrame(NSRect::new(
                NSPoint::new(8.0, y + 4.0),
                NSSize::new(250.0, 28.0),
            ));
            content.addSubview(&label);
        }
        let control = match &field.kind {
            FormFieldKind::Choice { options, selected } => {
                let popup = PopUp::new(mtm, &field.label, &target, sel!(changed:));
                popup.set_items(options, *selected);
                Control::Choice(popup)
            }
            FormFieldKind::Checkbox { checked } => {
                let check = CheckBox::new(mtm, &field.label, &target, sel!(changed:));
                check.set_checked(*checked);
                Control::Check(check)
            }
            // A focusable text, as the Windows read-only edit: Tab reaches
            // it and VoiceOver reads it, a running timer for example.
            FormFieldKind::ReadOnly { value } => {
                let text = ReadOnlyText::new(mtm);
                text.set_text(value);
                text.set_label(&field.label);
                Control::ReadOnly(text)
            }
            kind => {
                let secret = matches!(kind, FormFieldKind::Text { secret: true, .. });
                let text = match kind {
                    FormFieldKind::Text { value, .. } | FormFieldKind::File { value, .. } => {
                        value.clone()
                    }
                    FormFieldKind::Number { value, .. } => value.to_string(),
                    _ => unreachable!(),
                };
                let field_view: Retained<NSTextField> = if secret {
                    Retained::into_super(NSSecureTextField::new(mtm))
                } else {
                    NSTextField::new(mtm)
                };
                field_view.setStringValue(&NSString::from_str(&text));
                field_view.setAccessibilityLabel(Some(&NSString::from_str(&field.label)));
                field_view.setEditable(true);
                field_view.setSelectable(true);
                // SAFETY: the form target outlives the field; the delegate is weak.
                unsafe { field_view.setDelegate(Some(ProtocolObject::from_ref(&*target))) };
                Control::Text(field_view)
            }
        };
        let (x, width) = if matches!(control, Control::Check(_)) {
            (8.0, 660.0)
        } else {
            (264.0, 400.0)
        };
        control
            .view()
            .setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(width, 30.0)));
        content.addSubview(control.view());
        bound.push(control);
    }
    alert.setAccessoryView(Some(&scroll));
    let mut order: Vec<&NSView> = bound.iter().map(Control::view).collect();
    order.push(&save);
    order.push(&cancel);
    controls::set_tab_order(&order);
    if let Some(first) = bound.get(form.initial_focus).or(bound.first()) {
        alert.window().setInitialFirstResponder(Some(first.view()));
    }
    if let Some(callback) = preview {
        *target.ivars().preview.borrow_mut() = Some(PreviewState {
            form: form.clone(),
            controls: bound.clone(),
            callback,
        });
    }
    loop {
        if alert.runModal() != NSAlertFirstButtonReturn {
            target.ivars().preview.borrow_mut().take();
            return None;
        }
        let raw = read_values(form, &bound);
        match form.validate(&raw) {
            Ok(values) => {
                target.ivars().preview.borrow_mut().take();
                return Some(values);
            }
            Err(label) => {
                let error = NSAlert::new(mtm);
                error.setMessageText(&NSString::from_str(&form.title));
                error.setInformativeText(&NSString::from_str(&form.error_message(&label)));
                error.runModal();
                if let Some(index) = form.fields.iter().position(|field| field.label == label) {
                    alert
                        .window()
                        .setInitialFirstResponder(Some(bound[index].view()));
                }
            }
        }
    }
}
