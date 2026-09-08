//! Isolated unsafe Win32 boundary for the production Windows shell.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{
    ffi::c_void,
    mem::size_of,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        mpsc::{self, Receiver, TryRecvError},
    },
};

use crate::{
    bookmark_dialog_win32::{
        BookmarkDialogEntry, BookmarkDialogLabels, BookmarkDialogRequest, BookmarkDialogResponse,
    },
    player_controls_win32::{PlayerControlActivation, PlayerControls},
};
use apricot_app::{
    ActionFinderContext, ActivationRequest, Application, MainMenuModel, PlaybackPhase,
    PlayerNavigationOutcome, SearchApplyOutcome, SearchWork, SearchWorkKind, SessionToggle,
    YoutubeSearchKind,
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
use apricot_platform::{
    YoutubeSearchService, YoutubeSearchServiceUpdate, scan_local_media_folder_with_cancel,
};
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
            Input::KeyboardAndMouse::{
                EnableWindow, GetAsyncKeyState, GetFocus, SetFocus, VK_CONTROL, VK_MENU, VK_RETURN,
                VK_SHIFT,
            },
            Shell::{
                DefSubclassProc, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
                NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
                RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, BS_DEFPUSHBUTTON, CBS_DROPDOWNLIST, CW_USEDEFAULT, CreatePopupMenu,
                CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
                ES_AUTOHSCROLL, GetClientRect, GetCursorPos, GetMessageW, GetParent,
                GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, HMENU,
                IDC_ARROW, IDI_APPLICATION, IsDialogMessageW, KillTimer, LB_ADDSTRING, LB_GETCOUNT,
                LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, LBN_DBLCLK, LBN_SELCHANGE, LBS_NOTIFY,
                LoadCursorW, LoadIconW, MB_ICONINFORMATION, MB_OK, MF_GRAYED, MF_STRING, MSG,
                MessageBoxW, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW,
                RegisterWindowMessageW, SW_HIDE, SW_SHOW, SendMessageW, SetForegroundWindow,
                SetTimer, SetWindowLongPtrW, SetWindowTextW, ShowWindow, TPM_LEFTALIGN,
                TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU,
                WM_COPYDATA, WM_CREATE, WM_DESTROY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK,
                WM_NCDESTROY, WM_RBUTTONUP, WM_SETFONT, WM_SIZE, WM_SYSKEYUP, WM_TIMER, WNDCLASSW,
                WS_CHILD, WS_EX_CLIENTEDGE, WS_GROUP, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
                WS_VSCROLL,
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
const ID_PLAY_FOLDER: usize = 1007;
const ID_SHUFFLE_FOLDER: usize = 1008;
const ID_ADD_FOLDER_TO_QUEUE: usize = 1009;
const ID_FOLDER_PLAYBACK_QUEUE: usize = 1010;
const ID_DIRECT_PLAY: usize = 1011;
const ID_DIRECT_DOWNLOAD_AUDIO: usize = 1012;
const ID_DIRECT_DOWNLOAD_VIDEO: usize = 1013;
const ID_DIRECT_COPY_STREAM: usize = 1014;
const ID_DIRECT_ENTER: usize = 1015;
const ID_COLLECTION_REMOVE: usize = 1016;
const ID_HISTORY_CLEAR: usize = 1017;
const ID_PLAYLIST_CREATE: usize = 1018;
const ID_PLAYLIST_PLAY_ALL: usize = 1019;
const ID_PLAYLIST_SHUFFLE: usize = 1020;
const ID_PLAYLIST_ADD_ALL_TO_QUEUE: usize = 1021;
const ID_NOTIFICATION_CLEAR: usize = 1022;
const ID_CONTEXT_PLAY: usize = 1101;
const ID_CONTEXT_PLAY_FOLDER: usize = 1102;
const ID_CONTEXT_SHUFFLE_FOLDER: usize = 1103;
const ID_CONTEXT_ADD_TO_QUEUE: usize = 1104;
const ID_CONTEXT_REMOVE_FROM_QUEUE: usize = 1105;
const ID_CONTEXT_ADD_FOLDER_TO_QUEUE: usize = 1106;
const ID_CONTEXT_PLAYBACK_QUEUE: usize = 1107;
const ID_CONTEXT_COPY_LOCATION: usize = 1108;
const ID_CONTEXT_COPY_STREAM_URL: usize = 1109;
const ID_CONTEXT_COPY_TIMESTAMP: usize = 1110;
const ID_CONTEXT_CLOSE_PLAYER: usize = 1111;
const ID_CONTEXT_DETAILS: usize = 1112;
const ID_CONTEXT_ADD_FAVORITE: usize = 1113;
const ID_CONTEXT_COLLECTION_REMOVE: usize = 1114;
const ID_CONTEXT_HISTORY_CLEAR: usize = 1115;
const ID_CONTEXT_CREATE_PLAYLIST: usize = 1116;
const ID_CONTEXT_PLAY_PLAYLIST: usize = 1117;
const ID_CONTEXT_SHUFFLE_PLAYLIST: usize = 1118;
const ID_CONTEXT_ADD_TO_PLAYLIST: usize = 1119;
const ID_CONTEXT_REMOVE_FROM_PLAYLIST: usize = 1120;
const ID_CONTEXT_REMOVE_PLAYLIST: usize = 1121;
const ID_CONTEXT_ADD_PLAYLIST_TO_QUEUE: usize = 1122;
const ID_CONTEXT_CLEAR_NOTIFICATIONS: usize = 1123;
const WM_PROCESS_ACTIVATION: u32 = WM_APP + 1;
const WM_TRAY_ICON: u32 = WM_APP + 2;
const YOUTUBE_TIMER_ID: usize = 1;
const YOUTUBE_TIMER_INTERVAL_MS: u32 = 25;
const PLAYBACK_TIMER_ID: usize = 2;
const PLAYBACK_TIMER_INTERVAL_MS: u32 = 25;
const CONTROLLED_REPEAT_TIMER_ID: usize = 3;
const LOCAL_FOLDER_TIMER_ID: usize = 4;
const SEEK_HOLD_DELAY_MS: u32 = 180;
const SEEK_HOLD_INTERVAL_MS: u32 = 110;
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
    DirectLink,
    Results,
    LocalFolder,
    Favorites,
    History,
    NotificationCenter,
    UserPlaylists,
    UserPlaylistItems,
    Player,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QueueStartMode {
    Front,
    Matching,
}

struct PendingQueuedStart {
    item: apricot_core::MediaItem,
    mode: QueueStartMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum YoutubeResolvePurpose {
    Playback,
    CopyStreamUrl,
}

#[derive(Clone, Debug, PartialEq)]
struct PendingYoutubeResolve {
    token: u64,
    purpose: YoutubeResolvePurpose,
    original_item: apricot_core::MediaItem,
    session_shuffle: Option<bool>,
    preserve_sequence: bool,
    start_position_seconds: Option<f64>,
}

#[derive(Clone, Copy)]
struct ControlledRepeatState {
    action_id: &'static str,
    virtual_key: usize,
    chord: apricot_core::shortcut::ShortcutChord,
}

struct PendingLocalFolderScan {
    generation: u64,
    path: PathBuf,
    cancelled: Arc<AtomicBool>,
    receiver: Receiver<std::result::Result<Vec<apricot_core::MediaItem>, String>>,
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
    play_folder: HWND,
    shuffle_folder: HWND,
    add_folder_to_queue: HWND,
    folder_playback_queue: HWND,
    direct_play: HWND,
    direct_download_audio: HWND,
    direct_download_video: HWND,
    direct_copy_stream: HWND,
    collection_remove: HWND,
    history_clear: HWND,
    notification_clear: HWND,
    playlist_create: HWND,
    playlist_play_all: HWND,
    playlist_shuffle: HWND,
    playlist_add_all_to_queue: HWND,
    video_host: HWND,
    player_controls: PlayerControls,
    status: HWND,
    announcer: crate::announcement_win32::WindowsAnnouncer,
    model: MainMenuModel,
    application: Application,
    settings_open: bool,
    modal_open: bool,
    tray_icon_added: bool,
    lifecycle: WindowLifecycle,
    taskbar_created_message: u32,
    view: MainView,
    youtube_search: YoutubeSearchService,
    pending_youtube_work: Option<SearchWork>,
    pending_youtube_resolve: Option<PendingYoutubeResolve>,
    pending_player_navigation: Option<i32>,
    pending_queued_start: Option<PendingQueuedStart>,
    next_youtube_resolve_token: u64,
    playback: Option<PlaybackRuntime>,
    controlled_repeat: Option<ControlledRepeatState>,
    pending_local_folder_scan: Option<PendingLocalFolderScan>,
    next_local_folder_generation: u64,
    current_user_playlist_index: usize,
    current_user_playlist_item_index: usize,
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
    crate::playback_queue_win32::register()?;
    crate::playlist_dialog_win32::register()?;
    crate::bookmark_dialog_win32::register()?;
    crate::details_win32::register()?;

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
        handle_controlled_repeat_release(window, &message);
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
            handle_window_command(window, wparam);
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
                if !state.modal_open {
                    restore_from_tray(window);
                }
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
        WM_TIMER if wparam.0 == CONTROLLED_REPEAT_TIMER_ID => {
            tick_controlled_repeat(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == LOCAL_FOLDER_TIMER_ID => {
            poll_local_folder_scan(window);
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

unsafe fn handle_window_command(window: HWND, wparam: WPARAM) {
    let command = wparam.0 & 0xffff;
    let notification = (wparam.0 >> 16) & 0xffff;
    let player_activation = state(window).and_then(|state| {
        (state.view == MainView::Player)
            .then(|| state.player_controls.activation_for_command(command))
            .flatten()
    });
    if let Some(activation) = player_activation {
        activate_player_control(window, activation);
    } else if command == ID_OPEN
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
    } else if command == ID_DIRECT_ENTER {
        submit_primary_text(window);
    } else if command == ID_BACK {
        navigate_back(window);
    } else if command == ID_PLAY_FOLDER {
        play_current_local_folder(window, false);
    } else if command == ID_SHUFFLE_FOLDER {
        play_current_local_folder(window, true);
    } else if command == ID_ADD_FOLDER_TO_QUEUE {
        add_current_local_folder_to_queue(window);
    } else if command == ID_FOLDER_PLAYBACK_QUEUE {
        show_playback_queue(window);
    } else if command == ID_DIRECT_PLAY {
        activate_direct_link(window, "play");
    } else if command == ID_DIRECT_DOWNLOAD_AUDIO {
        activate_direct_link(window, "download_audio");
    } else if command == ID_DIRECT_DOWNLOAD_VIDEO {
        activate_direct_link(window, "download_video");
    } else if command == ID_DIRECT_COPY_STREAM {
        activate_direct_link(window, "copy_stream_url");
    } else if command == ID_COLLECTION_REMOVE {
        remove_selected_collection_item(window);
    } else if command == ID_HISTORY_CLEAR {
        clear_history(window);
    } else if command == ID_NOTIFICATION_CLEAR {
        clear_notifications(window);
    } else if command == ID_PLAYLIST_CREATE {
        create_user_playlist(window, None);
    } else if command == ID_PLAYLIST_PLAY_ALL {
        play_current_user_playlist(window, false);
    } else if command == ID_PLAYLIST_SHUFFLE {
        play_current_user_playlist(window, true);
    } else if command == ID_PLAYLIST_ADD_ALL_TO_QUEUE {
        add_current_user_playlist_to_queue(window);
    } else if matches!(
        command,
        ID_TRAY_SHOW | ID_TRAY_SETTINGS | ID_TRAY_CHECK_SUBSCRIPTIONS | ID_TRAY_EXIT
    ) {
        handle_tray_command(window, command);
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
    if !SetWindowSubclass(search_edit, Some(text_entry_proc), 1, 0).as_bool() {
        return Err(windows::core::Error::from_thread());
    }
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
    let play_folder_text = wide(catalog.text("play_folder"));
    let play_folder = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(play_folder_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_PLAY_FOLDER,
    )?;
    let shuffle_folder_text = wide(catalog.text("shuffle_folder"));
    let shuffle_folder = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(shuffle_folder_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_SHUFFLE_FOLDER,
    )?;
    let add_folder_to_queue_text = wide(catalog.text("add_folder_to_queue"));
    let add_folder_to_queue = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(add_folder_to_queue_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_ADD_FOLDER_TO_QUEUE,
    )?;
    let playback_queue_text = wide(catalog.text("playback_queue"));
    let folder_playback_queue = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(playback_queue_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_FOLDER_PLAYBACK_QUEUE,
    )?;
    let direct_play_text = wide(catalog.text("play_direct_link"));
    let direct_play = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(direct_play_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_DIRECT_PLAY,
    )?;
    let direct_audio_text = wide(catalog.text("download_direct_audio"));
    let direct_download_audio = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(direct_audio_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_DIRECT_DOWNLOAD_AUDIO,
    )?;
    let direct_video_text = wide(catalog.text("download_direct_video"));
    let direct_download_video = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(direct_video_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_DIRECT_DOWNLOAD_VIDEO,
    )?;
    let direct_stream_text = wide(catalog.text("copy_stream_url"));
    let direct_copy_stream = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(direct_stream_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_DIRECT_COPY_STREAM,
    )?;
    let collection_remove_text = wide(catalog.text("remove"));
    let collection_remove = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(collection_remove_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_COLLECTION_REMOVE,
    )?;
    let history_clear_text = wide(catalog.text("clear_history"));
    let history_clear = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(history_clear_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_HISTORY_CLEAR,
    )?;
    let notification_clear_text = wide(catalog.text("clear_notifications"));
    let notification_clear = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(notification_clear_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_NOTIFICATION_CLEAR,
    )?;
    let playlist_create_text = wide(catalog.text("create_playlist"));
    let playlist_create = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(playlist_create_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_PLAYLIST_CREATE,
    )?;
    let playlist_play_all_text = wide(catalog.text("play_playlist"));
    let playlist_play_all = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(playlist_play_all_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_PLAYLIST_PLAY_ALL,
    )?;
    let playlist_shuffle_text = wide(catalog.text("shuffle_playlist"));
    let playlist_shuffle = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(playlist_shuffle_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_PLAYLIST_SHUFFLE,
    )?;
    let playlist_queue_text = wide(catalog.text("add_to_playback_queue"));
    let playlist_add_all_to_queue = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(playlist_queue_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_PLAYLIST_ADD_ALL_TO_QUEUE,
    )?;
    let player_controls = PlayerControls::create(parent, instance)?;
    let video_host = player_controls.video_host();
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
        play_folder,
        shuffle_folder,
        add_folder_to_queue,
        folder_playback_queue,
        direct_play,
        direct_download_audio,
        direct_download_video,
        direct_copy_stream,
        collection_remove,
        history_clear,
        notification_clear,
        playlist_create,
        playlist_play_all,
        playlist_shuffle,
        playlist_add_all_to_queue,
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
        play_folder,
        shuffle_folder,
        add_folder_to_queue,
        folder_playback_queue,
        direct_play,
        direct_download_audio,
        direct_download_video,
        direct_copy_stream,
        collection_remove,
        history_clear,
        notification_clear,
        playlist_create,
        playlist_play_all,
        playlist_shuffle,
        playlist_add_all_to_queue,
        video_host,
        player_controls,
        status,
        announcer: crate::announcement_win32::WindowsAnnouncer::new(status),
        model,
        application,
        settings_open: false,
        modal_open: false,
        tray_icon_added: false,
        lifecycle: WindowLifecycle::Visible,
        taskbar_created_message: RegisterWindowMessageW(w!("TaskbarCreated")),
        view: MainView::MainMenu,
        youtube_search: YoutubeSearchService::default(),
        pending_youtube_work: None,
        pending_youtube_resolve: None,
        pending_player_navigation: None,
        pending_queued_start: None,
        next_youtube_resolve_token: 0,
        playback: None,
        controlled_repeat: None,
        pending_local_folder_scan: None,
        next_local_folder_generation: 0,
        current_user_playlist_index: 0,
        current_user_playlist_item_index: 0,
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
    if message == WM_CONTEXTMENU
        && let Ok(parent) = windows::Win32::UI::WindowsAndMessaging::GetParent(window)
    {
        show_list_context_menu(parent, lparam);
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(menu_list_proc), subclass_id);
    }
    DefSubclassProc(window, message, wparam, lparam)
}

unsafe extern "system" fn text_entry_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    if message == WM_KEYDOWN
        && wparam.0 == usize::from(VK_RETURN.0)
        && let Ok(parent) = GetParent(window)
    {
        SendMessageW(parent, WM_COMMAND, Some(WPARAM(ID_DIRECT_ENTER)), None);
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(text_entry_proc), subclass_id);
    }
    DefSubclassProc(window, message, wparam, lparam)
}

unsafe fn show_list_context_menu(window: HWND, location: LPARAM) {
    let Some((view, language, list)) = state(window).map(|main_state| {
        (
            main_state.view,
            main_state.application.settings().language.clone(),
            main_state.list,
        )
    }) else {
        return;
    };
    let Some(entries) = list_context_entries(view) else {
        return;
    };
    let active_item = active_media_item(window);
    let active_is_local = active_item
        .as_ref()
        .is_some_and(apricot_core::MediaItem::is_local_media);
    let catalog = apricot_app::embedded_catalog(&language);
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    if view == MainView::UserPlaylistItems && active_item.is_none() {
        let label = wide(catalog.text("playlist_empty"));
        let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(label.as_ptr()));
    } else {
        for (id, key) in entries {
            if *id == ID_CONTEXT_COPY_STREAM_URL && active_is_local {
                continue;
            }
            let key = if *id == ID_CONTEXT_COPY_LOCATION && active_is_local {
                "copy_path"
            } else {
                key
            };
            let label = wide(catalog.text(key));
            let _ = AppendMenuW(menu, MF_STRING, *id, PCWSTR(label.as_ptr()));
        }
    }
    let mut fallback_point = POINT::default();
    let point = context_menu_point(location, list).or_else(|| {
        GetCursorPos(&raw mut fallback_point)
            .is_ok()
            .then_some(fallback_point)
    });
    if let Some(point) = point {
        if let Some(state) = state_mut(window) {
            state.modal_open = true;
        }
        let selected = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            None,
            window,
            None,
        );
        if let Some(state) = state_mut(window) {
            state.modal_open = false;
        }
        resume_deferred_window_work(window);
        match usize::try_from(selected.0).unwrap_or_default() {
            ID_CONTEXT_PLAY => activate_selection(window),
            ID_CONTEXT_PLAY_FOLDER => play_current_local_folder(window, false),
            ID_CONTEXT_SHUFFLE_FOLDER => play_current_local_folder(window, true),
            ID_CONTEXT_ADD_TO_QUEUE => add_active_item_to_playback_queue(window),
            ID_CONTEXT_REMOVE_FROM_QUEUE => remove_active_item_from_playback_queue(window),
            ID_CONTEXT_ADD_FOLDER_TO_QUEUE => add_current_local_folder_to_queue(window),
            ID_CONTEXT_PLAYBACK_QUEUE => show_playback_queue(window),
            ID_CONTEXT_COPY_LOCATION => copy_active_location(window),
            ID_CONTEXT_COPY_STREAM_URL => copy_active_stream_url(window),
            ID_CONTEXT_ADD_FAVORITE => add_active_favorite(window),
            ID_CONTEXT_COLLECTION_REMOVE => remove_selected_collection_item(window),
            ID_CONTEXT_HISTORY_CLEAR => clear_history(window),
            ID_CONTEXT_CREATE_PLAYLIST => create_user_playlist(window, None),
            ID_CONTEXT_PLAY_PLAYLIST => play_current_user_playlist(window, false),
            ID_CONTEXT_SHUFFLE_PLAYLIST => play_current_user_playlist(window, true),
            ID_CONTEXT_ADD_TO_PLAYLIST => add_active_item_to_user_playlist(window),
            ID_CONTEXT_REMOVE_FROM_PLAYLIST => remove_active_item_from_user_playlist(window),
            ID_CONTEXT_REMOVE_PLAYLIST => remove_selected_user_playlist(window),
            ID_CONTEXT_ADD_PLAYLIST_TO_QUEUE => add_current_user_playlist_to_queue(window),
            ID_CONTEXT_CLEAR_NOTIFICATIONS => clear_notifications(window),
            _ => {}
        }
    }
    let _ = DestroyMenu(menu);
    if let Some(state) = state(window) {
        let _ = SetFocus(Some(active_primary_control(state)));
    }
}

fn list_context_entries(view: MainView) -> Option<&'static [(usize, &'static str)]> {
    match view {
        MainView::LocalFolder => Some(&[
            (ID_CONTEXT_PLAY, "play"),
            (ID_CONTEXT_PLAY_FOLDER, "play_folder"),
            (ID_CONTEXT_SHUFFLE_FOLDER, "shuffle_folder"),
            (ID_CONTEXT_ADD_TO_QUEUE, "add_to_playback_queue"),
            (ID_CONTEXT_ADD_FOLDER_TO_QUEUE, "add_folder_to_queue"),
            (ID_CONTEXT_PLAYBACK_QUEUE, "playback_queue"),
            (ID_CONTEXT_COPY_LOCATION, "copy_path"),
            (ID_CONTEXT_ADD_TO_PLAYLIST, "add_to_playlist"),
        ]),
        MainView::Favorites => Some(&[
            (ID_CONTEXT_PLAY, "play"),
            (ID_CONTEXT_ADD_TO_QUEUE, "add_to_playback_queue"),
            (ID_CONTEXT_COPY_LOCATION, "copy_link"),
            (ID_CONTEXT_COLLECTION_REMOVE, "remove_favorite"),
            (ID_CONTEXT_ADD_TO_PLAYLIST, "add_to_playlist"),
        ]),
        MainView::History => Some(&[
            (ID_CONTEXT_PLAY, "play"),
            (ID_CONTEXT_ADD_FAVORITE, "add_favorite"),
            (ID_CONTEXT_ADD_TO_QUEUE, "add_to_playback_queue"),
            (ID_CONTEXT_COPY_LOCATION, "copy_link"),
            (ID_CONTEXT_COLLECTION_REMOVE, "remove_history_item"),
            (ID_CONTEXT_HISTORY_CLEAR, "clear_history"),
            (ID_CONTEXT_ADD_TO_PLAYLIST, "add_to_playlist"),
        ]),
        MainView::NotificationCenter => Some(&[
            (ID_CONTEXT_PLAY, "play"),
            (ID_CONTEXT_COPY_LOCATION, "copy_url"),
            (ID_CONTEXT_CLEAR_NOTIFICATIONS, "clear_notifications"),
        ]),
        MainView::Results => Some(&[
            (ID_CONTEXT_PLAY, "play"),
            (ID_CONTEXT_ADD_TO_QUEUE, "add_to_playback_queue"),
            (ID_CONTEXT_REMOVE_FROM_QUEUE, "remove_from_playback_queue"),
            (ID_CONTEXT_PLAYBACK_QUEUE, "playback_queue"),
            (ID_CONTEXT_COPY_LOCATION, "copy_link"),
            (ID_CONTEXT_COPY_STREAM_URL, "copy_stream_url"),
            (ID_CONTEXT_ADD_TO_PLAYLIST, "add_to_playlist"),
        ]),
        MainView::UserPlaylists => Some(&[
            (ID_CONTEXT_PLAY, "open_playlist"),
            (ID_CONTEXT_CREATE_PLAYLIST, "create_playlist"),
            (ID_CONTEXT_PLAY_PLAYLIST, "play_playlist"),
            (ID_CONTEXT_SHUFFLE_PLAYLIST, "shuffle_playlist"),
            (ID_CONTEXT_ADD_PLAYLIST_TO_QUEUE, "add_to_playback_queue"),
            (ID_CONTEXT_REMOVE_PLAYLIST, "remove_playlist"),
        ]),
        MainView::UserPlaylistItems => Some(&[
            (ID_CONTEXT_PLAY, "play"),
            (ID_CONTEXT_PLAY_PLAYLIST, "play_playlist"),
            (ID_CONTEXT_SHUFFLE_PLAYLIST, "shuffle_playlist"),
            (ID_CONTEXT_ADD_TO_QUEUE, "add_to_playback_queue"),
            (ID_CONTEXT_REMOVE_FROM_QUEUE, "remove_from_playback_queue"),
            (ID_CONTEXT_REMOVE_FROM_PLAYLIST, "remove_from_playlist"),
            (ID_CONTEXT_COPY_LOCATION, "copy_link"),
            (ID_CONTEXT_COPY_STREAM_URL, "copy_stream_url"),
        ]),
        _ => None,
    }
}

