//! `AudioVault` screens of the main window, Python `AudioVaultMixin`: the menu,
//! search, recently added titles, TV show episodes, login, playback and
//! downloads. The network runs on worker threads that report back through a
//! channel polled on `AUDIOVAULT_TIMER_ID`.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

use apricot_app::audiovault::{self as vault, ArchiveMember, AudiovaultKind, AudiovaultMode};
use apricot_core::{MediaItem, Route, RouteFrame, TranslationCatalog};
use apricot_platform::{
    audiovault::{
        AUDIOVAULT_BASE_URL, AUDIOVAULT_REGISTER_URL, AudiovaultClient, AudiovaultError,
        RemoteMemberRequest, ShowProgress, episode_files, is_audio_file, natural_sort_key,
        show_cache_is_complete,
    },
    audiovault_credentials::{protect_password, unprotect_password},
};
use serde_json::Value;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
        UI::{
            Input::KeyboardAndMouse::{GetFocus, SetFocus},
            WindowsAndMessaging::{
                CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL, CBS_DROPDOWNLIST,
                ES_AUTOHSCROLL, GetWindowTextLengthW, GetWindowTextW, KillTimer, LB_GETCURSEL,
                LB_RESETCONTENT, LB_SETCURSEL, MoveWindow, SW_HIDE, SW_SHOW, SendMessageW,
                SetTimer, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WS_CHILD,
                WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

use super::{
    MainView, WindowState, add_list_string, cancel_local_folder_scan, cancel_youtube_work,
    create_control, layout_button_row, layout_controls_state, remember_menu_item,
    restore_from_tray, set_status, show_error_message, show_main_menu, start_media_item,
    start_sequence_media_item, state, state_mut, stop_controlled_repeat, wide,
};

pub(super) const AUDIOVAULT_TIMER_ID: usize = 19;
const AUDIOVAULT_TIMER_INTERVAL_MS: u32 = 50;
const ID_AUDIOVAULT_QUERY: usize = 1060;
const ID_AUDIOVAULT_TYPE: usize = 1061;

/// Python `audiovault_view`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum View {
    Menu,
    Search,
    Recent(AudiovaultMode),
    Episodes,
}

/// Python `audiovault_parent_results`, `_view` and `_title`.
#[derive(Clone)]
struct Parent {
    results: Vec<MediaItem>,
    view: View,
    title: String,
}

/// Python `audiovault_player_return_state`.
#[derive(Clone)]
struct Snapshot {
    mode: AudiovaultMode,
    results: Vec<MediaItem>,
    view: View,
    title: String,
    parent: Option<Parent>,
}

/// What runs once a login succeeds, Python's `after_login` callbacks.
#[derive(Clone)]
enum AfterLogin {
    Nothing,
    Menu,
    Search,
    Recent(AudiovaultMode),
    SearchQuery(String, AudiovaultMode),
    PlayRemote(MediaItem),
    PrepareShow {
        item: MediaItem,
        download_after: bool,
    },
    PrepareEpisode {
        item: MediaItem,
        download_after: bool,
    },
    Download(MediaItem),
}

enum Event {
    Login {
        email: String,
        remember: bool,
        after: AfterLogin,
        result: std::result::Result<Option<String>, AudiovaultError>,
    },
    Results {
        view: View,
        retry: Option<AfterLogin>,
        result: std::result::Result<Vec<vault::VaultRecord>, AudiovaultError>,
        mode: AudiovaultMode,
    },
    Manifest {
        show: MediaItem,
        generation: u64,
        allow_retry: bool,
        result: std::result::Result<
            (u64, Vec<apricot_platform::zip_archive::ZipEntry>),
            AudiovaultError,
        >,
    },
    Progress {
        task: u64,
        message_key: &'static str,
        title: String,
        percent: u64,
    },
    ShowDone {
        show: MediaItem,
        download_after: bool,
        allow_retry: bool,
        task: u64,
        generation: u64,
        result: std::result::Result<(), AudiovaultError>,
    },
    EpisodeDone {
        item: MediaItem,
        download_after: bool,
        allow_retry: bool,
        task: u64,
        result: std::result::Result<(), AudiovaultError>,
    },
    Stream {
        item: MediaItem,
        allow_retry: bool,
        result: std::result::Result<ResolvedStream, AudiovaultError>,
    },
    Movie {
        item: MediaItem,
        allow_retry: bool,
        result: std::result::Result<PathBuf, AudiovaultError>,
    },
}

/// Python `resolve_audiovault_stream` without the open response.
struct ResolvedStream {
    final_url: String,
    content_type: String,
    disposition: String,
    headers: Vec<(String, String)>,
}

/// The controls only the `AudioVault` search and result screens have.
pub(super) struct Controls {
    query_label: HWND,
    query: HWND,
    type_label: HWND,
    kind: HWND,
    title: HWND,
}

impl Controls {
    pub(super) fn windows(&self) -> [HWND; 5] {
        [
            self.query_label,
            self.query,
            self.type_label,
            self.kind,
            self.title,
        ]
    }
}

pub(super) struct AudiovaultState {
    client: Option<Arc<AudiovaultClient>>,
    /// Python `audiovault_logged_in`, for this address.
    logged_in_email: Option<String>,
    mode: AudiovaultMode,
    view: View,
    title: String,
    results: Vec<MediaItem>,
    parent: Option<Parent>,
    manifests: HashMap<String, Vec<MediaItem>>,
    manifest_loading: HashSet<String>,
    episode_loading: HashSet<String>,
    progress_task: u64,
    progress_generation: u64,
    show_generation: u64,
    player_return: Option<Snapshot>,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    running: usize,
    /// A message box shown for one event must not handle the next one.
    polling: bool,
}

impl Default for AudiovaultState {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            client: None,
            logged_in_email: None,
            mode: AudiovaultMode::Movies,
            view: View::Menu,
            title: String::new(),
            results: Vec::new(),
            parent: None,
            manifests: HashMap::new(),
            manifest_loading: HashSet::new(),
            episode_loading: HashSet::new(),
            progress_task: 0,
            progress_generation: 0,
            show_generation: 0,
            player_return: None,
            sender,
            receiver,
            running: 0,
            polling: false,
        }
    }
}

pub(super) const fn is_view(view: MainView) -> bool {
    matches!(
        view,
        MainView::AudiovaultMenu | MainView::AudiovaultSearch | MainView::AudiovaultResults
    )
}

