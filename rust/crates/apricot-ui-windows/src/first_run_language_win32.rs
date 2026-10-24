//! Native one-time language selection shown before the main window.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use apricot_core::locale::LANGUAGES;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE, VK_RETURN},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, GetClientRect, GetMessageW, GetWindowLongPtrW, HMENU, IDC_ARROW,
                IsDialogMessageW, IsWindow, LB_ADDSTRING, LB_GETCURSEL, LB_SETCURSEL, LBN_DBLCLK,
                LBS_NOTIFY, LoadCursorW, MSG, MoveWindow, PostQuitMessage, RegisterClassW, SW_SHOW,
                SendMessageW, SetWindowLongPtrW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_KEYDOWN,
                WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE,
                WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaFirstRunLanguageWindow");
const ID_LANGUAGE_LIST: usize = 1501;
const ID_OK: usize = 1502;
const ID_CANCEL: usize = 1503;
/// `IsDialogMessageW` turns Enter and Escape into `IDOK` and `IDCANCEL`
/// before the focused list sees the key.
const IDOK_COMMAND: usize = 1;
const IDCANCEL_COMMAND: usize = 2;

struct LanguageWindowState {
    prompt: HWND,
    list: HWND,
    ok: HWND,
    cancel: HWND,
    selected: Option<&'static str>,
}

/// Shows the one-time accessible language chooser.
///
/// # Errors
///
/// Returns a Win32 error when the dialog class, controls, or message loop fail.
pub fn show(current_language: &str) -> Result<Option<&'static str>> {
    // SAFETY: This one-time dialog owns its controls and state on this thread.
    // Its state allocation is recovered exactly once after WM_NCDESTROY.
    unsafe { show_win32(current_language) }
}

unsafe fn show_win32(current_language: &str) -> Result<Option<&'static str>> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let class = WNDCLASSW {
        cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size fits in i32"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        lpfnWndProc: Some(window_proc),
        ..Default::default()
    };
    if RegisterClassW(&raw const class) == 0 {
        return Err(windows::core::Error::from_thread());
    }
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS_NAME,
        w!("Language"),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        520,
        560,
        None,
        None,
        Some(instance),
        None,
    )?;
    let state = match create_controls(window, instance, current_language) {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let initial_focus = state.list;
    let state_pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), state_pointer as isize);
    layout(window);
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
    let state = Box::from_raw(state_pointer);
    if let Some(error) = loop_error {
        return Err(error);
    }
    Ok(state.selected)
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
            if command == ID_OK
                || command == IDOK_COMMAND
                || (command == ID_LANGUAGE_LIST
                    && notification == usize::try_from(LBN_DBLCLK).expect("notification fits"))
            {
                accept(window);
            } else if command == ID_CANCEL || command == IDCANCEL_COMMAND {
                let _ = DestroyWindow(window);
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
    current_language: &str,
) -> Result<LanguageWindowState> {
    let prompt = create_control(
        parent,
        instance,
        w!("STATIC"),
        w!("Choose the language for ApricotPlayer."),
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let list = create_control(
        parent,
        instance,
        w!("LISTBOX"),
        w!("Language"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
        WS_EX_CLIENTEDGE,
        ID_LANGUAGE_LIST,
    )?;
    let mut selected = 0;
    for (index, language) in LANGUAGES.iter().enumerate() {
        if language.code == current_language {
            selected = index;
        }
        let name = wide(language.name);
        SendMessageW(
            list,
            LB_ADDSTRING,
            None,
            Some(LPARAM(name.as_ptr() as isize)),
        );
    }
    SendMessageW(list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    if !SetWindowSubclass(list, Some(list_proc), 1, 0).as_bool() {
        return Err(windows::core::Error::from_thread());
    }
    let ok = create_control(
        parent,
        instance,
        w!("BUTTON"),
        w!("OK"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        ID_OK,
    )?;
    let cancel = create_control(
        parent,
        instance,
        w!("BUTTON"),
        w!("Cancel"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_CANCEL,
    )?;
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in [prompt, list, ok, cancel] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    Ok(LanguageWindowState {
        prompt,
        list,
        ok,
        cancel,
        selected: None,
    })
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
        && let Ok(parent) = windows::Win32::UI::WindowsAndMessaging::GetParent(control)
    {
        if wparam.0 == usize::from(VK_RETURN.0) {
            accept(parent);
            return LRESULT(0);
        }
        if wparam.0 == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(parent);
            return LRESULT(0);
        }
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(control, Some(list_proc), subclass_id);
    }
    DefSubclassProc(control, message, wparam, lparam)
}

unsafe fn accept(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    let Ok(index) = usize::try_from(selected) else {
        return;
    };
    let Some(language) = LANGUAGES.get(index) else {
        return;
    };
    state.selected = Some(language.code);
    let _ = DestroyWindow(window);
}

unsafe fn layout(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let width = (bounds.right - bounds.left).max(360);
    let height = (bounds.bottom - bounds.top).max(320);
    let margin = 12;
    let prompt_height = 28;
    let button_height = 34;
    let button_width = 110;
    let _ = MoveWindow(
        state.prompt,
        margin,
        margin,
        width - margin * 2,
        prompt_height,
        true,
    );
    let list_top = margin + prompt_height;
    let _ = MoveWindow(
        state.list,
        margin,
        list_top,
        width - margin * 2,
        height - list_top - button_height - margin * 2,
        true,
    );
    let button_top = height - button_height - margin;
    let _ = MoveWindow(
        state.ok,
        width - margin - button_width * 2 - 8,
        button_top,
        button_width,
        button_height,
        true,
    );
    let _ = MoveWindow(
        state.cancel,
        width - margin - button_width,
        button_top,
        button_width,
        button_height,
        true,
    );
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
        100,
        30,
        Some(parent),
        Some(HMENU(id as *mut c_void)),
        Some(instance),
        None,
    )
}

unsafe fn state(window: HWND) -> Option<&'static LanguageWindowState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const LanguageWindowState;
    pointer.as_ref()
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut LanguageWindowState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut LanguageWindowState;
    pointer.as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
