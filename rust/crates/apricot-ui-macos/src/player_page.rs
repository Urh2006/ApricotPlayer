//! The player page (Windows `player_controls_win32.rs`): the navigation
//! buttons, the player surface, the action buttons and the check boxes of
//! [`PlayerScreenModel`], in the model's order, which is also the Tab order.

#![allow(unsafe_code)]

use apricot_app::{PlayerControlRole, PlayerScreenModel};
use apricot_playback::MpvRenderHost;
use apricot_shell::DetailsLabels;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSButtonType, NSControlStateValueOff, NSControlStateValueOn, NSScrollView, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::controls::{self, Button, ReadOnlyText};

/// Tags of the details buttons (Windows `DETAILS_COPY_ID`, `DETAILS_BACK_ID`).
const DETAILS_COPY_TAG: isize = 10_000;
const DETAILS_BACK_TAG: isize = 10_001;
const DETAILS_LABEL_HEIGHT: f64 = 22.0;
const DETAILS_TEXT_HEIGHT: f64 = 160.0;
const DETAILS_BUTTON_HEIGHT: f64 = 30.0;

/// Python `show_video_details`: a label, a read-only text and the Copy
/// details and Back buttons below the player controls, not a dialog.
struct DetailsPanel {
    label: Retained<NSTextField>,
    scroll: Retained<NSScrollView>,
    text: Retained<ReadOnlyText>,
    copy: Retained<Button>,
    back: Retained<Button>,
    visible: bool,
}
use crate::video::{PlayerSurface, player_surface};

struct PlayerControl {
    id: &'static str,
    action_id: Option<&'static str>,
    button: Retained<Button>,
    checkbox: bool,
    active: bool,
}

pub struct PlayerPage {
    container: Retained<NSView>,
    surface: Retained<PlayerSurface>,
    host: MpvRenderHost,
    controls: Vec<PlayerControl>,
    /// The ids of the shown controls in Tab order, `video_host` included.
    order: Vec<&'static str>,
    initial_focus_id: &'static str,
    details: DetailsPanel,
    target: Retained<AnyObject>,
    action: Sel,
}

