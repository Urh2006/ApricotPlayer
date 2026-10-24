//! Isolated Win32 boundary for the accessibility qualification executable.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Accessibility::NotifyWinEvent,
            Controls::{
                InitCommonControls, PBM_SETPOS, PBM_SETRANGE32, PROGRESS_CLASSW, TBM_SETPOS,
                TBM_SETRANGE, TBS_AUTOTICKS, TRACKBAR_CLASSW,
            },
            Input::KeyboardAndMouse::{GetFocus, GetKeyState, SetFocus, VK_SHIFT, VK_TAB},
            Shell::{
                DefSubclassProc, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
                NOTIFYICONDATAW, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, BS_AUTOCHECKBOX, CBS_DROPDOWNLIST, CHILDID_SELF, CREATESTRUCTW,
                CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
                DispatchMessageW, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY,
                EVENT_OBJECT_NAMECHANGE, GetCursorPos, GetMessageW, GetNextDlgTabItem, GetParent,
                GetWindowLongPtrW, GetWindowRect, HMENU, IDC_ARROW, IDI_APPLICATION,
                IsDialogMessageW, LB_ADDSTRING, LB_SETCURSEL, LBS_NOTIFY, LoadCursorW, LoadIconW,
                MB_OK, MF_STRING, MSG, MessageBoxW, OBJID_CLIENT, PostQuitMessage, RegisterClassW,
                SW_SHOW, SendMessageW, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
                TPM_LEFTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_COMMAND, WM_CONTEXTMENU, WM_CREATE,
                WM_DESTROY, WM_GETFONT, WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW,
                WS_BORDER, WS_CHILD, WS_EX_CLIENTEDGE, WS_GROUP, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
                WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const ID_LIST: usize = 1001;
const ID_CHECKBOX: usize = 1002;
const ID_COMBO: usize = 1003;
const ID_SLIDER: usize = 1004;
const ID_READ_ONLY: usize = 1005;
const ID_MODAL: usize = 1006;
const ID_ANNOUNCE: usize = 1007;
const ID_PROGRESS: usize = 1008;
const ID_VIDEO_HOST: usize = 1009;
const ID_STATUS: usize = 1010;
const ID_CONTEXT_ACTION: usize = 1101;
const WM_TRAY: u32 = 0x8001;

#[derive(Clone, Copy)]
struct SpikeControls {
    list: HWND,
    checkbox: HWND,
    combo: HWND,
    slider: HWND,
    read_only: HWND,
    modal: HWND,
    announce: HWND,
    progress: HWND,
    video_host: HWND,
    status: HWND,
}

pub fn run() -> Result<()> {
    // SAFETY: The Win32 message loop and every HWND it references live on this
    // thread. All pointers passed to Win32 are either null or point to values
    // whose lifetime covers the call.
    unsafe { run_win32() }
}

unsafe fn run_win32() -> Result<()> {
    InitCommonControls();
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let class_name = w!("ApricotAccessibilitySpike");
    let window_extra_bytes =
        i32::try_from(size_of::<isize>()).expect("pointer size fits in an i32");
    let class = WNDCLASSW {
        cbWndExtra: window_extra_bytes,
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hInstance: instance,
        lpszClassName: class_name,
        lpfnWndProc: Some(window_proc),
        ..Default::default()
    };
    if RegisterClassW(&raw const class) == 0 {
        return Err(windows::core::Error::from_thread());
    }

    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class_name,
        w!("ApricotPlayer Rust accessibility qualification"),
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        760,
        650,
        None,
        None,
        Some(instance),
        None,
    )?;
    let _ = ShowWindow(window, SW_SHOW);

    let mut message = MSG::default();
    while GetMessageW(&raw mut message, None, 0, 0).into() {
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            let instance = HINSTANCE(create.hInstance.0);
            match create_controls(window, instance) {
                Ok(controls) => {
                    let controls = Box::new(controls);
                    SetWindowLongPtrW(
                        window,
                        WINDOW_LONG_PTR_INDEX(0),
                        Box::into_raw(controls) as isize,
                    );
                    layout_controls(window);
                    add_tray_icon(window);
                    LRESULT(0)
                }
                Err(_) => LRESULT(-1),
            }
        }
        WM_SIZE => {
            layout_controls(window);
            LRESULT(0)
        }
        WM_COMMAND => {
            handle_command(window, wparam);
            LRESULT(0)
        }
        WM_CONTEXTMENU => {
            show_context_menu(window, lparam);
            LRESULT(0)
        }
        WM_DESTROY => {
            remove_tray_icon(window);
            let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut SpikeControls;
            if !pointer.is_null() {
                drop(Box::from_raw(pointer));
                SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

#[allow(clippy::too_many_lines)]
unsafe fn create_controls(window: HWND, instance: HINSTANCE) -> Result<SpikeControls> {
    let list = create_control(
        window,
        instance,
        w!("LISTBOX"),
        w!("Media results"),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_GROUP
            | WS_BORDER
            | WS_VSCROLL
            | WINDOW_STYLE(LBS_NOTIFY as u32),
        WS_EX_CLIENTEDGE,
        ID_LIST,
    )?;
    for item in [
        w!("First accessible result"),
        w!("Second accessible result"),
        w!("Third accessible result"),
    ] {
        SendMessageW(
            list,
            LB_ADDSTRING,
            None,
            Some(LPARAM(item.as_ptr() as isize)),
        );
    }
    SendMessageW(list, LB_SETCURSEL, Some(WPARAM(0)), None);

    let checkbox = create_control(
        window,
        instance,
        w!("BUTTON"),
        w!("Autoplay next item"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
        WINDOW_EX_STYLE::default(),
        ID_CHECKBOX,
    )?;
    let combo = create_control(
        window,
        instance,
        w!("COMBOBOX"),
        w!("Result type"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
        WINDOW_EX_STYLE::default(),
        ID_COMBO,
    )?;
    for item in [w!("Videos"), w!("Playlists"), w!("Channels")] {
        SendMessageW(combo, 0x0143, None, Some(LPARAM(item.as_ptr() as isize)));
    }
    SendMessageW(combo, 0x014E, Some(WPARAM(0)), None);

    let slider = create_control(
        window,
        instance,
        TRACKBAR_CLASSW,
        w!("Volume, 50 percent"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(TBS_AUTOTICKS),
        WINDOW_EX_STYLE::default(),
        ID_SLIDER,
    )?;
    SendMessageW(
        slider,
        TBM_SETRANGE,
        Some(WPARAM(1)),
        Some(LPARAM(100_i32.wrapping_shl(16) as isize)),
    );
    SendMessageW(slider, TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(50)));

    let read_only = create_control(
        window,
        instance,
        w!("EDIT"),
        w!(
            "Read-only lyrics and transcript fields use this native control.\r\nTab continues to the next control."
        ),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_VSCROLL
            | WINDOW_STYLE((ES_READONLY | ES_MULTILINE | ES_AUTOVSCROLL) as u32),
        WS_EX_CLIENTEDGE,
        ID_READ_ONLY,
    )?;
    if !SetWindowSubclass(read_only, Some(read_only_proc), 1, 0).as_bool() {
        return Err(windows::core::Error::from_thread());
    }
    let modal = create_control(
        window,
        instance,
        w!("BUTTON"),
        w!("Open modal dialog"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_MODAL,
    )?;
    let announce = create_control(
        window,
        instance,
        w!("BUTTON"),
        w!("Announce playback status"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_ANNOUNCE,
    )?;
    let progress = create_control(
        window,
        instance,
        PROGRESS_CLASSW,
        w!("Download progress, 35 percent"),
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        ID_PROGRESS,
    )?;
    SendMessageW(progress, PBM_SETRANGE32, Some(WPARAM(0)), Some(LPARAM(100)));
    SendMessageW(progress, PBM_SETPOS, Some(WPARAM(35)), Some(LPARAM(0)));
    let video_host = create_control(
        window,
        instance,
        w!("STATIC"),
        w!("Embedded video output host"),
        WS_CHILD | WS_VISIBLE | WS_BORDER,
        WS_EX_CLIENTEDGE,
        ID_VIDEO_HOST,
    )?;
    let status = create_control(
        window,
        instance,
        w!("STATIC"),
        w!("Playback status ready"),
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        ID_STATUS,
    )?;

    let controls = SpikeControls {
        list,
        checkbox,
        combo,
        slider,
        read_only,
        modal,
        announce,
        progress,
        video_host,
        status,
    };
    apply_default_font(window, &controls);
    layout_controls(window);
    let _ = SetFocus(Some(list));
    Ok(controls)
}

unsafe extern "system" fn read_only_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    if message == WM_KEYDOWN && wparam.0 == usize::from(VK_TAB.0) {
        let previous = GetKeyState(i32::from(VK_SHIFT.0)).is_negative();
        if let Ok(parent) = GetParent(window)
            && let Ok(next) = GetNextDlgTabItem(parent, Some(window), previous)
        {
            let _ = SetFocus(Some(next));
            return LRESULT(0);
        }
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(read_only_proc), subclass_id);
    }
    DefSubclassProc(window, message, wparam, lparam)
}

unsafe fn create_control(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    name: PCWSTR,
    style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE,
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

unsafe fn controls(window: HWND) -> Option<&'static SpikeControls> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const SpikeControls;
    pointer.as_ref()
}

unsafe fn layout_controls(window: HWND) {
    let Some(controls) = controls(window) else {
        return;
    };
    let mut bounds = RECT::default();
    let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(window, &raw mut bounds);
    let width = (bounds.right - bounds.left).max(400);
    let margin = 16;
    let content_width = width - margin * 2;
    let placements = [
        (controls.list, margin, 16, content_width, 120),
        (controls.checkbox, margin, 148, content_width, 28),
        (controls.combo, margin, 184, content_width, 200),
        (controls.slider, margin, 224, content_width, 36),
        (controls.read_only, margin, 270, content_width, 90),
        (controls.modal, margin, 372, 190, 32),
        (controls.announce, 216, 372, 230, 32),
        (controls.progress, margin, 416, content_width, 24),
        (controls.video_host, margin, 452, content_width, 92),
        (controls.status, margin, 556, content_width, 28),
    ];
    for (control, x, y, control_width, height) in placements {
        let _ = windows::Win32::UI::WindowsAndMessaging::MoveWindow(
            control,
            x,
            y,
            control_width,
            height,
            true,
        );
    }
}

unsafe fn apply_default_font(window: HWND, controls: &SpikeControls) {
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let font_param = Some(WPARAM(font.0 as usize));
    for control in [
        controls.list,
        controls.checkbox,
        controls.combo,
        controls.slider,
        controls.read_only,
        controls.modal,
        controls.announce,
        controls.progress,
        controls.video_host,
        controls.status,
    ] {
        SendMessageW(control, WM_SETFONT, font_param, Some(LPARAM(1)));
    }
    SendMessageW(window, WM_GETFONT, None, None);
}

unsafe fn handle_command(window: HWND, wparam: WPARAM) {
    let command = wparam.0 & 0xffff;
    match command {
        ID_MODAL => {
            let previous_focus = GetFocus();
            let _ = MessageBoxW(
                Some(window),
                w!("This native modal must announce once and return focus."),
                w!("Accessibility qualification"),
                MB_OK,
            );
            if !previous_focus.0.is_null() {
                let _ = SetFocus(Some(previous_focus));
            }
        }
        ID_ANNOUNCE | ID_CONTEXT_ACTION => {
            if let Some(controls) = controls(window) {
                let _ = SetWindowTextW(
                    controls.status,
                    w!("Playback paused at 1 minute 23 seconds"),
                );
                NotifyWinEvent(
                    EVENT_OBJECT_NAMECHANGE,
                    controls.status,
                    OBJID_CLIENT.0,
                    CHILDID_SELF.cast_signed(),
                );
            }
        }
        _ => {}
    }
}

unsafe fn show_context_menu(window: HWND, lparam: LPARAM) {
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        ID_CONTEXT_ACTION,
        w!("Announce current status"),
    );
    let packed = lparam.0.to_le_bytes();
    let mut point = POINT {
        x: i32::from(i16::from_le_bytes([packed[0], packed[1]])),
        y: i32::from(i16::from_le_bytes([packed[2], packed[3]])),
    };
    if point.x == -1 && point.y == -1 {
        let focus = GetFocus();
        if focus.0.is_null() {
            let _ = GetCursorPos(&raw mut point);
        } else {
            let mut bounds = RECT::default();
            if GetWindowRect(focus, &raw mut bounds).is_ok() {
                point.x = bounds.left;
                point.y = bounds.bottom;
            }
        }
    }
    let _ = TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_RIGHTBUTTON,
        point.x,
        point.y,
        None,
        window,
        None,
    );
    let _ = DestroyMenu(menu);
}

unsafe fn tray_data(window: HWND) -> NOTIFYICONDATAW {
    let structure_size =
        u32::try_from(size_of::<NOTIFYICONDATAW>()).expect("notification data size fits in a u32");
    let mut data = NOTIFYICONDATAW {
        cbSize: structure_size,
        hWnd: window,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
        ..Default::default()
    };
    let tooltip: Vec<u16> = "ApricotPlayer Rust accessibility spike\0"
        .encode_utf16()
        .collect();
    for (target, source) in data.szTip.iter_mut().zip(tooltip) {
        *target = source;
    }
    data
}

unsafe fn add_tray_icon(window: HWND) {
    let data = tray_data(window);
    let _ = Shell_NotifyIconW(NIM_ADD, &raw const data);
}

unsafe fn remove_tray_icon(window: HWND) {
    let data = tray_data(window);
    let _ = Shell_NotifyIconW(NIM_DELETE, &raw const data);
}
