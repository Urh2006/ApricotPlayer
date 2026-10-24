//! Independent, modeless download-progress window.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{PBM_SETPOS, PBM_SETRANGE32},
            Input::KeyboardAndMouse::SetFocus,
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                GetClientRect, GetWindowLongPtrW, IDC_ARROW, IsDialogMessageW, IsWindow,
                LoadCursorW, MSG, MoveWindow, PostMessageW, RegisterClassW, SW_HIDE, SW_SHOW,
                SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
                WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
                WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_MINIMIZEBOX,
                WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaDownloadProgressWindow");
const ID_MESSAGE: usize = 1701;
const ID_PROGRESS: usize = 1702;
const ID_HIDE: usize = 1703;
const ID_DETAILS: usize = 1704;

struct ProgressState {
    owner: HWND,
    details_message: u32,
    message: HWND,
    progress: HWND,
    hide: HWND,
    details: HWND,
}

#[derive(Clone, Copy)]
pub struct DownloadProgressWindow {
    window: HWND,
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

impl DownloadProgressWindow {
    pub unsafe fn create(
        owner: HWND,
        title: &str,
        message: &str,
        hide_label: &str,
        details_label: &str,
        details_message: u32,
    ) -> Result<Self> {
        let module = GetModuleHandleW(None)?;
        let instance = HINSTANCE(module.0);
        let title = wide(title);
        let window = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPED | WS_CAPTION | WS_MINIMIZEBOX | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            560,
            190,
            Some(owner),
            None,
            Some(instance),
            None,
        )?;
        let controls = create_controls(window, instance, message, hide_label, details_label);
        let (message, progress, hide, details) = match controls {
            Ok(controls) => controls,
            Err(error) => {
                let _ = DestroyWindow(window);
                return Err(error);
            }
        };
        let state = Box::new(ProgressState {
            owner,
            details_message,
            message,
            progress,
            hide,
            details,
        });
        SetWindowLongPtrW(
            window,
            WINDOW_LONG_PTR_INDEX(0),
            Box::into_raw(state) as isize,
        );
        SendMessageW(progress, PBM_SETRANGE32, Some(WPARAM(0)), Some(LPARAM(100)));
        layout(window);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(hide));
        Ok(Self { window })
    }

    pub unsafe fn is_open(self) -> bool {
        IsWindow(Some(self.window)).as_bool()
    }

    pub unsafe fn set_progress(self, percent: u32, message: &str) {
        let Some(state) = state(self.window) else {
            return;
        };
        let message = wide(message);
        let _ = SetWindowTextW(state.message, PCWSTR(message.as_ptr()));
        SendMessageW(
            state.progress,
            PBM_SETPOS,
            Some(WPARAM(percent.min(100) as usize)),
            None,
        );
    }

    pub unsafe fn show(self) {
        if self.is_open() {
            let _ = ShowWindow(self.window, SW_SHOW);
            let _ = SetForegroundWindow(self.window);
            if let Some(state) = state(self.window) {
                let _ = SetFocus(Some(state.hide));
            }
        }
    }

    pub unsafe fn handles_dialog_message(self, message: &MSG) -> bool {
        self.is_open() && IsDialogMessageW(self.window, message).as_bool()
    }

    pub unsafe fn destroy(self) {
        if self.is_open() {
            let _ = DestroyWindow(self.window);
        }
    }
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
            if command == ID_HIDE {
                let _ = ShowWindow(window, SW_HIDE);
            } else if command == ID_DETAILS
                && let Some(state) = state(window)
            {
                let _ = ShowWindow(window, SW_HIDE);
                let _ = PostMessageW(
                    Some(state.owner),
                    state.details_message,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = ShowWindow(window, SW_HIDE);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut ProgressState;
            SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            if !pointer.is_null() {
                drop(Box::from_raw(pointer));
            }
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn create_controls(
    parent: HWND,
    instance: HINSTANCE,
    message_text: &str,
    hide_label: &str,
    details_label: &str,
) -> Result<(HWND, HWND, HWND, HWND)> {
    let message = create_control(
        parent,
        instance,
        w!("STATIC"),
        message_text,
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        ID_MESSAGE,
    )?;
    let progress = create_control(
        parent,
        instance,
        w!("msctls_progress32"),
        "",
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        ID_PROGRESS,
    )?;
    let hide = create_control(
        parent,
        instance,
        w!("BUTTON"),
        hide_label,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        ID_HIDE,
    )?;
    let details = create_control(
        parent,
        instance,
        w!("BUTTON"),
        details_label,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_DETAILS,
    )?;
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in [message, progress, hide, details] {
        SendMessageW(control, WM_SETFONT, Some(WPARAM(font.0 as usize)), None);
    }
    Ok((message, progress, hide, details))
}

unsafe fn create_control(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    name: &str,
    style: WINDOW_STYLE,
    extended_style: WINDOW_EX_STYLE,
    id: usize,
) -> Result<HWND> {
    let name = wide(name);
    CreateWindowExW(
        extended_style,
        class,
        PCWSTR(name.as_ptr()),
        style,
        0,
        0,
        0,
        0,
        Some(parent),
        Some(windows::Win32::UI::WindowsAndMessaging::HMENU(
            id as *mut c_void,
        )),
        Some(instance),
        None,
    )
}

unsafe fn layout(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let mut client = RECT::default();
    if GetClientRect(window, &raw mut client).is_err() {
        return;
    }
    let width = (client.right - client.left).max(360);
    let height = (client.bottom - client.top).max(150);
    let margin = 12;
    let button_width = 120;
    let button_height = 28;
    let _ = MoveWindow(state.message, margin, margin, width - 2 * margin, 64, true);
    let _ = MoveWindow(state.progress, margin, 82, width - 2 * margin, 22, true);
    let button_y = height - margin - button_height;
    let _ = MoveWindow(
        state.hide,
        width - margin - 2 * button_width - 8,
        button_y,
        button_width,
        button_height,
        true,
    );
    let _ = MoveWindow(
        state.details,
        width - margin - button_width,
        button_y,
        button_width,
        button_height,
        true,
    );
}

unsafe fn state(window: HWND) -> Option<&'static ProgressState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const ProgressState;
    pointer.as_ref()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