unsafe fn show_context_menu_for_active_view(window: HWND) {
    match state(window).map(|state| state.view) {
        Some(
            MainView::Results
            | MainView::LocalFolder
            | MainView::Favorites
            | MainView::History
            | MainView::NotificationCenter
            | MainView::UserPlaylists
            | MainView::UserPlaylistItems,
        ) => {
            show_list_context_menu(window, LPARAM(-1));
        }
        Some(MainView::Player) => show_player_context_menu(window, LPARAM(-1)),
        _ => {}
    }
}

unsafe fn show_player_context_menu(window: HWND, location: LPARAM) {
    let Some((language, item, focused)) = state(window).and_then(|state| {
        Some((
            state.application.settings().language.clone(),
            state.application.player_session().current_item()?.clone(),
            GetFocus(),
        ))
    }) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&language);
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let mut entries = vec![
        (ID_CONTEXT_DETAILS, "show_video_details"),
        (
            ID_CONTEXT_COPY_LOCATION,
            if item.is_local_media() {
                "copy_path"
            } else {
                "copy_link"
            },
        ),
        (ID_CONTEXT_ADD_TO_QUEUE, "add_to_playback_queue"),
        (ID_CONTEXT_REMOVE_FROM_QUEUE, "remove_from_playback_queue"),
        (ID_CONTEXT_PLAYBACK_QUEUE, "playback_queue"),
        (ID_CONTEXT_ADD_TO_PLAYLIST, "add_to_playlist"),
        (ID_CONTEXT_REMOVE_FROM_PLAYLIST, "remove_from_playlist"),
    ];
    entries.insert(
        1,
        if state(window).is_some_and(|state| state.application.is_favorite(&item)) {
            (ID_CONTEXT_COLLECTION_REMOVE, "remove_favorite")
        } else {
            (ID_CONTEXT_ADD_FAVORITE, "add_favorite")
        },
    );
    if !item.is_local_media() {
        entries.insert(1, (ID_CONTEXT_COPY_STREAM_URL, "copy_stream_url"));
    }
    if item.youtube_url_at_timestamp(0.0).is_some() {
        entries.insert(1, (ID_CONTEXT_COPY_TIMESTAMP, "copy_timestamp_link"));
    }
    entries.push((ID_CONTEXT_CLOSE_PLAYER, "close_player"));
    for (id, key) in entries {
        let label = wide(catalog.text(key));
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(label.as_ptr()));
    }
    let mut fallback_point = POINT::default();
    let point = context_menu_point(location, focused).or_else(|| {
        GetCursorPos(&raw mut fallback_point)
            .is_ok()
            .then_some(fallback_point)
    });
    if let Some(point) = point {
        if let Some(state) = state_mut(window) {
            state.modal_open = true;
        }
        let selected = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            None,
            window,
            None,
        );
        if let Some(state) = state_mut(window) {
            state.modal_open = false;
        }
        resume_deferred_window_work(window);
        match usize::try_from(selected.0).unwrap_or_default() {
            ID_CONTEXT_DETAILS => show_player_details(window),
            ID_CONTEXT_COPY_LOCATION => copy_active_location(window),
            ID_CONTEXT_COPY_STREAM_URL => copy_active_stream_url(window),
            ID_CONTEXT_COPY_TIMESTAMP => copy_current_timestamp_link(window),
            ID_CONTEXT_ADD_TO_QUEUE => add_active_item_to_playback_queue(window),
            ID_CONTEXT_REMOVE_FROM_QUEUE => remove_active_item_from_playback_queue(window),
            ID_CONTEXT_PLAYBACK_QUEUE => show_playback_queue(window),
            ID_CONTEXT_ADD_FAVORITE => add_active_favorite(window),
            ID_CONTEXT_COLLECTION_REMOVE => remove_active_favorite(window),
            ID_CONTEXT_ADD_TO_PLAYLIST => add_active_item_to_user_playlist(window),
            ID_CONTEXT_REMOVE_FROM_PLAYLIST => remove_active_item_from_user_playlist(window),
            ID_CONTEXT_CLOSE_PLAYER => navigate_back(window),
            _ => {}
        }
    }
    let _ = DestroyMenu(menu);
    if focused.0.is_null() {
        if let Some(state) = state(window) {
            let _ = SetFocus(Some(active_primary_control(state)));
        }
    } else {
        let _ = SetFocus(Some(focused));
    }
}

unsafe fn context_menu_point(location: LPARAM, list: HWND) -> Option<POINT> {
    if location.0 != -1 {
        let packed = location.0.to_le_bytes();
        return Some(POINT {
            x: i32::from(i16::from_le_bytes([packed[0], packed[1]])),
            y: i32::from(i16::from_le_bytes([packed[2], packed[3]])),
        });
    }
    let mut bounds = RECT::default();
    GetWindowRect(list, &raw mut bounds).ok()?;
    Some(POINT {
        x: bounds.left.saturating_add(24),
        y: bounds.top.saturating_add(24),
    })
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
    let Some(state) = state_mut(window) else {
        return;
    };
    layout_controls_state(window, state);
}

unsafe fn layout_controls_state(window: HWND, state: &mut WindowState) {
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
    if matches!(state.view, MainView::Search | MainView::DirectLink) {
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
        if state.view == MainView::Search {
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
        }
    } else if state.view == MainView::Player {
        state
            .player_controls
            .layout(width, height, margin, status_height);
    } else {
        let action_rows = if state.view == MainView::LocalFolder {
            2
        } else {
            1
        };
        let _ = MoveWindow(
            state.list,
            margin,
            margin,
            width - margin * 2,
            height - button_height * action_rows - status_height - margin * (action_rows + 3),
            true,
        );
    }
    layout_bottom_controls(state, width, height, margin, button_height, status_height);
}

#[allow(clippy::too_many_lines)]
unsafe fn layout_bottom_controls(
    state: &WindowState,
    width: i32,
    height: i32,
    margin: i32,
    button_height: i32,
    status_height: i32,
) {
    let local_folder = state.view == MainView::LocalFolder;
    let direct_link = state.view == MainView::DirectLink;
    let favorites = state.view == MainView::Favorites;
    let history = state.view == MainView::History;
    let notification_center = state.view == MainView::NotificationCenter;
    let user_playlists = state.view == MainView::UserPlaylists;
    let user_playlist_items = state.view == MainView::UserPlaylistItems;
    let first_button_y = if local_folder {
        height - button_height * 2 - margin * 2
    } else {
        height - button_height - margin
    };
    let status_y = first_button_y - status_height - margin;
    let _ = MoveWindow(
        state.status,
        margin,
        status_y,
        width - margin * 2,
        status_height,
        true,
    );
    let _ = MoveWindow(
        state.open,
        margin,
        first_button_y,
        if local_folder {
            (width - margin * 4) / 3
        } else {
            120
        },
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
        if local_folder {
            margin * 2 + (width - margin * 4) / 3
        } else {
            margin + 132
        },
        first_button_y,
        if local_folder {
            (width - margin * 4) / 3
        } else {
            180
        },
        button_height,
        true,
    );
    if local_folder {
        layout_local_folder_buttons(state, width, height, first_button_y, margin, button_height);
    } else if direct_link {
        layout_button_row(
            &[
                state.back,
                state.direct_play,
                state.direct_download_audio,
                state.direct_download_video,
                state.direct_copy_stream,
            ],
            width,
            first_button_y,
            margin,
            button_height,
        );
    } else if favorites || history {
        let controls = if history {
            vec![
                state.back,
                state.open,
                state.collection_remove,
                state.history_clear,
            ]
        } else {
            vec![state.back, state.open, state.collection_remove]
        };
        layout_button_row(&controls, width, first_button_y, margin, button_height);
    } else if notification_center {
        layout_button_row(
            &[state.back, state.open, state.notification_clear],
            width,
            first_button_y,
            margin,
            button_height,
        );
    } else if user_playlists {
        layout_button_row(
            &[
                state.back,
                state.playlist_create,
                state.open,
                state.collection_remove,
            ],
            width,
            first_button_y,
            margin,
            button_height,
        );
    } else if user_playlist_items {
        layout_button_row(
            &[
                state.back,
                state.open,
                state.playlist_play_all,
                state.playlist_shuffle,
                state.collection_remove,
                state.playlist_add_all_to_queue,
            ],
            width,
            first_button_y,
            margin,
            button_height,
        );
    }
}

