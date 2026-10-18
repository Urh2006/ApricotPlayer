//! Python `add_background_player_section`: a label, the player and the
//! transport buttons below every other screen while background playback
//! runs, in that Tab order.
#![allow(unsafe_code)]

use crate::screen_buttons::ScreenButtons;
use apricot_app::BackgroundPlayerModel;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAccessibility, NSAccessibilityButtonRole, NSTextField, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

/// Windows `PLAYER_HEIGHT` of the section.
const PLAYER_HEIGHT: f64 = 96.0;
const LABEL_HEIGHT: f64 = 22.0;
const GAP: f64 = 6.0;

pub struct BackgroundPlayer {
    view: Retained<NSView>,
    label: Retained<NSTextField>,
    buttons: ScreenButtons,
    model: Option<BackgroundPlayerModel>,
    /// The player page's surface while the section shows it (beta 18): it
    /// keeps the player's keys, as the Windows video host does.
    surface: Option<Retained<NSView>>,
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
            surface: None,
        }
    }

    /// Shows the player surface between the label and the buttons.
    pub fn adopt_surface(&mut self, surface: Retained<NSView>) {
        if self.model.is_none() {
            return;
        }
        // SAFETY: reading the parent of a live view on the main thread.
        let here = unsafe { surface.superview() }.is_some_and(|parent| {
            std::ptr::eq(Retained::as_ptr(&parent), Retained::as_ptr(&self.view))
        });
        if !here {
            self.view.addSubview(&surface);
        }
        self.surface = Some(surface);
        self.layout();
    }
    pub fn view(&self) -> &NSView {
        &self.view
    }
    pub fn height(&self, width: f64) -> f64 {
        let player = if self.surface.is_some() {
            PLAYER_HEIGHT + GAP
        } else {
            0.0
        };
        self.model.as_ref().map_or(0.0, |model| {
            LABEL_HEIGHT
                + GAP
                + player
                + ScreenButtons::height_for(model.buttons.len(), width)
                + 12.0
        })
    }
    pub fn sync(&mut self, model: Option<BackgroundPlayerModel>) {
        if self.model == model {
            return;
        }
        self.view.setHidden(model.is_none());
        if model.is_none() {
            // The player page takes its surface back when it shows.
            self.surface = None;
        }
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
        let mut top = bounds.size.height - LABEL_HEIGHT;
        self.label.setFrame(NSRect::new(
            NSPoint::new(0.0, top),
            NSSize::new(bounds.size.width, LABEL_HEIGHT),
        ));
        top -= GAP;
        if let Some(surface) = &self.surface {
            top -= PLAYER_HEIGHT;
            surface.setFrame(NSRect::new(
                NSPoint::new(0.0, top.max(0.0)),
                NSSize::new(bounds.size.width, PLAYER_HEIGHT),
            ));
            top -= GAP;
        }
        self.buttons.view().setFrame(NSRect::new(
            NSPoint::ZERO,
            NSSize::new(bounds.size.width, top.max(0.0)),
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
    /// The section in Tab order: the player, then the buttons.
    pub fn controls(&self) -> Vec<&NSView> {
        self.model.as_ref().map_or_else(Vec::new, |model| {
            self.surface
                .iter()
                .map(|surface| &**surface)
                .chain(model.buttons.iter().filter_map(|button| {
                    self.buttons
                        .button(button.id)
                        .map(|button| button as &NSView)
                }))
                .collect()
        })
    }
}