pub(super) unsafe fn create_controls(
    parent: HWND,
    instance: HINSTANCE,
    catalog: &TranslationCatalog,
) -> Result<Controls> {
    let label = |key: &str| wide(catalog.text(key));
    let query_text = label("search_query");
    let query_label = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(query_text.as_ptr()),
        WS_CHILD,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let query = create_control(
        parent,
        instance,
        w!("EDIT"),
        PCWSTR::null(),
        WS_CHILD | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        WS_EX_CLIENTEDGE,
        ID_AUDIOVAULT_QUERY,
    )?;
    crate::accessibility_win32::annotate_control_name(query, catalog.text("search_query"));
    let type_text = label("type");
    let type_label = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(type_text.as_ptr()),
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
        ID_AUDIOVAULT_TYPE,
    )?;
    crate::accessibility_win32::annotate_control_name(kind, catalog.text("type"));
    let title = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR::null(),
        WS_CHILD,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    Ok(Controls {
        query_label,
        query,
        type_label,
        kind,
        title,
    })
}

fn catalog(state: &WindowState) -> TranslationCatalog {
    apricot_app::embedded_catalog(&state.application.settings().language)
}

fn text(state: &WindowState, key: &str) -> String {
    catalog(state).text(key).to_owned()
}

/// Python `friendly_error(exc)` of an `AudioVault` failure.
fn error_text(catalog: &TranslationCatalog, error: &AudiovaultError) -> String {
    let text = error
        .text_key()
        .map_or_else(|| error.to_string(), |key| catalog.text(key).to_owned());
    apricot_app::comments::friendly_error(catalog, &text)
}

fn keyed(state: &WindowState, key: &str, replacements: &[(&str, &str)]) -> String {
    let mut message = text(state, key);
    for (name, value) in replacements {
        message = message.replace(&format!("{{{name}}}"), value);
    }
    message
}

unsafe fn announce(window: HWND, key: &str, replacements: &[(&str, &str)]) {
    if let Some(state) = state(window) {
        set_status(state, &keyed(state, key, replacements), true);
    }
}

unsafe fn message(window: HWND, key: &str, error: &AudiovaultError) {
    let Some(state) = state(window) else {
        return;
    };
    let catalog = catalog(state);
    let text = catalog
        .text(key)
        .replace("{error}", &error_text(&catalog, error));
    show_error_message(window, &text);
}

fn client(state: &mut WindowState) -> Option<Arc<AudiovaultClient>> {
    if state.audiovault.client.is_none() {
        state.audiovault.client = AudiovaultClient::new(env!("CARGO_PKG_VERSION"))
            .ok()
            .map(Arc::new);
    }
    state.audiovault.client.clone()
}

/// Runs `work` on a worker thread; its event comes back on the timer.
unsafe fn spawn(
    window: HWND,
    work: impl FnOnce(&AudiovaultClient, &Sender<Event>) -> Option<Event> + Send + 'static,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(client) = client(state) else {
        return;
    };
    let sender = state.audiovault.sender.clone();
    state.audiovault.running += 1;
    let _ = SetTimer(
        Some(window),
        AUDIOVAULT_TIMER_ID,
        AUDIOVAULT_TIMER_INTERVAL_MS,
        None,
    );
    thread::spawn(move || {
        if let Some(event) = work(&client, &sender) {
            let _ = sender.send(event);
        }
    });
}

// ---------------------------------------------------------------------------
// Login

fn settings_email(state: &WindowState) -> String {
    state
        .application
        .settings()
        .audiovault_email
        .trim()
        .to_owned()
}

/// Python `audiovault_logged_in`. A changed address in the settings logs
/// out, as Python's `apply_settings_from_visible_controls` does.
fn logged_in(state: &WindowState) -> bool {
    state
        .audiovault
        .logged_in_email
        .as_deref()
        .is_some_and(|email| email == state.application.settings().audiovault_email)
}

/// Python `ensure_audiovault_login`.
unsafe fn ensure_login(window: HWND, after: AfterLogin) -> bool {
    let Some(state) = state_mut(window) else {
        return false;
    };
    if logged_in(state) {
        return true;
    }
    if state.audiovault.logged_in_email.take().is_some()
        && let Some(client) = &state.audiovault.client
    {
        client.clear_cookies();
    }
    let email = settings_email(state);
    let password = unprotect_password(&state.application.settings().audiovault_password_protected);
    if !email.is_empty() && !password.is_empty() {
        set_status(state, &text(state, "audiovault_logging_in"), true);
        start_login(window, email, password, after, false);
    } else {
        show_login(window, after, None);
    }
    false
}

/// Python `retry_audiovault_after_login`.
unsafe fn retry_after_login(window: HWND, after: AfterLogin) {
    if let Some(state) = state_mut(window) {
        state.audiovault.logged_in_email = None;
        if let Some(client) = &state.audiovault.client {
            client.clear_cookies();
        }
    }
    ensure_login(window, after);
}

