//! Native source list with keyboard, context menu and stable focused selection.
#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use apricot_shell::sources::{SourceBrowser, SourceSelection};
use std::{ffi::c_void, mem::size_of};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{ClientToScreen, DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                EnableWindow, GetFocus, GetKeyState, IsWindowEnabled, SetFocus, VK_APPS, VK_F10,
                VK_SHIFT,
            },
            WindowsAndMessaging::{
                AppendMenuW, BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetClientRect,
                GetMessageW, GetWindowLongPtrW, HMENU, IDC_ARROW, IsChild, IsDialogMessageW,
                IsWindow, LB_ADDSTRING, LB_GETCURSEL, LB_SETCURSEL, LBN_DBLCLK, LBS_NOTIFY,
                LoadCursorW, MF_STRING, MSG, MoveWindow, PostQuitMessage, RegisterClassW, SW_SHOW,
                SendMessageW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TPM_LEFTALIGN,
                TPM_RETURNCMD, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU,
                WM_KEYDOWN, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CHILD, WS_EX_DLGMODALFRAME,
                WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, w},
};

const CLASS: PCWSTR = w!("ApricotSourcesBrowser");
const LIST: usize = 100;
const OPEN: usize = 101;
const ACTIONS: usize = 102;
const BACK: usize = 103;
struct State {
    model: SourceBrowser,
    prompt: HWND,
    list: HWND,
    open: HWND,
    actions: HWND,
    back: HWND,
    result: Option<SourceSelection>,
}

pub(crate) fn show(owner: HWND, model: &SourceBrowser) -> Option<SourceSelection> {
    // SAFETY: The modal loop exclusively owns all dialog controls and state.
    unsafe { show_inner(owner, model).ok().flatten() }
}

#[allow(clippy::too_many_lines)] // Creates the window and runs its modal loop.
unsafe fn show_inner(
    owner: HWND,
    model: &SourceBrowser,
) -> windows::core::Result<Option<SourceSelection>> {
    let instance = HINSTANCE(GetModuleHandleW(None)?.0);
    let _ = RegisterClassW(&WNDCLASSW {
        cbWndExtra: i32::try_from(size_of::<isize>()).unwrap_or(8),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hInstance: instance,
        lpszClassName: CLASS,
        lpfnWndProc: Some(window_proc),
        ..Default::default()
    });
    let title = wide(&model.title);
    let window = CreateWindowExW(
        WS_EX_DLGMODALFRAME,
        CLASS,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        900,
        620,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let prompt = child(
        window,
        instance,
        w!("STATIC"),
        &model.prompt,
        WINDOW_STYLE::default(),
        0,
    )?;
    let list = child(
        window,
        instance,
        w!("LISTBOX"),
        "",
        WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
        LIST,
    )?;
    crate::accessibility_win32::set_control_name(list, &model.title);
    for row in &model.rows {
        let row = wide(row);
        SendMessageW(
            list,
            LB_ADDSTRING,
            None,
            Some(LPARAM(row.as_ptr() as isize)),
        );
    }
    if !model.rows.is_empty() {
        SendMessageW(
            list,
            LB_SETCURSEL,
            Some(WPARAM(model.selected.min(model.rows.len() - 1))),
            None,
        );
    }
    let open = child(
        window,
        instance,
        w!("BUTTON"),
        model.actions.first().map_or("Open", |action| &action.label),
        WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        OPEN,
    )?;
    let actions = child(
        window,
        instance,
        w!("BUTTON"),
        &model.actions_button,
        WS_TABSTOP,
        ACTIONS,
    )?;
    let back = child(
        window,
        instance,
        w!("BUTTON"),
        &model.back_button,
        WS_TABSTOP,
        BACK,
    )?;
    let mut state = Box::new(State {
        model: model.clone(),
        prompt,
        list,
        open,
        actions,
        back,
        result: None,
    });
    SetWindowLongPtrW(
        window,
        WINDOW_LONG_PTR_INDEX(0),
        std::ptr::from_mut(state.as_mut()) as isize,
    );
    let previous = GetFocus();
    let owner_enabled = IsWindowEnabled(owner).as_bool();
    let _ = EnableWindow(owner, false);
    layout(window, &state);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetForegroundWindow(window);
    let _ = SetFocus(Some(list));
    let mut message = MSG::default();
    while IsWindow(Some(window)).as_bool() {
        let received = GetMessageW(&raw mut message, None, 0, 0).0;
        if received <= 0 {
            if received == 0 {
                PostQuitMessage(i32::try_from(message.wParam.0).unwrap_or_default());
            }
            let _ = DestroyWindow(window);
            break;
        }
        if message.message == WM_KEYDOWN && IsChild(window, message.hwnd).as_bool() {
            let key = message.wParam.0;
            if key == usize::from(VK_APPS.0)
                || (key == usize::from(VK_F10.0) && GetKeyState(i32::from(VK_SHIFT.0)) < 0)
            {
                menu(window, &mut state);
                continue;
            }
            if let Some(chord) = crate::shortcut_win32::chord_from_message(&message)
                && let Some(action) = state
                    .model
                    .actions
                    .iter()
                    .find(|action| {
                        !action.shortcut.is_empty() && chord.matches_configured(&action.shortcut)
                    })
                    .map(|action| action.id.to_owned())
            {
                finish(window, &mut state, &action);
                continue;
            }
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    if owner_enabled {
        let _ = EnableWindow(owner, true);
    }
    let _ = SetForegroundWindow(owner);
    if IsWindow(Some(previous)).as_bool() {
        let _ = SetFocus(Some(previous));
    }
    Ok(state.result.take())
}

unsafe fn child(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    title: &str,
    style: WINDOW_STYLE,
    id: usize,
) -> windows::core::Result<HWND> {
    let title = wide(title);
    let control = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class,
        PCWSTR(title.as_ptr()),
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        0,
        0,
        Some(parent),
        Some(HMENU(id as *mut c_void)),
        Some(instance),
        None,
    )?;
    SendMessageW(
        control,
        WM_SETFONT,
        Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)),
        Some(LPARAM(1)),
    );
    Ok(control)
}

