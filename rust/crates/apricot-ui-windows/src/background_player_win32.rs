//! Python `add_background_player_section`: a label, the player and a row of
//! buttons appended to every other screen while background playback runs.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;

use apricot_app::BackgroundPlayerModel;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        UI::WindowsAndMessaging::{
            CreateWindowExW, GetWindowTextLengthW, GetWindowTextW, HMENU, MoveWindow, SW_HIDE,
            SW_SHOW, SendMessageW, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WM_SETFONT,
            WS_CHILD, WS_TABSTOP,
        },
    },
    core::{PCWSTR, Result, w},
};

const COMMAND_ID_BASE: usize = 2_200;
const BUTTON_COUNT: usize = 13;
const LABEL_HEIGHT: i32 = 22;
const PLAYER_HEIGHT: i32 = 96;
const BUTTON_HEIGHT: i32 = 30;
const GAP: i32 = 6;

struct SectionButton {
    id: &'static str,
    action_id: &'static str,
    window: HWND,
}

pub struct BackgroundPlayerSection {
    label: HWND,
    buttons: Vec<SectionButton>,
    visible: bool,
}

impl BackgroundPlayerSection {
    /// Created after every other control so that the section follows the
    /// screen in Tab order, as Python appends it to the end of the screen.
    pub unsafe fn create(parent: HWND, instance: HINSTANCE) -> Result<Self> {
        let font = GetStockObject(DEFAULT_GUI_FONT);
        let font_param = Some(WPARAM(font.0 as usize));
        let label = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            PCWSTR::null(),
            WS_CHILD,
            0,
            0,
            100,
            LABEL_HEIGHT,
            Some(parent),
            None,
            Some(instance),
            None,
        )?;
        SendMessageW(label, WM_SETFONT, font_param, Some(LPARAM(1)));
        let mut buttons = Vec::with_capacity(BUTTON_COUNT);
        for index in 0..BUTTON_COUNT {
            let window = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                PCWSTR::null(),
                WS_CHILD | WS_TABSTOP,
                0,
                0,
                100,
                BUTTON_HEIGHT,
                Some(parent),
                Some(HMENU((COMMAND_ID_BASE + index) as *mut c_void)),
                Some(instance),
                None,
            )?;
            SendMessageW(window, WM_SETFONT, font_param, Some(LPARAM(1)));
            buttons.push(SectionButton {
                id: "",
                action_id: "",
                window,
            });
        }
        Ok(Self {
            label,
            buttons,
            visible: false,
        })
    }

    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// The label the player is placed after in z-order, so Tab reaches the
    /// player right after the screen and right before the buttons.
    pub const fn label(&self) -> HWND {
        self.label
    }

    /// Height taken from the bottom of the window while the section shows.
    pub const fn height() -> i32 {
        LABEL_HEIGHT + PLAYER_HEIGHT + BUTTON_HEIGHT + GAP * 4
    }

    /// Shows the section with fresh labels, or hides it for `None`.
    pub unsafe fn sync(&mut self, model: Option<&BackgroundPlayerModel>) {
        let Some(model) = model else {
            self.visible = false;
            show(self.label, false);
            for button in &self.buttons {
                show(button.window, false);
            }
            return;
        };
        set_text_if_changed(self.label, &model.label);
        for (native, projected) in self.buttons.iter_mut().zip(&model.buttons) {
            native.id = projected.id;
            native.action_id = projected.action_id;
            set_text_if_changed(native.window, &projected.label);
        }
        self.visible = true;
        show(self.label, true);
        for button in &self.buttons {
            show(button.window, true);
        }
    }

    pub unsafe fn layout(&self, player: HWND, left: i32, top: i32, width: i32) {
        let _ = MoveWindow(self.label, left, top, width, LABEL_HEIGHT, true);
        let player_top = top + LABEL_HEIGHT + GAP;
        let _ = MoveWindow(player, left, player_top, width, PLAYER_HEIGHT, true);
        let buttons_top = player_top + PLAYER_HEIGHT + GAP;
        let count = i32::try_from(self.buttons.len()).unwrap_or(1).max(1);
        let button_width = ((width - GAP * (count - 1)) / count).max(40);
        for (index, button) in self.buttons.iter().enumerate() {
            let x = left + i32::try_from(index).unwrap_or_default() * (button_width + GAP);
            let _ = MoveWindow(
                button.window,
                x,
                buttons_top,
                button_width,
                BUTTON_HEIGHT,
                true,
            );
        }
    }

    pub fn action_for_command(&self, command_id: usize) -> Option<&'static str> {
        if !self.visible {
            return None;
        }
        self.buttons
            .iter()
            .enumerate()
            .find(|(index, _)| COMMAND_ID_BASE + index == command_id)
            .map(|(_, button)| button.action_id)
    }

    pub fn contains_button(&self, window: HWND) -> bool {
        self.visible && self.buttons.iter().any(|button| button.window == window)
    }

    pub fn is_play_pause_button(&self, window: HWND) -> bool {
        self.visible
            && self
                .buttons
                .iter()
                .any(|button| button.window == window && button.id == "play_pause")
    }

    /// The buttons in Tab order.
    pub fn button_windows(&self) -> Vec<HWND> {
        if !self.visible {
            return Vec::new();
        }
        self.buttons.iter().map(|button| button.window).collect()
    }
}

unsafe fn set_text_if_changed(window: HWND, value: &str) {
    let length = usize::try_from(GetWindowTextLengthW(window)).unwrap_or_default();
    let mut buffer = vec![0_u16; length + 1];
    let copied = usize::try_from(GetWindowTextW(window, &mut buffer)).unwrap_or_default();
    if String::from_utf16_lossy(&buffer[..copied]) == value {
        return;
    }
    let value: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let _ = SetWindowTextW(window, PCWSTR(value.as_ptr()));
}

unsafe fn show(window: HWND, visible: bool) {
    let _ = ShowWindow(window, if visible { SW_SHOW } else { SW_HIDE });
}