/// Python `show_audiovault_login`.
unsafe fn show_login(window: HWND, after: AfterLogin, owner: Option<HWND>) -> bool {
    let Some(state) = state_mut(window) else {
        return false;
    };
    let catalog = catalog(state);
    let email = state.application.settings().audiovault_email.clone();
    state.modal_open = true;
    let texts = crate::audiovault_login_win32::LoginTexts {
        title: catalog.text("audiovault_login"),
        email: catalog.text("email"),
        password: catalog.text("password"),
        register: catalog.text("register"),
        ok: catalog.text("ok"),
        cancel: catalog.text("cancel"),
    };
    let response = crate::audiovault_login_win32::show(
        owner.unwrap_or(window),
        &texts,
        &email,
        Box::new(open_registration),
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    super::resume_deferred_window_work(window);
    let Ok(Some((address, secret))) = response else {
        return false;
    };
    let address = address.trim().to_owned();
    if address.is_empty() || secret.is_empty() {
        if let Some(state) = super::state(window) {
            show_error_message(window, &text(state, "audiovault_credentials_required"));
        }
        return false;
    }
    announce(window, "audiovault_logging_in", &[]);
    start_login(window, address, secret, after, true);
    true
}

unsafe fn start_login(
    window: HWND,
    email: String,
    password: String,
    after: AfterLogin,
    remember: bool,
) {
    spawn(window, move |client, _| {
        let result = client.login_page().and_then(|page| {
            let token = vault::parse_vault_page(&page).token;
            if token.is_empty() {
                return Err(AudiovaultError::LoginPage);
            }
            client.submit_login(&token, &email, &password)?;
            if remember {
                protect_password(&password)
                    .map(Some)
                    .map_err(AudiovaultError::Message)
            } else {
                Ok(None)
            }
        });
        Some(Event::Login {
            email,
            remember,
            after,
            result,
        })
    });
}

/// Python `open_audiovault_registration`.
pub(super) fn open_registration() {
    let _ = std::process::Command::new("explorer.exe")
        .arg(AUDIOVAULT_REGISTER_URL)
        .spawn();
}

unsafe fn finish_login(
    window: HWND,
    email: &str,
    remember: bool,
    after: AfterLogin,
    result: std::result::Result<Option<String>, AudiovaultError>,
) {
    match result {
        Ok(protected) => {
            let Some(state) = state_mut(window) else {
                return;
            };
            state.audiovault.logged_in_email = Some(email.to_owned());
            let saved = state
                .application
                .store_audiovault_login(email, protected.filter(|_| remember));
            if let Err(error) = saved {
                show_error_message(window, &error.to_string());
            }
            announce(window, "audiovault_logged_in", &[]);
            run_after_login(window, after);
        }
        Err(error) => {
            let Some(state) = state_mut(window) else {
                return;
            };
            state.audiovault.logged_in_email = None;
            let retry_interactively = !remember;
            if retry_interactively {
                let _ = state.application.forget_audiovault_login(false);
            }
            let catalog = catalog(state);
            let text = catalog
                .text("audiovault_login_error")
                .replace("{error}", &error_text(&catalog, &error));
            show_error_message(window, &text);
            if retry_interactively {
                show_login(window, after, None);
            }
        }
    }
}

unsafe fn run_after_login(window: HWND, after: AfterLogin) {
    match after {
        AfterLogin::Nothing => {}
        AfterLogin::Menu => show_menu(window),
        AfterLogin::Search => show_search(window),
        AfterLogin::Recent(mode) => show_recent(window, mode, false),
        AfterLogin::SearchQuery(query, mode) => start_search(window, query, mode, false),
        AfterLogin::PlayRemote(item) => play_remote(window, item, false),
        AfterLogin::PrepareShow {
            item,
            download_after,
        } => prepare_show(window, item, download_after, false),
        AfterLogin::PrepareEpisode {
            item,
            download_after,
        } => prepare_remote_episode(window, item, download_after, false),
        AfterLogin::Download(item) => download_item(window, item, false),
    }
}

/// Settings: Python `login_audiovault_from_settings` and `logout_audiovault`.
pub(super) unsafe fn settings_request(window: HWND, request: usize, owner: HWND) {
    if request == super::AUDIOVAULT_REQUEST_LOGIN {
        let owner = (!owner.is_invalid()).then_some(owner);
        show_login(window, AfterLogin::Nothing, owner);
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    if let Some(client) = &state.audiovault.client {
        client.clear_cookies();
    }
    state.audiovault.logged_in_email = None;
    if let Err(error) = state.application.forget_audiovault_login(true) {
        show_error_message(window, &error.to_string());
        return;
    }
    announce(window, "audiovault_logged_out", &[]);
}

// ---------------------------------------------------------------------------
// Screens

/// Python `prepare_audiovault_screen`.
unsafe fn prepare_screen(window: HWND, view: View, main_view: MainView, route: Route) {
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
    state.audiovault.view = view;
    state.view = main_view;
}

/// Python `show_audiovault_menu`.
pub(super) unsafe fn show_menu(window: HWND) {
    if !ensure_login(window, AfterLogin::Menu) {
        return;
    }
    remember_menu_item(window, "audiovault");
    if let Some(state) = state_mut(window) {
        state.audiovault.parent = None;
    }
    prepare_screen(
        window,
        View::Menu,
        MainView::AudiovaultMenu,
        Route::AudiovaultMenu,
    );
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = catalog(state);
    super::set_open_button_label(state, "open");
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    crate::accessibility_win32::set_control_name(state.list, catalog.text("audiovault"));
    for key in [
        "search",
        "audiovault_recent_tv_shows",
        "audiovault_recent_movies",
    ] {
        add_list_string(state.list, catalog.text(key));
    }
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
    layout_controls_state(window, state);
    let _ = SetFocus(Some(state.list));
}

/// Python `activate_audiovault_menu_item`.
unsafe fn activate_menu_item(window: HWND) {
    let Some(index) = state(window).and_then(|state| {
        usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()
    }) else {
        return;
    };
    match index {
        0 => show_search(window),
        1 => show_recent(window, AudiovaultMode::Shows, true),
        2 => show_recent(window, AudiovaultMode::Movies, true),
        _ => {}
    }
}

/// The result list as `add_audiovault_results_list` creates it.
/// The placeholder row stands for no result, so Play and Download do
/// nothing until results arrive; Python kept the previous screen's results.
unsafe fn reset_result_list(state: &mut WindowState, name: &str) {
    state.audiovault.results.clear();
    let catalog = catalog(state);
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    crate::accessibility_win32::set_control_name(state.list, name);
    add_list_string(state.list, catalog.text("search_results_empty"));
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
}

/// Python `show_audiovault_search`.
pub(super) unsafe fn show_search(window: HWND) {
    if !ensure_login(window, AfterLogin::Search) {
        return;
    }
    // Python remembers `show_audiovault_search`, which is no main menu item.
    remember_menu_item(window, "audiovault_search");
    prepare_screen(
        window,
        View::Search,
        MainView::AudiovaultSearch,
        Route::AudiovaultSearch,
    );
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = catalog(state);
    catalog
        .text("search")
        .clone_into(&mut state.audiovault.title);
    let controls = &state.audiovault_controls;
    let _ = SetWindowTextW(controls.query, w!(""));
    SendMessageW(controls.kind, CB_RESETCONTENT, None, None);
    for key in ["movies", "tv_shows"] {
        let label = wide(catalog.text(key));
        SendMessageW(
            controls.kind,
            CB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    }
    let selection = usize::from(state.audiovault.mode == AudiovaultMode::Shows);
    SendMessageW(controls.kind, CB_SETCURSEL, Some(WPARAM(selection)), None);
    reset_result_list(state, catalog.text("result_list"));
    layout_controls_state(window, state);
    let _ = SetFocus(Some(state.audiovault_controls.query));
}

/// Python `show_audiovault_results_screen`.
unsafe fn show_results_screen(window: HWND, title: &str, view: View) {
    let route = if view == View::Episodes {
        Route::AudiovaultEpisodes
    } else {
        Route::AudiovaultResults
    };
    prepare_screen(window, view, MainView::AudiovaultResults, route);
    let Some(state) = state_mut(window) else {
        return;
    };
    title.clone_into(&mut state.audiovault.title);
    let title_text = wide(title);
    let _ = SetWindowTextW(state.audiovault_controls.title, PCWSTR(title_text.as_ptr()));
    reset_result_list(state, title);
    layout_controls_state(window, state);
    if GetFocus() != state.list {
        let _ = SetFocus(Some(state.list));
    }
}

/// Python `show_audiovault_recent`.
unsafe fn show_recent(window: HWND, mode: AudiovaultMode, allow_retry: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    state.audiovault.mode = mode;
    let title = text(state, mode.recent_title_key());
    show_results_screen(window, &title, View::Recent(mode));
    announce(window, "audiovault_loading_recent", &[]);
    spawn(window, move |client, _| {
        let result = client
            .fetch_page(&format!("{AUDIOVAULT_BASE_URL}/"))
            .map(|page| vault::parse_vault_page(&page).records);
        Some(Event::Results {
            view: View::Recent(mode),
            retry: allow_retry.then_some(AfterLogin::Recent(mode)),
            result,
            mode,
        })
    });
}

/// Python `back_from_audiovault`.
pub(super) unsafe fn back(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    state.audiovault.show_generation += 1;
    if let Some(parent) = state
        .audiovault
        .parent
        .take()
        .filter(|parent| !parent.results.is_empty())
    {
        if parent.view == View::Search {
            show_search(window);
        } else {
            show_results_screen(window, &parent.title, parent.view);
        }
        show_results(window, parent.results);
        return;
    }
    if state.audiovault.view == View::Menu {
        show_main_menu(window);
    } else {
        show_menu(window);
    }
}

/// Python `search_audiovault`.
pub(super) unsafe fn search(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let query = window_text(state.audiovault_controls.query)
        .trim()
        .to_owned();
    if query.is_empty() {
        show_error_message(window, &text(state, "enter_query"));
        return;
    }
    let selected = SendMessageW(state.audiovault_controls.kind, CB_GETCURSEL, None, None).0;
    let mode = if selected == 0 {
        AudiovaultMode::Movies
    } else {
        AudiovaultMode::Shows
    };
    state.audiovault.mode = mode;
    announce(window, "searching", &[("query", &query)]);
    start_search(window, query, mode, true);
}

unsafe fn start_search(window: HWND, query: String, mode: AudiovaultMode, allow_retry: bool) {
    spawn(window, move |client, _| {
        let result = client
            .fetch_page(&vault::catalog_url(mode, &query))
            .map(|page| vault::parse_vault_page(&page).records);
        Some(Event::Results {
            view: View::Search,
            retry: allow_retry.then_some(AfterLogin::SearchQuery(query, mode)),
            result,
            mode,
        })
    });
}

/// Python `show_audiovault_results`.
unsafe fn show_results(window: HWND, results: Vec<MediaItem>) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = catalog(state);
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    if results.is_empty() {
        add_list_string(state.list, catalog.text("no_results"));
    }
    for item in &results {
        add_list_string(state.list, &vault::result_line(item, &catalog));
    }
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
    let found = catalog
        .text("found")
        .replace("{count}", &results.len().to_string());
    state.audiovault.results = results;
    set_status(state, &found, true);
    if GetFocus() != state.list {
        let _ = SetFocus(Some(state.list));
    }
}

/// Python `selected_audiovault_item`.
pub(super) unsafe fn selected_item(state: &WindowState) -> Option<MediaItem> {
    if !matches!(
        state.view,
        MainView::AudiovaultSearch | MainView::AudiovaultResults
    ) {
        return None;
    }
    let index = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
    state.audiovault.results.get(index).cloned()
}

/// Enter, Open and Play on an `AudioVault` screen.
pub(super) unsafe fn activate(window: HWND) {
    if state(window).is_some_and(|state| state.view == MainView::AudiovaultMenu) {
        activate_menu_item(window);
    } else {
        activate_item(window);
    }
}

/// Python `activate_audiovault_item`.
unsafe fn activate_item(window: HWND) {
    let Some(item) = state(window).and_then(|state| selected_item(state)) else {
        return;
    };
    match vault::item_kind(&item) {
        Some(AudiovaultKind::Show) => prepare_show(window, item, false, true),
        Some(AudiovaultKind::RemoteEpisode) => prepare_remote_episode(window, item, false, true),
        Some(AudiovaultKind::Episode) => play_local(window, item),
        _ => play_remote(window, item, true),
    }
}

/// Button commands of the `AudioVault` screens.
pub(super) unsafe fn handle_command(window: HWND, command: usize) -> bool {
    if !state(window).is_some_and(|state| is_view(state.view)) {
        return false;
    }
    match command {
        super::ID_BACK => back(window),
        super::ID_OPEN | super::ID_SEARCH_PLAY => activate(window),
        super::ID_SEARCH => search(window),
        super::ID_SEARCH_DOWNLOAD_AUDIO => download_selected(window),
        _ => return false,
    }
    true
}

/// Python Enter in the query field and on the type choice.
pub(super) unsafe fn handles_enter(state: &WindowState, target: HWND) -> bool {
    state.view == MainView::AudiovaultSearch
        && (target == state.audiovault_controls.query || target == state.audiovault_controls.kind)
}

// ---------------------------------------------------------------------------
// TV shows

/// Python `prepare_audiovault_show`.
unsafe fn prepare_show(window: HWND, item: MediaItem, download_after: bool, allow_retry: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = catalog(state);
    let cache_dir = vault::show_cache_dir(&state.application.settings().cache_folder, &item);
    let episodes = vault::local_episode_items(&item, &episode_files(&cache_dir), &catalog);
    if !episodes.is_empty() && show_cache_is_complete(&cache_dir) {
        if download_after {
            copy_show_to_downloads(window, &item, &cache_dir);
        } else {
            show_episodes(window, &item, episodes);
        }
        return;
    }
    if download_after {
        start_full_show_job(window, item, cache_dir, true, allow_retry, 0);
        return;
    }
    let key = vault::show_key(&item);
    if let Some(cached) = state
        .audiovault
        .manifests
        .get(&key)
        .filter(|cached| !cached.is_empty())
        .cloned()
    {
        show_episodes(window, &item, cached);
        return;
    }
    let loading = keyed(
        state,
        "audiovault_loading_episodes",
        &[("title", &item.title)],
    );
    set_status(state, &loading, true);
    if state.audiovault.manifest_loading.contains(&key) {
        return;
    }
    state.audiovault.manifest_loading.insert(key);
    state.audiovault.show_generation += 1;
    let generation = state.audiovault.show_generation;
    spawn(window, move |client, _| {
        let url = item
            .url
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let result = client.remote_archive_entries(&url);
        Some(Event::Manifest {
            show: item,
            generation,
            allow_retry,
            result,
        })
    });
}

/// Python `load_audiovault_show_manifest_worker` back on the window thread.
unsafe fn finish_manifest(
    window: HWND,
    show: MediaItem,
    generation: u64,
    allow_retry: bool,
    result: std::result::Result<
        (u64, Vec<apricot_platform::zip_archive::ZipEntry>),
        AudiovaultError,
    >,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let key = vault::show_key(&show);
    state.audiovault.manifest_loading.remove(&key);
    let catalog = catalog(state);
    let cache_dir = vault::show_cache_dir(&state.application.settings().cache_folder, &show);
    match result {
        Ok((archive_size, entries)) => {
            let mut audio = entries
                .iter()
                .filter(|entry| !entry.is_dir() && is_audio_file(Path::new(&entry.name)))
                .collect::<Vec<_>>();
            audio.sort_by_cached_key(|entry| natural_sort_key(&entry.name));
            let members = audio
                .iter()
                .map(|entry| ArchiveMember {
                    name: &entry.name,
                    crc: entry.crc,
                    file_size: entry.file_size,
                    compress_size: entry.compress_size,
                })
                .collect::<Vec<_>>();
            let url = show
                .url
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();
            let episodes = vault::remote_episode_items(
                &show,
                &cache_dir,
                &url,
                archive_size,
                &members,
                &catalog,
            );
            if episodes.is_empty() {
                message(
                    window,
                    "audiovault_show_failed",
                    &AudiovaultError::NoEpisodes,
                );
                return;
            }
            state.audiovault.manifests.insert(key, episodes.clone());
            show_episodes_if_current(window, generation, &show, episodes);
        }
        Err(AudiovaultError::RangeUnsupported) => {
            start_full_show_job(window, show, cache_dir, false, allow_retry, generation);
        }
        Err(AudiovaultError::SessionExpired) if allow_retry => {
            retry_after_login(
                window,
                AfterLogin::PrepareShow {
                    item: show,
                    download_after: false,
                },
            );
        }
        Err(error) => message(window, "audiovault_show_failed", &error),
    }
}

/// Python `show_audiovault_episodes_if_current`.
unsafe fn show_episodes_if_current(
    window: HWND,
    generation: u64,
    show: &MediaItem,
    episodes: Vec<MediaItem>,
) {
    let Some(state) = state(window) else {
        return;
    };
    if generation != 0 && generation != state.audiovault.show_generation {
        return;
    }
    if !is_view(state.view) {
        return;
    }
    show_episodes(window, show, episodes);
}

/// Python `show_audiovault_episodes`: the episodes replace the list on the
/// same screen, which keeps its fields and title.
unsafe fn show_episodes(window: HWND, show: &MediaItem, episodes: Vec<MediaItem>) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = catalog(state);
    let audiovault = &mut state.audiovault;
    audiovault.parent = Some(Parent {
        results: audiovault.results.clone(),
        view: audiovault.view,
        title: audiovault.title.clone(),
    });
    audiovault.view = View::Episodes;
    audiovault.title = if show.title.is_empty() {
        catalog.text("tv_show").to_owned()
    } else {
        show.title.clone()
    };
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    for item in &episodes {
        add_list_string(state.list, &vault::result_line(item, &catalog));
    }
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
    let loaded = catalog
        .text("audiovault_episodes_loaded")
        .replace("{count}", &episodes.len().to_string())
        .replace("{title}", &show.title);
    state.audiovault.results = episodes;
    set_status(state, &loaded, true);
    if GetFocus() != state.list {
        let _ = SetFocus(Some(state.list));
    }
}

/// Python `next_audiovault_progress_task_id` and `show_audiovault_progress`.
unsafe fn start_progress(window: HWND, title: &str) -> u64 {
    let Some(state) = state_mut(window) else {
        return 0;
    };
    state.audiovault.progress_generation += 1;
    let task = state.audiovault.progress_generation;
    state.audiovault.progress_task = task;
    let message = keyed(
        state,
        "audiovault_progress_downloading",
        &[("title", title), ("percent", "0")],
    );
    set_status(state, &message, true);
    task
}

/// Python `close_audiovault_progress`.
fn close_progress(state: &mut WindowState, task: u64) {
    if task != 0 && state.audiovault.progress_task == task {
        state.audiovault.progress_task = 0;
    }
}

/// Python `start_audiovault_full_show_job`.
unsafe fn start_full_show_job(
    window: HWND,
    show: MediaItem,
    cache_dir: PathBuf,
    download_after: bool,
    allow_retry: bool,
    generation: u64,
) {
    if generation != 0
        && state(window).is_none_or(|state| {
            generation != state.audiovault.show_generation || !is_view(state.view)
        })
    {
        return;
    }
    let task = start_progress(window, &show.title);
    spawn(window, move |client, sender| {
        let url = show
            .url
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let title = show.title.clone();
        let result = client.download_show(&url, &cache_dir, &mut |progress| {
            let (message_key, percent) = match progress {
                ShowProgress::Downloading(percent) => ("audiovault_progress_downloading", percent),
                ShowProgress::Extracting(percent) => ("audiovault_progress_extracting", percent),
            };
            let _ = sender.send(Event::Progress {
                task,
                message_key,
                title: title.clone(),
                percent,
            });
        });
        let result = result.and_then(|()| {
            if episode_files(&cache_dir).is_empty() {
                Err(AudiovaultError::NoEpisodes)
            } else {
                Ok(())
            }
        });
        Some(Event::ShowDone {
            show,
            download_after,
            allow_retry,
            task,
            generation,
            result,
        })
    });
}

unsafe fn finish_show(
    window: HWND,
    show: MediaItem,
    download_after: bool,
    allow_retry: bool,
    task: u64,
    generation: u64,
    result: std::result::Result<(), AudiovaultError>,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    close_progress(state, task);
    let cache_dir = vault::show_cache_dir(&state.application.settings().cache_folder, &show);
    match result {
        Ok(()) => {
            if download_after {
                copy_show_to_downloads(window, &show, &cache_dir);
            } else {
                let episodes =
                    vault::local_episode_items(&show, &episode_files(&cache_dir), &catalog(state));
                show_episodes_if_current(window, generation, &show, episodes);
            }
        }
        Err(AudiovaultError::SessionExpired) if allow_retry => retry_after_login(
            window,
            AfterLogin::PrepareShow {
                item: show,
                download_after,
            },
        ),
        Err(error) => message(window, "audiovault_show_failed", &error),
    }
}

// ---------------------------------------------------------------------------
// Episodes

/// Python `prepare_audiovault_remote_episode`. A background start keeps the
/// main window's `background_start` until the episode plays.
#[allow(clippy::too_many_lines)]
pub(super) unsafe fn prepare_remote_episode(
    window: HWND,
    item: MediaItem,
    download_after: bool,
    allow_retry: bool,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let target = PathBuf::from(item.local_path.clone().unwrap_or_default());
    let expected = vault::metadata_u64(&item, "archive_file_size");
    if target.is_file()
        && (expected == 0 || std::fs::metadata(&target).is_ok_and(|meta| meta.len() == expected))
    {
        apricot_platform::audiovault::touch(&target);
        finish_remote_episode(window, &item, download_after);
        return;
    }
    let _ = std::fs::remove_file(&target);
    let key = target.to_string_lossy().into_owned();
    if state.audiovault.episode_loading.contains(&key) {
        let message = keyed(
            state,
            "audiovault_episode_preparing",
            &[("title", &item.title)],
        );
        set_status(state, &message, true);
        return;
    }
    state.audiovault.episode_loading.insert(key);
    let task = if download_after {
        start_progress(window, &item.title)
    } else {
        announce(
            window,
            "audiovault_episode_cache_notice",
            &[("title", &item.title)],
        );
        0
    };
    let Some(state) = super::state(window) else {
        return;
    };
    let settings = state.application.settings();
    let cache_root = Path::new(&settings.cache_folder).join("audiovault");
    let cache_limit = settings.cache_size_mb;
    let show_cache_dir = vault::episode_show(&item).map_or_else(
        || {
            target
                .parent()
                .and_then(Path::parent)
                .map_or_else(PathBuf::new, Path::to_path_buf)
        },
        |show| vault::show_cache_dir(&settings.cache_folder, &show),
    );
    let playing = state
        .application
        .player_session()
        .current_item()
        .and_then(|current| current.local_path.clone());
    spawn(window, move |client, sender| {
        let title = item.title.clone();
        let archive_url = {
            let url = vault::metadata_text(&item, "archive_url");
            if url.is_empty() {
                vault::metadata_text(&item, "webpage_url")
            } else {
                url
            }
        };
        let member = vault::metadata_text(&item, "archive_member");
        let request = RemoteMemberRequest {
            archive_url: &archive_url,
            member: &member,
            expected_crc: u32::try_from(vault::metadata_u64(&item, "archive_crc"))
                .unwrap_or_default(),
            target: &target,
            show_cache_dir: &show_cache_dir,
        };
        let mut last_status = None;
        let result = client.extract_remote_member(&request, &mut |done, total| {
            let percent = (done * 100).checked_div(total).unwrap_or(0);
            if task != 0 {
                let _ = sender.send(Event::Progress {
                    task,
                    message_key: "audiovault_progress_downloading",
                    title: title.clone(),
                    percent,
                });
            } else {
                // Python speaks the cache progress in steps of five percent.
                let step = percent.min(100) / 5 * 5;
                if last_status != Some(step) {
                    last_status = Some(step);
                    let _ = sender.send(Event::Progress {
                        task: 0,
                        message_key: "audiovault_episode_cache_progress",
                        title: title.clone(),
                        percent: step,
                    });
                }
            }
        });
        if result.is_ok() {
            let mut protected = vec![target.clone()];
            protected.extend(playing.map(PathBuf::from));
            apricot_platform::audiovault::trim_episode_cache(&cache_root, cache_limit, &protected);
        }
        Some(Event::EpisodeDone {
            item,
            download_after,
            allow_retry,
            task,
            result,
        })
    });
}

unsafe fn finish_episode(
    window: HWND,
    item: MediaItem,
    download_after: bool,
    allow_retry: bool,
    task: u64,
    result: std::result::Result<(), AudiovaultError>,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let key = item.local_path.clone().unwrap_or_default();
    state.audiovault.episode_loading.remove(&key);
    close_progress(state, task);
    match result {
        Ok(()) => finish_remote_episode(window, &item, download_after),
        Err(AudiovaultError::SessionExpired) if allow_retry => retry_after_login(
            window,
            AfterLogin::PrepareEpisode {
                item,
                download_after,
            },
        ),
        Err(error) => {
            if let Some(state) = state_mut(window) {
                state.background_start = false;
            }
            let key = if download_after {
                "download_failed"
            } else {
                "player_failed"
            };
            message(window, key, &error);
        }
    }
}

/// Python `finish_audiovault_remote_episode`.
unsafe fn finish_remote_episode(window: HWND, item: &MediaItem, download_after: bool) {
    let playable = vault::cached_episode(item);
    if download_after {
        copy_episode_to_downloads(window, &playable);
    } else {
        play_local(window, playable);
    }
}

/// Python `audiovault_player_return_state`.
fn snapshot(state: &WindowState) -> Snapshot {
    let audiovault = &state.audiovault;
    Snapshot {
        mode: audiovault.mode,
        results: audiovault.results.clone(),
        view: audiovault.view,
        title: audiovault.title.clone(),
        parent: audiovault.parent.clone(),
    }
}

/// Python `play_audiovault_local_item`.
unsafe fn play_local(window: HWND, item: MediaItem) {
    let Some(state) = state_mut(window) else {
        return;
    };
    state.audiovault.player_return = Some(snapshot(state));
    let sequence = state.audiovault.results.clone();
    state
        .application
        .prepare_audiovault_playback(&sequence, &item);
    if state.application.player_sequence_source()
        == Some(apricot_app::PlaybackSequenceSource::Audiovault)
    {
        start_sequence_media_item(window, item, None);
    } else {
        start_media_item(window, item, None);
    }
}

/// Python `play_audiovault_remote_item`.
pub(super) unsafe fn play_remote(window: HWND, item: MediaItem, allow_retry: bool) {
    announce(window, "preparing_stream", &[("title", &item.title)]);
    spawn(window, move |client, _| {
        let url = item
            .url
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let result = client.resolve_stream(&url).map(|stream| ResolvedStream {
            final_url: stream.final_url,
            content_type: stream.content_type,
            disposition: stream.disposition,
            headers: stream.headers,
        });
        Some(Event::Stream {
            item,
            allow_retry,
            result,
        })
    });
}

unsafe fn finish_stream(
    window: HWND,
    mut item: MediaItem,
    allow_retry: bool,
    result: std::result::Result<ResolvedStream, AudiovaultError>,
) {
    match result {
        Ok(ResolvedStream {
            final_url,
            content_type,
            disposition,
            headers,
        }) => {
            if content_type.contains("zip") || disposition.to_lowercase().contains(".zip") {
                prepare_show(window, item, false, true);
                return;
            }
            let Ok(stream_url) = final_url.parse() else {
                message(
                    window,
                    "player_failed",
                    &AudiovaultError::Message(final_url),
                );
                return;
            };
            item.stream_url = Some(stream_url);
            item.metadata.insert(
                "http_headers".to_owned(),
                Value::Object(
                    headers
                        .into_iter()
                        .map(|(name, value)| (name, Value::String(value)))
                        .collect(),
                ),
            );
            // Python `start_audiovault_player`.
            if let Some(state) = state_mut(window) {
                state.audiovault.player_return = Some(snapshot(state));
            }
            start_media_item(window, item, None);
        }
        Err(AudiovaultError::SessionExpired) if allow_retry => {
            retry_after_login(window, AfterLogin::PlayRemote(item));
        }
        Err(error) => {
            if let Some(state) = state_mut(window) {
                state.background_start = false;
            }
            message(window, "player_failed", &error);
        }
    }
}

/// Starting an `AudioVault` item from anywhere: a movie needs its stream and
/// a TV show episode its cached file. Returns whether it was taken over.
pub(super) unsafe fn intercept_start(window: HWND, item: &MediaItem) -> bool {
    match vault::item_kind(item) {
        Some(AudiovaultKind::Movie) if item.stream_url.is_none() => {
            play_remote(window, item.clone(), true);
            true
        }
        Some(AudiovaultKind::RemoteEpisode) => {
            prepare_remote_episode(window, item.clone(), false, true);
            true
        }
        _ => false,
    }
}

/// Back from the player to an `AudioVault` screen: Python
/// `restore_audiovault_player_results`.
pub(super) unsafe fn restore_after_player(window: HWND) {
    let Some(snapshot) = state_mut(window).and_then(|state| {
        state
            .audiovault
            .player_return
            .clone()
            .or_else(|| Some(snapshot(state)))
    }) else {
        return;
    };
    if let Some(state) = state_mut(window) {
        state.audiovault.mode = snapshot.mode;
    }
    if snapshot.view == View::Search {
        show_search(window);
    } else if snapshot.view == View::Menu {
        show_menu(window);
        return;
    } else {
        show_results_screen(window, &snapshot.title, snapshot.view);
    }
    if let Some(state) = state_mut(window) {
        state.audiovault.parent = snapshot.parent;
    }
    show_results(window, snapshot.results);
}

// ---------------------------------------------------------------------------
// Downloads

/// Python `download_audiovault_selected`.
unsafe fn download_selected(window: HWND) {
    if let Some(item) = state(window).and_then(|state| selected_item(state)) {
        download_item(window, item, true);
    }
}

/// Python `start_download` for `AudioVault` items.
pub(super) unsafe fn start_download(window: HWND, item: &MediaItem, audio: bool) -> bool {
    if vault::item_kind(item).is_none() {
        return false;
    }
    if audio {
        download_item(window, item.clone(), true);
    } else {
        announce(window, "audiovault_video_unavailable", &[]);
    }
    true
}

/// Python `download_audiovault_item`.
#[allow(clippy::too_many_lines)]
unsafe fn download_item(window: HWND, mut item: MediaItem, allow_retry: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let kind = vault::item_kind(&item);
    let settings = state.application.settings().clone();
    if settings.ask_download_location_each_time {
        let catalog = catalog(state);
        let show = kind == Some(AudiovaultKind::Show);
        let key = if show {
            "_audiovault_download_target_folder"
        } else {
            "_audiovault_download_target_path"
        };
        if vault::metadata_text(&item, key).is_empty() {
            let folder = vault::download_folder(&settings.download_folder, &item, show);
            let _ = std::fs::create_dir_all(&folder);
            state.modal_open = true;
            let chosen = if show {
                crate::folder_dialog_win32::choose_download_folder(
                    window,
                    catalog.text("choose_save_folder"),
                    &folder,
                )
            } else {
                let extension = settings.audio_format.trim().to_owned();
                let default_name = format!(
                    "{}.{extension}",
                    vault::safe_folder_name(if item.title.is_empty() {
                        "download"
                    } else {
                        &item.title
                    })
                );
                crate::file_dialog_win32::save_download_file(
                    window,
                    catalog.text("choose_save_path"),
                    &folder,
                    &default_name,
                    &extension,
                    &extension.to_uppercase(),
                    catalog.text("all_files"),
                )
                .ok()
                .flatten()
                .map(|path| {
                    if path.extension().is_none() {
                        path.with_extension(&extension)
                    } else {
                        path
                    }
                })
            };
            if let Some(state) = state_mut(window) {
                state.modal_open = false;
            }
            super::resume_deferred_window_work(window);
            let Some(chosen) = chosen else {
                announce(window, "download_cancelled", &[]);
                return;
            };
            item.metadata.insert(
                key.to_owned(),
                Value::String(chosen.to_string_lossy().into_owned()),
            );
        }
    }
    match kind {
        Some(AudiovaultKind::Show) => {
            prepare_show(window, item, true, allow_retry);
            announce(window, "audiovault_show_cache_notice", &[]);
        }
        Some(AudiovaultKind::RemoteEpisode) => {
            prepare_remote_episode(window, item, true, allow_retry);
        }
        Some(AudiovaultKind::Episode) => copy_episode_to_downloads(window, &item),
        _ => {
            let folder = Path::new(&settings.download_folder).join("AudioVault");
            let _ = std::fs::create_dir_all(&folder);
            spawn(window, move |client, _| {
                let url = item
                    .url
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                let fallback = vault::safe_folder_name(if item.title.is_empty() {
                    "AudioVault"
                } else {
                    &item.title
                });
                let selected = vault::metadata_text(&item, "_audiovault_download_target_path");
                let selected =
                    (!selected.trim().is_empty()).then(|| PathBuf::from(selected.trim()));
                let result = client.download_movie(&url, &folder, &fallback, selected.as_deref());
                Some(Event::Movie {
                    item,
                    allow_retry,
                    result,
                })
            });
        }
    }
}

unsafe fn finish_movie(
    window: HWND,
    item: MediaItem,
    allow_retry: bool,
    result: std::result::Result<PathBuf, AudiovaultError>,
) {
    match result {
        Ok(path) => announce_complete(window, &item.title, &path),
        Err(AudiovaultError::SessionExpired) if allow_retry => {
            retry_after_login(window, AfterLogin::Download(item));
        }
        Err(error) => message(window, "download_failed", &error),
    }
}

unsafe fn announce_complete(window: HWND, title: &str, path: &Path) {
    announce(
        window,
        "audiovault_download_complete_path",
        &[("title", title), ("path", &path.to_string_lossy())],
    );
}

/// Python `copy_audiovault_episode_to_downloads`.
unsafe fn copy_episode_to_downloads(window: HWND, item: &MediaItem) {
    let Some(state) = state(window) else {
        return;
    };
    let selected = vault::metadata_text(item, "_audiovault_download_target_path");
    let target = if selected.trim().is_empty() {
        vault::episode_download_target(&state.application.settings().download_folder, item)
    } else {
        PathBuf::from(selected.trim())
    };
    let source = PathBuf::from(item.local_path.clone().unwrap_or_default());
    // Python lets a copy failure end the handler without a message.
    if apricot_platform::audiovault::copy_episode(&source, &target).is_ok() {
        announce_complete(window, &item.title, &target);
    }
}

/// Python `copy_audiovault_show_to_downloads`.
unsafe fn copy_show_to_downloads(window: HWND, show: &MediaItem, cache_dir: &Path) {
    let Some(state) = state(window) else {
        return;
    };
    let selected = vault::metadata_text(show, "_audiovault_download_target_folder");
    let merge = !selected.trim().is_empty();
    let target = if merge {
        PathBuf::from(selected.trim())
    } else {
        vault::show_download_target(&state.application.settings().download_folder, show)
    };
    if apricot_platform::audiovault::copy_show(cache_dir, &target, merge).is_ok() {
        announce_complete(window, &show.title, &target);
    }
}

// ---------------------------------------------------------------------------
// Worker events

/// `AUDIOVAULT_TIMER_ID`: every finished worker's result, in order.
pub(super) unsafe fn poll(window: HWND) {
    loop {
        let Some(state) = state_mut(window) else {
            return;
        };
        if state.modal_open || state.audiovault.polling {
            return;
        }
        let Ok(event) = state.audiovault.receiver.try_recv() else {
            if state.audiovault.running == 0 {
                let _ = KillTimer(Some(window), AUDIOVAULT_TIMER_ID);
            }
            return;
        };
        if !matches!(event, Event::Progress { .. }) {
            state.audiovault.running = state.audiovault.running.saturating_sub(1);
        }
        state.audiovault.polling = true;
        handle_event(window, event);
        if let Some(state) = state_mut(window) {
            state.audiovault.polling = false;
        }
    }
}

unsafe fn handle_event(window: HWND, event: Event) {
    match event {
        Event::Login {
            email,
            remember,
            after,
            result,
        } => finish_login(window, &email, remember, after, result),
        Event::Results {
            view,
            retry,
            result,
            mode,
        } => finish_results(window, view, retry, result, mode),
        Event::Manifest {
            show,
            generation,
            allow_retry,
            result,
        } => finish_manifest(window, show, generation, allow_retry, result),
        Event::Progress {
            task,
            message_key,
            title,
            percent,
        } => {
            let Some(state) = state(window) else {
                return;
            };
            if task != 0 && task != state.audiovault.progress_task {
                return;
            }
            let percent = percent.to_string();
            let message = keyed(
                state,
                message_key,
                &[("title", &title), ("percent", &percent)],
            );
            set_status(state, &message, false);
        }
        Event::ShowDone {
            show,
            download_after,
            allow_retry,
            task,
            generation,
            result,
        } => finish_show(
            window,
            show,
            download_after,
            allow_retry,
            task,
            generation,
            result,
        ),
        Event::EpisodeDone {
            item,
            download_after,
            allow_retry,
            task,
            result,
        } => finish_episode(window, item, download_after, allow_retry, task, result),
        Event::Stream {
            item,
            allow_retry,
            result,
        } => finish_stream(window, item, allow_retry, result),
        Event::Movie {
            item,
            allow_retry,
            result,
        } => finish_movie(window, item, allow_retry, result),
    }
}

/// Python `load_audiovault_recent_worker` and `search_audiovault_worker`
/// back on the window thread.
unsafe fn finish_results(
    window: HWND,
    view: View,
    retry: Option<AfterLogin>,
    result: std::result::Result<Vec<vault::VaultRecord>, AudiovaultError>,
    mode: AudiovaultMode,
) {
    match result {
        Ok(records) => {
            let Some(state) = state(window) else {
                return;
            };
            // The list belongs to the screen that asked for the results.
            let current = matches!(
                (view, state.view, state.audiovault.view),
                (View::Search, MainView::AudiovaultSearch, View::Search)
                    | (
                        View::Recent(_),
                        MainView::AudiovaultResults,
                        View::Recent(_)
                    )
            );
            if !current {
                return;
            }
            let section = match view {
                View::Recent(mode) => Some(mode.recent_section()),
                _ => None,
            };
            let results = vault::results_from_records(&records, mode, section, &catalog(state));
            show_results(window, results);
        }
        Err(AudiovaultError::SessionExpired) if retry.is_some() => {
            retry_after_login(window, retry.unwrap_or(AfterLogin::Nothing));
        }
        Err(error) => message(window, "audiovault_search_failed", &error),
    }
}

// ---------------------------------------------------------------------------
// Layout

/// Visibility of the `AudioVault` controls and the shared ones they reuse.
pub(super) unsafe fn sync_visibility(state: &WindowState) {
    let search = state.view == MainView::AudiovaultSearch;
    let results = state.view == MainView::AudiovaultResults;
    let controls = &state.audiovault_controls;
    for (control, visible) in [
        (controls.query_label, search),
        (controls.query, search),
        (controls.type_label, search),
        (controls.kind, search),
        (controls.title, results),
    ] {
        let _ = ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE });
    }
    if !is_view(state.view) {
        return;
    }
    let menu = state.view == MainView::AudiovaultMenu;
    for (control, visible) in [
        (state.open, menu),
        (state.search, search),
        (state.search_play, !menu),
        (state.search_download_audio, !menu),
        (state.search_download_video, false),
        (state.search_add_favorite, false),
        (state.search_label, false),
        (state.search_edit, false),
        (state.provider_label, false),
        (state.provider, false),
        (state.kind_label, false),
        (state.kind, false),
    ] {
        let _ = ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE });
    }
}

