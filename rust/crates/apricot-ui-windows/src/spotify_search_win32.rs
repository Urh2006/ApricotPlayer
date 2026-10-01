//! Spotify search dialog (`docs/SPOTIFY_PLAN.md` 4.2 Search): the query, the
//! type (All and each searchable type), Search and Cancel. Enter in the
//! query or the type searches; Escape cancels. The results open as a list in
//! the main window.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, SetFocus, VK_ESCAPE, VK_RETURN},
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CBS_DROPDOWNLIST, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
                DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, GetClientRect, GetMessageW,
                GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsChild,
                IsDialogMessageW, IsWindow, LoadCursorW, MSG, MoveWindow, PostQuitMessage,
                RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW,
                ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE,
                WM_CLOSE, WM_COMMAND, WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW,
                WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
                WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS: PCWSTR = w!("ApricotPlayer2BetaSpotifySearchWindow");
const ID_QUERY: usize = 2101;
const ID_KIND: usize = 2102;
const ID_SEARCH: usize = 2103;
const ID_CANCEL: usize = 2104;
const IDOK_COMMAND: usize = 1;
const IDCANCEL_COMMAND: usize = 2;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const CB_GETDROPPEDSTATE: u32 = 0x0157;

pub struct SearchTexts<'a> {
    pub title: &'a str,
    pub query: &'a str,
    pub kind: &'a str,
    pub search: &'a str,
    pub cancel: &'a str,
}

struct SearchState {
    query_label: HWND,
    query: HWND,
    kind_label: HWND,
    kind: HWND,
    search: HWND,
    cancel: HWND,
    result: Option<(String, usize)>,
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

/// Shows the dialog with the last query and type. Returns the query (not
/// empty) and the chosen type index, or `None` after Cancel.
///
/// # Errors
///
/// Returns a Win32 error when the window or a control cannot be created.
pub fn show(
    owner: HWND,
    texts: &SearchTexts<'_>,
    query: &str,
    kinds: &[String],
    kind: usize,
) -> Result<Option<(String, usize)>> {
    // SAFETY: The nested modal loop owns its state and disables its owner
    // until the state allocation has been recovered.
    unsafe { show_win32(owner, texts, query, kinds, kind) }
}

#[allow(clippy::too_many_lines)]
unsafe fn show_win32(
    owner: HWND,
    texts: &SearchTexts<'_>,
    query: &str,
    kinds: &[String],
    kind: usize,
) -> Result<Option<(String, usize)>> {
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
        200,
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
    let built = (|| -> Result<SearchState> {
        let query_label = create(
            w!("STATIC"),
            texts.query,
            WINDOW_STYLE(0),
            WINDOW_EX_STYLE::default(),
            0,
        )?;
        let query_field = create(
            w!("EDIT"),
            query,
            WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            WS_EX_CLIENTEDGE,
            ID_QUERY,
        )?;
        let kind_label = create(
            w!("STATIC"),
            texts.kind,
            WINDOW_STYLE(0),
            WINDOW_EX_STYLE::default(),
            0,
        )?;
        let kind_box = create(
            w!("COMBOBOX"),
            "",
            WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            WINDOW_EX_STYLE::default(),
            ID_KIND,
        )?;
        let search = create(
            w!("BUTTON"),
            texts.search,
            WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            WINDOW_EX_STYLE::default(),
            ID_SEARCH,
        )?;
        let cancel = create(
            w!("BUTTON"),
            texts.cancel,
            WS_TABSTOP,
            WINDOW_EX_STYLE::default(),
            ID_CANCEL,
        )?;
        Ok(SearchState {
            query_label,
            query: query_field,
            kind_label,
            kind: kind_box,
            search,
            cancel,
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
        state.query_label,
        state.query,
        state.kind_label,
        state.kind,
        state.search,
        state.cancel,
    ] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    for label in kinds {
        let label = wide(label);
        SendMessageW(
            state.kind,
            CB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    }
    SendMessageW(
        state.kind,
        CB_SETCURSEL,
        Some(WPARAM(kind.min(kinds.len().saturating_sub(1)))),
        None,
    );
    crate::accessibility_win32::annotate_control_name(state.query, texts.query);
    crate::accessibility_win32::annotate_control_name(state.kind, texts.kind);
    let initial_focus = state.query;
    let pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), pointer as isize);
    layout(window);
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetFocus(Some(initial_focus));
    // The whole query is selected, so typing replaces the last search.
    SendMessageW(initial_focus, 0x00B1, Some(WPARAM(0)), Some(LPARAM(-1)));
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
        let dropped = SendMessageW((*pointer).kind, CB_GETDROPPEDSTATE, None, None).0 != 0;
        if inside
            && !dropped
            && message.message == WM_KEYDOWN
            && message.wParam.0 == usize::from(VK_ESCAPE.0)
        {
            let _ = DestroyWindow(window);
            continue;
        }
        if inside
            && !dropped
            && message.message == WM_KEYDOWN
            && message.wParam.0 == usize::from(VK_RETURN.0)
            && ((*pointer).query == message.hwnd
                || (*pointer).kind == message.hwnd
                || IsChild((*pointer).kind, message.hwnd).as_bool())
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
            if command == ID_SEARCH || command == IDOK_COMMAND {
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

/// An empty query keeps the dialog open with the focus in the field.
unsafe fn accept(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let query = window_text(state.query).trim().to_owned();
    if query.is_empty() {
        let _ = SetFocus(Some(state.query));
        return;
    }
    let kind = usize::try_from(SendMessageW(state.kind, CB_GETCURSEL, None, None).0).unwrap_or(0);
    state.result = Some((query, kind));
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
    let height = (bounds.bottom - bounds.top).max(150);
    let margin = 10;
    let label_width = 90;
    let field_height = 26;
    let button_height = 30;
    let field_x = margin + label_width + 6;
    let field_width = width - field_x - margin;
    let _ = MoveWindow(
        state.query_label,
        margin,
        margin + 4,
        label_width,
        field_height,
        true,
    );
    let _ = MoveWindow(
        state.query,
        field_x,
        margin,
        field_width,
        field_height,
        true,
    );
    let kind_y = margin + field_height + 8;
    let _ = MoveWindow(
        state.kind_label,
        margin,
        kind_y + 4,
        label_width,
        field_height,
        true,
    );
    let _ = MoveWindow(state.kind, field_x, kind_y, field_width, 240, true);
    let button_y = height - button_height - margin;
    let _ = MoveWindow(
        state.search,
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

unsafe fn state(window: HWND) -> Option<&'static mut SearchState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut SearchState;
    pointer.as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