unsafe fn layout(window: HWND, state: &State) {
    let mut rect = RECT::default();
    let _ = GetClientRect(window, &raw mut rect);
    let width = rect.right.max(400);
    let height = rect.bottom.max(250);
    let _ = MoveWindow(state.prompt, 12, 12, width - 24, 40, true);
    let _ = MoveWindow(state.list, 12, 58, width - 24, height - 118, true);
    for (index, control) in [state.open, state.actions, state.back]
        .into_iter()
        .enumerate()
    {
        let left = 12 + i32::try_from(index).unwrap_or_default() * 170;
        let _ = MoveWindow(control, left, height - 48, 158, 34, true);
    }
}

unsafe fn finish(window: HWND, state: &mut State, action: &str) {
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    state.result = Some(SourceSelection {
        index: usize::try_from(selected).unwrap_or_default(),
        action: action.into(),
    });
    let _ = DestroyWindow(window);
}

unsafe fn menu(window: HWND, state: &mut State) {
    let Ok(menu) = CreatePopupMenu() else { return };
    for (index, action) in state.model.actions.iter().enumerate() {
        let label = if action.shortcut.is_empty() {
            action.label.clone()
        } else {
            format!("{}\t{}", action.label, action.shortcut)
        };
        let label = wide(&label);
        let _ = AppendMenuW(menu, MF_STRING, index + 1, PCWSTR(label.as_ptr()));
    }
    let mut point = POINT::default();
    let _ = ClientToScreen(state.list, &raw mut point);
    let selected = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_LEFTALIGN,
        point.x + 16,
        point.y + 18,
        Some(0),
        window,
        None,
    )
    .0;
    let _ = DestroyMenu(menu);
    if let Some(index) = usize::try_from(selected)
        .ok()
        .and_then(|selected| selected.checked_sub(1))
        && let Some(action) = state
            .model
            .actions
            .get(index)
            .map(|action| action.id.to_owned())
    {
        finish(window, state, &action);
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut State;
    if !pointer.is_null() {
        let state = &mut *pointer;
        match message {
            WM_SIZE => {
                layout(window, state);
                return LRESULT(0);
            }
            WM_CLOSE => {
                let _ = DestroyWindow(window);
                return LRESULT(0);
            }
            WM_CONTEXTMENU => {
                menu(window, state);
                return LRESULT(0);
            }
            WM_COMMAND => {
                let id = wparam.0 & 0xffff;
                let notification = (wparam.0 >> 16) & 0xffff;
                match id {
                    1 | OPEN => {
                        if let Some(action) = state
                            .model
                            .actions
                            .first()
                            .map(|action| action.id.to_owned())
                        {
                            finish(window, state, &action);
                        }
                        return LRESULT(0);
                    }
                    2 | BACK => {
                        let _ = DestroyWindow(window);
                        return LRESULT(0);
                    }
                    ACTIONS => {
                        menu(window, state);
                        return LRESULT(0);
                    }
                    LIST if notification == LBN_DBLCLK as usize => {
                        if let Some(action) = state
                            .model
                            .actions
                            .first()
                            .map(|action| action.id.to_owned())
                        {
                            finish(window, state, &action);
                        }
                        return LRESULT(0);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    DefWindowProcW(window, message, wparam, lparam)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
