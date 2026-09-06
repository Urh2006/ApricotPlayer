//! Native accessible read-only player-details dialog.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::mem::size_of;

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, SetFocus, VK_ESCAPE},
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, GetClientRect,
                GetMessageW, GetWindowLongPtrW, HMENU, IDC_ARROW, IsDialogMessageW, IsWindow,
                LoadCursorW, MSG, MoveWindow, PostQuitMessage, RegisterClassW, SW_SHOW,
                SendMessageW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TranslateMessage,
                WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
                WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CHILD,
                WS_EX_CLIENTEDGE, WS_HSCROLL, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
                WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaDetailsWindow");
const ID_TEXT: usize = 1601;
const ID_COPY: usize = 1602;
const ID_BACK: usize = 1603;

pub struct DetailsDialogLabels {
    pub title: String,
    pub copy: String,
    pub copied: String,
    pub back: String,
}

struct DetailsDialogState {
    text_value: String,
    copied_message: String,
    text: HWND,
    copy: HWND,
    back: HWND,
    status: HWND,
    announcer: crate::announcement_win32::WindowsAnnouncer,
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

pub unsafe fn show(owner: HWND, text_value: String, labels: &DetailsDialogLabels) -> Result<()> {
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
        760,
        560,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let state = match create_controls(window, instance, text_value, labels) {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let initial_focus = state.text;
    let state_pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), state_pointer as isize);
    layout(window);
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
        if message.message == WM_KEYDOWN && message.wParam.0 == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(window);
            continue;
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    let state = Box::from_raw(state_pointer);
    if let Some(error) = loop_error {
        return Err(error);
    }
    drop(state);
    Ok(())
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
            match wparam.0 & 0xffff {
                ID_COPY => copy_details(window),
                ID_BACK => {
                    let _ = DestroyWindow(window);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(window);
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
    text_value: String,
    labels: &DetailsDialogLabels,
) -> Result<DetailsDialogState> {
    let text_wide = wide(&text_value);
    let text = create_control(
        parent,
        instance,
        w!("EDIT"),
        PCWSTR(text_wide.as_ptr()),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WINDOW_STYLE(ES_MULTILINE as u32)
            | WINDOW_STYLE(ES_READONLY as u32)
            | WINDOW_STYLE(ES_AUTOVSCROLL as u32)
            | WS_VSCROLL
            | WS_HSCROLL,
        WS_EX_CLIENTEDGE,
        ID_TEXT,
    )?;
    let copy_label = wide(&labels.copy);
    let copy = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(copy_label.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        ID_COPY,
    )?;
    let back_label = wide(&labels.back);
    let back = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(back_label.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_BACK,
    )?;
    let status = create_control(
        parent,
        instance,
        w!("STATIC"),
        w!(""),
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in [text, copy, back, status] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    Ok(DetailsDialogState {
        text_value,
        copied_message: labels.copied.clone(),
        text,
        copy,
        back,
        status,
        announcer: crate::announcement_win32::WindowsAnnouncer::new(status),
    })
}

unsafe fn create_control(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    name: PCWSTR,
    style: WINDOW_STYLE,
    extended_style: WINDOW_EX_STYLE,
    id: usize,
) -> Result<HWND> {
    CreateWindowExW(
        extended_style,
        class,
        name,
        style,
        0,
        0,
        0,
        0,
        Some(parent),
        Some(HMENU(id as *mut _)),
        Some(instance),
        None,
    )
}

unsafe fn copy_details(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if crate::clipboard_win32::copy_text(window, &state.text_value).is_ok() {
        state.announcer.announce(&state.copied_message, true);
    }
}

unsafe fn layout(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let mut rect = RECT::default();
    if GetClientRect(window, &raw mut rect).is_err() {
        return;
    }
    let margin = 12;
    let gap = 8;
    let button_height = 30;
    let status_height = 24;
    let button_width = 130;
    let width = (rect.right - rect.left - margin * 2).max(1);
    let text_height =
        (rect.bottom - rect.top - margin * 2 - gap * 2 - button_height - status_height).max(1);
    let _ = MoveWindow(state.text, margin, margin, width, text_height, true);
    let button_y = margin + text_height + gap;
    let _ = MoveWindow(
        state.copy,
        margin,
        button_y,
        button_width,
        button_height,
        true,
    );
    let _ = MoveWindow(
        state.back,
        margin + button_width + gap,
        button_y,
        button_width,
        button_height,
        true,
    );
    let _ = MoveWindow(
        state.status,
        margin,
        button_y + button_height + gap,
        width,
        status_height,
        true,
    );
}

unsafe fn state(window: HWND) -> Option<&'static DetailsDialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0));
    (!pointer.eq(&0)).then(|| &*(pointer as *const DetailsDialogState))
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut DetailsDialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0));
    (!pointer.eq(&0)).then(|| &mut *(pointer as *mut DetailsDialogState))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
