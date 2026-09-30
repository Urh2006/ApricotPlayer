//! The Spotify login dialog (`docs/SPOTIFY_PLAN.md` 9.1): a read-only status
//! field with the first focus, then Open browser again, Copy login link,
//! Try again and Cancel. The login itself runs in `apricot-spotify`; the main
//! window forwards its progress here with [`update`].

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus, VK_ESCAPE},
            WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, GetClientRect, GetMessageW,
                GetWindowLongPtrW, HMENU, IDC_ARROW, IsChild, IsDialogMessageW, IsWindow,
                LoadCursorW, MSG, MoveWindow, PostQuitMessage, RegisterClassW, SW_SHOW,
                SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
                TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE,
                WM_COMMAND, WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CAPTION,
                WS_CHILD, WS_EX_CLIENTEDGE, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS: PCWSTR = w!("ApricotPlayerSpotifyLoginWindow");
const ID_STATUS: usize = 1951;
const ID_OPEN_BROWSER: usize = 1952;
const ID_COPY_LINK: usize = 1953;
const ID_RETRY: usize = 1954;
const ID_CANCEL: usize = 1955;
/// `IsDialogMessageW` turns Escape into this.
const IDCANCEL_COMMAND: usize = 2;

pub struct LoginTexts<'a> {
    pub title: &'a str,
    pub status_name: &'a str,
    pub waiting: &'a str,
    pub open_browser: &'a str,
    pub copy_link: &'a str,
    pub retry: &'a str,
    pub cancel: &'a str,
}

/// Callbacks into the Spotify controller of the main window.
pub struct LoginActions {
    pub open_browser: Box<dyn Fn()>,
    pub copy_link: Box<dyn Fn()>,
    pub retry: Box<dyn Fn()>,
    pub cancel: Box<dyn Fn()>,
}

/// Progress forwarded by the main window.
pub enum LoginUpdate {
    /// The login failed: the text replaces the status and Try again is
    /// enabled and focused. The controller announces the text once.
    Failed(String),
    /// A new attempt started.
    Waiting,
    /// Logged in; the dialog closes.
    Succeeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoginOutcome {
    Succeeded,
    Closed,
}

struct LoginState {
    previous_focus: HWND,
    status: HWND,
    open_browser: HWND,
    copy_link: HWND,
    retry: HWND,
    cancel: HWND,
    waiting: String,
    actions: LoginActions,
    outcome: LoginOutcome,
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

/// Shows the modal dialog. `on_open` receives the dialog window right after
/// it exists, so the controller can forward [`LoginUpdate`]s to it.
///
/// # Errors
///
/// Returns a Win32 error when the window or a control cannot be created.
pub fn show(
    owner: HWND,
    texts: &LoginTexts<'_>,
    actions: LoginActions,
    on_open: impl FnOnce(HWND),
) -> Result<LoginOutcome> {
    // SAFETY: The nested modal loop owns its state and disables its owner
    // until the state allocation has been recovered.
    unsafe { show_win32(owner, texts, actions, on_open) }
}

#[allow(clippy::too_many_lines)]
unsafe fn show_win32(
    owner: HWND,
    texts: &LoginTexts<'_>,
    actions: LoginActions,
    on_open: impl FnOnce(HWND),
) -> Result<LoginOutcome> {
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
        520,
        240,
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
        let status = create(
            w!("EDIT"),
            texts.waiting,
            WS_TABSTOP | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
            WS_EX_CLIENTEDGE,
            ID_STATUS,
        )?;
        let button = |text: &str, id: usize| {
            create(
                w!("BUTTON"),
                text,
                WS_TABSTOP,
                WINDOW_EX_STYLE::default(),
                id,
            )
        };
        Ok(LoginState {
            previous_focus: GetFocus(),
            status,
            open_browser: button(texts.open_browser, ID_OPEN_BROWSER)?,
            copy_link: button(texts.copy_link, ID_COPY_LINK)?,
            retry: button(texts.retry, ID_RETRY)?,
            cancel: button(texts.cancel, ID_CANCEL)?,
            waiting: texts.waiting.to_owned(),
            actions,
            outcome: LoginOutcome::Closed,
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
        state.status,
        state.open_browser,
        state.copy_link,
        state.retry,
        state.cancel,
    ] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    crate::accessibility_win32::annotate_control_name(state.status, texts.status_name);
    // Try again only after a failure.
    let _ = EnableWindow(state.retry, false);
    let initial_focus = state.status;
    let pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), pointer as isize);
    layout(window);
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetFocus(Some(initial_focus));
    on_open(window);
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
            cancel(window);
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
    Ok(state.outcome)
}

/// Forwards login progress to an open dialog.
///
/// # Safety
///
/// `window` must be a dialog created by [`show`] or an already destroyed
/// handle (then nothing happens).
pub unsafe fn update(window: HWND, update: LoginUpdate) {
    if !IsWindow(Some(window)).as_bool() {
        return;
    }
    let Some(state) = state(window) else {
        return;
    };
    match update {
        LoginUpdate::Succeeded => {
            state.outcome = LoginOutcome::Succeeded;
            let _ = DestroyWindow(window);
        }
        LoginUpdate::Waiting => {
            let text = wide(&state.waiting);
            let _ = SetWindowTextW(state.status, PCWSTR(text.as_ptr()));
            let _ = EnableWindow(state.retry, false);
            let _ = SetFocus(Some(state.status));
        }
        LoginUpdate::Failed(message) => {
            let text = wide(&message);
            let _ = SetWindowTextW(state.status, PCWSTR(text.as_ptr()));
            let _ = EnableWindow(state.retry, true);
            let _ = SetFocus(Some(state.retry));
        }
    }
}

unsafe fn cancel(window: HWND) {
    if let Some(state) = state(window) {
        (state.actions.cancel)();
    }
    let _ = DestroyWindow(window);
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
            match command {
                ID_CANCEL | IDCANCEL_COMMAND => cancel(window),
                ID_OPEN_BROWSER => {
                    if let Some(state) = state(window) {
                        (state.actions.open_browser)();
                    }
                }
                ID_COPY_LINK => {
                    if let Some(state) = state(window) {
                        (state.actions.copy_link)();
                    }
                }
                ID_RETRY => {
                    if let Some(state) = state(window) {
                        (state.actions.retry)();
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            cancel(window);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
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
    let height = (bounds.bottom - bounds.top).max(180);
    let margin = 10;
    let button_height = 30;
    let button_y = height - button_height - margin;
    let _ = MoveWindow(
        state.status,
        margin,
        margin,
        width - 2 * margin,
        button_y - 2 * margin,
        true,
    );
    let buttons = [
        state.open_browser,
        state.copy_link,
        state.retry,
        state.cancel,
    ];
    let count = i32::try_from(buttons.len()).unwrap_or(4);
    let button_width = (width - 2 * margin - (count - 1) * 6) / count;
    for (index, button) in buttons.into_iter().enumerate() {
        let x = margin + i32::try_from(index).unwrap_or_default() * (button_width + 6);
        let _ = MoveWindow(button, x, button_y, button_width, button_height, true);
    }
}

unsafe fn state(window: HWND) -> Option<&'static mut LoginState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut LoginState;
    pointer.as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
