//! Spotify devices dialog (`docs/SPOTIFY_PLAN.md` 4.2 Devices): the Connect
//! devices of the account with this computer first, the playing one marked.
//! Enter (or the button, or the context menu) moves playback to the selected
//! device and closes the dialog. The list follows the confirmed Connect state
//! through [`update`]; the focus never moves on an update.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, SetFocus, VK_ESCAPE, VK_RETURN},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                AppendMenuW, BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetClientRect,
                GetCursorPos, GetMessageW, GetParent, GetWindowLongPtrW, HMENU, IDC_ARROW,
                IsDialogMessageW, IsWindow, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT,
                LB_SETCURSEL, LBN_DBLCLK, LBS_NOTIFY, LoadCursorW, MF_STRING, MSG, MoveWindow,
                PostQuitMessage, RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow,
                SetWindowLongPtrW, ShowWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
                TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX,
                WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_KEYDOWN, WM_NCDESTROY,
                WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW,
                WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaSpotifyDevicesWindow");
const ID_LIST: usize = 1701;
const ID_PLAY: usize = 1702;
const ID_BACK: usize = 1703;
/// `IsDialogMessageW` turns Enter and Escape into `IDOK` and `IDCANCEL`.
const IDOK_COMMAND: usize = 1;
const IDCANCEL_COMMAND: usize = 2;

pub struct SpotifyDevicesDialogLabels {
    pub title: String,
    pub instructions: String,
    pub play: String,
    pub back: String,
}

/// One row: the device ID and its text. An empty ID is a message row.
pub type DeviceRow = (String, String);

struct SpotifyDevicesDialogState {
    labels: SpotifyDevicesDialogLabels,
    /// Moves playback to the device; `true` closes the dialog.
    play: Box<dyn Fn(&str) -> bool>,
    rows: Vec<DeviceRow>,
    instructions: HWND,
    list: HWND,
    play_button: HWND,
    back: HWND,
}

pub unsafe fn register() -> Result<()> {
    let module = GetModuleHandleW(None)?;
    let class = WNDCLASSW {
        cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size fits in i32"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hInstance: HINSTANCE(module.0),
        lpszClassName: CLASS_NAME,
        lpfnWndProc: Some(window_proc),
        ..Default::default()
    };
    if RegisterClassW(&raw const class) == 0 {
        return Err(windows::core::Error::from_thread());
    }
    Ok(())
}

