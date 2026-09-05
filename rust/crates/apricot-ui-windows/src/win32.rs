//! Isolated unsafe Win32 boundary for the production Windows shell.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use apricot_app::{
    ActionFinderContext, ActivationRequest, Application, MainMenuModel, PlaybackPhase,
    SearchApplyOutcome, SearchWork, SearchWorkKind, SessionToggle, YoutubeSearchKind,
};
use apricot_core::{
    Route, RouteFrame,
    action::{ActionScope, RepeatPolicy},
    shortcut::{ShortcutContext, ShortcutKey, action_for_shortcut},
};
use apricot_media::{
    YoutubeBackend, YoutubeFormat, YoutubeSessionConfig, YoutubeStreamPreference,
    select_youtube_playback_formats,
};
use apricot_platform::{YoutubeSearchService, YoutubeSearchServiceUpdate};
use apricot_playback::{
    InitialPlaybackState, MpvCacheConfig, MpvLaunchOptions, MpvVideoMode, PlaybackCommand,
    PlaybackEvent, PlaybackRuntime, RepeatMode,
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, SetFocus, VK_RETURN},
            Shell::{
                DefSubclassProc, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
                NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
                RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, BS_DEFPUSHBUTTON, CBS_DROPDOWNLIST, CW_USEDEFAULT, CreatePopupMenu,
                CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
                ES_AUTOHSCROLL, GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW,
                GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IDI_APPLICATION,
                IsDialogMessageW, KillTimer, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT,
                LB_SETCURSEL, LBN_DBLCLK, LBN_SELCHANGE, LBS_NOTIFY, LoadCursorW, LoadIconW,
                MB_ICONINFORMATION, MB_OK, MF_STRING, MSG, MessageBoxW, MoveWindow, PostMessageW,
                PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SW_HIDE, SW_SHOW,
                SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowTextW,
                ShowWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
                TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_APP,
                WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_COPYDATA, WM_CREATE, WM_DESTROY,
                WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_NCDESTROY, WM_RBUTTONUP, WM_SETFONT, WM_SIZE,
                WM_TIMER, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE, WS_GROUP, WS_OVERLAPPEDWINDOW,
                WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const ID_MENU_LIST: usize = 1001;
const ID_OPEN: usize = 1002;
const ID_SEARCH_EDIT: usize = 1003;
const ID_SEARCH_KIND: usize = 1004;
const ID_SEARCH: usize = 1005;
const ID_BACK: usize = 1006;
const WM_PROCESS_ACTIVATION: u32 = WM_APP + 1;
const WM_TRAY_ICON: u32 = WM_APP + 2;
const YOUTUBE_TIMER_ID: usize = 1;
const YOUTUBE_TIMER_INTERVAL_MS: u32 = 25;
const PLAYBACK_TIMER_ID: usize = 2;
const PLAYBACK_TIMER_INTERVAL_MS: u32 = 25;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MainView {
    MainMenu,
    Search,
    Results,
    Player,
}

struct WindowState {
    list: HWND,
    open: HWND,
    search_label: HWND,
    search_edit: HWND,
    kind_label: HWND,
    kind: HWND,
    search: HWND,
    back: HWND,
    video_host: HWND,
    status: HWND,
    announcer: crate::announcement_win32::WindowsAnnouncer,
    model: MainMenuModel,
    application: Application,
    settings_open: bool,
    tray_icon_added: bool,
    lifecycle: WindowLifecycle,
    taskbar_created_message: u32,
    view: MainView,
    youtube_search: YoutubeSearchService,
    pending_youtube_work: Option<SearchWork>,
    pending_youtube_resolve: Option<u64>,
    next_youtube_resolve_token: u64,
    playback: Option<PlaybackRuntime>,
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
    crate::action_finder_win32::register()?;

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
        hide_to_tray(window, false);
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
        if handle_shortcut_message(window, &message) {
            continue;
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
            } else if command == ID_MENU_LIST
                && notification == usize::try_from(LBN_SELCHANGE).expect("notification fits")
            {
                result_selection_changed(window);
            } else if command == ID_SEARCH {
                submit_search(window);
            } else if command == ID_BACK {
                navigate_back(window);
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
                hide_to_tray(window, true);
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
        WM_TIMER if wparam.0 == YOUTUBE_TIMER_ID => {
            poll_youtube_runtime(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == PLAYBACK_TIMER_ID => {
            poll_playback_runtime(window);
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

#[allow(clippy::too_many_lines)]
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

    let catalog = apricot_app::embedded_catalog(&application.settings().language);
    let open_text = wide(catalog.text("open"));
    let open = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(open_text.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        ID_OPEN,
    )?;
    let ready = wide(catalog.text("ready"));
    let status = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(ready.as_ptr()),
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let search_label_text = wide(catalog.text("search_query"));
    let search_label = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(search_label_text.as_ptr()),
        WS_CHILD | WS_GROUP,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let search_edit = create_control(
        parent,
        instance,
        w!("EDIT"),
        PCWSTR::null(),
        WS_CHILD | WS_TABSTOP | WS_GROUP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        WS_EX_CLIENTEDGE,
        ID_SEARCH_EDIT,
    )?;
    let kind_label_text = wide(catalog.text("type"));
    let kind_label = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(kind_label_text.as_ptr()),
        WS_CHILD,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let kind = create_control(
        parent,
        instance,
        w!("COMBOBOX"),
        PCWSTR::null(),
        WS_CHILD | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL,
        WINDOW_EX_STYLE::default(),
        ID_SEARCH_KIND,
    )?;
    for key in ["all", "video", "playlist", "channel"] {
        let label = wide(catalog.text(key));
        SendMessageW(
            kind,
            CB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    }
    SendMessageW(kind, CB_SETCURSEL, Some(WPARAM(0)), None);
    let search_text = wide(catalog.text("search"));
    let search = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(search_text.as_ptr()),
        WS_CHILD | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        ID_SEARCH,
    )?;
    let back_text = wide(catalog.text("back"));
    let back = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(back_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_BACK,
    )?;
    let video_host_name = wide(catalog.text("player"));
    let video_host = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(video_host_name.as_ptr()),
        WS_CHILD | WS_TABSTOP | WS_GROUP,
        WS_EX_CLIENTEDGE,
        0,
    )?;
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let font_param = Some(WPARAM(font.0 as usize));
    for control in [
        list,
        open,
        search_label,
        search_edit,
        kind_label,
        kind,
        search,
        back,
        video_host,
        status,
    ] {
        SendMessageW(control, WM_SETFONT, font_param, Some(LPARAM(1)));
    }
    Ok(WindowState {
        list,
        open,
        search_label,
        search_edit,
        kind_label,
        kind,
        search,
        back,
        video_host,
        status,
        announcer: crate::announcement_win32::WindowsAnnouncer::new(status),
        model,
        application,
        settings_open: false,
        tray_icon_added: false,
        lifecycle: WindowLifecycle::Visible,
        taskbar_created_message: RegisterWindowMessageW(w!("TaskbarCreated")),
        view: MainView::MainMenu,
        youtube_search: YoutubeSearchService::default(),
        pending_youtube_work: None,
        pending_youtube_resolve: None,
        next_youtube_resolve_token: 0,
        playback: None,
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

#[allow(clippy::too_many_lines)]
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
    let status_height = 24;
    let label_height = 22;
    let field_height = 30;
    set_view_visibility(state);
    if state.view == MainView::Search {
        let _ = MoveWindow(
            state.search_label,
            margin,
            margin,
            width - margin * 2,
            label_height,
            true,
        );
        let _ = MoveWindow(
            state.search_edit,
            margin,
            margin + label_height,
            width - margin * 2,
            field_height,
            true,
        );
        let kind_y = margin + label_height + field_height + margin;
        let _ = MoveWindow(
            state.kind_label,
            margin,
            kind_y,
            width - margin * 2,
            label_height,
            true,
        );
        let _ = MoveWindow(
            state.kind,
            margin,
            kind_y + label_height,
            width - margin * 2,
            240,
            true,
        );
    } else if state.view == MainView::Player {
        let video_height = ((height - button_height - status_height - margin * 5) / 2).max(80);
        layout_player_controls(
            state,
            width,
            height,
            margin,
            button_height,
            status_height,
            video_height,
        );
    } else {
        let _ = MoveWindow(
            state.list,
            margin,
            margin,
            width - margin * 2,
            height - button_height - status_height - margin * 4,
            true,
        );
    }
    let _ = MoveWindow(
        state.status,
        margin,
        height - button_height - status_height - margin * 2,
        width - margin * 2,
        status_height,
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
    let _ = MoveWindow(
        state.search,
        margin,
        height - button_height - margin,
        120,
        button_height,
        true,
    );
    let _ = MoveWindow(
        state.back,
        margin + 132,
        height - button_height - margin,
        180,
        button_height,
        true,
    );
}

#[allow(clippy::too_many_arguments)]
unsafe fn layout_player_controls(
    state: &WindowState,
    width: i32,
    height: i32,
    margin: i32,
    button_height: i32,
    status_height: i32,
    video_height: i32,
) {
    let _ = MoveWindow(
        state.video_host,
        margin,
        margin,
        width - margin * 2,
        video_height,
        true,
    );
    let _ = MoveWindow(
        state.list,
        margin,
        margin * 2 + video_height,
        width - margin * 2,
        height - video_height - button_height - status_height - margin * 5,
        true,
    );
}

unsafe fn set_view_visibility(state: &WindowState) {
    let list_visible = state.view != MainView::Search;
    let search_visible = state.view == MainView::Search;
    let back_visible = state.view != MainView::MainMenu;
    let video_visible = state.view == MainView::Player;
    for (control, visible) in [
        (state.list, list_visible),
        (state.open, list_visible),
        (state.search_label, search_visible),
        (state.search_edit, search_visible),
        (state.kind_label, search_visible),
        (state.kind, search_visible),
        (state.search, search_visible),
        (state.back, back_visible),
        (state.video_host, video_visible),
    ] {
        let _ = ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE });
    }
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

unsafe fn hide_to_tray(window: HWND, announce: bool) {
    if !add_tray_icon(window) {
        let _ = ShowWindow(window, SW_SHOW);
        if let Some(state) = state(window) {
            let _ = SetFocus(Some(active_primary_control(state)));
        }
        return;
    }
    if let Some(state) = state_mut(window) {
        state.lifecycle = WindowLifecycle::HiddenInTray;
    }
    let _ = ShowWindow(window, SW_HIDE);
    if announce {
        announce_tray_state(window);
    }
}

unsafe fn announce_tray_state(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let message = catalog.text("tray_still_running");
    state.announcer.announce(message, true);
    if state.application.settings().tray_notification
        && state.application.settings().windows_notifications
    {
        show_tray_notification(window, "ApricotPlayer 2 Beta", message);
    }
}

unsafe fn show_tray_notification(window: HWND, title: &str, message: &str) {
    if !state(window).is_some_and(|state| state.tray_icon_added) {
        return;
    }
    let mut data = NOTIFYICONDATAW {
        cbSize: u32::try_from(size_of::<NOTIFYICONDATAW>()).expect("tray data size fits"),
        hWnd: window,
        uID: TRAY_ICON_ID,
        uFlags: NIF_INFO,
        dwInfoFlags: NIIF_INFO,
        ..Default::default()
    };
    copy_wide_array(&mut data.szInfoTitle, title);
    copy_wide_array(&mut data.szInfo, message);
    let _ = Shell_NotifyIconW(NIM_MODIFY, &raw const data);
}

unsafe fn restore_from_tray(window: HWND) {
    if let Some(state) = state_mut(window) {
        state.lifecycle = WindowLifecycle::Visible;
    }
    crate::activation_win32::restore_window(window);
    remove_tray_icon(window);
    if let Some(state) = state(window) {
        let _ = SetFocus(Some(active_primary_control(state)));
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
    match state(window).map(|state| state.view) {
        Some(MainView::MainMenu) => activate_main_menu_selection(window),
        Some(MainView::Results) => activate_result_selection(window),
        Some(MainView::Player) => activate_player_selection(window),
        Some(MainView::Search) | None => {}
    }
}

unsafe fn activate_main_menu_selection(window: HWND) {
    let Some((item_id, item_label)) = selected_main_menu_item(window) else {
        return;
    };
    if item_id == "exit" {
        let _ = DestroyWindow(window);
        return;
    }
    if item_id == "settings" {
        open_settings(window);
        return;
    }
    if item_id == "play_file" {
        open_media_file(window);
        return;
    }
    if item_id == "search" {
        show_search(window);
        return;
    }

    let message = wide(&format!(
        "{} is registered, but its Rust screen is not implemented in this internal build yet.",
        item_label.split('\t').next().unwrap_or(&item_label)
    ));
    let title = wide("ApricotPlayer 2 Beta");
    let _ = MessageBoxW(
        Some(window),
        PCWSTR(message.as_ptr()),
        PCWSTR(title.as_ptr()),
        MB_OK | MB_ICONINFORMATION,
    );
    if let Some(state) = state(window) {
        let _ = SetFocus(Some(active_primary_control(state)));
    }
}

unsafe fn activate_result_selection(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    let Ok(index) = usize::try_from(selected) else {
        return;
    };
    if !state.application.select_search_result(index) {
        return;
    }
    let Some(item) = state.application.search_session().selected_item().cloned() else {
        return;
    };
    if matches!(
        item.kind,
        apricot_core::MediaKind::Playlist | apricot_core::MediaKind::Channel
    ) {
        let message = wide(&format!(
            "{} is ready, but collection navigation is not implemented in this internal build yet.",
            item.title
        ));
        let _ = MessageBoxW(
            Some(window),
            PCWSTR(message.as_ptr()),
            w!("ApricotPlayer 2 Beta"),
            MB_OK | MB_ICONINFORMATION,
        );
        let _ = SetFocus(Some(state.list));
        return;
    }
    let Some(url) = item.url.as_ref().map(ToString::to_string) else {
        finish_youtube_error_state(window, state, 0, "The selected item has no media URL");
        return;
    };
    cancel_youtube_work(window, state);
    state.next_youtube_resolve_token = state.next_youtube_resolve_token.wrapping_add(1).max(1);
    let token = state.next_youtube_resolve_token;
    let backend = YoutubeBackend::from_setting_value(&state.application.settings().youtube_backend);
    let Some(components) = application_directory().map(|path| path.join("components")) else {
        finish_youtube_error_state(window, state, token, "Application path is unavailable");
        return;
    };
    let preference =
        youtube_stream_preference(&state.application.settings().stream_format_preference);
    let config = youtube_session_config(state);
    match state
        .youtube_search
        .start_resolve(backend, &components, config, token, url, preference)
    {
        Ok(()) => {
            state.pending_youtube_resolve = Some(token);
            set_status(
                state,
                &catalog_text(&state.application, "resolving_stream_url"),
                true,
            );
            let _ = SetTimer(
                Some(window),
                YOUTUBE_TIMER_ID,
                YOUTUBE_TIMER_INTERVAL_MS,
                None,
            );
        }
        Err(error) => finish_youtube_error_state(window, state, token, &error.to_string()),
    }
}

unsafe fn activate_player_selection(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    let Ok(index) = usize::try_from(selected) else {
        return;
    };
    let Some(model) = state.application.player_screen_model() else {
        return;
    };
    let Some(control) = model.controls.get(index) else {
        return;
    };
    if let Some(action_id) = control.action_id {
        activate_action(window, action_id);
    } else if control.id == "session_autoplay_next" {
        toggle_player_session_setting(window, SessionToggle::AutoplayNext);
    }
}

unsafe fn finish_youtube_resolve(
    window: HWND,
    token: u64,
    mut item: apricot_core::MediaItem,
    formats: &[YoutubeFormat],
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.pending_youtube_resolve != Some(token) {
        return;
    }
    state.pending_youtube_resolve = None;
    stop_youtube_timer(window);
    let preference =
        youtube_stream_preference(&state.application.settings().stream_format_preference);
    let Some(selection) = select_youtube_playback_formats(formats, preference) else {
        finish_youtube_resolve_selection_error(
            window,
            state,
            "No playable YouTube stream was returned",
        );
        return;
    };
    let Some(primary) = formats.get(selection.primary_index) else {
        finish_youtube_resolve_selection_error(
            window,
            state,
            "The selected YouTube stream was invalid",
        );
        return;
    };
    let Ok(stream_url) = primary.url.parse() else {
        finish_youtube_resolve_selection_error(
            window,
            state,
            "The selected YouTube stream URL was invalid",
        );
        return;
    };
    item.stream_url = Some(stream_url);
    item.external_audio_url = selection
        .external_audio_index
        .and_then(|index| formats.get(index))
        .and_then(|format| format.url.parse().ok());
    start_player(window, item);
}

unsafe fn finish_youtube_resolve_selection_error(window: HWND, state: &WindowState, message: &str) {
    set_status(state, message, true);
    show_error_message(window, message);
    let _ = SetFocus(Some(state.list));
}

unsafe fn start_player(window: HWND, item: apricot_core::MediaItem) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.playback.is_none() {
        match PlaybackRuntime::spawn() {
            Ok(runtime) => state.playback = Some(runtime),
            Err(error) => {
                show_error_message(window, &format!("Player did not start: {error}"));
                return;
            }
        }
    }
    let generation = state.application.start_player_item(item);
    let Some(options) = playback_launch_options(state) else {
        let message = "Internal mpv player was not found";
        let _ = state
            .application
            .apply_playback_event(generation, PlaybackEvent::Failed(message.to_owned()));
        show_error_message(window, message);
        return;
    };
    let start_result = state
        .playback
        .as_ref()
        .expect("playback runtime was initialized")
        .start(
            generation,
            options,
            state
                .application
                .player_session()
                .current_item()
                .expect("player item was started")
                .clone(),
        );
    if let Err(error) = start_result {
        let message = format!("Player did not start: {error}");
        let _ = state
            .application
            .apply_playback_event(generation, PlaybackEvent::Failed(message.clone()));
        show_error_message(window, &message);
        return;
    }
    if state.application.current_route() != Route::Player {
        state
            .application
            .navigate_to(RouteFrame::new(Route::Player));
    }
    state.view = MainView::Player;
    refresh_player(window, state, true, false);
    let _ = SetTimer(
        Some(window),
        PLAYBACK_TIMER_ID,
        PLAYBACK_TIMER_INTERVAL_MS,
        None,
    );
}

unsafe fn refresh_player(
    window: HWND,
    state: &mut WindowState,
    focus: bool,
    preserve_selection: bool,
) {
    let previous_selection = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    let Some(model) = state.application.player_screen_model() else {
        return;
    };
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let accessible_name = wide(&model.heading);
    let _ = SetWindowTextW(state.list, PCWSTR(accessible_name.as_ptr()));
    for control in &model.controls {
        add_list_string(state.list, &control.label);
    }
    let selected = if preserve_selection {
        usize::try_from(previous_selection)
            .ok()
            .filter(|index| *index < model.controls.len())
            .unwrap_or_default()
    } else {
        model
            .controls
            .iter()
            .position(|control| control.id == model.initial_focus_id)
            .unwrap_or_default()
    };
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    let title = wide(&model.window_title);
    let _ = SetWindowTextW(window, PCWSTR(title.as_ptr()));
    layout_controls(window);
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

unsafe fn selected_main_menu_item(window: HWND) -> Option<(&'static str, String)> {
    let state = state(window)?;
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    let index = usize::try_from(selected).ok()?;
    let item = state.model.items.get(index)?;
    Some((item.id, item.label.clone()))
}

unsafe fn show_main_menu(window: HWND) {
    restore_from_tray(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    state.application.navigate_main_menu();
    state.view = MainView::MainMenu;
    refresh_main_menu(state);
    set_status(state, &catalog_text(&state.application, "ready"), false);
    layout_controls(window);
    let _ = SetFocus(Some(state.list));
}

unsafe fn show_search(window: HWND) {
    restore_from_tray(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    if state.application.current_route() != Route::Search {
        state.application.navigate_main_menu();
        state
            .application
            .navigate_to(RouteFrame::new(Route::Search));
    }
    state.view = MainView::Search;
    set_status(state, &catalog_text(&state.application, "ready"), false);
    layout_controls(window);
    let _ = SetFocus(Some(state.search_edit));
}

unsafe fn navigate_back(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    if state.view == MainView::Player {
        close_player_runtime(window, state);
    }
    let route = state
        .application
        .navigate_back()
        .map_or(Route::MainMenu, |frame| frame.route);
    match route {
        Route::Search => {
            state.view = MainView::Search;
            layout_controls(window);
            let _ = SetFocus(Some(state.search_edit));
        }
        Route::Results => {
            state.view = MainView::Results;
            refresh_results(state, true);
            layout_controls(window);
        }
        Route::Player => {
            state.view = MainView::Player;
            refresh_player(window, state, true, true);
        }
        _ => {
            state.application.navigate_main_menu();
            state.view = MainView::MainMenu;
            refresh_main_menu(state);
            layout_controls(window);
            let _ = SetFocus(Some(state.list));
        }
    }
}

unsafe fn submit_search(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let query = window_text(state.search_edit);
    let kind = selected_search_kind(state.kind);
    let work = match state.application.begin_youtube_search(&query, kind) {
        Ok(work) => work,
        Err(error) => {
            set_status(state, &error.to_string(), true);
            let _ = SetFocus(Some(state.search_edit));
            return;
        }
    };
    let message = catalog_text(&state.application, "searching")
        .replace("{query}", state.application.search_session().query());
    set_status(state, &message, true);
    let _ = EnableWindow(state.search, false);
    start_youtube_work(window, work);
}

unsafe fn start_youtube_work(window: HWND, work: SearchWork) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let backend = YoutubeBackend::from_setting_value(&state.application.settings().youtube_backend);
    let Some(components) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("components")))
    else {
        finish_youtube_error_state(
            window,
            state,
            work.generation,
            "Application path is unavailable",
        );
        return;
    };
    let config = youtube_session_config(state);
    match state.youtube_search.start(
        backend,
        &components,
        config,
        work.generation,
        work.query.clone(),
        work.kind,
        work.limit,
    ) {
        Ok(()) => {
            state.pending_youtube_work = Some(work);
            let _ = SetTimer(
                Some(window),
                YOUTUBE_TIMER_ID,
                YOUTUBE_TIMER_INTERVAL_MS,
                None,
            );
        }
        Err(error) => {
            finish_youtube_error_state(window, state, work.generation, &error.to_string());
        }
    }
}

unsafe fn poll_youtube_runtime(window: HWND) {
    loop {
        let update = {
            let Some(state) = state_mut(window) else {
                return;
            };
            state.youtube_search.poll()
        };
        let update = match update {
            Ok(Some(update)) => update,
            Ok(None) => return,
            Err(error) => {
                let generation = state(window)
                    .and_then(|state| {
                        state.pending_youtube_resolve.or_else(|| {
                            state
                                .pending_youtube_work
                                .as_ref()
                                .map(|work| work.generation)
                        })
                    })
                    .unwrap_or_default();
                finish_youtube_error(window, generation, &error.to_string());
                return;
            }
        };
        match update {
            YoutubeSearchServiceUpdate::Results {
                token: generation,
                items,
                continuation,
            } => finish_youtube_search(window, generation, items, continuation),
            YoutubeSearchServiceUpdate::Resolved {
                token,
                item,
                formats,
            } => finish_youtube_resolve(window, token, *item, &formats),
            YoutubeSearchServiceUpdate::Failed {
                token: generation,
                message,
            } => finish_youtube_error(window, generation, &message),
        }
    }
}

unsafe fn poll_playback_runtime(window: HWND) {
    loop {
        let update = {
            let Some(state) = state(window) else {
                return;
            };
            let Some(runtime) = state.playback.as_ref() else {
                stop_playback_timer(window);
                return;
            };
            runtime.poll_update()
        };
        let update = match update {
            Ok(Some(update)) => update,
            Ok(None) => return,
            Err(error) => {
                stop_playback_timer(window);
                if let Some(state) = state_mut(window) {
                    state.playback = None;
                }
                show_error_message(window, &format!("Player stopped: {error}"));
                return;
            }
        };
        let Some(state) = state_mut(window) else {
            return;
        };
        if !state
            .application
            .apply_playback_event(update.generation, update.event.clone())
        {
            continue;
        }
        match update.event {
            PlaybackEvent::Started => {
                let title = state
                    .application
                    .player_session()
                    .current_item()
                    .map_or("", |item| item.title.as_str());
                let message = catalog_text(&state.application, "playing").replace("{title}", title);
                set_status(state, &message, true);
            }
            PlaybackEvent::Paused(paused) => {
                let key = if paused {
                    "playback_paused"
                } else {
                    "playback_playing"
                };
                set_status(
                    state,
                    &catalog_text(&state.application, key),
                    state.application.settings().announce_play_pause,
                );
            }
            PlaybackEvent::Position { .. } => {}
            PlaybackEvent::Ended => {
                set_status(
                    state,
                    &catalog_text(&state.application, "playback_finished"),
                    true,
                );
            }
            PlaybackEvent::Failed(error) => {
                let message =
                    catalog_text(&state.application, "player_failed").replace("{error}", &error);
                set_status(state, &message, true);
                show_error_message(window, &message);
            }
        }
    }
}

unsafe fn finish_youtube_search(
    window: HWND,
    generation: u64,
    items: Vec<apricot_core::MediaItem>,
    continuation: Option<String>,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let work_kind = state
        .pending_youtube_work
        .as_ref()
        .map(|work| work.work_kind);
    let outcome = state
        .application
        .apply_search_results(generation, items, continuation);
    state.pending_youtube_work = None;
    stop_youtube_timer(window);
    let _ = EnableWindow(state.search, true);
    match (work_kind, outcome) {
        (Some(SearchWorkKind::Initial), SearchApplyOutcome::Replaced) => {
            state
                .application
                .navigate_to(RouteFrame::new(Route::Results));
            state.view = MainView::Results;
            refresh_results(state, true);
            layout_controls(window);
        }
        (Some(SearchWorkKind::More), SearchApplyOutcome::Appended { added }) => {
            append_results(state, added);
        }
        _ => {}
    }
}

unsafe fn finish_youtube_error(window: HWND, generation: u64, message: &str) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.pending_youtube_resolve == Some(generation) {
        state.pending_youtube_resolve = None;
        stop_youtube_timer(window);
        set_status(state, message, true);
        show_error_message(window, message);
        let _ = SetFocus(Some(state.list));
        return;
    }
    finish_youtube_error_state(window, state, generation, message);
}

unsafe fn finish_youtube_error_state(
    window: HWND,
    state: &mut WindowState,
    generation: u64,
    message: &str,
) {
    let was_initial = state
        .pending_youtube_work
        .as_ref()
        .is_none_or(|work| work.work_kind == SearchWorkKind::Initial);
    let _ = state.application.fail_search(generation, message);
    state.pending_youtube_work = None;
    stop_youtube_timer(window);
    let _ = EnableWindow(state.search, true);
    set_status(state, message, true);
    if was_initial {
        let text = wide(message);
        let _ = MessageBoxW(
            Some(window),
            PCWSTR(text.as_ptr()),
            w!("ApricotPlayer 2 Beta"),
            MB_OK | MB_ICONINFORMATION,
        );
        let _ = SetFocus(Some(state.search_edit));
    }
}

unsafe fn stop_youtube_timer(window: HWND) {
    let _ = KillTimer(Some(window), YOUTUBE_TIMER_ID);
}

unsafe fn stop_playback_timer(window: HWND) {
    let _ = KillTimer(Some(window), PLAYBACK_TIMER_ID);
}

unsafe fn cancel_youtube_work(window: HWND, state: &mut WindowState) {
    if state.pending_youtube_work.is_none() && state.pending_youtube_resolve.is_none() {
        return;
    }
    if state.pending_youtube_work.is_some() {
        let _ = state.application.cancel_pending_search();
    }
    state.pending_youtube_work = None;
    state.pending_youtube_resolve = None;
    let _ = state.youtube_search.cancel();
    let _ = EnableWindow(state.search, true);
    stop_youtube_timer(window);
}

unsafe fn result_selection_changed(window: HWND) {
    let work = {
        let Some(state) = state_mut(window) else {
            return;
        };
        if state.view != MainView::Results {
            return;
        }
        let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
        let Ok(index) = usize::try_from(selected) else {
            return;
        };
        if !state.application.select_search_result(index) {
            return;
        }
        if index + 1 == state.application.search_session().items().len() {
            state.application.request_more_search_results()
        } else {
            None
        }
    };
    if let Some(work) = work {
        if let Some(state) = state_mut(window) {
            let message = catalog_text(&state.application, "loading_more_results");
            set_status(state, &message, true);
        }
        start_youtube_work(window, work);
    }
}

unsafe fn refresh_results(state: &mut WindowState, focus: bool) {
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let accessible_name = wide(catalog.text("result_list"));
    let _ = SetWindowTextW(state.list, PCWSTR(accessible_name.as_ptr()));
    let items = state.application.search_session().items();
    if items.is_empty() {
        add_list_string(state.list, catalog.text("no_results"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("no_results"), true);
    } else {
        for item in items {
            add_list_string(state.list, &result_label(item, &catalog));
        }
        let selected = state
            .application
            .search_session()
            .selected_index()
            .min(items.len() - 1);
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
        let found = catalog
            .text("found")
            .replace("{count}", &items.len().to_string());
        set_status(state, &found, true);
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

unsafe fn append_results(state: &mut WindowState, added: usize) {
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let items = state.application.search_session().items();
    if added == 0 {
        set_status(state, catalog.text("no_more_results"), true);
        return;
    }
    let first_new = items.len().saturating_sub(added);
    for item in &items[first_new..] {
        add_list_string(state.list, &result_label(item, &catalog));
    }
    let selected = state
        .application
        .search_session()
        .selected_index()
        .min(items.len() - 1);
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    let loaded = catalog
        .text("search_more_loaded")
        .replace("{count}", &items.len().to_string());
    set_status(state, &loaded, true);
}

fn result_label(
    item: &apricot_core::MediaItem,
    catalog: &apricot_core::TranslationCatalog,
) -> String {
    let kind = match item.kind {
        apricot_core::MediaKind::Playlist => catalog.text("playlist"),
        apricot_core::MediaKind::Channel => catalog.text("channel"),
        apricot_core::MediaKind::LiveStream => catalog.text("live_stream"),
        _ => catalog.text("video"),
    };
    if matches!(
        item.kind,
        apricot_core::MediaKind::Playlist | apricot_core::MediaKind::Channel
    ) {
        return format!("{} | {kind}", item.title);
    }
    let mut parts = vec![item.title.clone()];
    if !item.channel.is_empty() {
        parts.push(format!("{}: {}", catalog.text("channel"), item.channel));
    }
    if let Some(views) = item.metadata.get("views") {
        let views = views
            .as_str()
            .map_or_else(|| views.to_string(), ToOwned::to_owned);
        parts.push(format!("{}: {views}", catalog.text("views")));
    }
    if let Some(duration) = item.duration_seconds {
        parts.push(format_duration(duration));
    }
    parts.push(kind.to_owned());
    parts.join(" | ")
}

fn format_duration(seconds: f64) -> String {
    let seconds = std::time::Duration::try_from_secs_f64(seconds.max(0.0))
        .map_or(0, |duration| duration.as_secs());
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn selected_search_kind(control: HWND) -> YoutubeSearchKind {
    let selected = unsafe { SendMessageW(control, CB_GETCURSEL, None, None).0 };
    match selected {
        1 => YoutubeSearchKind::Video,
        2 => YoutubeSearchKind::Playlist,
        3 => YoutubeSearchKind::Channel,
        _ => YoutubeSearchKind::All,
    }
}

fn youtube_session_config(state: &WindowState) -> YoutubeSessionConfig {
    let settings = state.application.settings();
    YoutubeSessionConfig {
        cookies_header: None,
        cookies_file: nonempty(&settings.cookies_file),
        proxy_url: nonempty(&settings.proxy),
    }
}

fn youtube_stream_preference(value: &str) -> YoutubeStreamPreference {
    match value.trim().to_ascii_lowercase().as_str() {
        "video" => YoutubeStreamPreference::PreferVideo,
        "audio" => YoutubeStreamPreference::PreferAudio,
        _ => YoutubeStreamPreference::Automatic,
    }
}

fn application_directory() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
}

fn playback_launch_options(state: &WindowState) -> Option<MpvLaunchOptions> {
    let directory = application_directory()?;
    let executable = directory.join("mpv").join("mpv.exe");
    let library = directory.join("mpv").join("libmpv-2.dll");
    if !executable.is_file() || !library.is_file() {
        return None;
    }
    let settings = state.application.settings();
    let session = state.application.player_session();
    let audio = session.audio()?;
    let boosted = session
        .enabled_toggles()
        .contains(&SessionToggle::VolumeBoost)
        || audio.volume > 100.0;
    let mut options = MpvLaunchOptions::new(executable);
    options.library = Some(library);
    options.video_mode = MpvVideoMode::Embedded(state.video_host.0 as isize);
    options.initial_volume = audio.volume;
    options.volume_max = if boosted { 300 } else { 100 };
    options.initial_speed = audio.speed;
    options.initial_pitch = audio.pitch;
    options.initial_playback_state = if session.phase() == PlaybackPhase::Paused {
        InitialPlaybackState::Paused
    } else {
        InitialPlaybackState::Playing
    };
    options.repeat_mode = if session.enabled_toggles().contains(&SessionToggle::Repeat) {
        RepeatMode::One
    } else {
        RepeatMode::Off
    };
    options.gapless = settings.gapless_playback;
    options.replay_gain.clone_from(&settings.replaygain_mode);
    options.audio_device = nonempty(&audio.output_device);
    options.cache = settings.enable_stream_cache.then(|| MpvCacheConfig {
        megabytes: u32::try_from(settings.cache_size_mb.clamp(128, 4_096)).unwrap_or(512),
    });
    options.initial_audio_filter = player_equalizer_filter(state, None);
    Some(options)
}

fn player_equalizer_filter(
    state: &WindowState,
    bass_boost_override: Option<bool>,
) -> Option<String> {
    let audio = state.application.player_session().audio()?;
    let bass_boost = bass_boost_override.unwrap_or_else(|| {
        state
            .application
            .player_session()
            .enabled_toggles()
            .contains(&SessionToggle::BassBoost)
    });
    apricot_playback::build_equalizer_filter(apricot_playback::EqualizerFilterConfig {
        gains: &audio.equalizer.gains,
        equalizer_enabled: audio.equalizer.enabled,
        bass_boost,
        clipping_protection: state.application.settings().equalizer_clipping_protection,
    })
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn catalog_text(application: &Application, key: &str) -> String {
    apricot_app::embedded_catalog(&application.settings().language)
        .text(key)
        .to_owned()
}

unsafe fn set_status(state: &WindowState, message: &str, announce: bool) {
    let text = wide(message);
    let _ = SetWindowTextW(state.status, PCWSTR(text.as_ptr()));
    if announce {
        state.announcer.announce(message, false);
    }
}

unsafe fn show_error_message(window: HWND, message: &str) {
    let message = wide(message);
    let _ = MessageBoxW(
        Some(window),
        PCWSTR(message.as_ptr()),
        w!("ApricotPlayer 2 Beta"),
        MB_OK | MB_ICONINFORMATION,
    );
}

unsafe fn add_list_string(control: HWND, value: &str) {
    let value = wide(value);
    SendMessageW(
        control,
        LB_ADDSTRING,
        None,
        Some(LPARAM(value.as_ptr() as isize)),
    );
}

unsafe fn window_text(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    let mut value = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let copied = GetWindowTextW(control, &mut value);
    String::from_utf16_lossy(&value[..usize::try_from(copied).unwrap_or_default()])
}

unsafe fn handle_shortcut_message(window: HWND, message: &MSG) -> bool {
    let Some(chord) = crate::shortcut_win32::chord_from_message(message) else {
        return false;
    };
    if !chord.control
        && !chord.shift
        && !chord.alt
        && chord.key == ShortcutKey::Escape
        && state(window).is_some_and(|state| state.view != MainView::MainMenu)
    {
        navigate_back(window);
        return true;
    }
    let Some(state) = state(window) else {
        return false;
    };
    let (scope, accepts_text) = match state.view {
        MainView::Search => (ActionScope::Dialog, true),
        MainView::MainMenu | MainView::Results => (ActionScope::List, false),
        MainView::Player => (ActionScope::Player, false),
    };
    let Some(action) = action_for_shortcut(
        &state.application.settings().keyboard_shortcuts,
        chord,
        ShortcutContext::new(scope, accepts_text),
    ) else {
        return false;
    };
    let is_global = action.scopes.contains(&ActionScope::Global);
    if !is_global && action.id.as_str() != "open_selected" && state.view != MainView::Player {
        return false;
    }
    if crate::shortcut_win32::is_repeat(message) && action.repeat == RepeatPolicy::None {
        return true;
    }
    activate_action(window, action.id.as_str());
    true
}

unsafe fn activate_action(window: HWND, action_id: &str) {
    match action_id {
        "open_main_menu" => show_main_menu(window),
        "open_search" => show_search(window),
        "open_settings" => open_settings(window),
        "open_action_finder" => show_action_finder(window),
        "open_play_file" => open_media_file(window),
        "open_selected" => activate_selection(window),
        "background_play_pause" | "player_play_pause" => toggle_player_pause(window),
        "player_back" => navigate_back(window),
        "player_time" => announce_player_time(window),
        "player_volume_status" => announce_player_volume(window),
        "player_seek_back" => seek_player(window, -configured_seek_seconds(window)),
        "player_seek_forward" => seek_player(window, configured_seek_seconds(window)),
        "player_seek_back_large" => seek_player(window, -60.0),
        "player_seek_forward_large" => seek_player(window, 60.0),
        "player_seek_back_huge" => seek_player(window, -600.0),
        "player_seek_forward_huge" => seek_player(window, 600.0),
        "player_seek_start" => seek_player_absolute(window, 0.0),
        "player_seek_end" => seek_player_to_end(window),
        "player_volume_up" => adjust_player_volume(window, configured_volume_step(window)),
        "player_volume_down" => adjust_player_volume(window, -configured_volume_step(window)),
        "player_speed_up" => adjust_player_speed(window, configured_speed_step(window)),
        "player_speed_down" => adjust_player_speed(window, -configured_speed_step(window)),
        "player_pitch_up" => adjust_player_pitch(window, configured_pitch_step(window)),
        "player_pitch_down" => adjust_player_pitch(window, -configured_pitch_step(window)),
        "player_reset_speed_pitch" => reset_player_speed_pitch(window),
        "player_repeat" => toggle_player_session_setting(window, SessionToggle::Repeat),
        "player_bass_boost" => toggle_player_session_setting(window, SessionToggle::BassBoost),
        "player_volume_boost" => toggle_player_session_setting(window, SessionToggle::VolumeBoost),
        _ => show_unimplemented_action(window, action_id),
    }
}

unsafe fn execute_player_command(window: HWND, command: PlaybackCommand) -> bool {
    let Some(state) = state(window) else {
        return false;
    };
    let generation = state.application.player_session().generation();
    let Some(runtime) = state.playback.as_ref() else {
        return false;
    };
    match runtime.execute(generation, command) {
        Ok(()) => true,
        Err(error) => {
            show_error_message(window, &format!("Player command failed: {error}"));
            false
        }
    }
}

unsafe fn toggle_player_pause(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    if !state.application.player_session().is_open() {
        return;
    }
    let paused = state.application.player_session().phase() != PlaybackPhase::Paused;
    let _ = execute_player_command(window, PlaybackCommand::SetPaused(paused));
}

unsafe fn seek_player(window: HWND, seconds: f64) {
    let _ = execute_player_command(
        window,
        PlaybackCommand::SeekRelative {
            seconds,
            exact: false,
        },
    );
}

unsafe fn seek_player_absolute(window: HWND, seconds: f64) {
    let _ = execute_player_command(
        window,
        PlaybackCommand::SeekAbsolute {
            seconds,
            exact: false,
        },
    );
}

unsafe fn seek_player_to_end(window: HWND) {
    let Some(duration) =
        state(window).and_then(|state| state.application.player_session().duration_seconds())
    else {
        if let Some(state) = state(window) {
            set_status(state, "Timing is not available yet", true);
        }
        return;
    };
    seek_player_absolute(window, duration);
}

unsafe fn announce_player_time(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let session = state.application.player_session();
    let elapsed = format_duration(session.position_seconds());
    let message = session.duration_seconds().map_or_else(
        || format!("Elapsed {elapsed}"),
        |duration| {
            let remaining = format_duration((duration - session.position_seconds()).max(0.0));
            format!(
                "Elapsed {elapsed}, remaining {remaining}, total {}",
                format_duration(duration)
            )
        },
    );
    set_status(state, &message, true);
}

unsafe fn announce_player_volume(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let Some(audio) = state.application.player_session().audio() else {
        return;
    };
    set_status(state, &format!("Volume {:.0}", audio.volume), true);
}

unsafe fn adjust_player_volume(window: HWND, delta: f64) {
    let Some(state) = state(window) else {
        return;
    };
    let Some(audio) = state.application.player_session().audio() else {
        return;
    };
    let maximum = if state
        .application
        .player_session()
        .enabled_toggles()
        .contains(&SessionToggle::VolumeBoost)
    {
        300.0
    } else {
        100.0
    };
    let volume = (audio.volume + delta).clamp(0.0, maximum);
    if execute_player_command(window, PlaybackCommand::SetVolume(volume)) {
        let Some(state) = state_mut(window) else {
            return;
        };
        state.application.set_player_volume(volume);
        set_status(state, &format!("Volume {volume:.0}"), true);
    }
}

unsafe fn adjust_player_speed(window: HWND, delta: f64) {
    let Some(audio) = state(window).and_then(|state| state.application.player_session().audio())
    else {
        return;
    };
    let speed = (audio.speed + delta).clamp(0.25, 4.0);
    if execute_player_command(window, PlaybackCommand::SetSpeed(speed)) {
        let Some(state) = state_mut(window) else {
            return;
        };
        state.application.set_player_speed(speed);
        set_status(state, &format!("Speed {speed:.2}"), true);
    }
}

unsafe fn adjust_player_pitch(window: HWND, delta: f64) {
    let Some(audio) = state(window).and_then(|state| state.application.player_session().audio())
    else {
        return;
    };
    let pitch = (audio.pitch + delta).clamp(0.5, 2.0);
    if execute_player_command(window, PlaybackCommand::SetPitch(pitch)) {
        let Some(state) = state_mut(window) else {
            return;
        };
        state.application.set_player_pitch(pitch);
        set_status(state, &format!("Pitch {pitch:.2}"), true);
    }
}

unsafe fn reset_player_speed_pitch(window: HWND) {
    let speed = state(window).map_or(1.0, |state| {
        state
            .application
            .settings()
            .player_speed
            .parse::<f64>()
            .unwrap_or(1.0)
            .clamp(0.25, 4.0)
    });
    if !execute_player_command(window, PlaybackCommand::SetSpeed(speed))
        || !execute_player_command(window, PlaybackCommand::SetPitch(1.0))
    {
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    state.application.set_player_speed(speed);
    state.application.set_player_pitch(1.0);
    set_status(state, "Speed and pitch reset", true);
}

unsafe fn toggle_player_session_setting(window: HWND, toggle: SessionToggle) {
    let Some(state) = state(window) else {
        return;
    };
    let enabled = !state
        .application
        .player_session()
        .enabled_toggles()
        .contains(&toggle);
    let command_succeeded = match toggle {
        SessionToggle::Repeat => {
            execute_player_command(window, PlaybackCommand::SetRepeat(enabled))
        }
        SessionToggle::VolumeBoost => execute_player_command(
            window,
            PlaybackCommand::SetVolumeMax(if enabled { 300 } else { 100 }),
        ),
        SessionToggle::BassBoost => {
            let filter = player_equalizer_filter(state, Some(enabled));
            execute_player_command(window, PlaybackCommand::SetAudioFilter(filter))
        }
        SessionToggle::AutoplayNext | SessionToggle::Fullscreen | SessionToggle::Shuffle => true,
    };
    if !command_succeeded {
        return;
    }
    let clamped_volume = if toggle == SessionToggle::VolumeBoost && !enabled {
        let volume = state
            .application
            .player_session()
            .audio()
            .map_or(100.0, |audio| audio.volume.min(100.0));
        if !execute_player_command(window, PlaybackCommand::SetVolume(volume)) {
            return;
        }
        Some(volume)
    } else {
        None
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    state.application.set_player_toggle(toggle, enabled);
    if let Some(volume) = clamped_volume {
        state.application.set_player_volume(volume);
    }
    set_status(
        state,
        &format!(
            "{} {}",
            session_toggle_name(toggle),
            if enabled { "on" } else { "off" }
        ),
        true,
    );
    if state.view == MainView::Player {
        refresh_player(window, state, true, true);
    }
}

const fn session_toggle_name(toggle: SessionToggle) -> &'static str {
    match toggle {
        SessionToggle::AutoplayNext => "Autoplay next",
        SessionToggle::BassBoost => "Bass boost",
        SessionToggle::VolumeBoost => "Volume boost",
        SessionToggle::Repeat => "Repeat",
        SessionToggle::Shuffle => "Shuffle",
        SessionToggle::Fullscreen => "Fullscreen",
    }
}

unsafe fn configured_seek_seconds(window: HWND) -> f64 {
    state(window).map_or(5.0, |state| {
        state.application.settings().seek_seconds.clamp(0.1, 600.0)
    })
}

unsafe fn configured_volume_step(window: HWND) -> f64 {
    state(window).map_or(5.0, |state| {
        f64::from(
            i32::try_from(state.application.settings().volume_step.clamp(1, 100)).unwrap_or(5),
        )
    })
}

unsafe fn configured_speed_step(window: HWND) -> f64 {
    state(window).map_or(0.01, |state| {
        state.application.settings().speed_step.clamp(0.01, 1.0)
    })
}

unsafe fn configured_pitch_step(window: HWND) -> f64 {
    state(window).map_or(0.01, |state| {
        state.application.settings().pitch_step.clamp(0.01, 1.0)
    })
}

unsafe fn close_player_runtime(window: HWND, state: &mut WindowState) {
    let generation = state.application.player_session().generation();
    if let Some(runtime) = state.playback.as_ref() {
        let _ = runtime.close(generation);
    }
    state.application.close_player_session();
    stop_playback_timer(window);
    let title = wide("ApricotPlayer 2 Beta");
    let _ = SetWindowTextW(window, PCWSTR(title.as_ptr()));
}

unsafe fn play_local_file(window: HWND, path: &std::path::Path) {
    if !path.is_file() {
        show_error_message(window, "The selected media file does not exist");
        return;
    }
    let title = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map_or_else(|| path.display().to_string(), ToOwned::to_owned);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let kind = if matches!(
        extension.as_str(),
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "wmv"
    ) {
        apricot_core::MediaKind::Video
    } else {
        apricot_core::MediaKind::Audio
    };
    let path_text = path.to_string_lossy().into_owned();
    start_player(
        window,
        apricot_core::MediaItem {
            id: apricot_core::MediaId(path_text.clone()),
            source: apricot_core::MediaSource::Local,
            kind,
            title,
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: Some(path_text),
            channel: String::new(),
            duration_seconds: None,
            metadata: std::collections::BTreeMap::new(),
        },
    );
}

unsafe fn open_media_file(window: HWND) {
    let title = state(window).map_or_else(
        || "Play file".to_owned(),
        |state| {
            apricot_app::embedded_catalog(&state.application.settings().language)
                .text("play_file")
                .to_owned()
        },
    );
    match crate::file_dialog_win32::choose_media_file(window, &title) {
        Ok(Some(path)) => {
            if let Some(state) = state_mut(window) {
                state
                    .application
                    .enqueue_activation(ActivationRequest::OpenFile(path));
            }
            process_pending_activations(window);
        }
        Ok(None) => {}
        Err(error) => {
            let message = wide(&error);
            let _ = MessageBoxW(
                Some(window),
                PCWSTR(message.as_ptr()),
                w!("ApricotPlayer 2 Beta"),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }
    if let Some(state) = state(window) {
        let _ = SetFocus(Some(active_primary_control(state)));
    }
}

unsafe fn show_action_finder(window: HWND) {
    let Some(main_state) = state(window) else {
        return;
    };
    let model = main_state
        .application
        .action_finder_model(ActionFinderContext::default());
    match crate::action_finder_win32::show(window, model) {
        Ok(Some(action_id)) => activate_action(window, action_id),
        Ok(None) => {}
        Err(error) => {
            let message = wide(&error.to_string());
            let _ = MessageBoxW(
                Some(window),
                PCWSTR(message.as_ptr()),
                w!("ApricotPlayer 2 Beta"),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }
    if let Some(state) = state(window) {
        let _ = SetFocus(Some(active_primary_control(state)));
    }
}

unsafe fn show_unimplemented_action(window: HWND, action_id: &str) {
    let Some(state) = state(window) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let label_key =
        apricot_core::action::action_by_id(action_id).map_or(action_id, |action| action.label_key);
    let message = wide(&format!(
        "{} is registered, but its Rust route is not implemented in this internal build yet.",
        catalog.text(label_key)
    ));
    let _ = MessageBoxW(
        Some(window),
        PCWSTR(message.as_ptr()),
        w!("ApricotPlayer 2 Beta"),
        MB_OK | MB_ICONINFORMATION,
    );
}

unsafe fn open_settings(window: HWND) {
    let settings_result = {
        let Some(state) = state_mut(window) else {
            return;
        };
        if state.settings_open {
            return;
        }
        state.settings_open = true;
        crate::settings_win32::show(window, &mut state.application)
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    state.settings_open = false;
    match state.view {
        MainView::MainMenu => refresh_main_menu(state),
        MainView::Results => refresh_results(state, false),
        MainView::Search => {}
        MainView::Player => refresh_player(window, state, false, true),
    }
    layout_controls(window);
    let _ = SetFocus(Some(active_primary_control(state)));
    process_pending_activations(window);
    match settings_result {
        Ok(Some(action_id)) => activate_action(window, action_id),
        Ok(None) => {}
        Err(error) => {
            let message = wide(&error.to_string());
            let _ = MessageBoxW(
                Some(window),
                PCWSTR(message.as_ptr()),
                w!("ApricotPlayer 2 Beta"),
                MB_OK | MB_ICONINFORMATION,
            );
        }
    }
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
                play_local_file(window, &path);
            }
        }
    }
}

unsafe fn refresh_main_menu(state: &mut WindowState) {
    state.model = state.application.main_menu_model();
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let accessible_name = wide(&state.model.accessible_name);
    let _ = SetWindowTextW(state.list, PCWSTR(accessible_name.as_ptr()));
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

const fn active_primary_control(state: &WindowState) -> HWND {
    match state.view {
        MainView::Search => state.search_edit,
        MainView::MainMenu | MainView::Results | MainView::Player => state.list,
    }
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