unsafe fn layout_local_folder_buttons(
    state: &WindowState,
    width: i32,
    height: i32,
    first_button_y: i32,
    margin: i32,
    button_height: i32,
) {
    let button_width = (width - margin * 4) / 3;
    let row_two_y = height - button_height - margin;
    let _ = MoveWindow(
        state.play_folder,
        margin * 3 + button_width * 2,
        first_button_y,
        button_width,
        button_height,
        true,
    );
    for (index, control) in [
        state.shuffle_folder,
        state.add_folder_to_queue,
        state.folder_playback_queue,
    ]
    .into_iter()
    .enumerate()
    {
        let x = margin + i32::try_from(index).unwrap_or_default() * (button_width + margin);
        let _ = MoveWindow(control, x, row_two_y, button_width, button_height, true);
    }
}

unsafe fn layout_button_row(controls: &[HWND], width: i32, y: i32, margin: i32, height: i32) {
    let count = i32::try_from(controls.len()).unwrap_or(1).max(1);
    let button_width = (width - margin * (count + 1)) / count;
    for (index, control) in controls.iter().copied().enumerate() {
        let x = margin + i32::try_from(index).unwrap_or_default() * (button_width + margin);
        let _ = MoveWindow(control, x, y, button_width, height, true);
    }
}

unsafe fn set_view_visibility(state: &mut WindowState) {
    let list_visible = matches!(
        state.view,
        MainView::MainMenu
            | MainView::Results
            | MainView::LocalFolder
            | MainView::Favorites
            | MainView::History
            | MainView::NotificationCenter
            | MainView::UserPlaylists
            | MainView::UserPlaylistItems
    );
    let text_entry_visible = matches!(state.view, MainView::Search | MainView::DirectLink);
    let search_visible = state.view == MainView::Search;
    let direct_link_visible = state.view == MainView::DirectLink;
    let playlist_items_available = state.view != MainView::UserPlaylistItems
        || state
            .application
            .user_playlists()
            .get(state.current_user_playlist_index)
            .is_some_and(|playlist| !playlist.items.is_empty());
    let collection_visible = view_has_collection_remove(state.view) && playlist_items_available;
    let back_visible = view_has_back_button(state.view);
    let open_visible = list_visible && playlist_items_available;
    let folder_visible = state.view == MainView::LocalFolder;
    for (control, visible) in [
        (state.list, list_visible),
        (state.open, open_visible),
        (state.search_label, text_entry_visible),
        (state.search_edit, text_entry_visible),
        (state.kind_label, search_visible),
        (state.kind, search_visible),
        (state.search, search_visible),
        (state.back, back_visible),
        (state.play_folder, folder_visible),
        (state.shuffle_folder, folder_visible),
        (state.add_folder_to_queue, folder_visible),
        (state.folder_playback_queue, folder_visible),
        (state.direct_play, direct_link_visible),
        (state.direct_download_audio, direct_link_visible),
        (state.direct_download_video, direct_link_visible),
        (state.direct_copy_stream, direct_link_visible),
        (state.collection_remove, collection_visible),
        (state.history_clear, state.view == MainView::History),
        (
            state.notification_clear,
            state.view == MainView::NotificationCenter,
        ),
        (state.playlist_create, state.view == MainView::UserPlaylists),
        (
            state.playlist_play_all,
            state.view == MainView::UserPlaylistItems && playlist_items_available,
        ),
        (
            state.playlist_shuffle,
            state.view == MainView::UserPlaylistItems && playlist_items_available,
        ),
        (
            state.playlist_add_all_to_queue,
            state.view == MainView::UserPlaylistItems && playlist_items_available,
        ),
    ] {
        let _ = ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE });
    }
    state
        .player_controls
        .set_visible(state.view == MainView::Player);
}

const fn view_has_back_button(view: MainView) -> bool {
    matches!(
        view,
        MainView::Search
            | MainView::DirectLink
            | MainView::Results
            | MainView::LocalFolder
            | MainView::Favorites
            | MainView::History
            | MainView::NotificationCenter
            | MainView::UserPlaylists
            | MainView::UserPlaylistItems
    )
}

const fn view_has_collection_remove(view: MainView) -> bool {
    matches!(
        view,
        MainView::Favorites
            | MainView::History
            | MainView::UserPlaylists
            | MainView::UserPlaylistItems
    )
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
    stop_controlled_repeat(window);
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
        Some(MainView::LocalFolder) => activate_local_folder_selection(window),
        Some(MainView::Favorites | MainView::History) => activate_collection_selection(window),
        Some(MainView::NotificationCenter) => activate_notification_selection(window),
        Some(MainView::UserPlaylists) => open_selected_user_playlist(window),
        Some(MainView::UserPlaylistItems) => activate_user_playlist_item(window),
        Some(MainView::Search | MainView::DirectLink | MainView::Player) | None => {}
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
    if item_id == "play_folder" {
        open_media_folder(window);
        return;
    }
    if item_id == "search" {
        show_search(window);
        return;
    }
    if item_id == "resume_last_session" {
        resume_last_player_session(window);
        return;
    }
    if item_id == "direct_link" {
        show_direct_link(window);
        return;
    }
    if item_id == "favorites" {
        show_media_collection(window, MainView::Favorites);
        return;
    }
    if item_id == "history" {
        show_media_collection(window, MainView::History);
        return;
    }
    if item_id == "notification_center" {
        show_notification_center(window);
        return;
    }
    if item_id == "bookmarks" {
        show_bookmarks_dialog(window, false, false);
        return;
    }
    if item_id == "playlists" {
        show_user_playlists(window);
        return;
    }
    if item_id == "playback_queue" {
        show_playback_queue(window);
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
    let Some(item) = state.application.prepare_search_playback(index) else {
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
    start_sequence_media_item(window, item, None);
}

unsafe fn activate_local_folder_selection(window: HWND) {
    let selected = state(window).map(|state| SendMessageW(state.list, LB_GETCURSEL, None, None).0);
    let Some(Ok(index)) = selected.map(usize::try_from) else {
        return;
    };
    let item = state_mut(window).and_then(|state| {
        state
            .application
            .prepare_local_folder_playback(index, false)
    });
    if let Some(item) = item {
        start_sequence_media_item(window, item, None);
    }
}

unsafe fn activate_collection_selection(window: HWND) {
    let selection = state(window).and_then(|state| {
        let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
        usize::try_from(selected)
            .ok()
            .map(|index| (state.view, index))
    });
    let Some((view, index)) = selection else {
        return;
    };
    let item = state_mut(window).and_then(|state| match view {
        MainView::Favorites => state.application.prepare_favorite_playback(index),
        MainView::History => state.application.prepare_history_playback(index),
        _ => None,
    });
    if let Some(item) = item {
        if matches!(
            item.kind,
            apricot_core::MediaKind::Playlist | apricot_core::MediaKind::Channel
        ) {
            let message = format!(
                "{} is preserved in favorites, but collection navigation is not implemented in this internal build yet.",
                item.title
            );
            show_error_message(window, &message);
            return;
        }
        start_sequence_media_item(window, item, None);
    }
}

unsafe fn activate_notification_selection(window: HWND) {
    let selected = state(window).map(|state| SendMessageW(state.list, LB_GETCURSEL, None, None).0);
    let Some(Ok(index)) = selected.map(usize::try_from) else {
        return;
    };
    let item =
        state_mut(window).and_then(|state| state.application.prepare_notification_playback(index));
    if let Some(item) = item {
        start_media_item(window, item, None);
    } else if let Some(state) = state(window) {
        set_status(
            state,
            &catalog_text(&state.application, "notification_center_empty"),
            true,
        );
    }
}

unsafe fn play_current_local_folder(window: HWND, shuffle: bool) {
    let selected = state(window).map_or(0, |state| {
        usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).unwrap_or_default()
    });
    let item = state_mut(window).and_then(|state| {
        state
            .application
            .prepare_local_folder_playback(selected, shuffle)
    });
    let Some(item) = item else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "folder_no_media"),
                true,
            );
        }
        return;
    };
    start_media_item_with_shuffle(window, item, None, shuffle);
}

unsafe fn add_current_local_folder_to_queue(window: HWND) {
    let result =
        state_mut(window).map(|state| state.application.add_local_folder_to_playback_queue());
    let Some(result) = result else {
        return;
    };
    let Some(state) = state(window) else {
        return;
    };
    match result {
        Ok(outcome) => {
            let message = catalog_text(&state.application, "folder_queue_added")
                .replace("{count}", &outcome.added.to_string());
            set_status(state, &message, true);
        }
        Err(error) => {
            let message = format!("Playback queue was not updated: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
        }
    }
}

unsafe fn start_media_item(
    window: HWND,
    item: apricot_core::MediaItem,
    queue_mode: Option<QueueStartMode>,
) {
    start_media_item_with_options(window, item, queue_mode, None, false, None);
}

unsafe fn start_sequence_media_item(
    window: HWND,
    item: apricot_core::MediaItem,
    queue_mode: Option<QueueStartMode>,
) {
    start_media_item_with_options(window, item, queue_mode, None, true, None);
}

unsafe fn start_media_item_with_shuffle(
    window: HWND,
    item: apricot_core::MediaItem,
    queue_mode: Option<QueueStartMode>,
    shuffle: bool,
) {
    start_media_item_with_options(window, item, queue_mode, Some(shuffle), true, None);
}

unsafe fn start_media_item_at(
    window: HWND,
    item: apricot_core::MediaItem,
    start_position_seconds: f64,
) {
    start_media_item_with_options(
        window,
        item,
        None,
        None,
        false,
        Some(start_position_seconds.max(0.0)),
    );
}

unsafe fn start_media_item_with_options(
    window: HWND,
    item: apricot_core::MediaItem,
    queue_mode: Option<QueueStartMode>,
    session_shuffle: Option<bool>,
    preserve_sequence: bool,
    start_position_seconds: Option<f64>,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    let start_position_seconds =
        start_position_seconds.or_else(|| state.application.playback_resume_position(&item));
    if !preserve_sequence {
        state.application.prepare_standalone_playback();
    }
    state.pending_queued_start = queue_mode.map(|mode| PendingQueuedStart {
        item: item.clone(),
        mode,
    });
    if !matches!(
        item.source,
        apricot_core::MediaSource::Youtube | apricot_core::MediaSource::Direct
    ) {
        start_player_at(window, item, session_shuffle, start_position_seconds);
        return;
    }
    start_youtube_resolve_with_options(
        window,
        &item,
        YoutubeResolvePurpose::Playback,
        session_shuffle,
        preserve_sequence,
        start_position_seconds,
    );
}

unsafe fn start_youtube_resolve(
    window: HWND,
    item: &apricot_core::MediaItem,
    purpose: YoutubeResolvePurpose,
) {
    if purpose == YoutubeResolvePurpose::Playback
        && let Some(state) = state_mut(window)
    {
        state.application.prepare_standalone_playback();
    }
    start_youtube_resolve_with_options(window, item, purpose, None, false, None);
}

unsafe fn start_youtube_resolve_with_options(
    window: HWND,
    item: &apricot_core::MediaItem,
    purpose: YoutubeResolvePurpose,
    session_shuffle: Option<bool>,
    preserve_sequence: bool,
    start_position_seconds: Option<f64>,
) {
    let Some(url) = item.url.as_ref().map(ToString::to_string) else {
        if let Some(state) = state_mut(window) {
            report_youtube_resolve_start_error(
                window,
                state,
                purpose,
                "The selected item has no media URL",
            );
        }
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    state.next_youtube_resolve_token = state.next_youtube_resolve_token.wrapping_add(1).max(1);
    let token = state.next_youtube_resolve_token;
    let backend = media_resolve_backend(item, &state.application.settings().youtube_backend);
    let Some(components) = application_directory().map(|path| path.join("components")) else {
        report_youtube_resolve_start_error(
            window,
            state,
            purpose,
            "Application path is unavailable",
        );
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
            state.pending_youtube_resolve = Some(PendingYoutubeResolve {
                token,
                purpose,
                original_item: item.clone(),
                session_shuffle,
                preserve_sequence,
                start_position_seconds,
            });
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
        Err(error) => {
            report_youtube_resolve_start_error(window, state, purpose, &error.to_string());
        }
    }
}

fn media_resolve_backend(
    item: &apricot_core::MediaItem,
    configured_backend: &str,
) -> YoutubeBackend {
    if item.source == apricot_core::MediaSource::Direct
        && item.youtube_url_at_timestamp(0.0).is_none()
    {
        YoutubeBackend::YtDlp
    } else {
        YoutubeBackend::from_setting_value(configured_backend)
    }
}

fn resolved_playback_item(
    mut resolved: apricot_core::MediaItem,
    original: &apricot_core::MediaItem,
    preserve_sequence: bool,
) -> apricot_core::MediaItem {
    if preserve_sequence || original.source == apricot_core::MediaSource::Direct {
        resolved.source = original.source;
        resolved.id = original.id.clone();
        resolved.url.clone_from(&original.url);
    }
    resolved
}

unsafe fn report_youtube_resolve_start_error(
    window: HWND,
    state: &mut WindowState,
    purpose: YoutubeResolvePurpose,
    message: &str,
) {
    if purpose == YoutubeResolvePurpose::Playback {
        state.pending_queued_start = None;
    }
    let visible_message = if purpose == YoutubeResolvePurpose::CopyStreamUrl {
        catalog_text(&state.application, "stream_url_failed").replace("{error}", message)
    } else {
        message.to_owned()
    };
    set_status(state, &visible_message, true);
    if purpose == YoutubeResolvePurpose::Playback {
        show_error_message(window, &visible_message);
    }
    let _ = SetFocus(Some(active_primary_control(state)));
}

unsafe fn activate_player_control(window: HWND, activation: PlayerControlActivation) {
    match activation {
        PlayerControlActivation::Action(action_id) => activate_action(window, action_id),
        PlayerControlActivation::SessionAutoplayNext => {
            toggle_player_session_setting(window, SessionToggle::AutoplayNext);
        }
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
    let Some(pending) = state
        .pending_youtube_resolve
        .as_ref()
        .filter(|pending| pending.token == token)
        .cloned()
    else {
        return;
    };
    state.pending_youtube_resolve = None;
    stop_youtube_timer(window);
    let preference =
        youtube_stream_preference(&state.application.settings().stream_format_preference);
    let Some(selection) = select_youtube_playback_formats(formats, preference) else {
        report_youtube_resolve_start_error(
            window,
            state,
            pending.purpose,
            "No playable YouTube stream was returned",
        );
        return;
    };
    let Some(primary) = formats.get(selection.primary_index) else {
        report_youtube_resolve_start_error(
            window,
            state,
            pending.purpose,
            "The selected YouTube stream was invalid",
        );
        return;
    };
    if pending.purpose == YoutubeResolvePurpose::CopyStreamUrl {
        copy_text_and_announce(window, &primary.url, "stream_url_copied");
        return;
    }
    item = resolved_playback_item(item, &pending.original_item, pending.preserve_sequence);
    let Ok(stream_url) = primary.url.parse() else {
        report_youtube_resolve_start_error(
            window,
            state,
            pending.purpose,
            "The selected YouTube stream URL was invalid",
        );
        return;
    };
    item.stream_url = Some(stream_url);
    item.external_audio_url = selection
        .external_audio_index
        .and_then(|index| formats.get(index))
        .and_then(|format| format.url.parse().ok());
    start_player_at(
        window,
        item,
        pending.session_shuffle,
        pending.start_position_seconds,
    );
}

unsafe fn start_player_at(
    window: HWND,
    item: apricot_core::MediaItem,
    session_shuffle: Option<bool>,
    start_position_seconds: Option<f64>,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.playback.is_none() {
        match PlaybackRuntime::spawn() {
            Ok(runtime) => state.playback = Some(runtime),
            Err(error) => {
                state.pending_queued_start = None;
                show_error_message(window, &format!("Player did not start: {error}"));
                return;
            }
        }
    }
    persist_current_playback_position(state);
    let generation = state.application.start_player_item_with_shuffle_at(
        item,
        session_shuffle,
        start_position_seconds,
    );
    let Some(options) = playback_launch_options(state, start_position_seconds) else {
        let message = "Internal mpv player was not found";
        let _ = state
            .application
            .apply_playback_event(generation, PlaybackEvent::Failed(message.to_owned()));
        state.pending_queued_start = None;
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
        state.pending_queued_start = None;
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
    let previous_focus = preserve_selection
        .then(|| GetFocus())
        .and_then(|focused| state.player_controls.control_id_for_window(focused));
    let Some(model) = state.application.player_screen_model() else {
        return;
    };
    state.player_controls.sync(&model);
    let title = wide(&model.window_title);
    let _ = SetWindowTextW(window, PCWSTR(title.as_ptr()));
    layout_controls_state(window, state);
    if focus {
        let target = previous_focus
            .and_then(|id| state.player_controls.window_for_id(id))
            .unwrap_or_else(|| state.player_controls.initial_focus());
        let _ = SetFocus(Some(target));
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
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    state.application.navigate_main_menu();
    state.view = MainView::MainMenu;
    refresh_main_menu(state);
    set_status(state, &catalog_text(&state.application, "ready"), false);
    layout_controls_state(window, state);
    let _ = SetFocus(Some(state.list));
}

unsafe fn show_search(window: HWND) {
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != Route::Search {
        state.application.navigate_main_menu();
        state
            .application
            .navigate_to(RouteFrame::new(Route::Search));
    }
    state.view = MainView::Search;
    set_control_text(state, state.search_label, "search_query");
    set_control_text(state, state.search, "search");
    set_status(state, &catalog_text(&state.application, "ready"), false);
    layout_controls_state(window, state);
    let _ = SetFocus(Some(state.search_edit));
}

unsafe fn resume_last_player_session(window: HWND) {
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    let Some(resume) = state.application.prepare_last_player_session_resume() else {
        set_status(
            state,
            &catalog_text(&state.application, "resume_last_session_unavailable"),
            true,
        );
        return;
    };
    if resume.return_screen == "user_playlist_items" {
        state.current_user_playlist_index = resume
            .return_data
            .get("playlist_index")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_default();
        state.current_user_playlist_item_index = resume
            .return_data
            .get("item_index")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_default();
    }
    if resume.sequence_active {
        start_sequence_media_item(window, resume.item, None);
    } else {
        start_media_item(window, resume.item, None);
    }
}

unsafe fn show_direct_link(window: HWND) {
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != Route::DirectLink {
        state.application.navigate_main_menu();
        state
            .application
            .navigate_to(RouteFrame::new(Route::DirectLink));
    }
    state.view = MainView::DirectLink;
    set_control_text(state, state.search_label, "direct_link_url");
    let _ = SetWindowTextW(state.search_edit, w!(""));
    set_status(state, &catalog_text(&state.application, "ready"), false);
    layout_controls_state(window, state);
    let _ = SetFocus(Some(state.search_edit));
}

unsafe fn show_media_collection(window: HWND, view: MainView) {
    if view == MainView::History
        && state(window).is_some_and(|state| !state.application.settings().enable_history)
    {
        show_main_menu(window);
        return;
    }
    let route = match view {
        MainView::Favorites => Route::Favorites,
        MainView::History => Route::History,
        _ => return,
    };
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != route {
        state.application.navigate_main_menu();
        state.application.navigate_to(RouteFrame::new(route));
    }
    state.view = view;
    refresh_media_collection(state, true, true);
    layout_controls_state(window, state);
}

unsafe fn show_notification_center(window: HWND) {
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != Route::NotificationCenter {
        state.application.navigate_main_menu();
        state
            .application
            .navigate_to(RouteFrame::new(Route::NotificationCenter));
    }
    state.view = MainView::NotificationCenter;
    refresh_notification_center(state, true, true, None);
    layout_controls_state(window, state);
}

unsafe fn show_user_playlists(window: HWND) {
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != Route::UserPlaylists {
        state.application.navigate_main_menu();
        state
            .application
            .navigate_to(RouteFrame::new(Route::UserPlaylists));
    }
    state.view = MainView::UserPlaylists;
    refresh_user_playlists(state, true, true);
    layout_controls_state(window, state);
}

unsafe fn open_selected_user_playlist(window: HWND) {
    let selected = state(window).map(|state| SendMessageW(state.list, LB_GETCURSEL, None, None).0);
    let Some(Ok(index)) = selected.map(usize::try_from) else {
        return;
    };
    if state(window).is_none_or(|state| index >= state.application.user_playlists().len()) {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "no_playlists"),
                true,
            );
        }
        return;
    }
    show_user_playlist_items(window, index, true);
}

unsafe fn show_user_playlist_items(window: HWND, playlist_index: usize, push_route: bool) {
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    if playlist_index >= state.application.user_playlists().len() {
        show_user_playlists(window);
        return;
    }
    state.current_user_playlist_index = playlist_index;
    state.current_user_playlist_item_index = 0;
    if push_route && state.application.current_route() != Route::UserPlaylistItems {
        let mut frame = RouteFrame::new(Route::UserPlaylistItems);
        frame
            .parameters
            .insert("playlist_index".to_string(), playlist_index.into());
        state.application.navigate_to(frame);
    }
    state.view = MainView::UserPlaylistItems;
    refresh_user_playlist_items(state, true, true);
    layout_controls_state(window, state);
}

unsafe fn activate_user_playlist_item(window: HWND) {
    let Some((playlist_index, item_index)) = state(window).and_then(|state| {
        let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
        usize::try_from(selected)
            .ok()
            .map(|item_index| (state.current_user_playlist_index, item_index))
    }) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "playlist_empty"),
                true,
            );
        }
        return;
    };
    let item = state_mut(window).and_then(|state| {
        state.current_user_playlist_item_index = item_index;
        state
            .application
            .prepare_user_playlist_item_playback(playlist_index, item_index)
    });
    if let Some(item) = item {
        start_media_item(window, item, None);
    }
}