/// Shows the dialog modally; `opened` receives its window for [`update`].
pub unsafe fn show(
    owner: HWND,
    rows: Vec<DeviceRow>,
    labels: SpotifyDevicesDialogLabels,
    play: Box<dyn Fn(&str) -> bool>,
    opened: impl FnOnce(HWND),
) -> Result<()> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let title = wide(&labels.title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS_NAME,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        640,
        420,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let state = match create_controls(window, instance, rows, labels, play) {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let initial_focus = state.list;
    let state_pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), state_pointer as isize);
    fill_list(window, 0);
    layout(window);
    opened(window);
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetFocus(Some(initial_focus));

    let mut loop_error = None;
    let mut message = MSG::default();
    while IsWindow(Some(window)).as_bool() {
        let status = GetMessageW(&raw mut message, None, 0, 0);
        if status.0 == -1 {
            loop_error = Some(windows::core::Error::from_thread());
            let _ = DestroyWindow(window);
            break;
        }
        if status.0 == 0 {
            let _ = DestroyWindow(window);
            PostQuitMessage(0);
            break;
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    drop(Box::from_raw(state_pointer));
    loop_error.map_or(Ok(()), Err)
}

/// The device list changed. The same device stays selected, or the same
/// position when it is gone; an unchanged list is left alone.
pub unsafe fn update(window: HWND, rows: Vec<DeviceRow>) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.rows == rows {
        return;
    }
    let index = selected_index(state).unwrap_or(0);
    let selected = state.rows.get(index).map(|(id, _)| id.clone());
    state.rows = rows;
    let selection = selected
        .filter(|id| !id.is_empty())
        .and_then(|id| state.rows.iter().position(|(row, _)| *row == id))
        .unwrap_or(index);
    fill_list(window, selection);
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SIZE => {
            layout(window);
            LRESULT(0)
        }
        WM_COMMAND => {
            let command = wparam.0 & 0xffff;
            let notification = (wparam.0 >> 16) & 0xffff;
            if command == ID_PLAY
                || command == IDOK_COMMAND
                || (command == ID_LIST
                    && notification == usize::try_from(LBN_DBLCLK).expect("notification fits"))
            {
                play_selected(window);
            } else if command == ID_BACK || command == IDCANCEL_COMMAND {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(window);
            LRESULT(0)
        }
        WM_CONTEXTMENU => {
            show_context_menu(window);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn create_controls(
    parent: HWND,
    instance: HINSTANCE,
    rows: Vec<DeviceRow>,
    labels: SpotifyDevicesDialogLabels,
    play: Box<dyn Fn(&str) -> bool>,
) -> Result<SpotifyDevicesDialogState> {
    let instructions = create_control(
        parent,
        instance,
        w!("STATIC"),
        &labels.instructions,
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let list = create_control(
        parent,
        instance,
        w!("LISTBOX"),
        &labels.title,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
        WS_EX_CLIENTEDGE,
        ID_LIST,
    )?;
    let play_button = create_button(parent, instance, &labels.play, ID_PLAY, true)?;
    let back = create_button(parent, instance, &labels.back, ID_BACK, false)?;
    if !SetWindowSubclass(list, Some(list_proc), 1, 0).as_bool() {
        return Err(windows::core::Error::from_thread());
    }
    crate::accessibility_win32::set_control_name(list, &labels.title);
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in [instructions, list, play_button, back] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    Ok(SpotifyDevicesDialogState {
        labels,
        play,
        rows,
        instructions,
        list,
        play_button,
        back,
    })
}

unsafe fn create_button(
    parent: HWND,
    instance: HINSTANCE,
    label: &str,
    id: usize,
    default: bool,
) -> Result<HWND> {
    let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;
    if default {
        style |= WINDOW_STYLE(BS_DEFPUSHBUTTON as u32);
    }
    create_control(
        parent,
        instance,
        w!("BUTTON"),
        label,
        style,
        WINDOW_EX_STYLE::default(),
        id,
    )
}

unsafe fn create_control(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    label: &str,
    style: WINDOW_STYLE,
    extended_style: WINDOW_EX_STYLE,
    id: usize,
) -> Result<HWND> {
    let label = wide(label);
    CreateWindowExW(
        extended_style,
        class,
        PCWSTR(label.as_ptr()),
        style,
        0,
        0,
        100,
        30,
        Some(parent),
        Some(HMENU(id as *mut c_void)),
        Some(instance),
        None,
    )
}

unsafe extern "system" fn list_proc(
    control: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    if message == WM_KEYDOWN
        && let Ok(parent) = GetParent(control)
    {
        if wparam.0 == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(parent);
            return LRESULT(0);
        }
        if wparam.0 == usize::from(VK_RETURN.0) {
            play_selected(parent);
            return LRESULT(0);
        }
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(control, Some(list_proc), subclass_id);
    }
    DefSubclassProc(control, message, wparam, lparam)
}

unsafe fn selected_index(state: &SpotifyDevicesDialogState) -> Option<usize> {
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    let selected = usize::try_from(selected).ok()?;
    (selected < state.rows.len()).then_some(selected)
}

unsafe fn selected_device(window: HWND) -> Option<String> {
    let state = state(window)?;
    let (id, _) = state.rows.get(selected_index(state)?)?;
    (!id.is_empty()).then(|| id.clone())
}

unsafe fn play_selected(window: HWND) {
    let Some(id) = selected_device(window) else {
        return;
    };
    if state(window).is_some_and(|state| (state.play)(&id)) {
        let _ = DestroyWindow(window);
    }
}

/// Applications key, Shift+F10 and the right button: the one action.
unsafe fn show_context_menu(window: HWND) {
    if selected_device(window).is_none() {
        return;
    }
    let Some(state) = state(window) else {
        return;
    };
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let label = wide(&state.labels.play);
    let _ = AppendMenuW(menu, MF_STRING, ID_PLAY, PCWSTR(label.as_ptr()));
    let mut point = POINT::default();
    if GetCursorPos(&raw mut point).is_ok() {
        let selected = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            None,
            window,
            None,
        );
        if usize::try_from(selected.0).unwrap_or_default() == ID_PLAY {
            play_selected(window);
        }
    }
    let _ = DestroyMenu(menu);
}

unsafe fn fill_list(window: HWND, selection: usize) {
    let Some(state) = state(window) else {
        return;
    };
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    for (_, label) in &state.rows {
        let value = wide(label);
        SendMessageW(
            state.list,
            LB_ADDSTRING,
            None,
            Some(LPARAM(value.as_ptr() as isize)),
        );
    }
    let selection = selection.min(state.rows.len().saturating_sub(1));
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selection)), None);
}

unsafe fn layout(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let width = (bounds.right - bounds.left).max(420);
    let height = (bounds.bottom - bounds.top).max(280);
    let margin = 12;
    let instructions_height = 34;
    let button_height = 34;
    let gap = 6;
    let button_width = ((width - margin * 2 - gap) / 2).max(120);
    let _ = MoveWindow(
        state.instructions,
        margin,
        margin,
        width - margin * 2,
        instructions_height,
        true,
    );
    let list_top = margin + instructions_height;
    let button_top = height - margin - button_height;
    let _ = MoveWindow(
        state.list,
        margin,
        list_top,
        width - margin * 2,
        button_top - list_top - margin,
        true,
    );
    for (index, control) in [state.play_button, state.back].into_iter().enumerate() {
        let left = margin + i32::try_from(index).unwrap_or_default() * (button_width + gap);
        let _ = MoveWindow(control, left, button_top, button_width, button_height, true);
    }
}

unsafe fn state(window: HWND) -> Option<&'static SpotifyDevicesDialogState> {
    let pointer =
        GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const SpotifyDevicesDialogState;
    pointer.as_ref()
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut SpotifyDevicesDialogState> {
    let pointer =
        GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut SpotifyDevicesDialogState;
    pointer.as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
