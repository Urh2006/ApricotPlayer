//! Native background transport controls, projected from the shared model.
#![allow(unsafe_code)]

use crate::screen_buttons::ScreenButtons;
use apricot_app::BackgroundPlayerModel;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAccessibility, NSAccessibilityButtonRole, NSTextField, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub struct BackgroundPlayer {
    view: Retained<NSView>,
    label: Retained<NSTextField>,
    buttons: ScreenButtons,
    model: Option<BackgroundPlayerModel>,
}

impl BackgroundPlayer {
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, action: Sel) -> Self {
        let view = NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO);
        let label = NSTextField::labelWithString(&NSString::new(), mtm);
        let buttons = ScreenButtons::new(mtm, target, action);
        view.addSubview(&label);
        view.addSubview(buttons.view());
        view.setHidden(true);
        Self {
            view,
            label,
            buttons,
            model: None,
        }
    }
    pub fn view(&self) -> &NSView {
        &self.view
    }
    pub fn height(&self, width: f64) -> f64 {
        self.model.as_ref().map_or(0.0, |model| {
            28.0 + ScreenButtons::height_for(model.buttons.len(), width) + 12.0
        })
    }
    pub fn sync(&mut self, model: Option<BackgroundPlayerModel>) {
        if self.model == model {
            return;
        }
        self.view.setHidden(model.is_none());
        if let Some(model) = &model {
            self.label.setStringValue(&NSString::from_str(&model.label));
            let buttons: Vec<_> = model
                .buttons
                .iter()
                .map(|button| (button.id, button.label.as_str()))
                .collect();
            self.buttons.sync(&buttons);
            for projected in &model.buttons {
                if let Some(button) = self.buttons.button(projected.id) {
                    // SAFETY: AppKit's constant is process-lifetime immutable.
                    button.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
                    button.setAccessibilityElement(true);
                    button.setAccessibilityLabel(Some(&NSString::from_str(&projected.label)));
                }
            }
        }
        self.model = model;
        self.layout();
    }
    pub fn layout(&mut self) {
        let bounds = self.view.bounds();
        self.label.setFrame(NSRect::new(
            NSPoint::new(0.0, bounds.size.height - 22.0),
            NSSize::new(bounds.size.width, 22.0),
        ));
        self.buttons.view().setFrame(NSRect::new(
            NSPoint::ZERO,
            NSSize::new(bounds.size.width, (bounds.size.height - 28.0).max(0.0)),
        ));
        if let Some(model) = &self.model {
            let buttons: Vec<_> = model
                .buttons
                .iter()
                .map(|button| (button.id, button.label.as_str()))
                .collect();
            self.buttons.sync(&buttons);
        }
    }
    pub fn activation(&self, tag: isize) -> Option<&'static str> {
        let id = self.buttons.id_for_tag(tag)?;
        self.model
            .as_ref()?
            .buttons
            .iter()
            .find(|button| button.id == id)
            .map(|button| button.action_id)
    }
    pub fn control_id(&self, responder: &AnyObject) -> Option<&'static str> {
        if self.view.isHidden() {
            return None;
        }
        self.model.as_ref()?.buttons.iter().find_map(|button| {
            let view = self.buttons.button(button.id)?;
            std::ptr::eq(responder, std::ptr::from_ref::<AnyObject>(view.as_ref()))
                .then_some(button.id)
        })
    }
    pub fn controls(&self) -> Vec<&NSView> {
        self.model.as_ref().map_or_else(Vec::new, |model| {
            model
                .buttons
                .iter()
                .filter_map(|button| {
                    self.buttons
                        .button(button.id)
                        .map(|button| button as &NSView)
                })
                .collect()
        })
    }
}