unsafe fn navigate_back(window: HWND) {
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.view == MainView::Player {
        close_player_runtime(window, state);
    }
    let frame = state
        .application
        .navigate_back()
        .unwrap_or_else(|| RouteFrame::new(Route::MainMenu));
    let route = frame.route;
    let saved_index = frame
        .parameters
        .get("index")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok());
    match route {
        Route::Search => {
            state.view = MainView::Search;
            layout_controls_state(window, state);
            let _ = SetFocus(Some(state.search_edit));
        }
        Route::DirectLink => {
            state.view = MainView::DirectLink;
            set_control_text(state, state.search_label, "direct_link_url");
            layout_controls_state(window, state);
            let _ = SetFocus(Some(state.search_edit));
        }
        Route::Results => {
            state.view = MainView::Results;
            refresh_results(state, true);
            layout_controls_state(window, state);
        }
        Route::LocalFolder => {
            state.view = MainView::LocalFolder;
            refresh_local_folder(state, true, false);
            layout_controls_state(window, state);
        }
        Route::Favorites => {
            state.view = MainView::Favorites;
            refresh_media_collection(state, true, false);
            select_list_index(state.list, saved_index);
            layout_controls_state(window, state);
        }
        Route::History => {
            state.view = MainView::History;
            refresh_media_collection(state, true, false);
            select_list_index(state.list, saved_index);
            layout_controls_state(window, state);
        }
        Route::NotificationCenter => {
            state.view = MainView::NotificationCenter;
            refresh_notification_center(state, true, false, saved_index);
            layout_controls_state(window, state);
        }
        Route::Bookmarks => {
            state.view = MainView::MainMenu;
            refresh_main_menu(state);
            layout_controls_state(window, state);
            show_bookmarks_dialog(window, false, true);
        }
        Route::UserPlaylists => {
            state.view = MainView::UserPlaylists;
            refresh_user_playlists(state, true, false);
            layout_controls_state(window, state);
        }
        Route::UserPlaylistItems => {
            state.view = MainView::UserPlaylistItems;
            refresh_user_playlist_items(state, true, false);
            layout_controls_state(window, state);
        }
        Route::Player => {
            state.view = MainView::Player;
            refresh_player(window, state, true, true);
        }
        _ => {
            state.application.navigate_main_menu();
            state.view = MainView::MainMenu;
            refresh_main_menu(state);
            layout_controls_state(window, state);
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

unsafe fn submit_primary_text(window: HWND) {
    match state(window).map(|state| state.view) {
        Some(MainView::Search) => submit_search(window),
        Some(MainView::DirectLink) => {
            let action = state(window).map_or_else(
                || "play".to_owned(),
                |state| {
                    state
                        .application
                        .settings()
                        .direct_link_enter_action
                        .clone()
                },
            );
            activate_direct_link(window, &action);
        }
        _ => {}
    }
}

unsafe fn activate_direct_link(window: HWND, action: &str) {
    let item = state(window)
        .map(|state| window_text(state.search_edit))
        .and_then(|value| apricot_core::MediaItem::from_direct_link(&value));
    let Some(item) = item else {
        if let Some(state) = state(window) {
            let message = catalog_text(&state.application, "direct_link_invalid");
            set_status(state, &message, true);
            let _ = SetFocus(Some(state.search_edit));
        }
        return;
    };
    match action {
        "copy_stream_url" => {
            start_youtube_resolve(window, &item, YoutubeResolvePurpose::CopyStreamUrl);
        }
        "download_audio" | "download_video" => {
            if let Some(state) = state(window) {
                let message =
                    "Direct-link downloads are not implemented in this internal Rust build yet.";
                set_status(state, message, true);
                let _ = SetFocus(Some(state.search_edit));
            }
        }
        _ => start_youtube_resolve(window, &item, YoutubeResolvePurpose::Playback),
    }
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
    if state(window).is_some_and(|state| state.modal_open) {
        return;
    }
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
                        state
                            .pending_youtube_resolve
                            .as_ref()
                            .map(|pending| pending.token)
                            .or_else(|| {
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
    if state(window).is_some_and(|state| state.modal_open) {
        return;
    }
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
                    state.pending_queued_start = None;
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
                confirm_pending_queued_start(window, state);
                let current_item = state.application.player_session().current_item().cloned();
                let title = current_item.as_ref().map_or("", |item| item.title.as_str());
                let message = catalog_text(&state.application, "playing").replace("{title}", title);
                set_status(state, &message, true);
                if let Some(item) = current_item {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0.0, |duration| duration.as_secs_f64());
                    if let Err(error) = state.application.record_history(item, "played", timestamp)
                    {
                        let message = format!("History was not saved: {error}");
                        set_status(state, &message, true);
                    }
                }
            }
            PlaybackEvent::Paused(paused) => {
                let key = if paused {
                    "playback_paused"
                } else {
                    "playback_playing"
                };
                let focused_play_pause = state
                    .player_controls
                    .control_id_for_window(GetFocus())
                    .is_some_and(|id| id == "play_pause");
                set_status(
                    state,
                    &catalog_text(&state.application, key),
                    state.application.settings().announce_play_pause && !focused_play_pause,
                );
                refresh_player(window, state, false, true);
            }
            PlaybackEvent::Position { .. } | PlaybackEvent::MediaInfo(_) => {}
            PlaybackEvent::Ended => {
                let autoplay_next = state
                    .application
                    .player_session()
                    .enabled_toggles()
                    .contains(&SessionToggle::AutoplayNext);
                if autoplay_next {
                    navigate_player_relative(window, 1);
                    return;
                }
                set_status(
                    state,
                    &catalog_text(&state.application, "playback_finished"),
                    state.application.settings().announce_playback_finished,
                );
            }
            PlaybackEvent::Failed(error) => {
                state.pending_queued_start = None;
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
            layout_controls_state(window, state);
        }
        (Some(SearchWorkKind::More), SearchApplyOutcome::Appended { added }) => {
            append_results(state, added);
        }
        _ => {}
    }
    let continue_navigation = state.pending_player_navigation.take();
    if let Some(delta) = continue_navigation {
        navigate_player_relative(window, delta);
    }
}

unsafe fn finish_youtube_error(window: HWND, generation: u64, message: &str) {
    let direct_fallback = {
        let Some(state) = state_mut(window) else {
            return;
        };
        if state
            .pending_youtube_resolve
            .as_ref()
            .is_some_and(|pending| pending.token == generation)
        {
            let pending = state
                .pending_youtube_resolve
                .take()
                .expect("matching pending resolve exists");
            if pending.purpose == YoutubeResolvePurpose::Playback {
                state.pending_queued_start = None;
            }
            stop_youtube_timer(window);
            if pending.purpose == YoutubeResolvePurpose::Playback
                && pending.original_item.source == apricot_core::MediaSource::Direct
                && pending
                    .original_item
                    .youtube_url_at_timestamp(0.0)
                    .is_none()
            {
                let fallback_message = catalog_text(&state.application, "direct_link_fallback");
                set_status(state, &fallback_message, true);
                Some((
                    pending.original_item,
                    pending.session_shuffle,
                    pending.start_position_seconds,
                ))
            } else {
                let visible_message = if pending.purpose == YoutubeResolvePurpose::CopyStreamUrl {
                    catalog_text(&state.application, "stream_url_failed")
                        .replace("{error}", message)
                } else {
                    message.to_owned()
                };
                set_status(state, &visible_message, true);
                if pending.purpose == YoutubeResolvePurpose::Playback {
                    show_error_message(window, &visible_message);
                }
                let _ = SetFocus(Some(active_primary_control(state)));
                None
            }
        } else {
            finish_youtube_error_state(window, state, generation, message);
            None
        }
    };
    if let Some((item, session_shuffle, start_position_seconds)) = direct_fallback {
        start_player_at(window, item, session_shuffle, start_position_seconds);
    }
}

unsafe fn finish_youtube_error_state(
    window: HWND,
    state: &mut WindowState,
    generation: u64,
    message: &str,
) {
    state.pending_queued_start = None;
    let was_initial = state
        .pending_youtube_work
        .as_ref()
        .is_none_or(|work| work.work_kind == SearchWorkKind::Initial);
    let _ = state.application.fail_search(generation, message);
    state.pending_youtube_work = None;
    state.pending_player_navigation = None;
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
    state.pending_player_navigation = None;
    state.pending_queued_start = None;
    let _ = state.youtube_search.cancel();
    let _ = EnableWindow(state.search, true);
    stop_youtube_timer(window);
}

unsafe fn result_selection_changed(window: HWND) {
    if state(window).is_some_and(|state| state.view == MainView::LocalFolder) {
        local_folder_selection_changed(window);
        return;
    }
    if let Some(state) = state_mut(window)
        && state.view == MainView::UserPlaylists
    {
        let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
        if let Ok(index) = usize::try_from(selected)
            && index < state.application.user_playlists().len()
        {
            state.current_user_playlist_index = index;
        }
        return;
    }
    if let Some(state) = state_mut(window)
        && state.view == MainView::UserPlaylistItems
    {
        let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
        if let Ok(index) = usize::try_from(selected)
            && state
                .application
                .user_playlists()
                .get(state.current_user_playlist_index)
                .is_some_and(|playlist| index < playlist.items.len())
        {
            state.current_user_playlist_item_index = index;
        }
        return;
    }
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

unsafe fn local_folder_selection_changed(window: HWND) {
    let (before, added) = {
        let Some(state) = state_mut(window) else {
            return;
        };
        let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
        let Ok(index) = usize::try_from(selected) else {
            return;
        };
        if !state.application.select_local_folder_item(index) {
            return;
        }
        let before = state
            .application
            .local_folder_session()
            .visible_items()
            .len();
        let added = if index + 1 == before && state.application.local_folder_session().has_more() {
            state.application.append_local_folder_batch()
        } else {
            0
        };
        (before, added)
    };
    if added == 0 {
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    for item in &state.application.local_folder_session().visible_items()[before..] {
        add_list_string(state.list, &local_folder_result_label(item, &catalog));
    }
    let selected = state.application.local_folder_session().selected_index();
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    let loaded = format!(
        "{} of {} files loaded",
        state
            .application
            .local_folder_session()
            .visible_items()
            .len(),
        state.application.local_folder_session().items().len()
    );
    set_status(state, &loaded, true);
}

unsafe fn refresh_results(state: &mut WindowState, focus: bool) {
    set_open_button_label(state, "open");
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

unsafe fn refresh_local_folder(state: &mut WindowState, focus: bool, announce_status: bool) {
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let accessible_name = wide(catalog.text("play_from_folder"));
    let _ = SetWindowTextW(state.list, PCWSTR(accessible_name.as_ptr()));
    set_open_button_label(state, "play");
    let session = state.application.local_folder_session();
    if session.is_empty() {
        add_list_string(state.list, catalog.text("folder_no_media"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("folder_no_media"), announce_status);
    } else {
        for item in session.visible_items() {
            add_list_string(state.list, &local_folder_result_label(item, &catalog));
        }
        let selected = session
            .selected_index()
            .min(session.visible_items().len() - 1);
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
        let message = catalog
            .text("folder_loaded")
            .replace("{count}", &session.items().len().to_string());
        set_status(state, &message, announce_status);
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

unsafe fn refresh_media_collection(state: &mut WindowState, focus: bool, announce_status: bool) {
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let (name_key, empty_key, items) = match state.view {
        MainView::Favorites => (
            "favorites",
            "favorites_empty",
            state.application.favorites(),
        ),
        MainView::History => ("history", "history_empty", state.application.history()),
        _ => return,
    };
    let accessible_name = wide(catalog.text(name_key));
    let _ = SetWindowTextW(state.list, PCWSTR(accessible_name.as_ptr()));
    set_open_button_label(state, "play");
    set_control_text(
        state,
        state.collection_remove,
        if state.view == MainView::Favorites {
            "remove_favorite"
        } else {
            "remove_history_item"
        },
    );
    if items.is_empty() {
        add_list_string(state.list, catalog.text(empty_key));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text(empty_key), announce_status);
    } else {
        for item in items {
            add_list_string(
                state.list,
                &media_collection_label(item, state.view, &catalog),
            );
        }
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(
            state,
            &format!("{}: {}", catalog.text(name_key), items.len()),
            announce_status,
        );
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

unsafe fn refresh_notification_center(
    state: &mut WindowState,
    focus: bool,
    announce_status: bool,
    selected_index: Option<usize>,
) {
    let previous = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok();
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let accessible_name = wide(catalog.text("notification_center"));
    let _ = SetWindowTextW(state.list, PCWSTR(accessible_name.as_ptr()));
    set_open_button_label(state, "play");
    let notifications = state.application.notifications();
    if notifications.is_empty() {
        add_list_string(state.list, catalog.text("notification_center_empty"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(
            state,
            catalog.text("notification_center_empty"),
            announce_status,
        );
    } else {
        for notification in notifications {
            add_list_string(state.list, &notification_label(notification, &catalog));
        }
        let selected = selected_index
            .or(previous)
            .unwrap_or_default()
            .min(notifications.len() - 1);
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
        set_status(
            state,
            &format!(
                "{}: {}",
                catalog.text("notification_center"),
                notifications.len()
            ),
            announce_status,
        );
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

fn notification_label(
    notification: &apricot_app::AppNotification,
    catalog: &apricot_core::TranslationCatalog,
) -> String {
    let mut parts = Vec::new();
    if !notification.title.trim().is_empty() {
        parts.push(notification.title.clone());
    }
    if !notification.message.trim().is_empty() {
        parts.push(notification.message.clone());
    }
    if let Some(item) = &notification.item {
        if !item.title.trim().is_empty() {
            parts.push(item.title.clone());
        }
        if !item.channel.trim().is_empty() {
            parts.push(format!("{}: {}", catalog.text("channel"), item.channel));
        }
    }
    if notification.timestamp > 0.0
        && let Some(timestamp) = std::time::Duration::try_from_secs_f64(notification.timestamp)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
    {
        parts.push(
            timestamp
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
        );
    }
    parts.join(" | ")
}

unsafe fn refresh_user_playlists(state: &mut WindowState, focus: bool, announce_status: bool) {
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let accessible_name = wide(catalog.text("playlists"));
    let _ = SetWindowTextW(state.list, PCWSTR(accessible_name.as_ptr()));
    set_open_button_label(state, "open_playlist");
    set_control_text(state, state.collection_remove, "remove_playlist");
    let playlists = state.application.user_playlists();
    if playlists.is_empty() {
        add_list_string(state.list, catalog.text("no_playlists"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("no_playlists"), announce_status);
    } else {
        for playlist in playlists {
            add_list_string(
                state.list,
                &format!(
                    "{} | {} {}",
                    playlist.title,
                    playlist.items.len(),
                    catalog.text("video")
                ),
            );
        }
        let selected = state
            .current_user_playlist_index
            .min(playlists.len().saturating_sub(1));
        state.current_user_playlist_index = selected;
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
        set_status(
            state,
            &format!("{}: {}", catalog.text("playlists"), playlists.len()),
            announce_status,
        );
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

unsafe fn refresh_user_playlist_items(state: &mut WindowState, focus: bool, announce_status: bool) {
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let accessible_name = wide(catalog.text("playlist_items"));
    let _ = SetWindowTextW(state.list, PCWSTR(accessible_name.as_ptr()));
    set_open_button_label(state, "play");
    set_control_text(state, state.collection_remove, "remove_from_playlist");
    let Some(playlist) = state
        .application
        .user_playlists()
        .get(state.current_user_playlist_index)
    else {
        add_list_string(state.list, catalog.text("playlist_empty"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("playlist_empty"), announce_status);
        return;
    };
    if playlist.items.is_empty() {
        add_list_string(state.list, catalog.text("playlist_empty"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("playlist_empty"), announce_status);
    } else {
        for item in &playlist.items {
            add_list_string(
                state.list,
                &media_collection_label(item, MainView::UserPlaylistItems, &catalog),
            );
        }
        let selected = state
            .current_user_playlist_item_index
            .min(playlist.items.len().saturating_sub(1));
        state.current_user_playlist_item_index = selected;
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
        set_status(
            state,
            &format!(
                "{}: {} | {}",
                catalog.text("playlist_items"),
                playlist.title,
                playlist.items.len()
            ),
            announce_status,
        );
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

fn media_collection_label(
    item: &apricot_core::MediaItem,
    view: MainView,
    catalog: &apricot_core::TranslationCatalog,
) -> String {
    let mut parts = Vec::new();
    if view == MainView::History {
        if let Some(timestamp) = item
            .metadata
            .get("timestamp")
            .and_then(serde_json::Value::as_f64)
            .and_then(|seconds| std::time::Duration::try_from_secs_f64(seconds).ok())
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        {
            parts.push(
                timestamp
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string(),
            );
        }
        if let Some(action) = item.metadata.get("action").and_then(|value| value.as_str())
            && !action.trim().is_empty()
        {
            parts.push(action.to_owned());
        }
    }
    parts.push(item.title.clone());
    if !item.channel.is_empty() {
        parts.push(format!("{}: {}", catalog.text("channel"), item.channel));
    }
    parts.join(" | ")
}

fn local_folder_result_label(
    item: &apricot_core::MediaItem,
    catalog: &apricot_core::TranslationCatalog,
) -> String {
    let relative = item
        .metadata
        .get("relative_path")
        .and_then(|value| value.as_str())
        .unwrap_or(&item.title);
    let format = item
        .metadata
        .get("extension")
        .and_then(|value| value.as_str())
        .unwrap_or_else(|| catalog.text("file_format_unknown"));
    let folder = item
        .metadata
        .get("folder")
        .and_then(|value| value.as_str())
        .unwrap_or(&item.channel);
    catalog
        .text("local_file_result_line")
        .replace("{title}", relative)
        .replace("{format}", format)
        .replace("{folder}", folder)
}

unsafe fn set_open_button_label(state: &WindowState, key: &str) {
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let label = wide(catalog.text(key));
    let _ = SetWindowTextW(state.open, PCWSTR(label.as_ptr()));
}

unsafe fn set_control_text(state: &WindowState, control: HWND, key: &str) {
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let label = wide(catalog.text(key));
    let _ = SetWindowTextW(control, PCWSTR(label.as_ptr()));
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

fn playback_launch_options(
    state: &WindowState,
    start_position_seconds: Option<f64>,
) -> Option<MpvLaunchOptions> {
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
    options.initial_position_seconds = start_position_seconds;
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

unsafe fn select_list_index(control: HWND, requested: Option<usize>) {
    let Some(requested) = requested else {
        return;
    };
    let count =
        usize::try_from(SendMessageW(control, LB_GETCOUNT, None, None).0).unwrap_or_default();
    if count == 0 {
        return;
    }
    SendMessageW(
        control,
        LB_SETCURSEL,
        Some(WPARAM(requested.min(count - 1))),
        None,
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
    if state.view == MainView::Player
        && !chord.control
        && !chord.shift
        && !chord.alt
        && chord.key == ShortcutKey::Space
        && state.player_controls.is_native_action_control(GetFocus())
    {
        return false;
    }
    let (scope, accepts_text) = match state.view {
        MainView::Search | MainView::DirectLink => (ActionScope::Dialog, true),
        MainView::MainMenu
        | MainView::Results
        | MainView::LocalFolder
        | MainView::Favorites
        | MainView::History
        | MainView::NotificationCenter
        | MainView::UserPlaylists
        | MainView::UserPlaylistItems => (ActionScope::List, false),
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
    if !is_global && matches!(state.view, MainView::Search | MainView::DirectLink) {
        return false;
    }
    if !is_global && state.view == MainView::MainMenu && action.id.as_str() != "open_selected" {
        return false;
    }
    if crate::shortcut_win32::is_repeat(message) && action.repeat == RepeatPolicy::None {
        return true;
    }
    if action.repeat == RepeatPolicy::Controlled {
        if !crate::shortcut_win32::is_repeat(message) {
            start_controlled_repeat(window, action.id.as_str(), chord, message.wParam.0);
        }
        return true;
    }
    activate_action(window, action.id.as_str());
    true
}

unsafe fn start_controlled_repeat(
    window: HWND,
    action_id: &'static str,
    chord: apricot_core::shortcut::ShortcutChord,
    virtual_key: usize,
) {
    stop_controlled_repeat(window);
    let delay = {
        let Some(state) = state_mut(window) else {
            return;
        };
        state.controlled_repeat = Some(ControlledRepeatState {
            action_id,
            virtual_key,
            chord,
        });
        let settings = state.application.settings();
        controlled_repeat_timing(
            action_id,
            settings.speed_pitch_hold_delay_ms,
            settings.speed_pitch_hold_interval_ms,
        )
        .0
    };
    activate_action(window, action_id);
    if state(window).is_some_and(|state| state.controlled_repeat.is_some()) {
        let _ = SetTimer(Some(window), CONTROLLED_REPEAT_TIMER_ID, delay.max(1), None);
    }
}

unsafe fn tick_controlled_repeat(window: HWND) {
    let repeat = {
        let Some(state) = state(window) else {
            return;
        };
        let Some(repeat) = state.controlled_repeat else {
            stop_controlled_repeat(window);
            return;
        };
        if state.view != MainView::Player
            || !state.application.player_session().is_open()
            || !controlled_repeat_keys_still_down(repeat)
        {
            stop_controlled_repeat(window);
            return;
        }
        let settings = state.application.settings();
        let interval = controlled_repeat_timing(
            repeat.action_id,
            settings.speed_pitch_hold_delay_ms,
            settings.speed_pitch_hold_interval_ms,
        )
        .1;
        (repeat, interval)
    };
    let _ = SetTimer(
        Some(window),
        CONTROLLED_REPEAT_TIMER_ID,
        repeat.1.max(1),
        None,
    );
    activate_action(window, repeat.0.action_id);
}

unsafe fn handle_controlled_repeat_release(window: HWND, message: &MSG) {
    if !matches!(message.message, WM_KEYUP | WM_SYSKEYUP) {
        return;
    }
    let Some(repeat) = state(window).and_then(|state| state.controlled_repeat) else {
        return;
    };
    let released = message.wParam.0;
    let required_modifier_released = (repeat.chord.control
        && released == usize::from(VK_CONTROL.0))
        || (repeat.chord.shift && released == usize::from(VK_SHIFT.0))
        || (repeat.chord.alt && released == usize::from(VK_MENU.0));
    if released == repeat.virtual_key || required_modifier_released {
        stop_controlled_repeat(window);
    }
}

unsafe fn stop_controlled_repeat(window: HWND) {
    let _ = KillTimer(Some(window), CONTROLLED_REPEAT_TIMER_ID);
    if let Some(state) = state_mut(window) {
        state.controlled_repeat = None;
    }
}

fn controlled_repeat_timing(
    action_id: &str,
    configured_delay: i64,
    configured_interval: i64,
) -> (u32, u32) {
    if matches!(
        action_id,
        "player_speed_up" | "player_speed_down" | "player_pitch_up" | "player_pitch_down"
    ) {
        return (
            u32::try_from(configured_delay.clamp(50, 1_000)).unwrap_or(180),
            u32::try_from(configured_interval.clamp(20, 500)).unwrap_or(110),
        );
    }
    (SEEK_HOLD_DELAY_MS, SEEK_HOLD_INTERVAL_MS)
}

unsafe fn controlled_repeat_keys_still_down(repeat: ControlledRepeatState) -> bool {
    virtual_key_is_down(repeat.virtual_key)
        && virtual_key_is_down(usize::from(VK_CONTROL.0)) == repeat.chord.control
        && virtual_key_is_down(usize::from(VK_SHIFT.0)) == repeat.chord.shift
        && virtual_key_is_down(usize::from(VK_MENU.0)) == repeat.chord.alt
}

unsafe fn virtual_key_is_down(key: usize) -> bool {
    i32::try_from(key).is_ok_and(|key| GetAsyncKeyState(key).is_negative())
}

unsafe fn activate_action(window: HWND, action_id: &str) {
    match action_id {
        "open_main_menu" => show_main_menu(window),
        "open_search" => show_search(window),
        "resume_last_session" => resume_last_player_session(window),
        "open_direct_link" => show_direct_link(window),
        "open_favorites" => show_media_collection(window, MainView::Favorites),
        "open_history" => show_media_collection(window, MainView::History),
        "new_subscription_videos" => show_notification_center(window),
        "open_bookmarks" => show_bookmarks_dialog(window, false, false),
        "open_playlists" => show_user_playlists(window),
        "open_settings" => open_settings(window),
        "open_action_finder" => show_action_finder(window),
        "open_play_file" => open_media_file(window),
        "open_play_from_folder" => open_media_folder(window),
        "open_selected" => activate_selection(window),
        "background_play_pause" | "player_play_pause" => toggle_player_pause(window),
        "player_back" => navigate_back(window),
        "player_previous" => navigate_player_relative(window, -1),
        "player_next" => navigate_player_relative(window, 1),
        "player_time" => announce_player_time(window),
        "player_volume_status" => announce_player_volume(window),
        "player_format_status" => announce_player_format(window),
        "player_details" => show_player_details(window),
        "player_add_bookmark" => show_add_current_bookmark_prompt(window),
        "player_bookmarks" => show_bookmarks_dialog(window, true, false),
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
        "copy_link" | "player_copy_link" => copy_active_location(window),
        "player_copy_timestamp_link" => copy_current_timestamp_link(window),
        "copy_stream_url" => copy_active_stream_url(window),
        "add_favorite" => add_active_favorite(window),
        "remove_favorite" => remove_active_favorite(window),
        "create_playlist" => create_user_playlist(window, None),
        "add_to_playlist" => add_active_item_to_user_playlist(window),
        "remove_from_playlist" => remove_active_item_from_user_playlist(window),
        "remove_selected" => remove_selected_collection_item(window),
        "context_menu" => show_context_menu_for_active_view(window),
        "add_to_playback_queue" => add_active_item_to_playback_queue(window),
        "remove_from_playback_queue" => remove_active_item_from_playback_queue(window),
        "open_playback_queue" => show_playback_queue(window),
        _ => show_unimplemented_action(window, action_id),
    }
}

unsafe fn navigate_player_relative(window: HWND, delta: i32) {
    let outcome = {
        let Some(state) = state_mut(window) else {
            return;
        };
        if state.pending_player_navigation.is_some() || state.pending_youtube_resolve.is_some() {
            return;
        }
        state.application.request_relative_player_item(delta)
    };
    match outcome {
        PlayerNavigationOutcome::Item { item, origin } => {
            if let Some(state) = state_mut(window) {
                sync_user_playlist_item_selection(state, &item);
            }
            let preserve_sequence = origin == apricot_app::PlayerNavigationOrigin::Sequence;
            if preserve_sequence {
                start_sequence_media_item(window, *item, None);
            } else {
                start_media_item(
                    window,
                    *item,
                    (origin == apricot_app::PlayerNavigationOrigin::Queue)
                        .then_some(QueueStartMode::Front),
                );
            }
        }
        PlayerNavigationOutcome::LoadingMore(work) => {
            let Some(state) = state_mut(window) else {
                return;
            };
            state.pending_player_navigation = Some(delta);
            let message = catalog_text(&state.application, "loading_more_results");
            set_status(state, &message, true);
            start_youtube_work(window, work);
        }
        PlayerNavigationOutcome::Unavailable => {
            let Some(state) = state(window) else {
                return;
            };
            let key = if delta < 0 {
                "no_previous_item"
            } else {
                "no_next_item"
            };
            set_status(state, &catalog_text(&state.application, key), true);
        }
    }
}

fn sync_user_playlist_item_selection(state: &mut WindowState, item: &apricot_core::MediaItem) {
    let Some(apricot_app::PlaybackSequenceSource::UserPlaylist { playlist_index }) =
        state.application.player_sequence_source()
    else {
        return;
    };
    let Some(identity) = item.stable_identity() else {
        return;
    };
    let Some(item_index) = state
        .application
        .user_playlists()
        .get(playlist_index)
        .and_then(|playlist| {
            playlist
                .items
                .iter()
                .position(|candidate| candidate.stable_identity().as_deref() == Some(&identity))
        })
    else {
        return;
    };
    state.current_user_playlist_index = playlist_index;
    state.current_user_playlist_item_index = item_index;
}

unsafe fn confirm_pending_queued_start(window: HWND, state: &mut WindowState) {
    let Some(pending) = state.pending_queued_start.take() else {
        return;
    };
    let result = match pending.mode {
        QueueStartMode::Front => state.application.confirm_queued_item_started(&pending.item),
        QueueStartMode::Matching => state.application.remove_from_playback_queue(&pending.item),
    };
    if let Err(error) = result {
        let message = format!("Playback queue was not updated: {error}");
        set_status(state, &message, true);
        show_error_message(window, &message);
    }
}

unsafe fn active_media_item(window: HWND) -> Option<apricot_core::MediaItem> {
    let state = state(window)?;
    match state.view {
        MainView::Results => {
            let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
            let index = usize::try_from(selected).ok()?;
            state
                .application
                .search_session()
                .items()
                .get(index)
                .cloned()
        }
        MainView::LocalFolder => {
            let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
            let index = usize::try_from(selected).ok()?;
            state
                .application
                .local_folder_session()
                .visible_items()
                .get(index)
                .cloned()
        }
        MainView::Favorites => {
            let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
            let index = usize::try_from(selected).ok()?;
            state.application.favorites().get(index).cloned()
        }
        MainView::History => {
            let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
            let index = usize::try_from(selected).ok()?;
            state.application.history().get(index).cloned()
        }
        MainView::NotificationCenter => {
            let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
            let index = usize::try_from(selected).ok()?;
            state
                .application
                .notifications()
                .get(index)
                .and_then(|notification| notification.item.clone())
        }
        MainView::UserPlaylistItems => {
            let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
            let index = usize::try_from(selected).ok()?;
            state
                .application
                .user_playlists()
                .get(state.current_user_playlist_index)?
                .items
                .get(index)
                .cloned()
        }
        MainView::Player => state.application.player_session().current_item().cloned(),
        MainView::MainMenu | MainView::Search | MainView::DirectLink | MainView::UserPlaylists => {
            None
        }
    }
}

unsafe fn add_active_favorite(window: HWND) {
    let Some(item) = active_media_item(window) else {
        return;
    };
    let result = state_mut(window).map(|state| state.application.add_favorite(item));
    let Some(result) = result else {
        return;
    };
    let Some(state) = state(window) else {
        return;
    };
    match result {
        Ok(apricot_app::CollectionAddOutcome::Added) => {
            set_status(
                state,
                &catalog_text(&state.application, "favorite_added"),
                true,
            );
        }
        Ok(apricot_app::CollectionAddOutcome::AlreadyPresent) => {
            set_status(
                state,
                &catalog_text(&state.application, "favorite_exists"),
                true,
            );
        }
        Ok(apricot_app::CollectionAddOutcome::Unplayable) => {
            set_status(
                state,
                &catalog_text(&state.application, "no_selection"),
                true,
            );
        }
        Err(error) => {
            let message = format!("Favorites were not updated: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
        }
    }
}

unsafe fn remove_active_favorite(window: HWND) {
    let Some(item) = active_media_item(window) else {
        return;
    };
    let result = state_mut(window).map(|state| state.application.remove_favorite_item(&item));
    match result {
        Some(Ok(Some(_))) => {
            if let Some(state) = state_mut(window) {
                if state.view == MainView::Favorites {
                    refresh_media_collection(state, true, false);
                }
                set_status(
                    state,
                    &catalog_text(&state.application, "favorite_removed"),
                    true,
                );
            }
        }
        Some(Ok(None)) => {
            if let Some(state) = state(window) {
                set_status(
                    state,
                    &catalog_text(&state.application, "not_in_favorites"),
                    true,
                );
            }
        }
        Some(Err(error)) => {
            let message = format!("Favorites were not updated: {error}");
            if let Some(state) = state(window) {
                set_status(state, &message, true);
            }
            show_error_message(window, &message);
        }
        None => {}
    }
}

unsafe fn remove_selected_collection_item(window: HWND) {
    let selection = state(window).and_then(|state| {
        let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
        usize::try_from(selected)
            .ok()
            .map(|index| (state.view, index))
    });
    let Some((view, index)) = selection else {
        return;
    };
    match view {
        MainView::Favorites => remove_favorite_at(window, index),
        MainView::History => {
            let result =
                state_mut(window).map(|state| state.application.remove_history_item(index));
            finish_collection_removal(window, result, "history_removed");
        }
        MainView::NotificationCenter => remove_notification_at(window, index),
        MainView::UserPlaylists => remove_selected_user_playlist(window),
        MainView::UserPlaylistItems => remove_selected_user_playlist_item(window),
        _ => {}
    }
}

unsafe fn remove_notification_at(window: HWND, index: usize) {
    let result = state_mut(window).map(|state| state.application.remove_notification(index));
    let Some(result) = result else {
        return;
    };
    match result {
        Ok(Some(_)) => {
            if let Some(state) = state_mut(window) {
                refresh_notification_center(state, true, false, Some(index.saturating_sub(1)));
            }
        }
        Ok(None) => {}
        Err(error) => {
            let message = format!("Notification was not removed: {error}");
            if let Some(state) = state(window) {
                set_status(state, &message, true);
            }
            show_error_message(window, &message);
        }
    }
}

unsafe fn remove_favorite_at(window: HWND, index: usize) {
    let result = state_mut(window).map(|state| state.application.remove_favorite(index));
    finish_collection_removal(window, result, "favorite_removed");
}

unsafe fn finish_collection_removal(
    window: HWND,
    result: Option<
        std::result::Result<
            Option<apricot_core::MediaItem>,
            apricot_app::MediaCollectionControllerError,
        >,
    >,
    success_key: &str,
) {
    let Some(result) = result else {
        return;
    };
    match result {
        Ok(Some(_)) => {
            if let Some(state) = state_mut(window) {
                refresh_media_collection(state, true, false);
                set_status(state, &catalog_text(&state.application, success_key), true);
            }
        }
        Ok(None) => {}
        Err(error) => {
            let message = format!("Media collection was not updated: {error}");
            if let Some(state) = state(window) {
                set_status(state, &message, true);
            }
            show_error_message(window, &message);
        }
    }
}

unsafe fn clear_history(window: HWND) {
    let result = state_mut(window).map(|state| state.application.clear_history());
    let Some(result) = result else {
        return;
    };
    match result {
        Ok(_) => {
            if let Some(state) = state_mut(window) {
                refresh_media_collection(state, true, false);
                set_status(
                    state,
                    &catalog_text(&state.application, "history_cleared"),
                    true,
                );
            }
        }
        Err(error) => {
            let message = format!("History was not cleared: {error}");
            if let Some(state) = state(window) {
                set_status(state, &message, true);
            }
            show_error_message(window, &message);
        }
    }
}

unsafe fn clear_notifications(window: HWND) {
    let result = state_mut(window).map(|state| state.application.clear_notifications());
    let Some(result) = result else {
        return;
    };
    match result {
        Ok(_) => {
            if let Some(state) = state_mut(window) {
                refresh_notification_center(state, true, false, None);
                set_status(
                    state,
                    &catalog_text(&state.application, "notifications_cleared"),
                    true,
                );
            }
        }
        Err(error) => {
            let message = format!("Notifications were not cleared: {error}");
            if let Some(state) = state(window) {
                set_status(state, &message, true);
            }
            show_error_message(window, &message);
        }
    }
}

unsafe fn show_bookmarks_dialog(window: HWND, current_only: bool, return_on_close: bool) {
    let Some((labels, entries, can_add)) = state_mut(window).map(|state| {
        state.modal_open = true;
        let labels = BookmarkDialogOwnedLabels {
            title: catalog_text(&state.application, "bookmarks"),
            list_name: catalog_text(&state.application, "bookmarks"),
            empty: catalog_text(&state.application, "bookmarks_empty"),
            add: catalog_text(&state.application, "add_bookmark"),
            play: catalog_text(&state.application, "play"),
            rename: catalog_text(&state.application, "rename_bookmark"),
            delete: catalog_text(&state.application, "delete_bookmark"),
            copy: catalog_text(&state.application, "copy_timestamp_link"),
            close: catalog_text(&state.application, "back"),
        };
        let entries = bookmark_dialog_entries(&state.application, current_only);
        let can_add = state.application.player_session().is_open()
            && state
                .application
                .player_session()
                .current_item()
                .is_some_and(apricot_core::MediaItem::is_playable);
        (labels, entries, can_add)
    }) else {
        return;
    };
    let result = crate::bookmark_dialog_win32::show(
        window,
        labels.as_borrowed(),
        entries,
        can_add,
        Box::new(move |dialog, request| {
            handle_bookmark_dialog_request(window, dialog, current_only, request)
        }),
    );
    let Some(state) = state_mut(window) else {
        return;
    };
    state.modal_open = false;
    resume_deferred_window_work(window);
    if let Err(error) = result {
        let message = format!("Bookmarks dialog failed: {error}");
        set_status(state, &message, true);
        show_error_message(window, &message);
        return;
    }
    if return_on_close && state.application.current_route() == Route::Bookmarks {
        navigate_back(window);
    }
}

struct BookmarkDialogOwnedLabels {
    title: String,
    list_name: String,
    empty: String,
    add: String,
    play: String,
    rename: String,
    delete: String,
    copy: String,
    close: String,
}

impl BookmarkDialogOwnedLabels {
    fn as_borrowed(&self) -> BookmarkDialogLabels<'_> {
        BookmarkDialogLabels {
            title: &self.title,
            list_name: &self.list_name,
            empty: &self.empty,
            add: &self.add,
            play: &self.play,
            rename: &self.rename,
            delete: &self.delete,
            copy: &self.copy,
            close: &self.close,
        }
    }
}

fn bookmark_dialog_entries(
    application: &Application,
    current_only: bool,
) -> Vec<BookmarkDialogEntry> {
    let catalog = apricot_app::embedded_catalog(&application.settings().language);
    let bookmarks = if current_only {
        application
            .player_session()
            .current_item()
            .map_or_else(Vec::new, |item| application.bookmarks_for_item(item))
    } else {
        application.sorted_bookmarks()
    };
    bookmarks
        .into_iter()
        .enumerate()
        .map(|(index, bookmark)| {
            let name = if bookmark.name.trim().is_empty() {
                catalog.text("bookmark")
            } else {
                bookmark.name.trim()
            };
            let mut parts = vec![
                format!("{}. {}", index + 1, format_duration(bookmark.position)),
                name.to_owned(),
            ];
            if !current_only && !bookmark.media_title.trim().is_empty() {
                parts.push(bookmark.media_title.trim().to_owned());
            }
            BookmarkDialogEntry {
                id: bookmark.id.clone(),
                label: parts.join(" | "),
            }
        })
        .collect()
}

unsafe fn handle_bookmark_dialog_request(
    window: HWND,
    dialog: HWND,
    current_only: bool,
    request: BookmarkDialogRequest,
) -> BookmarkDialogResponse {
    match request {
        BookmarkDialogRequest::Add => {
            let selected_id = prompt_add_current_bookmark(window, dialog);
            bookmark_refresh_response(window, current_only, selected_id)
        }
        BookmarkDialogRequest::Rename(id) => {
            let selected_id = prompt_rename_bookmark(window, dialog, &id).then_some(id);
            bookmark_refresh_response(window, current_only, selected_id)
        }
        BookmarkDialogRequest::Delete(id) => {
            let result = state_mut(window).map(|state| state.application.delete_bookmark(&id));
            match result {
                Some(Ok(true)) => {
                    if let Some(state) = state(window) {
                        set_status(
                            state,
                            &catalog_text(&state.application, "bookmark_deleted"),
                            true,
                        );
                    }
                    bookmark_refresh_response(window, current_only, None)
                }
                Some(Ok(false)) | None => BookmarkDialogResponse::KeepOpen,
                Some(Err(error)) => {
                    report_bookmark_error(window, &error.to_string());
                    BookmarkDialogResponse::KeepOpen
                }
            }
        }
        BookmarkDialogRequest::Copy(id) => {
            copy_bookmark_timestamp(window, &id);
            BookmarkDialogResponse::KeepOpen
        }
        BookmarkDialogRequest::Play(id) => {
            play_bookmark(window, &id);
            BookmarkDialogResponse::Close
        }
    }
}

unsafe fn bookmark_refresh_response(
    window: HWND,
    current_only: bool,
    selected_id: Option<String>,
) -> BookmarkDialogResponse {
    let Some(state) = state(window) else {
        return BookmarkDialogResponse::KeepOpen;
    };
    BookmarkDialogResponse::Refresh {
        entries: bookmark_dialog_entries(&state.application, current_only),
        selected_id,
    }
}

unsafe fn show_add_current_bookmark_prompt(window: HWND) {
    if let Some(state) = state_mut(window) {
        state.modal_open = true;
    }
    let _ = prompt_add_current_bookmark(window, window);
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
}

unsafe fn prompt_add_current_bookmark(window: HWND, owner: HWND) -> Option<String> {
    let Some((item, position, title, prompt, default_name, ok, cancel)) =
        state(window).and_then(|state| {
            let session = state.application.player_session();
            let item = session.current_item()?.clone();
            let position = session.position_seconds().max(0.0);
            let default_name = catalog_text(&state.application, "bookmark_default_name")
                .replace("{time}", &format_duration(position));
            Some((
                item,
                position,
                catalog_text(&state.application, "add_bookmark"),
                catalog_text(&state.application, "bookmark_name_prompt"),
                default_name,
                catalog_text(&state.application, "ok"),
                catalog_text(&state.application, "cancel"),
            ))
        })
    else {
        if let Some(state) = state(window) {
            set_status(state, &catalog_text(&state.application, "no_player"), true);
        }
        return None;
    };
    let name = match crate::playlist_dialog_win32::prompt_name_with_initial(
        owner,
        &title,
        &prompt,
        &default_name,
        &ok,
        &cancel,
    ) {
        Ok(Some(name)) => {
            let name = name.trim();
            if name.is_empty() {
                default_name
            } else {
                name.to_owned()
            }
        }
        Ok(None) => return None,
        Err(error) => {
            report_bookmark_error(window, &error.to_string());
            return None;
        }
    };
    let result = state_mut(window).map(|state| {
        state
            .application
            .add_bookmark(&name, position, item, unix_timestamp())
    });
    match result {
        Some(Ok(Some(bookmark))) => {
            if let Some(state) = state(window) {
                let message = catalog_text(&state.application, "bookmark_added")
                    .replace("{name}", &bookmark.name)
                    .replace("{time}", &format_duration(bookmark.position));
                set_status(state, &message, true);
            }
            Some(bookmark.id)
        }
        Some(Ok(None)) | None => None,
        Some(Err(error)) => {
            report_bookmark_error(window, &error.to_string());
            None
        }
    }
}

unsafe fn prompt_rename_bookmark(window: HWND, owner: HWND, id: &str) -> bool {
    let Some((current_name, title, prompt, ok, cancel)) = state(window).and_then(|state| {
        let bookmark = state.application.bookmark(id)?;
        Some((
            bookmark.name.clone(),
            catalog_text(&state.application, "rename_bookmark"),
            catalog_text(&state.application, "bookmark_name_prompt"),
            catalog_text(&state.application, "ok"),
            catalog_text(&state.application, "cancel"),
        ))
    }) else {
        return false;
    };
    let name = match crate::playlist_dialog_win32::prompt_name_with_initial(
        owner,
        &title,
        &prompt,
        &current_name,
        &ok,
        &cancel,
    ) {
        Ok(Some(name)) if !name.trim().is_empty() => name,
        Ok(_) => return false,
        Err(error) => {
            report_bookmark_error(window, &error.to_string());
            return false;
        }
    };
    match state_mut(window).map(|state| {
        state
            .application
            .rename_bookmark(id, &name, unix_timestamp())
    }) {
        Some(Ok(true)) => {
            if let Some(state) = state(window) {
                let message = catalog_text(&state.application, "bookmark_renamed")
                    .replace("{name}", name.trim());
                set_status(state, &message, true);
            }
            true
        }
        Some(Ok(false)) | None => false,
        Some(Err(error)) => {
            report_bookmark_error(window, &error.to_string());
            false
        }
    }
}

unsafe fn copy_bookmark_timestamp(window: HWND, id: &str) {
    let url = state(window).and_then(|state| {
        let bookmark = state.application.bookmark(id)?;
        bookmark
            .media
            .youtube_url_at_timestamp(bookmark.position)
            .map(|url| url.to_string())
    });
    if let Some(url) = url {
        copy_text_and_announce(window, &url, "timestamp_url_copied");
    } else if let Some(state) = state(window) {
        set_status(
            state,
            &catalog_text(&state.application, "timestamp_url_unavailable"),
            true,
        );
    }
}

unsafe fn play_bookmark(window: HWND, id: &str) {
    let Some((bookmark, same_item)) = state(window).and_then(|state| {
        let bookmark = state.application.bookmark(id)?.clone();
        let same_item = state
            .application
            .player_session()
            .current_item()
            .is_some_and(|item| {
                state
                    .application
                    .bookmarks_for_item(item)
                    .iter()
                    .any(|candidate| candidate.id == bookmark.id)
            });
        Some((bookmark, same_item))
    }) else {
        return;
    };
    if same_item
        && execute_player_command(
            window,
            PlaybackCommand::SeekAbsolute {
                seconds: bookmark.position,
                exact: true,
            },
        )
    {
        if let Some(state) = state(window) {
            announce_selected_bookmark(state, &bookmark.name, bookmark.position);
        }
        return;
    }
    if let Some(state) = state_mut(window) {
        if state.application.current_route() == Route::Player {
            let _ = state.application.navigate_back();
        }
        if state.application.current_route() != Route::Bookmarks {
            state
                .application
                .navigate_to(RouteFrame::new(Route::Bookmarks));
        }
    }
    start_media_item_at(window, bookmark.media, bookmark.position);
}

unsafe fn announce_selected_bookmark(state: &WindowState, bookmark_name: &str, position: f64) {
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let name = if bookmark_name.trim().is_empty() {
        catalog.text("bookmark")
    } else {
        bookmark_name.trim()
    };
    let message = catalog
        .text("bookmark_selected")
        .replace("{name}", name)
        .replace("{time}", &format_duration(position));
    set_status(state, &message, true);
}

unsafe fn report_bookmark_error(window: HWND, detail: &str) {
    let message = format!("Bookmarks were not updated: {detail}");
    if let Some(state) = state(window) {
        set_status(state, &message, true);
    }
    show_error_message(window, &message);
}

unsafe fn create_user_playlist(window: HWND, initial_item: Option<apricot_core::MediaItem>) {
    let Some((title, prompt, ok_label, cancel_label)) = state_mut(window).map(|state| {
        state.modal_open = true;
        (
            catalog_text(&state.application, "create_playlist"),
            catalog_text(&state.application, "playlist_name"),
            catalog_text(&state.application, "ok"),
            catalog_text(&state.application, "cancel"),
        )
    }) else {
        return;
    };
    let result = crate::playlist_dialog_win32::prompt_name(
        window,
        &title,
        &prompt,
        &ok_label,
        &cancel_label,
    );
    let Some(state) = state_mut(window) else {
        return;
    };
    state.modal_open = false;
    resume_deferred_window_work(window);
    let name = match result {
        Ok(Some(name)) => name,
        Ok(None) => return,
        Err(error) => {
            let message = format!("Playlist name dialog failed: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
            return;
        }
    };
    let timestamp = unix_timestamp();
    let outcome = if let Some(item) = initial_item {
        state
            .application
            .create_user_playlist_with_item(&name, item, timestamp)
    } else {
        state.application.create_user_playlist(&name, timestamp)
    };
    match outcome {
        Ok(apricot_app::PlaylistCreateOutcome::Created(index)) => {
            state.current_user_playlist_index = index;
            if state.view == MainView::UserPlaylists {
                refresh_user_playlists(state, true, false);
            } else if state.view == MainView::UserPlaylistItems {
                refresh_user_playlist_items(state, true, false);
            }
            let message = catalog_text(&state.application, "playlist_created")
                .replace("{title}", name.trim());
            set_status(state, &message, true);
        }
        Ok(apricot_app::PlaylistCreateOutcome::AlreadyExists) => {
            set_status(
                state,
                &catalog_text(&state.application, "playlist_exists"),
                true,
            );
        }
        Ok(apricot_app::PlaylistCreateOutcome::EmptyName) => {}
        Ok(apricot_app::PlaylistCreateOutcome::UnsupportedItem) => {
            set_status(
                state,
                &catalog_text(&state.application, "no_selection"),
                true,
            );
        }
        Err(error) => {
            let message = format!("Playlist was not created: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
        }
    }
}

unsafe fn choose_user_playlist(window: HWND, title_key: &str) -> Option<usize> {
    let (title, prompt, choices, ok_label, cancel_label) = state_mut(window).map(|state| {
        state.modal_open = true;
        (
            catalog_text(&state.application, title_key),
            catalog_text(&state.application, "select_playlist"),
            state
                .application
                .user_playlists()
                .iter()
                .map(|playlist| playlist.title.clone())
                .collect::<Vec<_>>(),
            catalog_text(&state.application, "ok"),
            catalog_text(&state.application, "cancel"),
        )
    })?;
    let result = crate::playlist_dialog_win32::choose(
        window,
        &title,
        &prompt,
        &choices,
        &ok_label,
        &cancel_label,
    );
    let state = state_mut(window)?;
    state.modal_open = false;
    resume_deferred_window_work(window);
    match result {
        Ok(selection) => selection.filter(|index| *index < choices.len()),
        Err(error) => {
            let message = format!("Playlist chooser failed: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
            None
        }
    }
}

unsafe fn add_active_item_to_user_playlist(window: HWND) {
    let Some(item) = active_media_item(window).filter(apricot_core::MediaItem::is_playable) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "no_selection"),
                true,
            );
        }
        return;
    };
    let count = state(window).map_or(0, |state| state.application.user_playlists().len());
    if count == 0 {
        create_user_playlist(window, Some(item));
        return;
    }
    let playlist_index = if count == 1 {
        0
    } else {
        let Some(index) = choose_user_playlist(window, "add_to_playlist") else {
            return;
        };
        index
    };
    add_item_to_user_playlist(window, playlist_index, item);
}

unsafe fn add_item_to_user_playlist(
    window: HWND,
    playlist_index: usize,
    item: apricot_core::MediaItem,
) {
    let title = item.title.clone();
    let result = state_mut(window).map(|state| {
        state
            .application
            .add_item_to_user_playlist(playlist_index, item, unix_timestamp())
    });
    let Some(result) = result else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    match result {
        Ok(apricot_app::PlaylistAddOutcome::Added(_)) => {
            if state.view == MainView::UserPlaylistItems
                && state.current_user_playlist_index == playlist_index
            {
                refresh_user_playlist_items(state, false, false);
            }
            let playlist = state
                .application
                .user_playlists()
                .get(playlist_index)
                .map_or("", |playlist| playlist.title.as_str());
            let message = catalog_text(&state.application, "added_to_playlist")
                .replace("{playlist}", playlist)
                .replace("{title}", &title);
            set_status(state, &message, true);
        }
        Ok(apricot_app::PlaylistAddOutcome::AlreadyPresent) => {
            set_status(
                state,
                &catalog_text(&state.application, "playlist_exists"),
                true,
            );
        }
        Ok(
            apricot_app::PlaylistAddOutcome::MissingPlaylist
            | apricot_app::PlaylistAddOutcome::Unsupported,
        ) => {
            set_status(
                state,
                &catalog_text(&state.application, "no_selection"),
                true,
            );
        }
        Err(error) => {
            let message = format!("Playlist was not updated: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
        }
    }
}

unsafe fn remove_selected_user_playlist(window: HWND) {
    let Some(index) = state(window).and_then(|state| {
        usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()
    }) else {
        return;
    };
    let result = state_mut(window).map(|state| state.application.remove_user_playlist(index));
    match result {
        Some(Ok(Some(_))) => {
            if let Some(state) = state_mut(window) {
                state.current_user_playlist_index =
                    index.min(state.application.user_playlists().len().saturating_sub(1));
                refresh_user_playlists(state, true, false);
                set_status(
                    state,
                    &catalog_text(&state.application, "playlist_removed"),
                    true,
                );
            }
        }
        Some(Ok(None)) | None => {
            if let Some(state) = state(window) {
                set_status(
                    state,
                    &catalog_text(&state.application, "no_playlists"),
                    true,
                );
            }
        }
        Some(Err(error)) => {
            let message = format!("Playlist was not removed: {error}");
            if let Some(state) = state(window) {
                set_status(state, &message, true);
            }
            show_error_message(window, &message);
        }
    }
}

unsafe fn remove_selected_user_playlist_item(window: HWND) {
    let Some((playlist_index, item_index)) = state(window).and_then(|state| {
        usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0)
            .ok()
            .map(|item_index| (state.current_user_playlist_index, item_index))
    }) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "playlist_empty"),
                true,
            );
        }
        return;
    };
    let result = state_mut(window).map(|state| {
        state
            .application
            .remove_user_playlist_item(playlist_index, item_index, unix_timestamp())
    });
    finish_user_playlist_item_removal(window, result);
}

unsafe fn remove_active_item_from_user_playlist(window: HWND) {
    if state(window).is_some_and(|state| state.view == MainView::UserPlaylistItems) {
        remove_selected_user_playlist_item(window);
        return;
    }
    let Some(item) = active_media_item(window) else {
        return;
    };
    let matches = state(window).map_or_else(Vec::new, |state| {
        state.application.user_playlist_matches(&item)
    });
    let Some(playlist_index) = (match matches.as_slice() {
        [] => {
            if let Some(state) = state(window) {
                set_status(
                    state,
                    &catalog_text(&state.application, "not_in_playlist"),
                    true,
                );
            }
            None
        }
        [index] => Some(*index),
        _ => choose_user_playlist_from_indices(window, "remove_from_playlist", &matches),
    }) else {
        return;
    };
    let Some(item_index) = state(window).and_then(|state| {
        let identity = item.copy_location().or_else(|| item.stable_identity())?;
        state
            .application
            .user_playlists()
            .get(playlist_index)?
            .items
            .iter()
            .position(|candidate| {
                candidate
                    .copy_location()
                    .or_else(|| candidate.stable_identity())
                    .as_deref()
                    == Some(&identity)
            })
    }) else {
        return;
    };
    let result = state_mut(window).map(|state| {
        state
            .application
            .remove_user_playlist_item(playlist_index, item_index, unix_timestamp())
    });
    finish_user_playlist_item_removal(window, result);
}

unsafe fn choose_user_playlist_from_indices(
    window: HWND,
    title_key: &str,
    indices: &[usize],
) -> Option<usize> {
    let (title, prompt, choices, ok_label, cancel_label) = state_mut(window).map(|state| {
        state.modal_open = true;
        let choices = indices
            .iter()
            .filter_map(|index| state.application.user_playlists().get(*index))
            .map(|playlist| playlist.title.clone())
            .collect::<Vec<_>>();
        (
            catalog_text(&state.application, title_key),
            catalog_text(&state.application, "select_playlist"),
            choices,
            catalog_text(&state.application, "ok"),
            catalog_text(&state.application, "cancel"),
        )
    })?;
    let result = crate::playlist_dialog_win32::choose(
        window,
        &title,
        &prompt,
        &choices,
        &ok_label,
        &cancel_label,
    );
    let state = state_mut(window)?;
    state.modal_open = false;
    resume_deferred_window_work(window);
    match result {
        Ok(Some(choice)) => indices.get(choice).copied(),
        Ok(None) => None,
        Err(error) => {
            let message = format!("Playlist chooser failed: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
            None
        }
    }
}

unsafe fn finish_user_playlist_item_removal(
    window: HWND,
    result: Option<
        std::result::Result<
            Option<apricot_core::MediaItem>,
            apricot_app::UserPlaylistControllerError,
        >,
    >,
) {
    match result {
        Some(Ok(Some(_))) => {
            if let Some(state) = state_mut(window) {
                if state.view == MainView::UserPlaylistItems {
                    refresh_user_playlist_items(state, true, false);
                }
                set_status(
                    state,
                    &catalog_text(&state.application, "removed_from_playlist"),
                    true,
                );
            }
        }
        Some(Ok(None)) | None => {}
        Some(Err(error)) => {
            let message = format!("Playlist was not updated: {error}");
            if let Some(state) = state(window) {
                set_status(state, &message, true);
            }
            show_error_message(window, &message);
        }
    }
}

unsafe fn play_current_user_playlist(window: HWND, shuffle: bool) {
    let item = state_mut(window).and_then(|state| {
        let item = state
            .application
            .prepare_user_playlist_playback(state.current_user_playlist_index, shuffle)?;
        if let Some(index) = state
            .application
            .user_playlists()
            .get(state.current_user_playlist_index)
            .and_then(|playlist| {
                let identity = item.stable_identity()?;
                playlist
                    .items
                    .iter()
                    .position(|candidate| candidate.stable_identity().as_deref() == Some(&identity))
            })
        {
            state.current_user_playlist_item_index = index;
        }
        Some(item)
    });
    let Some(item) = item else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "playlist_empty"),
                true,
            );
        }
        return;
    };
    start_media_item_with_shuffle(window, item, None, shuffle);
}

unsafe fn add_current_user_playlist_to_queue(window: HWND) {
    if state(window).is_none_or(|state| {
        state
            .application
            .user_playlists()
            .get(state.current_user_playlist_index)
            .is_none_or(|playlist| playlist.items.is_empty())
    }) {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "playlist_empty"),
                true,
            );
        }
        return;
    }
    let result = state_mut(window).map(|state| {
        state
            .application
            .add_user_playlist_to_playback_queue(state.current_user_playlist_index)
    });
    let Some(result) = result else {
        return;
    };
    let Some(state) = state(window) else {
        return;
    };
    match result {
        Ok(Some(outcome)) if outcome.added > 0 => {
            let message = catalog_text(&state.application, "playback_queue_added_count")
                .replace("{count}", &outcome.added.to_string());
            set_status(state, &message, true);
        }
        Ok(Some(_)) => {
            set_status(
                state,
                &catalog_text(&state.application, "playback_queue_exists"),
                true,
            );
        }
        Ok(None) => {}
        Err(error) => {
            let message = format!("Playback queue was not updated: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
        }
    }
}

fn unix_timestamp() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

unsafe fn copy_active_location(window: HWND) {
    let Some(item) = active_media_item(window) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "no_selection"),
                true,
            );
        }
        return;
    };
    let local_media = item.is_local_media();
    let Some(location) = item.copy_location() else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "no_selection"),
                true,
            );
        }
        return;
    };
    copy_text_and_announce(
        window,
        &location,
        if local_media {
            "path_copied"
        } else {
            "url_copied"
        },
    );
}

unsafe fn copy_current_timestamp_link(window: HWND) {
    let Some((url, message_key)) = state(window).and_then(|state| {
        let session = state.application.player_session();
        session
            .current_item()?
            .youtube_url_at_timestamp(session.position_seconds())
            .map(|url| (url.to_string(), "timestamp_url_copied"))
    }) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "timestamp_url_unavailable"),
                true,
            );
        }
        return;
    };
    copy_text_and_announce(window, &url, message_key);
}

unsafe fn copy_active_stream_url(window: HWND) {
    let Some(item) = active_media_item(window) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "no_selection"),
                true,
            );
        }
        return;
    };
    if item.is_local_media() {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "direct_media_link_unavailable_local"),
                true,
            );
        }
        return;
    }
    if let Some(stream_url) = item.stream_url.as_ref() {
        copy_text_and_announce(window, stream_url.as_str(), "stream_url_copied");
        return;
    }
    if item.source == apricot_core::MediaSource::Youtube {
        start_youtube_resolve(window, &item, YoutubeResolvePurpose::CopyStreamUrl);
        return;
    }
    if item.source == apricot_core::MediaSource::Direct
        && let Some(url) = item.url.as_ref()
    {
        copy_text_and_announce(window, url.as_str(), "stream_url_copied");
        return;
    }
    if let Some(state) = state(window) {
        let message = catalog_text(&state.application, "stream_url_failed").replace(
            "{error}",
            "this source has not exposed a direct media URL yet",
        );
        set_status(state, &message, true);
    }
}

