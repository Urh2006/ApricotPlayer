//! The `AudioVault` login dialog, Python `show_audiovault_login`: Email,
//! Password, Register, then OK and Cancel.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus, VK_ESCAPE, VK_RETURN},
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, ES_AUTOHSCROLL, ES_PASSWORD, GetClientRect, GetMessageW,
                GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsChild,
                IsDialogMessageW, IsWindow, LoadCursorW, MSG, MoveWindow, PostQuitMessage,
                RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW,
                ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE,
                WM_CLOSE, WM_COMMAND, WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW,
                WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS: PCWSTR = w!("ApricotPlayer2BetaAudiovaultLoginWindow");
const ID_EMAIL: usize = 1901;
const ID_PASSWORD: usize = 1902;
const ID_REGISTER: usize = 1903;
const ID_OK: usize = 1904;
const ID_CANCEL: usize = 1905;
/// `IsDialogMessageW` turns Enter and Escape into these.
const IDOK_COMMAND: usize = 1;
const IDCANCEL_COMMAND: usize = 2;

/// The dialog texts, Python `t(...)` values.
pub struct LoginTexts<'a> {
    pub title: &'a str,
    pub email: &'a str,
    pub password: &'a str,
    pub register: &'a str,
    pub ok: &'a str,
    pub cancel: &'a str,
}

struct LoginState {
    previous_focus: HWND,
    email_label: HWND,
    email: HWND,
    password_label: HWND,
    password: HWND,
    register: HWND,
    ok: HWND,
    cancel: HWND,
    on_register: Box<dyn Fn()>,
    result: Option<(String, String)>,
}

pub fn register() -> Result<()> {
    // SAFETY: The class is registered once before the main message loop.
    unsafe {
        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size fits in i32"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hInstance: HINSTANCE(module.0),
            lpszClassName: CLASS,
            lpfnWndProc: Some(window_proc),
            ..Default::default()
        };
        if RegisterClassW(&raw const class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
    }
    Ok(())
}

/// Shows the modal dialog with `email` filled in. Returns the typed email
/// and password after OK, or `None` after Cancel.
///
/// # Errors
///
/// Returns a Win32 error when the window or a control cannot be created.
pub fn show(
    owner: HWND,
    texts: &LoginTexts<'_>,
    email: &str,
    on_register: Box<dyn Fn()>,
) -> Result<Option<(String, String)>> {
    // SAFETY: The nested modal loop owns its state and disables its owner
    // until the state allocation has been recovered.
    unsafe { show_win32(owner, texts, email, on_register) }
}