impl PlayerPage {
    /// The page; its buttons send `action` to `target` with their tag.
    pub fn new(mtm: MainThreadMarker, target: &AnyObject, action: Sel) -> Self {
        let container = NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO);
        let (surface, host) = player_surface(mtm);
        container.addSubview(&surface);
        container.setHidden(true);
        let label = NSTextField::labelWithString(&NSString::new(), mtm);
        let text = ReadOnlyText::new(mtm);
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), NSRect::ZERO);
        scroll.setDocumentView(Some(&text));
        scroll.setHasVerticalScroller(true);
        let copy = Button::new(mtm, "", target, action);
        copy.setTag(DETAILS_COPY_TAG);
        let back = Button::new(mtm, "", target, action);
        back.setTag(DETAILS_BACK_TAG);
        for view in [&*label as &NSView, &scroll, &copy, &back] {
            view.setHidden(true);
            container.addSubview(view);
        }
        let details = DetailsPanel {
            label,
            scroll,
            text,
            copy,
            back,
            visible: false,
        };
        Self {
            container,
            surface,
            host,
            controls: Vec::new(),
            order: Vec::new(),
            initial_focus_id: "video_host",
            details,
            target: target.retain(),
            action,
        }
    }

    pub fn view(&self) -> &NSView {
        &self.container
    }

    pub const fn render_host(&self) -> &MpvRenderHost {
        &self.host
    }

    pub fn set_visible(&self, visible: bool) {
        self.container.setHidden(!visible);
    }

    /// Windows `PlayerControls::sync` and `layout`.
    pub fn sync(&mut self, model: &PlayerScreenModel) {
        let mtm = MainThreadMarker::from(&*self.container);
        self.initial_focus_id = model.initial_focus_id;
        for control in &mut self.controls {
            control.active = false;
        }
        self.order.clear();
        for projected in &model.controls {
            self.order.push(projected.id);
            if projected.role == PlayerControlRole::VideoHost {
                self.surface.set_label(&projected.label);
                continue;
            }
            let checkbox = projected.role == PlayerControlRole::Checkbox;
            let existing = self
                .controls
                .iter()
                .position(|control| control.id == projected.id && control.checkbox == checkbox);
            let index = if let Some(index) = existing {
                index
            } else {
                let button = Button::new(mtm, "", &self.target, self.action);
                if checkbox {
                    button.setButtonType(NSButtonType::Switch);
                }
                button.setTag(isize::try_from(self.controls.len()).unwrap_or(0));
                self.container.addSubview(&button);
                self.controls.push(PlayerControl {
                    id: projected.id,
                    action_id: None,
                    button,
                    checkbox,
                    active: false,
                });
                self.controls.len() - 1
            };
            let control = &mut self.controls[index];
            control.active = true;
            control.action_id = projected.action_id;
            control
                .button
                .setTitle(&NSString::from_str(&projected.label));
            if checkbox {
                control
                    .button
                    .setState(if projected.checked.unwrap_or(false) {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    });
            }
        }
        for control in &self.controls {
            control.button.setHidden(!control.active);
        }
        self.update_tab_order();
        self.layout();
    }

    /// The page's Tab order: the model's controls, then the details.
    fn update_tab_order(&self) {
        let mut views: Vec<&NSView> = self
            .order
            .iter()
            .filter_map(|id| self.view_for_id(id))
            .collect();
        if self.details.visible {
            views.extend([
                &*self.details.text as &NSView,
                &*self.details.copy,
                &*self.details.back,
            ]);
        }
        controls::set_tab_order(&views);
    }

    /// Windows `PlayerControls::show_details`.
    pub fn show_details(&mut self, labels: &DetailsLabels, text: &str) {
        let details = &self.details;
        details
            .label
            .setStringValue(&NSString::from_str(&labels.title));
        details.text.set_label(&labels.title);
        details.copy.setTitle(&NSString::from_str(&labels.copy));
        details.back.setTitle(&NSString::from_str(&labels.back));
        details.text.set_text(text);
        self.details.visible = true;
        for view in [
            &*self.details.label as &NSView,
            &self.details.scroll,
            &self.details.copy,
            &self.details.back,
        ] {
            view.setHidden(false);
        }
        self.update_tab_order();
        self.layout();
    }

    /// Windows `PlayerControls::hide_details`.
    pub fn hide_details(&mut self) {
        self.details.visible = false;
        for view in [
            &*self.details.label as &NSView,
            &self.details.scroll,
            &self.details.copy,
            &self.details.back,
        ] {
            view.setHidden(true);
        }
        self.update_tab_order();
        self.layout();
    }

    pub fn set_details_text(&self, text: &str) {
        if self.details.visible {
            self.details.text.set_text(text);
        }
    }

    pub fn details_text(&self) -> &NSView {
        &self.details.text
    }

    /// Windows `PlayerControls::layout_below` in the container's bounds.
    pub fn layout(&self) {
        let bounds = self.container.bounds();
        let margin = 12.0;
        let gap = 6.0;
        let width = bounds.size.width;
        let height = bounds.size.height;
        let inner_width = (width - margin * 2.0).max(1.0);
        let video_height = ((height - margin * 3.0) / 4.0).clamp(80.0, 180.0);
        self.surface.setFrame(NSRect::new(
            NSPoint::new(margin, height - margin - video_height),
            NSSize::new(inner_width, video_height),
        ));
        let active: Vec<&PlayerControl> = self.order_controls().collect();
        if active.is_empty() {
            return;
        }
        let controls_top = height - margin * 2.0 - video_height;
        let details_height = if self.details.visible {
            DETAILS_LABEL_HEIGHT + DETAILS_TEXT_HEIGHT + DETAILS_BUTTON_HEIGHT + gap * 3.0
        } else {
            0.0
        };
        let available_height = (controls_top - margin - details_height).max(28.0);
        if self.details.visible {
            let mut y = margin + details_height;
            y -= DETAILS_LABEL_HEIGHT;
            self.details.label.setFrame(NSRect::new(
                NSPoint::new(margin, y),
                NSSize::new(inner_width, DETAILS_LABEL_HEIGHT),
            ));
            y -= gap + DETAILS_TEXT_HEIGHT;
            self.details.scroll.setFrame(NSRect::new(
                NSPoint::new(margin, y),
                NSSize::new(inner_width, DETAILS_TEXT_HEIGHT),
            ));
            self.details.text.setFrame(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(inner_width, DETAILS_TEXT_HEIGHT),
            ));
            y -= gap + DETAILS_BUTTON_HEIGHT;
            self.details.copy.setFrame(NSRect::new(
                NSPoint::new(margin, y),
                NSSize::new(160.0, DETAILS_BUTTON_HEIGHT),
            ));
            self.details.back.setFrame(NSRect::new(
                NSPoint::new(margin + 160.0 + gap, y),
                NSSize::new(160.0, DETAILS_BUTTON_HEIGHT),
            ));
        }
        let preferred = 34.0;
        let rows_at_preferred = (available_height / (preferred + gap)).floor().max(1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rows_at_preferred = rows_at_preferred as usize;
        let required_columns = active.len().div_ceil(rows_at_preferred);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let maximum_columns = ((inner_width / 140.0).floor().max(1.0)) as usize;
        let columns = required_columns.clamp(1, maximum_columns);
        let rows = active.len().div_ceil(columns);
        #[allow(clippy::cast_precision_loss)]
        let (rows_f, columns_f) = (rows as f64, columns as f64);
        let button_height = ((available_height - gap * (rows_f - 1.0)) / rows_f).clamp(24.0, 34.0);
        let button_width = ((inner_width - gap * (columns_f - 1.0)) / columns_f).max(80.0);
        for (index, control) in active.into_iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let (column, row) = ((index / rows) as f64, (index % rows) as f64);
            let x = margin + column * (button_width + gap);
            let y = controls_top - (row + 1.0) * button_height - row * gap;
            control.button.setFrame(NSRect::new(
                NSPoint::new(x, y),
                NSSize::new(button_width, button_height),
            ));
        }
    }

    fn order_controls(&self) -> impl Iterator<Item = &PlayerControl> {
        self.order.iter().filter_map(|id| {
            self.controls
                .iter()
                .find(|control| control.active && control.id == *id)
        })
    }

    fn view_for_id(&self, id: &str) -> Option<&NSView> {
        match id {
            "video_host" => return Some(&self.surface),
            "details_text" if self.details.visible => return Some(&self.details.text),
            "details_copy" if self.details.visible => return Some(&self.details.copy),
            "details_back" if self.details.visible => return Some(&self.details.back),
            _ => {}
        }
        self.controls
            .iter()
            .find(|control| control.active && control.id == id)
            .map(|control| -> &NSView { &control.button })
    }

    /// The view that gets focus when the page opens (Windows
    /// `PlayerControls::initial_focus`).
    pub fn initial_focus(&self) -> &NSView {
        self.view_for_id(self.initial_focus_id)
            .unwrap_or(&self.surface)
    }

    pub fn view_for(&self, id: &str) -> Option<&NSView> {
        self.view_for_id(id)
    }

    /// The id of `view` when it is a shown control of the page.
    pub fn control_id(&self, view: &AnyObject) -> Option<&'static str> {
        let pointer: *const AnyObject = view;
        if std::ptr::eq(
            pointer,
            std::ptr::from_ref::<AnyObject>(self.surface.as_ref()),
        ) {
            return Some("video_host");
        }
        if self.details.visible {
            for (id, view) in [
                ("details_text", self.details.text.as_ref() as &AnyObject),
                ("details_copy", self.details.copy.as_ref()),
                ("details_back", self.details.back.as_ref()),
            ] {
                if std::ptr::eq(pointer, std::ptr::from_ref::<AnyObject>(view)) {
                    return Some(id);
                }
            }
        }
        self.controls
            .iter()
            .find(|control| {
                control.active
                    && std::ptr::eq(
                        pointer,
                        std::ptr::from_ref::<AnyObject>(control.button.as_ref()),
                    )
            })
            .map(|control| control.id)
    }

    /// Whether the control `id` is a check box.
    pub fn is_checkbox(&self, id: &str) -> bool {
        self.controls
            .iter()
            .any(|control| control.active && control.id == id && control.checkbox)
    }

    /// The action of the control with `tag` (Windows
    /// `activation_for_command`); the session autoplay check box has none
    /// and is toggled by its id.
    pub fn activation(&self, tag: isize) -> Option<&'static str> {
        match tag {
            DETAILS_COPY_TAG => return Some("details_copy"),
            DETAILS_BACK_TAG => return Some("details_back"),
            _ => {}
        }
        let control = self.controls.get(usize::try_from(tag).ok()?)?;
        if !control.active {
            return None;
        }
        control
            .action_id
            .or_else(|| (control.id == "session_autoplay_next").then_some("session_autoplay_next"))
    }
}
