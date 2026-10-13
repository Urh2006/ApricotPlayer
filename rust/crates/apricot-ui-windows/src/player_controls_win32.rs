//! Native player controls with real button and checkbox accessibility roles.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;

use apricot_app::{PlayerControlRole, PlayerScreenModel};
use windows::Win32::UI::Controls::EM_SETSEL;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        UI::WindowsAndMessaging::{
            BS_AUTOCHECKBOX, CreateWindowExW, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE,
            ES_READONLY, GetWindowTextLengthW, GetWindowTextW, HMENU, MoveWindow, SW_HIDE, SW_SHOW,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SendMessageW, SetWindowPos, SetWindowTextW,
            ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETFONT, WS_CHILD, WS_EX_CLIENTEDGE,
            WS_GROUP, WS_HSCROLL, WS_TABSTOP, WS_VSCROLL,
        },
    },
    core::{PCWSTR, Result, w},
};

const CONTROL_ID_BASE: usize = 2_000;
const DETAILS_COPY_ID: usize = 2_100;
const DETAILS_BACK_ID: usize = 2_101;
const DETAILS_LABEL_HEIGHT: i32 = 22;
const DETAILS_TEXT_HEIGHT: i32 = 160;
const DETAILS_BUTTON_HEIGHT: i32 = 30;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: usize = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeRole {
    Button,
    Checkbox,
}

#[derive(Clone, Copy, Debug)]
struct ControlSpec {
    id: &'static str,
    role: NativeRole,
}

const NAVIGATION_SPECS: &[ControlSpec] = &[
    button("back"),
    button("back_results"),
    button("back_main"),
    button("back_fullscreen_results"),
];

const ACTION_SPECS: &[ControlSpec] = &[
    button("previous"),
    button("play_pause"),
    button("next"),
    button("queue"),
    button("add_to_playlist"),
    button("add_bookmark"),
    button("bookmarks"),
    button("output_devices"),
    button("equalizer"),
    button("audio_normalization"),
    button("chapters"),
    button("transcript"),
    button("lyrics"),
    button("comments"),
    button("edit_mode"),
    button("copy_location"),
    button("copy_stream_url"),
    button("save_podcast_speed"),
    button("details"),
    button("close_player"),
    checkbox("fullscreen"),
    checkbox("repeat"),
    checkbox("session_autoplay_next"),
    checkbox("bass_boost"),
];

const fn button(id: &'static str) -> ControlSpec {
    ControlSpec {
        id,
        role: NativeRole::Button,
    }
}

const fn checkbox(id: &'static str) -> ControlSpec {
    ControlSpec {
        id,
        role: NativeRole::Checkbox,
    }
}

