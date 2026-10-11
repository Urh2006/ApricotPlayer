//! The labelled text fields and drop-down lists of a screen (the search
//! query, provider and type of Windows `create_controls`), stacked above
//! the list with the label above each control, as `layout_search_screen`
//! lays them out.

#![allow(unsafe_code)]

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSAccessibility, NSTextField, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::form_controls::PopUp;

const LABEL_HEIGHT: f64 = 22.0;
const FIELD_HEIGHT: f64 = 30.0;
const GAP: f64 = 12.0;

/// The control of one field.
pub enum FieldControl {
    Text(Retained<NSTextField>),
    Choice(Retained<PopUp>),
}

impl FieldControl {
    pub fn view(&self) -> &NSView {
        match self {
            Self::Text(field) => field,
            Self::Choice(popup) => popup,
        }
    }
}

struct Field {
    id: &'static str,
    label: Retained<NSTextField>,
    control: FieldControl,
    active: bool,
}

/// What one field shows (from `apricot_shell::ScreenControl`).
pub enum FieldSpec<'a> {
    Text {
        id: &'static str,
        label: &'a str,
        value: Option<&'a str>,
    },
    Choice {
        id: &'static str,
        label: &'a str,
        options: &'a [String],
        selected: Option<usize>,
    },
}

impl FieldSpec<'_> {
    const fn id(&self) -> &'static str {
        match self {
            Self::Text { id, .. } | Self::Choice { id, .. } => id,
        }
    }
}

pub struct ScreenFields {
    container: Retained<NSView>,
    fields: Vec<Field>,
    target: Retained<AnyObject>,
    text_action: Sel,
    choice_action: Sel,
}

impl ScreenFields {
    /// Return in a text field sends `text_action`, a new choice
    /// `choice_action`, both to `target` with the field's tag.
    pub fn new(
        mtm: MainThreadMarker,
        target: &AnyObject,
        text_action: Sel,
        choice_action: Sel,
    ) -> Self {
        Self {
            container: NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO),
            fields: Vec::new(),
            target: target.retain(),
            text_action,
            choice_action,
        }
    }

    pub fn view(&self) -> &NSView {
        &self.container
    }

    /// Shows exactly `specs` in this order.
    pub fn sync(&mut self, specs: &[FieldSpec<'_>]) {
        for field in &mut self.fields {
            field.active = false;
        }
        for spec in specs {
            let index = self.field_index(spec);
            let field = &mut self.fields[index];
            field.active = true;
            match (spec, &field.control) {
                (FieldSpec::Text { label, value, .. }, FieldControl::Text(text)) => {
                    field.label.setStringValue(&NSString::from_str(label));
                    text.setAccessibilityLabel(Some(&NSString::from_str(label)));
                    if let Some(value) = value {
                        text.setStringValue(&NSString::from_str(value));
                    }
                }
                (
                    FieldSpec::Choice {
                        label,
                        options,
                        selected,
                        ..
                    },
                    FieldControl::Choice(popup),
                ) => {
                    field.label.setStringValue(&NSString::from_str(label));
                    popup.setAccessibilityLabel(Some(&NSString::from_str(label)));
                    let keep = popup.selected().unwrap_or(0);
                    popup.set_items(options, selected.unwrap_or(keep));
                }
                _ => {}
            }
        }
        for field in &self.fields {
            field.label.setHidden(!field.active);
            field.control.view().setHidden(!field.active);
        }
        self.layout(specs);
    }

    fn field_index(&mut self, spec: &FieldSpec<'_>) -> usize {
        if let Some(index) = self.fields.iter().position(|field| field.id == spec.id()) {
            return index;
        }
        let mtm = MainThreadMarker::from(&*self.container);
        let tag = isize::try_from(self.fields.len()).unwrap_or(0);
        let label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        label.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable);
        self.container.addSubview(&label);
        let control = match spec {
            FieldSpec::Text { .. } => {
                let text = NSTextField::initWithFrame(NSTextField::alloc(mtm), NSRect::ZERO);
                // SAFETY: the target outlives the field (it owns the window).
                unsafe {
                    text.setTarget(Some(&self.target));
                    text.setAction(Some(self.text_action));
                }
                text.setTag(tag);
                text.setAutoresizingMask(
                    objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable,
                );
                self.container.addSubview(&text);
                FieldControl::Text(text)
            }
            FieldSpec::Choice { label, .. } => {
                let popup = PopUp::new(mtm, label, &self.target, self.choice_action);
                popup.setTag(tag);
                popup.setAutoresizingMask(
                    objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable,
                );
                self.container.addSubview(&popup);
                FieldControl::Choice(popup)
            }
        };
        self.fields.push(Field {
            id: spec.id(),
            label,
            control,
            active: false,
        });
        self.fields.len() - 1
    }

    /// Label above control, from the top.
    fn layout(&self, specs: &[FieldSpec<'_>]) {
        let width = self.container.bounds().size.width;
        let height = self.container.bounds().size.height;
        let mut top = 0.0;
        for spec in specs {
            let Some(field) = self.fields.iter().find(|field| field.id == spec.id()) else {
                continue;
            };
            field.label.setFrame(NSRect::new(
                NSPoint::new(0.0, height - top - LABEL_HEIGHT),
                NSSize::new(width, LABEL_HEIGHT),
            ));
            field.control.view().setFrame(NSRect::new(
                NSPoint::new(0.0, height - top - LABEL_HEIGHT - FIELD_HEIGHT),
                NSSize::new(width, FIELD_HEIGHT),
            ));
            top += LABEL_HEIGHT + FIELD_HEIGHT + GAP;
        }
    }

    /// The shown control with `id`.
    pub fn control(&self, id: &str) -> Option<&FieldControl> {
        self.fields
            .iter()
            .find(|field| field.active && field.id == id)
            .map(|field| &field.control)
    }

    /// The id of the field with `tag`.
    pub fn id_for_tag(&self, tag: isize) -> Option<&'static str> {
        let field = self.fields.get(usize::try_from(tag).ok()?)?;
        field.active.then_some(field.id)
    }

    pub fn text(&self, id: &str) -> String {
        match self.control(id) {
            Some(FieldControl::Text(text)) => text.stringValue().to_string(),
            _ => String::new(),
        }
    }

    pub fn choice(&self, id: &str) -> Option<usize> {
        match self.control(id) {
            Some(FieldControl::Choice(popup)) => popup.selected(),
            _ => None,
        }
    }

    pub fn set_choices(&self, id: &str, options: &[String], selected: usize) {
        if let Some(FieldControl::Choice(popup)) = self.control(id) {
            popup.set_items(options, selected);
        }
    }

    pub fn set_hidden(&self, hidden: bool) {
        self.container.setHidden(hidden);
    }

    /// Height of `count` fields.
    #[must_use]
    pub fn height_for(count: usize) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let count = count as f64;
        count * (LABEL_HEIGHT + FIELD_HEIGHT + GAP)
    }
}
