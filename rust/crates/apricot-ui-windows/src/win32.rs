//! Isolated unsafe Win32 boundary for the production Windows shell.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use apricot_app::{ActivationRequest, Application, MainMenuModel};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{SetFocus, VK_RETURN},
            Shell::{
                DefSubclassProc, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
                NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, RemoveWindowSubclass,
                SetWindowSubclass, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetClientRect,
                GetCursorPos, GetMessageW, GetWindowLongPtrW, HMENU, IDC_ARROW, IDI_APPLICATION,
                IsDialogMessageW, LB_ADDSTRING, LB_GETCURSEL, LB_SETCURSEL, LBN_DBLCLK, LBS_NOTIFY,
                LoadCursorW, LoadIconW, MB_ICONINFORMATION, MB_OK, MF_STRING, MSG, MessageBoxW,
                MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
                SW_HIDE, SW_SHOW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
                TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
                WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND,
                WM_CONTEXTMENU, WM_COPYDATA, WM_CREATE, WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDBLCLK,
                WM_NCDESTROY, WM_RBUTTONUP, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CHILD,
                WS_EX_CLIENTEDGE, WS_GROUP, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
                WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const ID_MENU_LIST: usize = 1001;
const ID_OPEN: usize = 1002;
const WM_PROCESS_ACTIVATION: u32 = WM_APP + 1;
const WM_TRAY_ICON: u32 = WM_APP + 2;
const TRAY_ICON_ID: u32 = 1;
const ID_TRAY_SHOW: usize = 1301;
const ID_TRAY_SETTINGS: usize = 1302;
const ID_TRAY_CHECK_SUBSCRIPTIONS: usize = 1303;
const ID_TRAY_EXIT: usize = 1304;
const NIN_SELECT_CODE: u32 = 1024;
const NIN_KEYSELECT_CODE: u32 = 1025;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowLifecycle {
    Visible,
    HiddenInTray,
    Exiting,
}

struct WindowState {
    list: HWND,
    open: HWND,
    model: MainMenuModel,
    application: Application,
    settings_open: bool,
    tray_icon_added: bool,
    lifecycle: WindowLifecycle,
    taskbar_created_message: u32,
}

pub fn run_application(application: Application, version: &str, start_hidden: bool) -> Result<()> {
    // SAFETY: The window, state pointer, controls, and message loop are confined
    // to this thread. Dynamic UTF-16 buffers outlive each Win32 call that uses
    // them, and owned state is released exactly once during WM_DESTROY.
    unsafe { run_win32(application, version, start_hidden) }
}

unsafe fn run_win32(application: Application, version: &str, start_hidden: bool) -> Result<()> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let class_name = crate::activation_win32::MAIN_WINDOW_CLASS;
    let class = WNDCLASSW {
        cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size fits in i32"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hInstance: instance,
        lpszClassName: class_name,
        lpfnWndProc: Some(window_proc),
        ..Default::default()
    };
    if RegisterClassW(&raw const class) == 0 {
        return Err(windows::core::Error::from_thread());
    }
    crate::settings_win32::register()?;

    let title = wide(&format!("ApricotPlayer 2 Beta {version}"));
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class_name,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        760,
        600,
        None,
        None,
        Some(instance),
        None,
    )?;
    let state = match create_controls(window, instance, application) {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let initial_focus = state.list;
    SetWindowLongPtrW(
        window,
        WINDOW_LONG_PTR_INDEX(0),
        Box::into_raw(Box::new(state)) as isize,
    );
    layout_controls(window);
    if start_hidden {
        hide_to_tray(window);
    } else {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetFocus(Some(initial_focus));
    }
    process_pending_activations(window);

    let mut message = MSG::default();
    loop {
        let result = GetMessageW(&raw mut message, None, 0, 0);
        if result.0 == -1 {
            return Err(windows::core::Error::from_thread());
        }
        if result.0 == 0 {
            break;
        }
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
    if let Some(state) = state_mut(window)
        && message == state.taskbar_created_message
    {
        state.tray_icon_added = false;
        if state.lifecycle == WindowLifecycle::HiddenInTray {
            add_tray_icon(window);
        }
        return LRESULT(0);
    }
    match message {
        WM_CREATE => LRESULT(0),
        WM_SIZE => {
            layout_controls(window);
            LRESULT(0)
        }
        WM_COMMAND => {
            let command = wparam.0 & 0xffff;
            let notification = (wparam.0 >> 16) & 0xffff;
            if command == ID_OPEN
                || (command == ID_MENU_LIST
                    && notification == usize::try_from(LBN_DBLCLK).expect("notification fits"))
            {
                activate_selection(window);
            } else if matches!(
                command,
                ID_TRAY_SHOW | ID_TRAY_SETTINGS | ID_TRAY_CHECK_SUBSCRIPTIONS | ID_TRAY_EXIT
            ) {
                handle_tray_command(window, command);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if state(window).is_some_and(|state| {
                state.lifecycle != WindowLifecycle::Exiting
                    && state.application.settings().close_to_tray
            }) {
                hide_to_tray(window);
            } else {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_TRAY_ICON => {
            handle_tray_message(window, lparam);
            LRESULT(0)
        }
        WM_COPYDATA => {
            if let Some(request) = crate::activation_win32::decode_request(lparam)
                && let Some(state) = state_mut(window)
            {
                state.application.enqueue_activation(request);
                restore_from_tray(window);
                let _ = PostMessageW(Some(window), WM_PROCESS_ACTIVATION, WPARAM(0), LPARAM(0));
                return LRESULT(1);
            }
            LRESULT(0)
        }
        WM_PROCESS_ACTIVATION => {
            process_pending_activations(window);
            LRESULT(0)
        }
        WM_DESTROY => {
            remove_tray_icon(window);
            let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut WindowState;
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

unsafe fn create_controls(
    parent: HWND,
    instance: HINSTANCE,
    application: Application,
) -> Result<WindowState> {
    let model = application.main_menu_model();
    let accessible_name = wide(&model.accessible_name);
    let list = create_control(
        parent,
        instance,
        w!("LISTBOX"),
        PCWSTR(accessible_name.as_ptr()),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_GROUP
            | WS_VSCROLL
            | WINDOW_STYLE(LBS_NOTIFY as u32),
        WS_EX_CLIENTEDGE,
        ID_MENU_LIST,
    )?;
    for item in &model.items {
        let label = wide(&item.label);
        SendMessageW(
            list,
            LB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    }
    SendMessageW(list, LB_SETCURSEL, Some(WPARAM(0)), None);
    if !SetWindowSubclass(list, Some(menu_list_proc), 1, 0).as_bool() {
        return Err(windows::core::Error::from_thread());
    }

    let open = create_control(
        parent,
        instance,
        w!("BUTTON"),
        w!("Open"),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        ID_OPEN,
    )?;
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let font_param = Some(WPARAM(font.0 as usize));
    SendMessageW(list, WM_SETFONT, font_param, Some(LPARAM(1)));
    SendMessageW(open, WM_SETFONT, font_param, Some(LPARAM(1)));
    Ok(WindowState {
        list,
        open,
        model,
        application,
        settings_open: false,
        tray_icon_added: false,
        lifecycle: WindowLifecycle::Visible,
        taskbar_created_message: RegisterWindowMessageW(w!("TaskbarCreated")),
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
        100,
        30,
        Some(parent),
        Some(HMENU(id as *mut c_void)),
        Some(instance),
        None,
    )
}

unsafe extern "system" fn menu_list_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    if message == WM_KEYDOWN
        && wparam.0 == usize::from(VK_RETURN.0)
        && let Ok(parent) = windows::Win32::UI::WindowsAndMessaging::GetParent(window)
    {
        SendMessageW(parent, WM_COMMAND, Some(WPARAM(ID_OPEN)), None);
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(menu_list_proc), subclass_id);
    }
    DefSubclassProc(window, message, wparam, lparam)
}

unsafe fn state(window: HWND) -> Option<&'static WindowState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const WindowState;
    pointer.as_ref()
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut WindowState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut WindowState;
    pointer.as_mut()
}

unsafe fn layout_controls(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let width = (bounds.right - bounds.left).max(320);
    let height = (bounds.bottom - bounds.top).max(240);
    let margin = 12;
    let button_height = 34;
    let _ = MoveWindow(
        state.list,
        margin,
        margin,
        width - margin * 2,
        height - button_height - margin * 3,
        true,
    );
    let _ = MoveWindow(
        state.open,
        margin,
        height - button_height - margin,
        120,
        button_height,
        true,
    );
}

unsafe fn add_tray_icon(window: HWND) -> bool {
    if state(window).is_some_and(|state| state.tray_icon_added) {
        return true;
    }
    let Ok(icon) = LoadIconW(None, IDI_APPLICATION) else {
        return false;
    };
    let mut data = NOTIFYICONDATAW {
        cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>()).expect("tray data size fits"),
        hWnd: window,
        uID: TRAY_ICON_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY_ICON,
        hIcon: icon,
        ..Default::default()
    };
    copy_wide_array(&mut data.szTip, "ApricotPlayer 2 Beta");
    if !Shell_NotifyIconW(NIM_ADD, &raw const data).as_bool() {
        return false;
    }
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    let _ = Shell_NotifyIconW(NIM_SETVERSION, &raw const data);
    if let Some(state) = state_mut(window) {
        state.tray_icon_added = true;
    }
    true
}

unsafe fn remove_tray_icon(window: HWND) {
    if !state(window).is_some_and(|state| state.tray_icon_added) {
        return;
    }
    let data = NOTIFYICONDATAW {
        cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>()).expect("tray data size fits"),
        hWnd: window,
        uID: TRAY_ICON_ID,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &raw const data);
    if let Some(state) = state_mut(window) {
        state.tray_icon_added = false;
    }
}

unsafe fn hide_to_tray(window: HWND) {
    if !add_tray_icon(window) {
        let _ = ShowWindow(window, SW_SHOW);
        if let Some(state) = state(window) {
            let _ = SetFocus(Some(state.list));
        }
        return;
    }
    if let Some(state) = state_mut(window) {
        state.lifecycle = WindowLifecycle::HiddenInTray;
    }
    let _ = ShowWindow(window, SW_HIDE);
}

unsafe fn restore_from_tray(window: HWND) {
    if let Some(state) = state_mut(window) {
        state.lifecycle = WindowLifecycle::Visible;
    }
    crate::activation_win32::restore_window(window);
    remove_tray_icon(window);
    if let Some(state) = state(window) {
        let _ = SetFocus(Some(state.list));
    }
}

unsafe fn handle_tray_message(window: HWND, lparam: LPARAM) {
    let event = u32::try_from(lparam.0 & 0xffff).unwrap_or_default();
    if matches!(
        event,
        WM_LBUTTONDBLCLK | NIN_SELECT_CODE | NIN_KEYSELECT_CODE
    ) {
        restore_from_tray(window);
    } else if matches!(event, WM_RBUTTONUP | WM_CONTEXTMENU) {
        show_tray_menu(window);
    }
}

unsafe fn show_tray_menu(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    for (id, key) in [
        (ID_TRAY_SHOW, "tray_show"),
        (ID_TRAY_SETTINGS, "tray_settings"),
        (ID_TRAY_CHECK_SUBSCRIPTIONS, "tray_check_subscriptions"),
        (ID_TRAY_EXIT, "tray_exit"),
    ] {
        let label = wide(catalog.text(key));
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(label.as_ptr()));
    }
    let mut point = POINT::default();
    if GetCursorPos(&raw mut point).is_ok() {
        let _ = SetForegroundWindow(window);
        let selected = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            None,
            window,
            None,
        );
        if selected.0 > 0 {
            handle_tray_command(window, usize::try_from(selected.0).unwrap_or_default());
        }
    }
    let _ = DestroyMenu(menu);
}

unsafe fn handle_tray_command(window: HWND, command: usize) {
    match command {
        ID_TRAY_SHOW => restore_from_tray(window),
        ID_TRAY_SETTINGS => {
            restore_from_tray(window);
            open_settings(window);
        }
        ID_TRAY_CHECK_SUBSCRIPTIONS => {
            let message = wide(
                "Subscription checking is registered, but its Rust service is not implemented in this internal build yet.",
            );
            let _ = MessageBoxW(
                Some(window),
                PCWSTR(message.as_ptr()),
                w!("ApricotPlayer 2 Beta"),
                MB_OK | MB_ICONINFORMATION,
            );
        }
        ID_TRAY_EXIT => {
            if let Some(state) = state_mut(window) {
                state.lifecycle = WindowLifecycle::Exiting;
            }
            let _ = DestroyWindow(window);
        }
        _ => {}
    }
}

fn copy_wide_array<const N: usize>(target: &mut [u16; N], value: &str) {
    target.fill(0);
    let mut units = value.encode_utf16();
    for slot in target.iter_mut().take(N.saturating_sub(1)) {
        let Some(unit) = units.next() else {
            break;
        };
        *slot = unit;
    }
}

unsafe fn activate_selection(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    let Ok(index) = usize::try_from(selected) else {
        return;
    };
    let Some(item) = state.model.items.get(index) else {
        return;
    };
    if item.id == "exit" {
        let _ = DestroyWindow(window);
        return;
    }
    if item.id == "settings" {
        open_settings(window);
        return;
    }

    let message = wide(&format!(
        "{} is registered, but its Rust screen is not implemented in this internal build yet.",
        item.label.split('\t').next().unwrap_or(&item.label)
    ));
    let title = wide("ApricotPlayer 2 Beta");
    let _ = MessageBoxW(
        Some(window),
        PCWSTR(message.as_ptr()),
        PCWSTR(title.as_ptr()),
        MB_OK | MB_ICONINFORMATION,
    );
    let _ = SetFocus(Some(state.list));
}

unsafe fn open_settings(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.settings_open {
        return;
    }
    state.settings_open = true;
    if let Err(error) = crate::settings_win32::show(window, &mut state.application) {
        let message = wide(&error.to_string());
        let _ = MessageBoxW(
            Some(window),
            PCWSTR(message.as_ptr()),
            w!("ApricotPlayer 2 Beta"),
            MB_OK | MB_ICONINFORMATION,
        );
    }
    state.settings_open = false;
    refresh_main_menu(state);
    let _ = SetFocus(Some(state.list));
    process_pending_activations(window);
}

unsafe fn process_pending_activations(window: HWND) {
    loop {
        let Some(state) = state_mut(window) else {
            return;
        };
        if state.settings_open {
            return;
        }
        let Some(request) = state.application.take_activation() else {
            return;
        };
        match request {
            ActivationRequest::Show => restore_from_tray(window),
            ActivationRequest::OpenSettings => {
                restore_from_tray(window);
                open_settings(window);
            }
            ActivationRequest::OpenFile(path) => {
                restore_from_tray(window);
                let message = wide(&format!(
                    "{} is ready for the local-file route, which is not implemented in this internal build yet.",
                    path.display()
                ));
                let _ = MessageBoxW(
                    Some(window),
                    PCWSTR(message.as_ptr()),
                    w!("ApricotPlayer 2 Beta"),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
        }
    }
}

unsafe fn refresh_main_menu(state: &mut WindowState) {
    state.model = state.application.main_menu_model();
    SendMessageW(state.list, 0x0184, None, None);
    for item in &state.model.items {
        let label = wide(&item.label);
        SendMessageW(
            state.list,
            LB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    }
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::copy_wide_array;

    #[test]
    fn tray_text_is_cleared_truncated_and_null_terminated() {
        let mut target = [u16::MAX; 5];
        copy_wide_array(&mut target, "abcdef");
        assert_eq!(target, ['a' as u16, 'b' as u16, 'c' as u16, 'd' as u16, 0]);

        copy_wide_array(&mut target, "x");
        assert_eq!(target, ['x' as u16, 0, 0, 0, 0]);
    }
}