struct NativeControl {
    id: &'static str,
    command_id: usize,
    role: NativeRole,
    window: HWND,
    action_id: Option<&'static str>,
    active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerControlActivation {
    Action(&'static str),
    SessionAutoplayNext,
    CopyDetails,
    HideDetails,
}

/// Python `show_video_details`: a label, a read-only text field and the Copy
/// details and Back buttons added below the player controls, not a dialog.
struct DetailsPanel {
    label: HWND,
    text: HWND,
    copy: HWND,
    back: HWND,
    visible: bool,
}

pub struct DetailsLabels {
    pub title: String,
    pub copy: String,
    pub back: String,
}

pub struct PlayerControls {
    video_host: HWND,
    controls: Vec<NativeControl>,
    details: DetailsPanel,
    surface_visible: bool,
    // The background player shows the video host on another screen.
    video_host_shared: bool,
    initial_focus_id: &'static str,
}

impl PlayerControls {
    pub unsafe fn create(parent: HWND, instance: HINSTANCE) -> Result<Self> {
        let font = GetStockObject(DEFAULT_GUI_FONT);
        let font_param = Some(WPARAM(font.0 as usize));
        let mut controls = Vec::with_capacity(NAVIGATION_SPECS.len() + ACTION_SPECS.len());
        for spec in NAVIGATION_SPECS {
            controls.push(create_native_control(
                parent,
                instance,
                *spec,
                CONTROL_ID_BASE + controls.len(),
                font_param,
            )?);
        }
        let video_host = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("STATIC"),
            PCWSTR::null(),
            WS_CHILD | WS_TABSTOP | WS_GROUP,
            0,
            0,
            100,
            80,
            Some(parent),
            None,
            Some(instance),
            None,
        )?;
        SendMessageW(video_host, WM_SETFONT, font_param, Some(LPARAM(1)));
        for spec in ACTION_SPECS {
            controls.push(create_native_control(
                parent,
                instance,
                *spec,
                CONTROL_ID_BASE + controls.len(),
                font_param,
            )?);
        }
        // Created last so the details follow the checkboxes in Tab order, as
        // Python appends them to the end of the player page.
        let details = DetailsPanel::create(parent, instance, font_param)?;
        Ok(Self {
            video_host,
            controls,
            details,
            surface_visible: false,
            video_host_shared: false,
            initial_focus_id: "video_host",
        })
    }

    pub const fn details_visible(&self) -> bool {
        self.details.visible
    }

    pub const fn details_text(&self) -> HWND {
        self.details.text
    }

    /// Shows the details with the caret at the start; the caller focuses and
    /// announces them like Python `show_video_details`.
    pub unsafe fn show_details(&mut self, labels: &DetailsLabels, text: &str) {
        set_text(self.details.label, &labels.title);
        crate::accessibility_win32::annotate_control_name(self.details.text, &labels.title);
        set_text(self.details.copy, &labels.copy);
        set_text(self.details.back, &labels.back);
        self.details.visible = true;
        self.set_details_text(text);
        self.details.show(self.surface_visible);
    }

    pub unsafe fn hide_details(&mut self) {
        self.details.visible = false;
        self.details.show(false);
    }

    /// Python `update_details_text`: replaces the value and puts the caret at
    /// the start. Unchanged text is left alone so periodic refreshes do not
    /// move the reading position.
    pub unsafe fn set_details_text(&self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        if window_text(self.details.text) == text {
            return;
        }
        set_text(self.details.text, &text);
        SendMessageW(
            self.details.text,
            EM_SETSEL,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        );
    }

    pub const fn video_host(&self) -> HWND {
        self.video_host
    }

    pub unsafe fn sync(&mut self, model: &PlayerScreenModel) {
        self.initial_focus_id = model.initial_focus_id;
        if let Some(video) = model
            .controls
            .iter()
            .find(|control| control.id == "video_host")
        {
            set_text(self.video_host, &video.label);
        }
        for native in &mut self.controls {
            let projected = model
                .controls
                .iter()
                .find(|control| control.id == native.id);
            native.active =
                projected.is_some_and(|control| role_matches(native.role, control.role));
            native.action_id = projected.and_then(|control| control.action_id);
            if let Some(control) = projected
                && native.active
            {
                set_text(native.window, &control.label);
                if native.role == NativeRole::Checkbox {
                    SendMessageW(
                        native.window,
                        BM_SETCHECK,
                        Some(WPARAM(if control.checked.unwrap_or(false) {
                            BST_CHECKED
                        } else {
                            0
                        })),
                        None,
                    );
                }
            }
            show(native.window, self.surface_visible && native.active);
        }
        show(
            self.video_host,
            self.surface_visible || self.video_host_shared,
        );
        self.details.show(self.surface_visible);
    }

    pub unsafe fn set_visible(&mut self, visible: bool) {
        self.surface_visible = visible;
        show(self.video_host, visible || self.video_host_shared);
        for control in &self.controls {
            show(control.window, visible && control.active);
        }
        self.details.show(visible);
    }

    /// While the background player shows the video host on another screen,
    /// refreshes never hide it, so it keeps focus.
    pub const fn set_video_host_shared(&mut self, shared: bool) {
        self.video_host_shared = shared;
    }

    /// Python `add_background_player_section` reuses the player: shown on
    /// another screen it follows `previous` in Tab order.
    pub unsafe fn show_video_host_after(&self, previous: HWND) {
        let _ = SetWindowPos(
            self.video_host,
            Some(previous),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        show(self.video_host, true);
    }

    /// Puts the player back between the navigation buttons and the action
    /// buttons of the player page.
    pub unsafe fn restore_video_host_order(&self) {
        if let Some(last_navigation) = self.controls.get(NAVIGATION_SPECS.len() - 1) {
            let _ = SetWindowPos(
                self.video_host,
                Some(last_navigation.window),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    /// The page's focusable controls in Tab order, for the page with the
    /// embedded result list, which sits in `list_slot` after the navigation.
    pub fn tab_order_with(&self, list_slot: HWND) -> Vec<HWND> {
        let mut order: Vec<_> = self.controls[..NAVIGATION_SPECS.len()]
            .iter()
            .filter(|control| control.active)
            .map(|control| control.window)
            .collect();
        order.push(list_slot);
        order.push(self.video_host);
        order.extend(
            self.controls[NAVIGATION_SPECS.len()..]
                .iter()
                .filter(|control| control.active)
                .map(|control| control.window),
        );
        if self.details.visible {
            order.extend([self.details.text, self.details.copy, self.details.back]);
        }
        order
    }

    pub unsafe fn layout(&self, width: i32, height: i32, margin: i32, status_height: i32) {
        self.layout_below(width, height, margin, status_height, 0);
    }

    /// Lays the page out below `top`, where the embedded result list sits.
    pub unsafe fn layout_below(
        &self,
        width: i32,
        height: i32,
        margin: i32,
        status_height: i32,
        top: i32,
    ) {
        let inner_width = (width - margin * 2).max(1);
        let video_height = ((height - top - status_height - margin * 4) / 4).clamp(80, 180);
        let _ = MoveWindow(
            self.video_host,
            margin,
            margin + top,
            inner_width,
            video_height,
            true,
        );

        let active: Vec<_> = self
            .controls
            .iter()
            .filter(|control| control.active)
            .collect();
        if active.is_empty() {
            return;
        }
        let gap = 6;
        let controls_top = margin * 2 + top + video_height;
        let details_height = if self.details.visible {
            DETAILS_LABEL_HEIGHT + DETAILS_TEXT_HEIGHT + DETAILS_BUTTON_HEIGHT + gap * 3
        } else {
            0
        };
        let available_height =
            (height - controls_top - status_height - margin * 2 - details_height).max(28);
        if self.details.visible {
            self.details.layout(
                margin,
                controls_top + available_height + gap,
                inner_width,
                gap,
            );
        }
        let preferred_button_height = 34;
        let rows_at_preferred = (available_height / (preferred_button_height + gap)).max(1);
        let required_columns = div_ceil(
            active.len(),
            usize::try_from(rows_at_preferred).unwrap_or(1),
        );
        let maximum_columns = usize::try_from((inner_width / 140).max(1)).unwrap_or(1);
        let columns = required_columns.clamp(1, maximum_columns);
        let rows = div_ceil(active.len(), columns);
        let rows_i32 = i32::try_from(rows).unwrap_or(i32::MAX).max(1);
        let button_height = ((available_height - gap * (rows_i32 - 1)) / rows_i32).clamp(24, 34);
        let columns_i32 = i32::try_from(columns).unwrap_or(1).max(1);
        let button_width = ((inner_width - gap * (columns_i32 - 1)) / columns_i32).max(80);

        for (index, control) in active.into_iter().enumerate() {
            let column = index / rows;
            let row = index % rows;
            let x = margin + i32::try_from(column).unwrap_or_default() * (button_width + gap);
            let y = controls_top + i32::try_from(row).unwrap_or_default() * (button_height + gap);
            let _ = MoveWindow(control.window, x, y, button_width, button_height, true);
        }
    }

    pub fn activation_for_command(&self, command_id: usize) -> Option<PlayerControlActivation> {
        if self.details.visible {
            match command_id {
                DETAILS_COPY_ID => return Some(PlayerControlActivation::CopyDetails),
                DETAILS_BACK_ID => return Some(PlayerControlActivation::HideDetails),
                _ => {}
            }
        }
        let control = self
            .controls
            .iter()
            .find(|control| control.command_id == command_id && control.active)?;
        control.action_id.map_or_else(
            || {
                (control.id == "session_autoplay_next")
                    .then_some(PlayerControlActivation::SessionAutoplayNext)
            },
            |action| Some(PlayerControlActivation::Action(action)),
        )
    }

    /// Puts a native checkbox back to the session state, for example after an
    /// action that could not be performed.
    pub unsafe fn set_checked(&self, id: &str, checked: bool) {
        if let Some(control) = self
            .controls
            .iter()
            .find(|control| control.id == id && control.role == NativeRole::Checkbox)
        {
            SendMessageW(
                control.window,
                BM_SETCHECK,
                Some(WPARAM(if checked { BST_CHECKED } else { 0 })),
                None,
            );
        }
    }

    pub fn control_id_for_window(&self, window: HWND) -> Option<&'static str> {
        if window == self.video_host {
            return Some("video_host");
        }
        self.controls
            .iter()
            .find(|control| control.active && control.window == window)
            .map(|control| control.id)
    }

    pub fn window_for_id(&self, id: &str) -> Option<HWND> {
        if id == "video_host" {
            return Some(self.video_host);
        }
        self.controls
            .iter()
            .find(|control| control.active && control.id == id)
            .map(|control| control.window)
    }

    pub fn initial_focus(&self) -> HWND {
        if self.details.visible {
            return self.details.text;
        }
        self.window_for_id(self.initial_focus_id)
            .unwrap_or(self.video_host)
    }

    pub fn is_native_action_control(&self, window: HWND) -> bool {
        self.controls
            .iter()
            .any(|control| control.active && control.window == window)
            || (self.details.visible && [self.details.copy, self.details.back].contains(&window))
    }
}

impl DetailsPanel {
    unsafe fn create(parent: HWND, instance: HINSTANCE, font: Option<WPARAM>) -> Result<Self> {
        let label = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            PCWSTR::null(),
            WS_CHILD,
            0,
            0,
            100,
            DETAILS_LABEL_HEIGHT,
            Some(parent),
            None,
            Some(instance),
            None,
        )?;
        let text = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            PCWSTR::null(),
            WS_CHILD
                | WS_TABSTOP
                | WS_VSCROLL
                | WS_HSCROLL
                | WINDOW_STYLE(
                    (ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | ES_AUTOHSCROLL) as u32,
                ),
            0,
            0,
            100,
            DETAILS_TEXT_HEIGHT,
            Some(parent),
            None,
            Some(instance),
            None,
        )?;
        let mut buttons = [HWND::default(); 2];
        for (button, id) in buttons.iter_mut().zip([DETAILS_COPY_ID, DETAILS_BACK_ID]) {
            *button = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                PCWSTR::null(),
                WS_CHILD | WS_TABSTOP,
                0,
                0,
                100,
                DETAILS_BUTTON_HEIGHT,
                Some(parent),
                Some(HMENU(id as *mut c_void)),
                Some(instance),
                None,
            )?;
        }
        for window in [label, text, buttons[0], buttons[1]] {
            SendMessageW(window, WM_SETFONT, font, Some(LPARAM(1)));
        }
        Ok(Self {
            label,
            text,
            copy: buttons[0],
            back: buttons[1],
            visible: false,
        })
    }

    unsafe fn show(&self, surface_visible: bool) {
        for window in [self.label, self.text, self.copy, self.back] {
            show(window, surface_visible && self.visible);
        }
    }

    unsafe fn layout(&self, left: i32, top: i32, width: i32, gap: i32) {
        let _ = MoveWindow(self.label, left, top, width, DETAILS_LABEL_HEIGHT, true);
        let text_top = top + DETAILS_LABEL_HEIGHT + gap;
        let _ = MoveWindow(self.text, left, text_top, width, DETAILS_TEXT_HEIGHT, true);
        let buttons_top = text_top + DETAILS_TEXT_HEIGHT + gap;
        let _ = MoveWindow(
            self.copy,
            left,
            buttons_top,
            160,
            DETAILS_BUTTON_HEIGHT,
            true,
        );
        let _ = MoveWindow(
            self.back,
            left + 160 + gap,
            buttons_top,
            120,
            DETAILS_BUTTON_HEIGHT,
            true,
        );
    }
}

unsafe fn create_native_control(
    parent: HWND,
    instance: HINSTANCE,
    spec: ControlSpec,
    command_id: usize,
    font: Option<WPARAM>,
) -> Result<NativeControl> {
    let mut style = WS_CHILD | WS_TABSTOP;
    if spec.role == NativeRole::Checkbox {
        style |= WINDOW_STYLE(BS_AUTOCHECKBOX as u32);
    }
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        PCWSTR::null(),
        style,
        0,
        0,
        100,
        30,
        Some(parent),
        Some(HMENU(command_id as *mut c_void)),
        Some(instance),
        None,
    )?;
    SendMessageW(window, WM_SETFONT, font, Some(LPARAM(1)));
    Ok(NativeControl {
        id: spec.id,
        command_id,
        role: spec.role,
        window,
        action_id: None,
        active: false,
    })
}

