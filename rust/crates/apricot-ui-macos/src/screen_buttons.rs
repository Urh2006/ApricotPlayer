//! The push buttons of a list screen (Open, Back and the screen's own
//! buttons, Windows `layout_bottom_controls`), below the list.

#![allow(unsafe_code)]

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::controls::Button;

const BUTTON_WIDTH: f64 = 170.0;
const BUTTON_HEIGHT: f64 = 32.0;
const GAP: f64 = 6.0;

struct ScreenButton {
    id: &'static str,
    button: Retained<Button>,
    active: bool,
}

pub struct ScreenButtons {
    container: Retained<NSView>,
    buttons: Vec<ScreenButton>,
    target: Retained<AnyObject>,
    action: Sel,
}

impl ScreenButtons {
    /// The buttons send `action` to `target` with their tag.
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, action: Sel) -> Self {
        Self {
            container: NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO),
            buttons: Vec::new(),
            target: target.retain(),
            action,
        }
    }

    pub fn view(&self) -> &NSView {
        &self.container
    }

    /// Shows exactly `buttons` (id, label) in this order.
    pub fn sync(&mut self, buttons: &[(&'static str, &str)]) {
        let mtm = MainThreadMarker::from(&*self.container);
        for button in &mut self.buttons {
            button.active = false;
        }
        for (id, label) in buttons {
            let index = if let Some(index) = self.buttons.iter().position(|button| button.id == *id)
            {
                index
            } else {
                let button = Button::new(mtm, "", &self.target, self.action);
                button.setTag(isize::try_from(self.buttons.len()).unwrap_or(0));
                self.container.addSubview(&button);
                self.buttons.push(ScreenButton {
                    id,
                    button,
                    active: false,
                });
                self.buttons.len() - 1
            };
            let entry = &mut self.buttons[index];
            entry.active = true;
            entry.button.setTitle(&NSString::from_str(label));
        }
        for button in &self.buttons {
            button.button.setHidden(!button.active);
        }
        self.layout(buttons);
    }

    /// Lays the shown buttons out in rows from the top left.
    fn layout(&self, order: &[(&'static str, &str)]) {
        let width = self.container.bounds().size.width.max(BUTTON_WIDTH);
        let height = self.container.bounds().size.height;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let per_row = ((width + GAP) / (BUTTON_WIDTH + GAP)).floor().max(1.0) as usize;
        for (index, (id, _)) in order.iter().enumerate() {
            let Some(button) = self.button(id) else {
                continue;
            };
            #[allow(clippy::cast_precision_loss)]
            let (column, row) = ((index % per_row) as f64, (index / per_row) as f64);
            button.setFrame(NSRect::new(
                NSPoint::new(
                    column * (BUTTON_WIDTH + GAP),
                    height - (row + 1.0) * BUTTON_HEIGHT - row * GAP,
                ),
                NSSize::new(BUTTON_WIDTH, BUTTON_HEIGHT),
            ));
        }
    }

    pub fn button(&self, id: &str) -> Option<&Button> {
        self.buttons
            .iter()
            .find(|button| button.active && button.id == id)
            .map(|button| &*button.button)
    }

    /// The id of the button with `tag`.
    pub fn id_for_tag(&self, tag: isize) -> Option<&'static str> {
        let button = self.buttons.get(usize::try_from(tag).ok()?)?;
        button.active.then_some(button.id)
    }

    pub fn set_hidden(&self, hidden: bool) {
        self.container.setHidden(hidden);
    }

    /// Rows needed for `count` buttons in `width`.
    #[must_use]
    pub fn height_for(count: usize, width: f64) -> f64 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let per_row = ((width + GAP) / (BUTTON_WIDTH + GAP)).floor().max(1.0) as usize;
        #[allow(clippy::cast_precision_loss)]
        let rows = count.div_ceil(per_row).max(1) as f64;
        rows * BUTTON_HEIGHT + (rows - 1.0) * GAP
    }
}
