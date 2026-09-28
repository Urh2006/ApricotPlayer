//! Isolated unsafe Win32 boundary for the production Windows shell.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{
    collections::{HashMap, HashSet, VecDeque},
    ffi::c_void,
    fs,
    io::Read,
    mem::size_of,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
};

use crate::{
    bookmark_dialog_win32::{
        BookmarkDialogEntry, BookmarkDialogLabels, BookmarkDialogRequest, BookmarkDialogResponse,
    },
    download_progress_win32::DownloadProgressWindow,
    download_win32::{DownloadWorkerUpdate, spawn_batch_download, spawn_download},
    player_controls_win32::{PlayerControlActivation, PlayerControls},
    podcast_win32::{PendingPodcastWork, PodcastWorkResult},
};
use apricot_app::{
    ActivationRequest, ActiveDownload, Application, ContextCommand, ContextMenuContext,
    ContextMenuEntry, DownloadChoice, DownloadTaskKind, DownloadTaskStatus, MainMenuModel,
    PlaybackPhase, PlayerNavigationOutcome, RssFeedAddOutcome, SearchApplyOutcome, SearchWork,
    SearchWorkKind, SessionToggle, SubscriptionAddOutcome, SubscriptionCheckResult,
    SubscriptionRemoveOutcome, YOUTUBE_TRENDING_CATEGORIES, YOUTUBE_TRENDING_COUNTRIES,
    YoutubeCollectionApplyOutcome, YoutubeCollectionKind, YoutubeCollectionPhase,
    YoutubeCollectionWork, YoutubeCollectionWorkKind, YoutubeSearchKind, YoutubeTrendingWork,
    context_menu, youtube_trending_category_id, youtube_trending_public_url,
};
use apricot_core::{
    Route, RouteFrame,
    action::{ActionScope, RepeatPolicy},
    shortcut::{ShortcutContext, ShortcutKey, action_for_shortcut},
};
use apricot_media::{
    PodcastDirectoryItem, YoutubeBackend, YoutubeFormat, YoutubeSessionConfig,
    YoutubeStreamPreference, select_youtube_playback_formats,
};
use apricot_platform::{
    DownloadEvent, DownloadMode, DownloadOptions, DownloadPhase, DownloadRequest,
    VideoDownloadFormat, YoutubeDataApiClient, YoutubeSearchService, YoutubeSearchServiceUpdate,
    scan_local_media_folder_with_cancel,
};
use apricot_playback::{
    InitialPlaybackState, MpvCacheConfig, MpvLaunchOptions, MpvVideoMode, PitchMode,
    PlaybackCommand, PlaybackEvent, PlaybackRuntime, RepeatMode, SpeedAudioMode,
    audio_filter_chain, is_default_rate, mpv_pitch_property,
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            DEFAULT_GUI_FONT, GetMonitorInfoW, GetStockObject, MONITOR_DEFAULTTONEAREST,
            MONITORINFO, MonitorFromWindow,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::InitCommonControls,
            Input::KeyboardAndMouse::{
                EnableWindow, GetAsyncKeyState, GetFocus, SetFocus, VK_CONTROL, VK_DOWN, VK_END,
                VK_MENU, VK_RETURN, VK_SHIFT, VK_TAB,
            },
            Shell::{
                DefSubclassProc, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
                NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
                RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, BS_DEFPUSHBUTTON, CBN_SELCHANGE, CBS_DROPDOWNLIST, CW_USEDEFAULT,
                CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
                DispatchMessageW, ES_AUTOHSCROLL, GA_ROOTOWNER, GWL_STYLE, GetAncestor,
                GetClientRect, GetCursorPos, GetForegroundWindow, GetMessageW, GetParent,
                GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, GetWindowTextLengthW,
                GetWindowTextW, HMENU, HWND_TOP, IDC_ARROW, IDI_APPLICATION, IDYES, IsChild,
                IsDialogMessageW, KillTimer, LB_ADDSTRING, LB_DELETESTRING, LB_GETCOUNT,
                LB_GETCURSEL, LB_INSERTSTRING, LB_RESETCONTENT, LB_SETCURSEL, LBN_DBLCLK,
                LBN_SELCHANGE, LBS_NOTIFY, LoadCursorW, LoadIconW, MB_ICONINFORMATION, MB_OK,
                MB_YESNO, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, MessageBoxW,
                MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
                SW_HIDE, SW_SHOW, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
                SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
                SetWindowPlacement, SetWindowPos, SetWindowTextW, ShowWindow, TPM_LEFTALIGN,
                TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WINDOWPLACEMENT, WM_APP, WM_CLOSE, WM_COMMAND,
                WM_CONTEXTMENU, WM_COPYDATA, WM_CREATE, WM_DESTROY, WM_KEYDOWN, WM_KEYUP,
                WM_LBUTTONDBLCLK, WM_NCDESTROY, WM_RBUTTONUP, WM_SETFONT, WM_SIZE, WM_SYSKEYUP,
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
const ID_TRENDING_COUNTRY: usize = 1023;
const ID_TRENDING_CATEGORY: usize = 1024;
const ID_LOAD_TRENDING: usize = 1025;
const ID_SUBSCRIPTION_CHECK: usize = 1026;
const ID_SUBSCRIPTION_NEW: usize = 1027;
const ID_SUBSCRIPTION_FILTER: usize = 1028;
const ID_SUBSCRIPTION_SET_CATEGORY: usize = 1029;
const ID_RSS_SEARCH: usize = 1030;
const ID_RSS_CATEGORIES: usize = 1031;
const ID_RSS_ADD: usize = 1032;
const ID_RSS_REFRESH: usize = 1033;
const ID_RSS_FILTER: usize = 1034;
const ID_RSS_SET_CATEGORY: usize = 1035;
const ID_RSS_IMPORT: usize = 1036;
const ID_RSS_EXPORT: usize = 1037;
const ID_RSS_DOWNLOAD_FEED: usize = 1038;
const ID_RSS_SPEED: usize = 1039;
const ID_RSS_TOGGLE_PLAYED: usize = 1040;
const ID_RSS_CLEAR_PROGRESS: usize = 1041;
const ID_PODCAST_ADD: usize = 1042;
const ID_OPEN_BROWSER: usize = 1043;
const ID_RSS_DOWNLOAD_EPISODE: usize = 1044;
const ID_DOWNLOAD_ALL_AUDIO: usize = 1045;
const ID_DOWNLOAD_ALL_VIDEO: usize = 1046;
const ID_DOWNLOAD_CANCEL: usize = 1047;
const ID_DOWNLOAD_CANCEL_ALL: usize = 1048;
const ID_PLAYLIST_DOWNLOAD: usize = 1049;
const ID_CONTEXT_PLAY: usize = 1101;
const ID_CONTEXT_ADD_TO_QUEUE: usize = 1104;
const ID_CONTEXT_REMOVE_FROM_QUEUE: usize = 1105;
const ID_CONTEXT_COPY_LOCATION: usize = 1108;
const ID_CONTEXT_COLLECTION_REMOVE: usize = 1114;
const ID_CONTEXT_ADD_TO_PLAYLIST: usize = 1119;
const ID_CONTEXT_CLEAR_NOTIFICATIONS: usize = 1123;
const ID_CONTEXT_SUBSCRIPTION_OPEN: usize = 1130;
const ID_CONTEXT_SUBSCRIPTION_NEW: usize = 1131;
const ID_CONTEXT_SUBSCRIPTION_CHECK: usize = 1132;
const ID_CONTEXT_SUBSCRIPTION_SET_CATEGORY: usize = 1133;
const ID_CONTEXT_SUBSCRIPTION_FILTER: usize = 1134;
const ID_CONTEXT_UNSUBSCRIBE: usize = 1135;
const ID_CONTEXT_RSS_OPEN: usize = 1137;
const ID_CONTEXT_RSS_REFRESH: usize = 1138;
const ID_CONTEXT_RSS_SPEED: usize = 1139;
const ID_CONTEXT_RSS_SET_CATEGORY: usize = 1140;
const ID_CONTEXT_RSS_FILTER: usize = 1141;
const ID_CONTEXT_RSS_REMOVE: usize = 1142;
const ID_CONTEXT_RSS_TOGGLE_PLAYED: usize = 1143;
const ID_CONTEXT_RSS_CLEAR_PROGRESS: usize = 1144;
const ID_CONTEXT_RSS_DOWNLOAD_FEED: usize = 1145;
const ID_CONTEXT_RSS_DOWNLOAD_EPISODE: usize = 1146;
const ID_CONTEXT_OPEN_BROWSER: usize = 1147;
const ID_CONTEXT_PODCAST_ADD: usize = 1148;
const ID_CONTEXT_RSS_QUEUE_EPISODE: usize = 1149;
const ID_CONTEXT_DOWNLOAD_AUDIO: usize = 1150;
const ID_CONTEXT_DOWNLOAD_VIDEO: usize = 1151;
const ID_CONTEXT_DOWNLOAD_SELECTED: usize = 1152;
const ID_CONTEXT_DOWNLOAD_ALL_AUDIO: usize = 1153;
const ID_CONTEXT_DOWNLOAD_ALL_VIDEO: usize = 1154;
const ID_CONTEXT_DOWNLOAD_CANCEL: usize = 1155;
const ID_CONTEXT_DOWNLOAD_CANCEL_ALL: usize = 1156;
const ID_CONTEXT_DOWNLOAD_REMOVE_QUEUED: usize = 1157;
/// First command id of menus built from `apricot_app::context_menu`.
const ID_CONTEXT_MODEL_FIRST: usize = 2001;
const WM_PROCESS_ACTIVATION: u32 = WM_APP + 1;
const WM_TRAY_ICON: u32 = WM_APP + 2;
const WM_SHOW_DOWNLOAD_DETAILS: u32 = WM_APP + 3;
const WM_SYNC_FULLSCREEN: u32 = WM_APP + 4;
const YOUTUBE_TIMER_ID: usize = 1;
const YOUTUBE_TIMER_INTERVAL_MS: u32 = 25;
const YOUTUBE_METADATA_BATCH_SIZE: usize = 5;
const YOUTUBE_API_METADATA_BATCH_SIZE: usize = 50;
const PLAYBACK_TIMER_ID: usize = 2;
const PLAYBACK_TIMER_INTERVAL_MS: u32 = 25;
const CONTROLLED_REPEAT_TIMER_ID: usize = 3;
const LOCAL_FOLDER_TIMER_ID: usize = 4;
const SUBSCRIPTION_TIMER_ID: usize = 5;
const RSS_TIMER_ID: usize = 6;
const DOWNLOAD_TIMER_ID: usize = 7;
const CHAPTER_TIMER_ID: usize = 8;
const RELATED_TIMER_ID: usize = 9;
const DOWNLOAD_TIMER_INTERVAL_MS: u32 = 50;
const SEEK_HOLD_DELAY_MS: u32 = 180;
const SEEK_HOLD_INTERVAL_MS: u32 = 110;
const PODCAST_GENRES: [(&str, u32); 10] = [
    ("genre_arts", 1301),
    ("genre_business", 1304),
    ("genre_comedy", 1303),
    ("genre_education", 1307),
    ("genre_music", 1310),
    ("genre_news", 1311),
    ("genre_science", 1315),
    ("genre_sports", 1316),
    ("genre_technology", 1318),
    ("genre_true_crime", 1324),
];
const PODCAST_SPEED_STEPS: [f64; 19] = [
    0.25, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 1.0, 1.1, 1.2, 1.25, 1.3, 1.4, 1.5, 1.75, 2.0, 2.5, 3.0,
    4.0,
];
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_RESETCONTENT: u32 = 0x014B;
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
    Trending,
    DirectLink,
    Results,
    YoutubeCollection,
    LocalFolder,
    Favorites,
    History,
    NotificationCenter,
    Subscriptions,
    RssFeeds,
    RssItems,
    PodcastSearchResults,
    PodcastCategories,
    UserPlaylists,
    UserPlaylistItems,
    DownloadQueue,
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

#[derive(Clone, Debug, PartialEq)]
struct PendingYoutubePlaylistPlayback {
    token: u64,
    shuffle: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct PendingYoutubeTrending {
    work: YoutubeTrendingWork,
    api_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum PendingYoutubeListWork {
    Search(SearchWork),
    Trending(PendingYoutubeTrending),
    Collection(YoutubeCollectionWork),
    PlaylistPlayback(PendingYoutubePlaylistPlayback),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum YoutubeMetadataScope {
    Search(u64),
    Collection(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingYoutubeMetadata {
    token: u64,
    scope: YoutubeMetadataScope,
}

struct PendingYoutubeApiMetadata {
    scope: YoutubeMetadataScope,
    urls: Vec<String>,
    receiver: Receiver<std::result::Result<Vec<apricot_core::MediaItem>, String>>,
}

struct PendingYoutubeTrendingApi {
    generation: u64,
    receiver: Receiver<std::result::Result<Vec<apricot_core::MediaItem>, String>>,
}

struct PendingSubscriptionCheck {
    manual: bool,
    queue: VecDeque<(String, String)>,
    current: Option<(u64, String, String)>,
    results: Vec<SubscriptionCheckResult>,
    errors: Vec<String>,
}

impl PendingYoutubeListWork {
    const fn token(&self) -> u64 {
        match self {
            Self::Search(work) => work.generation,
            Self::Trending(pending) => pending.work.generation,
            Self::Collection(work) => work.generation,
            Self::PlaylistPlayback(work) => work.token,
        }
    }
}

#[derive(Clone, Copy)]
struct ControlledRepeatState {
    action_id: &'static str,
    virtual_key: usize,
    chord: apricot_core::shortcut::ShortcutChord,
}

struct ClipPreview {
    generation: u64,
}

struct PendingChapters {
    generation: u64,
    // None opens the list; Some selects the next/previous chapter.
    direction: Option<bool>,
    receiver: Receiver<Vec<serde_json::Value>>,
}

/// Python `fetch_related_and_play_next` running in the background.
struct PendingRelatedVideos {
    generation: u64,
    // Ctrl+Shift+PageDown announces a missing related video; the end of
    // playback falls back to the next item instead.
    manual: bool,
    receiver: Receiver<std::result::Result<Vec<apricot_core::MediaItem>, String>>,
}

/// Window style and placement to restore after full screen.
struct FullscreenRestore {
    style: isize,
    placement: WINDOWPLACEMENT,
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
    trending_country_label: HWND,
    trending_country: HWND,
    trending_category_label: HWND,
    trending_category: HWND,
    load_trending: HWND,
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
    subscription_check: HWND,
    subscription_new: HWND,
    subscription_filter: HWND,
    subscription_set_category: HWND,
    rss_search: HWND,
    rss_categories: HWND,
    rss_add: HWND,
    rss_refresh: HWND,
    rss_filter: HWND,
    rss_set_category: HWND,
    rss_import: HWND,
    rss_export: HWND,
    rss_download_feed: HWND,
    rss_speed: HWND,
    rss_toggle_played: HWND,
    rss_clear_progress: HWND,
    podcast_add: HWND,
    open_browser: HWND,
    rss_download_episode: HWND,
    playlist_create: HWND,
    playlist_play_all: HWND,
    playlist_shuffle: HWND,
    playlist_add_all_to_queue: HWND,
    playlist_download: HWND,
    download_all_audio: HWND,
    download_all_video: HWND,
    download_cancel: HWND,
    download_cancel_all: HWND,
    video_host: HWND,
    player_controls: PlayerControls,
    status: HWND,
    announcer: crate::announcement_win32::WindowsAnnouncer,
    model: MainMenuModel,
    application: Application,
    settings_open: bool,
    modal_open: bool,
    tray_icon_added: bool,
    last_activated_menu_item: Option<&'static str>,
    lifecycle: WindowLifecycle,
    taskbar_created_message: u32,
    view: MainView,
    youtube_search: YoutubeSearchService,
    youtube_metadata: YoutubeSearchService,
    youtube_subscriptions: YoutubeSearchService,
    pending_youtube_work: Option<PendingYoutubeListWork>,
    pending_youtube_resolve: Option<PendingYoutubeResolve>,
    pending_youtube_metadata: Option<PendingYoutubeMetadata>,
    pending_youtube_api_metadata: Option<PendingYoutubeApiMetadata>,
    pending_youtube_trending_api: Option<PendingYoutubeTrendingApi>,
    pending_subscription_check: Option<PendingSubscriptionCheck>,
    pending_podcast_work: Option<PendingPodcastWork>,
    podcast_search_results: Vec<PodcastDirectoryItem>,
    podcast_search_query: String,
    current_rss_feed_index: usize,
    current_rss_item_index: usize,
    rss_visible_item_count: usize,
    hydrated_youtube_urls: HashSet<String>,
    youtube_api_metadata_disabled_scopes: HashSet<YoutubeMetadataScope>,
    deferred_youtube_metadata_rows: HashSet<usize>,
    last_download_shortcut: Option<DownloadShortcutPress>,
    result_column_cursor: apricot_app::result_columns::ResultColumnCursor,
    pending_player_navigation: Option<i32>,
    pending_queued_start: Option<PendingQueuedStart>,
    next_youtube_operation_token: u64,
    playback: Option<PlaybackRuntime>,
    controlled_repeat: Option<ControlledRepeatState>,
    clip_preview: Option<ClipPreview>,
    pending_local_folder_scan: Option<PendingLocalFolderScan>,
    next_local_folder_generation: u64,
    current_user_playlist_index: usize,
    current_user_playlist_item_index: usize,
    download_sender: SyncSender<DownloadWorkerUpdate>,
    download_receiver: Receiver<DownloadWorkerUpdate>,
    download_cancellations: HashMap<u64, Arc<AtomicBool>>,
    download_progress_reports: HashMap<u64, DownloadProgressReport>,
    download_progress_window: Option<DownloadProgressWindow>,
    download_progress_task_ids: HashSet<u64>,
    download_progress_task_id: Option<u64>,
    clip_exports: Vec<Receiver<std::result::Result<PathBuf, String>>>,
    pending_chapters: Option<PendingChapters>,
    pending_related: Option<PendingRelatedVideos>,
    fullscreen_restore: Option<FullscreenRestore>,
}

pub fn run_application(application: Application, version: &str, start_hidden: bool) -> Result<()> {
    // SAFETY: The window, state pointer, controls, and message loop are confined
    // to this thread. Dynamic UTF-16 buffers outlive each Win32 call that uses
    // them, and owned state is released exactly once during WM_DESTROY.
    unsafe { run_win32(application, version, start_hidden) }
}

unsafe fn run_win32(application: Application, version: &str, start_hidden: bool) -> Result<()> {
    InitCommonControls();
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
    crate::download_progress_win32::register()?;

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
    let initial_state = match create_controls(window, instance, application) {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let initial_focus = initial_state.list;
    SetWindowLongPtrW(
        window,
        WINDOW_LONG_PTR_INDEX(0),
        Box::into_raw(Box::new(initial_state)) as isize,
    );
    layout_controls(window);
    if start_hidden {
        hide_to_tray(window, false);
    } else {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetFocus(Some(initial_focus));
        // Python `setup_taskbar_icon`: the icon stays for the whole session.
        add_tray_icon(window);
        let startup_announcement =
            state_mut(window).and_then(|state| state.application.take_startup_announcement());
        if let Some(key) = startup_announcement {
            announce_player_text(window, key, &[]);
        }
    }
    process_pending_activations(window);
    configure_subscription_timer(window);
    check_subscriptions_if_due(window);
    configure_rss_timer(window);
    refresh_rss_feeds_on_startup(window);

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
        if state(window)
            .and_then(|state| state.download_progress_window)
            .is_some_and(|progress_window| progress_window.handles_dialog_message(&message))
        {
            continue;
        }
        if handle_view_tab_message(window, &message) {
            continue;
        }
        if handle_text_entry_enter(window, &message) {
            continue;
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

/// Python `on_char_hook`: Enter in the search or direct link field runs the
/// search or the configured direct link action. `IsDialogMessageW` would turn
/// it into an unhandled IDOK command before the field ever saw the key.
unsafe fn handle_text_entry_enter(window: HWND, message: &MSG) -> bool {
    if message.message != WM_KEYDOWN || message.wParam.0 != usize::from(VK_RETURN.0) {
        return false;
    }
    if !state(window).is_some_and(|state| {
        matches!(state.view, MainView::Search | MainView::DirectLink)
            && message.hwnd == state.search_edit
    }) {
        return false;
    }
    submit_primary_text(window);
    true
}

/// A multiline edit keeps Tab for itself; Python's details field passes it on
/// to the next control like any other.
unsafe fn handle_details_tab_message(window: HWND, state: &WindowState, message: &MSG) -> bool {
    if state.view != MainView::Player
        || !state.player_controls.details_visible()
        || GetFocus() != state.player_controls.details_text()
    {
        return false;
    }
    if message.message == WM_KEYDOWN {
        let backward = virtual_key_is_down(usize::from(VK_SHIFT.0));
        if let Ok(next) = windows::Win32::UI::WindowsAndMessaging::GetNextDlgTabItem(
            window,
            Some(GetFocus()),
            backward,
        ) {
            let _ = SetFocus(Some(next));
        }
    }
    true
}

unsafe fn handle_view_tab_message(window: HWND, message: &MSG) -> bool {
    if message.wParam.0 != usize::from(VK_TAB.0)
        || !matches!(message.message, WM_KEYDOWN | WM_KEYUP)
    {
        return false;
    }
    let Some(state) = state(window) else {
        return false;
    };
    if handle_details_tab_message(window, state, message) {
        return true;
    }
    let controls = match state.view {
        MainView::Trending => vec![
            state.trending_country,
            state.trending_category,
            state.list,
            state.back,
            state.load_trending,
        ],
        MainView::Subscriptions => vec![
            state.back,
            state.subscription_check,
            state.open,
            state.subscription_new,
            state.collection_remove,
            state.subscription_filter,
            state.subscription_set_category,
            state.list,
        ],
        MainView::RssFeeds => vec![
            state.back,
            state.rss_search,
            state.rss_categories,
            state.rss_add,
            state.rss_refresh,
            state.open,
            state.collection_remove,
            state.rss_filter,
            state.rss_set_category,
            state.rss_import,
            state.rss_export,
            state.list,
        ],
        MainView::RssItems => vec![
            state.back,
            state.rss_refresh,
            state.open,
            state.rss_download_episode,
            state.rss_download_feed,
            state.list,
        ],
        MainView::PodcastSearchResults => vec![
            state.back,
            state.podcast_add,
            state.open_browser,
            state.list,
        ],
        MainView::PodcastCategories => vec![state.back, state.open, state.list],
        // Python `show_notification_center`: Back, Play, Clear notifications, list.
        MainView::NotificationCenter => {
            vec![state.back, state.open, state.notification_clear, state.list]
        }
        MainView::DownloadQueue => {
            let mut controls = vec![state.back];
            if !state.application.downloads().queued().is_empty() {
                controls.extend([
                    state.open,
                    state.download_all_audio,
                    state.download_all_video,
                ]);
            }
            if !state.application.downloads().active().is_empty() {
                controls.extend([state.download_cancel, state.download_cancel_all]);
            }
            controls.push(state.list);
            controls
        }
        MainView::UserPlaylists | MainView::UserPlaylistItems => user_playlist_tab_controls(state),
        _ => return false,
    };
    let Some(current) = controls.iter().position(|control| *control == GetFocus()) else {
        return false;
    };
    if message.message == WM_KEYUP {
        return true;
    }
    let next = if virtual_key_is_down(usize::from(VK_SHIFT.0)) {
        (current + controls.len() - 1) % controls.len()
    } else {
        (current + 1) % controls.len()
    };
    let _ = SetFocus(Some(controls[next]));
    true
}

fn user_playlist_tab_controls(state: &WindowState) -> Vec<HWND> {
    if state.view == MainView::UserPlaylists {
        return vec![
            state.back,
            state.playlist_create,
            state.open,
            state.playlist_download,
            state.collection_remove,
            state.list,
        ];
    }
    let mut controls = vec![state.back];
    if state
        .application
        .user_playlists()
        .get(state.current_user_playlist_index)
        .is_some_and(|playlist| !playlist.items.is_empty())
    {
        controls.extend([
            state.open,
            state.playlist_download,
            state.collection_remove,
            state.playlist_play_all,
            state.playlist_shuffle,
            state.playlist_add_all_to_queue,
        ]);
    }
    controls.push(state.list);
    controls
}

// Keep the native message dispatch table together.
#[allow(clippy::too_many_lines)]
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
        add_tray_icon(window);
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
        WM_SHOW_DOWNLOAD_DETAILS => show_download_progress_details(window),
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
        WM_TIMER if wparam.0 == SUBSCRIPTION_TIMER_ID => {
            check_subscriptions_if_due(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == RSS_TIMER_ID => {
            refresh_all_rss_feeds_background(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == DOWNLOAD_TIMER_ID => {
            poll_download_updates(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == CHAPTER_TIMER_ID => {
            poll_external_chapters(window);
            LRESULT(0)
        }
        WM_SYNC_FULLSCREEN => {
            apply_window_fullscreen(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == RELATED_TIMER_ID => {
            poll_related_videos(window);
            LRESULT(0)
        }
        WM_DESTROY => {
            remove_tray_icon(window);
            let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut WindowState;
            if !pointer.is_null() {
                let mut state = Box::from_raw(pointer);
                for cancellation in state.download_cancellations.values() {
                    cancellation.store(true, AtomicOrdering::Release);
                }
                state.download_cancellations.clear();
                if let Some(progress_window) = state.download_progress_window.take() {
                    progress_window.destroy();
                }
                drop(state);
                SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

#[allow(clippy::too_many_lines)]
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
    } else if command == ID_LOAD_TRENDING
        || (matches!(command, ID_TRENDING_COUNTRY | ID_TRENDING_CATEGORY)
            && notification == usize::try_from(CBN_SELCHANGE).expect("notification fits"))
    {
        load_trending_results(window);
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
    } else if command == ID_SUBSCRIPTION_CHECK {
        check_subscriptions(window, true);
    } else if command == ID_SUBSCRIPTION_NEW {
        open_selected_subscription_new_videos(window);
    } else if command == ID_SUBSCRIPTION_FILTER {
        choose_subscription_category_filter(window);
    } else if command == ID_SUBSCRIPTION_SET_CATEGORY {
        set_selected_subscription_category(window);
    } else if command == ID_RSS_SEARCH {
        prompt_podcast_search(window);
    } else if command == ID_RSS_CATEGORIES {
        show_podcast_categories(window);
    } else if command == ID_RSS_ADD {
        prompt_add_rss_feed(window);
    } else if command == ID_RSS_REFRESH {
        refresh_rss_from_active_view(window);
    } else if command == ID_RSS_FILTER {
        choose_rss_category_filter(window);
    } else if command == ID_RSS_SET_CATEGORY {
        set_selected_rss_category(window);
    } else if command == ID_RSS_IMPORT {
        import_rss_opml(window);
    } else if command == ID_RSS_EXPORT {
        export_rss_opml(window);
    } else if command == ID_RSS_DOWNLOAD_FEED {
        download_current_rss_feed(window);
    } else if command == ID_RSS_SPEED {
        choose_rss_speed_preset(window);
    } else if command == ID_RSS_TOGGLE_PLAYED {
        toggle_selected_rss_played(window);
    } else if command == ID_RSS_CLEAR_PROGRESS {
        clear_selected_rss_progress(window);
    } else if command == ID_RSS_DOWNLOAD_EPISODE {
        download_selected_rss_episode(window);
    } else if command == ID_PODCAST_ADD {
        add_selected_podcast_result(window);
    } else if command == ID_OPEN_BROWSER {
        open_selected_podcast_in_browser(window);
    } else if command == ID_PLAYLIST_CREATE {
        create_user_playlist(window, None);
    } else if command == ID_PLAYLIST_PLAY_ALL {
        play_current_user_playlist(window, false);
    } else if command == ID_PLAYLIST_SHUFFLE {
        play_current_user_playlist(window, true);
    } else if command == ID_PLAYLIST_ADD_ALL_TO_QUEUE {
        add_current_user_playlist_to_queue(window);
    } else if command == ID_PLAYLIST_DOWNLOAD {
        download_current_user_playlist(window);
    } else if command == ID_DOWNLOAD_ALL_AUDIO {
        start_all_queued_downloads(window, DownloadChoice::Audio);
    } else if command == ID_DOWNLOAD_ALL_VIDEO {
        start_all_queued_downloads(window, DownloadChoice::Video);
    } else if command == ID_DOWNLOAD_CANCEL {
        cancel_selected_download(window);
    } else if command == ID_DOWNLOAD_CANCEL_ALL {
        cancel_all_downloads(window);
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
    let trending_country_label_text = wide(catalog.text("trending_country"));
    let trending_country_label = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(trending_country_label_text.as_ptr()),
        WS_CHILD | WS_GROUP,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let trending_country = create_control(
        parent,
        instance,
        w!("COMBOBOX"),
        PCWSTR(trending_country_label_text.as_ptr()),
        WS_CHILD | WS_TABSTOP | WS_GROUP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL,
        WINDOW_EX_STYLE::default(),
        ID_TRENDING_COUNTRY,
    )?;
    for choice in YOUTUBE_TRENDING_COUNTRIES {
        let label = wide(choice.label_key);
        SendMessageW(
            trending_country,
            CB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    }
    SendMessageW(trending_country, CB_SETCURSEL, Some(WPARAM(0)), None);
    let trending_category_label_text = wide(catalog.text("trending_category"));
    let trending_category_label = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(trending_category_label_text.as_ptr()),
        WS_CHILD,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let trending_category = create_control(
        parent,
        instance,
        w!("COMBOBOX"),
        PCWSTR(trending_category_label_text.as_ptr()),
        WS_CHILD | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL,
        WINDOW_EX_STYLE::default(),
        ID_TRENDING_CATEGORY,
    )?;
    for choice in YOUTUBE_TRENDING_CATEGORIES {
        let label = wide(catalog.text(choice.label_key));
        SendMessageW(
            trending_category,
            CB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    }
    SendMessageW(trending_category, CB_SETCURSEL, Some(WPARAM(0)), None);
    let load_trending_text = wide(catalog.text("load_trending"));
    let load_trending = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(load_trending_text.as_ptr()),
        WS_CHILD | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        ID_LOAD_TRENDING,
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
    let subscription_check_text = wide(catalog.text("subscription_check_now"));
    let subscription_check = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(subscription_check_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_SUBSCRIPTION_CHECK,
    )?;
    let subscription_new_text = wide(catalog.text("subscription_new_videos_button"));
    let subscription_new = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(subscription_new_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_SUBSCRIPTION_NEW,
    )?;
    let subscription_filter_text = wide(catalog.text("filter_category"));
    let subscription_filter = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(subscription_filter_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_SUBSCRIPTION_FILTER,
    )?;
    let subscription_set_category_text = wide(catalog.text("set_category"));
    let subscription_set_category = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(subscription_set_category_text.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_SUBSCRIPTION_SET_CATEGORY,
    )?;
    let rss_search = create_button(parent, instance, &catalog, "search_podcasts", ID_RSS_SEARCH)?;
    let rss_categories = create_button(
        parent,
        instance,
        &catalog,
        "podcast_categories",
        ID_RSS_CATEGORIES,
    )?;
    let rss_add = create_button(parent, instance, &catalog, "add_rss_feed", ID_RSS_ADD)?;
    let rss_refresh = create_button(parent, instance, &catalog, "refresh_feeds", ID_RSS_REFRESH)?;
    let rss_filter = create_button(parent, instance, &catalog, "filter_category", ID_RSS_FILTER)?;
    let rss_set_category = create_button(
        parent,
        instance,
        &catalog,
        "set_category",
        ID_RSS_SET_CATEGORY,
    )?;
    let rss_import = create_button(parent, instance, &catalog, "import_opml", ID_RSS_IMPORT)?;
    let rss_export = create_button(parent, instance, &catalog, "export_opml", ID_RSS_EXPORT)?;
    let rss_download_feed = create_button(
        parent,
        instance,
        &catalog,
        "download_feed",
        ID_RSS_DOWNLOAD_FEED,
    )?;
    let rss_speed = create_button(
        parent,
        instance,
        &catalog,
        "podcast_speed_preset",
        ID_RSS_SPEED,
    )?;
    let rss_toggle_played = create_button(
        parent,
        instance,
        &catalog,
        "mark_episode_played",
        ID_RSS_TOGGLE_PLAYED,
    )?;
    let rss_clear_progress = create_button(
        parent,
        instance,
        &catalog,
        "clear_episode_progress",
        ID_RSS_CLEAR_PROGRESS,
    )?;
    let podcast_add = create_button(parent, instance, &catalog, "add_podcast", ID_PODCAST_ADD)?;
    let open_browser = create_button(parent, instance, &catalog, "open_browser", ID_OPEN_BROWSER)?;
    let rss_download_episode = create_button(
        parent,
        instance,
        &catalog,
        "download_episode_audio",
        ID_RSS_DOWNLOAD_EPISODE,
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
    let playlist_download = create_button(
        parent,
        instance,
        &catalog,
        "download_user_playlist",
        ID_PLAYLIST_DOWNLOAD,
    )?;
    let download_all_audio = create_button(
        parent,
        instance,
        &catalog,
        "download_all_as_audio",
        ID_DOWNLOAD_ALL_AUDIO,
    )?;
    let download_all_video = create_button(
        parent,
        instance,
        &catalog,
        "download_all_as_video",
        ID_DOWNLOAD_ALL_VIDEO,
    )?;
    let download_cancel = create_button(
        parent,
        instance,
        &catalog,
        "cancel_download",
        ID_DOWNLOAD_CANCEL,
    )?;
    let download_cancel_all = create_button(
        parent,
        instance,
        &catalog,
        "cancel_all_downloads",
        ID_DOWNLOAD_CANCEL_ALL,
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
        trending_country_label,
        trending_country,
        trending_category_label,
        trending_category,
        load_trending,
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
        subscription_check,
        subscription_new,
        subscription_filter,
        subscription_set_category,
        rss_search,
        rss_categories,
        rss_add,
        rss_refresh,
        rss_filter,
        rss_set_category,
        rss_import,
        rss_export,
        rss_download_feed,
        rss_speed,
        rss_toggle_played,
        rss_clear_progress,
        podcast_add,
        open_browser,
        rss_download_episode,
        playlist_create,
        playlist_play_all,
        playlist_shuffle,
        playlist_add_all_to_queue,
        playlist_download,
        download_all_audio,
        download_all_video,
        download_cancel,
        download_cancel_all,
        status,
    ] {
        SendMessageW(control, WM_SETFONT, font_param, Some(LPARAM(1)));
    }
    let (download_sender, download_receiver) = mpsc::sync_channel(256);
    Ok(WindowState {
        list,
        open,
        search_label,
        search_edit,
        kind_label,
        kind,
        search,
        back,
        trending_country_label,
        trending_country,
        trending_category_label,
        trending_category,
        load_trending,
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
        subscription_check,
        subscription_new,
        subscription_filter,
        subscription_set_category,
        rss_search,
        rss_categories,
        rss_add,
        rss_refresh,
        rss_filter,
        rss_set_category,
        rss_import,
        rss_export,
        rss_download_feed,
        rss_speed,
        rss_toggle_played,
        rss_clear_progress,
        podcast_add,
        open_browser,
        rss_download_episode,
        playlist_create,
        playlist_play_all,
        playlist_shuffle,
        playlist_add_all_to_queue,
        playlist_download,
        download_all_audio,
        download_all_video,
        download_cancel,
        download_cancel_all,
        video_host,
        player_controls,
        status,
        announcer: crate::announcement_win32::WindowsAnnouncer::new(status),
        model,
        application,
        settings_open: false,
        modal_open: false,
        tray_icon_added: false,
        last_activated_menu_item: None,
        lifecycle: WindowLifecycle::Visible,
        taskbar_created_message: RegisterWindowMessageW(w!("TaskbarCreated")),
        view: MainView::MainMenu,
        youtube_search: YoutubeSearchService::default(),
        youtube_metadata: YoutubeSearchService::default(),
        youtube_subscriptions: YoutubeSearchService::default(),
        pending_youtube_work: None,
        pending_youtube_resolve: None,
        pending_youtube_metadata: None,
        pending_youtube_api_metadata: None,
        pending_youtube_trending_api: None,
        pending_subscription_check: None,
        pending_podcast_work: None,
        podcast_search_results: Vec::new(),
        podcast_search_query: String::new(),
        current_rss_feed_index: 0,
        current_rss_item_index: 0,
        rss_visible_item_count: 0,
        hydrated_youtube_urls: HashSet::new(),
        youtube_api_metadata_disabled_scopes: HashSet::new(),
        deferred_youtube_metadata_rows: HashSet::new(),
        last_download_shortcut: None,
        result_column_cursor: apricot_app::result_columns::ResultColumnCursor::default(),
        pending_player_navigation: None,
        pending_queued_start: None,
        next_youtube_operation_token: 0,
        playback: None,
        controlled_repeat: None,
        clip_preview: None,
        pending_local_folder_scan: None,
        next_local_folder_generation: 0,
        current_user_playlist_index: 0,
        current_user_playlist_item_index: 0,
        download_sender,
        download_receiver,
        download_cancellations: HashMap::new(),
        download_progress_reports: HashMap::new(),
        download_progress_window: None,
        download_progress_task_ids: HashSet::new(),
        download_progress_task_id: None,
        clip_exports: Vec::new(),
        pending_chapters: None,
        pending_related: None,
        fullscreen_restore: None,
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

unsafe fn create_button(
    parent: HWND,
    instance: HINSTANCE,
    catalog: &apricot_core::TranslationCatalog,
    label_key: &str,
    id: usize,
) -> Result<HWND> {
    let label = wide(catalog.text(label_key));
    create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(label.as_ptr()),
        WS_CHILD | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        id,
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
        && matches!(wparam.0, key if key == usize::from(VK_END.0) || key == usize::from(VK_DOWN.0))
        && let Ok(parent) = windows::Win32::UI::WindowsAndMessaging::GetParent(window)
        && state(parent).is_some_and(|state| {
            let count = SendMessageW(window, LB_GETCOUNT, None, None).0;
            state.view == MainView::RssItems
                && count > 0
                && (wparam.0 == usize::from(VK_END.0)
                    || SendMessageW(window, LB_GETCURSEL, None, None).0 >= count - 1)
        })
    {
        maybe_extend_rss_items(parent);
    }
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
    let active_item = active_media_item(window);
    let catalog = apricot_app::embedded_catalog(&language);
    let model_entries = state(window)
        .and_then(|state| model_context_entries(state, &catalog, view, active_item.as_ref()));
    let entries = if model_entries.is_some() {
        None
    } else if view == MainView::DownloadQueue {
        download_queue_context_entries(window)
    } else {
        list_context_entries(view)
    };
    if model_entries.is_none() && entries.is_none() {
        return;
    }
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let mut model_commands = Vec::new();
    if let Some(model_entries) = &model_entries {
        append_context_model_entries(menu, model_entries, &mut model_commands);
    } else if let Some(entries) = &entries {
        append_list_context_entries(menu, &catalog, entries, active_item.as_ref());
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
        let selected = usize::try_from(selected.0).unwrap_or_default();
        if let Some(command) = context_model_command(&model_commands, selected) {
            execute_context_command(window, command, active_item);
        } else {
            execute_list_context_command(window, selected, view);
        }
    }
    let _ = DestroyMenu(menu);
    if let Some(state) = state(window) {
        let _ = SetFocus(Some(active_primary_control(state)));
    }
}

unsafe fn append_list_context_entries(
    menu: HMENU,
    catalog: &apricot_core::TranslationCatalog,
    entries: &[(usize, &'static str)],
    active_item: Option<&apricot_core::MediaItem>,
) {
    let active_is_local = active_item.is_some_and(apricot_core::MediaItem::is_local_media);
    for (id, key) in entries {
        let key = if *id == ID_CONTEXT_COPY_LOCATION && active_is_local {
            "copy_path"
        } else if *id == ID_CONTEXT_RSS_TOGGLE_PLAYED
            && active_item.is_some_and(|item| metadata_bool(item, "played"))
        {
            "mark_episode_unplayed"
        } else {
            key
        };
        let label = wide(catalog.text(key));
        let _ = AppendMenuW(menu, MF_STRING, *id, PCWSTR(label.as_ptr()));
    }
}

/// Menus that follow Python's `menus.py` and `library.py` item for item.
fn model_context_entries(
    state: &WindowState,
    catalog: &apricot_core::TranslationCatalog,
    view: MainView,
    item: Option<&apricot_core::MediaItem>,
) -> Option<Vec<ContextMenuEntry>> {
    let user_playlist_titles = state
        .application
        .user_playlists()
        .iter()
        .map(|playlist| playlist.title.clone())
        .collect::<Vec<_>>();
    let context = ContextMenuContext {
        catalog,
        settings: state.application.settings(),
        user_playlist_titles: &user_playlist_titles,
        queued_download_count: state.application.downloads().queued().len(),
    };
    Some(match view {
        MainView::Results
        | MainView::Trending
        | MainView::YoutubeCollection
        | MainView::LocalFolder => context_menu::results_context_menu(&context, item),
        MainView::Favorites => context_menu::favorites_context_menu(&context, item),
        MainView::History => context_menu::history_context_menu(&context, item),
        MainView::UserPlaylists => context_menu::user_playlists_context_menu(&context),
        MainView::UserPlaylistItems => {
            context_menu::user_playlist_items_context_menu(&context, item)
        }
        MainView::Player => context_menu::player_context_menu(&context, item?),
        _ => return None,
    })
}

unsafe fn append_context_model_entries(
    menu: HMENU,
    entries: &[ContextMenuEntry],
    commands: &mut Vec<ContextCommand>,
) {
    for entry in entries {
        let label = wide(entry.label());
        match entry {
            ContextMenuEntry::Command { command, .. } => {
                let id = ID_CONTEXT_MODEL_FIRST + commands.len();
                commands.push(*command);
                let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(label.as_ptr()));
            }
            ContextMenuEntry::Submenu { entries, .. } => {
                let Ok(submenu) = CreatePopupMenu() else {
                    continue;
                };
                append_context_model_entries(submenu, entries, commands);
                if AppendMenuW(menu, MF_POPUP, submenu.0 as usize, PCWSTR(label.as_ptr())).is_err()
                {
                    let _ = DestroyMenu(submenu);
                }
            }
            ContextMenuEntry::Disabled { .. } => {
                let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, PCWSTR(label.as_ptr()));
            }
        }
    }
}

fn context_model_command(commands: &[ContextCommand], selected: usize) -> Option<ContextCommand> {
    selected
        .checked_sub(ID_CONTEXT_MODEL_FIRST)
        .and_then(|index| commands.get(index).copied())
}

unsafe fn execute_context_command(
    window: HWND,
    command: ContextCommand,
    active_item: Option<apricot_core::MediaItem>,
) {
    use ContextCommand as C;
    match command {
        C::Play | C::OpenUserPlaylist => activate_selection(window),
        C::DownloadAudio => start_active_download(window, DownloadChoice::Audio),
        C::DownloadVideo => start_active_download(window, DownloadChoice::Video),
        C::DownloadAllAudio => start_all_queued_downloads(window, DownloadChoice::Audio),
        C::DownloadAllVideo => start_all_queued_downloads(window, DownloadChoice::Video),
        C::AddFavorite => add_active_favorite(window),
        C::RemoveFavorite => remove_active_favorite(window),
        C::SubscribeChannel => subscribe_active_channel(window),
        C::UnsubscribeChannel => unsubscribe_active_channel(window),
        C::OpenChannel => activate_action(window, "open_channel"),
        C::AddToPlaybackQueue => add_active_item_to_playback_queue(window),
        C::RemoveFromPlaybackQueue => remove_active_item_from_playback_queue(window),
        C::RemoveFromPlaylist => remove_active_item_from_user_playlist(window),
        C::OpenInBrowser => open_item_in_browser(window, active_item.as_ref()),
        C::CopyStreamUrl => copy_active_stream_url(window),
        C::CopyLocation => copy_active_location(window),
        C::CopyTimestampLink => copy_current_timestamp_link(window),
        C::ChannelOptions => {
            if let Some(item) = active_item {
                show_channel_options(window, item);
            }
        }
        C::ChannelVideos | C::ChannelPopular | C::ChannelPlaylists | C::ChannelStreams => {
            if let Some(item) = active_item {
                let kind = match command {
                    C::ChannelVideos => YoutubeCollectionKind::ChannelVideos,
                    C::ChannelPopular => YoutubeCollectionKind::ChannelPopular,
                    C::ChannelPlaylists => YoutubeCollectionKind::ChannelPlaylists,
                    _ => YoutubeCollectionKind::ChannelStreams,
                };
                open_youtube_collection(window, item, kind);
            }
        }
        C::PlayPlaylist | C::ShufflePlaylist => {
            if let Some(item) = active_item.as_ref() {
                play_youtube_playlist(window, item, command == C::ShufflePlaylist);
            }
        }
        C::OpenPlaylistVideos => {
            if let Some(item) = active_item {
                open_youtube_collection(window, item, YoutubeCollectionKind::PlaylistVideos);
            }
        }
        C::AddToPlaylist => add_active_item_to_user_playlist(window),
        C::AddToUserPlaylist(index) => add_active_item_to_chosen_user_playlist(window, index),
        C::RemoveSelected => remove_selected_collection_item(window),
        C::ClearHistory => clear_history(window),
        C::CreatePlaylist => create_user_playlist(window, None),
        C::DownloadUserPlaylist => download_current_user_playlist(window),
        C::RemoveUserPlaylist => remove_selected_user_playlist(window),
        C::PlayerAction(action_id) => activate_action(window, action_id),
        C::ClosePlayer => navigate_back(window),
    }
}

/// Python `open_remote_url_in_browser` for the item's page or URL.
unsafe fn open_item_in_browser(window: HWND, item: Option<&apricot_core::MediaItem>) {
    let Some(item) = item else {
        return;
    };
    let Some(state) = state(window) else {
        return;
    };
    let Some(url) = context_menu::browser_url(item) else {
        // Python `validate_remote_http_url` raises this English text and
        // `open_remote_url_in_browser` announces it.
        set_status(state, "URL must use HTTP or HTTPS", true);
        return;
    };
    if let Err(error) = std::process::Command::new("explorer.exe").arg(&url).spawn() {
        set_status(state, &error.to_string(), true);
    }
}

unsafe fn execute_list_context_command(window: HWND, command: usize, view: MainView) {
    match command {
        ID_CONTEXT_PLAY | ID_CONTEXT_RSS_OPEN => activate_selection(window),
        ID_CONTEXT_ADD_TO_QUEUE => add_active_item_to_playback_queue(window),
        ID_CONTEXT_REMOVE_FROM_QUEUE => remove_active_item_from_playback_queue(window),
        ID_CONTEXT_COPY_LOCATION => copy_context_location(window),
        ID_CONTEXT_COLLECTION_REMOVE => remove_selected_collection_item(window),
        ID_CONTEXT_ADD_TO_PLAYLIST => add_active_item_to_user_playlist(window),
        ID_CONTEXT_CLEAR_NOTIFICATIONS => clear_notifications(window),
        ID_CONTEXT_SUBSCRIPTION_OPEN => open_selected_subscription_videos(window),
        ID_CONTEXT_SUBSCRIPTION_NEW => open_selected_subscription_new_videos(window),
        ID_CONTEXT_SUBSCRIPTION_CHECK => check_subscriptions(window, true),
        ID_CONTEXT_SUBSCRIPTION_SET_CATEGORY => set_selected_subscription_category(window),
        ID_CONTEXT_SUBSCRIPTION_FILTER => choose_subscription_category_filter(window),
        ID_CONTEXT_UNSUBSCRIBE => {
            if view == MainView::Subscriptions {
                remove_selected_subscription(window);
            } else {
                unsubscribe_active_channel(window);
            }
        }
        ID_CONTEXT_RSS_REFRESH => refresh_selected_rss_feed(window),
        ID_CONTEXT_RSS_SPEED => choose_rss_speed_preset(window),
        ID_CONTEXT_RSS_SET_CATEGORY => set_selected_rss_category(window),
        ID_CONTEXT_RSS_FILTER => choose_rss_category_filter(window),
        ID_CONTEXT_RSS_REMOVE => remove_selected_rss_feed(window),
        ID_CONTEXT_RSS_TOGGLE_PLAYED => toggle_selected_rss_played(window),
        ID_CONTEXT_RSS_CLEAR_PROGRESS => clear_selected_rss_progress(window),
        ID_CONTEXT_RSS_DOWNLOAD_FEED => download_current_rss_feed(window),
        ID_CONTEXT_RSS_DOWNLOAD_EPISODE => download_selected_rss_episode(window),
        ID_CONTEXT_RSS_QUEUE_EPISODE => queue_selected_rss_episode_download(window),
        ID_CONTEXT_DOWNLOAD_AUDIO => start_active_download(window, DownloadChoice::Audio),
        ID_CONTEXT_DOWNLOAD_VIDEO => start_active_download(window, DownloadChoice::Video),
        ID_CONTEXT_DOWNLOAD_SELECTED => activate_selected_download(window),
        ID_CONTEXT_DOWNLOAD_ALL_AUDIO => {
            start_all_queued_downloads(window, DownloadChoice::Audio);
        }
        ID_CONTEXT_DOWNLOAD_ALL_VIDEO => {
            start_all_queued_downloads(window, DownloadChoice::Video);
        }
        ID_CONTEXT_DOWNLOAD_CANCEL => cancel_selected_download(window),
        ID_CONTEXT_DOWNLOAD_CANCEL_ALL => cancel_all_downloads(window),
        ID_CONTEXT_DOWNLOAD_REMOVE_QUEUED => remove_selected_queued_download(window),
        ID_CONTEXT_OPEN_BROWSER => open_selected_podcast_in_browser(window),
        ID_CONTEXT_PODCAST_ADD => add_selected_podcast_result(window),
        _ => {}
    }
}

/// Views whose menus are not yet part of the Python menu model.
fn list_context_entries(view: MainView) -> Option<Vec<(usize, &'static str)>> {
    match view {
        MainView::NotificationCenter => Some(vec![
            (ID_CONTEXT_PLAY, "play"),
            (ID_CONTEXT_COPY_LOCATION, "copy_url"),
            (ID_CONTEXT_CLEAR_NOTIFICATIONS, "clear_notifications"),
        ]),
        MainView::Subscriptions => Some(vec![
            (ID_CONTEXT_SUBSCRIPTION_OPEN, "subscription_open_videos"),
            (
                ID_CONTEXT_SUBSCRIPTION_NEW,
                "subscription_new_videos_button",
            ),
            (ID_CONTEXT_SUBSCRIPTION_CHECK, "subscription_check_now"),
            (ID_CONTEXT_SUBSCRIPTION_SET_CATEGORY, "set_category"),
            (ID_CONTEXT_SUBSCRIPTION_FILTER, "filter_category"),
            (ID_CONTEXT_COPY_LOCATION, "copy_url"),
            (ID_CONTEXT_UNSUBSCRIBE, "unsubscribe_channel"),
            (ID_CONTEXT_COLLECTION_REMOVE, "remove"),
        ]),
        MainView::RssFeeds => Some(vec![
            (ID_CONTEXT_RSS_OPEN, "open_feed"),
            (ID_CONTEXT_RSS_DOWNLOAD_FEED, "download_feed"),
            (ID_CONTEXT_RSS_SPEED, "podcast_speed_preset"),
            (ID_CONTEXT_RSS_REFRESH, "refresh_feed"),
            (ID_CONTEXT_RSS_SET_CATEGORY, "set_category"),
            (ID_CONTEXT_RSS_FILTER, "filter_category"),
            (ID_CONTEXT_COPY_LOCATION, "copy_url"),
            (ID_CONTEXT_RSS_REMOVE, "remove_feed"),
        ]),
        MainView::RssItems => Some(vec![
            (ID_CONTEXT_PLAY, "play_episode"),
            (ID_CONTEXT_RSS_TOGGLE_PLAYED, "mark_episode_played"),
            (ID_CONTEXT_RSS_CLEAR_PROGRESS, "clear_episode_progress"),
            (ID_CONTEXT_RSS_DOWNLOAD_EPISODE, "download_episode_audio"),
            (ID_CONTEXT_RSS_QUEUE_EPISODE, "queue_episode_audio"),
            (ID_CONTEXT_ADD_TO_PLAYLIST, "add_to_playlist"),
            (ID_CONTEXT_ADD_TO_QUEUE, "add_to_playback_queue"),
            (ID_CONTEXT_REMOVE_FROM_QUEUE, "remove_from_playback_queue"),
            (ID_CONTEXT_RSS_DOWNLOAD_FEED, "download_feed"),
            (ID_CONTEXT_OPEN_BROWSER, "open_episode_page"),
            (ID_CONTEXT_COPY_LOCATION, "copy_url"),
        ]),
        MainView::PodcastSearchResults => Some(vec![
            (ID_CONTEXT_PODCAST_ADD, "add_podcast"),
            (ID_CONTEXT_OPEN_BROWSER, "open_browser"),
            (ID_CONTEXT_COPY_LOCATION, "copy_url"),
        ]),
        _ => None,
    }
}

unsafe fn download_queue_context_entries(window: HWND) -> Option<Vec<(usize, &'static str)>> {
    let state = state(window)?;
    let selected = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
    let active_count = state.application.downloads().active().len();
    if selected < active_count {
        return Some(vec![
            (ID_CONTEXT_DOWNLOAD_CANCEL, "cancel_download"),
            (ID_CONTEXT_DOWNLOAD_CANCEL_ALL, "cancel_all_downloads"),
        ]);
    }
    let queued_index = selected.checked_sub(active_count)?;
    state.application.downloads().queued().get(queued_index)?;
    Some(vec![
        (ID_CONTEXT_DOWNLOAD_SELECTED, "download_selected_queued"),
        (ID_CONTEXT_DOWNLOAD_AUDIO, "download_audio"),
        (ID_CONTEXT_DOWNLOAD_VIDEO, "download_video"),
        (ID_CONTEXT_DOWNLOAD_ALL_AUDIO, "download_all_as_audio"),
        (ID_CONTEXT_DOWNLOAD_ALL_VIDEO, "download_all_as_video"),
        (ID_CONTEXT_DOWNLOAD_REMOVE_QUEUED, "remove_from_queue"),
    ])
}

unsafe fn show_context_menu_for_active_view(window: HWND) {
    match state(window).map(|state| state.view) {
        Some(
            MainView::Results
            | MainView::Trending
            | MainView::YoutubeCollection
            | MainView::LocalFolder
            | MainView::Favorites
            | MainView::History
            | MainView::NotificationCenter
            | MainView::Subscriptions
            | MainView::RssFeeds
            | MainView::RssItems
            | MainView::PodcastSearchResults
            | MainView::PodcastCategories
            | MainView::UserPlaylists
            | MainView::UserPlaylistItems
            | MainView::DownloadQueue,
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
    let Some(entries) = state(window)
        .and_then(|state| model_context_entries(state, &catalog, MainView::Player, Some(&item)))
    else {
        return;
    };
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let mut commands = Vec::new();
    append_context_model_entries(menu, &entries, &mut commands);
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
        let selected = usize::try_from(selected.0).unwrap_or_default();
        if let Some(command) = context_model_command(&commands, selected) {
            execute_context_command(window, command, Some(item));
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
    sync_window_fullscreen(window, state);
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
    } else if state.view == MainView::Trending {
        layout_trending_controls(
            state,
            width,
            height,
            margin,
            label_height,
            field_height,
            button_height + status_height,
        );
    } else if state.view == MainView::Player {
        state
            .player_controls
            .layout(width, height, margin, status_height);
    } else {
        let action_rows = if matches!(
            state.view,
            MainView::LocalFolder | MainView::Subscriptions | MainView::RssFeeds
        ) {
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

unsafe fn layout_trending_controls(
    state: &WindowState,
    width: i32,
    height: i32,
    margin: i32,
    label_height: i32,
    field_height: i32,
    bottom_reserved_height: i32,
) {
    let country_y = margin;
    let category_y = country_y + label_height + field_height + margin;
    let list_y = category_y + label_height + field_height + margin;
    let list_height = height - list_y - bottom_reserved_height - margin * 3;
    for (control, y, control_height) in [
        (state.trending_country_label, country_y, label_height),
        (state.trending_country, country_y + label_height, 240),
        (state.trending_category_label, category_y, label_height),
        (state.trending_category, category_y + label_height, 240),
        (state.list, list_y, list_height.max(40)),
    ] {
        let _ = MoveWindow(control, margin, y, width - margin * 2, control_height, true);
    }
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
    let subscriptions = state.view == MainView::Subscriptions;
    let rss_feeds = state.view == MainView::RssFeeds;
    let rss_items = state.view == MainView::RssItems;
    let trending = state.view == MainView::Trending;
    let user_playlists = state.view == MainView::UserPlaylists;
    let user_playlist_items = state.view == MainView::UserPlaylistItems;
    let download_queue = state.view == MainView::DownloadQueue;
    let first_button_y = if local_folder || subscriptions || rss_feeds {
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
    if trending {
        layout_button_row(
            &[state.back, state.load_trending],
            width,
            first_button_y,
            margin,
            button_height,
        );
    } else if local_folder {
        layout_local_folder_buttons(state, width, height, first_button_y, margin, button_height);
    } else if subscriptions {
        layout_button_row(
            &[
                state.back,
                state.subscription_check,
                state.open,
                state.subscription_new,
                state.collection_remove,
            ],
            width,
            first_button_y,
            margin,
            button_height,
        );
    } else if rss_feeds {
        layout_button_row(
            &[
                state.back,
                state.rss_search,
                state.rss_categories,
                state.rss_add,
                state.rss_refresh,
            ],
            width,
            first_button_y,
            margin,
            button_height,
        );
        layout_button_row(
            &[
                state.open,
                state.collection_remove,
                state.rss_filter,
                state.rss_set_category,
                state.rss_import,
                state.rss_export,
            ],
            width,
            height - button_height - margin,
            margin,
            button_height,
        );
    } else if rss_items {
        layout_button_row(
            &[
                state.back,
                state.rss_refresh,
                state.open,
                state.rss_download_episode,
                state.rss_download_feed,
            ],
            width,
            first_button_y,
            margin,
            button_height,
        );
    } else if state.view == MainView::PodcastSearchResults {
        layout_button_row(
            &[state.back, state.podcast_add, state.open_browser],
            width,
            first_button_y,
            margin,
            button_height,
        );
    } else if state.view == MainView::PodcastCategories {
        layout_button_row(
            &[state.back, state.open],
            width,
            first_button_y,
            margin,
            button_height,
        );
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
                state.playlist_download,
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
                state.playlist_download,
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
    } else if download_queue {
        let mut controls = vec![state.back];
        if !state.application.downloads().queued().is_empty() {
            controls.extend([
                state.open,
                state.download_all_audio,
                state.download_all_video,
            ]);
        }
        if !state.application.downloads().active().is_empty() {
            controls.extend([state.download_cancel, state.download_cancel_all]);
        }
        layout_button_row(&controls, width, first_button_y, margin, button_height);
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

#[allow(clippy::too_many_lines)]
unsafe fn set_view_visibility(state: &mut WindowState) {
    let list_visible = matches!(
        state.view,
        MainView::MainMenu
            | MainView::Results
            | MainView::Trending
            | MainView::YoutubeCollection
            | MainView::LocalFolder
            | MainView::Favorites
            | MainView::History
            | MainView::NotificationCenter
            | MainView::Subscriptions
            | MainView::RssFeeds
            | MainView::RssItems
            | MainView::PodcastSearchResults
            | MainView::PodcastCategories
            | MainView::UserPlaylists
            | MainView::UserPlaylistItems
            | MainView::DownloadQueue
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
    let open_visible = list_visible
        && playlist_items_available
        && state.view != MainView::Trending
        && (state.view != MainView::DownloadQueue
            || !state.application.downloads().queued().is_empty());
    let folder_visible = state.view == MainView::LocalFolder;
    let trending_visible = state.view == MainView::Trending;
    let subscriptions_visible = state.view == MainView::Subscriptions;
    let rss_feeds_visible = state.view == MainView::RssFeeds;
    let rss_items_visible = state.view == MainView::RssItems;
    let podcast_directory_visible = state.view == MainView::PodcastSearchResults;
    let download_queue_visible = state.view == MainView::DownloadQueue;
    let queued_downloads_visible =
        download_queue_visible && !state.application.downloads().queued().is_empty();
    let active_downloads_visible =
        download_queue_visible && !state.application.downloads().active().is_empty();
    for (control, visible) in [
        (state.list, list_visible),
        (state.open, open_visible),
        (state.search_label, text_entry_visible),
        (state.search_edit, text_entry_visible),
        (state.kind_label, search_visible),
        (state.kind, search_visible),
        (state.search, search_visible),
        (state.back, back_visible),
        (state.trending_country_label, trending_visible),
        (state.trending_country, trending_visible),
        (state.trending_category_label, trending_visible),
        (state.trending_category, trending_visible),
        (state.load_trending, trending_visible),
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
        (state.subscription_check, subscriptions_visible),
        (state.subscription_new, subscriptions_visible),
        (state.subscription_filter, subscriptions_visible),
        (state.subscription_set_category, subscriptions_visible),
        (state.rss_search, rss_feeds_visible),
        (state.rss_categories, rss_feeds_visible),
        (state.rss_add, rss_feeds_visible),
        (state.rss_refresh, rss_feeds_visible || rss_items_visible),
        (state.rss_filter, rss_feeds_visible),
        (state.rss_set_category, rss_feeds_visible),
        (state.rss_import, rss_feeds_visible),
        (state.rss_export, rss_feeds_visible),
        (state.rss_download_feed, rss_items_visible),
        (state.rss_speed, false),
        (state.rss_toggle_played, false),
        (state.rss_clear_progress, false),
        (state.podcast_add, podcast_directory_visible),
        (state.open_browser, podcast_directory_visible),
        (state.rss_download_episode, rss_items_visible),
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
        (
            state.playlist_download,
            matches!(
                state.view,
                MainView::UserPlaylists | MainView::UserPlaylistItems
            ) && playlist_items_available
                && !state.application.user_playlists().is_empty(),
        ),
        (state.download_all_audio, queued_downloads_visible),
        (state.download_all_video, queued_downloads_visible),
        (state.download_cancel, active_downloads_visible),
        (state.download_cancel_all, active_downloads_visible),
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
            | MainView::Trending
            | MainView::DirectLink
            | MainView::Results
            | MainView::YoutubeCollection
            | MainView::LocalFolder
            | MainView::Favorites
            | MainView::History
            | MainView::NotificationCenter
            | MainView::Subscriptions
            | MainView::RssFeeds
            | MainView::RssItems
            | MainView::PodcastSearchResults
            | MainView::PodcastCategories
            | MainView::UserPlaylists
            | MainView::UserPlaylistItems
            | MainView::DownloadQueue
    )
}

const fn view_has_collection_remove(view: MainView) -> bool {
    matches!(
        view,
        MainView::Favorites
            | MainView::History
            | MainView::Subscriptions
            | MainView::RssFeeds
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
    // Python `announce_player`: status text first, then speech.
    set_status(state, message, false);
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
        if id == ID_TRAY_EXIT {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        }
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
            check_subscriptions(window, true);
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
        Some(MainView::Results | MainView::Trending) => activate_result_selection(window),
        Some(MainView::YoutubeCollection) => activate_youtube_collection_selection(window),
        Some(MainView::LocalFolder) => activate_local_folder_selection(window),
        Some(MainView::Favorites | MainView::History) => activate_collection_selection(window),
        Some(MainView::NotificationCenter) => activate_notification_selection(window),
        Some(MainView::Subscriptions) => open_selected_subscription_videos(window),
        Some(MainView::RssFeeds) => open_selected_rss_feed(window),
        Some(MainView::RssItems) => play_selected_rss_episode(window),
        Some(MainView::PodcastSearchResults) => add_selected_podcast_result(window),
        Some(MainView::PodcastCategories) => open_selected_podcast_category(window),
        Some(MainView::UserPlaylists) => open_selected_user_playlist(window),
        Some(MainView::UserPlaylistItems) => activate_user_playlist_item(window),
        Some(MainView::DownloadQueue) => activate_selected_download(window),
        Some(MainView::Search | MainView::DirectLink | MainView::Player) | None => {}
    }
}

unsafe fn activate_main_menu_selection(window: HWND) {
    let Some((item_id, item_label)) = selected_main_menu_item(window) else {
        return;
    };
    remember_menu_item(window, item_id);
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
    if item_id == "trending" {
        show_trending(window);
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
    if item_id == "subscriptions" {
        show_subscriptions(window);
        return;
    }
    if item_id == "rss_feeds" {
        show_rss_feeds(window);
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
    if item_id == "current_downloads" {
        show_download_queue(window);
        return;
    }

    announce_unavailable_feature(window, item_label.split('\t').next().unwrap_or(&item_label));
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
    activate_youtube_item(window, item);
}

unsafe fn activate_youtube_collection_selection(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    let Ok(index) = usize::try_from(selected) else {
        return;
    };
    let Some(item) = state.application.prepare_youtube_collection_playback(index) else {
        return;
    };
    activate_youtube_item(window, item);
}

unsafe fn activate_youtube_item(window: HWND, item: apricot_core::MediaItem) {
    match item.kind {
        apricot_core::MediaKind::Channel => show_channel_options(window, item),
        apricot_core::MediaKind::Playlist => {
            open_youtube_collection(window, item, YoutubeCollectionKind::PlaylistVideos);
        }
        // Python `play_selected` turns shuffle off.
        _ => start_media_item_with_shuffle(window, item, None, false),
    }
}

unsafe fn show_channel_options(window: HWND, item: apricot_core::MediaItem) {
    let Some((title, prompt, choices, ok, cancel)) = state_mut(window).map(|state| {
        state.modal_open = true;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        (
            catalog.text("channel_options").to_owned(),
            item.title.clone(),
            [
                "channel_videos",
                "channel_playlists",
                "channel_live_streams",
                "channel_popular",
            ]
            .map(|key| catalog.text(key).to_owned()),
            catalog.text("open").to_owned(),
            catalog.text("cancel").to_owned(),
        )
    }) else {
        return;
    };
    let selection =
        crate::playlist_dialog_win32::choose(window, &title, &prompt, &choices, &ok, &cancel);
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    let kind = match selection {
        Ok(Some(0)) => YoutubeCollectionKind::ChannelVideos,
        Ok(Some(1)) => YoutubeCollectionKind::ChannelPlaylists,
        Ok(Some(2)) => YoutubeCollectionKind::ChannelStreams,
        Ok(Some(3)) => YoutubeCollectionKind::ChannelPopular,
        Ok(Some(_) | None) => {
            if let Some(state) = state(window) {
                let _ = SetFocus(Some(state.list));
            }
            return;
        }
        Err(error) => {
            show_error_message(window, &format!("Channel options did not open: {error}"));
            return;
        }
    };
    open_youtube_collection(window, item, kind);
}

/// Python's `announce_active_media_column`, only while a media list has focus.
unsafe fn announce_active_media_column(window: HWND, forward: bool) {
    let focused = state(window).is_some_and(|state| {
        GetFocus() == state.list && !matches!(state.view, MainView::MainMenu | MainView::Player)
    });
    if !focused {
        return;
    }
    let item = active_media_item(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let message = match item {
        Some(item) => apricot_app::result_columns::result_column_announcement(
            &mut state.result_column_cursor,
            &item,
            &catalog,
            forward,
        ),
        None => catalog.text("no_selection").to_owned(),
    };
    set_status(state, &message, true);
}

/// Python's `open_item_channel`: open the uploading channel's videos for the
/// selected row, or for the playing item when the selection has no channel.
unsafe fn open_item_channel(window: HWND) {
    let channel = active_media_item(window)
        .as_ref()
        .and_then(apricot_app::context_menu::youtube_channel_item_for_video)
        .or_else(|| {
            state(window)?
                .application
                .player_session()
                .current_item()
                .and_then(apricot_app::context_menu::youtube_channel_item_for_video)
        });
    match channel {
        Some(channel) => {
            open_youtube_collection(window, channel, YoutubeCollectionKind::ChannelVideos);
        }
        None => {
            if let Some(state) = state(window) {
                set_status(state, &catalog_text(&state.application, "no_channel"), true);
            }
        }
    }
}

unsafe fn open_youtube_collection(
    window: HWND,
    item: apricot_core::MediaItem,
    kind: YoutubeCollectionKind,
) {
    let Some(url) = item.url.as_ref().map(ToString::to_string) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "no_selection"),
                true,
            );
        }
        return;
    };
    let work = {
        let Some(state) = state_mut(window) else {
            return;
        };
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        let title = match kind {
            YoutubeCollectionKind::PlaylistVideos => item.title,
            YoutubeCollectionKind::ChannelVideos => {
                format!("{} - {}", item.title, catalog.text("channel_videos"))
            }
            YoutubeCollectionKind::ChannelPlaylists => {
                format!("{} - {}", item.title, catalog.text("channel_playlists"))
            }
            YoutubeCollectionKind::ChannelStreams => {
                format!("{} - {}", item.title, catalog.text("channel_live_streams"))
            }
            YoutubeCollectionKind::ChannelPopular => {
                format!("{} - {}", item.title, catalog.text("channel_popular"))
            }
        };
        match state
            .application
            .begin_youtube_collection(title.clone(), url, kind)
        {
            Ok(work) => {
                let message = catalog.text("loading_playlist").replace("{title}", &title);
                set_status(state, &message, true);
                work
            }
            Err(error) => {
                set_status(state, &error.to_string(), true);
                return;
            }
        }
    };
    start_youtube_collection_work(window, work);
}

unsafe fn play_youtube_playlist(window: HWND, item: &apricot_core::MediaItem, shuffle: bool) {
    let Some(url) = item.url.as_ref().map(ToString::to_string) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "no_selection"),
                true,
            );
        }
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    state.next_youtube_operation_token = state.next_youtube_operation_token.wrapping_add(1).max(1);
    let token = state.next_youtube_operation_token;
    let backend = collection_backend(YOUTUBE_BACKEND, YoutubeCollectionKind::PlaylistVideos);
    let Some(components) = application_directory().map(|path| path.join("components")) else {
        show_error_message(window, "Application path is unavailable");
        return;
    };
    let config = youtube_session_config(state);
    match state.youtube_search.start_collection_all(
        backend,
        &components,
        config,
        token,
        url,
        YoutubeCollectionKind::PlaylistVideos,
    ) {
        Ok(()) => {
            state.pending_youtube_work = Some(PendingYoutubeListWork::PlaylistPlayback(
                PendingYoutubePlaylistPlayback { token, shuffle },
            ));
            let message = catalog_text(&state.application, "loading_playlist")
                .replace("{title}", &item.title);
            set_status(state, &message, true);
            let _ = SetTimer(
                Some(window),
                YOUTUBE_TIMER_ID,
                YOUTUBE_TIMER_INTERVAL_MS,
                None,
            );
        }
        Err(error) => {
            let message = error.to_string();
            set_status(state, &message, true);
            show_error_message(window, &message);
            let _ = SetFocus(Some(state.list));
        }
    }
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
        open_library_item(window, item);
    }
}

/// Python's `open_library_item`: a favorited or history channel opens its
/// videos directly and a playlist opens its videos, both with Back returning
/// to the library screen. Everything else plays.
unsafe fn open_library_item(window: HWND, item: apricot_core::MediaItem) {
    match item.kind {
        apricot_core::MediaKind::Channel
            if item.source == apricot_core::MediaSource::Soundcloud =>
        {
            if let Some(state) = state(window) {
                let feature = catalog_text(&state.application, "open_channel");
                announce_unavailable_feature(window, &feature);
            }
        }
        apricot_core::MediaKind::Channel => {
            open_youtube_collection(window, item, YoutubeCollectionKind::ChannelVideos);
        }
        apricot_core::MediaKind::Playlist => {
            open_youtube_collection(window, item, YoutubeCollectionKind::PlaylistVideos);
        }
        // Python `play_selected` turns shuffle off.
        _ => start_media_item_with_shuffle(window, item, None, false),
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
    state.next_youtube_operation_token = state.next_youtube_operation_token.wrapping_add(1).max(1);
    let token = state.next_youtube_operation_token;
    let backend = media_resolve_backend(item, YOUTUBE_BACKEND.setting_value());
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
        // Python `on_player_fullscreen_changed` keeps focus on the checkbox.
        PlayerControlActivation::Action("player_fullscreen") => {
            toggle_player_fullscreen(window, true, true);
        }
        PlayerControlActivation::Action(action_id) => activate_action(window, action_id),
        PlayerControlActivation::SessionAutoplayNext => {
            toggle_player_session_setting(window, SessionToggle::AutoplayNext);
        }
        PlayerControlActivation::CopyDetails => copy_player_details(window),
        PlayerControlActivation::HideDetails => hide_player_details(window),
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
                let message = player_failed_message(&state.application, &error.to_string());
                show_error_message(window, &message);
                return;
            }
        }
    }
    persist_current_playback_position(state);
    let podcast_speed =
        metadata_number(&item, "podcast_speed_preset").filter(|speed| (0.25..=4.0).contains(speed));
    state.clip_preview = None;
    let generation = state.application.start_player_item_with_shuffle_at(
        item,
        session_shuffle,
        start_position_seconds,
    );
    if let Some(speed) = podcast_speed {
        state.application.set_player_speed(speed);
    }
    let Some(options) = playback_launch_options(state, start_position_seconds) else {
        let message = player_failed_message(&state.application, "mpv was not found");
        let _ = state
            .application
            .apply_playback_event(generation, PlaybackEvent::Failed(message.clone()));
        state.pending_queued_start = None;
        show_error_message(window, &message);
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
        let message = player_failed_message(&state.application, &error.to_string());
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
    let show_details = focus && state.application.settings().show_video_details_by_default;
    if focus {
        state.player_controls.hide_details();
    }
    state.player_controls.sync(&model);
    if show_details {
        let text = player_details_text(&state.application);
        state
            .player_controls
            .show_details(&player_details_labels(&state.application), &text);
    } else {
        sync_player_details(state);
    }
    let title = wide(&model.window_title);
    let _ = SetWindowTextW(window, PCWSTR(title.as_ptr()));
    layout_controls_state(window, state);
    if show_details {
        let _ = SetFocus(Some(state.player_controls.details_text()));
        set_status(
            state,
            &catalog_text(&state.application, "video_details"),
            true,
        );
        return;
    }
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

/// Python `last_activated_menu_action`: the screen the main menu selects
/// when the user comes back to it.
unsafe fn remember_menu_item(window: HWND, item_id: &'static str) {
    if let Some(state) = state_mut(window) {
        state.last_activated_menu_item = Some(item_id);
    }
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
    refresh_main_menu(state, MainMenuSelection::LastActivated);
    set_status(state, &catalog_text(&state.application, "ready"), false);
    layout_controls_state(window, state);
    let _ = SetFocus(Some(state.list));
}

unsafe fn show_search(window: HWND) {
    remember_menu_item(window, "search");
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

unsafe fn show_trending(window: HWND) {
    remember_menu_item(window, "trending");
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if !state.application.settings().enable_trending {
        let message = catalog_text(&state.application, "trending_disabled");
        set_status(state, &message, true);
        show_main_menu(window);
        return;
    }
    if state.application.current_route() != Route::Trending {
        state.application.navigate_main_menu();
        state
            .application
            .navigate_to(RouteFrame::new(Route::Trending));
    }
    state.view = MainView::Trending;
    set_control_text(state, state.trending_country_label, "trending_country");
    set_control_text(state, state.trending_category_label, "trending_category");
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    crate::accessibility_win32::set_control_name(
        state.trending_country,
        catalog.text("trending_country"),
    );
    crate::accessibility_win32::set_control_name(
        state.trending_category,
        catalog.text("trending_category"),
    );
    refresh_trending_category_choices(state);
    set_control_text(state, state.load_trending, "load_trending");
    crate::accessibility_win32::set_control_name(state.list, catalog.text("trending"));
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    add_list_string(state.list, catalog.text("search_results_empty"));
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
    set_status(state, &catalog_text(&state.application, "ready"), false);
    layout_controls_state(window, state);
    let _ = SetFocus(Some(state.trending_country));
    load_trending_results(window);
}

unsafe fn load_trending_results(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.view != MainView::Trending {
        return;
    }
    cancel_youtube_work(window, state);
    let country_index = selected_combo_index(state.trending_country)
        .unwrap_or_default()
        .min(YOUTUBE_TRENDING_COUNTRIES.len().saturating_sub(1));
    let category_index = selected_combo_index(state.trending_category)
        .unwrap_or_default()
        .min(YOUTUBE_TRENDING_CATEGORIES.len().saturating_sub(1));
    let country = YOUTUBE_TRENDING_COUNTRIES[country_index];
    let category = YOUTUBE_TRENDING_CATEGORIES[category_index];
    state.application.update_trending_route_context(
        country_index,
        category_index,
        country.code,
        category.code,
    );
    let work = match state
        .application
        .begin_youtube_trending(country.code, category.code)
    {
        Ok(work) => work,
        Err(error) => {
            set_status(state, &error.to_string(), true);
            return;
        }
    };
    state.hydrated_youtube_urls.clear();
    state.youtube_api_metadata_disabled_scopes.clear();
    state.deferred_youtube_metadata_rows.clear();
    let country_label = country.label_key;
    let category_label = catalog_text(&state.application, category.label_key);
    let message = catalog_text(&state.application, "trending_loading_official")
        .replace("{country}", country_label)
        .replace("{category}", &category_label);
    set_status(state, &message, true);
    start_youtube_trending_work(window, work);
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
    } else if resume.return_screen == "rss_items" {
        state.current_rss_feed_index = resume
            .return_data
            .get("feed_index")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_default();
        state.current_rss_item_index = resume
            .return_data
            .get("item_index")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_default();
        state.rss_visible_item_count = state.current_rss_item_index.saturating_add(1);
    }
    if resume.sequence_active {
        start_sequence_media_item(window, resume.item, None);
    } else {
        start_media_item(window, resume.item, None);
    }
}

unsafe fn show_direct_link(window: HWND) {
    remember_menu_item(window, "direct_link");
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
    remember_menu_item(
        window,
        if view == MainView::History {
            "history"
        } else {
            "favorites"
        },
    );
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
    refresh_media_collection(state, true);
    layout_controls_state(window, state);
}

unsafe fn show_notification_center(window: HWND) {
    remember_menu_item(window, "notification_center");
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
    // Python `show_notification_center` only moves focus to the list.
    refresh_notification_center(state, true, false, None);
    layout_controls_state(window, state);
}

unsafe fn show_subscriptions(window: HWND) {
    remember_menu_item(window, "subscriptions");
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != Route::Subscriptions {
        state.application.navigate_main_menu();
        state
            .application
            .navigate_to(RouteFrame::new(Route::Subscriptions));
    }
    state.view = MainView::Subscriptions;
    set_open_button_label(state, "subscription_open_videos");
    set_control_text(state, state.collection_remove, "remove");
    set_control_text(state, state.subscription_check, "subscription_check_now");
    set_control_text(
        state,
        state.subscription_new,
        "subscription_new_videos_button",
    );
    set_control_text(state, state.subscription_filter, "filter_category");
    set_control_text(state, state.subscription_set_category, "set_category");
    refresh_subscriptions(state, true, None);
    layout_controls_state(window, state);
}

unsafe fn show_rss_feeds(window: HWND) {
    remember_menu_item(window, "rss_feeds");
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if !state.application.settings().enable_podcasts_rss {
        show_main_menu(window);
        return;
    }
    if state.application.current_route() != Route::RssFeeds {
        state.application.navigate_main_menu();
        state
            .application
            .navigate_to(RouteFrame::new(Route::RssFeeds));
    }
    state.view = MainView::RssFeeds;
    set_open_button_label(state, "open_feed");
    set_control_text(state, state.collection_remove, "remove_feed");
    set_control_text(state, state.rss_refresh, "refresh_feeds");
    refresh_rss_feeds(state, true, None);
    layout_controls_state(window, state);
}

/// Python `refresh_rss_feed_list`: only an empty list writes the status bar.
unsafe fn refresh_rss_feeds(state: &mut WindowState, focus: bool, preferred_url: Option<&str>) {
    let previous_url = preferred_url
        .map(str::to_owned)
        .or_else(|| selected_rss_feed(state).map(|feed| feed.url.clone()));
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    crate::accessibility_win32::set_control_name(state.list, catalog.text("rss_feeds"));
    let visible = state.application.visible_rss_feed_indices();
    if state.application.rss_feeds().is_empty() {
        add_list_string(state.list, catalog.text("rss_feeds_empty"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("rss_feeds_empty"), false);
    } else if visible.is_empty() {
        add_list_string(state.list, catalog.text("category_filter_empty"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("category_filter_empty"), false);
    } else {
        for index in &visible {
            if let Some(feed) = state.application.rss_feeds().get(*index) {
                add_list_string(state.list, &rss_feed_label(feed, &catalog));
            }
        }
        let selected = previous_url
            .as_deref()
            .and_then(|url| {
                visible.iter().position(|index| {
                    state
                        .application
                        .rss_feeds()
                        .get(*index)
                        .is_some_and(|feed| feed.url.eq_ignore_ascii_case(url))
                })
            })
            .or_else(|| {
                visible
                    .iter()
                    .position(|index| *index == state.current_rss_feed_index)
            })
            .unwrap_or_default();
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
        state.current_rss_feed_index = visible[selected];
    }
    if let Some(error) = state.application.rss_feed_load_error() {
        show_error_message(
            GetParent(state.list).unwrap_or_default(),
            &format!("Podcast feeds could not be loaded: {error}"),
        );
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

fn rss_feed_label(
    feed: &apricot_app::RssFeed,
    catalog: &apricot_core::TranslationCatalog,
) -> String {
    let checked = feed
        .last_checked
        .and_then(format_timestamp)
        .unwrap_or_else(|| catalog.text("rss_feed_never_checked").to_owned());
    let played_count = feed
        .items
        .iter()
        .filter(|item| metadata_bool(item, "played"))
        .count();
    let mut parts = vec![if feed.title.trim().is_empty() {
        catalog.text("rss_unknown_feed_title").to_owned()
    } else {
        feed.title.clone()
    }];
    if !feed.category.trim().is_empty() {
        parts.push(
            catalog
                .text("category_value")
                .replace("{category}", &feed.category),
        );
    }
    parts.push(
        catalog
            .text("rss_feed_item_count")
            .replace("{count}", &feed.items.len().to_string()),
    );
    if played_count > 0 {
        parts.push(
            catalog
                .text("rss_feed_played_count")
                .replace("{count}", &played_count.to_string()),
        );
    }
    if let Some(speed) = feed.speed_preset {
        parts.push(
            catalog
                .text("podcast_speed_preset_marker")
                .replace("{speed}", &format_rate(speed)),
        );
    }
    parts.push(if feed.last_checked.is_some() {
        catalog
            .text("rss_feed_last_checked")
            .replace("{time}", &checked)
    } else {
        checked
    });
    parts.join(" | ")
}

unsafe fn selected_rss_feed_index(state: &WindowState) -> Option<usize> {
    let selected = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
    state
        .application
        .visible_rss_feed_indices()
        .get(selected)
        .copied()
}

unsafe fn selected_rss_feed(state: &WindowState) -> Option<&apricot_app::RssFeed> {
    let index = selected_rss_feed_index(state)?;
    state.application.rss_feeds().get(index)
}

unsafe fn open_selected_rss_feed(window: HWND) {
    let Some((index, refresh_legacy)) = state(window).and_then(|state| {
        let index = selected_rss_feed_index(state)?;
        let refresh_legacy = state
            .application
            .rss_feeds()
            .get(index)
            .is_some_and(|feed| feed.items_complete != Some(true));
        Some((index, refresh_legacy))
    }) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    state.current_rss_feed_index = index;
    state.current_rss_item_index = 0;
    state.rss_visible_item_count = 0;
    let mut frame = RouteFrame::new(Route::RssItems);
    frame
        .parameters
        .insert("feed_index".to_owned(), serde_json::Value::from(index));
    state.application.navigate_to(frame);
    state.view = MainView::RssItems;
    set_open_button_label(state, "play_episode");
    set_control_text(state, state.rss_refresh, "refresh_feed");
    refresh_rss_items(state, true, true);
    layout_controls_state(window, state);
    if refresh_legacy {
        refresh_rss_feed_background(window, index);
    }
}

unsafe fn refresh_rss_items(state: &mut WindowState, focus: bool, announce_status: bool) {
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    crate::accessibility_win32::set_control_name(state.list, catalog.text("rss_feed_items"));
    let Some(feed) = state
        .application
        .rss_feeds()
        .get(state.current_rss_feed_index)
    else {
        add_list_string(state.list, catalog.text("rss_items_empty"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("rss_items_empty"), announce_status);
        return;
    };
    if feed.items.is_empty() {
        state.rss_visible_item_count = 0;
        add_list_string(state.list, catalog.text("rss_items_empty"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("rss_items_empty"), announce_status);
    } else {
        let batch_size = usize::try_from(state.application.settings().rss_max_items.clamp(25, 500))
            .unwrap_or(100);
        state.rss_visible_item_count = state
            .rss_visible_item_count
            .max(batch_size)
            .min(feed.items.len());
        for item in &feed.items[..state.rss_visible_item_count] {
            add_list_string(state.list, &rss_episode_row(state, item, &catalog));
        }
        state.current_rss_item_index = state
            .current_rss_item_index
            .min(state.rss_visible_item_count.saturating_sub(1));
        SendMessageW(
            state.list,
            LB_SETCURSEL,
            Some(WPARAM(state.current_rss_item_index)),
            None,
        );
        set_status(
            state,
            &format!(
                "{}: {} of {}",
                feed.title,
                state.rss_visible_item_count,
                feed.items.len()
            ),
            announce_status,
        );
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

unsafe fn maybe_extend_rss_items(window: HWND) {
    let Some((selected, loaded)) = state(window).and_then(|state| {
        let total = state
            .application
            .rss_feeds()
            .get(state.current_rss_feed_index)?
            .items
            .len();
        (state.rss_visible_item_count < total).then(|| {
            (
                usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0)
                    .unwrap_or_default(),
                state
                    .application
                    .settings()
                    .rss_max_items
                    .clamp(25, 500)
                    .try_into()
                    .unwrap_or(100),
            )
        })
    }) else {
        return;
    };
    if let Some(state) = state_mut(window) {
        let before = state.rss_visible_item_count;
        state.rss_visible_item_count = state.rss_visible_item_count.saturating_add(loaded);
        state.current_rss_item_index = selected;
        refresh_rss_items(state, false, false);
        let added = state.rss_visible_item_count.saturating_sub(before);
        let message = catalog_text(&state.application, "podcast_more_episodes_loaded")
            .replace("{count}", &added.to_string());
        set_status(state, &message, true);
    }
}

/// Python's `rss_item_line` for one visible episode.
fn rss_episode_row(
    state: &WindowState,
    item: &apricot_core::MediaItem,
    catalog: &apricot_core::TranslationCatalog,
) -> String {
    rss_episode_label(
        item,
        catalog,
        (!metadata_bool(item, "played"))
            .then(|| state.application.playback_resume_position(item))
            .flatten(),
        state.application.downloads().queued_item(item).is_some(),
    )
}

/// Replaces one visible episode row and keeps the selection, as Python's
/// `refresh_rss_items_list(selection)` does after a queue change.
unsafe fn refresh_rss_episode_line(state: &WindowState, index: usize) {
    let Some(item) = state
        .application
        .rss_feeds()
        .get(state.current_rss_feed_index)
        .and_then(|feed| feed.items.get(index))
        .filter(|_| index < state.rss_visible_item_count)
    else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let label = wide(&rss_episode_row(state, item, &catalog));
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    SendMessageW(state.list, LB_DELETESTRING, Some(WPARAM(index)), None);
    SendMessageW(
        state.list,
        LB_INSERTSTRING,
        Some(WPARAM(index)),
        Some(LPARAM(label.as_ptr() as isize)),
    );
    if let Ok(selected) = usize::try_from(selected) {
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    }
}

fn rss_episode_label(
    item: &apricot_core::MediaItem,
    catalog: &apricot_core::TranslationCatalog,
    resume_position: Option<f64>,
    queued: bool,
) -> String {
    let mut parts = vec![item.title.clone()];
    if metadata_bool(item, "played") {
        parts.push(catalog.text("played_marker").to_owned());
    }
    if let Some(position) = resume_position {
        parts.push(
            catalog
                .text("episode_resume_marker")
                .replace("{time}", &format_duration(position)),
        );
    }
    if let Some(timestamp) = metadata_number(item, "timestamp").and_then(format_timestamp) {
        parts.push(format!("{}: {timestamp}", catalog.text("published")));
    }
    if let Some(duration) = metadata_text(item, "duration")
        .filter(|duration| !duration.is_empty())
        .or_else(|| item.duration_seconds.map(format_duration))
    {
        parts.push(duration);
    }
    parts.push(catalog.text("podcast_episode").to_owned());
    if queued {
        parts.push(catalog.text("podcast_audio_queued_marker").to_owned());
    }
    parts.join(" | ")
}

unsafe fn selected_rss_episode(state: &WindowState) -> Option<&apricot_core::MediaItem> {
    let selected = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
    if selected >= state.rss_visible_item_count {
        return None;
    }
    state
        .application
        .rss_feeds()
        .get(state.current_rss_feed_index)?
        .items
        .get(selected)
}

unsafe fn play_selected_rss_episode(window: HWND) {
    let Some((feed_index, item_index)) = state(window).and_then(|state| {
        let selected =
            usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
        (selected < state.rss_visible_item_count)
            .then_some((state.current_rss_feed_index, selected))
    }) else {
        return;
    };
    let item = state_mut(window).and_then(|state| {
        state.current_rss_item_index = item_index;
        state
            .application
            .prepare_rss_episode_playback(feed_index, item_index)
    });
    if let Some(item) = item {
        start_sequence_media_item(window, item, None);
    } else if let Some(state) = state_mut(window) {
        set_status(
            state,
            &catalog_text(&state.application, "no_selection"),
            true,
        );
    }
}

fn metadata_bool(item: &apricot_core::MediaItem, key: &str) -> bool {
    item.metadata
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn metadata_number(item: &apricot_core::MediaItem, key: &str) -> Option<f64> {
    item.metadata.get(key).and_then(serde_json::Value::as_f64)
}

fn metadata_text(item: &apricot_core::MediaItem, key: &str) -> Option<String> {
    item.metadata
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn format_rate(rate: f64) -> String {
    let value = format!("{rate:.2}");
    value.trim_end_matches('0').trim_end_matches('.').to_owned()
}

unsafe fn prompt_add_rss_feed(window: HWND) {
    let Some((title, prompt, ok, cancel, proxy, unknown_title)) = state_mut(window).map(|state| {
        state.modal_open = true;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        (
            catalog.text("add_rss_feed").to_owned(),
            catalog.text("rss_feed_url").to_owned(),
            catalog.text("ok").to_owned(),
            catalog.text("cancel").to_owned(),
            state.application.settings().proxy.clone(),
            catalog.text("rss_unknown_feed_title").to_owned(),
        )
    }) else {
        return;
    };
    let response = crate::playlist_dialog_win32::prompt_name(window, &title, &prompt, &ok, &cancel);
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    match response {
        Ok(Some(url)) if !url.trim().is_empty() => {
            let url = if url.trim().to_ascii_lowercase().starts_with("http://")
                || url.trim().to_ascii_lowercase().starts_with("https://")
            {
                url.trim().to_owned()
            } else {
                format!("https://{}", url.trim())
            };
            let work = crate::podcast_win32::add_feed(url, proxy, unknown_title, unix_timestamp());
            start_podcast_work(window, work, "rss_refresh_started");
        }
        Ok(_) => {}
        Err(error) => show_error_message(window, &format!("Add feed dialog did not open: {error}")),
    }
}

unsafe fn prompt_podcast_search(window: HWND) {
    let Some((title, prompt, ok, cancel, country, limit, proxy)) = state_mut(window).map(|state| {
        state.modal_open = true;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        (
            catalog.text("search_podcasts").to_owned(),
            catalog.text("podcast_search_query").to_owned(),
            catalog.text("search").to_owned(),
            catalog.text("cancel").to_owned(),
            state.application.settings().podcast_search_country.clone(),
            u32::try_from(
                state
                    .application
                    .settings()
                    .podcast_search_limit
                    .clamp(1, 200),
            )
            .unwrap_or(20),
            state.application.settings().proxy.clone(),
        )
    }) else {
        return;
    };
    let response = crate::playlist_dialog_win32::prompt_name(window, &title, &prompt, &ok, &cancel);
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    match response {
        Ok(Some(query)) if !query.trim().is_empty() => {
            let query = query.trim().to_owned();
            let message = state(window)
                .map(|state| {
                    catalog_text(&state.application, "podcast_searching").replace("{query}", &query)
                })
                .unwrap_or_default();
            let work = crate::podcast_win32::search_directory(query, country, limit, proxy);
            start_podcast_work_with_message(window, work, &message);
        }
        Ok(_) => {}
        Err(error) => {
            show_error_message(
                window,
                &format!("Podcast search dialog did not open: {error}"),
            );
        }
    }
}

unsafe fn start_podcast_work(window: HWND, work: PendingPodcastWork, status_key: &str) {
    let message = state(window)
        .map(|state| catalog_text(&state.application, status_key))
        .unwrap_or_default();
    start_podcast_work_with_message(window, work, &message);
}

unsafe fn start_podcast_work_with_message(window: HWND, work: PendingPodcastWork, message: &str) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.pending_podcast_work.is_some() {
        set_status(state, "A podcast operation is already in progress.", true);
        return;
    }
    state.pending_podcast_work = Some(work);
    set_status(state, message, true);
    let _ = SetTimer(
        Some(window),
        YOUTUBE_TIMER_ID,
        YOUTUBE_TIMER_INTERVAL_MS,
        None,
    );
}

unsafe fn refresh_rss_from_active_view(window: HWND) {
    let Some((view, feeds, proxy, unknown_title)) = state(window).map(|state| {
        let feeds = match state.view {
            MainView::RssItems => state
                .application
                .rss_feeds()
                .get(state.current_rss_feed_index)
                .map(|feed| vec![(feed.url.clone(), feed.url.clone())])
                .unwrap_or_default(),
            MainView::RssFeeds => state
                .application
                .rss_feeds()
                .iter()
                .map(|feed| (feed.url.clone(), feed.url.clone()))
                .collect(),
            _ => Vec::new(),
        };
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        (
            state.view,
            feeds,
            state.application.settings().proxy.clone(),
            catalog.text("rss_unknown_feed_title").to_owned(),
        )
    }) else {
        return;
    };
    if feeds.is_empty() {
        if let Some(state) = state_mut(window) {
            let key = if view == MainView::RssFeeds {
                "rss_feeds_empty"
            } else {
                "rss_items_empty"
            };
            set_status(state, &catalog_text(&state.application, key), true);
        }
        return;
    }
    let work =
        crate::podcast_win32::refresh_feeds(feeds, proxy, unknown_title, unix_timestamp(), false);
    start_podcast_work(window, work, "rss_refresh_started");
}

unsafe fn refresh_selected_rss_feed(window: HWND) {
    let Some((url, proxy, unknown_title)) = state(window).and_then(|state| {
        let feed = selected_rss_feed(state)?;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        Some((
            feed.url.clone(),
            state.application.settings().proxy.clone(),
            catalog.text("rss_unknown_feed_title").to_owned(),
        ))
    }) else {
        return;
    };
    let work = crate::podcast_win32::refresh_feeds(
        vec![(url.clone(), url)],
        proxy,
        unknown_title,
        unix_timestamp(),
        false,
    );
    start_podcast_work(window, work, "rss_refresh_started");
}

unsafe fn refresh_all_rss_feeds_background(window: HWND) {
    let Some((feeds, proxy, unknown_title)) = state(window).and_then(|state| {
        if state.modal_open
            || state.pending_podcast_work.is_some()
            || !state.application.settings().enable_podcasts_rss
            || state.application.rss_feeds().is_empty()
        {
            return None;
        }
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        Some((
            state
                .application
                .rss_feeds()
                .iter()
                .map(|feed| (feed.url.clone(), feed.url.clone()))
                .collect(),
            state.application.settings().proxy.clone(),
            catalog.text("rss_unknown_feed_title").to_owned(),
        ))
    }) else {
        return;
    };
    let work =
        crate::podcast_win32::refresh_feeds(feeds, proxy, unknown_title, unix_timestamp(), true);
    let Some(state) = state_mut(window) else {
        return;
    };
    state.pending_podcast_work = Some(work);
    let _ = SetTimer(
        Some(window),
        YOUTUBE_TIMER_ID,
        YOUTUBE_TIMER_INTERVAL_MS,
        None,
    );
}

unsafe fn refresh_rss_feed_background(window: HWND, feed_index: usize) {
    let Some((url, proxy, unknown_title)) = state(window).and_then(|state| {
        if state.modal_open || state.pending_podcast_work.is_some() {
            return None;
        }
        let feed = state.application.rss_feeds().get(feed_index)?;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        Some((
            feed.url.clone(),
            state.application.settings().proxy.clone(),
            catalog.text("rss_unknown_feed_title").to_owned(),
        ))
    }) else {
        return;
    };
    let work = crate::podcast_win32::refresh_feeds(
        vec![(url.clone(), url)],
        proxy,
        unknown_title,
        unix_timestamp(),
        true,
    );
    let Some(state) = state_mut(window) else {
        return;
    };
    state.pending_podcast_work = Some(work);
    let _ = SetTimer(
        Some(window),
        YOUTUBE_TIMER_ID,
        YOUTUBE_TIMER_INTERVAL_MS,
        None,
    );
}

#[allow(clippy::too_many_lines)]
unsafe fn poll_podcast_work(window: HWND) {
    let outcome = {
        let Some(state) = state(window) else {
            return;
        };
        let Some(pending) = state.pending_podcast_work.as_ref() else {
            return;
        };
        match pending.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(PodcastWorkResult::FeedAdded(Err(
                "Podcast worker stopped unexpectedly".to_owned(),
            ))),
        }
    };
    let Some(outcome) = outcome else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    state.pending_podcast_work = None;
    match outcome {
        PodcastWorkResult::FeedAdded(Ok(feed)) => {
            let title = feed.title.clone();
            match state.application.add_rss_feed(*feed) {
                Ok(RssFeedAddOutcome::Added(_)) => {
                    let message = catalog_text(&state.application, "rss_feed_added")
                        .replace("{title}", &title);
                    set_status(state, &message, true);
                    if state.view == MainView::RssFeeds {
                        refresh_rss_feeds(state, false, None);
                    }
                }
                Ok(RssFeedAddOutcome::AlreadyPresent) => {
                    set_status(
                        state,
                        &catalog_text(&state.application, "rss_feed_exists"),
                        true,
                    );
                }
                Err(error) => show_error_message(window, &error.to_string()),
            }
        }
        PodcastWorkResult::FeedAdded(Err(error)) => {
            let message =
                catalog_text(&state.application, "rss_refresh_failed").replace("{error}", &error);
            set_status(state, &message, true);
            show_error_message(window, &message);
        }
        PodcastWorkResult::FeedsRefreshed { results, silent } => {
            match state
                .application
                .apply_rss_refreshes(results, unix_timestamp())
            {
                Ok(summary) => {
                    let timestamp = unix_timestamp();
                    let catalog =
                        apricot_app::embedded_catalog(&state.application.settings().language);
                    for (feed, item) in &summary.new_items {
                        let notification_message = catalog
                            .text("notification_new_podcast")
                            .replace("{feed}", feed)
                            .replace("{title}", &item.title);
                        let _ =
                            state
                                .application
                                .add_notification(apricot_app::AppNotification::new(
                                    "podcast",
                                    catalog.text("rss_feeds"),
                                    &notification_message,
                                    Some(item.clone()),
                                    timestamp,
                                ));
                        if state.application.settings().windows_notifications {
                            show_tray_notification(
                                window,
                                catalog.text("rss_feeds"),
                                &notification_message,
                            );
                        }
                    }
                    if !silent {
                        let message = if summary.failures == 0 {
                            catalog_text(&state.application, "rss_refresh_done")
                        } else {
                            format!(
                                "{} {} succeeded, {} failed.",
                                catalog_text(&state.application, "rss_refresh_done"),
                                summary.successes,
                                summary.failures
                            )
                        };
                        set_status(state, &message, true);
                    }
                    if state.view == MainView::RssFeeds {
                        refresh_rss_feeds(state, false, None);
                    } else if state.view == MainView::RssItems {
                        refresh_rss_items(state, false, false);
                    }
                }
                Err(error) => show_error_message(window, &error.to_string()),
            }
        }
        PodcastWorkResult::DirectorySearched { query, result } => match result {
            Ok(results) => {
                state.podcast_search_query = query;
                state.podcast_search_results = results;
                state
                    .application
                    .navigate_to(RouteFrame::new(Route::PodcastSearchResults));
                state.view = MainView::PodcastSearchResults;
                refresh_podcast_directory_results(state, true, true);
                layout_controls_state(window, state);
            }
            Err(error) => {
                let message = catalog_text(&state.application, "podcast_search_failed")
                    .replace("{error}", &error);
                set_status(state, &message, true);
                show_error_message(window, &message);
            }
        },
        PodcastWorkResult::CategoryLoaded { category, result } => match result {
            Ok(results) => {
                state.podcast_search_query = category;
                state.podcast_search_results = results;
                let _ = state.application.navigate_back();
                state
                    .application
                    .navigate_to(RouteFrame::new(Route::PodcastSearchResults));
                state.view = MainView::PodcastSearchResults;
                set_open_button_label(state, "open");
                refresh_podcast_directory_results(state, true, true);
                layout_controls_state(window, state);
            }
            Err(error) => {
                let message = catalog_text(&state.application, "podcast_search_failed")
                    .replace("{error}", &error);
                set_status(state, &message, true);
                show_error_message(window, &message);
            }
        },
        PodcastWorkResult::FeedsImported { feeds, failures } => {
            match state.application.import_rss_feeds(feeds) {
                Ok(summary) => {
                    refresh_rss_feeds(state, false, None);
                    let key = if failures == 0 {
                        "opml_import_done"
                    } else {
                        "opml_import_done_with_errors"
                    };
                    let message = catalog_text(&state.application, key)
                        .replace("{count}", &summary.added.to_string())
                        .replace("{imported}", &summary.added.to_string())
                        .replace("{failed}", &failures.to_string());
                    set_status(state, &message, true);
                }
                Err(error) => show_error_message(window, &error.to_string()),
            }
        }
    }
    stop_youtube_timer(window);
}

unsafe fn refresh_podcast_directory_results(
    state: &mut WindowState,
    focus: bool,
    announce_status: bool,
) {
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let name = if state.view == MainView::PodcastCategories {
        catalog.text("podcast_categories_title")
    } else {
        catalog.text("podcast_search_results")
    };
    crate::accessibility_win32::set_control_name(state.list, name);
    if state.podcast_search_results.is_empty() {
        add_list_string(state.list, catalog.text("podcast_search_empty"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("podcast_search_empty"), announce_status);
    } else {
        for item in &state.podcast_search_results {
            add_list_string(state.list, &podcast_directory_label(item, &catalog));
        }
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(
            state,
            &catalog
                .text("podcast_search_done")
                .replace("{count}", &state.podcast_search_results.len().to_string()),
            announce_status,
        );
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

fn podcast_directory_label(
    item: &PodcastDirectoryItem,
    catalog: &apricot_core::TranslationCatalog,
) -> String {
    let mut parts = vec![item.title.clone()];
    if !item.author.is_empty() {
        parts.push(format!(
            "{}: {}",
            catalog.text("podcast_author"),
            item.author
        ));
    }
    if !item.genre.is_empty() {
        parts.push(format!("{}: {}", catalog.text("podcast_genre"), item.genre));
    }
    if item.episode_count > 0 {
        parts.push(
            catalog
                .text("podcast_episode_count")
                .replace("{count}", &item.episode_count.to_string()),
        );
    }
    parts.join(" | ")
}

unsafe fn selected_podcast_result(state: &WindowState) -> Option<&PodcastDirectoryItem> {
    let selected = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
    state.podcast_search_results.get(selected)
}

unsafe fn add_selected_podcast_result(window: HWND) {
    let Some((url, proxy, unknown_title)) = state(window).and_then(|state| {
        let item = selected_podcast_result(state)?;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        Some((
            item.feed_url.to_string(),
            state.application.settings().proxy.clone(),
            catalog.text("rss_unknown_feed_title").to_owned(),
        ))
    }) else {
        return;
    };
    let work = crate::podcast_win32::add_feed(url, proxy, unknown_title, unix_timestamp());
    start_podcast_work(window, work, "rss_refresh_started");
}

unsafe fn choose_rss_category_filter(window: HWND) {
    let Some((title, prompt, choices, current, ok, cancel)) = state_mut(window).map(|state| {
        state.modal_open = true;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        let mut choices = vec![catalog.text("all_categories").to_owned()];
        choices.extend(state.application.rss_categories());
        (
            catalog.text("filter_category").to_owned(),
            catalog.text("category_filter_prompt").to_owned(),
            choices,
            state.application.rss_category_filter().to_owned(),
            catalog.text("ok").to_owned(),
            catalog.text("cancel").to_owned(),
        )
    }) else {
        return;
    };
    let initial = choices
        .iter()
        .position(|choice| !current.is_empty() && choice.eq_ignore_ascii_case(&current))
        .unwrap_or_default();
    let selected = crate::playlist_dialog_win32::choose_with_initial(
        window, &title, &prompt, &choices, initial, &ok, &cancel,
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    match selected {
        Ok(Some(index)) => {
            let category = choices
                .get(index)
                .filter(|_| index > 0)
                .cloned()
                .unwrap_or_default();
            if let Some(state) = state_mut(window) {
                state.application.set_rss_category_filter(&category);
                refresh_rss_feeds(state, true, None);
                let message = if category.is_empty() {
                    catalog_text(&state.application, "category_filter_all")
                } else {
                    catalog_text(&state.application, "category_filter_applied")
                        .replace("{category}", &category)
                };
                set_status(state, &message, true);
            }
        }
        Ok(None) => {
            if let Some(state) = state(window) {
                let _ = SetFocus(Some(state.list));
            }
        }
        Err(error) => show_error_message(window, &format!("Category filter did not open: {error}")),
    }
}

unsafe fn set_selected_rss_category(window: HWND) {
    let Some((index, feed, title, prompt, ok, cancel)) = state_mut(window).and_then(|state| {
        let index = selected_rss_feed_index(state)?;
        let feed = state.application.rss_feeds().get(index)?.clone();
        state.modal_open = true;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        Some((
            index,
            feed.clone(),
            catalog.text("set_category").to_owned(),
            catalog
                .text("category_prompt")
                .replace("{title}", &feed.title),
            catalog.text("ok").to_owned(),
            catalog.text("cancel").to_owned(),
        ))
    }) else {
        return;
    };
    let response = crate::playlist_dialog_win32::prompt_name_with_initial(
        window,
        &title,
        &prompt,
        &feed.category,
        &ok,
        &cancel,
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    match response {
        Ok(Some(category)) => {
            let category = apricot_app::normalize_category(&category);
            let result = state_mut(window)
                .map(|state| state.application.set_rss_feed_category(index, &category));
            match result {
                Some(Ok(_)) => {
                    if let Some(state) = state_mut(window) {
                        refresh_rss_feeds(state, true, Some(&feed.url));
                        let message = if category.is_empty() {
                            catalog_text(&state.application, "category_cleared")
                                .replace("{title}", &feed.title)
                        } else {
                            catalog_text(&state.application, "category_assigned")
                                .replace("{title}", &feed.title)
                                .replace("{category}", &category)
                        };
                        set_status(state, &message, true);
                    }
                }
                Some(Err(error)) => show_error_message(window, &error.to_string()),
                None => {}
            }
        }
        Ok(None) => {
            if let Some(state) = state(window) {
                let _ = SetFocus(Some(state.list));
            }
        }
        Err(error) => show_error_message(window, &format!("Category editor did not open: {error}")),
    }
}

unsafe fn remove_selected_rss_feed(window: HWND) {
    let Some((index, preferred_url)) = state(window).and_then(|state| {
        let index = selected_rss_feed_index(state)?;
        let visible = state.application.visible_rss_feed_indices();
        let selected = visible.iter().position(|candidate| *candidate == index)?;
        let preferred = visible
            .get(selected + 1)
            .or_else(|| {
                selected
                    .checked_sub(1)
                    .and_then(|previous| visible.get(previous))
            })
            .and_then(|candidate| state.application.rss_feeds().get(*candidate))
            .map(|feed| feed.url.clone());
        Some((index, preferred))
    }) else {
        return;
    };
    let result = state_mut(window).map(|state| state.application.remove_rss_feed(index));
    match result {
        Some(Ok(Some(_))) => {
            if let Some(state) = state_mut(window) {
                refresh_rss_feeds(state, true, preferred_url.as_deref());
                set_status(
                    state,
                    &catalog_text(&state.application, "rss_feed_removed"),
                    true,
                );
            }
        }
        Some(Err(error)) => show_error_message(window, &error.to_string()),
        Some(Ok(None)) | None => {}
    }
}

unsafe fn choose_rss_speed_preset(window: HWND) {
    let Some((index, feed, title, prompt, choices, initial, ok, cancel)) = state_mut(window)
        .and_then(|state| {
            let index = if state.view == MainView::RssItems {
                state.current_rss_feed_index
            } else {
                selected_rss_feed_index(state)?
            };
            let feed = state.application.rss_feeds().get(index)?.clone();
            state.modal_open = true;
            let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
            let mut choices = vec![catalog.text("podcast_speed_use_global").to_owned()];
            choices.extend(PODCAST_SPEED_STEPS.iter().map(|speed| {
                catalog
                    .text("playback_rate_x")
                    .replace("{speed}", &format_rate(*speed))
            }));
            let initial = feed
                .speed_preset
                .and_then(|current| {
                    PODCAST_SPEED_STEPS
                        .iter()
                        .position(|speed| (*speed - current).abs() < 0.001)
                })
                .map_or(0, |position| position + 1);
            Some((
                index,
                feed.clone(),
                catalog.text("podcast_speed_preset").to_owned(),
                catalog
                    .text("podcast_speed_preset_prompt")
                    .replace("{title}", &feed.title),
                choices,
                initial,
                catalog.text("ok").to_owned(),
                catalog.text("cancel").to_owned(),
            ))
        })
    else {
        return;
    };
    let selected = crate::playlist_dialog_win32::choose_with_initial(
        window, &title, &prompt, &choices, initial, &ok, &cancel,
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    match selected {
        Ok(Some(choice)) => {
            let speed = choice
                .checked_sub(1)
                .and_then(|position| PODCAST_SPEED_STEPS.get(position))
                .copied();
            let result =
                state_mut(window).map(|state| state.application.set_rss_feed_speed(index, speed));
            match result {
                Some(Ok(_)) => {
                    if let Some(state) = state_mut(window) {
                        if state.view == MainView::RssFeeds {
                            refresh_rss_feeds(state, true, Some(&feed.url));
                        } else {
                            refresh_rss_items(state, true, false);
                        }
                        let message = speed.map_or_else(
                            || {
                                catalog_text(&state.application, "podcast_speed_preset_cleared")
                                    .replace("{title}", &feed.title)
                            },
                            |speed| {
                                catalog_text(&state.application, "podcast_speed_preset_saved")
                                    .replace("{title}", &feed.title)
                                    .replace("{speed}", &format_rate(speed))
                            },
                        );
                        set_status(state, &message, true);
                    }
                }
                Some(Err(error)) => show_error_message(window, &error.to_string()),
                None => {}
            }
        }
        Ok(None) => {
            if let Some(state) = state(window) {
                let _ = SetFocus(Some(state.list));
            }
        }
        Err(error) => show_error_message(window, &format!("Speed preset did not open: {error}")),
    }
}

unsafe fn toggle_selected_rss_played(window: HWND) {
    let Some((feed_index, item_index, item, played)) = state(window).and_then(|state| {
        let (feed_index, item_index, item) = active_rss_episode(state)?;
        Some((
            feed_index,
            item_index,
            item.clone(),
            !metadata_bool(&item, "played"),
        ))
    }) else {
        return;
    };
    let result = state_mut(window).map(|state| {
        state
            .application
            .set_rss_episode_played(feed_index, item_index, played, unix_timestamp())
    });
    match result {
        Some(Ok(Some(_))) => {
            if let Some(state) = state_mut(window) {
                if played {
                    let _ = state.application.clear_playback_position(&item);
                }
                if state.view == MainView::RssItems {
                    state.current_rss_item_index = item_index;
                    refresh_rss_items(state, true, false);
                }
                let key = if played {
                    "episode_marked_played"
                } else {
                    "episode_marked_unplayed"
                };
                let message = catalog_text(&state.application, key).replace("{title}", &item.title);
                set_status(state, &message, true);
            }
        }
        Some(Err(error)) => show_error_message(window, &error.to_string()),
        Some(Ok(None)) | None => {}
    }
}

unsafe fn clear_selected_rss_progress(window: HWND) {
    let Some((_, item_index, item)) = state(window).and_then(|state| active_rss_episode(state))
    else {
        return;
    };
    let result = state_mut(window).map(|state| state.application.clear_playback_position(&item));
    match result {
        Some(Ok(apricot_app::PlaybackPositionUpdate::Cleared)) => {
            if let Some(state) = state_mut(window) {
                if state.view == MainView::RssItems {
                    state.current_rss_item_index = item_index;
                    refresh_rss_items(state, true, false);
                }
                let message = catalog_text(&state.application, "episode_progress_cleared")
                    .replace("{title}", &item.title);
                set_status(state, &message, true);
            }
        }
        Some(Ok(_)) => {
            if let Some(state) = state(window) {
                set_status(
                    state,
                    &catalog_text(&state.application, "episode_progress_not_found"),
                    true,
                );
            }
        }
        Some(Err(error)) => show_error_message(window, &error.to_string()),
        None => {}
    }
}

unsafe fn active_rss_episode(
    state: &WindowState,
) -> Option<(usize, usize, apricot_core::MediaItem)> {
    if state.view == MainView::RssItems {
        let item_index =
            usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
        return selected_rss_episode(state)
            .cloned()
            .map(|item| (state.current_rss_feed_index, item_index, item));
    }
    current_podcast_episode(state)
}

fn current_podcast_episode(state: &WindowState) -> Option<(usize, usize, apricot_core::MediaItem)> {
    let item = state.application.player_session().current_item()?.clone();
    if item.kind != apricot_core::MediaKind::PodcastEpisode {
        return None;
    }
    let (feed_index, item_index) = state.application.rss_episode_location(&item)?;
    Some((feed_index, item_index, item))
}

unsafe fn mark_current_podcast_episode_played(state: &mut WindowState) {
    let Some((feed_index, item_index, item)) = current_podcast_episode(state) else {
        return;
    };
    if metadata_bool(&item, "played") {
        return;
    }
    match state
        .application
        .set_rss_episode_played(feed_index, item_index, true, unix_timestamp())
    {
        Ok(Some(_)) => {
            let _ = state.application.clear_playback_position(&item);
        }
        Ok(None) => {}
        Err(error) => {
            set_status(
                state,
                &format!("Podcast played state was not saved: {error}"),
                false,
            );
        }
    }
}

unsafe fn save_current_podcast_speed_preset(window: HWND) {
    let Some((feed_index, title, speed)) = state(window).and_then(|state| {
        let item = state.application.player_session().current_item()?;
        if item.kind != apricot_core::MediaKind::PodcastEpisode {
            return None;
        }
        let (feed_index, _) = state.application.rss_episode_location(item)?;
        let title = state.application.rss_feeds().get(feed_index)?.title.clone();
        let speed = state.application.player_session().audio()?.speed;
        Some((feed_index, title, speed))
    }) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "podcast_speed_preset_no_episode"),
                true,
            );
        }
        return;
    };
    let result = state_mut(window).map(|state| {
        state
            .application
            .set_rss_feed_speed(feed_index, Some(speed))
    });
    match result {
        Some(Ok(_)) => {
            if let Some(state) = state(window) {
                let message = catalog_text(&state.application, "podcast_speed_preset_saved")
                    .replace("{title}", &title)
                    .replace("{speed}", &format_rate(speed));
                set_status(state, &message, true);
            }
        }
        Some(Err(error)) => show_error_message(window, &error.to_string()),
        None => {}
    }
}

unsafe fn import_rss_opml(window: HWND) {
    let Some((title, type_label, proxy, unknown_title)) = state_mut(window).map(|state| {
        state.modal_open = true;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        (
            catalog.text("import_opml").to_owned(),
            catalog.text("opml_files").to_owned(),
            state.application.settings().proxy.clone(),
            catalog.text("rss_unknown_feed_title").to_owned(),
        )
    }) else {
        return;
    };
    let selected = crate::file_dialog_win32::choose_opml_file(window, &title, &type_label);
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    let path = match selected {
        Ok(Some(path)) => path,
        Ok(None) => return,
        Err(error) => {
            show_error_message(window, &error);
            return;
        }
    };
    let parsed = (|| {
        let file = fs::File::open(&path).map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.take((apricot_media::MAX_OPML_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        apricot_media::parse_opml(&bytes).map_err(|error| error.to_string())
    })();
    let feeds = match parsed {
        Ok(feeds) => feeds,
        Err(error) => {
            let message = state(window).map_or_else(
                || error.clone(),
                |state| {
                    catalog_text(&state.application, "opml_import_failed_msg")
                        .replace("{error}", &error)
                },
            );
            show_error_message(window, &message);
            return;
        }
    };
    let Some(entries) = state(window).map(|state| {
        let existing = state
            .application
            .rss_feeds()
            .iter()
            .filter_map(|feed| apricot_app::canonical_feed_url(&feed.url))
            .map(|url| url.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        feeds
            .into_iter()
            .filter(|feed| {
                let identity = apricot_app::canonical_feed_url(feed.url.as_str())
                    .unwrap_or_else(|| feed.url.to_string())
                    .to_ascii_lowercase();
                !existing.contains(&identity)
            })
            .map(|feed| (feed.url.to_string(), feed.title))
            .collect::<Vec<_>>()
    }) else {
        return;
    };
    if entries.is_empty() {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "opml_all_feeds_exist"),
                true,
            );
        }
        return;
    }
    let count = entries.len();
    let work = crate::podcast_win32::import_feeds(entries, proxy, unknown_title, unix_timestamp());
    let message = state(window)
        .map(|state| {
            catalog_text(&state.application, "opml_import_started")
                .replace("{count}", &count.to_string())
        })
        .unwrap_or_default();
    start_podcast_work_with_message(window, work, &message);
}

unsafe fn export_rss_opml(window: HWND) {
    let Some((title, type_label, feeds)) = state_mut(window).map(|state| {
        state.modal_open = true;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        (
            catalog.text("export_opml").to_owned(),
            catalog.text("opml_files").to_owned(),
            state
                .application
                .rss_feeds()
                .iter()
                .filter_map(|feed| {
                    Some(apricot_media::OpmlFeed {
                        title: feed.title.clone(),
                        url: feed.url.parse().ok()?,
                    })
                })
                .collect::<Vec<_>>(),
        )
    }) else {
        return;
    };
    if feeds.is_empty() {
        if let Some(state) = state_mut(window) {
            state.modal_open = false;
            set_status(
                state,
                &catalog_text(&state.application, "opml_no_feeds"),
                true,
            );
        }
        return;
    }
    let selected = crate::file_dialog_win32::save_opml_file(window, &title, &type_label);
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    let result = selected.and_then(|path| {
        path.map_or(Ok(None), |path| {
            let bytes = apricot_media::write_opml(&feeds).map_err(|error| error.to_string())?;
            fs::write(path, bytes)
                .map(|()| Some(()))
                .map_err(|error| error.to_string())
        })
    });
    match result {
        Ok(Some(())) => {
            if let Some(state) = state(window) {
                set_status(
                    state,
                    &catalog_text(&state.application, "opml_export_success"),
                    true,
                );
            }
        }
        Ok(None) => {}
        Err(error) => {
            let message = state(window).map_or_else(
                || error.clone(),
                |state| {
                    catalog_text(&state.application, "opml_export_failed")
                        .replace("{error}", &error)
                },
            );
            show_error_message(window, &message);
        }
    }
}

unsafe fn download_selected_rss_episode(window: HWND) {
    let Some(item) =
        state(window).and_then(|state| active_rss_episode(state).map(|(_, _, item)| item))
    else {
        return;
    };
    start_download_item(window, &item, DownloadChoice::Audio, false);
}

#[allow(clippy::too_many_lines)]
unsafe fn download_current_rss_feed(window: HWND) {
    let Some((title, items, ask_location, initial, language)) = state(window).and_then(|state| {
        let feed = state
            .application
            .rss_feeds()
            .get(state.current_rss_feed_index)?;
        let items = feed
            .items
            .iter()
            .filter(|item| item.url.is_some())
            .cloned()
            .collect::<Vec<_>>();
        let initial_item = items.first()?;
        let initial =
            download_folder_for_item(state.application.settings(), initial_item, true).ok()?;
        Some((
            feed.title.clone(),
            items,
            state.application.settings().ask_download_location_each_time,
            initial,
            state.application.settings().language.clone(),
        ))
    }) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "rss_items_empty"),
                true,
            );
        }
        return;
    };
    let output_override = if ask_location {
        let catalog = apricot_app::embedded_catalog(&language);
        match crate::folder_dialog_win32::choose_download_folder(
            window,
            catalog.text("choose_save_folder"),
            &initial,
        ) {
            Some(folder) => Some(folder),
            None => return,
        }
    } else {
        None
    };
    let mut requests = Vec::with_capacity(items.len());
    for item in &items {
        match build_download_request(
            window,
            item,
            DownloadChoice::Audio,
            output_override.as_deref(),
        ) {
            Ok(Some(request)) => requests.push(request),
            Ok(None) => return,
            Err(error) => {
                show_error_message(window, &error);
                return;
            }
        }
    }
    let Some(executable) =
        application_directory().map(|directory| directory.join("components").join("yt-dlp.exe"))
    else {
        return;
    };
    let (task_id, cancellation, sender) = {
        let Some(state) = state_mut(window) else {
            return;
        };
        let mut feed_item = items[0].clone();
        feed_item.title.clone_from(&title);
        feed_item.kind = apricot_core::MediaKind::PodcastFeed;
        let task_id = state.application.downloads_mut().begin(
            feed_item,
            DownloadChoice::Audio,
            DownloadTaskKind::PodcastFeed,
            requests.len(),
        );
        let cancellation = Arc::new(AtomicBool::new(false));
        state
            .download_cancellations
            .insert(task_id, Arc::clone(&cancellation));
        (task_id, cancellation, state.download_sender.clone())
    };
    let completion_directory = output_override.unwrap_or(initial);
    spawn_batch_download(
        task_id,
        executable,
        requests,
        completion_directory,
        cancellation,
        sender,
    );
    let _ = SetTimer(
        Some(window),
        DOWNLOAD_TIMER_ID,
        DOWNLOAD_TIMER_INTERVAL_MS,
        None,
    );
    if let Some(state) = state_mut(window) {
        set_status(
            state,
            &catalog_text(&state.application, "download_feed_start"),
            true,
        );
        refresh_download_projection(window, state, false);
    }
}

unsafe fn queue_selected_rss_episode_download(window: HWND) {
    let Some(item) =
        state(window).and_then(|state| active_rss_episode(state).map(|(_, _, item)| item))
    else {
        return;
    };
    toggle_download_queue_item(window, &item, DownloadChoice::Audio);
}

unsafe fn open_selected_podcast_in_browser(window: HWND) {
    let url = state(window).and_then(|state| match state.view {
        MainView::RssFeeds => selected_rss_feed(state).map(|feed| {
            if feed.site_url.trim().is_empty() {
                feed.url.clone()
            } else {
                feed.site_url.clone()
            }
        }),
        MainView::RssItems => selected_rss_episode(state).and_then(|item| {
            metadata_text(item, "webpage_url")
                .or_else(|| item.url.as_ref().map(ToString::to_string))
        }),
        MainView::PodcastSearchResults => {
            selected_podcast_result(state).map(|item| item.webpage_url.to_string())
        }
        _ => None,
    });
    let Some(url) = url else {
        return;
    };
    if let Err(error) = std::process::Command::new("explorer.exe").arg(&url).spawn() {
        show_error_message(window, &format!("Could not open the browser: {error}"));
    }
}

unsafe fn show_podcast_categories(window: HWND) {
    remember_menu_item(window, "rss_feeds");
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != Route::PodcastCategories {
        state
            .application
            .navigate_to(RouteFrame::new(Route::PodcastCategories));
    }
    state.view = MainView::PodcastCategories;
    set_open_button_label(state, "open");
    refresh_podcast_categories(state, true);
    layout_controls_state(window, state);
}

unsafe fn refresh_podcast_categories(state: &WindowState, focus: bool) {
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    crate::accessibility_win32::set_control_name(
        state.list,
        catalog.text("podcast_categories_title"),
    );
    for (key, _) in PODCAST_GENRES {
        add_list_string(state.list, catalog.text(key));
    }
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
    set_status(state, catalog.text("podcast_categories_title"), false);
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

unsafe fn open_selected_podcast_category(window: HWND) {
    let Some((category, genre_id, proxy, message)) = state(window).and_then(|state| {
        let selected =
            usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
        let (key, genre_id) = PODCAST_GENRES.get(selected).copied()?;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        Some((
            catalog.text(key).to_owned(),
            genre_id,
            state.application.settings().proxy.clone(),
            catalog.text("fetching_category_podcasts").to_owned(),
        ))
    }) else {
        return;
    };
    let work = crate::podcast_win32::load_category(category, genre_id, proxy);
    start_podcast_work_with_message(window, work, &message);
}

unsafe fn selected_subscription_index(state: &WindowState) -> Option<usize> {
    let selected = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
    state
        .application
        .visible_subscription_indices()
        .get(selected)
        .copied()
}

unsafe fn selected_subscription(state: &WindowState) -> Option<apricot_app::Subscription> {
    let index = selected_subscription_index(state)?;
    state.application.subscriptions().get(index).cloned()
}

fn subscription_media_item(
    subscription: &apricot_app::Subscription,
) -> Option<apricot_core::MediaItem> {
    let url = subscription.url.parse().ok()?;
    Some(apricot_core::MediaItem {
        id: apricot_core::MediaId(subscription.url.clone()),
        source: apricot_core::MediaSource::Youtube,
        kind: apricot_core::MediaKind::Channel,
        title: subscription.title.clone(),
        url: Some(url),
        stream_url: None,
        external_audio_url: None,
        local_path: None,
        channel: subscription.title.clone(),
        duration_seconds: None,
        metadata: std::collections::BTreeMap::new(),
    })
}

unsafe fn open_selected_subscription_videos(window: HWND) {
    let Some(item) = state(window)
        .and_then(|state| selected_subscription(state))
        .and_then(|subscription| subscription_media_item(&subscription))
    else {
        show_no_selection_message(window);
        return;
    };
    open_youtube_collection(window, item, YoutubeCollectionKind::ChannelVideos);
}

/// Python `self.message(self.t("no_selection"))`.
unsafe fn show_no_selection_message(window: HWND) {
    if let Some(message) =
        state(window).map(|state| catalog_text(&state.application, "no_selection"))
    {
        show_error_message(window, &message);
    }
}

unsafe fn open_selected_subscription_new_videos(window: HWND) {
    let Some(subscription) = state(window).and_then(|state| selected_subscription(state)) else {
        show_no_selection_message(window);
        return;
    };
    if subscription.last_new_items.is_empty() {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "subscription_no_saved_new_videos"),
                true,
            );
        }
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    let query = catalog_text(&state.application, "subscription_new_videos_title")
        .replace("{title}", &subscription.title);
    if !state
        .application
        .show_saved_subscription_results(query, subscription.last_new_items)
    {
        set_status(
            state,
            &catalog_text(&state.application, "subscription_no_saved_new_videos"),
            true,
        );
        return;
    }
    state
        .application
        .navigate_to(RouteFrame::new(Route::Results));
    state.view = MainView::Results;
    refresh_results(state, true);
    layout_controls_state(window, state);
}

unsafe fn remove_selected_subscription(window: HWND) {
    let Some((index, title)) = state(window).and_then(|state| {
        let index = selected_subscription_index(state)?;
        let title = state.application.subscriptions().get(index)?.title.clone();
        Some((index, title))
    }) else {
        return;
    };
    let result = state_mut(window).map(|state| state.application.remove_subscription(index));
    match result {
        Some(Ok(Some(_))) => {
            if let Some(state) = state_mut(window) {
                refresh_subscriptions(state, true, None);
                let message = catalog_text(&state.application, "subscription_removed")
                    .replace("{title}", &title);
                set_status(state, &message, true);
                layout_controls_state(window, state);
            }
        }
        Some(Err(error)) => show_error_message(window, &error.to_string()),
        Some(Ok(None)) | None => {}
    }
}

unsafe fn choose_subscription_category_filter(window: HWND) {
    let Some((title, prompt, choices, current, ok, cancel)) = state_mut(window).map(|state| {
        state.modal_open = true;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        let categories = state.application.subscription_categories();
        let mut choices = vec![catalog.text("all_categories").to_owned()];
        choices.extend(categories);
        (
            catalog.text("filter_category").to_owned(),
            catalog.text("category_filter_prompt").to_owned(),
            choices,
            state.application.subscription_category_filter().to_owned(),
            catalog.text("ok").to_owned(),
            catalog.text("cancel").to_owned(),
        )
    }) else {
        return;
    };
    let initial_selection = choices
        .iter()
        .position(|choice| !current.is_empty() && choice.eq_ignore_ascii_case(&current))
        .unwrap_or_default();
    let selected = crate::playlist_dialog_win32::choose_with_initial(
        window,
        &title,
        &prompt,
        &choices,
        initial_selection,
        &ok,
        &cancel,
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    match selected {
        Ok(Some(index)) => {
            let category = choices
                .get(index)
                .filter(|_| index > 0)
                .cloned()
                .unwrap_or_default();
            if let Some(state) = state_mut(window) {
                state
                    .application
                    .set_subscription_category_filter(&category);
                refresh_subscriptions(state, true, None);
                let message = if category.is_empty() {
                    catalog_text(&state.application, "category_filter_all")
                } else {
                    catalog_text(&state.application, "category_filter_applied")
                        .replace("{category}", &category)
                };
                set_status(state, &message, true);
                layout_controls_state(window, state);
            }
        }
        Ok(None) => {
            if let Some(state) = state(window) {
                let _ = SetFocus(Some(state.list));
            }
        }
        Err(error) => show_error_message(window, &format!("Category filter did not open: {error}")),
    }
}

unsafe fn set_selected_subscription_category(window: HWND) {
    let Some((index, subscription, title, prompt, ok, cancel)) =
        state_mut(window).and_then(|state| {
            let index = selected_subscription_index(state)?;
            let subscription = state.application.subscriptions().get(index)?.clone();
            state.modal_open = true;
            let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
            let prompt = catalog
                .text("category_prompt")
                .replace("{title}", &subscription.title);
            Some((
                index,
                subscription,
                catalog.text("set_category").to_owned(),
                prompt,
                catalog.text("ok").to_owned(),
                catalog.text("cancel").to_owned(),
            ))
        })
    else {
        return;
    };
    let response = crate::playlist_dialog_win32::prompt_name_with_initial(
        window,
        &title,
        &prompt,
        &subscription.category,
        &ok,
        &cancel,
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    match response {
        Ok(Some(category)) => {
            let category = apricot_app::normalize_category(&category);
            let result = state_mut(window).map(|state| {
                state
                    .application
                    .set_subscription_category(index, &category)
            });
            match result {
                Some(Ok(_)) => {
                    if let Some(state) = state_mut(window) {
                        refresh_subscriptions(state, true, Some(&subscription.url));
                        let message = if category.is_empty() {
                            catalog_text(&state.application, "category_cleared")
                                .replace("{title}", &subscription.title)
                        } else {
                            catalog_text(&state.application, "category_assigned")
                                .replace("{title}", &subscription.title)
                                .replace("{category}", &category)
                        };
                        set_status(state, &message, true);
                    }
                }
                Some(Err(error)) => show_error_message(window, &error.to_string()),
                None => {}
            }
        }
        Ok(None) => {
            if let Some(state) = state(window) {
                let _ = SetFocus(Some(state.list));
            }
        }
        Err(error) => show_error_message(window, &format!("Category editor did not open: {error}")),
    }
}

unsafe fn check_subscriptions(window: HWND, manual: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.pending_subscription_check.is_some() {
        return;
    }
    let queue = state
        .application
        .subscriptions()
        .iter()
        .map(|subscription| (subscription.url.clone(), subscription.title.clone()))
        .collect::<VecDeque<_>>();
    if queue.is_empty() {
        if manual {
            set_status(
                state,
                &catalog_text(&state.application, "subscription_empty"),
                true,
            );
        }
        return;
    }
    state.pending_subscription_check = Some(PendingSubscriptionCheck {
        manual,
        queue,
        current: None,
        results: Vec::new(),
        errors: Vec::new(),
    });
    if manual {
        set_status(
            state,
            &catalog_text(&state.application, "subscription_checking"),
            true,
        );
    }
    let _ = SetTimer(
        Some(window),
        YOUTUBE_TIMER_ID,
        YOUTUBE_TIMER_INTERVAL_MS,
        None,
    );
    start_next_subscription_check(window);
}

unsafe fn configure_subscription_timer(window: HWND) {
    let _ = KillTimer(Some(window), SUBSCRIPTION_TIMER_ID);
    let Some(state) = state(window) else {
        return;
    };
    if !state.application.settings().subscription_check_enabled {
        return;
    }
    let hours = state
        .application
        .settings()
        .subscription_check_interval_hours;
    let seconds = if hours.is_finite() {
        hours.clamp(0.5, 168.0) * 60.0 * 60.0
    } else {
        6.0 * 60.0 * 60.0
    };
    let interval_ms = std::time::Duration::try_from_secs_f64(seconds)
        .ok()
        .and_then(|duration| u32::try_from(duration.as_millis()).ok())
        .unwrap_or(21_600_000);
    let _ = SetTimer(Some(window), SUBSCRIPTION_TIMER_ID, interval_ms, None);
}

unsafe fn configure_rss_timer(window: HWND) {
    let _ = KillTimer(Some(window), RSS_TIMER_ID);
    let Some(state) = state(window) else {
        return;
    };
    let settings = state.application.settings();
    if !settings.enable_podcasts_rss || !settings.rss_auto_refresh_enabled {
        return;
    }
    let hours = settings.rss_refresh_interval_hours;
    let seconds = if hours.is_finite() {
        hours.clamp(0.5, 168.0) * 60.0 * 60.0
    } else {
        12.0 * 60.0 * 60.0
    };
    let interval_ms = std::time::Duration::try_from_secs_f64(seconds)
        .ok()
        .and_then(|duration| u32::try_from(duration.as_millis()).ok())
        .unwrap_or(43_200_000);
    let _ = SetTimer(Some(window), RSS_TIMER_ID, interval_ms, None);
}

unsafe fn refresh_rss_feeds_on_startup(window: HWND) {
    if state(window).is_some_and(|state| {
        state.application.settings().enable_podcasts_rss
            && state.application.settings().rss_refresh_on_startup
    }) {
        refresh_all_rss_feeds_background(window);
    }
}

unsafe fn check_subscriptions_if_due(window: HWND) {
    let due = state(window).is_some_and(|state| {
        if state.modal_open
            || !state.application.settings().subscription_check_enabled
            || state.application.subscriptions().is_empty()
        {
            return false;
        }
        let hours = state
            .application
            .settings()
            .subscription_check_interval_hours;
        let interval = if hours.is_finite() {
            hours.clamp(0.5, 168.0) * 60.0 * 60.0
        } else {
            6.0 * 60.0 * 60.0
        };
        let last = state.application.settings().last_subscription_check;
        !last.is_finite() || unix_timestamp() - last.max(0.0) >= interval
    });
    if due {
        check_subscriptions(window, false);
    }
}

unsafe fn start_next_subscription_check(window: HWND) {
    loop {
        let next = state_mut(window).and_then(|state| {
            state
                .pending_subscription_check
                .as_mut()
                .and_then(|pending| pending.queue.pop_front())
        });
        let Some((url, title)) = next else {
            finish_subscription_check(window);
            return;
        };
        let start: std::result::Result<(), String> = {
            let Some(state) = state_mut(window) else {
                return;
            };
            state.next_youtube_operation_token =
                state.next_youtube_operation_token.wrapping_add(1).max(1);
            let token = state.next_youtube_operation_token;
            if let Some(pending) = state.pending_subscription_check.as_mut() {
                pending.current = Some((token, url.clone(), title));
            }
            let backend = collection_backend(YOUTUBE_BACKEND, YoutubeCollectionKind::ChannelVideos);
            match application_directory().map(|path| path.join("components")) {
                Some(components) => {
                    let config = youtube_session_config(state);
                    state
                        .youtube_subscriptions
                        .start_collection(
                            backend,
                            &components,
                            config,
                            token,
                            url.clone(),
                            YoutubeCollectionKind::ChannelVideos,
                            5,
                        )
                        .map_err(|error| error.to_string())
                }
                None => Err("Application path is unavailable".to_owned()),
            }
        };
        match start {
            Ok(()) => return,
            Err(message) => {
                if let Some(state) = state_mut(window)
                    && let Some(pending) = state.pending_subscription_check.as_mut()
                {
                    pending.current = None;
                    pending.errors.push(message.clone());
                    pending.results.push(SubscriptionCheckResult {
                        url,
                        result: Err(message),
                    });
                }
            }
        }
    }
}

unsafe fn poll_subscription_runtime(window: HWND) {
    let update = {
        let Some(state) = state_mut(window) else {
            return;
        };
        state.youtube_subscriptions.poll()
    };
    let update = match update {
        Ok(Some(update)) => update,
        Ok(None) => return,
        Err(error) => {
            finish_subscription_item(window, 0, Err(error.to_string()));
            return;
        }
    };
    match update {
        YoutubeSearchServiceUpdate::Results { token, items, .. } => {
            finish_subscription_item(window, token, Ok(items));
        }
        YoutubeSearchServiceUpdate::Failed { token, message } => {
            finish_subscription_item(window, token, Err(message));
        }
        YoutubeSearchServiceUpdate::Resolved { token, .. }
        | YoutubeSearchServiceUpdate::Hydrated { token, .. } => finish_subscription_item(
            window,
            token,
            Err("YouTube component returned an unexpected subscription response".to_owned()),
        ),
    }
}

unsafe fn finish_subscription_item(
    window: HWND,
    token: u64,
    result: std::result::Result<Vec<apricot_core::MediaItem>, String>,
) {
    let matched = state_mut(window).is_some_and(|state| {
        let Some(pending) = state.pending_subscription_check.as_mut() else {
            return false;
        };
        let Some((current_token, url, title)) = pending.current.take() else {
            return false;
        };
        if token != 0 && current_token != token {
            pending.current = Some((current_token, url, title));
            return false;
        }
        if let Err(error) = &result {
            pending.errors.push(error.clone());
        }
        pending
            .results
            .push(SubscriptionCheckResult { url, result });
        true
    });
    if matched {
        start_next_subscription_check(window);
    }
}

unsafe fn finish_subscription_check(window: HWND) {
    let Some(pending) = state_mut(window).and_then(|state| state.pending_subscription_check.take())
    else {
        stop_youtube_timer(window);
        return;
    };
    let timestamp = unix_timestamp();
    let outcome = state_mut(window).map(|state| {
        state
            .application
            .apply_subscription_checks(pending.results, timestamp)
    });
    let Some(outcome) = outcome else {
        return;
    };
    let summary = match outcome {
        Ok(summary) => summary,
        Err(error) => {
            show_error_message(window, &error.to_string());
            stop_youtube_timer(window);
            return;
        }
    };
    if let Some(state) = state_mut(window) {
        if summary.successes > 0
            && let Err(error) = state.application.record_subscription_check(timestamp)
        {
            set_status(
                state,
                &format!("Subscription check time was not saved: {error}"),
                true,
            );
        }
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        let notification_title = catalog.text("notification_subscription_title").to_owned();
        let mut channel_counts: Vec<(String, usize)> = Vec::new();
        for (channel, item) in summary.new_items {
            if let Some((_, count)) = channel_counts
                .iter_mut()
                .find(|(known, _)| known == &channel)
            {
                *count += 1;
            } else {
                channel_counts.push((channel.clone(), 1));
            }
            let notification_message = catalog
                .text("notification_new_video")
                .replace("{channel}", &channel)
                .replace("{title}", &item.title);
            if let Err(error) =
                state
                    .application
                    .add_notification(apricot_app::AppNotification::new(
                        "subscription",
                        &notification_title,
                        &notification_message,
                        Some(item),
                        timestamp,
                    ))
            {
                set_status(
                    state,
                    &format!("Subscription notification was not saved: {error}"),
                    true,
                );
            }
            if state.application.settings().windows_notifications
                && state.application.settings().subscription_notifications
            {
                show_tray_notification(window, &notification_title, &notification_message);
            }
        }
        for (channel, count) in channel_counts {
            let message = catalog
                .text("subscription_new_videos")
                .replace("{count}", &count.to_string())
                .replace("{title}", &channel);
            set_status(state, &message, true);
        }
        if state.view == MainView::Subscriptions {
            refresh_subscriptions(state, true, None);
            layout_controls_state(window, state);
        }
        if pending.manual {
            let final_message = if summary.successes == 0 && summary.failures > 0 {
                catalog.text("subscription_check_failed").replace(
                    "{error}",
                    pending
                        .errors
                        .last()
                        .map_or("Unknown error", String::as_str),
                )
            } else if summary.total_new > 0 || summary.failures > 0 {
                catalog.text("subscription_check_complete").to_owned()
            } else {
                catalog.text("subscription_no_new").to_owned()
            };
            set_status(state, &final_message, true);
        }
    }
    stop_youtube_timer(window);
}

unsafe fn show_user_playlists(window: HWND) {
    remember_menu_item(window, "playlists");
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
    refresh_user_playlists(state, true);
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

#[allow(clippy::too_many_lines)]
unsafe fn navigate_back(window: HWND) {
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    let leaving_youtube_collection = state.view == MainView::YoutubeCollection;
    if state.view == MainView::Player {
        close_player_runtime(window, state);
    }
    if leaving_youtube_collection {
        let _ = state.application.pop_youtube_collection();
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
        Route::Trending => {
            restore_trending_view(window, state, &frame);
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
        Route::ChannelResults | Route::PlaylistResults => {
            state.view = MainView::YoutubeCollection;
            refresh_youtube_collection(state, true);
            layout_controls_state(window, state);
        }
        Route::LocalFolder => {
            state.view = MainView::LocalFolder;
            refresh_local_folder(state, true, false);
            layout_controls_state(window, state);
        }
        Route::Favorites => {
            state.view = MainView::Favorites;
            refresh_media_collection(state, true);
            select_list_index(state.list, saved_index);
            layout_controls_state(window, state);
        }
        Route::History => {
            state.view = MainView::History;
            refresh_media_collection(state, true);
            select_list_index(state.list, saved_index);
            layout_controls_state(window, state);
        }
        Route::NotificationCenter => {
            state.view = MainView::NotificationCenter;
            refresh_notification_center(state, true, false, saved_index);
            layout_controls_state(window, state);
        }
        Route::Subscriptions => {
            state.view = MainView::Subscriptions;
            refresh_subscriptions(state, true, None);
            layout_controls_state(window, state);
        }
        Route::RssFeeds => {
            state.view = MainView::RssFeeds;
            refresh_rss_feeds(state, true, None);
            layout_controls_state(window, state);
        }
        Route::RssItems => {
            state.view = MainView::RssItems;
            refresh_rss_items(state, true, false);
            select_list_index(state.list, saved_index);
            layout_controls_state(window, state);
        }
        Route::PodcastSearchResults => {
            state.view = MainView::PodcastSearchResults;
            refresh_podcast_directory_results(state, true, false);
            select_list_index(state.list, saved_index);
            layout_controls_state(window, state);
        }
        Route::PodcastCategories => {
            state.view = MainView::PodcastCategories;
            set_open_button_label(state, "open");
            refresh_podcast_categories(state, true);
            select_list_index(state.list, saved_index);
            layout_controls_state(window, state);
        }
        Route::Bookmarks => {
            state.view = MainView::MainMenu;
            refresh_main_menu(state, MainMenuSelection::LastActivated);
            layout_controls_state(window, state);
            show_bookmarks_dialog(window, false, true);
        }
        Route::UserPlaylists => {
            state.view = MainView::UserPlaylists;
            refresh_user_playlists(state, true);
            layout_controls_state(window, state);
        }
        Route::UserPlaylistItems => {
            state.view = MainView::UserPlaylistItems;
            refresh_user_playlist_items(state, true, false);
            layout_controls_state(window, state);
        }
        Route::DownloadQueue => {
            state.view = MainView::DownloadQueue;
            refresh_download_queue(state, true, false);
            layout_controls_state(window, state);
        }
        Route::Player => {
            state.view = MainView::Player;
            refresh_player(window, state, true, true);
        }
        _ => {
            state.application.navigate_main_menu();
            state.view = MainView::MainMenu;
            refresh_main_menu(state, MainMenuSelection::LastActivated);
            layout_controls_state(window, state);
            let _ = SetFocus(Some(state.list));
        }
    }
}

unsafe fn restore_trending_view(window: HWND, state: &mut WindowState, frame: &RouteFrame) {
    state.view = MainView::Trending;
    restore_trending_controls(state, frame);
    refresh_results(state, true);
    layout_controls_state(window, state);
}

unsafe fn restore_trending_controls(state: &WindowState, frame: &RouteFrame) {
    let country_index = frame
        .parameters
        .get("country_index")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or_default()
        .min(YOUTUBE_TRENDING_COUNTRIES.len().saturating_sub(1));
    let category_index = frame
        .parameters
        .get("category_index")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or_default()
        .min(YOUTUBE_TRENDING_CATEGORIES.len().saturating_sub(1));
    SendMessageW(
        state.trending_country,
        CB_SETCURSEL,
        Some(WPARAM(country_index)),
        None,
    );
    SendMessageW(
        state.trending_category,
        CB_SETCURSEL,
        Some(WPARAM(category_index)),
        None,
    );
}

unsafe fn refresh_trending_category_choices(state: &WindowState) {
    let selected = selected_combo_index(state.trending_category).unwrap_or_default();
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    SendMessageW(state.trending_category, CB_RESETCONTENT, None, None);
    for choice in YOUTUBE_TRENDING_CATEGORIES {
        let label = wide(catalog.text(choice.label_key));
        SendMessageW(
            state.trending_category,
            CB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    }
    SendMessageW(
        state.trending_category,
        CB_SETCURSEL,
        Some(WPARAM(
            selected.min(YOUTUBE_TRENDING_CATEGORIES.len().saturating_sub(1)),
        )),
        None,
    );
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

/// Python `direct_link_item`: a link without a scheme gets `https://`.
fn direct_link_with_scheme(value: &str) -> String {
    let value = value.trim();
    let has_scheme = value.split_once("://").is_some_and(|(scheme, _)| {
        let mut characters = scheme.chars();
        characters
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
            && characters.all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '.' | '-')
            })
    });
    if has_scheme {
        value.to_owned()
    } else {
        format!("https://{value}")
    }
}

unsafe fn activate_direct_link(window: HWND, action: &str) {
    let text = state(window).map(|state| window_text(state.search_edit));
    // Python `direct_link_item` returns nothing for an empty field and the
    // action shows the `no_selection` message box.
    if text.as_deref().is_none_or(|text| text.trim().is_empty()) {
        show_no_selection_message(window);
        return;
    }
    let item = text.and_then(|value| {
        apricot_core::MediaItem::from_direct_link(&direct_link_with_scheme(&value))
    });
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
        "download_audio" => start_download_item(window, &item, DownloadChoice::Audio, false),
        "download_video" => start_download_item(window, &item, DownloadChoice::Video, false),
        _ => start_youtube_resolve(window, &item, YoutubeResolvePurpose::Playback),
    }
}

unsafe fn show_download_queue(window: HWND) {
    remember_menu_item(window, "current_downloads");
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != Route::DownloadQueue {
        state.application.navigate_main_menu();
        state
            .application
            .navigate_to(RouteFrame::new(Route::DownloadQueue));
    }
    state.view = MainView::DownloadQueue;
    refresh_download_queue(state, true, true);
    layout_controls_state(window, state);
}

unsafe fn refresh_download_queue(state: &mut WindowState, focus: bool, announce_status: bool) {
    let previous = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok();
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    crate::accessibility_win32::set_control_name(state.list, catalog.text("current_downloads"));
    set_open_button_label(state, "download_selected_queued");
    let downloads = state.application.downloads();
    for task in downloads.active() {
        add_list_string(state.list, &active_download_label(task, &catalog));
    }
    for queued in downloads.queued() {
        add_list_string(state.list, &queued_download_label(queued, &catalog));
    }
    let count = downloads.count();
    if count == 0 {
        add_list_string(state.list, catalog.text("no_queued_downloads"));
    }
    select_list_index(state.list, previous.filter(|_| count > 0).or(Some(0)));
    if announce_status {
        let message = if count == 0 {
            catalog.text("no_queued_downloads").to_owned()
        } else {
            format!("{}: {count}", catalog.text("current_downloads"))
        };
        set_status(state, &message, true);
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

fn active_download_label(
    task: &apricot_app::ActiveDownload,
    catalog: &apricot_core::locale::TranslationCatalog,
) -> String {
    let state_key = match task.status {
        DownloadTaskStatus::Downloading => "download_state_downloading",
        DownloadTaskStatus::Processing => "download_state_processing",
        DownloadTaskStatus::CancelRequested => "download_state_cancelled",
    };
    let mut parts = vec![task.title.clone(), catalog.text(state_key).to_owned()];
    if task.total > 0 {
        parts.push(
            catalog
                .text("downloads_remaining")
                .replace("{remaining}", &task.remaining().to_string())
                .replace("{total}", &task.total.to_string()),
        );
    }
    if task.current_title != task.title || task.total > 1 {
        parts.push(task.current_title.clone());
    }
    if let Some(percent) = task.percent.filter(|percent| *percent > 0.0) {
        parts.push(
            catalog
                .text("download_percent_value")
                .replace("{percent}", &format!("{percent:.0}")),
        );
    }
    parts.join(" | ")
}

/// Python's `queue_mode_label`.
const fn queued_marker_key(queued: &apricot_app::QueuedDownload) -> &'static str {
    match (queued.item.kind, queued.choice) {
        (apricot_core::MediaKind::PodcastEpisode, _) => "podcast_audio_queued_marker",
        (
            apricot_core::MediaKind::Playlist | apricot_core::MediaKind::Channel,
            DownloadChoice::Audio,
        ) => "collection_audio_queued_marker",
        (
            apricot_core::MediaKind::Playlist | apricot_core::MediaKind::Channel,
            DownloadChoice::Video,
        ) => "collection_video_queued_marker",
        (_, DownloadChoice::Audio) => "audio_queued_marker",
        (_, DownloadChoice::Video) => "video_queued_marker",
        (_, DownloadChoice::Ask) => "selected_queued_marker",
    }
}

fn queued_download_label(
    queued: &apricot_app::QueuedDownload,
    catalog: &apricot_core::locale::TranslationCatalog,
) -> String {
    let mode_key = queued_marker_key(queued);
    let mut parts = vec![
        queued.item.title.clone(),
        media_kind_label(&queued.item, catalog).to_owned(),
    ];
    if queued.item.kind == apricot_core::MediaKind::Video && !queued.item.channel.trim().is_empty()
    {
        parts.push(format!(
            "{}: {}",
            catalog.text("channel"),
            queued.item.channel
        ));
    }
    parts.extend([
        catalog.text(mode_key).to_owned(),
        catalog.text("download_state_queued").to_owned(),
    ]);
    parts.join(" | ")
}

fn media_kind_label<'a>(
    item: &apricot_core::MediaItem,
    catalog: &'a apricot_core::locale::TranslationCatalog,
) -> &'a str {
    let key = match item.kind {
        apricot_core::MediaKind::Audio => "download_audio_mode",
        apricot_core::MediaKind::Video => "video",
        apricot_core::MediaKind::LiveStream => "live_stream",
        apricot_core::MediaKind::Playlist => "playlist",
        apricot_core::MediaKind::Channel => "channel",
        apricot_core::MediaKind::PodcastFeed => "rss_feeds",
        apricot_core::MediaKind::PodcastEpisode => "podcast_episode",
        apricot_core::MediaKind::Movie => "movie",
        apricot_core::MediaKind::TvShow => "tv_show",
        apricot_core::MediaKind::TvEpisode => "episode",
        apricot_core::MediaKind::Unknown => "unknown",
    };
    catalog.text(key)
}

unsafe fn activate_selected_download(window: HWND) {
    start_selected_queued_download(window, None);
}

unsafe fn start_selected_queued_download(window: HWND, requested_choice: Option<DownloadChoice>) {
    let Some((selected, active_count, queued_choice, queued_item)) =
        state(window).and_then(|state| {
            let selected =
                usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
            let active_count = state.application.downloads().active().len();
            let queued = selected
                .checked_sub(active_count)
                .and_then(|index| state.application.downloads().queued().get(index));
            Some((
                selected,
                active_count,
                queued.map(|item| item.choice),
                queued.map(|item| item.item.clone()),
            ))
        })
    else {
        return;
    };
    if selected < active_count {
        cancel_selected_download(window);
        return;
    }
    let Some(item) = queued_item else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "download_queue_empty"),
                true,
            );
        }
        return;
    };
    let choice = requested_choice.unwrap_or(queued_choice.unwrap_or(DownloadChoice::Ask));
    let choice = match choice {
        DownloadChoice::Ask => match choose_download_format(window) {
            Some(choice) => choice,
            None => return,
        },
        choice => choice,
    };
    start_download_item(window, &item, choice, true);
}

unsafe fn choose_download_format(window: HWND) -> Option<DownloadChoice> {
    let (title, prompt, choices, ok, cancel) = state_mut(window).map(|state| {
        state.modal_open = true;
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        (
            catalog.text("select_download_format").to_owned(),
            catalog.text("select_download_format_message").to_owned(),
            vec![
                catalog.text("download_audio").to_owned(),
                catalog.text("download_video").to_owned(),
            ],
            catalog.text("ok").to_owned(),
            catalog.text("cancel").to_owned(),
        )
    })?;
    let result = crate::playlist_dialog_win32::choose_with_initial(
        window, &title, &prompt, &choices, 0, &ok, &cancel,
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    match result {
        Ok(Some(0)) => Some(DownloadChoice::Audio),
        Ok(Some(_)) => Some(DownloadChoice::Video),
        Ok(None) => None,
        Err(error) => {
            show_error_message(
                window,
                &format!("Download format dialog did not open: {error}"),
            );
            None
        }
    }
}

/// One press of a download shortcut, as Python's `last_download_shortcut`.
#[derive(Clone, Debug, PartialEq)]
struct DownloadShortcutPress {
    choice: DownloadChoice,
    identity: String,
    at: std::time::Instant,
}

/// Python ignores the same download shortcut for the same item within 0.35 s,
/// so a repeated or doubled key press does not start the download twice.
const DOWNLOAD_SHORTCUT_REPEAT_WINDOW: std::time::Duration = std::time::Duration::from_millis(350);

fn accept_download_shortcut(
    last: &mut Option<DownloadShortcutPress>,
    press: DownloadShortcutPress,
) -> bool {
    if let Some(previous) = last.as_ref()
        && previous.choice == press.choice
        && previous.identity == press.identity
        && press.at.saturating_duration_since(previous.at) < DOWNLOAD_SHORTCUT_REPEAT_WINDOW
    {
        return false;
    }
    *last = Some(press);
    true
}

/// Python's `start_download_shortcut`.
unsafe fn start_download_shortcut(window: HWND, choice: DownloadChoice) {
    if state(window).is_none_or(|state| state.view == MainView::MainMenu) {
        return;
    }
    let identity = active_media_item(window)
        .and_then(|item| item.stable_identity())
        .unwrap_or_default();
    let press = DownloadShortcutPress {
        choice,
        identity,
        at: std::time::Instant::now(),
    };
    let accepted = state_mut(window)
        .is_some_and(|state| accept_download_shortcut(&mut state.last_download_shortcut, press));
    if accepted {
        start_active_download(window, choice);
    }
}

unsafe fn start_active_download(window: HWND, requested_choice: DownloadChoice) {
    if state(window).is_some_and(|state| {
        state.view == MainView::Player
            && (state
                .application
                .player_session()
                .clip_start_seconds()
                .is_some()
                || state
                    .application
                    .player_session()
                    .clip_end_seconds()
                    .is_some())
    }) {
        start_clip_export(window, requested_choice);
        return;
    }
    if state(window).is_some_and(|state| state.view == MainView::DownloadQueue) {
        start_selected_queued_download(window, Some(requested_choice));
        return;
    }
    if state(window).is_none_or(|state| state.view == MainView::MainMenu) {
        return;
    }
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
    start_download_item(window, &item, requested_choice, false);
}

#[allow(clippy::too_many_lines)]
unsafe fn start_clip_export(window: HWND, choice: DownloadChoice) {
    let Some((item, range, settings)) = state(window).and_then(|state| {
        let session = state.application.player_session();
        Some((
            session.current_item()?.clone(),
            session.clip_range(),
            state.application.settings().clone(),
        ))
    }) else {
        return;
    };
    let Some((start, end)) = range else {
        if let Some(state) = state(window) {
            let session = state.application.player_session();
            let key =
                if session.clip_start_seconds().is_some() && session.clip_end_seconds().is_some() {
                    "clip_marker_invalid"
                } else {
                    "clip_markers_missing"
                };
            set_status(state, &catalog_text(&state.application, key), true);
        }
        return;
    };
    let primary = item.local_path.clone().or_else(|| {
        item.stream_url
            .as_ref()
            .or(item.url.as_ref())
            .map(ToString::to_string)
    });
    let Some(primary) = primary else {
        return;
    };
    let configured = PathBuf::from(settings.ffmpeg_location.trim());
    let ffmpeg = if configured.is_file() {
        configured
    } else if configured.is_dir() {
        configured.join("ffmpeg.exe")
    } else {
        application_directory()
            .unwrap_or_default()
            .join("ffmpeg")
            .join("ffmpeg.exe")
    };
    let mut folder_item = item.clone();
    if folder_item.kind == apricot_core::MediaKind::PodcastEpisode {
        folder_item.kind = apricot_core::MediaKind::PodcastFeed;
    }
    let folder = match download_folder_for_item(&settings, &folder_item, false) {
        Ok(folder) => folder.join("clips"),
        Err(error) => {
            show_error_message(window, &error);
            return;
        }
    };
    let extension = if choice == DownloadChoice::Audio {
        normalized_audio_format(&settings.audio_format)
    } else {
        item.local_path
            .as_deref()
            .and_then(|path| std::path::Path::new(path).extension())
            .and_then(|ext| ext.to_str())
            .unwrap_or("mp4")
            .to_owned()
    };
    let stem = format!(
        "{} - {}-{}",
        safe_path_component(&item.title),
        format_duration(start).replace(':', "-"),
        format_duration(end).replace(':', "-")
    );
    let mut output = folder.join(format!("{stem}.{extension}"));
    let mut counter = 2;
    while output.exists() {
        output = folder.join(format!("{stem} ({counter}).{extension}"));
        counter += 1;
    }
    if settings.ask_download_location_each_time {
        let catalog = apricot_app::embedded_catalog(&settings.language);
        let default_name = output.file_name().unwrap_or_default().to_string_lossy();
        match crate::file_dialog_win32::save_download_file(
            window,
            catalog.text("choose_save_path"),
            &folder,
            &default_name,
            &extension,
            &extension.to_ascii_uppercase(),
            catalog.text("all_files"),
        ) {
            Ok(Some(path)) => output = path,
            Ok(None) => return,
            Err(error) => {
                show_error_message(window, &error);
                return;
            }
        }
    }
    let request = apricot_platform::ClipExportRequest {
        ffmpeg,
        primary_input: primary,
        external_audio_input: item.external_audio_url.as_ref().map(ToString::to_string),
        start_seconds: start,
        end_seconds: end,
        output_path: output,
        mode: if choice == DownloadChoice::Audio {
            apricot_platform::ClipExportMode::Audio
        } else {
            apricot_platform::ClipExportMode::Video
        },
        audio_format: settings.audio_format,
        audio_quality: settings.audio_quality,
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result =
            apricot_platform::export_marked_clip(&request).map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
    if let Some(state) = state_mut(window) {
        state.clip_exports.push(receiver);
        set_status(
            state,
            &catalog_text(&state.application, "clip_export_started"),
            true,
        );
    }
    let _ = SetTimer(
        Some(window),
        DOWNLOAD_TIMER_ID,
        DOWNLOAD_TIMER_INTERVAL_MS,
        None,
    );
}

unsafe fn poll_clip_exports(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut completed = Vec::new();
    state
        .clip_exports
        .retain(|receiver| match receiver.try_recv() {
            Ok(result) => {
                completed.push(result);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                completed.push(Err("Clip export worker stopped".to_owned()));
                false
            }
        });
    for result in completed {
        let message = match result {
            Ok(path) => catalog_text(&state.application, "clip_export_done").replace(
                "{title}",
                &path.file_name().unwrap_or_default().to_string_lossy(),
            ),
            Err(error) => {
                catalog_text(&state.application, "clip_export_failed").replace("{error}", &error)
            }
        };
        set_status(state, &message, true);
    }
    if !state.clip_exports.is_empty() {
        let _ = SetTimer(
            Some(window),
            DOWNLOAD_TIMER_ID,
            DOWNLOAD_TIMER_INTERVAL_MS,
            None,
        );
    }
}

struct UserPlaylistDownloadPlan {
    playlist_index: usize,
    title: String,
    items: Vec<apricot_core::MediaItem>,
    settings: apricot_storage::SettingsDocument,
}

unsafe fn selected_user_playlist_download_plan(window: HWND) -> Option<UserPlaylistDownloadPlan> {
    let playlist_index = state(window).and_then(|state| match state.view {
        MainView::UserPlaylists => {
            usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()
        }
        MainView::UserPlaylistItems => Some(state.current_user_playlist_index),
        _ => None,
    })?;
    let state = state(window)?;
    let playlist = state.application.user_playlists().get(playlist_index)?;
    Some(UserPlaylistDownloadPlan {
        playlist_index,
        title: playlist.title.clone(),
        items: playlist
            .items
            .iter()
            .filter(|item| !item.is_local_media() && item.url.is_some())
            .cloned()
            .collect(),
        settings: state.application.settings().clone(),
    })
}

unsafe fn choose_user_playlist_download_folder(
    window: HWND,
    plan: &UserPlaylistDownloadPlan,
) -> std::result::Result<Option<PathBuf>, String> {
    let default_folder = user_playlist_download_folder(&plan.settings, &plan.title)?;
    if !plan.settings.ask_download_location_each_time {
        return Ok(Some(default_folder));
    }
    let catalog = apricot_app::embedded_catalog(&plan.settings.language);
    let initial = default_folder
        .parent()
        .filter(|path| path.is_dir())
        .unwrap_or(&default_folder);
    Ok(crate::folder_dialog_win32::choose_download_folder(
        window,
        catalog.text("choose_save_folder"),
        initial,
    ))
}

unsafe fn build_user_playlist_download_requests(
    window: HWND,
    plan: &UserPlaylistDownloadPlan,
    output_folder: &std::path::Path,
) -> std::result::Result<Vec<DownloadRequest>, String> {
    let mut requests = Vec::with_capacity(plan.items.len());
    for item in &plan.items {
        let Some(mut request) =
            build_download_request(window, item, DownloadChoice::Video, Some(output_folder))?
        else {
            return Ok(Vec::new());
        };
        if matches!(
            item.kind,
            apricot_core::MediaKind::Playlist | apricot_core::MediaKind::Channel
        ) {
            request.output_directory = output_folder.join(safe_path_component(&item.title));
            fs::create_dir_all(&request.output_directory)
                .map_err(|error| format!("Could not create the download folder: {error}"))?;
        }
        requests.push(request);
    }
    Ok(requests)
}

unsafe fn spawn_user_playlist_download(
    window: HWND,
    plan: &UserPlaylistDownloadPlan,
    requests: Vec<DownloadRequest>,
    output_folder: PathBuf,
) -> std::result::Result<(), String> {
    let executable = application_directory()
        .map(|directory| directory.join("components").join("yt-dlp.exe"))
        .ok_or_else(|| "Application path is unavailable.".to_owned())?;
    let (task_id, cancellation, sender) = {
        let state =
            state_mut(window).ok_or_else(|| "Application state is unavailable.".to_owned())?;
        state.current_user_playlist_index = plan.playlist_index;
        let mut batch_item = plan.items[0].clone();
        batch_item.title.clone_from(&plan.title);
        let task_id = state.application.downloads_mut().begin(
            batch_item,
            DownloadChoice::Video,
            DownloadTaskKind::UserPlaylist,
            requests.len(),
        );
        let cancellation = Arc::new(AtomicBool::new(false));
        state
            .download_cancellations
            .insert(task_id, Arc::clone(&cancellation));
        (task_id, cancellation, state.download_sender.clone())
    };
    spawn_batch_download(
        task_id,
        executable,
        requests,
        output_folder,
        cancellation,
        sender,
    );
    let _ = SetTimer(
        Some(window),
        DOWNLOAD_TIMER_ID,
        DOWNLOAD_TIMER_INTERVAL_MS,
        None,
    );
    show_download_progress_window(window, task_id);
    Ok(())
}

unsafe fn download_current_user_playlist(window: HWND) {
    let Some(plan) = selected_user_playlist_download_plan(window) else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "no_playlists"),
                true,
            );
        }
        return;
    };
    if plan.items.is_empty() {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "playlist_empty"),
                true,
            );
        }
        return;
    }
    let output_folder = match choose_user_playlist_download_folder(window, &plan) {
        Ok(Some(folder)) => folder,
        Ok(None) => {
            if let Some(state) = state(window) {
                set_status(
                    state,
                    &catalog_text(&state.application, "download_cancelled"),
                    true,
                );
            }
            return;
        }
        Err(error) => {
            show_error_message(window, &error);
            return;
        }
    };
    let requests = match build_user_playlist_download_requests(window, &plan, &output_folder) {
        Ok(requests) if !requests.is_empty() => requests,
        Ok(_) => return,
        Err(error) => {
            show_error_message(window, &error);
            return;
        }
    };
    let count = requests.len();
    if let Err(error) = spawn_user_playlist_download(window, &plan, requests, output_folder) {
        show_error_message(window, &error);
        return;
    }
    if let Some(state) = state_mut(window) {
        let message = catalog_text(&state.application, "batch_download_start")
            .replace("{count}", &count.to_string());
        set_status(state, &message, true);
        refresh_download_projection(window, state, false);
    }
}

unsafe fn start_download_item(
    window: HWND,
    item: &apricot_core::MediaItem,
    requested_choice: DownloadChoice,
    remove_queued: bool,
) {
    if item.is_local_media() {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "no_selection"),
                true,
            );
        }
        return;
    }
    let choice = if item.kind == apricot_core::MediaKind::PodcastEpisode {
        DownloadChoice::Audio
    } else {
        requested_choice
    };
    let choice = match choice {
        DownloadChoice::Ask => match choose_download_format(window) {
            Some(choice) => choice,
            None => return,
        },
        choice => choice,
    };
    if !confirm_download(window, item, choice) {
        return;
    }
    if remove_queued && let Some(state) = state_mut(window) {
        let _ = state.application.downloads_mut().remove_queued(item);
    }
    let request = match build_download_request(window, item, choice, None) {
        Ok(Some(request)) => request,
        Ok(None) => {
            if let Some(state) = state(window) {
                set_status(
                    state,
                    &catalog_text(&state.application, "download_cancelled"),
                    true,
                );
            }
            return;
        }
        Err(error) => {
            show_error_message(window, &error);
            return;
        }
    };
    let Some(executable) =
        application_directory().map(|directory| directory.join("components").join("yt-dlp.exe"))
    else {
        show_error_message(window, "Application path is unavailable.");
        return;
    };
    let allow_playlist = request.allow_playlist;
    let task_kind = match item.kind {
        apricot_core::MediaKind::Playlist => DownloadTaskKind::Playlist,
        apricot_core::MediaKind::Channel => DownloadTaskKind::Channel,
        apricot_core::MediaKind::PodcastFeed => DownloadTaskKind::PodcastFeed,
        _ => DownloadTaskKind::Single,
    };
    let total = usize::from(!allow_playlist);
    let (task_id, sender, cancellation) = {
        let Some(state) = state_mut(window) else {
            return;
        };
        let task_id =
            state
                .application
                .downloads_mut()
                .begin(item.clone(), choice, task_kind, total);
        let cancellation = Arc::new(AtomicBool::new(false));
        state
            .download_cancellations
            .insert(task_id, Arc::clone(&cancellation));
        (task_id, state.download_sender.clone(), cancellation)
    };
    spawn_download(task_id, executable, request, cancellation, sender);
    let _ = SetTimer(
        Some(window),
        DOWNLOAD_TIMER_ID,
        DOWNLOAD_TIMER_INTERVAL_MS,
        None,
    );
    if let Some(state) = state_mut(window) {
        let key = match task_kind {
            DownloadTaskKind::Playlist => "download_playlist_start",
            DownloadTaskKind::Channel => "download_channel_start",
            DownloadTaskKind::PodcastFeed => "download_feed_start",
            _ if choice == DownloadChoice::Audio => "download_audio_start",
            _ => "download_video_start",
        };
        set_status(
            state,
            &catalog_text(&state.application, "download_started"),
            true,
        );
        set_status(state, &catalog_text(&state.application, key), false);
        refresh_download_projection(window, state, false);
    }
    track_collection_download_progress(window, task_kind, task_id);
}

unsafe fn confirm_download(
    window: HWND,
    item: &apricot_core::MediaItem,
    choice: DownloadChoice,
) -> bool {
    let Some((required, message)) = state(window).map(|state| {
        let required = state.application.settings().confirm_before_download;
        let action = if choice == DownloadChoice::Audio {
            catalog_text(&state.application, "download_audio_mode")
        } else {
            catalog_text(&state.application, "download_video_mode")
        };
        let message = catalog_text(&state.application, "download_confirm")
            .replace("{action}", &action)
            .replace("{title}", &item.title);
        (required, message)
    }) else {
        return false;
    };
    if !required {
        return true;
    }
    if let Some(state) = state_mut(window) {
        state.modal_open = true;
    }
    let message = wide(&message);
    let result = MessageBoxW(
        Some(window),
        PCWSTR(message.as_ptr()),
        w!("ApricotPlayer 2 Beta"),
        MB_YESNO | MB_ICONINFORMATION,
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    if result != IDYES {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "download_cancelled"),
                true,
            );
        }
        return false;
    }
    true
}

unsafe fn build_download_request(
    window: HWND,
    item: &apricot_core::MediaItem,
    choice: DownloadChoice,
    output_override: Option<&std::path::Path>,
) -> std::result::Result<Option<DownloadRequest>, String> {
    let Some((settings, settings_file, language)) = state(window).map(|state| {
        (
            state.application.settings().clone(),
            state.application.settings_file(),
            state.application.settings().language.clone(),
        )
    }) else {
        return Err("Application state is unavailable.".to_owned());
    };
    let url = item
        .url
        .as_ref()
        .map(ToString::to_string)
        .ok_or_else(|| "The selected item has no downloadable URL.".to_owned())?;
    let url = collection_download_url(item, &url);
    let mode = if choice == DownloadChoice::Audio {
        DownloadMode::Audio
    } else {
        DownloadMode::Video
    };
    let allow_playlist = matches!(
        item.kind,
        apricot_core::MediaKind::Playlist
            | apricot_core::MediaKind::Channel
            | apricot_core::MediaKind::PodcastFeed
    );
    let mut output_directory = download_folder_for_item(&settings, item, allow_playlist)?;
    if let Some(output_override) = output_override {
        output_override.clone_into(&mut output_directory);
    }
    fs::create_dir_all(&output_directory)
        .map_err(|error| format!("Could not create the download folder: {error}"))?;
    let mut target_path = None;
    if settings.ask_download_location_each_time && output_override.is_none() {
        let catalog = apricot_app::embedded_catalog(&language);
        if allow_playlist {
            let selected = crate::folder_dialog_win32::choose_download_folder(
                window,
                catalog.text("choose_save_folder"),
                &output_directory,
            );
            let Some(selected) = selected else {
                return Ok(None);
            };
            output_directory = selected;
        } else {
            let extension = if mode == DownloadMode::Audio {
                normalized_audio_format(&settings.audio_format)
            } else {
                "mp4".to_owned()
            };
            let default_file = format!("{}.{}", safe_path_component(&item.title), extension);
            let type_label = extension.to_ascii_uppercase();
            let selected = crate::file_dialog_win32::save_download_file(
                window,
                catalog.text("choose_save_path"),
                &output_directory,
                &default_file,
                &extension,
                &type_label,
                catalog.text("all_files"),
            )?;
            let Some(selected) = selected else {
                return Ok(None);
            };
            output_directory = selected
                .parent()
                .map(std::path::Path::to_path_buf)
                .ok_or_else(|| "The selected download path has no folder.".to_owned())?;
            target_path = Some(selected);
        }
    }
    let mut options = download_options_from_settings(&settings, &settings_file);
    options.audio_format = normalized_audio_format(&settings.audio_format);
    Ok(Some(DownloadRequest {
        url,
        title: item.title.clone(),
        mode,
        output_directory,
        target_path,
        allow_playlist,
        options,
    }))
}

#[allow(clippy::too_many_lines)]
unsafe fn start_all_queued_downloads(window: HWND, requested_choice: DownloadChoice) {
    let Some((queued, ask_location, default_folder, language)) = state(window).map(|state| {
        (
            state.application.downloads().queued().to_vec(),
            state.application.settings().ask_download_location_each_time,
            PathBuf::from(state.application.settings().download_folder.trim()),
            state.application.settings().language.clone(),
        )
    }) else {
        return;
    };
    if queued.is_empty() {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "download_queue_empty"),
                true,
            );
        }
        return;
    }
    let batch_folder = if ask_location {
        let catalog = apricot_app::embedded_catalog(&language);
        let initial = if default_folder.is_absolute() {
            default_folder.clone()
        } else {
            PathBuf::from(r"C:\")
        };
        if let Some(folder) = crate::folder_dialog_win32::choose_download_folder(
            window,
            catalog.text("choose_save_folder"),
            &initial,
        ) {
            Some(folder)
        } else {
            if let Some(state) = state(window) {
                set_status(
                    state,
                    &catalog_text(&state.application, "download_cancelled"),
                    true,
                );
            }
            return;
        }
    } else {
        None
    };
    let mut requests = Vec::with_capacity(queued.len());
    for queued_item in &queued {
        let choice = if queued_item.item.kind == apricot_core::MediaKind::PodcastEpisode {
            DownloadChoice::Audio
        } else {
            requested_choice
        };
        match build_download_request(window, &queued_item.item, choice, batch_folder.as_deref()) {
            Ok(Some(request)) => requests.push(request),
            Ok(None) => return,
            Err(error) => {
                show_error_message(window, &error);
                return;
            }
        }
    }
    let Some(executable) =
        application_directory().map(|directory| directory.join("components").join("yt-dlp.exe"))
    else {
        show_error_message(window, "Application path is unavailable.");
        return;
    };
    let (task_id, cancellation, sender, batch_title) = {
        let Some(state) = state_mut(window) else {
            return;
        };
        let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
        let batch_title = if requested_choice == DownloadChoice::Audio {
            catalog.text("download_all_as_audio").to_owned()
        } else {
            catalog.text("download_all_as_video").to_owned()
        };
        let mut batch_item = queued[0].item.clone();
        batch_item.title.clone_from(&batch_title);
        let _ = state.application.downloads_mut().take_queued();
        let task_id = state.application.downloads_mut().begin(
            batch_item,
            requested_choice,
            DownloadTaskKind::Batch,
            requests.len(),
        );
        let cancellation = Arc::new(AtomicBool::new(false));
        state
            .download_cancellations
            .insert(task_id, Arc::clone(&cancellation));
        (
            task_id,
            cancellation,
            state.download_sender.clone(),
            batch_title,
        )
    };
    let completion_directory = batch_folder.unwrap_or(default_folder);
    spawn_batch_download(
        task_id,
        executable,
        requests,
        completion_directory,
        cancellation,
        sender,
    );
    let _ = SetTimer(
        Some(window),
        DOWNLOAD_TIMER_ID,
        DOWNLOAD_TIMER_INTERVAL_MS,
        None,
    );
    if let Some(state) = state_mut(window) {
        let message = catalog_text(&state.application, "batch_download_start")
            .replace("{count}", &queued.len().to_string());
        set_status(state, &message, true);
        if let Some(task) = state.application.downloads_mut().active_task(task_id) {
            debug_assert_eq!(task.title, batch_title);
        }
        refresh_download_projection(window, state, false);
    }
    if queued.len() > 5
        || queued.iter().any(|queued| {
            matches!(
                queued.item.kind,
                apricot_core::MediaKind::Playlist | apricot_core::MediaKind::Channel
            )
        })
    {
        show_download_progress_window(window, task_id);
    }
}

fn download_options_from_settings(
    settings: &apricot_storage::SettingsDocument,
    settings_file: &std::path::Path,
) -> DownloadOptions {
    let packaged_ffmpeg = application_directory()
        .map(|directory| directory.join("ffmpeg").join("ffmpeg.exe"))
        .filter(|path| path.is_file());
    let configured_ffmpeg = nonempty(&settings.ffmpeg_location)
        .map(PathBuf::from)
        .filter(|path| path.exists());
    let cookies_file = nonempty(&settings.cookies_file)
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    DownloadOptions {
        audio_format: normalized_audio_format(&settings.audio_format),
        audio_quality: settings.audio_quality.trim().to_owned(),
        video_format: VideoDownloadFormat::from_setting(&settings.video_format),
        max_video_height: u32::try_from(settings.max_video_height.clamp(144, 8_640))
            .unwrap_or(1_080),
        quiet: settings.quiet_downloads,
        keep_playlist_order: settings.keep_playlist_order,
        filename_template: settings.filename_template.clone(),
        write_thumbnail: settings.write_thumbnail,
        write_description: settings.write_description,
        write_info_json: settings.write_info_json,
        write_subtitles: settings.write_subtitles,
        write_automatic_subtitles: settings.auto_subtitles,
        subtitle_languages: settings
            .subtitle_languages
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect(),
        embed_metadata: settings.embed_metadata,
        embed_thumbnail: settings.embed_thumbnail,
        restrict_filenames: settings.restrict_filenames,
        concurrent_fragments: u32::try_from(settings.concurrent_fragments.clamp(1, 32))
            .unwrap_or(4),
        retries: u32::try_from(settings.retries.clamp(0, 100)).unwrap_or(10),
        socket_timeout_seconds: u32::try_from(settings.socket_timeout.clamp(1, 300)).unwrap_or(20),
        rate_limit: nonempty(&settings.rate_limit),
        proxy_url: nonempty(&settings.proxy),
        cookies_file,
        ffmpeg_location: configured_ffmpeg.or(packaged_ffmpeg),
        download_archive: settings.download_archive.then(|| {
            settings_file
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("download-archive.txt")
        }),
    }
}

fn download_folder_for_item(
    settings: &apricot_storage::SettingsDocument,
    item: &apricot_core::MediaItem,
    collection: bool,
) -> std::result::Result<PathBuf, String> {
    let root = PathBuf::from(settings.download_folder.trim());
    if !root.is_absolute() {
        return Err("The configured download folder must be an absolute path.".to_owned());
    }
    let root_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut folder = if item.kind == apricot_core::MediaKind::PodcastEpisode
        || item.kind == apricot_core::MediaKind::PodcastFeed
    {
        if root_name == "podcasts" {
            root
        } else if root_name == "music" {
            root.parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("podcasts")
        } else {
            root.join("podcasts")
        }
    } else if root_name == "music" {
        root
    } else {
        root.join("music")
    };
    if item.kind == apricot_core::MediaKind::PodcastEpisode {
        let feed = if item.channel.trim().is_empty() {
            "Unknown podcast"
        } else {
            &item.channel
        };
        folder.push(safe_path_component(feed));
    } else if collection {
        folder.push(safe_path_component(&item.title));
    }
    Ok(folder)
}

fn user_playlist_download_folder(
    settings: &apricot_storage::SettingsDocument,
    title: &str,
) -> std::result::Result<PathBuf, String> {
    let root = PathBuf::from(settings.download_folder.trim());
    if !root.is_absolute() {
        return Err("The configured download folder must be an absolute path.".to_owned());
    }
    let root_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let music = if root_name == "music" {
        root
    } else {
        root.join("music")
    };
    Ok(music.join(safe_path_component(title)))
}

fn normalized_audio_format(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "m4a" | "opus" | "wav" | "flac" => value.trim().to_ascii_lowercase(),
        _ => "mp3".to_owned(),
    }
}

fn collection_download_url(item: &apricot_core::MediaItem, url: &str) -> String {
    if item.kind != apricot_core::MediaKind::Channel {
        return url.to_owned();
    }
    let mut url = url.trim_end_matches('/').to_owned();
    if !url.ends_with("/videos") {
        url.push_str("/videos");
    }
    url
}

fn safe_path_component(value: &str) -> String {
    let result = value
        .trim()
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) || character.is_control()
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let mut result = result
        .chars()
        .take(120)
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_owned();
    if result.is_empty() {
        result.push_str("download");
    }
    let reserved_stem = result
        .split_once('.')
        .map_or(result.as_str(), |(stem, _)| stem)
        .to_ascii_uppercase();
    let reserved = matches!(reserved_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || reserved_stem
            .strip_prefix("COM")
            .or_else(|| reserved_stem.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'));
    if reserved {
        result.insert(0, '_');
    }
    result
}

struct DownloadProgressPresentation {
    percent: u32,
    message: String,
}

unsafe fn show_download_progress_details(window: HWND) -> LRESULT {
    restore_from_tray(window);
    show_download_queue(window);
    LRESULT(0)
}

unsafe fn track_collection_download_progress(
    window: HWND,
    task_kind: DownloadTaskKind,
    task_id: u64,
) {
    if matches!(
        task_kind,
        DownloadTaskKind::Playlist | DownloadTaskKind::Channel | DownloadTaskKind::UserPlaylist
    ) {
        show_download_progress_window(window, task_id);
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn rounded_percent(percent: f64) -> u32 {
    percent.round().clamp(0.0, 100.0) as u32
}

fn download_progress_presentation(
    task: &ActiveDownload,
    catalog: &apricot_core::TranslationCatalog,
) -> DownloadProgressPresentation {
    let total = task.total;
    let completed = task.completed.min(total);
    let remaining = total.saturating_sub(completed);
    let percent = completed
        .saturating_mul(100)
        .saturating_add(total / 2)
        .checked_div(total)
        .map_or_else(
            || rounded_percent(task.percent.unwrap_or_default()),
            |rounded| u32::try_from(rounded.min(100)).unwrap_or(100),
        );
    let title = if task.current_title.trim().is_empty() {
        &task.title
    } else {
        &task.current_title
    };
    let message = catalog
        .text("download_progress_message")
        .replace("{title}", title)
        .replace("{completed}", &completed.to_string())
        .replace("{total}", &total.to_string())
        .replace("{remaining}", &remaining.to_string());
    DownloadProgressPresentation { percent, message }
}

unsafe fn show_download_progress_window(window: HWND, task_id: u64) {
    let Some((language, task, existing)) = state(window).and_then(|state| {
        Some((
            state.application.settings().language.clone(),
            state.application.downloads().active_task(task_id)?.clone(),
            state.download_progress_window,
        ))
    }) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&language);
    let presentation = download_progress_presentation(&task, &catalog);
    if let Some(state) = state_mut(window) {
        state.download_progress_task_ids.insert(task_id);
        state.download_progress_task_id = Some(task_id);
    }
    if let Some(progress_window) = existing.filter(|progress_window| progress_window.is_open()) {
        progress_window.set_progress(presentation.percent, &presentation.message);
        progress_window.show();
        return;
    }
    let created = DownloadProgressWindow::create(
        window,
        catalog.text("download_progress_title"),
        &presentation.message,
        catalog.text("download_progress_hide"),
        catalog.text("download_progress_details"),
        WM_SHOW_DOWNLOAD_DETAILS,
    );
    match created {
        Ok(progress_window) => {
            progress_window.set_progress(presentation.percent, &presentation.message);
            if let Some(state) = state_mut(window) {
                state.download_progress_window = Some(progress_window);
            } else {
                progress_window.destroy();
            }
        }
        Err(error) => {
            if let Some(state) = state_mut(window) {
                state.download_progress_task_ids.remove(&task_id);
                state.download_progress_task_id = None;
                set_status(
                    state,
                    &format!("Download progress window could not open: {error}"),
                    true,
                );
            }
        }
    }
}

unsafe fn update_download_progress_window(state: &WindowState) {
    let Some(task_id) = state.download_progress_task_id else {
        return;
    };
    let Some(progress_window) = state
        .download_progress_window
        .filter(|progress_window| progress_window.is_open())
    else {
        return;
    };
    let Some(task) = state.application.downloads().active_task(task_id) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let presentation = download_progress_presentation(task, &catalog);
    progress_window.set_progress(presentation.percent, &presentation.message);
}

unsafe fn close_download_progress_window(state: &mut WindowState, task_id: u64) {
    state.download_progress_task_ids.remove(&task_id);
    if state.download_progress_task_id != Some(task_id) {
        return;
    }
    let next_task_id = state
        .application
        .downloads()
        .active()
        .iter()
        .map(|task| task.id)
        .find(|candidate| state.download_progress_task_ids.contains(candidate));
    if let Some(next_task_id) = next_task_id {
        state.download_progress_task_id = Some(next_task_id);
        update_download_progress_window(state);
        return;
    }
    state.download_progress_task_id = None;
    state.download_progress_task_ids.clear();
    if let Some(progress_window) = state.download_progress_window.take() {
        progress_window.destroy();
    }
}

unsafe fn poll_download_updates(window: HWND) {
    poll_clip_exports(window);
    loop {
        let update = state(window).map(|state| state.download_receiver.try_recv());
        match update {
            Some(Ok(update)) => apply_download_update(window, update),
            Some(Err(TryRecvError::Empty)) | None => break,
            Some(Err(TryRecvError::Disconnected)) => {
                let _ = KillTimer(Some(window), DOWNLOAD_TIMER_ID);
                break;
            }
        }
    }
    if state(window).is_none_or(|state| {
        state.application.downloads().active().is_empty() && state.clip_exports.is_empty()
    }) {
        let _ = KillTimer(Some(window), DOWNLOAD_TIMER_ID);
    }
}

/// The last progress a download reported to the UI.
#[derive(Clone, Debug, PartialEq)]
struct DownloadProgressReport {
    percent_bucket: Option<i64>,
    title: String,
    at: std::time::Instant,
}

impl DownloadProgressReport {
    fn new(percent: Option<f64>, title: &str) -> Self {
        Self {
            percent_bucket: download_percent_bucket(percent),
            title: title.to_owned(),
            at: std::time::Instant::now(),
        }
    }
}

/// Python's progress hook reports the whole percent, clamped to 0..100.
#[allow(clippy::cast_possible_truncation)]
fn download_percent_bucket(percent: Option<f64>) -> Option<i64> {
    percent
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 100.0) as i64)
}

/// Python's `make_download_progress_hook`: report when the whole percent or
/// the title changes, otherwise at most every 0.75 s.
fn should_report_download_progress(
    last: Option<&mut DownloadProgressReport>,
    percent: Option<f64>,
    title: &str,
    now: std::time::Instant,
) -> bool {
    let Some(last) = last else {
        return true;
    };
    let bucket = download_percent_bucket(percent);
    let report = bucket != last.percent_bucket
        || title != last.title
        || now.saturating_duration_since(last.at) >= std::time::Duration::from_millis(750);
    if report {
        last.percent_bucket = bucket;
        title.clone_into(&mut last.title);
        last.at = now;
    }
    report
}

fn accept_download_progress(
    state: &mut WindowState,
    task_id: u64,
    phase: DownloadPhase,
    percent: Option<f64>,
    title: &str,
) -> bool {
    let now = std::time::Instant::now();
    let last = state.download_progress_reports.get_mut(&task_id);
    if last.is_none() {
        state
            .download_progress_reports
            .insert(task_id, DownloadProgressReport::new(percent, title));
        return true;
    }
    phase == DownloadPhase::Processing || should_report_download_progress(last, percent, title, now)
}

#[allow(clippy::too_many_lines)]
unsafe fn apply_download_update(window: HWND, update: DownloadWorkerUpdate) {
    match update {
        DownloadWorkerUpdate::Event { task_id, event } => {
            let Some(state) = state_mut(window) else {
                return;
            };
            match event {
                DownloadEvent::Progress {
                    phase,
                    title,
                    percent,
                    playlist_index,
                    playlist_count,
                } => {
                    if !accept_download_progress(state, task_id, phase, percent, &title) {
                        return;
                    }
                    let status = if phase == DownloadPhase::Processing {
                        DownloadTaskStatus::Processing
                    } else {
                        DownloadTaskStatus::Downloading
                    };
                    let _ = state.application.downloads_mut().update_progress(
                        task_id,
                        status,
                        &title,
                        percent,
                        playlist_index.and_then(|value| usize::try_from(value).ok()),
                        playlist_count.and_then(|value| usize::try_from(value).ok()),
                    );
                }
                DownloadEvent::ItemFailed { message } => {
                    let _ = state
                        .application
                        .downloads_mut()
                        .add_item_failure(task_id, &message);
                }
                DownloadEvent::FileFinished { title, .. } => {
                    if let Some(task) = state.application.downloads().active_task(task_id) {
                        let should_increment = !matches!(
                            task.kind,
                            DownloadTaskKind::Playlist | DownloadTaskKind::Channel
                        ) || task.total == 0;
                        let completed =
                            task.completed.saturating_add(usize::from(should_increment));
                        let total = task.total.max(completed);
                        let _ = state
                            .application
                            .downloads_mut()
                            .set_completed(task_id, completed, total);
                    }
                    let _ = state.application.downloads_mut().update_progress(
                        task_id,
                        DownloadTaskStatus::Processing,
                        &title,
                        Some(100.0),
                        None,
                        None,
                    );
                }
            }
            refresh_download_projection(window, state, false);
            update_download_progress_window(state);
        }
        DownloadWorkerUpdate::Finished {
            task_id,
            result,
            output_directory,
        } => {
            let Some(state) = state_mut(window) else {
                return;
            };
            state.download_cancellations.remove(&task_id);
            state.download_progress_reports.remove(&task_id);
            let Some(task) = state.application.downloads_mut().finish(task_id) else {
                return;
            };
            close_download_progress_window(state, task_id);
            let cancelled = result
                .as_ref()
                .err()
                .is_some_and(|error| error.to_ascii_lowercase().contains("cancelled"));
            let succeeded = result.is_ok();
            let message = match result {
                Ok(summary) if !summary.item_failures.is_empty() => {
                    catalog_text(&state.application, "batch_download_done_with_errors")
                        .replace("{failed}", &summary.item_failures.len().to_string())
                }
                Ok(_) => {
                    let key = match task.kind {
                        DownloadTaskKind::Batch => "batch_download_done",
                        DownloadTaskKind::PodcastFeed => "download_feed_done",
                        DownloadTaskKind::Playlist | DownloadTaskKind::UserPlaylist => {
                            "download_playlist_done"
                        }
                        DownloadTaskKind::Channel => "download_channel_done",
                        DownloadTaskKind::Single if task.choice == DownloadChoice::Audio => {
                            "download_audio_done"
                        }
                        DownloadTaskKind::Single => "download_video_done",
                    };
                    catalog_text(&state.application, key).replace("{title}", &task.title)
                }
                Err(_) if cancelled => catalog_text(&state.application, "download_cancelled"),
                Err(error) => {
                    catalog_text(&state.application, "download_failed").replace("{error}", &error)
                }
            };
            set_status(state, &message, true);
            if succeeded {
                let history_action = match task.kind {
                    DownloadTaskKind::Single if task.choice == DownloadChoice::Audio => {
                        Some("downloaded audio")
                    }
                    DownloadTaskKind::Single => Some("downloaded video"),
                    DownloadTaskKind::Playlist => Some("downloaded playlist"),
                    DownloadTaskKind::Channel => Some("downloaded channel"),
                    DownloadTaskKind::PodcastFeed
                    | DownloadTaskKind::UserPlaylist
                    | DownloadTaskKind::Batch => None,
                };
                if let Some(action) = history_action
                    && let Err(error) = state.application.record_history(
                        task.item.clone(),
                        action,
                        unix_timestamp(),
                    )
                {
                    set_status(
                        state,
                        &format!("Download history was not saved: {error}"),
                        true,
                    );
                }
            }
            let foreground = GetForegroundWindow();
            let app_has_focus = foreground == window
                || IsChild(window, foreground).as_bool()
                || GetAncestor(foreground, GA_ROOTOWNER) == window;
            let popup = app_has_focus
                && state.application.settings().popup_when_download_complete
                && !cancelled
                && succeeded;
            let open_folder = state.application.settings().open_folder_after_download
                && succeeded
                && output_directory.is_dir();
            let notify = !app_has_focus
                && state.application.settings().download_notifications
                && state.application.settings().windows_notifications
                && succeeded;
            refresh_download_projection(window, state, true);
            if notify {
                let title = catalog_text(&state.application, "notification_download_title");
                show_tray_notification(window, &title, &message);
            }
            if popup {
                show_error_message(window, &message);
            }
            if open_folder
                && let Err(error) = std::process::Command::new("explorer.exe")
                    .arg(&output_directory)
                    .spawn()
            {
                set_status(
                    state,
                    &format!("Could not open the download folder: {error}"),
                    true,
                );
            }
        }
    }
}

unsafe fn refresh_download_projection(
    window: HWND,
    state: &mut WindowState,
    update_main_menu: bool,
) {
    if state.view == MainView::DownloadQueue {
        refresh_download_queue(state, false, false);
        layout_controls_state(window, state);
    } else if update_main_menu && state.view == MainView::MainMenu {
        refresh_main_menu(state, MainMenuSelection::Preserve);
        layout_controls_state(window, state);
    }
}

unsafe fn cancel_selected_download(window: HWND) {
    let task_id = state(window).and_then(|state| {
        let selected =
            usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
        state
            .application
            .downloads()
            .active()
            .get(selected)
            .map(|task| task.id)
    });
    let Some(task_id) = task_id else {
        if let Some(state) = state(window) {
            set_status(
                state,
                &catalog_text(&state.application, "no_active_download"),
                true,
            );
        }
        return;
    };
    cancel_download_task(window, task_id);
}

unsafe fn remove_selected_queued_download(window: HWND) {
    let Some(item) = state(window).and_then(|state| {
        let selected =
            usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
        let queued_index = selected.checked_sub(state.application.downloads().active().len())?;
        state
            .application
            .downloads()
            .queued()
            .get(queued_index)
            .map(|queued| queued.item.clone())
    }) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let _ = state.application.downloads_mut().remove_queued(&item);
    let message =
        catalog_text(&state.application, "download_deselected").replace("{title}", &item.title);
    set_status(state, &message, true);
    refresh_download_projection(window, state, false);
}

unsafe fn cancel_download_task(window: HWND, task_id: u64) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let title = state
        .application
        .downloads()
        .active_task(task_id)
        .map(|task| task.title.clone());
    let Some(title) = title else {
        return;
    };
    if let Some(cancellation) = state.download_cancellations.get(&task_id) {
        cancellation.store(true, AtomicOrdering::Release);
    }
    let _ = state.application.downloads_mut().request_cancel(task_id);
    let message =
        catalog_text(&state.application, "download_cancel_requested").replace("{title}", &title);
    set_status(state, &message, true);
    refresh_download_projection(window, state, false);
}

unsafe fn cancel_all_downloads(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.application.downloads().active().is_empty() {
        set_status(
            state,
            &catalog_text(&state.application, "no_active_download"),
            true,
        );
        return;
    }
    for cancellation in state.download_cancellations.values() {
        cancellation.store(true, AtomicOrdering::Release);
    }
    let _ = state.application.downloads_mut().request_cancel_all();
    set_status(
        state,
        &catalog_text(&state.application, "all_downloads_cancel_requested"),
        true,
    );
    refresh_download_projection(window, state, false);
}

unsafe fn toggle_active_download_queue(window: HWND) {
    let Some(item) = active_media_item(window) else {
        return;
    };
    let choice = if item.kind == apricot_core::MediaKind::PodcastEpisode {
        DownloadChoice::Audio
    } else {
        DownloadChoice::Ask
    };
    toggle_download_queue_item(window, &item, choice);
}

unsafe fn toggle_download_queue_item(
    window: HWND,
    item: &apricot_core::MediaItem,
    choice: DownloadChoice,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let outcome = state
        .application
        .downloads_mut()
        .queue_item(item.clone(), choice);
    let collection = matches!(
        item.kind,
        apricot_core::MediaKind::Playlist | apricot_core::MediaKind::Channel
    );
    let key = match outcome {
        apricot_app::QueueToggleOutcome::Deselected => "download_deselected",
        apricot_app::QueueToggleOutcome::Rejected => "no_selection",
        _ if item.kind == apricot_core::MediaKind::PodcastEpisode => {
            "podcast_episode_audio_selected_download"
        }
        _ => match choice {
            DownloadChoice::Ask => "selected_for_download_or_playlist",
            DownloadChoice::Audio if collection => "collection_audio_selected_download",
            DownloadChoice::Video if collection => "collection_video_selected_download",
            DownloadChoice::Audio => "audio_selected_download",
            DownloadChoice::Video => "video_selected_download",
        },
    };
    let message = catalog_text(&state.application, key).replace("{title}", &item.title);
    set_status(state, &message, true);
    refresh_queued_row(state);
    refresh_download_projection(window, state, false);
}

/// Shows or hides the queue marker on the selected row. Python defers the
/// result row while the list has focus on it, so the marker appears once the
/// selection moves, and rebuilds the episode list at once.
unsafe fn refresh_queued_row(state: &mut WindowState) {
    let Ok(index) = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0) else {
        return;
    };
    match state.view {
        MainView::Results | MainView::Trending | MainView::YoutubeCollection => {
            if GetFocus() == state.list {
                state.deferred_youtube_metadata_rows.insert(index);
            } else {
                refresh_youtube_result_line(state, index);
            }
        }
        MainView::RssItems => refresh_rss_episode_line(state, index),
        _ => {}
    }
}

unsafe fn start_youtube_work(window: HWND, work: SearchWork) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let backend = YOUTUBE_BACKEND;
    let generation = work.generation;
    let query = work.query.clone();
    let kind = work.kind;
    let limit = work.limit;
    state.pending_youtube_work = Some(PendingYoutubeListWork::Search(work));
    let Some(components) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("components")))
    else {
        finish_youtube_error_state(window, state, generation, "Application path is unavailable");
        return;
    };
    let config = youtube_session_config(state);
    match state
        .youtube_search
        .start(backend, &components, config, generation, query, kind, limit)
    {
        Ok(()) => {
            let _ = SetTimer(
                Some(window),
                YOUTUBE_TIMER_ID,
                YOUTUBE_TIMER_INTERVAL_MS,
                None,
            );
        }
        Err(error) => {
            finish_youtube_error_state(window, state, generation, &error.to_string());
        }
    }
}

unsafe fn start_youtube_trending_work(window: HWND, work: YoutubeTrendingWork) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let api_key = state
        .application
        .settings()
        .youtube_data_api_key
        .trim()
        .to_owned();
    let generation = work.generation;
    let country_code = work.country_code;
    let category_code = work.category_code;
    let limit = work.limit;
    state.pending_youtube_work = Some(PendingYoutubeListWork::Trending(PendingYoutubeTrending {
        work,
        api_error: None,
    }));
    if api_key.is_empty() {
        start_public_trending_work(window);
        return;
    }
    let proxy = nonempty(&state.application.settings().proxy);
    let region = (country_code != "global").then_some(country_code);
    let category = youtube_trending_category_id(category_code);
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = YoutubeDataApiClient::new(proxy.as_deref())
            .and_then(|client| client.fetch_trending(&api_key, region, category, limit))
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
    state.pending_youtube_trending_api = Some(PendingYoutubeTrendingApi {
        generation,
        receiver,
    });
    let _ = SetTimer(
        Some(window),
        YOUTUBE_TIMER_ID,
        YOUTUBE_TIMER_INTERVAL_MS,
        None,
    );
}

unsafe fn start_public_trending_work(window: HWND) {
    let pending = state(window).and_then(|state| match state.pending_youtube_work.as_ref() {
        Some(PendingYoutubeListWork::Trending(pending)) => Some(pending.clone()),
        _ => None,
    });
    let Some(pending) = pending else {
        return;
    };
    let Some(url) =
        youtube_trending_public_url(pending.work.country_code, pending.work.category_code)
    else {
        let mut message = pending.api_error.unwrap_or_default();
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        if let Some(state) = state(window) {
            message.push_str(&catalog_text(
                &state.application,
                "trending_api_key_required",
            ));
        }
        finish_youtube_trending_error(window, pending.work.generation, &message);
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(components) = application_directory().map(|path| path.join("components")) else {
        finish_youtube_trending_error(
            window,
            pending.work.generation,
            "Application path is unavailable",
        );
        return;
    };
    let config = youtube_session_config(state);
    let start_result = state.youtube_search.start_collection(
        YoutubeBackend::YtDlp,
        &components,
        config,
        pending.work.generation,
        url,
        YoutubeCollectionKind::PlaylistVideos,
        pending.work.limit,
    );
    match start_result {
        Ok(()) => {
            let _ = SetTimer(
                Some(window),
                YOUTUBE_TIMER_ID,
                YOUTUBE_TIMER_INTERVAL_MS,
                None,
            );
        }
        Err(error) => {
            let mut message = pending.api_error.unwrap_or_default();
            if !message.is_empty() {
                message.push_str("\n\n");
            }
            message.push_str(&error.to_string());
            finish_youtube_trending_error(window, pending.work.generation, &message);
        }
    }
}

unsafe fn start_youtube_collection_work(window: HWND, work: YoutubeCollectionWork) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let backend = collection_backend(YOUTUBE_BACKEND, work.kind);
    let Some(components) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("components")))
    else {
        report_youtube_collection_start_error(
            window,
            state,
            &work,
            "Application path is unavailable",
        );
        return;
    };
    let config = youtube_session_config(state);
    match state.youtube_search.start_collection(
        backend,
        &components,
        config,
        work.generation,
        work.url.clone(),
        work.kind,
        work.limit,
    ) {
        Ok(()) => {
            state.pending_youtube_work = Some(PendingYoutubeListWork::Collection(work));
            let route = if state
                .application
                .youtube_collection()
                .is_some_and(|collection| {
                    collection.kind() == YoutubeCollectionKind::PlaylistVideos
                }) {
                Route::PlaylistResults
            } else {
                Route::ChannelResults
            };
            state.application.navigate_to(RouteFrame::new(route));
            state.view = MainView::YoutubeCollection;
            refresh_youtube_collection(state, true);
            layout_controls_state(window, state);
            let _ = SetTimer(
                Some(window),
                YOUTUBE_TIMER_ID,
                YOUTUBE_TIMER_INTERVAL_MS,
                None,
            );
        }
        Err(error) => {
            report_youtube_collection_start_error(window, state, &work, &error.to_string());
        }
    }
}

unsafe fn report_youtube_collection_start_error(
    window: HWND,
    state: &mut WindowState,
    work: &YoutubeCollectionWork,
    message: &str,
) {
    let _ = state
        .application
        .fail_youtube_collection(work.generation, message);
    let _ = state.application.pop_youtube_collection();
    set_status(state, message, true);
    show_error_message(window, message);
    let _ = SetFocus(Some(active_primary_control(state)));
}

/// Python always uses yt-dlp. The Rusty YTDL helper path stays in the code but
/// is no longer selectable in Settings.
const YOUTUBE_BACKEND: YoutubeBackend = YoutubeBackend::YtDlp;

fn collection_backend(selected: YoutubeBackend, kind: YoutubeCollectionKind) -> YoutubeBackend {
    if selected == YoutubeBackend::RustyYtdl && kind != YoutubeCollectionKind::PlaylistVideos {
        YoutubeBackend::YtDlp
    } else {
        selected
    }
}

unsafe fn poll_youtube_runtime(window: HWND) {
    if state(window).is_some_and(|state| state.modal_open) {
        return;
    }
    poll_youtube_trending_api(window);
    poll_podcast_work(window);
    loop {
        let update = {
            let Some(state) = state_mut(window) else {
                return;
            };
            state.youtube_search.poll()
        };
        let update = match update {
            Ok(Some(update)) => update,
            Ok(None) => break,
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
                                    .map(PendingYoutubeListWork::token)
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
            } => finish_youtube_list(window, generation, items, continuation),
            YoutubeSearchServiceUpdate::Resolved {
                token,
                item,
                formats,
            } => finish_youtube_resolve(window, token, *item, &formats),
            YoutubeSearchServiceUpdate::Hydrated { .. } => {}
            YoutubeSearchServiceUpdate::Failed {
                token: generation,
                message,
            } => finish_youtube_error(window, generation, &message),
        }
    }
    poll_youtube_metadata_runtime(window);
    poll_subscription_runtime(window);
}

unsafe fn poll_youtube_trending_api(window: HWND) {
    let outcome = {
        let Some(state) = state(window) else {
            return;
        };
        let Some(pending) = state.pending_youtube_trending_api.as_ref() else {
            return;
        };
        match pending.receiver.try_recv() {
            Ok(result) => Some((pending.generation, result)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some((
                pending.generation,
                Err("YouTube Data API worker stopped unexpectedly".to_owned()),
            )),
        }
    };
    let Some((generation, outcome)) = outcome else {
        return;
    };
    let active = state_mut(window)
        .and_then(|state| state.pending_youtube_trending_api.take())
        .is_some_and(|pending| pending.generation == generation);
    if !active
        || state(window).is_none_or(|state| {
            !matches!(
                state.pending_youtube_work.as_ref(),
                Some(PendingYoutubeListWork::Trending(pending))
                    if pending.work.generation == generation
            )
        })
    {
        stop_youtube_timer(window);
        return;
    }
    match outcome {
        Ok(items) if !items.is_empty() => {
            finish_youtube_trending(window, generation, items, true);
        }
        Ok(_) => {
            if let Some(state) = state_mut(window)
                && let Some(PendingYoutubeListWork::Trending(pending)) =
                    state.pending_youtube_work.as_mut()
            {
                pending.api_error =
                    Some("Official YouTube trending returned no videos.".to_owned());
            }
            start_public_trending_work(window);
        }
        Err(error) => {
            if let Some(state) = state_mut(window)
                && let Some(PendingYoutubeListWork::Trending(pending)) =
                    state.pending_youtube_work.as_mut()
            {
                pending.api_error = Some(error);
            }
            start_public_trending_work(window);
        }
    }
}

unsafe fn poll_youtube_metadata_runtime(window: HWND) {
    poll_youtube_api_metadata(window);
    loop {
        let update = {
            let Some(state) = state_mut(window) else {
                return;
            };
            state.youtube_metadata.poll()
        };
        let update = match update {
            Ok(Some(update)) => update,
            Ok(None) => return,
            Err(_) => {
                if let Some(state) = state_mut(window) {
                    state.pending_youtube_metadata = None;
                }
                stop_youtube_timer(window);
                start_result_metadata_hydration(window);
                return;
            }
        };
        match update {
            YoutubeSearchServiceUpdate::Hydrated { token, items } => {
                finish_youtube_metadata(window, token, &items);
            }
            YoutubeSearchServiceUpdate::Failed { token, .. } => {
                finish_youtube_metadata(window, token, &[]);
            }
            YoutubeSearchServiceUpdate::Results { .. }
            | YoutubeSearchServiceUpdate::Resolved { .. } => {}
        }
    }
}

unsafe fn poll_youtube_api_metadata(window: HWND) {
    let outcome = {
        let Some(state) = state(window) else {
            return;
        };
        let Some(pending) = state.pending_youtube_api_metadata.as_ref() else {
            return;
        };
        match pending.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(
                "YouTube Data API worker stopped unexpectedly".to_owned(),
            )),
        }
    };
    let Some(outcome) = outcome else {
        return;
    };
    let Some(pending) =
        state_mut(window).and_then(|state| state.pending_youtube_api_metadata.take())
    else {
        return;
    };
    if let Ok(items) = outcome {
        if items.len() != pending.urls.len()
            && let Some(state) = state_mut(window)
        {
            state
                .youtube_api_metadata_disabled_scopes
                .insert(pending.scope);
            for url in &pending.urls {
                state.hydrated_youtube_urls.remove(url);
            }
        }
        apply_youtube_metadata_results(window, pending.scope, &items);
    } else {
        let Some(state) = state_mut(window) else {
            return;
        };
        state
            .youtube_api_metadata_disabled_scopes
            .insert(pending.scope);
        for url in pending.urls {
            state.hydrated_youtube_urls.remove(&url);
        }
        stop_youtube_timer(window);
        start_result_metadata_hydration(window);
    }
}

#[allow(clippy::too_many_lines)]
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
            PlaybackEvent::PreviewFinished => {
                if state
                    .clip_preview
                    .as_ref()
                    .is_some_and(|preview| preview.generation == update.generation)
                {
                    state.clip_preview = None;
                    set_status(
                        state,
                        &catalog_text(&state.application, "clip_preview_finished"),
                        true,
                    );
                }
                refresh_player(window, state, false, true);
            }
            PlaybackEvent::Ended => {
                if state.clip_preview.take().is_some() {
                    let _ = execute_player_command(window, PlaybackCommand::SetPaused(true));
                    set_status(
                        state,
                        &catalog_text(&state.application, "clip_preview_finished"),
                        true,
                    );
                    continue;
                }
                mark_current_podcast_episode_played(state);
                let autoplay_next = state
                    .application
                    .player_session()
                    .enabled_toggles()
                    .contains(&SessionToggle::AutoplayNext);
                if autoplay_next {
                    // Python `handle_player_eof`: with "autoplay related" a
                    // related YouTube video plays instead of the next item.
                    let related = state.application.settings().autoplay_related
                        && state
                            .application
                            .player_session()
                            .current_item()
                            .is_some_and(|item| item.youtube_video_id().is_some());
                    if related {
                        play_related_video(window, false);
                    } else {
                        navigate_player_relative_after_end(window);
                    }
                    return;
                }
                set_status(
                    state,
                    &catalog_text(&state.application, "playback_finished"),
                    state.application.settings().announce_playback_finished,
                );
            }
            PlaybackEvent::Failed(error) => {
                state.clip_preview = None;
                state.pending_queued_start = None;
                let message = player_failed_message(&state.application, &error);
                set_status(state, &message, true);
                show_error_message(window, &message);
            }
            PlaybackEvent::CommandFailed(_) => {
                let message = catalog_text(&state.application, "timing_unavailable");
                set_status(state, &message, true);
            }
        }
    }
}

unsafe fn finish_youtube_list(
    window: HWND,
    generation: u64,
    items: Vec<apricot_core::MediaItem>,
    continuation: Option<String>,
) {
    let pending = state(window).and_then(|state| {
        state
            .pending_youtube_work
            .as_ref()
            .filter(|work| work.token() == generation)
            .cloned()
    });
    match pending {
        Some(PendingYoutubeListWork::Search(_)) => {
            finish_youtube_search(window, generation, items, continuation);
        }
        Some(PendingYoutubeListWork::Trending(_)) => {
            finish_youtube_trending(window, generation, items, false);
        }
        Some(PendingYoutubeListWork::Collection(_)) => {
            finish_youtube_collection(window, generation, items);
        }
        Some(PendingYoutubeListWork::PlaylistPlayback(_)) => {
            finish_youtube_playlist_playback(window, generation, items);
        }
        None => {}
    }
}

unsafe fn finish_youtube_trending(
    window: HWND,
    generation: u64,
    items: Vec<apricot_core::MediaItem>,
    used_api: bool,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let active = matches!(
        state.pending_youtube_work.as_ref(),
        Some(PendingYoutubeListWork::Trending(pending))
            if pending.work.generation == generation
    );
    if !active {
        return;
    }
    if !used_api && items.is_empty() {
        let mut message = state
            .pending_youtube_work
            .as_ref()
            .and_then(|pending| match pending {
                PendingYoutubeListWork::Trending(pending) => pending.api_error.clone(),
                _ => None,
            })
            .unwrap_or_default();
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str("The public YouTube chart returned no videos.");
        finish_youtube_trending_error(window, generation, &message);
        return;
    }
    let outcome = state
        .application
        .apply_search_results(generation, items, None);
    state.pending_youtube_work = None;
    state.pending_youtube_trending_api = None;
    let _ = EnableWindow(state.load_trending, true);
    stop_youtube_timer(window);
    if outcome == SearchApplyOutcome::Replaced {
        state.view = MainView::Trending;
        refresh_results(state, true);
        let source_key = if used_api {
            "trending_source_api"
        } else {
            "trending_source_public"
        };
        set_status(state, &catalog_text(&state.application, source_key), false);
        layout_controls_state(window, state);
        start_result_metadata_hydration(window);
    }
}

unsafe fn finish_youtube_playlist_playback(
    window: HWND,
    token: u64,
    items: Vec<apricot_core::MediaItem>,
) {
    let item = {
        let Some(state) = state_mut(window) else {
            return;
        };
        let shuffle = state
            .pending_youtube_work
            .as_ref()
            .and_then(|work| match work {
                PendingYoutubeListWork::PlaylistPlayback(work) if work.token == token => {
                    Some(work.shuffle)
                }
                _ => None,
            });
        let Some(shuffle) = shuffle else {
            return;
        };
        state.pending_youtube_work = None;
        stop_youtube_timer(window);
        state
            .application
            .prepare_youtube_playlist_playback(token, items, shuffle)
    };
    if let Some(item) = item {
        start_sequence_media_item(window, item, None);
    } else if let Some(state) = state(window) {
        set_status(
            state,
            &catalog_text(&state.application, "playlist_no_videos"),
            true,
        );
        let _ = SetFocus(Some(state.list));
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
        .and_then(|work| match work {
            PendingYoutubeListWork::Search(work) => Some(work.work_kind),
            PendingYoutubeListWork::Trending(_)
            | PendingYoutubeListWork::Collection(_)
            | PendingYoutubeListWork::PlaylistPlayback(_) => None,
        });
    let outcome = state
        .application
        .apply_search_results(generation, items, continuation);
    state.pending_youtube_work = None;
    stop_youtube_timer(window);
    let _ = EnableWindow(state.search, true);
    match (work_kind, outcome) {
        (Some(SearchWorkKind::Initial), SearchApplyOutcome::Replaced) => {
            state.hydrated_youtube_urls.clear();
            state.youtube_api_metadata_disabled_scopes.clear();
            state.deferred_youtube_metadata_rows.clear();
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
    start_result_metadata_hydration(window);
}

unsafe fn finish_youtube_collection(
    window: HWND,
    generation: u64,
    items: Vec<apricot_core::MediaItem>,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let work = state
        .pending_youtube_work
        .as_ref()
        .and_then(|work| match work {
            PendingYoutubeListWork::Collection(work) => Some(work.clone()),
            PendingYoutubeListWork::Search(_)
            | PendingYoutubeListWork::Trending(_)
            | PendingYoutubeListWork::PlaylistPlayback(_) => None,
        });
    let outcome = state
        .application
        .apply_youtube_collection_results(generation, items);
    state.pending_youtube_work = None;
    stop_youtube_timer(window);
    match (work, outcome) {
        (
            Some(YoutubeCollectionWork {
                work_kind: YoutubeCollectionWorkKind::Initial,
                ..
            }),
            YoutubeCollectionApplyOutcome::Replaced,
        ) => {
            state.hydrated_youtube_urls.clear();
            state.youtube_api_metadata_disabled_scopes.clear();
            state.deferred_youtube_metadata_rows.clear();
            state.view = MainView::YoutubeCollection;
            refresh_youtube_collection(state, true);
            layout_controls_state(window, state);
        }
        (
            Some(YoutubeCollectionWork {
                work_kind: YoutubeCollectionWorkKind::More,
                ..
            }),
            YoutubeCollectionApplyOutcome::Appended { added },
        ) => append_youtube_collection_results(state, added),
        _ => {}
    }
    let continue_navigation = state.pending_player_navigation.take();
    if let Some(delta) = continue_navigation {
        navigate_player_relative(window, delta);
    }
    start_result_metadata_hydration(window);
}

unsafe fn finish_youtube_error(window: HWND, generation: u64, message: &str) {
    if state(window).is_some_and(|state| {
        matches!(
            state.pending_youtube_work.as_ref(),
            Some(PendingYoutubeListWork::Trending(pending))
                if pending.work.generation == generation
        )
    }) {
        finish_youtube_trending_error(window, generation, message);
        return;
    }
    let return_from_collection = state(window).is_some_and(|state| {
        matches!(
            state.pending_youtube_work.as_ref(),
            Some(PendingYoutubeListWork::Collection(YoutubeCollectionWork {
                work_kind: YoutubeCollectionWorkKind::Initial,
                ..
            }))
        )
    });
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
    if return_from_collection {
        navigate_back(window);
        return;
    }
    if let Some((item, session_shuffle, start_position_seconds)) = direct_fallback {
        start_player_at(window, item, session_shuffle, start_position_seconds);
    }
}

unsafe fn finish_youtube_trending_error(window: HWND, generation: u64, error: &str) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let active = matches!(
        state.pending_youtube_work.as_ref(),
        Some(PendingYoutubeListWork::Trending(pending))
            if pending.work.generation == generation
    );
    if !active {
        return;
    }
    let _ = state.application.fail_search(generation, error);
    state.pending_youtube_work = None;
    state.pending_youtube_trending_api = None;
    state.pending_player_navigation = None;
    let _ = EnableWindow(state.load_trending, true);
    stop_youtube_timer(window);
    let visible =
        catalog_text(&state.application, "trending_official_unavailable").replace("{error}", error);
    set_status(state, &visible, true);
    show_error_message(window, &visible);
    let returning = catalog_text(&state.application, "trending_unavailable_returning");
    set_status(state, &returning, true);
    show_main_menu(window);
}

unsafe fn finish_youtube_error_state(
    window: HWND,
    state: &mut WindowState,
    generation: u64,
    message: &str,
) {
    state.pending_queued_start = None;
    let pending = state.pending_youtube_work.clone();
    let (was_initial, restore_search_focus) = match pending.as_ref() {
        Some(PendingYoutubeListWork::Search(work)) => {
            let _ = state.application.fail_search(generation, message);
            (work.work_kind == SearchWorkKind::Initial, true)
        }
        Some(PendingYoutubeListWork::Trending(_)) => {
            let _ = state.application.fail_search(generation, message);
            (true, false)
        }
        Some(PendingYoutubeListWork::Collection(work)) => {
            let _ = state
                .application
                .fail_youtube_collection(generation, message);
            (work.work_kind == YoutubeCollectionWorkKind::Initial, false)
        }
        Some(PendingYoutubeListWork::PlaylistPlayback(_)) | None => (true, false),
    };
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
        let _ = SetFocus(Some(if restore_search_focus {
            state.search_edit
        } else {
            active_primary_control(state)
        }));
    }
}

unsafe fn stop_youtube_timer(window: HWND) {
    if state(window).is_none_or(|state| {
        !state.youtube_search.is_pending()
            && !state.youtube_metadata.is_pending()
            && !state.youtube_subscriptions.is_pending()
            && state.pending_youtube_api_metadata.is_none()
            && state.pending_youtube_trending_api.is_none()
            && state.pending_subscription_check.is_none()
            && state.pending_podcast_work.is_none()
    }) {
        let _ = KillTimer(Some(window), YOUTUBE_TIMER_ID);
    }
}

unsafe fn stop_playback_timer(window: HWND) {
    let _ = KillTimer(Some(window), PLAYBACK_TIMER_ID);
}

unsafe fn cancel_youtube_work(window: HWND, state: &mut WindowState) {
    if state.pending_youtube_work.is_none()
        && state.pending_youtube_resolve.is_none()
        && state.pending_youtube_metadata.is_none()
        && state.pending_youtube_api_metadata.is_none()
        && state.pending_youtube_trending_api.is_none()
        && !state.youtube_search.is_pending()
        && !state.youtube_metadata.is_pending()
    {
        return;
    }
    if let Some(work) = state.pending_youtube_work.as_ref() {
        match work {
            PendingYoutubeListWork::Search(_) | PendingYoutubeListWork::Trending(_) => {
                let _ = state.application.cancel_pending_search();
            }
            PendingYoutubeListWork::Collection(_) => {
                let _ = state.application.cancel_pending_youtube_collection();
            }
            PendingYoutubeListWork::PlaylistPlayback(_) => {}
        }
    }
    state.pending_youtube_work = None;
    state.pending_youtube_resolve = None;
    state.pending_youtube_metadata = None;
    state.pending_youtube_api_metadata = None;
    state.pending_youtube_trending_api = None;
    state.hydrated_youtube_urls.clear();
    state.youtube_api_metadata_disabled_scopes.clear();
    state.deferred_youtube_metadata_rows.clear();
    state.pending_player_navigation = None;
    state.pending_queued_start = None;
    let _ = state.youtube_search.cancel();
    let _ = state.youtube_metadata.cancel();
    let _ = EnableWindow(state.search, true);
    let _ = EnableWindow(state.load_trending, true);
    stop_youtube_timer(window);
}

unsafe fn start_result_metadata_hydration(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.pending_youtube_metadata.is_some()
        || state.pending_youtube_api_metadata.is_some()
        || state.youtube_metadata.is_pending()
    {
        return;
    }
    if state.hydrated_youtube_urls.len() > 1_000 {
        state.hydrated_youtube_urls.clear();
    }
    let (scope, items) = match state.view {
        MainView::Results | MainView::Trending => (
            YoutubeMetadataScope::Search(state.application.search_session().generation()),
            state.application.search_session().items(),
        ),
        MainView::YoutubeCollection => {
            let Some(collection) = state.application.youtube_collection() else {
                return;
            };
            (
                YoutubeMetadataScope::Collection(collection.generation()),
                collection.items(),
            )
        }
        _ => return,
    };
    let api_key = state.application.settings().youtube_data_api_key.trim();
    let use_api =
        !api_key.is_empty() && !state.youtube_api_metadata_disabled_scopes.contains(&scope);
    let batch_size = if use_api {
        YOUTUBE_API_METADATA_BATCH_SIZE
    } else {
        YOUTUBE_METADATA_BATCH_SIZE
    };
    let candidates: Vec<apricot_core::MediaItem> = items
        .iter()
        .filter(|item| item_needs_youtube_metadata(item))
        .filter(|item| {
            item.url
                .as_ref()
                .is_some_and(|url| !state.hydrated_youtube_urls.contains(url.as_str()))
        })
        .take(batch_size)
        .cloned()
        .collect();
    if candidates.is_empty() {
        stop_youtube_timer(window);
        return;
    }
    let urls: Vec<String> = candidates
        .iter()
        .filter_map(|item| item.url.as_ref().map(ToString::to_string))
        .collect();
    if use_api {
        let api_key = api_key.to_owned();
        let proxy = nonempty(&state.application.settings().proxy);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = YoutubeDataApiClient::new(proxy.as_deref())
                .and_then(|client| client.fetch_metadata(&api_key, &candidates))
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        state.hydrated_youtube_urls.extend(urls.clone());
        state.pending_youtube_api_metadata = Some(PendingYoutubeApiMetadata {
            scope,
            urls,
            receiver,
        });
        let _ = SetTimer(
            Some(window),
            YOUTUBE_TIMER_ID,
            YOUTUBE_TIMER_INTERVAL_MS,
            None,
        );
        return;
    }
    state.next_youtube_operation_token = state.next_youtube_operation_token.wrapping_add(1).max(1);
    let token = state.next_youtube_operation_token;
    let Some(components) = application_directory().map(|path| path.join("components")) else {
        return;
    };
    let backend = YOUTUBE_BACKEND;
    let config = youtube_session_config(state);
    if state
        .youtube_metadata
        .start_metadata(backend, &components, config, token, urls.clone())
        .is_ok()
    {
        state.hydrated_youtube_urls.extend(urls);
        state.pending_youtube_metadata = Some(PendingYoutubeMetadata { token, scope });
        let _ = SetTimer(
            Some(window),
            YOUTUBE_TIMER_ID,
            YOUTUBE_TIMER_INTERVAL_MS,
            None,
        );
    }
}

unsafe fn finish_youtube_metadata(window: HWND, token: u64, items: &[apricot_core::MediaItem]) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(pending) = state.pending_youtube_metadata else {
        return;
    };
    if pending.token != token {
        return;
    }
    state.pending_youtube_metadata = None;
    apply_youtube_metadata_results(window, pending.scope, items);
}

unsafe fn apply_youtube_metadata_results(
    window: HWND,
    scope: YoutubeMetadataScope,
    items: &[apricot_core::MediaItem],
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut changed_rows = Vec::new();
    for item in items {
        let index = youtube_metadata_item_index(state, scope, item);
        let changed = match scope {
            YoutubeMetadataScope::Search(generation) => {
                state.application.apply_search_metadata(generation, item)
            }
            YoutubeMetadataScope::Collection(generation) => state
                .application
                .apply_youtube_collection_metadata(generation, item),
        };
        if changed && let Some(index) = index {
            changed_rows.push(index);
        }
    }
    let visible = match scope {
        YoutubeMetadataScope::Search(generation) => {
            matches!(state.view, MainView::Results | MainView::Trending)
                && state.application.search_session().generation() == generation
        }
        YoutubeMetadataScope::Collection(generation) => {
            state.view == MainView::YoutubeCollection
                && state
                    .application
                    .youtube_collection()
                    .is_some_and(|collection| collection.generation() == generation)
        }
    };
    if visible {
        let focused_index = (GetFocus() == state.list)
            .then(|| usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok())
            .flatten();
        for index in changed_rows {
            if focused_index == Some(index) {
                state.deferred_youtube_metadata_rows.insert(index);
            } else {
                refresh_youtube_result_line(state, index);
            }
        }
    }
    stop_youtube_timer(window);
    start_result_metadata_hydration(window);
}

fn item_needs_youtube_metadata(item: &apricot_core::MediaItem) -> bool {
    if item.source != apricot_core::MediaSource::Youtube
        || !matches!(
            item.kind,
            apricot_core::MediaKind::Video | apricot_core::MediaKind::LiveStream
        )
        || item.url.is_none()
    {
        return false;
    }
    let has_views = ["views", "view_count"]
        .iter()
        .any(|key| metadata_value_present(item.metadata.get(*key)));
    let has_upload_time = [
        "age",
        "timestamp",
        "release_timestamp",
        "upload_date",
        "uploaded_at",
        "publish_date",
    ]
    .iter()
    .any(|key| metadata_value_present(item.metadata.get(*key)));
    !has_views || !has_upload_time
}

fn metadata_value_present(value: Option<&serde_json::Value>) -> bool {
    value.is_some_and(|value| {
        !value.is_null() && value.as_str().is_none_or(|value| !value.trim().is_empty())
    })
}

fn youtube_metadata_item_index(
    state: &WindowState,
    scope: YoutubeMetadataScope,
    hydrated: &apricot_core::MediaItem,
) -> Option<usize> {
    let identity = hydrated.stable_identity()?;
    let items = match scope {
        YoutubeMetadataScope::Search(generation)
            if state.application.search_session().generation() == generation =>
        {
            state.application.search_session().items()
        }
        YoutubeMetadataScope::Collection(generation) => {
            let collection = state.application.youtube_collection()?;
            if collection.generation() != generation {
                return None;
            }
            collection.items()
        }
        YoutubeMetadataScope::Search(_) => return None,
    };
    items
        .iter()
        .position(|item| item.stable_identity().as_deref() == Some(identity.as_str()))
}

unsafe fn refresh_youtube_result_line(state: &WindowState, index: usize) {
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let (item, selected) = match state.view {
        MainView::Results | MainView::Trending => (
            state.application.search_session().items().get(index),
            state.application.search_session().selected_index(),
        ),
        MainView::YoutubeCollection => state
            .application
            .youtube_collection()
            .map_or((None, 0), |collection| {
                (collection.items().get(index), collection.selected_index())
            }),
        _ => return,
    };
    let Some(item) = item else {
        return;
    };
    let label = wide(&result_label(item, &catalog, state.application.downloads()));
    SendMessageW(state.list, LB_DELETESTRING, Some(WPARAM(index)), None);
    SendMessageW(
        state.list,
        LB_INSERTSTRING,
        Some(WPARAM(index)),
        Some(LPARAM(label.as_ptr() as isize)),
    );
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
}

unsafe fn apply_deferred_youtube_metadata_rows(state: &mut WindowState, exclude: Option<usize>) {
    let pending = std::mem::take(&mut state.deferred_youtube_metadata_rows);
    for index in pending {
        if Some(index) == exclude {
            state.deferred_youtube_metadata_rows.insert(index);
        } else {
            refresh_youtube_result_line(state, index);
        }
    }
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
        if !matches!(
            state.view,
            MainView::Results | MainView::Trending | MainView::YoutubeCollection
        ) {
            return;
        }
        let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
        let Ok(index) = usize::try_from(selected) else {
            return;
        };
        if matches!(state.view, MainView::Results | MainView::Trending) {
            if !state.application.select_search_result(index) {
                return;
            }
            if index + 1 == state.application.search_session().items().len() {
                state
                    .application
                    .request_more_search_results()
                    .map(PendingYoutubeListWork::Search)
            } else {
                None
            }
        } else {
            if !state.application.select_youtube_collection_result(index) {
                return;
            }
            if state
                .application
                .youtube_collection()
                .is_some_and(|collection| index + 1 == collection.items().len())
            {
                state
                    .application
                    .request_more_youtube_collection_results()
                    .map(PendingYoutubeListWork::Collection)
            } else {
                None
            }
        }
    };
    if let Some(state) = state_mut(window) {
        let selected = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok();
        apply_deferred_youtube_metadata_rows(state, selected);
    }
    if let Some(work) = work {
        if let Some(state) = state_mut(window) {
            let message = catalog_text(&state.application, "loading_more_results");
            set_status(state, &message, true);
        }
        match work {
            PendingYoutubeListWork::Search(work) => start_youtube_work(window, work),
            PendingYoutubeListWork::Trending(_) | PendingYoutubeListWork::PlaylistPlayback(_) => {}
            PendingYoutubeListWork::Collection(work) => {
                start_youtube_collection_work(window, work);
            }
        }
    }
}

unsafe fn local_folder_selection_changed(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    if let Ok(index) = usize::try_from(selected) {
        let _ = state.application.select_local_folder_item(index);
    }
}

unsafe fn refresh_results(state: &mut WindowState, focus: bool) {
    state.deferred_youtube_metadata_rows.clear();
    set_open_button_label(state, "open");
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let accessible_name = if state.view == MainView::Trending {
        catalog.text("trending")
    } else {
        catalog.text("result_list")
    };
    crate::accessibility_win32::set_control_name(state.list, accessible_name);
    let items = state.application.search_session().items();
    if items.is_empty() {
        add_list_string(state.list, catalog.text("no_results"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("no_results"), true);
    } else {
        for item in items {
            add_list_string(
                state.list,
                &result_label(item, &catalog, state.application.downloads()),
            );
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

unsafe fn refresh_youtube_collection(state: &mut WindowState, focus: bool) {
    state.deferred_youtube_metadata_rows.clear();
    set_open_button_label(state, "open");
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let Some(collection) = state.application.youtube_collection() else {
        return;
    };
    crate::accessibility_win32::set_control_name(state.list, collection.title());
    if collection.phase() == YoutubeCollectionPhase::LoadingInitial {
        let loading = catalog
            .text("loading_playlist")
            .replace("{title}", collection.title());
        add_list_string(state.list, &loading);
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, &loading, false);
        if focus {
            let _ = SetFocus(Some(state.list));
        }
        return;
    }
    if collection.items().is_empty() {
        add_list_string(state.list, catalog.text("no_results"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("no_results"), true);
    } else {
        for item in collection.items() {
            add_list_string(
                state.list,
                &result_label(item, &catalog, state.application.downloads()),
            );
        }
        let selected = collection
            .selected_index()
            .min(collection.items().len() - 1);
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
        let found = catalog
            .text("found")
            .replace("{count}", &collection.items().len().to_string());
        set_status(state, &found, true);
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

unsafe fn refresh_local_folder(state: &mut WindowState, focus: bool, announce_status: bool) {
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    crate::accessibility_win32::set_control_name(state.list, catalog.text("play_from_folder"));
    set_open_button_label(state, "play");
    let session = state.application.local_folder_session();
    if session.is_empty() {
        add_list_string(state.list, catalog.text("folder_no_media"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("folder_no_media"), announce_status);
    } else {
        for item in session.items() {
            add_list_string(state.list, &local_folder_result_label(item, &catalog));
        }
        let selected = session.selected_index().min(session.items().len() - 1);
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

/// Python `refresh_favorites` and `refresh_history`: only an empty list
/// writes the status bar, and opening the screen announces nothing.
unsafe fn refresh_media_collection(state: &mut WindowState, focus: bool) {
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
    crate::accessibility_win32::set_control_name(state.list, catalog.text(name_key));
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
        set_status(state, catalog.text(empty_key), false);
    } else {
        for item in items {
            add_list_string(
                state.list,
                &media_collection_label(item, state.view, &catalog),
            );
        }
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
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
    crate::accessibility_win32::set_control_name(state.list, catalog.text("notification_center"));
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
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

/// Python `refresh_subscriptions`: keeps the selected subscription, or the same
/// row when it is gone, and only an empty list writes the status bar.
unsafe fn refresh_subscriptions(state: &mut WindowState, focus: bool, preferred_url: Option<&str>) {
    let previous_row = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok();
    let previous_url = preferred_url
        .map(str::to_owned)
        .or_else(|| selected_subscription(state).map(|subscription| subscription.url));
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    crate::accessibility_win32::set_control_name(state.list, catalog.text("subscriptions"));
    set_open_button_label(state, "subscription_open_videos");
    set_control_text(state, state.collection_remove, "remove");
    let visible = state.application.visible_subscription_indices();
    if state.application.subscriptions().is_empty() {
        add_list_string(state.list, catalog.text("subscription_empty"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("subscription_empty"), false);
    } else if visible.is_empty() {
        add_list_string(state.list, catalog.text("category_filter_empty"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("category_filter_empty"), false);
    } else {
        for index in &visible {
            if let Some(subscription) = state.application.subscriptions().get(*index) {
                add_list_string(state.list, &subscription_label(subscription, &catalog));
            }
        }
        let selected = previous_url
            .as_deref()
            .and_then(|url| {
                visible.iter().position(|index| {
                    state
                        .application
                        .subscriptions()
                        .get(*index)
                        .is_some_and(|subscription| subscription.url == url)
                })
            })
            .or(previous_row)
            .unwrap_or_default()
            .min(visible.len() - 1);
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    }
    if let Some(error) = state.application.subscription_load_error() {
        set_status(
            state,
            &format!("Subscriptions could not be loaded: {error}"),
            true,
        );
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

fn subscription_label(
    subscription: &apricot_app::Subscription,
    catalog: &apricot_core::TranslationCatalog,
) -> String {
    let mut parts = vec![subscription.title.clone()];
    if !subscription.category.trim().is_empty() {
        parts.push(
            catalog
                .text("category_value")
                .replace("{category}", &subscription.category),
        );
    }
    let checked = subscription
        .last_checked
        .filter(|timestamp| *timestamp > 0.0)
        .and_then(format_timestamp)
        .map_or_else(
            || catalog.text("subscription_never_checked").to_owned(),
            |time| {
                catalog
                    .text("subscription_last_checked")
                    .replace("{time}", &time)
            },
        );
    parts.push(checked);
    if subscription.last_new_count > 0 {
        parts.push(
            catalog
                .text("subscription_new_videos")
                .replace("{count}", &subscription.last_new_count.to_string())
                .replace("{title}", &subscription.title),
        );
    }
    parts.join(" | ")
}

fn format_timestamp(timestamp: f64) -> Option<String> {
    std::time::Duration::try_from_secs_f64(timestamp)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|timestamp| {
            timestamp
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
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

/// Python `refresh_user_playlists`: only an empty list writes the status bar.
unsafe fn refresh_user_playlists(state: &mut WindowState, focus: bool) {
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    crate::accessibility_win32::set_control_name(state.list, catalog.text("playlists"));
    set_open_button_label(state, "open_playlist");
    set_control_text(state, state.collection_remove, "remove_playlist");
    let playlists = state.application.user_playlists();
    if playlists.is_empty() {
        add_list_string(state.list, catalog.text("no_playlists"));
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        set_status(state, catalog.text("no_playlists"), false);
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
    }
    if focus {
        let _ = SetFocus(Some(state.list));
    }
}

unsafe fn refresh_user_playlist_items(state: &mut WindowState, focus: bool, announce_status: bool) {
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    crate::accessibility_win32::set_control_name(state.list, catalog.text("playlist_items"));
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
        add_list_string(
            state.list,
            &result_label(item, &catalog, state.application.downloads()),
        );
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

unsafe fn append_youtube_collection_results(state: &mut WindowState, added: usize) {
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let Some(collection) = state.application.youtube_collection() else {
        return;
    };
    if added == 0 {
        set_status(state, catalog.text("no_more_results"), true);
        return;
    }
    let first_new = collection.items().len().saturating_sub(added);
    for item in &collection.items()[first_new..] {
        add_list_string(
            state.list,
            &result_label(item, &catalog, state.application.downloads()),
        );
    }
    let selected = collection
        .selected_index()
        .min(collection.items().len() - 1);
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    let loaded = catalog
        .text("search_more_loaded")
        .replace("{count}", &collection.items().len().to_string());
    set_status(state, &loaded, true);
}

/// Python's `result_line` for online results, with the download queue marker
/// last.
fn result_label(
    item: &apricot_core::MediaItem,
    catalog: &apricot_core::TranslationCatalog,
    downloads: &apricot_app::DownloadController,
) -> String {
    let mut parts = result_label_parts(item, catalog);
    if let Some(queued) = downloads.queued_item(item) {
        parts.push(catalog.text(queued_marker_key(queued)).to_owned());
    }
    parts.join(" | ")
}

fn result_label_parts(
    item: &apricot_core::MediaItem,
    catalog: &apricot_core::TranslationCatalog,
) -> Vec<String> {
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
        let mut parts = vec![item.title.clone(), kind.to_owned()];
        if item.kind == apricot_core::MediaKind::Playlist
            && let Some(count) = playlist_count_text(item, catalog)
        {
            parts.push(count);
        }
        return parts;
    }
    let mut parts = vec![
        item.title.clone(),
        format!("{}: {}", catalog.text("channel"), item.channel),
    ];
    let views = apricot_app::player_information::display_count(&item.metadata, "views")
        .or_else(|| apricot_app::player_information::display_count(&item.metadata, "view_count"))
        .unwrap_or_default();
    parts.push(format!("{}: {views}", catalog.text("views")));
    let uploaded = apricot_app::player_information::display_upload_age(catalog, item);
    parts.push(if uploaded.is_empty() {
        catalog.text("uploaded_unknown").to_owned()
    } else {
        uploaded
    });
    if let Some(duration) = item.duration_seconds {
        parts.push(format_duration(duration));
    }
    parts.push(kind.to_owned());
    parts
}

/// Python's `playlist_count_text`.
fn playlist_count_text(
    item: &apricot_core::MediaItem,
    catalog: &apricot_core::TranslationCatalog,
) -> Option<String> {
    let raw = ["playlist_count", "n_entries", "video_count"]
        .iter()
        .filter_map(|key| item.metadata.get(*key))
        .find_map(|value| match value {
            serde_json::Value::Number(number) => Some(number.to_string()),
            serde_json::Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        })?;
    Some(match raw.replace(',', "").trim().parse::<i64>() {
        Ok(count) => catalog
            .text("playlist_video_count")
            .replace("{count}", &count.to_string()),
        Err(_) => raw,
    })
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

fn selected_combo_index(control: HWND) -> Option<usize> {
    let selected = unsafe { SendMessageW(control, CB_GETCURSEL, None, None).0 };
    usize::try_from(selected).ok()
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
    options.initial_pitch = mpv_pitch_property(pitch_mode(state), audio.pitch);
    options.audio_pitch_correction = speed_audio_mode(state).audio_pitch_correction();
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
    options.initial_audio_filter = player_audio_filter(state, None, None);
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

/// Python `player_failed` message shown when mpv cannot start or play.
fn player_failed_message(application: &Application, error: &str) -> String {
    catalog_text(application, "player_failed").replace("{error}", error)
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
    // The message box hands activation back to the main window itself, so
    // focus returns to the control that had it, as with wx.MessageBox.
    let previous = GetFocus();
    let message = wide(message);
    let _ = MessageBoxW(
        Some(window),
        PCWSTR(message.as_ptr()),
        w!("ApricotPlayer 2 Beta"),
        MB_OK | MB_ICONINFORMATION,
    );
    if !previous.is_invalid()
        && windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(previous)).as_bool()
    {
        let _ = SetFocus(Some(previous));
    }
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
    if state(window).is_some_and(|state| {
        state.view == MainView::Player
            && state.player_controls.details_visible()
            && GetFocus() == state.player_controls.details_text()
            && details_text_navigation_key(chord)
    }) {
        return false;
    }
    if !chord.control
        && !chord.shift
        && !chord.alt
        && chord.key == ShortcutKey::Escape
        && state(window).is_some_and(|state| state.view != MainView::MainMenu)
    {
        if !handle_player_back_in_place(window) {
            navigate_back(window);
        }
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
        | MainView::Trending
        | MainView::YoutubeCollection
        | MainView::LocalFolder
        | MainView::Favorites
        | MainView::History
        | MainView::NotificationCenter
        | MainView::Subscriptions
        | MainView::RssFeeds
        | MainView::RssItems
        | MainView::PodcastSearchResults
        | MainView::PodcastCategories
        | MainView::UserPlaylists
        | MainView::UserPlaylistItems
        | MainView::DownloadQueue => (ActionScope::List, false),
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
    if action.id.as_str() == "player_back" && handle_player_back_in_place(window) {
        return true;
    }
    activate_action(window, action.id.as_str());
    true
}

/// Python `details_text_navigation_key`: reading keys stay in the details
/// field instead of seeking or changing volume.
fn details_text_navigation_key(chord: apricot_core::shortcut::ShortcutChord) -> bool {
    matches!(
        chord.key,
        ShortcutKey::Up
            | ShortcutKey::Down
            | ShortcutKey::Left
            | ShortcutKey::Right
            | ShortcutKey::Home
            | ShortcutKey::End
            | ShortcutKey::PageUp
            | ShortcutKey::PageDown
    ) || (chord.control && matches!(chord.key, ShortcutKey::Character('c' | 'C' | 'a' | 'A')))
}

/// Python `player_back` on the player page: it first hides details, then
/// leaves full screen, and only then leaves the player.
unsafe fn handle_player_back_in_place(window: HWND) -> bool {
    if hide_player_details_for_back(window) {
        return true;
    }
    let fullscreen = state(window).is_some_and(|state| {
        state.view == MainView::Player
            && state
                .application
                .player_session()
                .enabled_toggles()
                .contains(&SessionToggle::Fullscreen)
    });
    if fullscreen {
        // Python `exit_fullscreen_to_player` without an announcement.
        set_player_fullscreen(window, false, false, false);
    }
    fullscreen
}

/// Python `player_back`: while details are shown the shortcut only hides them.
unsafe fn hide_player_details_for_back(window: HWND) -> bool {
    if !state(window).is_some_and(|state| {
        state.view == MainView::Player && state.player_controls.details_visible()
    }) {
        return false;
    }
    hide_player_details(window);
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
        "trending" | "open_trending" => show_trending(window),
        "resume_last_session" => resume_last_player_session(window),
        "open_direct_link" => show_direct_link(window),
        "open_favorites" => show_media_collection(window, MainView::Favorites),
        "open_history" => show_media_collection(window, MainView::History),
        "open_subscriptions" => show_subscriptions(window),
        "open_podcasts_rss" => show_rss_feeds(window),
        "new_subscription_videos" => show_notification_center(window),
        "open_bookmarks" => show_bookmarks_dialog(window, false, false),
        "open_playlists" => show_user_playlists(window),
        "open_settings" => open_settings(window),
        "open_action_finder" => show_action_finder(window),
        "open_play_file" => open_media_file(window),
        "open_play_from_folder" => open_media_folder(window),
        "open_selected" => activate_selection(window),
        "open_channel" => open_item_channel(window),
        "result_column_previous" => announce_active_media_column(window, false),
        "result_column_next" => announce_active_media_column(window, true),
        "background_play_pause" | "player_play_pause" => toggle_player_pause(window),
        "player_back" => navigate_back(window),
        "player_previous" => navigate_player_relative(window, -1),
        "player_next" => navigate_player_relative(window, 1),
        "player_time" => announce_player_time(window),
        "player_previous_chapter" => seek_relative_player_chapter(window, false),
        "player_chapters" => show_player_chapters(window),
        "player_next_chapter" => seek_relative_player_chapter(window, true),
        "player_marker_start" => toggle_player_clip_marker(window, true),
        "player_marker_end" => toggle_player_clip_marker(window, false),
        "player_preview_marked_clip" => preview_marked_clip(window),
        "player_volume_status" => announce_player_volume(window),
        "player_format_status" => announce_player_format(window),
        "player_details" => show_player_details(window),
        "player_lyrics" => show_player_lyrics(window),
        "player_transcript" => show_player_transcript(window),
        "player_add_bookmark" => show_add_current_bookmark_prompt(window),
        "player_bookmarks" => show_bookmarks_dialog(window, true, false),
        "player_seek_back" => seek_player(window, -configured_seek_seconds(window)),
        "player_seek_forward" => seek_player(window, configured_seek_seconds(window)),
        "player_seek_back_large" => seek_player(window, -60.0),
        "player_seek_forward_large" => seek_player(window, 60.0),
        "player_seek_back_huge" => seek_player(window, -600.0),
        "player_seek_forward_huge" => seek_player(window, 600.0),
        "player_seek_start" => seek_player_to_start(window),
        "player_seek_end" => seek_player_to_end(window),
        "player_volume_up" => adjust_player_volume(window, configured_volume_step(window)),
        "player_volume_down" => adjust_player_volume(window, -configured_volume_step(window)),
        "player_speed_up" => adjust_player_speed(window, configured_speed_step(window)),
        "player_speed_down" => adjust_player_speed(window, -configured_speed_step(window)),
        "player_pitch_up" => adjust_player_pitch(window, configured_pitch_step(window)),
        "player_pitch_down" => adjust_player_pitch(window, -configured_pitch_step(window)),
        "player_reset_speed_pitch" => reset_player_speed_pitch(window),
        "player_repeat" => toggle_player_session_setting(window, SessionToggle::Repeat),
        "player_shuffle" => toggle_player_session_setting(window, SessionToggle::Shuffle),
        "player_replaygain" => cycle_player_replaygain(window),
        "player_next_related" => play_related_video(window, true),
        "player_fullscreen" => toggle_player_fullscreen(window, false, true),
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
        "subscribe_channel" => subscribe_active_channel(window),
        "unsubscribe_channel" => unsubscribe_active_channel(window),
        "context_menu" => show_context_menu_for_active_view(window),
        "add_to_playback_queue" => add_active_item_to_playback_queue(window),
        "remove_from_playback_queue" => remove_active_item_from_playback_queue(window),
        "open_playback_queue" => show_playback_queue(window),
        "open_current_downloads" => show_download_queue(window),
        "download_audio" => start_download_shortcut(window, DownloadChoice::Audio),
        "download_video" => start_download_shortcut(window, DownloadChoice::Video),
        "queue_audio" => toggle_active_download_queue(window),
        "toggle_podcast_played" => toggle_selected_rss_played(window),
        "clear_podcast_progress" => clear_selected_rss_progress(window),
        "save_podcast_speed_preset" => save_current_podcast_speed_preset(window),
        _ => announce_unimplemented_action(window, action_id),
    }
}

unsafe fn navigate_player_relative(window: HWND, delta: i32) {
    navigate_player_relative_from(window, delta, false);
}

/// Python `handle_player_eof` and `play_next_standard_fallback`: without a
/// next item the end of playback is announced as finished.
unsafe fn navigate_player_relative_after_end(window: HWND) {
    navigate_player_relative_from(window, 1, true);
}

unsafe fn navigate_player_relative_from(window: HWND, delta: i32, after_end: bool) {
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
        PlayerNavigationOutcome::LoadingMoreCollection(work) => {
            let Some(state) = state_mut(window) else {
                return;
            };
            state.pending_player_navigation = Some(delta);
            let message = catalog_text(&state.application, "loading_more_results");
            set_status(state, &message, true);
            start_youtube_collection_work(window, work);
        }
        PlayerNavigationOutcome::Unavailable => {
            let Some(state) = state(window) else {
                return;
            };
            if after_end {
                set_status(
                    state,
                    &catalog_text(&state.application, "playback_finished"),
                    state.application.settings().announce_playback_finished,
                );
                return;
            }
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
        MainView::Results | MainView::Trending => {
            let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
            let index = usize::try_from(selected).ok()?;
            state
                .application
                .search_session()
                .items()
                .get(index)
                .cloned()
        }
        MainView::YoutubeCollection => {
            let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
            let index = usize::try_from(selected).ok()?;
            state
                .application
                .youtube_collection()?
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
                .items()
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
        MainView::Subscriptions => selected_subscription(state)
            .as_ref()
            .and_then(subscription_media_item),
        MainView::RssItems => selected_rss_episode(state).cloned(),
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
        MainView::MainMenu
        | MainView::Search
        | MainView::DirectLink
        | MainView::RssFeeds
        | MainView::PodcastSearchResults
        | MainView::PodcastCategories
        | MainView::UserPlaylists
        | MainView::DownloadQueue => None,
    }
}

unsafe fn subscribe_active_channel(window: HWND) {
    let Some(item) = active_media_item(window) else {
        return;
    };
    let title = if item.channel.trim().is_empty() {
        item.title.clone()
    } else {
        item.channel.clone()
    };
    let result =
        state_mut(window).map(|state| state.application.subscribe_to_item(&item, unix_timestamp()));
    let Some(state) = state(window) else {
        return;
    };
    match result {
        Some(Ok(SubscriptionAddOutcome::Added(_))) => {
            let message =
                catalog_text(&state.application, "subscription_added").replace("{title}", &title);
            set_status(state, &message, true);
        }
        Some(Ok(SubscriptionAddOutcome::AlreadyPresent)) => {
            let message =
                catalog_text(&state.application, "subscription_exists").replace("{title}", &title);
            set_status(state, &message, true);
        }
        Some(Ok(SubscriptionAddOutcome::Unsupported)) => set_status(
            state,
            &catalog_text(&state.application, "no_selection"),
            true,
        ),
        Some(Err(error)) => show_error_message(window, &error.to_string()),
        None => {}
    }
}

unsafe fn unsubscribe_active_channel(window: HWND) {
    let Some(item) = active_media_item(window) else {
        return;
    };
    let title = if item.channel.trim().is_empty() {
        item.title.clone()
    } else {
        item.channel.clone()
    };
    let result = state_mut(window).map(|state| state.application.unsubscribe_from_item(&item));
    let Some(state) = state(window) else {
        return;
    };
    match result {
        Some(Ok(SubscriptionRemoveOutcome::Removed)) => {
            let message =
                catalog_text(&state.application, "subscription_removed").replace("{title}", &title);
            set_status(state, &message, true);
        }
        Some(Ok(SubscriptionRemoveOutcome::NotFound)) => {
            let message = catalog_text(&state.application, "subscription_not_found")
                .replace("{title}", &title);
            set_status(state, &message, true);
        }
        Some(Ok(SubscriptionRemoveOutcome::Unsupported)) => set_status(
            state,
            &catalog_text(&state.application, "no_selection"),
            true,
        ),
        Some(Err(error)) => show_error_message(window, &error.to_string()),
        None => {}
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
                    refresh_media_collection(state, true);
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
        MainView::Subscriptions => remove_selected_subscription(window),
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
                // Python `clear_selected_notification` keeps the row index.
                refresh_notification_center(state, true, false, Some(index));
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
                refresh_media_collection(state, true);
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
                refresh_media_collection(state, true);
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
                // Python `clear_notifications` leaves focus where it was.
                refresh_notification_center(state, false, false, None);
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
    if !current_only {
        remember_menu_item(window, "bookmarks");
    }
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
                refresh_user_playlists(state, true);
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

/// Python `add_items_to_playlist` from the Add to playlist submenu.
unsafe fn add_active_item_to_chosen_user_playlist(window: HWND, playlist_index: usize) {
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
                refresh_user_playlists(state, true);
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

unsafe fn copy_context_location(window: HWND) {
    let special = state(window).and_then(|state| match state.view {
        MainView::RssFeeds => selected_rss_feed(state).map(|feed| feed.url.clone()),
        MainView::PodcastSearchResults => {
            selected_podcast_result(state).map(|item| item.feed_url.to_string())
        }
        _ => None,
    });
    if let Some(url) = special {
        copy_text_and_announce(window, &url, "url_copied");
    } else {
        copy_active_location(window);
    }
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
    if runtime.execute(generation, command).is_ok() {
        return true;
    }
    // Python announces a failed player command and keeps playing.
    announce_player_text(window, "timing_unavailable", &[]);
    false
}

unsafe fn toggle_player_pause(window: HWND) {
    if let Some(state) = state_mut(window) {
        state.clip_preview = None;
    }
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
    if let Some(state) = state_mut(window) {
        state.clip_preview = None;
    }
    let _ = execute_player_command(
        window,
        PlaybackCommand::SeekRelative {
            seconds,
            exact: false,
        },
    );
}

/// Python `player_seek_absolute`: exact absolute seek followed by the
/// localized jump announcement.
unsafe fn seek_player_absolute(window: HWND, seconds: f64, announcement_key: &str) {
    if let Some(state) = state_mut(window) {
        state.clip_preview = None;
    }
    if execute_player_command(
        window,
        PlaybackCommand::SeekAbsolute {
            seconds: seconds.max(0.0),
            exact: true,
        },
    ) {
        announce_player_text(window, announcement_key, &[]);
    }
}

unsafe fn seek_player_to_start(window: HWND) {
    seek_player_absolute(window, 0.0, "seeked_to_start");
}

unsafe fn seek_player_to_end(window: HWND) {
    let Some(duration) =
        state(window).and_then(|state| state.application.player_session().duration_seconds())
    else {
        announce_player_text(window, "timing_unavailable", &[]);
        return;
    };
    seek_player_absolute(window, (duration - 0.5).max(0.0), "seeked_to_end");
}

/// Python `announce_player`: status text plus speech for one catalog key.
unsafe fn announce_player_text(window: HWND, key: &str, replacements: &[(&str, &str)]) {
    let Some(state) = state(window) else {
        return;
    };
    let mut message = catalog_text(&state.application, key);
    for (name, value) in replacements {
        message = message.replace(&format!("{{{name}}}"), value);
    }
    set_status(state, &message, true);
}

/// Python `format_rate_for_speech`.
fn format_rate_for_speech(value: f64) -> String {
    format!("{value:.2}")
}

/// Python `clamp_rate`: clamp and round to two decimals.
fn clamp_rate(value: f64, minimum: f64, maximum: f64) -> f64 {
    (value.clamp(minimum, maximum) * 100.0).round() / 100.0
}

unsafe fn announce_player_time(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let session = state.application.player_session();
    let elapsed = session.position_seconds();
    let Some(duration) = session.duration_seconds() else {
        announce_player_text(window, "timing_unavailable", &[]);
        return;
    };
    announce_player_text(
        window,
        "time_announcement",
        &[
            ("elapsed", &format_duration(elapsed)),
            ("remaining", &format_duration((duration - elapsed).max(0.0))),
            ("total", &format_duration(duration)),
        ],
    );
}

unsafe fn request_external_chapters(window: HWND, direction: Option<bool>) -> bool {
    let Some(state) = state_mut(window) else {
        return false;
    };
    let session = state.application.player_session();
    let Some(item) = session.current_item() else {
        return false;
    };
    if !apricot_app::chapters::item_chapters(item).is_empty()
        || item
            .metadata
            .get("_chapters_url_checked")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        return false;
    }
    let Some(url) = item
        .metadata
        .get("chapters_url")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .map(str::to_owned)
    else {
        return false;
    };
    let generation = session.generation();
    if let Some(pending) = state.pending_chapters.as_mut()
        && pending.generation == generation
    {
        pending.direction = direction;
        return true;
    }
    let proxy = state.application.settings().proxy.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let chapters = apricot_platform::RssClient::new(Some(&proxy))
            .and_then(|client| client.fetch_chapters(&url))
            .unwrap_or_default();
        let _ = sender.send(chapters);
    });
    state.pending_chapters = Some(PendingChapters {
        generation,
        direction,
        receiver,
    });
    let _ = SetTimer(Some(window), CHAPTER_TIMER_ID, 50, None);
    true
}

unsafe fn poll_external_chapters(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(pending) = state.pending_chapters.as_ref() else {
        let _ = KillTimer(Some(window), CHAPTER_TIMER_ID);
        return;
    };
    if pending.generation != state.application.player_session().generation()
        || state.application.current_route() != Route::Player
    {
        state.pending_chapters = None;
        let _ = KillTimer(Some(window), CHAPTER_TIMER_ID);
        return;
    }
    if state.modal_open {
        return;
    }
    let chapters = match pending.receiver.try_recv() {
        Ok(chapters) => chapters,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
    };
    let pending = state
        .pending_chapters
        .take()
        .expect("pending chapter request");
    let _ = KillTimer(Some(window), CHAPTER_TIMER_ID);
    if !state
        .application
        .cache_external_chapters(pending.generation, chapters)
    {
        return;
    }
    match pending.direction {
        Some(next) => seek_relative_player_chapter(window, next),
        None => show_player_chapters(window),
    }
}

/// Python `ensure_player_for_auxiliary_view`. The Rust player lives only on
/// its own page, so outside it there is no player to show.
unsafe fn ensure_player_for_auxiliary_view(window: HWND) -> bool {
    let Some(state) = state(window) else {
        return false;
    };
    if state.view == MainView::Player && state.application.player_session().current_item().is_some()
    {
        return true;
    }
    set_status(state, &catalog_text(&state.application, "no_player"), true);
    false
}

unsafe fn show_player_chapters(window: HWND) {
    stop_controlled_repeat(window);
    if !ensure_player_for_auxiliary_view(window) {
        return;
    }
    if request_external_chapters(window, None) {
        return;
    }
    let Some(main_state) = state_mut(window) else {
        return;
    };
    let session = main_state.application.player_session();
    let Some(_) = session.current_item() else {
        return;
    };
    let chapters = apricot_app::chapters::session_chapters(session);
    if chapters.is_empty() {
        set_status(
            main_state,
            &catalog_text(&main_state.application, "no_chapters_available"),
            true,
        );
        return;
    }
    let generation = session.generation();
    let position = session.position_seconds();
    let selected = chapters
        .iter()
        .rposition(|chapter| chapter.start_seconds <= position + 0.1)
        .unwrap_or(0);
    let title = catalog_text(&main_state.application, "chapters");
    let list_name = catalog_text(&main_state.application, "chapter_list");
    let play = catalog_text(&main_state.application, "play");
    let back = catalog_text(&main_state.application, "back");
    let choices = chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            let name = if chapter.title.is_empty() {
                title.clone()
            } else {
                chapter.title.clone()
            };
            let time = format_duration(chapter.start_seconds);
            let range = chapter.end_seconds.map_or(time.clone(), |end| {
                format!("{time} - {}", format_duration(end))
            });
            format!("{}. {range}. {name}", index + 1)
        })
        .collect::<Vec<_>>();
    main_state.modal_open = true;
    let shortcuts = &main_state.application.settings().keyboard_shortcuts;
    let configured = |id: &str, default: &str| {
        apricot_core::shortcut::ShortcutChord::parse(
            shortcuts.get(id).map_or(default, String::as_str),
        )
    };
    let behavior = crate::playlist_dialog_win32::PickerBehavior {
        initial_selection: selected,
        accept: configured("open_selected", "Enter"),
        back: configured("player_back", "Escape"),
    };
    let outcome = crate::playlist_dialog_win32::choose_configured(
        window, &title, &list_name, &choices, behavior, &play, &back,
    );
    if let Some(main_state) = state_mut(window) {
        main_state.modal_open = false;
    }
    resume_deferred_window_work(window);
    match outcome {
        Ok(Some(index)) => {
            if state(window).is_some_and(|state| {
                state.application.current_route() == Route::Player
                    && state.application.player_session().is_open()
                    && state.application.player_session().generation() == generation
            }) && let Some(chapter) = chapters.get(index)
                && seek_player_chapter(window, chapter.start_seconds)
                && let Some(main_state) = state(window)
            {
                let name = if chapter.title.is_empty() {
                    &title
                } else {
                    &chapter.title
                };
                let message = catalog_text(&main_state.application, "chapter_selected")
                    .replace("{title}", name)
                    .replace("{time}", &format_duration(chapter.start_seconds));
                set_status(main_state, &message, true);
            }
        }
        Ok(None) => {}
        Err(error) => show_error_message(window, &error.to_string()),
    }
    if let Some(main_state) = state(window) {
        let _ = SetFocus(Some(active_primary_control(main_state)));
    }
}

unsafe fn seek_relative_player_chapter(window: HWND, next: bool) {
    if request_external_chapters(window, Some(next)) {
        return;
    }
    let Some(main_state) = state(window) else {
        return;
    };
    let session = main_state.application.player_session();
    let Some(_) = session.current_item() else {
        return;
    };
    let chapters = apricot_app::chapters::session_chapters(session);
    let Some(chapter) =
        apricot_app::chapters::relative_chapter(&chapters, session.position_seconds(), next)
    else {
        set_status(
            main_state,
            &catalog_text(&main_state.application, "no_chapters_available"),
            true,
        );
        return;
    };
    let title = if chapter.title.is_empty() {
        catalog_text(&main_state.application, "chapters")
    } else {
        chapter.title.clone()
    };
    let message = catalog_text(&main_state.application, "chapter_selected")
        .replace("{title}", &title)
        .replace("{time}", &format_duration(chapter.start_seconds));
    let target = chapter.start_seconds;
    if seek_player_chapter(window, target)
        && let Some(state) = state(window)
    {
        set_status(state, &message, true);
    }
}

unsafe fn seek_player_chapter(window: HWND, seconds: f64) -> bool {
    if let Some(state) = state_mut(window) {
        state.clip_preview = None;
    }
    execute_player_command(
        window,
        PlaybackCommand::SeekAbsolute {
            seconds,
            exact: true,
        },
    )
}

unsafe fn toggle_player_clip_marker(window: HWND, start: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.clip_preview.is_some() {
        if let Some(runtime) = state.playback.as_ref()
            && let Err(error) =
                runtime.cancel_preview(state.application.player_session().generation())
        {
            show_error_message(window, &format!("Player command failed: {error}"));
            return;
        }
        state.clip_preview = None;
    }
    let session = state.application.player_session();
    if !session.is_open() {
        return;
    }
    if matches!(
        session.phase(),
        PlaybackPhase::Starting | PlaybackPhase::Failed
    ) {
        set_status(
            state,
            &catalog_text(&state.application, "timing_unavailable"),
            true,
        );
        return;
    }
    let position = state.application.toggle_player_clip_marker(start);
    let key = match (start, position.is_some()) {
        (true, true) => "clip_start_marker_set",
        (true, false) => "clip_start_marker_cleared",
        (false, true) => "clip_end_marker_set",
        (false, false) => "clip_end_marker_cleared",
    };
    let mut message = catalog_text(&state.application, key);
    if let Some(position) = position {
        message = message.replace("{time}", &format_duration(position));
    }
    set_status(state, &message, true);
}

unsafe fn preview_marked_clip(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let session = state.application.player_session();
    let Some((start, end)) = session.clip_range() else {
        let key = if session.clip_start_seconds().is_some() && session.clip_end_seconds().is_some()
        {
            "clip_marker_invalid"
        } else {
            "clip_markers_missing"
        };
        set_status(state, &catalog_text(&state.application, key), true);
        return;
    };
    let generation = session.generation();
    let Some(runtime) = state.playback.as_ref() else {
        return;
    };
    if let Err(error) = runtime.preview(generation, start, end) {
        show_error_message(window, &format!("Player command failed: {error}"));
        return;
    }
    state.clip_preview = Some(ClipPreview { generation });
    let message = catalog_text(&state.application, "clip_preview_started")
        .replace("{start}", &format_duration(start))
        .replace("{end}", &format_duration(end));
    set_status(state, &message, true);
}

unsafe fn announce_player_volume(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let Some(audio) = state.application.player_session().audio() else {
        announce_player_text(window, "timing_unavailable", &[]);
        return;
    };
    let volume = format!("{:.0}", audio.volume.round());
    announce_player_text(window, "volume_announcement", &[("volume", &volume)]);
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

/// Python `build_video_details_text`.
fn player_details_text(application: &Application) -> String {
    application
        .player_details_text()
        .unwrap_or_else(|| catalog_text(application, "details_unavailable"))
}

fn player_details_labels(application: &Application) -> crate::player_controls_win32::DetailsLabels {
    crate::player_controls_win32::DetailsLabels {
        title: catalog_text(application, "video_details"),
        copy: catalog_text(application, "copy_details"),
        back: catalog_text(application, "back"),
    }
}

/// Python `update_details_text` after speed, pitch or metadata changes.
unsafe fn sync_player_details(state: &WindowState) {
    if state.view == MainView::Player && state.player_controls.details_visible() {
        state
            .player_controls
            .set_details_text(&player_details_text(&state.application));
    }
}

/// Python `show_video_details`: the details open inside the player page.
unsafe fn show_player_details(window: HWND) {
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.view != MainView::Player {
        set_status(state, &catalog_text(&state.application, "no_player"), true);
        return;
    }
    let text = player_details_text(&state.application);
    state
        .player_controls
        .show_details(&player_details_labels(&state.application), &text);
    layout_controls_state(window, state);
    let _ = SetFocus(Some(state.player_controls.details_text()));
    set_status(
        state,
        &catalog_text(&state.application, "video_details"),
        true,
    );
}

/// Python `hide_video_details`.
unsafe fn hide_player_details(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if !state.player_controls.details_visible() {
        return;
    }
    state.player_controls.hide_details();
    layout_controls_state(window, state);
    let _ = SetFocus(Some(state.player_controls.video_host()));
    set_status(
        state,
        &catalog_text(&state.application, "details_closed"),
        true,
    );
}

/// Python `copy_video_details`: copies freshly built text.
unsafe fn copy_player_details(window: HWND) {
    let Some(text) = state(window).map(|state| player_details_text(&state.application)) else {
        return;
    };
    if crate::clipboard_win32::copy_text(window, &text).is_ok()
        && let Some(state) = state(window)
    {
        set_status(
            state,
            &catalog_text(&state.application, "details_copied"),
            true,
        );
    }
}

// Keep the generation guard and modal lifetime in one place.
#[allow(clippy::too_many_lines)]
unsafe fn show_player_transcript(window: HWND) {
    stop_controlled_repeat(window);
    if !ensure_player_for_auxiliary_view(window) {
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(item) = state.application.player_session().current_item().cloned() else {
        return;
    };
    let generation = state.application.player_session().generation();
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let keys = [
        "transcript",
        "transcript_search",
        "transcript_loading",
        "play",
        "copy_transcript_line",
        "copy_transcript",
        "copy_timestamp_link",
        "back",
        "transcript_no_search_results",
        "no_transcript_available",
        "transcript_failed",
        "transcript_rate_limited",
        "transcript_copied",
        "transcript_line_copied",
        "transcript_loaded_from_source",
        "transcript_source_local",
        "transcript_source_subtitles",
        "transcript_source_auto_captions",
    ];
    let labels = keys
        .into_iter()
        .map(|key| (key.to_owned(), catalog.text(key).to_owned()))
        .collect();
    let languages = state.application.settings().subtitle_languages.clone();
    let user_agent = state.application.settings().cookie_user_agent.clone();
    let config = youtube_session_config(state);
    let cached = state.application.player_session().transcript().cloned();
    let (sender, receiver) = std::sync::mpsc::channel();
    if let Some(cached) = cached {
        let source_key = match cached.source_key.as_str() {
            "transcript_source_local" => "transcript_source_local",
            "transcript_source_auto_captions" => "transcript_source_auto_captions",
            _ => "transcript_source_subtitles",
        };
        let _ = sender.send(Ok(crate::transcript_loader::LoadedTranscript {
            entries: cached.entries,
            source_key,
        }));
    } else {
        let worker_item = item.clone();
        let executable =
            application_directory().map(|path| path.join("components").join("yt-dlp.exe"));
        std::thread::spawn(move || {
            let result = crate::transcript_loader::load(
                &worker_item,
                executable.as_deref().unwrap_or(std::path::Path::new("")),
                config,
                &languages,
                &user_agent,
            );
            let _ = sender.send(result);
        });
    }
    let options = crate::transcript_win32::TranscriptDialogOptions {
        labels,
        can_copy_timestamp: item.youtube_url_at_timestamp(0.0).is_some(),
        receiver,
        accept: apricot_core::shortcut::ShortcutChord::parse(
            state
                .application
                .settings()
                .keyboard_shortcuts
                .get("open_selected")
                .map_or("Enter", String::as_str),
        ),
        back: apricot_core::shortcut::ShortcutChord::parse(
            state
                .application
                .settings()
                .keyboard_shortcuts
                .get("player_back")
                .map_or("Escape", String::as_str),
        ),
    };
    state.modal_open = true;
    let mut action = |action| {
        let Some(state) = self::state(window) else {
            return String::new();
        };
        if state.application.player_session().generation() != generation {
            return catalog.text("no_player").to_owned();
        }
        match action {
            crate::transcript_win32::TranscriptAction::Seek(entry) => {
                if seek_player_chapter(window, entry.start) {
                    let text = if entry.text.chars().count() > 90 {
                        format!(
                            "{}...",
                            entry.text.chars().take(87).collect::<String>().trim_end()
                        )
                    } else {
                        entry.text
                    };
                    catalog
                        .text("transcript_selected")
                        .replace("{time}", &format_duration(entry.start))
                        .replace("{text}", &text)
                } else {
                    catalog.text("timing_unavailable").to_owned()
                }
            }
            crate::transcript_win32::TranscriptAction::CopyTimestamp(entry) => {
                if let Some(url) = item.youtube_url_at_timestamp(entry.start) {
                    if crate::clipboard_win32::copy_text(window, url.as_str()).is_ok() {
                        catalog.text("timestamp_url_copied").to_owned()
                    } else {
                        catalog.text("copy_failed").to_owned()
                    }
                } else {
                    catalog.text("timestamp_url_unavailable").to_owned()
                }
            }
        }
    };
    let result = crate::transcript_win32::show(window, options, &mut action);
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
        if let Ok(Some(loaded)) = &result {
            state.application.cache_transcript(
                generation,
                apricot_app::transcript::CachedTranscript {
                    entries: loaded.entries.clone(),
                    source_key: loaded.source_key.to_owned(),
                },
            );
        }
    }
    resume_deferred_window_work(window);
    if let Err(error) = result {
        show_error_message(window, &error.to_string());
    }
    if let Some(state) = state_mut(window) {
        let _ = SetFocus(Some(active_primary_control(state)));
    }
}

unsafe fn show_player_lyrics(window: HWND) {
    stop_controlled_repeat(window);
    if !ensure_player_for_auxiliary_view(window) {
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(item) = state.application.player_session().current_item().cloned() else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let labels = crate::details_win32::DetailsDialogLabels {
        title: catalog.text("lyrics").to_owned(),
        copy: catalog.text("copy_lyrics").to_owned(),
        copied: catalog.text("lyrics_copied").to_owned(),
        back: catalog.text("back").to_owned(),
    };
    let local_source = catalog.text("lyrics_source_local").to_owned();
    let online_source = catalog.text("lyrics_source_online").to_owned();
    let loading = catalog.text("lyrics_fetching").to_owned();
    let unavailable = catalog.text("no_lyrics_available").to_owned();
    let online = state.application.settings().enable_online_lyrics;
    let proxy = state.application.settings().proxy.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use apricot_platform::lyrics::{LyricsQuery, LyricsSource, fetch_lyrics};
        let query = LyricsQuery::from_item(&item);
        let text = fetch_lyrics(
            item.local_path.as_deref().map(std::path::Path::new),
            &query,
            online,
            Some(&proxy),
        )
        .ok()
        .flatten()
        .map(|lyrics| {
            let source = match lyrics.source {
                LyricsSource::Local => &local_source,
                LyricsSource::Online => &online_source,
            };
            apricot_media::lyrics::LyricsDocument::parse(&lyrics.text, source)
        });
        let _ = sender.send(text);
    });
    let pending = crate::details_win32::AsyncText {
        position: state.playback.as_ref().map(|runtime| {
            (
                runtime.position_reader(),
                state.application.player_session().generation(),
            )
        }),
        receiver,
        unavailable,
        ready: labels.title.clone(),
    };
    state.modal_open = true;
    let result = crate::details_win32::show_async(window, loading, &labels, pending);
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    if let Err(error) = result {
        show_error_message(window, &error.to_string());
    }
    if let Some(state) = state_mut(window) {
        let _ = SetFocus(Some(active_primary_control(state)));
    }
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
        // Python `change_volume_async` adjusts volume without an announcement.
        state.application.set_player_volume(volume);
    }
}

fn speed_audio_mode(state: &WindowState) -> SpeedAudioMode {
    SpeedAudioMode::from_setting(&state.application.settings().speed_audio_mode)
}

fn pitch_mode(state: &WindowState) -> PitchMode {
    PitchMode::from_setting(&state.application.settings().pitch_mode)
}

/// Complete tagged `af` chain for the current session with optional
/// bass-boost and pitch overrides.
fn player_audio_filter(
    state: &WindowState,
    bass_boost_override: Option<bool>,
    pitch_override: Option<f64>,
) -> Option<String> {
    let pitch = pitch_override.unwrap_or_else(|| {
        state
            .application
            .player_session()
            .audio()
            .map_or(1.0, |audio| audio.pitch)
    });
    let equalizer = player_equalizer_filter(state, bass_boost_override);
    audio_filter_chain(
        speed_audio_mode(state),
        equalizer.as_deref(),
        pitch_mode(state),
        pitch,
    )
}

/// Python `apply_speed_target_worker`.
unsafe fn apply_player_speed(window: HWND, speed: f64) -> bool {
    let Some(correction) =
        state(window).map(|state| speed_audio_mode(state).audio_pitch_correction())
    else {
        return false;
    };
    if !execute_player_command(window, PlaybackCommand::SetAudioPitchCorrection(correction))
        || !execute_player_command(window, PlaybackCommand::SetSpeed(speed))
    {
        return false;
    }
    if let Some(state) = state_mut(window) {
        state.application.set_player_speed(speed);
        sync_player_details(state);
    }
    true
}

/// Python `apply_pitch_value`, including linked speed changes.
unsafe fn apply_player_pitch(window: HWND, pitch: f64, speed_delta: Option<f64>) -> bool {
    let Some((mode, filter)) = state(window).map(|state| {
        (
            pitch_mode(state),
            player_audio_filter(state, None, Some(pitch)),
        )
    }) else {
        return false;
    };
    if !execute_player_command(window, PlaybackCommand::SetAudioPitchCorrection(true))
        || !execute_player_command(
            window,
            PlaybackCommand::SetPitch(mpv_pitch_property(mode, pitch)),
        )
        || !execute_player_command(window, PlaybackCommand::SetAudioFilter(filter))
    {
        return false;
    }
    if let Some(state) = state_mut(window) {
        state.application.set_player_pitch(pitch);
        sync_player_details(state);
    }
    if mode == PitchMode::LinkedSpeed
        && let Some(delta) = speed_delta
        && let Some(current) = state(window)
            .and_then(|state| state.application.player_session().audio())
            .map(|audio| audio.speed)
    {
        let speed = clamp_rate(current + delta, 0.25, 4.0);
        if execute_player_command(window, PlaybackCommand::SetSpeed(speed))
            && let Some(state) = state_mut(window)
        {
            state.application.set_player_speed(speed);
            sync_player_details(state);
        }
    }
    true
}

unsafe fn adjust_player_speed(window: HWND, delta: f64) {
    let Some(audio) = state(window).and_then(|state| state.application.player_session().audio())
    else {
        return;
    };
    let speed = clamp_rate(audio.speed + delta, 0.25, 4.0);
    if !apply_player_speed(window, speed) {
        return;
    }
    announce_player_text(
        window,
        "speed_announcement",
        &[("speed", &format_rate_for_speech(speed))],
    );
    if is_default_rate(speed) {
        crate::sound_win32::play_default_reached_sound();
    }
}

unsafe fn adjust_player_pitch(window: HWND, delta: f64) {
    let Some(audio) = state(window).and_then(|state| state.application.player_session().audio())
    else {
        return;
    };
    let pitch = clamp_rate(audio.pitch + delta, 0.5, 2.0);
    if !apply_player_pitch(window, pitch, Some(pitch - audio.pitch)) {
        return;
    }
    announce_player_text(
        window,
        "pitch_announcement",
        &[("pitch", &format_rate_for_speech(pitch))],
    );
    if is_default_rate(pitch) {
        crate::sound_win32::play_default_reached_sound();
    }
}

/// Python `reset_speed_pitch_worker`.
unsafe fn reset_player_speed_pitch(window: HWND) {
    let speed = state(window).map_or(1.0, |state| {
        apricot_app::player_start_speed(&state.application.settings().player_speed)
    });
    if !apply_player_speed(window, speed) || !apply_player_pitch(window, 1.0, None) {
        return;
    }
    announce_player_text(
        window,
        "speed_pitch_reset",
        &[
            ("speed", &format_rate_for_speech(speed)),
            ("pitch", &format_rate_for_speech(1.0)),
        ],
    );
    crate::sound_win32::play_default_reached_sound();
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
            let filter = player_audio_filter(state, Some(enabled), None);
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
    let message = catalog_text(&state.application, session_toggle_key(toggle, enabled));
    set_status(state, &message, true);
    if state.view == MainView::Player {
        refresh_player(window, state, true, true);
    }
}

/// Python announcement keys for session toggles.
const fn session_toggle_key(toggle: SessionToggle, enabled: bool) -> &'static str {
    match (toggle, enabled) {
        (SessionToggle::AutoplayNext, true) => "autoplay_next_on",
        (SessionToggle::AutoplayNext, false) => "autoplay_next_off",
        (SessionToggle::BassBoost, true) => "bass_boost_on",
        (SessionToggle::BassBoost, false) => "bass_boost_off",
        (SessionToggle::VolumeBoost, true) => "volume_boost_on",
        (SessionToggle::VolumeBoost, false) => "volume_boost_off",
        (SessionToggle::Repeat, true) => "repeat_on",
        (SessionToggle::Repeat, false) => "repeat_off",
        (SessionToggle::Shuffle, true) => "shuffle_on",
        (SessionToggle::Shuffle, false) => "shuffle_off",
        (SessionToggle::Fullscreen, true) => "fullscreen_on",
        (SessionToggle::Fullscreen, false) => "fullscreen_off",
    }
}

/// Python `cycle_replaygain_mode`: Off, Track, Album, saved at once and
/// applied to the running player.
unsafe fn cycle_player_replaygain(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mode = match state.application.cycle_replaygain_mode() {
        Ok(mode) => mode,
        Err(error) => {
            let message = format!("Settings were not saved: {error}");
            set_status(state, &message, true);
            return;
        }
    };
    let applied = !state.application.player_session().is_open()
        || state.playback.as_ref().is_some_and(|runtime| {
            runtime
                .execute(
                    state.application.player_session().generation(),
                    PlaybackCommand::SetReplayGain(mode.clone()),
                )
                .is_ok()
        });
    let mode_label = catalog_text(
        &state.application,
        apricot_app::player_model::replaygain_mode_key(&mode),
    );
    let message = catalog_text(&state.application, "audio_normalization_changed")
        .replace("{mode}", &mode_label);
    set_status(state, &message, true);
    if !applied {
        let message = catalog_text(&state.application, "audio_normalization_restart_needed");
        set_status(state, &message, true);
    }
    if state.view == MainView::Player {
        refresh_player(window, state, false, true);
    }
}

/// Python `play_related_item` (manual) and the related branch of
/// `handle_player_eof`: loads the related videos of the current `YouTube`
/// video in the background.
unsafe fn play_related_video(window: HWND, manual: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let session = state.application.player_session();
    let current = session.is_open().then(|| session.current_item()).flatten();
    let Some(item) = current else {
        set_status(state, &catalog_text(&state.application, "no_player"), true);
        return;
    };
    let Some(video_id) = item.youtube_video_id() else {
        set_status(
            state,
            &catalog_text(&state.application, "no_related_video"),
            true,
        );
        return;
    };
    let generation = session.generation();
    if manual {
        let message = catalog_text(&state.application, "loading_related_video");
        set_status(state, &message, false);
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let url = format!("https://www.youtube.com/watch?v={video_id}");
        let result = apricot_platform::youtube_related::fetch_related_videos(&url)
            .map_err(|error| error.to_string());
        let _ = sender.send(result);
    });
    state.pending_related = Some(PendingRelatedVideos {
        generation,
        manual,
        receiver,
    });
    let _ = SetTimer(Some(window), RELATED_TIMER_ID, 50, None);
}

unsafe fn poll_related_videos(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(pending) = state.pending_related.as_ref() else {
        let _ = KillTimer(Some(window), RELATED_TIMER_ID);
        return;
    };
    // Python `apply_related_videos_and_play` ignores results for an older
    // player generation.
    if pending.generation != state.application.player_session().generation()
        || !state.application.player_session().is_open()
    {
        state.pending_related = None;
        let _ = KillTimer(Some(window), RELATED_TIMER_ID);
        return;
    }
    if state.modal_open {
        return;
    }
    let videos = match pending.receiver.try_recv() {
        Ok(result) => result.unwrap_or_default(),
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
    };
    let manual = pending.manual;
    state.pending_related = None;
    let _ = KillTimer(Some(window), RELATED_TIMER_ID);
    if let Some(item) = state.application.apply_related_videos(videos) {
        start_sequence_media_item(window, item, None);
        return;
    }
    // Python `play_next_standard_fallback`.
    if manual {
        set_status(
            state,
            &catalog_text(&state.application, "no_related_video"),
            true,
        );
    } else {
        navigate_player_relative_after_end(window);
    }
}

/// Python `toggle_player_fullscreen`. `focus_checkbox` keeps focus on the
/// Full screen checkbox; otherwise focus goes to the player.
unsafe fn toggle_player_fullscreen(window: HWND, focus_checkbox: bool, announce: bool) {
    let enabled = !state(window).is_some_and(|state| {
        state
            .application
            .player_session()
            .enabled_toggles()
            .contains(&SessionToggle::Fullscreen)
    });
    set_player_fullscreen(window, enabled, focus_checkbox, announce);
}

/// Python `enter_player_fullscreen` and `exit_fullscreen_to_player`.
unsafe fn set_player_fullscreen(window: HWND, enabled: bool, focus_checkbox: bool, announce: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if enabled && !state.application.player_session().is_open() {
        restore_player_checkbox(state, "fullscreen", SessionToggle::Fullscreen);
        set_status(state, &catalog_text(&state.application, "no_player"), true);
        return;
    }
    state
        .application
        .set_player_toggle(SessionToggle::Fullscreen, enabled);
    if state.view == MainView::Player {
        refresh_player(window, state, false, true);
        let target = if focus_checkbox {
            state.player_controls.window_for_id("fullscreen")
        } else {
            Some(state.player_controls.video_host())
        };
        if let Some(target) = target {
            let _ = SetFocus(Some(target));
        }
    }
    if announce {
        let key = if enabled {
            "fullscreen_on"
        } else {
            "fullscreen_off"
        };
        set_status(state, &catalog_text(&state.application, key), true);
    }
}

/// Python `ShowFullScreen` follows the player page: the window covers the
/// monitor while the player is shown in full screen mode. The change is
/// posted so that it never runs inside a layout pass.
unsafe fn sync_window_fullscreen(window: HWND, state: &WindowState) {
    if wants_window_fullscreen(state) != state.fullscreen_restore.is_some() {
        let _ = PostMessageW(Some(window), WM_SYNC_FULLSCREEN, WPARAM(0), LPARAM(0));
    }
}

fn wants_window_fullscreen(state: &WindowState) -> bool {
    let session = state.application.player_session();
    state.view == MainView::Player
        && session.is_open()
        && session
            .enabled_toggles()
            .contains(&SessionToggle::Fullscreen)
}

unsafe fn apply_window_fullscreen(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let wanted = wants_window_fullscreen(state);
    if wanted && state.fullscreen_restore.is_none() {
        let mut placement = WINDOWPLACEMENT {
            length: u32::try_from(std::mem::size_of::<WINDOWPLACEMENT>()).unwrap_or_default(),
            ..WINDOWPLACEMENT::default()
        };
        if GetWindowPlacement(window, &raw mut placement).is_err() {
            return;
        }
        let mut monitor = MONITORINFO {
            cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).unwrap_or_default(),
            ..MONITORINFO::default()
        };
        if !GetMonitorInfoW(
            MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
            &raw mut monitor,
        )
        .as_bool()
        {
            return;
        }
        let style = GetWindowLongPtrW(window, GWL_STYLE);
        state.fullscreen_restore = Some(FullscreenRestore { style, placement });
        let overlapped = isize::try_from(WS_OVERLAPPEDWINDOW.0).unwrap_or_default();
        let _ = SetWindowLongPtrW(window, GWL_STYLE, style & !overlapped);
        let bounds = monitor.rcMonitor;
        let _ = SetWindowPos(
            window,
            Some(HWND_TOP),
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
        );
    } else if !wanted && let Some(restore) = state.fullscreen_restore.take() {
        let _ = SetWindowLongPtrW(window, GWL_STYLE, restore.style);
        let _ = SetWindowPlacement(window, &raw const restore.placement);
        let _ = SetWindowPos(
            window,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
        );
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
    state.clip_preview = None;
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
    let session = state.application.player_session();
    let near_end = session.duration_seconds().is_some_and(|duration| {
        duration > 0.0 && session.position_seconds() >= (duration - 8.0).max(5.0)
    });
    if near_end {
        mark_current_podcast_episode_played(state);
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
    remember_menu_item(window, "play_file");
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
    remember_menu_item(window, "play_folder");
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
    let model = main_state.application.action_finder_model();
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
    remember_menu_item(window, "playback_queue");
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
        removed: catalog.text("playback_queue_removed").to_owned(),
        reordered: catalog.text("playback_queue_reordered").to_owned(),
        cleared: catalog.text("playback_queue_cleared").to_owned(),
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
                refresh_main_menu(main_state, MainMenuSelection::Preserve);
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

/// Speaks Python's message, or the beta message, for an action without a Rust
/// route. No window opens and focus stays where it is.
unsafe fn announce_unimplemented_action(window: HWND, action_id: &str) {
    let Some(state) = state(window) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let session = state.application.player_session();
    let current_item = session.is_open().then(|| session.current_item()).flatten();
    if let Some(message) =
        apricot_app::unavailable_action_message(&catalog, action_id, current_item)
    {
        set_status(state, &message, true);
    }
}

unsafe fn restore_player_checkbox(state: &WindowState, control_id: &str, toggle: SessionToggle) {
    let checked = state
        .application
        .player_session()
        .enabled_toggles()
        .contains(&toggle);
    state.player_controls.set_checked(control_id, checked);
}

/// Speaks the beta message for a Python feature without a Rust screen.
unsafe fn announce_unavailable_feature(window: HWND, feature: &str) {
    let Some(state) = state(window) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let message = apricot_app::unavailable_feature_message(&catalog, feature);
    set_status(state, &message, true);
}

unsafe fn open_settings(window: HWND) {
    remember_menu_item(window, "settings");
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
        MainView::MainMenu => refresh_main_menu(state, MainMenuSelection::LastActivated),
        MainView::Results => refresh_results(state, false),
        MainView::Trending => {
            refresh_trending_category_choices(state);
            refresh_results(state, false);
        }
        MainView::YoutubeCollection => refresh_youtube_collection(state, false),
        MainView::LocalFolder => refresh_local_folder(state, false, false),
        MainView::Favorites | MainView::History => {
            refresh_media_collection(state, false);
        }
        MainView::NotificationCenter => {
            refresh_notification_center(state, false, false, None);
        }
        MainView::Subscriptions => refresh_subscriptions(state, false, None),
        MainView::RssFeeds => refresh_rss_feeds(state, false, None),
        MainView::RssItems => refresh_rss_items(state, false, false),
        MainView::PodcastSearchResults => {
            refresh_podcast_directory_results(state, false, false);
        }
        MainView::PodcastCategories => refresh_podcast_categories(state, false),
        MainView::UserPlaylists => refresh_user_playlists(state, false),
        MainView::UserPlaylistItems => refresh_user_playlist_items(state, false, false),
        MainView::DownloadQueue => refresh_download_queue(state, false, false),
        MainView::Search | MainView::DirectLink => {}
        MainView::Player => refresh_player(window, state, false, true),
    }
    layout_controls_state(window, state);
    let _ = SetFocus(Some(active_primary_control(state)));
    configure_subscription_timer(window);
    check_subscriptions_if_due(window);
    configure_rss_timer(window);
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MainMenuSelection {
    /// Python `show_main_menu`: the last opened screen's item, else the first.
    LastActivated,
    /// Python `refresh_main_menu_download_label`: the same label, else the
    /// same row, so background updates never move the selection.
    Preserve,
}

unsafe fn refresh_main_menu(state: &mut WindowState, selection: MainMenuSelection) {
    set_open_button_label(state, "open");
    let old_selection = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok();
    let old_label = old_selection
        .and_then(|index| state.model.items.get(index))
        .map(|item| item.label.clone());
    state.model = state.application.main_menu_model();
    let labels: Vec<_> = state
        .model
        .items
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    let selected = match selection {
        MainMenuSelection::LastActivated => {
            main_menu_last_activated_index(&state.model.items, state.last_activated_menu_item)
        }
        MainMenuSelection::Preserve => {
            main_menu_preserved_index(&labels, old_label.as_deref(), old_selection)
        }
    };
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    crate::accessibility_win32::set_control_name(state.list, &state.model.accessible_name);
    for item in &state.model.items {
        let label = wide(&item.label);
        SendMessageW(
            state.list,
            LB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    }
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
}

fn main_menu_last_activated_index(
    items: &[apricot_app::MainMenuItem],
    last_activated: Option<&str>,
) -> usize {
    last_activated
        .and_then(|id| items.iter().position(|item| item.id == id))
        .unwrap_or_default()
}

fn main_menu_preserved_index(
    labels: &[&str],
    old_label: Option<&str>,
    old_selection: Option<usize>,
) -> usize {
    if let Some(index) = old_label.and_then(|old| labels.iter().position(|label| *label == old)) {
        return index;
    }
    old_selection
        .unwrap_or_default()
        .min(labels.len().saturating_sub(1))
}

fn active_primary_control(state: &WindowState) -> HWND {
    match state.view {
        MainView::Search | MainView::DirectLink => state.search_edit,
        MainView::MainMenu
        | MainView::Results
        | MainView::Trending
        | MainView::YoutubeCollection
        | MainView::LocalFolder
        | MainView::Favorites
        | MainView::History
        | MainView::NotificationCenter
        | MainView::Subscriptions
        | MainView::RssFeeds
        | MainView::RssItems
        | MainView::PodcastSearchResults
        | MainView::PodcastCategories
        | MainView::UserPlaylists
        | MainView::UserPlaylistItems
        | MainView::DownloadQueue => state.list,
        MainView::Player => state.player_controls.initial_focus(),
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        MainView, SEEK_HOLD_DELAY_MS, SEEK_HOLD_INTERVAL_MS, collection_backend,
        collection_download_url, controlled_repeat_timing, copy_wide_array,
        details_text_navigation_key, direct_link_with_scheme, download_folder_for_item,
        download_progress_presentation, item_needs_youtube_metadata, list_context_entries,
        main_menu_last_activated_index, main_menu_preserved_index, media_resolve_backend,
        normalized_audio_format, notification_label, queued_download_label, resolved_playback_item,
        result_label, safe_path_component, subscription_label, user_playlist_download_folder,
        view_has_back_button, view_has_collection_remove,
    };
    use apricot_app::{
        ActiveDownload, AppNotification, ContextMenuContext, DownloadChoice, DownloadTaskKind,
        DownloadTaskStatus, QueuedDownload, context_menu,
    };
    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_media::{YoutubeBackend, YoutubeCollectionKind};
    use apricot_storage::SettingsDocument;

    #[test]
    fn player_announcements_use_python_catalog_keys_and_formats() {
        use apricot_app::SessionToggle;
        let catalog = apricot_app::embedded_catalog("en");
        for (key, expected) in [
            ("timing_unavailable", "Timing is not available yet."),
            (
                "time_announcement",
                "Elapsed {elapsed}, remaining {remaining}, total {total}.",
            ),
            ("volume_announcement", "Volume: {volume}"),
            ("speed_announcement", "Playback speed {speed}x."),
            ("pitch_announcement", "Pitch {pitch}x."),
            (
                "speed_pitch_reset",
                "Speed reset to {speed}x and pitch reset to {pitch}x.",
            ),
            ("seeked_to_start", "Jumped to start."),
            ("seeked_to_end", "Jumped to end."),
            ("player_failed", "Player did not start: {error}"),
        ] {
            assert_eq!(catalog.text(key), expected);
        }
        for toggle in [
            SessionToggle::AutoplayNext,
            SessionToggle::BassBoost,
            SessionToggle::VolumeBoost,
            SessionToggle::Repeat,
            SessionToggle::Shuffle,
            SessionToggle::Fullscreen,
        ] {
            for enabled in [true, false] {
                let key = super::session_toggle_key(toggle, enabled);
                assert_ne!(catalog.text(key), key, "missing catalog text for {key}");
            }
        }
        assert_eq!(
            catalog.text(super::session_toggle_key(SessionToggle::Repeat, true)),
            "Repeat on."
        );
        assert_eq!(super::format_rate_for_speech(1.25), "1.25");
        assert_eq!(super::format_rate_for_speech(1.0), "1.00");
        assert!((super::clamp_rate(1.004 + 0.01, 0.25, 4.0) - 1.01).abs() < f64::EPSILON);
        assert!((super::clamp_rate(0.1, 0.25, 4.0) - 0.25).abs() < f64::EPSILON);
    }

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
    fn youtube_metadata_hydration_only_targets_incomplete_playable_rows() {
        let mut item = youtube_item("video");
        assert!(item_needs_youtube_metadata(&item));
        item.metadata.insert("view_count".to_owned(), 0_u64.into());
        item.metadata
            .insert("upload_date".to_owned(), "20260101".into());
        assert!(!item_needs_youtube_metadata(&item));

        item.kind = MediaKind::Playlist;
        item.metadata.clear();
        assert!(!item_needs_youtube_metadata(&item));
        item.kind = MediaKind::Video;
        item.source = MediaSource::Soundcloud;
        assert!(!item_needs_youtube_metadata(&item));
    }

    #[test]
    fn main_menu_selects_the_last_opened_screen_like_python() {
        let items: Vec<_> = ["search", "favorites", "settings", "exit"]
            .into_iter()
            .map(|id| apricot_app::MainMenuItem {
                id,
                label: id.to_owned(),
            })
            .collect();
        assert_eq!(main_menu_last_activated_index(&items, None), 0);
        assert_eq!(main_menu_last_activated_index(&items, Some("settings")), 2);
        assert_eq!(main_menu_last_activated_index(&items, Some("trending")), 0);
    }

    #[test]
    fn background_main_menu_refresh_keeps_the_selection_like_python() {
        let labels = ["Current downloads (1)", "Search", "Favorites", "Settings"];
        assert_eq!(
            main_menu_preserved_index(&labels, Some("Favorites"), Some(1)),
            2
        );
        assert_eq!(
            main_menu_preserved_index(&labels, Some("Current downloads (2)"), Some(0)),
            0
        );
        assert_eq!(
            main_menu_preserved_index(&labels[..2], Some("Gone"), Some(3)),
            1
        );
        assert_eq!(main_menu_preserved_index(&labels, None, None), 0);
    }

    #[test]
    fn notification_center_shows_back_without_a_python_incompatible_remove_button() {
        assert!(view_has_back_button(MainView::NotificationCenter));
        assert!(!view_has_collection_remove(MainView::NotificationCenter));
    }

    #[test]
    fn queued_podcast_download_label_has_source_mode_and_state_in_order() {
        let mut item = youtube_item("podcast");
        item.kind = MediaKind::PodcastEpisode;
        item.source = MediaSource::Podcast;
        item.title = "Episode One".to_owned();
        let queued = QueuedDownload {
            item,
            choice: DownloadChoice::Audio,
        };
        let catalog = apricot_app::embedded_catalog("en");

        assert_eq!(
            queued_download_label(&queued, &catalog),
            "Episode One | Podcast episode | podcast audio queued | Queued"
        );
    }

    #[test]
    fn download_paths_are_absolute_source_aware_and_windows_safe() {
        let mut settings = SettingsDocument {
            download_folder: r"C:\Downloads".to_owned(),
            ..SettingsDocument::default()
        };
        let mut podcast = youtube_item("podcast");
        podcast.kind = MediaKind::PodcastEpisode;
        podcast.source = MediaSource::Podcast;
        podcast.channel = "A:Podcast".to_owned();
        assert_eq!(
            download_folder_for_item(&settings, &podcast, false).expect("podcast folder"),
            std::path::PathBuf::from(r"C:\Downloads\podcasts\A_Podcast")
        );

        settings.download_folder = "relative".to_owned();
        assert!(download_folder_for_item(&settings, &podcast, false).is_err());
        assert_eq!(safe_path_component("CON.txt"), "_CON.txt");
        assert_eq!(safe_path_component("Lpt9"), "_Lpt9");
        assert_eq!(
            safe_path_component("name:with*invalid?chars"),
            "name_with_invalid_chars"
        );
        assert_eq!(
            safe_path_component(&format!("{}.tail", "a".repeat(119))),
            "a".repeat(119)
        );
    }

    #[test]
    fn channel_downloads_use_only_the_videos_tab() {
        let mut channel = youtube_item("channel");
        channel.kind = MediaKind::Channel;
        assert_eq!(
            collection_download_url(&channel, "https://youtube.com/@apricot/"),
            "https://youtube.com/@apricot/videos"
        );
        assert_eq!(
            collection_download_url(&channel, "https://youtube.com/@apricot/videos"),
            "https://youtube.com/@apricot/videos"
        );
    }

    #[test]
    fn unsupported_audio_format_uses_python_mp3_fallback() {
        assert_eq!(normalized_audio_format("FLAC"), "flac");
        assert_eq!(normalized_audio_format("aac"), "mp3");
    }

    #[test]
    fn subscriptions_use_python_list_semantics_and_context_order() {
        assert!(view_has_back_button(MainView::Subscriptions));
        assert!(view_has_collection_remove(MainView::Subscriptions));
        let labels = list_context_entries(MainView::Subscriptions)
            .expect("subscription menu")
            .into_iter()
            .map(|(_, label)| label)
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                "subscription_open_videos",
                "subscription_new_videos_button",
                "subscription_check_now",
                "set_category",
                "filter_category",
                "copy_url",
                "unsubscribe_channel",
                "remove",
            ]
        );
    }

    #[test]
    fn subscription_label_matches_python_field_order() {
        let mut subscription =
            apricot_app::Subscription::new("Channel", "https://www.youtube.com/@channel", 1.0);
        subscription.last_checked = None;
        subscription.category = "Music".to_owned();
        subscription.last_new_count = 2;
        let catalog = apricot_app::embedded_catalog("en");
        assert_eq!(
            subscription_label(&subscription, &catalog),
            "Channel | Category: Music | never checked | 2 new videos from Channel."
        );
    }

    #[test]
    fn youtube_result_label_matches_python_field_order_and_formatting() {
        let mut item = youtube_item("video");
        item.channel = "OpenAI".to_owned();
        item.duration_seconds = Some(65.0);
        item.metadata.insert("views".to_owned(), 37_000_000.into());
        item.metadata
            .insert("age".to_owned(), "Uploaded 2 days ago".into());
        let catalog = apricot_app::embedded_catalog("en");

        assert_eq!(
            result_label(&item, &catalog, &apricot_app::DownloadController::default()),
            "Video | Channel: OpenAI | Views: 37.0M | Uploaded 2 days ago | 1:05 | Video"
        );
    }

    #[test]
    fn a_repeated_download_shortcut_for_the_same_item_is_ignored_like_python() {
        let start = std::time::Instant::now();
        let press = |choice, identity: &str, millis| super::DownloadShortcutPress {
            choice,
            identity: identity.to_owned(),
            at: start + std::time::Duration::from_millis(millis),
        };
        let mut last = None;
        assert!(super::accept_download_shortcut(
            &mut last,
            press(DownloadChoice::Audio, "a", 0)
        ));
        assert!(!super::accept_download_shortcut(
            &mut last,
            press(DownloadChoice::Audio, "a", 200)
        ));
        assert!(super::accept_download_shortcut(
            &mut last,
            press(DownloadChoice::Video, "a", 250)
        ));
        assert!(super::accept_download_shortcut(
            &mut last,
            press(DownloadChoice::Video, "b", 300)
        ));
        assert!(super::accept_download_shortcut(
            &mut last,
            press(DownloadChoice::Video, "b", 700)
        ));
    }

    #[test]
    fn download_progress_reports_whole_percent_changes_or_every_three_quarters_second() {
        let start = std::time::Instant::now();
        let at = |millis| start + std::time::Duration::from_millis(millis);
        let mut last = super::DownloadProgressReport {
            percent_bucket: Some(10),
            title: "Song".to_owned(),
            at: start,
        };
        assert!(!super::should_report_download_progress(
            Some(&mut last),
            Some(10.4),
            "Song",
            at(100)
        ));
        assert!(super::should_report_download_progress(
            Some(&mut last),
            Some(11.0),
            "Song",
            at(150)
        ));
        assert!(!super::should_report_download_progress(
            Some(&mut last),
            Some(11.9),
            "Song",
            at(800)
        ));
        assert!(super::should_report_download_progress(
            Some(&mut last),
            Some(11.9),
            "Song",
            at(900)
        ));
        assert!(super::should_report_download_progress(
            Some(&mut last),
            Some(11.9),
            "Next",
            at(950)
        ));
        assert!(super::should_report_download_progress(
            None,
            None,
            "Song",
            at(960)
        ));
    }

    #[test]
    fn queued_episode_rows_end_with_the_podcast_marker() {
        let catalog = apricot_app::embedded_catalog("en");
        let mut episode = youtube_item("episode");
        episode.kind = apricot_core::MediaKind::PodcastEpisode;
        episode.title = "Episode".to_owned();
        assert_eq!(
            super::rss_episode_label(&episode, &catalog, None, false),
            "Episode | Podcast episode"
        );
        assert_eq!(
            super::rss_episode_label(&episode, &catalog, None, true),
            "Episode | Podcast episode | podcast audio queued"
        );
    }

    #[test]
    fn playlist_rows_show_the_video_count_and_queued_rows_their_marker() {
        let catalog = apricot_app::embedded_catalog("en");
        let mut playlist = youtube_item("list");
        playlist.kind = apricot_core::MediaKind::Playlist;
        playlist.title = "Mix".to_owned();
        playlist
            .metadata
            .insert("playlist_count".to_owned(), 42.into());
        let mut downloads = apricot_app::DownloadController::default();
        assert_eq!(
            result_label(&playlist, &catalog, &downloads),
            "Mix | Playlist | 42 videos"
        );
        playlist
            .metadata
            .insert("playlist_count".to_owned(), "1,204".into());
        assert_eq!(
            result_label(&playlist, &catalog, &downloads),
            "Mix | Playlist | 1204 videos"
        );
        downloads.queue_item(playlist.clone(), DownloadChoice::Audio);
        assert_eq!(
            result_label(&playlist, &catalog, &downloads),
            "Mix | Playlist | 1204 videos | collection audio queued"
        );
        let mut video = youtube_item("video");
        video.metadata.insert("views".to_owned(), 1.into());
        downloads.queue_item(video.clone(), DownloadChoice::Ask);
        assert!(result_label(&video, &catalog, &downloads).ends_with(" | Video | selected"));
    }

    #[test]
    fn youtube_collections_show_back_and_use_an_honest_backend_fallback() {
        assert!(view_has_back_button(MainView::YoutubeCollection));
        assert!(!view_has_collection_remove(MainView::YoutubeCollection));
        assert_eq!(
            collection_backend(
                YoutubeBackend::RustyYtdl,
                YoutubeCollectionKind::PlaylistVideos
            ),
            YoutubeBackend::RustyYtdl
        );
        assert_eq!(
            collection_backend(
                YoutubeBackend::RustyYtdl,
                YoutubeCollectionKind::ChannelVideos
            ),
            YoutubeBackend::YtDlp
        );
    }

    #[test]
    fn trending_is_a_result_list_with_python_compatible_navigation_and_actions() {
        assert!(view_has_back_button(MainView::Trending));
        assert!(!view_has_collection_remove(MainView::Trending));
        // Trending, results, collections and folders all use Python's
        // `open_context_menu`, modelled and tested in `apricot_app::context_menu`.
        for view in [
            MainView::Trending,
            MainView::Results,
            MainView::YoutubeCollection,
            MainView::LocalFolder,
            MainView::Favorites,
            MainView::History,
            MainView::UserPlaylists,
            MainView::UserPlaylistItems,
        ] {
            assert_eq!(list_context_entries(view), None, "{view:?}");
        }
    }

    #[test]
    fn download_progress_uses_aggregate_collection_progress_and_current_title() {
        let task = ActiveDownload {
            id: 7,
            item: youtube_item("video"),
            choice: DownloadChoice::Audio,
            kind: DownloadTaskKind::Batch,
            status: DownloadTaskStatus::Downloading,
            title: "Batch".to_owned(),
            current_title: "Track three".to_owned(),
            percent: Some(91.0),
            total: 10,
            completed: 3,
            item_failures: Vec::new(),
        };
        let catalog = apricot_app::embedded_catalog("en");
        let presentation = download_progress_presentation(&task, &catalog);

        assert_eq!(presentation.percent, 30);
        assert_eq!(
            presentation.message,
            "Track three\nCompleted: 3 of 10\nRemaining: 7"
        );
    }

    #[test]
    fn user_playlist_download_uses_one_safe_music_subfolder_and_exposes_the_action() {
        let settings = SettingsDocument {
            download_folder: r"C:\Downloads".to_owned(),
            ..SettingsDocument::default()
        };
        assert_eq!(
            user_playlist_download_folder(&settings, "Road: trip"),
            Ok(std::path::PathBuf::from(r"C:\Downloads\music\Road_ trip"))
        );
        let catalog = apricot_app::embedded_catalog("en");
        let titles = ["Road".to_owned()];
        let context = ContextMenuContext {
            catalog: &catalog,
            settings: &settings,
            user_playlist_titles: &titles,
            queued_download_count: 0,
        };
        let entries = context_menu::user_playlists_context_menu(&context);
        assert!(
            entries
                .iter()
                .any(|entry| entry.label() == catalog.text("download_user_playlist"))
        );
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

    #[test]
    fn direct_links_without_a_scheme_get_https_like_python() {
        assert_eq!(
            direct_link_with_scheme(" youtube.com/watch?v=x "),
            "https://youtube.com/watch?v=x"
        );
        assert_eq!(
            direct_link_with_scheme("HTTP://example.test/a"),
            "HTTP://example.test/a"
        );
        assert_eq!(
            direct_link_with_scheme("svn+ssh://host/x"),
            "svn+ssh://host/x"
        );
        assert_eq!(direct_link_with_scheme("1http://x"), "https://1http://x");
    }

    #[test]
    fn details_field_keeps_python_reading_keys() {
        use apricot_core::shortcut::ShortcutChord;
        for key in [
            "Up",
            "Ctrl+Left",
            "Shift+End",
            "PageDown",
            "Ctrl+C",
            "Ctrl+A",
        ] {
            assert!(
                details_text_navigation_key(ShortcutChord::parse(key).unwrap()),
                "{key}"
            );
        }
        for key in ["Space", "C", "Escape", "F7", "Ctrl+L"] {
            assert!(
                !details_text_navigation_key(ShortcutChord::parse(key).unwrap()),
                "{key}"
            );
        }
    }
}