/// Python's `AudioVault` screens: the button row first, then the fields or the
/// title, then the list.
pub(super) unsafe fn layout(
    state: &WindowState,
    width: i32,
    height: i32,
    margin: i32,
    button_height: i32,
    status_height: i32,
) {
    let label_height = 22;
    let field_height = 30;
    let field_width = width - margin * 2;
    let mut y = margin;
    let controls = &state.audiovault_controls;
    match state.view {
        MainView::AudiovaultMenu => {
            layout_button_row(&[state.back, state.open], width, y, margin, button_height);
            y += button_height + margin;
        }
        MainView::AudiovaultSearch => {
            let _ = MoveWindow(state.back, margin, y, 180, button_height, true);
            y += button_height + margin;
            for (label, field, box_height) in [
                (controls.query_label, controls.query, field_height),
                (controls.type_label, controls.kind, 160),
            ] {
                let _ = MoveWindow(label, margin, y, field_width, label_height, true);
                let _ = MoveWindow(
                    field,
                    margin,
                    y + label_height,
                    field_width,
                    box_height,
                    true,
                );
                y += label_height + field_height + margin;
            }
            layout_button_row(
                &[state.search, state.search_play, state.search_download_audio],
                width,
                y,
                margin,
                button_height,
            );
            y += button_height + margin;
        }
        MainView::AudiovaultResults => {
            layout_button_row(
                &[state.back, state.search_play, state.search_download_audio],
                width,
                y,
                margin,
                button_height,
            );
            y += button_height + margin;
            let _ = MoveWindow(controls.title, margin, y, field_width, label_height, true);
            y += label_height;
        }
        _ => return,
    }
    let status_y = height - status_height - margin;
    let _ = MoveWindow(
        state.status,
        margin,
        status_y,
        field_width,
        status_height,
        true,
    );
    let _ = MoveWindow(
        state.list,
        margin,
        y,
        field_width,
        (status_y - margin - y).max(40),
        true,
    );
}

/// Python's Tab order: the button row, the fields, the list.
pub(super) fn tab_controls(state: &WindowState) -> Option<Vec<HWND>> {
    let controls = &state.audiovault_controls;
    Some(match state.view {
        MainView::AudiovaultMenu => vec![state.back, state.open, state.list],
        MainView::AudiovaultSearch => vec![
            state.back,
            controls.query,
            controls.kind,
            state.search,
            state.search_play,
            state.search_download_audio,
            state.list,
        ],
        MainView::AudiovaultResults => vec![
            state.back,
            state.search_play,
            state.search_download_audio,
            state.list,
        ],
        _ => return None,
    })
}

/// The query field until results arrive, then the list, where Python's
/// `show_audiovault_search` and `show_audiovault_results` put the focus.
pub(super) fn primary_control(state: &WindowState) -> HWND {
    if state.view == MainView::AudiovaultSearch && state.audiovault.results.is_empty() {
        state.audiovault_controls.query
    } else {
        state.list
    }
}

unsafe fn window_text(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    let mut buffer = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let copied = GetWindowTextW(control, &mut buffer);
    String::from_utf16_lossy(&buffer[..usize::try_from(copied).unwrap_or_default()])
}