#[allow(clippy::too_many_lines)]
unsafe fn show_win32(
    owner: HWND,
    texts: &LoginTexts<'_>,
    email: &str,
    on_register: Box<dyn Fn()>,
) -> Result<Option<(String, String)>> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let title = wide(texts.title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS,
        PCWSTR(title.as_ptr()),
        WS_CAPTION | WS_SYSMENU,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        460,
        230,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let create = |class: PCWSTR, text: &str, style: WINDOW_STYLE, extended, id: usize| {
        let text = wide(text);
        CreateWindowExW(
            extended,
            class,
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            100,
            30,
            Some(window),
            Some(HMENU(id as *mut c_void)),
            Some(instance),
            None,
        )
    };
    let built = (|| -> Result<LoginState> {
        let email_label = create(
            w!("STATIC"),
            texts.email,
            WINDOW_STYLE(0),
            WINDOW_EX_STYLE::default(),
            0,
        )?;
        let email_field = create(
            w!("EDIT"),
            email,
            WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            WS_EX_CLIENTEDGE,
            ID_EMAIL,
        )?;
        let password_label = create(
            w!("STATIC"),
            texts.password,
            WINDOW_STYLE(0),
            WINDOW_EX_STYLE::default(),
            0,
        )?;
        let password = create(
            w!("EDIT"),
            "",
            WS_TABSTOP | WINDOW_STYLE((ES_AUTOHSCROLL | ES_PASSWORD) as u32),
            WS_EX_CLIENTEDGE,
            ID_PASSWORD,
        )?;
        let register = create(
            w!("BUTTON"),
            texts.register,
            WS_TABSTOP,
            WINDOW_EX_STYLE::default(),
            ID_REGISTER,
        )?;
        let ok = create(
            w!("BUTTON"),
            texts.ok,
            WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            WINDOW_EX_STYLE::default(),
            ID_OK,
        )?;
        let cancel = create(
            w!("BUTTON"),
            texts.cancel,
            WS_TABSTOP,
            WINDOW_EX_STYLE::default(),
            ID_CANCEL,
        )?;
        Ok(LoginState {
            previous_focus: GetFocus(),
            email_label,
            email: email_field,
            password_label,
            password,
            register,
            ok,
            cancel,
            on_register,
            result: None,
        })
    })();
    let state = match built {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in [
        state.email_label,
        state.email,
        state.password_label,
        state.password,
        state.register,
        state.ok,
        state.cancel,
    ] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    crate::accessibility_win32::annotate_control_name(state.email, texts.email);
    crate::accessibility_win32::annotate_control_name(state.password, texts.password);
    let initial_focus = state.email;
    let pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), pointer as isize);
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
            PostQuitMessage(i32::try_from(message.wParam.0).unwrap_or_default());
            break;
        }
        let inside = message.hwnd == window || IsChild(window, message.hwnd).as_bool();
        if inside && message.message == WM_KEYDOWN && message.wParam.0 == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(window);
            continue;
        }
        // wx: Enter in either field ends the dialog with OK.
        if inside
            && message.message == WM_KEYDOWN
            && message.wParam.0 == usize::from(VK_RETURN.0)
            && ((*pointer).email == message.hwnd || (*pointer).password == message.hwnd)
        {
            accept(window);
            continue;
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    let state = Box::from_raw(pointer);
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    if !state.previous_focus.is_invalid() && IsWindow(Some(state.previous_focus)).as_bool() {
        let _ = SetFocus(Some(state.previous_focus));
    }
    if let Some(error) = loop_error {
        return Err(error);
    }
    Ok(state.result)
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
            if command == ID_OK || command == IDOK_COMMAND {
                accept(window);
            } else if command == ID_CANCEL || command == IDCANCEL_COMMAND {
                let _ = DestroyWindow(window);
            } else if command == ID_REGISTER
                && let Some(state) = state(window)
            {
                (state.on_register)();
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

unsafe fn accept(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    state.result = Some((window_text(state.email), window_text(state.password)));
    let _ = DestroyWindow(window);
}

unsafe fn window_text(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    let mut value = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let copied = GetWindowTextW(control, &mut value);
    String::from_utf16_lossy(&value[..usize::try_from(copied).unwrap_or_default()])
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
    let height = (bounds.bottom - bounds.top).max(180);
    let margin = 10;
    let label_width = 90;
    let field_height = 26;
    let button_height = 30;
    let field_x = margin + label_width + 6;
    let field_width = width - field_x - margin;
    for (row, (label, field)) in [
        (state.email_label, state.email),
        (state.password_label, state.password),
    ]
    .into_iter()
    .enumerate()
    {
        let y = margin + i32::try_from(row).unwrap_or_default() * (field_height + 6);
        let _ = MoveWindow(label, margin, y + 4, label_width, field_height, true);
        let _ = MoveWindow(field, field_x, y, field_width, field_height, true);
    }
    let register_y = margin * 2 + (field_height + 6) * 2;
    let _ = MoveWindow(state.register, margin, register_y, 120, button_height, true);
    let button_y = height - button_height - margin;
    let _ = MoveWindow(
        state.ok,
        width - margin - 200 - 6,
        button_y,
        100,
        button_height,
        true,
    );
    let _ = MoveWindow(
        state.cancel,
        width - margin - 100,
        button_y,
        100,
        button_height,
        true,
    );
}

unsafe fn state(window: HWND) -> Option<&'static mut LoginState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut LoginState;
    pointer.as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