const fn role_matches(native: NativeRole, projected: PlayerControlRole) -> bool {
    matches!(
        (native, projected),
        (NativeRole::Button, PlayerControlRole::Button)
            | (NativeRole::Checkbox, PlayerControlRole::Checkbox)
    )
}

unsafe fn window_text(window: HWND) -> String {
    let length = usize::try_from(GetWindowTextLengthW(window)).unwrap_or_default();
    let mut buffer = vec![0_u16; length + 1];
    let copied = usize::try_from(GetWindowTextW(window, &mut buffer)).unwrap_or_default();
    String::from_utf16_lossy(&buffer[..copied])
}

unsafe fn set_text(window: HWND, value: &str) {
    let value = wide(value);
    let _ = SetWindowTextW(window, PCWSTR(value.as_ptr()));
}

unsafe fn show(window: HWND, visible: bool) {
    let _ = ShowWindow(window, if visible { SW_SHOW } else { SW_HIDE });
}

const fn div_ceil(value: usize, divisor: usize) -> usize {
    value.div_ceil(divisor)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use apricot_app::{PlayerScreenModel, PlayerToggle, PlayerViewState, english_catalog};
    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_storage::SettingsDocument;

    use super::{ACTION_SPECS, NAVIGATION_SPECS};

    #[test]
    fn native_inventory_covers_every_projected_player_control() {
        let mut native_ids: HashSet<_> = NAVIGATION_SPECS
            .iter()
            .chain(ACTION_SPECS)
            .map(|spec| spec.id)
            .collect();
        assert!(native_ids.insert("video_host"));
        let mut settings = SettingsDocument::default();
        for (source, kind, local_path) in [
            (MediaSource::Youtube, MediaKind::Video, None),
            (
                MediaSource::Local,
                MediaKind::Audio,
                Some(r"C:\Music\track.mp3".to_owned()),
            ),
            (MediaSource::Podcast, MediaKind::PodcastEpisode, None),
        ] {
            for background in [false, true] {
                for (autoplay, fullscreen) in [(false, false), (true, false), (false, true)] {
                    let mut view = PlayerViewState::default();
                    if fullscreen {
                        view.enabled_toggles.insert(PlayerToggle::Fullscreen);
                    }
                    settings.enable_background_playback = background;
                    settings.autoplay_next = autoplay;
                    let model = PlayerScreenModel::build(
                        &english_catalog(),
                        &settings,
                        &MediaItem {
                            id: MediaId("item".to_owned()),
                            source,
                            kind,
                            title: "Item".to_owned(),
                            url: None,
                            stream_url: None,
                            external_audio_url: None,
                            local_path: local_path.clone(),
                            channel: String::new(),
                            duration_seconds: None,
                            metadata: BTreeMap::new(),
                        },
                        &view,
                    );
                    for control in model.controls {
                        assert!(native_ids.contains(control.id), "missing {}", control.id);
                    }
                }
            }
        }
    }
}