unsafe fn copy_text_and_announce(window: HWND, text: &str, success_key: &str) {
    let Some(state) = state(window) else {
        return;
    };
    match crate::clipboard_win32::copy_text(window, text) {
        Ok(()) => set_status(state, &catalog_text(&state.application, success_key), true),
        Err(error) => {
            let message = format!("Could not copy to the clipboard: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
        }
    }
}

unsafe fn add_active_item_to_playback_queue(window: HWND) {
    let Some(item) = active_media_item(window) else {
        return;
    };
    let title = item.title.clone();
    let Some(state) = state_mut(window) else {
        return;
    };
    match state.application.add_to_playback_queue(item) {
        Ok(apricot_app::QueueAddOutcome::Added) => {
            let message =
                catalog_text(&state.application, "playback_queue_added").replace("{title}", &title);
            set_status(state, &message, true);
        }
        Ok(apricot_app::QueueAddOutcome::AlreadyPresent) => {
            let message = catalog_text(&state.application, "playback_queue_already_added")
                .replace("{title}", &title);
            set_status(state, &message, true);
        }
        Ok(apricot_app::QueueAddOutcome::Unplayable) => {
            let message = format!("{title} cannot be added to the playback queue");
            set_status(state, &message, true);
        }
        Err(error) => {
            let message = format!("Playback queue was not updated: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
        }
    }
}

unsafe fn remove_active_item_from_playback_queue(window: HWND) {
    let Some(item) = active_media_item(window) else {
        return;
    };
    let title = item.title.clone();
    let Some(state) = state_mut(window) else {
        return;
    };
    match state.application.remove_from_playback_queue(&item) {
        Ok(true) => {
            let message = catalog_text(&state.application, "playback_queue_removed")
                .replace("{title}", &title);
            set_status(state, &message, true);
        }
        Ok(false) => set_status(
            state,
            &catalog_text(&state.application, "playback_queue_not_found"),
            true,
        ),
        Err(error) => {
            let message = format!("Playback queue was not updated: {error}");
            set_status(state, &message, true);
            show_error_message(window, &message);
        }
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

unsafe fn announce_player_format(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let message = state
        .application
        .player_format_status()
        .unwrap_or_else(|| catalog_text(&state.application, "no_player"));
    set_status(state, &message, true);
}

unsafe fn show_player_details(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(details) = state.application.player_details_text() else {
        let message = catalog_text(&state.application, "details_unavailable");
        set_status(state, &message, true);
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let labels = crate::details_win32::DetailsDialogLabels {
        title: catalog.text("video_details").to_owned(),
        copy: catalog.text("copy_details").to_owned(),
        copied: catalog.text("details_copied").to_owned(),
        back: catalog.text("back").to_owned(),
    };
    state.modal_open = true;
    let _ = crate::details_win32::show(window, details, &labels);
    let Some(state) = state_mut(window) else {
        return;
    };
    state.modal_open = false;
    resume_deferred_window_work(window);
    let message = catalog_text(&state.application, "details_closed");
    set_status(state, &message, false);
    let _ = SetFocus(Some(state.player_controls.initial_focus()));
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
    let _ = KillTimer(Some(window), CONTROLLED_REPEAT_TIMER_ID);
    state.controlled_repeat = None;
    let generation = state.application.player_session().generation();
    if let Some(runtime) = state.playback.as_ref() {
        let _ = runtime.close(generation);
    }
    persist_current_playback_position(state);
    state.application.close_player_session();
    state.pending_queued_start = None;
    stop_playback_timer(window);
    let title = wide("ApricotPlayer 2 Beta");
    let _ = SetWindowTextW(window, PCWSTR(title.as_ptr()));
}

unsafe fn persist_current_playback_position(state: &mut WindowState) {
    if let Err(error) = state.application.save_current_playback_position() {
        set_status(
            state,
            &format!("Playback position was not saved: {error}"),
            false,
        );
    }
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
    start_media_item(
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
        None,
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

unsafe fn open_media_folder(window: HWND) {
    stop_controlled_repeat(window);
    let title = state(window).map_or_else(
        || "Choose a folder with audio or video files".to_owned(),
        |state| {
            apricot_app::embedded_catalog(&state.application.settings().language)
                .text("select_media_folder")
                .to_owned()
        },
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = true;
    }
    let selection = crate::folder_dialog_win32::choose_media_folder(window, &title);
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    let Some(path) = selection else {
        if let Some(state) = state(window) {
            let _ = SetFocus(Some(active_primary_control(state)));
        }
        return;
    };
    start_local_folder_scan(window, path);
}

unsafe fn start_local_folder_scan(window: HWND, path: PathBuf) {
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_local_folder_scan(window, state);
    state.next_local_folder_generation = state.next_local_folder_generation.wrapping_add(1).max(1);
    let generation = state.next_local_folder_generation;
    let worker_path = path.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = scan_local_media_folder_with_cancel(&worker_path, &worker_cancelled)
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
    state.pending_local_folder_scan = Some(PendingLocalFolderScan {
        generation,
        path,
        cancelled,
        receiver,
    });
    set_status(state, "Loading folder...", true);
    let _ = SetTimer(
        Some(window),
        LOCAL_FOLDER_TIMER_ID,
        YOUTUBE_TIMER_INTERVAL_MS,
        None,
    );
}

unsafe fn poll_local_folder_scan(window: HWND) {
    let update = {
        let Some(state) = state(window) else {
            return;
        };
        if state.modal_open {
            return;
        }
        let Some(pending) = state.pending_local_folder_scan.as_ref() else {
            let _ = KillTimer(Some(window), LOCAL_FOLDER_TIMER_ID);
            return;
        };
        match pending.receiver.try_recv() {
            Ok(result) => Some((pending.generation, pending.path.clone(), result)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some((
                pending.generation,
                pending.path.clone(),
                Err("Local folder scan stopped unexpectedly".to_owned()),
            )),
        }
    };
    let Some((generation, path, result)) = update else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    if state
        .pending_local_folder_scan
        .as_ref()
        .is_none_or(|pending| pending.generation != generation)
    {
        return;
    }
    state.pending_local_folder_scan = None;
    let _ = KillTimer(Some(window), LOCAL_FOLDER_TIMER_ID);
    match result {
        Ok(items) if !items.is_empty() => {
            state.application.load_local_folder(path, items);
            state.application.navigate_main_menu();
            state
                .application
                .navigate_to(RouteFrame::new(Route::LocalFolder));
            state.view = MainView::LocalFolder;
            refresh_local_folder(state, true, true);
            layout_controls_state(window, state);
        }
        Ok(_) => {
            let message = catalog_text(&state.application, "folder_no_media");
            set_status(state, &message, true);
            show_error_message(window, &message);
            let _ = SetFocus(Some(active_primary_control(state)));
        }
        Err(error) => {
            set_status(state, &error, true);
            show_error_message(window, &error);
            let _ = SetFocus(Some(active_primary_control(state)));
        }
    }
}

unsafe fn cancel_local_folder_scan(window: HWND, state: &mut WindowState) {
    if let Some(pending) = state.pending_local_folder_scan.take() {
        pending.cancelled.store(true, AtomicOrdering::Relaxed);
        let _ = KillTimer(Some(window), LOCAL_FOLDER_TIMER_ID);
    }
}

unsafe fn show_action_finder(window: HWND) {
    stop_controlled_repeat(window);
    let Some(main_state) = state_mut(window) else {
        return;
    };
    let model = main_state
        .application
        .action_finder_model(ActionFinderContext::default());
    main_state.modal_open = true;
    let outcome = crate::action_finder_win32::show(window, model);
    if let Some(main_state) = state_mut(window) {
        main_state.modal_open = false;
    }
    resume_deferred_window_work(window);
    match outcome {
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

unsafe fn show_playback_queue(window: HWND) {
    stop_controlled_repeat(window);
    let Some(main_state) = state_mut(window) else {
        return;
    };
    if main_state.application.playback_queue().is_empty() {
        set_status(
            main_state,
            &catalog_text(&main_state.application, "playback_queue_empty"),
            true,
        );
        return;
    }
    let catalog = apricot_app::embedded_catalog(&main_state.application.settings().language);
    let labels = crate::playback_queue_win32::PlaybackQueueDialogLabels {
        title: catalog.text("playback_queue").to_owned(),
        instructions: catalog.text("playback_queue_instructions").to_owned(),
        empty: catalog.text("playback_queue_empty").to_owned(),
        play: catalog.text("play").to_owned(),
        move_up: catalog.text("move_up").to_owned(),
        move_down: catalog.text("move_down").to_owned(),
        remove: catalog.text("remove_from_playback_queue").to_owned(),
        clear: catalog.text("clear_playback_queue").to_owned(),
        back: catalog.text("back").to_owned(),
        channel: catalog.text("channel").to_owned(),
        audio: catalog.text("download_audio_mode").to_owned(),
        video: catalog.text("video").to_owned(),
        live_stream: catalog.text("live_stream").to_owned(),
        playlist: catalog.text("playlist").to_owned(),
        channel_kind: catalog.text("channel").to_owned(),
        podcast_feed: catalog.text("rss_feeds").to_owned(),
        podcast_episode: catalog.text("podcast_episode").to_owned(),
        movie: catalog.text("movie").to_owned(),
        tv_show: catalog.text("tv_show").to_owned(),
        tv_episode: catalog.text("episode").to_owned(),
        unknown: catalog.text("unknown").to_owned(),
    };
    let items = main_state.application.playback_queue().items().to_vec();
    main_state.modal_open = true;
    let outcome = crate::playback_queue_win32::show(window, items, labels);
    let Some(main_state) = state_mut(window) else {
        return;
    };
    main_state.modal_open = false;
    resume_deferred_window_work(window);
    match outcome {
        Ok(outcome) => {
            if outcome.changed
                && let Err(error) = main_state.application.replace_playback_queue(outcome.items)
            {
                let message = format!("Playback queue was not updated: {error}");
                set_status(main_state, &message, true);
                show_error_message(window, &message);
                let _ = SetFocus(Some(active_primary_control(main_state)));
                return;
            }
            if main_state.view == MainView::MainMenu {
                refresh_main_menu(main_state);
            }
            if let Some(item) = outcome.play {
                start_media_item(window, item, Some(QueueStartMode::Matching));
                return;
            }
        }
        Err(error) => {
            let message = format!("Playback queue did not open: {error}");
            show_error_message(window, &message);
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
    stop_controlled_repeat(window);
    let settings_result = {
        let Some(state) = state_mut(window) else {
            return;
        };
        if state.settings_open {
            return;
        }
        state.settings_open = true;
        state.modal_open = true;
        crate::settings_win32::show(window, &mut state.application)
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    state.settings_open = false;
    state.modal_open = false;
    resume_deferred_window_work(window);
    match state.view {
        MainView::MainMenu => refresh_main_menu(state),
        MainView::Results => refresh_results(state, false),
        MainView::LocalFolder => refresh_local_folder(state, false, false),
        MainView::Favorites | MainView::History => {
            refresh_media_collection(state, false, false);
        }
        MainView::NotificationCenter => {
            refresh_notification_center(state, false, false, None);
        }
        MainView::UserPlaylists => refresh_user_playlists(state, false, false),
        MainView::UserPlaylistItems => refresh_user_playlist_items(state, false, false),
        MainView::Search | MainView::DirectLink => {}
        MainView::Player => refresh_player(window, state, false, true),
    }
    layout_controls_state(window, state);
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
        if state.settings_open || state.modal_open {
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

unsafe fn resume_deferred_window_work(window: HWND) {
    let _ = PostMessageW(Some(window), WM_PROCESS_ACTIVATION, WPARAM(0), LPARAM(0));
}

unsafe fn refresh_main_menu(state: &mut WindowState) {
    set_open_button_label(state, "open");
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

fn active_primary_control(state: &WindowState) -> HWND {
    match state.view {
        MainView::Search | MainView::DirectLink => state.search_edit,
        MainView::MainMenu
        | MainView::Results
        | MainView::LocalFolder
        | MainView::Favorites
        | MainView::History
        | MainView::NotificationCenter
        | MainView::UserPlaylists
        | MainView::UserPlaylistItems => state.list,
        MainView::Player => state.player_controls.initial_focus(),
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        MainView, SEEK_HOLD_DELAY_MS, SEEK_HOLD_INTERVAL_MS, controlled_repeat_timing,
        copy_wide_array, media_resolve_backend, notification_label, resolved_playback_item,
        view_has_back_button, view_has_collection_remove,
    };
    use apricot_app::AppNotification;
    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_media::YoutubeBackend;

    #[test]
    fn tray_text_is_cleared_truncated_and_null_terminated() {
        let mut target = [u16::MAX; 5];
        copy_wide_array(&mut target, "abcdef");
        assert_eq!(target, ['a' as u16, 'b' as u16, 'c' as u16, 'd' as u16, 0]);

        copy_wide_array(&mut target, "x");
        assert_eq!(target, ['x' as u16, 0, 0, 0, 0]);
    }

    #[test]
    fn notification_label_matches_the_python_accessible_field_order() {
        let mut item = youtube_item("video");
        item.title = "Track title".to_owned();
        item.channel = "Channel name".to_owned();
        let notification = AppNotification::new(
            "subscription_video",
            "New video",
            "A subscribed channel published a video.",
            Some(item),
            0.0,
        );
        let catalog = apricot_app::embedded_catalog("en");

        assert_eq!(
            notification_label(&notification, &catalog),
            "New video | A subscribed channel published a video. | Track title | Channel: Channel name"
        );
    }

    #[test]
    fn notification_center_shows_back_without_a_python_incompatible_remove_button() {
        assert!(view_has_back_button(MainView::NotificationCenter));
        assert!(!view_has_collection_remove(MainView::NotificationCenter));
    }

    #[test]
    fn controlled_repeat_keeps_seek_fixed_and_speed_pitch_configurable() {
        assert_eq!(
            controlled_repeat_timing("player_seek_forward", 50, 20),
            (SEEK_HOLD_DELAY_MS, SEEK_HOLD_INTERVAL_MS)
        );
        assert_eq!(
            controlled_repeat_timing("player_speed_up", 240, 75),
            (240, 75)
        );
        assert_eq!(
            controlled_repeat_timing("player_pitch_down", 5, 2_000),
            (50, 500)
        );
    }

    #[test]
    fn generic_direct_links_use_ytdlp_but_youtube_links_honor_the_setting() {
        let generic = MediaItem::from_direct_link("https://media.example/song.mp3")
            .expect("generic direct link");
        assert_eq!(generic.source, MediaSource::Direct);
        assert_eq!(
            media_resolve_backend(&generic, "rusty_ytdl"),
            YoutubeBackend::YtDlp
        );

        let youtube = MediaItem::from_direct_link("https://youtu.be/dQw4w9WgXcQ")
            .expect("YouTube direct link");
        assert_eq!(
            media_resolve_backend(&youtube, "rusty_ytdl"),
            YoutubeBackend::RustyYtdl
        );
        assert_eq!(
            media_resolve_backend(&youtube, "yt_dlp"),
            YoutubeBackend::YtDlp
        );
    }

    #[test]
    fn resolved_sequence_item_keeps_legacy_python_identity() {
        let original = youtube_item("https://www.youtube.com/watch?v=abcdefghijk");
        let mut resolved = original.clone();
        resolved.id = MediaId("abcdefghijk".to_owned());
        resolved.stream_url = Some("https://cdn.example/video".parse().expect("stream URL"));

        let merged = resolved_playback_item(resolved, &original, true);
        assert_eq!(merged.id, original.id);
        assert_eq!(merged.url, original.url);
        assert!(merged.stream_url.is_some());
    }

    #[test]
    fn standalone_youtube_resolution_keeps_the_resolved_identity() {
        let original = youtube_item("old-url-identity");
        let mut resolved = original.clone();
        resolved.id = MediaId("abcdefghijk".to_owned());

        let merged = resolved_playback_item(resolved, &original, false);
        assert_eq!(merged.id.0, "abcdefghijk");
    }

    fn youtube_item(id: &str) -> MediaItem {
        MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: "Video".to_owned(),
            url: Some(
                "https://www.youtube.com/watch?v=abcdefghijk"
                    .parse()
                    .expect("URL"),
            ),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: std::collections::BTreeMap::new(),
        }
    }
}
