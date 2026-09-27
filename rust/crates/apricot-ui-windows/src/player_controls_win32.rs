//! Native player controls with real button and checkbox accessibility roles.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;

use apricot_app::{PlayerControlRole, PlayerScreenModel};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        UI::WindowsAndMessaging::{
            BS_AUTOCHECKBOX, CreateWindowExW, HMENU, MoveWindow, SW_HIDE, SW_SHOW, SendMessageW,
            SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETFONT, WS_CHILD,
            WS_EX_CLIENTEDGE, WS_GROUP, WS_TABSTOP,
        },
    },
    core::{PCWSTR, Result, w},
};

const CONTROL_ID_BASE: usize = 2_000;
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

const NAVIGATION_SPECS: &[ControlSpec] =
    &[button("back"), button("back_results"), button("back_main")];

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
}

pub struct PlayerControls {
    video_host: HWND,
    controls: Vec<NativeControl>,
    surface_visible: bool,
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
        Ok(Self {
            video_host,
            controls,
            surface_visible: false,
            initial_focus_id: "video_host",
        })
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
        show(self.video_host, self.surface_visible);
    }

    pub unsafe fn set_visible(&mut self, visible: bool) {
        self.surface_visible = visible;
        show(self.video_host, visible);
        for control in &self.controls {
            show(control.window, visible && control.active);
        }
    }

    pub unsafe fn layout(&self, width: i32, height: i32, margin: i32, status_height: i32) {
        let inner_width = (width - margin * 2).max(1);
        let video_height = ((height - status_height - margin * 4) / 4).clamp(80, 180);
        let _ = MoveWindow(
            self.video_host,
            margin,
            margin,
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
        let controls_top = margin * 2 + video_height;
        let available_height = (height - controls_top - status_height - margin * 2).max(28);
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
        self.window_for_id(self.initial_focus_id)
            .unwrap_or(self.video_host)
    }

    pub fn is_native_action_control(&self, window: HWND) -> bool {
        self.controls
            .iter()
            .any(|control| control.active && control.window == window)
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

    use apricot_app::{PlayerScreenModel, PlayerViewState, english_catalog};
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
                for autoplay in [false, true] {
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
                        &PlayerViewState::default(),
                    );
                    for control in model.controls {
                        assert!(native_ids.contains(control.id), "missing {}", control.id);
                    }
                }
            }
        }
    }
}
