//! Spotify screens of the main window (`docs/SPOTIFY_PLAN.md` 4.1, 9.1): the
//! hub opened from the main menu, the account list and the browser login.
//! The session runtime is `apricot-spotify`; its events arrive as
//! `WM_SPOTIFY_EVENT` and are handled in [`poll`].

use std::{
    ffi::c_void,
    sync::{Arc, mpsc::Receiver},
};

use apricot_app::spotify::{
    SpotifyAccountRow, SpotifyHubEntry, SpotifyHubModel, account_rows, error_text, named,
    queue_rows,
};
use apricot_core::{
    Route, RouteFrame, SpotifyEntityKind, SpotifyEpochs, SpotifyRef, SpotifyStamp,
    TranslationCatalog,
};
use apricot_spotify::{
    CallbackPage, PlaybackNotice, RepeatMode, SpotifyAccounts, SpotifyError, SpotifyEvent,
    SpotifyService, SpotifyTrack,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::{
            Input::KeyboardAndMouse::{GetFocus, SetFocus},
            Shell::ShellExecuteW,
            WindowsAndMessaging::{
                IDYES, IsWindow, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, MB_ICONQUESTION,
                MB_YESNO, MessageBoxW, PostMessageW, SW_SHOWNORMAL, SendMessageW, SetWindowTextW,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{
    MainView, WM_SPOTIFY_EVENT, WindowState, add_list_string, cancel_local_folder_scan,
    cancel_youtube_work, layout_controls_state, remember_menu_item, restore_from_tray,
    resume_deferred_window_work, set_status, show_error_message, show_main_menu, start_media_item,
    start_media_item_at, state, state_mut, stop_controlled_repeat, wide,
};

/// Where a finished login returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReturnTo {
    Hub,
    Accounts,
}

#[derive(Default)]
pub(super) struct SpotifyState {
    service: Option<Arc<SpotifyService>>,
    receiver: Option<Receiver<SpotifyEvent>>,
    epochs: SpotifyEpochs,
    /// ID of the login the dialog is waiting for; older results are stale.
    login: u64,
    login_url: Option<String>,
    login_dialog: Option<HWND>,
    login_account: Option<String>,
    hub_entries: Vec<SpotifyHubEntry>,
    hub_selected: Option<SpotifyHubEntry>,
    rows: Vec<SpotifyAccountRow>,
    /// The switch the user asked for; only its result is announced.
    connect: Option<SpotifyStamp>,
    /// A link waiting for the connection of the active account.
    pending_play: Option<String>,
    /// The link whose metadata is being read; only the latest one plays.
    resolve: Option<SpotifyStamp>,
    /// The open queue dialog and the reload it waits for.
    queue_dialog: Option<HWND>,
    queue_load: Option<SpotifyStamp>,
    /// The open device list.
    devices_dialog: Option<HWND>,
    /// The transfer the user asked for and the device's name.
    transfer: Option<(SpotifyStamp, String)>,
    polling: bool,
}

pub(super) const fn is_view(view: MainView) -> bool {
    matches!(
        view,
        MainView::SpotifyHub | MainView::SpotifyAccounts | MainView::SpotifyBrowse
    )
}

impl SpotifyState {
    /// A request stamp of the current account.
    pub(super) fn epochs_begin(&mut self) -> SpotifyStamp {
        self.epochs.begin()
    }
}

pub(super) fn catalog(state: &WindowState) -> TranslationCatalog {
    apricot_app::embedded_catalog(&state.application.settings().language)
}

pub(super) unsafe fn announce(window: HWND, text: &str) {
    if let Some(state) = state(window) {
        set_status(state, text, true);
    }
}

/// The runtime starts on first use, not with the application.
pub(super) unsafe fn service(window: HWND) -> Option<Arc<SpotifyService>> {
    let state = state_mut(window)?;
    if state.spotify.service.is_none() {
        let app_data = apricot_platform::discover_app_paths().ok()?.app_data;
        let handle = window.0 as isize;
        let (service, receiver) = SpotifyService::new(
            &app_data,
            Arc::new(move || {
                // SAFETY: Posting to a window handle is thread-safe; a closed
                // window simply drops the message.
                let _ = unsafe {
                    PostMessageW(
                        Some(HWND(handle as *mut c_void)),
                        WM_SPOTIFY_EVENT,
                        WPARAM(0),
                        LPARAM(0),
                    )
                };
            }),
        );
        state.spotify.service = Some(Arc::new(service));
        state.spotify.receiver = Some(receiver);
    }
    state.spotify.service.clone()
}

unsafe fn accounts(window: HWND) -> SpotifyAccounts {
    service(window)
        .map(|service| service.accounts())
        .unwrap_or_default()
}

pub(super) unsafe fn prepare_screen(window: HWND, main_view: MainView, route: Route) {
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != route {
        state.application.navigate_main_menu();
        if matches!(route, Route::SpotifyAccounts | Route::SpotifyBrowse) {
            state
                .application
                .navigate_to(RouteFrame::new(Route::SpotifyHub));
        }
        state.application.navigate_to(RouteFrame::new(route));
    }
    state.view = main_view;
}

/// Main menu Spotify, `open_spotify`.
pub(super) unsafe fn show_hub(window: HWND) {
    remember_menu_item(window, "spotify");
    let accounts = accounts(window);
    prepare_screen(window, MainView::SpotifyHub, Route::SpotifyHub);
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = catalog(state);
    let settings = state.application.settings();
    let model = SpotifyHubModel::build(
        &catalog,
        &accounts,
        settings.show_shortcuts_in_labels,
        &settings.keyboard_shortcuts,
    );
    super::set_open_button_label(state, "open");
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    crate::accessibility_win32::set_control_name(state.list, &model.accessible_name);
    for item in &model.items {
        add_list_string(state.list, &item.label);
    }
    state.spotify.hub_entries = model.items.iter().map(|item| item.entry).collect();
    let selected = state
        .spotify
        .hub_selected
        .and_then(|entry| state.spotify.hub_entries.iter().position(|e| *e == entry))
        .unwrap_or(0);
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    layout_controls_state(window, state);
    let _ = SetFocus(Some(state.list));
}

/// `spotify_accounts`; `select` keeps an account selected after a change.
pub(super) unsafe fn show_accounts(window: HWND, select: Option<&str>) {
    prepare_screen(window, MainView::SpotifyAccounts, Route::SpotifyAccounts);
    refresh_accounts(window, select);
    if let Some(state) = state(window) {
        let _ = SetFocus(Some(state.list));
    }
}

/// Refills the account list in place; the focus stays where it is.
unsafe fn refresh_accounts(window: HWND, select: Option<&str>) {
    let accounts = accounts(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = catalog(state);
    let rows = account_rows(&catalog, &accounts);
    super::set_open_button_label(state, "open");
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    crate::accessibility_win32::set_control_name(state.list, catalog.text("spotify_accounts"));
    for row in &rows {
        add_list_string(state.list, row.label());
    }
    let wanted = select.or(accounts.active.as_deref());
    let selected = wanted
        .and_then(|key| rows.iter().position(|row| row.key() == Some(key)))
        .unwrap_or(0);
    state.spotify.rows = rows;
    state.spotify.hub_selected = Some(SpotifyHubEntry::Accounts);
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    layout_controls_state(window, state);
}

unsafe fn selected_index(state: &WindowState) -> Option<usize> {
    usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()
}

/// Enter and Open.
pub(super) unsafe fn activate(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let Some(index) = selected_index(state) else {
        return;
    };
    match state.view {
        MainView::SpotifyHub => match state.spotify.hub_entries.get(index).copied() {
            Some(SpotifyHubEntry::LogIn) => {
                if let Some(state) = state_mut(window) {
                    state.spotify.hub_selected = Some(SpotifyHubEntry::LogIn);
                }
                log_in(window, ReturnTo::Hub);
            }
            Some(entry @ SpotifyHubEntry::Search) => {
                remember_hub_entry(window, entry);
                show_search(window);
            }
            Some(entry @ SpotifyHubEntry::Library) => {
                remember_hub_entry(window, entry);
                show_library(window);
            }
            Some(entry @ SpotifyHubEntry::LikedSongs) => {
                remember_hub_entry(window, entry);
                show_liked_songs(window);
            }
            Some(entry @ SpotifyHubEntry::Playlists) => {
                remember_hub_entry(window, entry);
                show_playlists(window);
            }
            Some(SpotifyHubEntry::Queue) => {
                if let Some(state) = state_mut(window) {
                    state.spotify.hub_selected = Some(SpotifyHubEntry::Queue);
                }
                show_queue(window);
            }
            Some(SpotifyHubEntry::Devices) => {
                if let Some(state) = state_mut(window) {
                    state.spotify.hub_selected = Some(SpotifyHubEntry::Devices);
                }
                show_devices(window);
            }
            Some(SpotifyHubEntry::Accounts) => show_accounts(window, None),
            None => {}
        },
        MainView::SpotifyAccounts => match state.spotify.rows.get(index).cloned() {
            Some(SpotifyAccountRow::AddAccount { .. }) => log_in(window, ReturnTo::Accounts),
            Some(SpotifyAccountRow::Account { key, .. }) => {
                let accounts = accounts(window);
                let Some(account) = accounts.accounts.iter().find(|a| a.key == key) else {
                    return;
                };
                if !account.is_logged_in() {
                    log_in(window, ReturnTo::Accounts);
                } else if accounts.active.as_deref() != Some(key.as_str()) {
                    use_account(window, &key);
                } else {
                    let text = named(&catalog(state), "spotify_connected", &account.display_name);
                    announce(window, &text);
                }
            }
            None => {}
        },
        _ => {}
    }
}

/// The selected row of the account list.
pub(super) enum SelectedRow {
    AddAccount,
    Account { logged_in: bool, active: bool },
}

impl SelectedRow {
    /// The shape `spotify_accounts_context_menu` takes.
    pub(super) const fn menu_account(&self) -> Option<(bool, bool)> {
        match self {
            Self::AddAccount => None,
            Self::Account { logged_in, active } => Some((*logged_in, *active)),
        }
    }
}

pub(super) unsafe fn selected_account(state: &WindowState) -> Option<SelectedRow> {
    if state.view != MainView::SpotifyAccounts {
        return None;
    }
    let row = state.spotify.rows.get(selected_index(state)?)?;
    let Some(key) = row.key() else {
        return Some(SelectedRow::AddAccount);
    };
    let accounts = state
        .spotify
        .service
        .as_ref()
        .map(|service| service.accounts())
        .unwrap_or_default();
    let account = accounts.accounts.iter().find(|a| a.key == key)?;
    Some(SelectedRow::Account {
        logged_in: account.is_logged_in(),
        active: accounts.active.as_deref() == Some(key),
    })
}

unsafe fn selected_key(window: HWND) -> Option<String> {
    let state = state(window)?;
    state
        .spotify
        .rows
        .get(selected_index(state)?)?
        .key()
        .map(str::to_owned)
}

/// Context menu Use this account and Enter on another logged-in account.
pub(super) unsafe fn use_selected_account(window: HWND) {
    if let Some(key) = selected_key(window) {
        use_account(window, &key);
    }
}

unsafe fn use_account(window: HWND, key: &str) {
    let Some(service) = service(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    state.spotify.epochs.next_account();
    let stamp = state.spotify.epochs.begin();
    state.spotify.connect = Some(stamp);
    let text = catalog(state).text("spotify_connecting").to_owned();
    set_status(state, &text, true);
    service.connect(stamp, key);
    refresh_accounts(window, Some(key));
}

/// Context menu Log in / Log in again.
pub(super) unsafe fn log_in_from_accounts(window: HWND) {
    log_in(window, ReturnTo::Accounts);
}

/// Context menu Log out.
pub(super) unsafe fn log_out_selected(window: HWND) {
    let Some(key) = selected_key(window) else {
        return;
    };
    let Some(service) = service(window) else {
        return;
    };
    let name = display_name(&service.accounts(), &key);
    if let Some(state) = state_mut(window) {
        state.spotify.epochs.next_account();
        state.spotify.connect = None;
    }
    match service.logout(&key) {
        Ok(_) => {
            let text = state(window)
                .map(|state| named(&catalog(state), "spotify_logged_out_done", &name))
                .unwrap_or_default();
            refresh_accounts(window, Some(&key));
            announce(window, &text);
        }
        Err(error) => report_error(window, &error),
    }
}

/// Context menu Remove account and Delete on an account row.
pub(super) unsafe fn remove_selected(window: HWND) {
    let Some(key) = selected_key(window) else {
        return;
    };
    let Some(service) = service(window) else {
        return;
    };
    let name = display_name(&service.accounts(), &key);
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = catalog(state);
    let question = wide(&named(&catalog, "spotify_remove_account_confirm", &name));
    let title = wide(catalog.text("spotify_remove_account"));
    state.modal_open = true;
    let previous = GetFocus();
    let answer = MessageBoxW(
        Some(window),
        PCWSTR(question.as_ptr()),
        PCWSTR(title.as_ptr()),
        MB_YESNO | MB_ICONQUESTION,
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    resume_deferred_window_work(window);
    if !previous.is_invalid() && IsWindow(Some(previous)).as_bool() {
        let _ = SetFocus(Some(previous));
    }
    if answer != IDYES {
        return;
    }
    if let Some(state) = state_mut(window) {
        state.spotify.epochs.next_account();
        state.spotify.connect = None;
    }
    match service.remove(&key) {
        Ok(remaining) => {
            let text = named(&catalog, "spotify_account_removed", &name);
            if remaining.accounts.is_empty() {
                show_hub(window);
            } else {
                refresh_accounts(window, remaining.active.as_deref());
            }
            announce(window, &text);
        }
        Err(error) => report_error(window, &error),
    }
}

fn display_name(accounts: &SpotifyAccounts, key: &str) -> String {
    accounts
        .accounts
        .iter()
        .find(|account| account.key == key)
        .map(|account| account.display_name.clone())
        .unwrap_or_default()
}

unsafe fn report_error(window: HWND, error: &SpotifyError) {
    if let Some(state) = state(window) {
        let text = error_text(&catalog(state), error);
        show_error_message(window, &text);
    }
}

// ---------------------------------------------------------------------------
// Login

unsafe fn open_url(url: &str) {
    // Debug builds on the invisible test desktop must never start the
    // user's browser.
    if cfg!(debug_assertions) && std::env::var_os("APRICOT_SPOTIFY_TEST_NO_BROWSER").is_some() {
        return;
    }
    let url = wide(url);
    let _ = ShellExecuteW(
        None,
        w!("open"),
        PCWSTR(url.as_ptr()),
        None,
        None,
        SW_SHOWNORMAL,
    );
}

unsafe fn start_login(window: HWND) {
    let Some(service) = service(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    state.spotify.login += 1;
    state.spotify.login_url = None;
    let catalog = catalog(state);
    let page = CallbackPage::from_texts(
        catalog.selected_code(),
        catalog.text("spotify_browser_success"),
        catalog.text("spotify_browser_failure"),
    );
    service.begin_login(state.spotify.login, page);
}

#[allow(clippy::too_many_lines)]
unsafe fn log_in(window: HWND, return_to: ReturnTo) {
    if service(window).is_none() {
        return;
    }
    let Some(window_state) = state_mut(window) else {
        return;
    };
    let texts_catalog = catalog(window_state);
    let texts = crate::spotify_login_win32::LoginTexts {
        title: texts_catalog.text("spotify_login"),
        status_name: texts_catalog.text("spotify_login"),
        waiting: texts_catalog.text("spotify_login_waiting"),
        open_browser: texts_catalog.text("spotify_open_browser_again"),
        copy_link: texts_catalog.text("spotify_copy_login_link"),
        retry: texts_catalog.text("spotify_retry"),
        cancel: texts_catalog.text("cancel"),
    };
    let handle = window.0 as isize;
    let main = move || HWND(handle as *mut c_void);
    let actions = crate::spotify_login_win32::LoginActions {
        open_browser: Box::new(move || {
            if let Some(url) = state(main()).and_then(|state| state.spotify.login_url.clone()) {
                open_url(&url);
            }
        }),
        copy_link: Box::new(move || {
            let window = main();
            let Some(url) = state(window).and_then(|state| state.spotify.login_url.clone()) else {
                return;
            };
            if crate::clipboard_win32::copy_text(window, &url).is_ok()
                && let Some(state) = state(window)
            {
                let text = catalog_for(state)
                    .text("spotify_login_link_copied")
                    .to_owned();
                set_status(state, &text, true);
            }
        }),
        retry: Box::new(move || {
            let window = main();
            if let Some(dialog) = state(window).and_then(|state| state.spotify.login_dialog) {
                crate::spotify_login_win32::update(
                    dialog,
                    crate::spotify_login_win32::LoginUpdate::Waiting,
                );
            }
            start_login(window);
        }),
        cancel: Box::new(move || {
            let window = main();
            if let Some(state) = state_mut(window) {
                // Any late result of this login is now stale.
                state.spotify.login += 1;
                if let Some(service) = &state.spotify.service {
                    service.cancel_login();
                }
            }
        }),
    };
    window_state.modal_open = true;
    window_state.spotify.login_account = None;
    let outcome = crate::spotify_login_win32::show(window, &texts, actions, |dialog| {
        if let Some(state) = state_mut(window) {
            state.spotify.login_dialog = Some(dialog);
        }
        start_login(window);
    });
    let name = state_mut(window).and_then(|state| {
        state.modal_open = false;
        state.spotify.login_dialog = None;
        state.spotify.login_account.take()
    });
    resume_deferred_window_work(window);
    match outcome {
        Ok(crate::spotify_login_win32::LoginOutcome::Succeeded) => {
            let key = state(window).and_then(|state| {
                state
                    .spotify
                    .service
                    .as_ref()
                    .and_then(|service| service.accounts().active)
            });
            if let Some(state) = state_mut(window) {
                // The new login is the active session.
                state.spotify.epochs.next_account();
                state.spotify.connect = None;
                state.spotify.hub_selected = Some(SpotifyHubEntry::Accounts);
            }
            match return_to {
                ReturnTo::Hub => show_hub(window),
                ReturnTo::Accounts => show_accounts(window, key.as_deref()),
            }
            if let Some(state) = state(window) {
                let text = named(
                    &catalog(state),
                    "spotify_logged_in",
                    name.as_deref().unwrap_or(""),
                );
                announce(window, &text);
            }
        }
        Ok(crate::spotify_login_win32::LoginOutcome::Closed) => {
            if let Some(state) = state(window) {
                let text = catalog(state).text("spotify_login_cancelled").to_owned();
                announce(window, &text);
            }
        }
        Err(_) => {}
    }
}

fn catalog_for(state: &WindowState) -> TranslationCatalog {
    catalog(state)
}

/// `WM_SPOTIFY_EVENT`.
pub(super) unsafe fn poll(window: HWND) {
    loop {
        let Some(state) = state_mut(window) else {
            return;
        };
        if state.spotify.polling {
            return;
        }
        let event = state
            .spotify
            .receiver
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        let notice = if event.is_none() {
            state
                .spotify
                .service
                .as_ref()
                .and_then(|service| service.take_notice())
        } else {
            None
        };
        if event.is_none() && notice.is_none() {
            return;
        }
        state.spotify.polling = true;
        if let Some(event) = event {
            handle_event(window, event);
        }
        if let Some(notice) = notice {
            handle_notice(window, notice);
        }
        if let Some(state) = state_mut(window) {
            state.spotify.polling = false;
        }
    }
}

#[allow(clippy::too_many_lines)]
unsafe fn handle_event(window: HWND, event: SpotifyEvent) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = catalog(state);
    match event {
        SpotifyEvent::LoginUrl { login, url } => {
            if login != state.spotify.login {
                return;
            }
            state.spotify.login_url = Some(url.clone());
            open_url(&url);
        }
        SpotifyEvent::LoginFinished { login, result } => {
            if login != state.spotify.login {
                return;
            }
            let Some(dialog) = state.spotify.login_dialog else {
                return;
            };
            match result {
                Ok(account) => {
                    state.spotify.login_account = Some(account.display_name);
                    crate::spotify_login_win32::update(
                        dialog,
                        crate::spotify_login_win32::LoginUpdate::Succeeded,
                    );
                }
                Err(SpotifyError::Cancelled) => {}
                Err(error) => {
                    let text = error_text(&texts, &error);
                    crate::spotify_login_win32::update(
                        dialog,
                        crate::spotify_login_win32::LoginUpdate::Failed(text.clone()),
                    );
                    set_status(state, &text, true);
                }
            }
        }
        SpotifyEvent::Connected { stamp, result } => {
            // Every connect starts a new account epoch; later requests of
            // other kinds (links, the queue) do not make it stale.
            if !state.spotify.epochs.is_current_account(stamp) {
                return;
            }
            let requested = state.spotify.connect.take() == Some(stamp);
            let text = match &result {
                Ok(account) => named(&texts, "spotify_connected", &account.display_name),
                Err(error) => error_text(&texts, error),
            };
            if state.view == MainView::SpotifyAccounts {
                // The selection stays on the row the user is on.
                let key = selected_key(window);
                refresh_accounts(window, key.as_deref());
            }
            let pending = state.spotify.pending_play.take();
            if requested || result.is_err() {
                announce(window, &text);
            }
            if let (Ok(_), Some(uri)) = (&result, pending) {
                resolve(window, &uri);
            }
        }
        SpotifyEvent::Disconnected => {
            let text = texts.text("spotify_disconnected").to_owned();
            set_status(state, &text, true);
        }
        SpotifyEvent::Resolved { stamp, result } => {
            if state.spotify.resolve != Some(stamp) {
                return;
            }
            state.spotify.resolve = None;
            match result {
                Ok((track, context)) => {
                    let mut item = track.media_item();
                    if let Some(context) = context {
                        // An album or playlist link plays as that context.
                        item.metadata.insert(
                            "spotify_context".to_owned(),
                            serde_json::Value::String(context),
                        );
                    }
                    play_track(window, item);
                }
                Err(error) => {
                    let text = error_text(&texts, &error);
                    show_error_message(window, &text);
                }
            }
        }
        SpotifyEvent::Catalog { stamp, result } => {
            super::spotify_browse::loaded(window, stamp, result);
        }
        SpotifyEvent::Transferred { stamp, result } => {
            let Some((wanted, name)) = state.spotify.transfer.take() else {
                return;
            };
            if wanted != stamp {
                state.spotify.transfer = Some((wanted, name));
                return;
            }
            let text = match result {
                Ok(()) => named(&texts, "spotify_transferred", &name),
                Err(error) => error_text(&texts, &error),
            };
            announce(window, &text);
        }
        SpotifyEvent::Queue { stamp, queue } => {
            if state.spotify.queue_load != Some(stamp) {
                return;
            }
            state.spotify.queue_load = None;
            if let Some(dialog) = state.spotify.queue_dialog {
                crate::spotify_queue_win32::update(dialog, queue_rows(&texts, &queue));
            }
        }
    }
}

unsafe fn remember_hub_entry(window: HWND, entry: SpotifyHubEntry) {
    if let Some(state) = state_mut(window) {
        state.spotify.hub_selected = Some(entry);
    }
}

/// The lists need a connected account; without one, it says what to do.
unsafe fn ready_for_lists(window: HWND) -> bool {
    let accounts = accounts(window);
    let Some(service) = service(window) else {
        return false;
    };
    let Some(state) = state(window) else {
        return false;
    };
    let texts = catalog(state);
    if apricot_app::spotify::active_account(&accounts).is_none() {
        let text = texts.text("spotify_log_in_first").to_owned();
        announce(window, &text);
        return false;
    }
    if service.connected_account().is_none() {
        let text = texts.text("spotify_not_connected").to_owned();
        announce(window, &text);
        return false;
    }
    true
}

/// `spotify_search`: the search dialog, then the results list.
pub(super) unsafe fn show_search(window: HWND) {
    stop_controlled_repeat(window);
    if !ready_for_lists(window) {
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = catalog(state);
    let kinds = apricot_app::spotify::search_kind_labels(&texts);
    let search_texts = crate::spotify_search_win32::SearchTexts {
        title: texts.text("spotify_search_dialog"),
        query: texts.text("spotify_search_query"),
        kind: texts.text("spotify_search_type"),
        search: texts.text("search"),
        cancel: texts.text("cancel"),
    };
    let query = state.spotify_browse.last_query.clone();
    let kind = state.spotify_browse.last_kind;
    state.modal_open = true;
    let result = crate::spotify_search_win32::show(window, &search_texts, &query, &kinds, kind);
    let Some(state) = state_mut(window) else {
        return;
    };
    state.modal_open = false;
    resume_deferred_window_work(window);
    match result {
        Ok(Some((query, kind))) => {
            state.spotify_browse.last_query.clone_from(&query);
            state.spotify_browse.last_kind = kind;
            let title = super::spotify_browse::search_title(state, &query);
            let searching = texts.text("spotify_searching").replace("{query}", &query);
            set_status(state, &searching, true);
            let kind = apricot_spotify::SearchKind::ALL
                .get(kind)
                .copied()
                .unwrap_or(apricot_spotify::SearchKind::All);
            super::spotify_browse::open_root(
                window,
                super::spotify_browse::Source::Search { query, kind },
                title,
            );
        }
        Ok(None) => {
            let _ = SetFocus(Some(super::active_primary_control(state)));
        }
        Err(error) => {
            let message = format!("Spotify search did not open: {error}");
            show_error_message(window, &message);
        }
    }
}

unsafe fn show_list(window: HWND, source: super::spotify_browse::Source, title_key: &str) {
    stop_controlled_repeat(window);
    if !ready_for_lists(window) {
        return;
    }
    let Some(state) = state(window) else {
        return;
    };
    let title = catalog(state).text(title_key).to_owned();
    super::spotify_browse::open_root(window, source, title);
}

/// `spotify_library`: everything saved, as Spotify's library lists it.
pub(super) unsafe fn show_library(window: HWND) {
    show_list(
        window,
        super::spotify_browse::Source::Library {
            filter: apricot_spotify::LibraryFilter::All,
            folder: None,
        },
        "spotify_library",
    );
}

/// `spotify_liked_songs`.
pub(super) unsafe fn show_liked_songs(window: HWND) {
    show_list(
        window,
        super::spotify_browse::Source::LikedSongs,
        "spotify_liked_songs",
    );
}

/// `spotify_playlists`: the library's playlists and folders.
pub(super) unsafe fn show_playlists(window: HWND) {
    show_list(
        window,
        super::spotify_browse::Source::Library {
            filter: apricot_spotify::LibraryFilter::Playlists,
            folder: None,
        },
        "spotify_playlists",
    );
}

/// Back and Escape: accounts return to the hub, the hub to the main menu.
pub(super) unsafe fn back(window: HWND) {
    match state(window).map(|state| state.view) {
        Some(MainView::SpotifyBrowse) => super::spotify_browse::back(window),
        Some(MainView::SpotifyAccounts) => show_hub(window),
        Some(MainView::SpotifyHub) => show_main_menu(window),
        _ => {}
    }
}

/// Closes the session with the application.
pub(super) unsafe fn shutdown(window: HWND) {
    if let Some(service) = state(window).and_then(|state| state.spotify.service.clone()) {
        service.shutdown();
    }
}

// ---------------------------------------------------------------------------
// Playback

/// At start-up the active account connects in the background, so its
/// Connect device is there for the phone. Nothing is announced.
pub(super) unsafe fn autoconnect(window: HWND) {
    let accounts = accounts(window);
    let Some(key) = apricot_app::spotify::active_account(&accounts).map(|a| a.key.clone()) else {
        return;
    };
    let Some(service) = service(window) else {
        return;
    };
    if let Some(state) = state_mut(window) {
        state.spotify.epochs.next_account();
        let stamp = state.spotify.epochs.begin();
        state.spotify.connect = None;
        service.connect(stamp, &key);
    }
}

/// A `spotify:` URI or an `open.spotify.com` link from Direct link.
/// Returns `false` when the text is not a Spotify reference.
pub(super) unsafe fn play_link(window: HWND, text: &str, action: &str) -> bool {
    let Some(reference) = SpotifyRef::parse(text) else {
        return false;
    };
    let Some(state) = state_mut(window) else {
        return true;
    };
    let texts = catalog(state);
    if action != "play" {
        // Spotify content is never downloaded or exported (plan 3.5).
        let text = texts.text("spotify_no_export").to_owned();
        show_error_message(window, &text);
        return true;
    }
    if !matches!(
        reference.kind,
        SpotifyEntityKind::Track
            | SpotifyEntityKind::Episode
            | SpotifyEntityKind::Album
            | SpotifyEntityKind::Playlist
    ) {
        let text = texts.text("spotify_link_kind_unsupported").to_owned();
        show_error_message(window, &text);
        return true;
    }
    let uri = reference.to_uri();
    let connected = state
        .spotify
        .service
        .as_ref()
        .is_some_and(|service| service.connected_account().is_some());
    if connected {
        resolve(window, &uri);
        return true;
    }
    let accounts = accounts(window);
    let Some(key) = apricot_app::spotify::active_account(&accounts).map(|a| a.key.clone()) else {
        let text = texts.text("spotify_log_in_first").to_owned();
        show_hub(window);
        announce(window, &text);
        return true;
    };
    if let (Some(service), Some(state)) = (service(window), state_mut(window)) {
        state.spotify.pending_play = Some(uri);
        state.spotify.epochs.next_account();
        let stamp = state.spotify.epochs.begin();
        state.spotify.connect = None;
        let text = texts.text("spotify_connecting").to_owned();
        set_status(state, &text, true);
        service.connect(stamp, &key);
    }
    true
}

unsafe fn resolve(window: HWND, uri: &str) {
    let Some(service) = service(window) else {
        return;
    };
    if let Some(state) = state_mut(window) {
        let stamp = state.spotify.epochs.begin();
        state.spotify.resolve = Some(stamp);
        service.resolve_track(stamp, uri);
    }
}

/// Music starts at 0:00, spoken content resumes (plan D14).
pub(super) unsafe fn play_track(window: HWND, item: apricot_core::MediaItem) {
    let episode =
        item.metadata.get("kind").and_then(|kind| kind.as_str()) == Some("spotify_episode");
    if episode {
        start_media_item(window, item, None);
    } else {
        start_media_item_at(window, item, 0.0);
    }
}

/// Connect: another device started a track on Apricot. The previous item
/// saves its position and stops, the new one plays in the background and
/// the focus stays where it is (plan D08). One short announcement.
unsafe fn handle_notice(window: HWND, notice: PlaybackNotice) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = catalog(state);
    match notice {
        PlaybackNotice::Playing {
            track,
            requested: true,
            ..
        } => {
            // An album or playlist started as a whole becomes its first track.
            let collection = state
                .application
                .player_session()
                .current_item()
                .and_then(|item| item.metadata.get("spotify_collection"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            // Also when Spotify started another track than the item names
            // (a context that could not find the occurrence); a relinked
            // track (another ID, same title) stays as it is.
            let other_track = state
                .application
                .player_session()
                .current_item()
                .is_some_and(|item| item.id.0 != track.uri && item.title != track.title);
            if collection || other_track {
                continue_spotify_item(window, &track);
            }
            push_volume(window);
        }
        PlaybackNotice::Playing {
            track,
            position_ms,
            requested: false,
        } => {
            if continue_spotify_item(window, &track) {
                return;
            }
            let Some(state) = state_mut(window) else {
                return;
            };
            let mut item = track.media_item();
            item.metadata
                .insert("spotify_attach".to_owned(), serde_json::Value::Bool(true));
            state.background_start = state.view != MainView::Player;
            // The player's own "Playing: <title>" is the one announcement.
            start_media_item_at(window, item, f64::from(position_ms) / 1000.0);
            push_volume(window);
        }
        PlaybackNotice::Unavailable { .. } => {
            let text = texts.text("spotify_track_unavailable").to_owned();
            set_status(state, &text, true);
        }
        PlaybackNotice::QueueChanged => {
            if state.spotify.queue_dialog.is_some() {
                reload_queue(window);
            }
        }
        PlaybackNotice::DevicesChanged => {
            if let Some(dialog) = state.spotify.devices_dialog {
                let rows = device_rows(window);
                crate::spotify_devices_win32::update(dialog, rows);
            }
        }
        PlaybackNotice::Volume(volume) => apply_connect_volume(window, volume),
    }
}

/// The playing Spotify item's player volume, when a Spotify item plays.
unsafe fn spotify_volume(state: &WindowState) -> Option<f64> {
    let session = state.application.player_session();
    let spotify = session.is_open()
        && session
            .current_item()
            .is_some_and(|item| item.source == apricot_core::MediaSource::Spotify);
    spotify
        .then(|| session.audio().map(|audio| audio.volume))
        .flatten()
}

/// Apricot's volume changed (Up, Down, boost): the Connect device shows it,
/// so the phone's volume slider follows (SD-4).
pub(super) unsafe fn volume_changed(window: HWND, volume: f64) {
    let Some(state) = state(window) else {
        return;
    };
    if spotify_volume(state).is_none() {
        return;
    }
    if let Some(playback) = state
        .spotify
        .service
        .as_ref()
        .and_then(|service| service.playback())
    {
        playback.set_volume(volume);
    }
}

/// A Spotify item started: Connect gets Apricot's volume.
unsafe fn push_volume(window: HWND) {
    if let Some(volume) = state(window).and_then(|state| spotify_volume(state)) {
        volume_changed(window, volume);
    }
}

/// The phone (or another device) set the volume of Apricot's Connect device:
/// the player volume follows without an announcement, like Up and Down. Our
/// own change comes back the same and is ignored; above 100 percent (boost)
/// Connect shows 100 and the boost stays.
unsafe fn apply_connect_volume(window: HWND, volume: u16) {
    let Some(state) = state(window) else {
        return;
    };
    let Some(current) = spotify_volume(state) else {
        return;
    };
    let percent = apricot_spotify::playback::percent_from_volume(volume);
    if (percent - current.min(100.0)).abs() < 0.5 {
        return;
    }
    if super::execute_player_command(
        window,
        apricot_playback::PlaybackCommand::SetVolume(percent),
    ) && let Some(state) = state_mut(window)
    {
        state.application.set_player_volume(percent);
        if state.view == MainView::Player {
            super::refresh_player(window, state, false, true);
        }
    }
}

/// Rows of the device list: this computer first, the playing device marked.
unsafe fn device_rows(window: HWND) -> Vec<crate::spotify_devices_win32::DeviceRow> {
    let Some(state) = state(window) else {
        return Vec::new();
    };
    let texts = catalog(state);
    let devices = state
        .spotify
        .service
        .as_ref()
        .and_then(|service| service.devices_now())
        .map(|(devices, _)| devices)
        .unwrap_or_default();
    if devices.is_empty() {
        return vec![(String::new(), texts.text("spotify_devices_none").to_owned())];
    }
    let labels = apricot_app::spotify::device_rows(&texts, &devices);
    devices
        .into_iter()
        .map(|device| device.id)
        .zip(labels)
        .collect()
}

/// Moves playback to the device `id`; `false` keeps the dialog open (nothing
/// plays, or the device already plays) after saying why.
unsafe fn transfer_to(window: HWND, id: &str) -> bool {
    let Some(service) = service(window) else {
        return false;
    };
    let Some(state) = state_mut(window) else {
        return false;
    };
    let texts = catalog(state);
    let Some((devices, playing)) = service.devices_now() else {
        return false;
    };
    let Some(device) = devices.into_iter().find(|device| device.id == id) else {
        return false;
    };
    if !playing {
        let text = texts.text("spotify_nothing_playing").to_owned();
        announce(window, &text);
        return false;
    }
    if device.active {
        let text = named(&texts, "spotify_device_already", &device.name);
        announce(window, &text);
        return false;
    }
    let stamp = state.spotify.epochs.begin();
    state.spotify.transfer = Some((stamp, device.name));
    service.transfer_to(stamp, id);
    true
}

/// `spotify_devices` and the hub entry: the Connect devices in a dialog over
/// the current screen, so playback goes on.
pub(super) unsafe fn show_devices(window: HWND) {
    stop_controlled_repeat(window);
    let accounts = accounts(window);
    let Some(service) = service(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.spotify.devices_dialog.is_some() {
        return;
    }
    let texts = catalog(state);
    if apricot_app::spotify::active_account(&accounts).is_none() {
        let text = texts.text("spotify_log_in_first").to_owned();
        announce(window, &text);
        return;
    }
    if service.devices_now().is_none() {
        let text = texts.text("spotify_not_connected").to_owned();
        announce(window, &text);
        return;
    }
    let labels = crate::spotify_devices_win32::SpotifyDevicesDialogLabels {
        title: texts.text("spotify_devices").to_owned(),
        instructions: texts.text("spotify_devices_instructions").to_owned(),
        play: texts.text("spotify_devices_play").to_owned(),
        back: texts.text("back").to_owned(),
    };
    let rows = device_rows(window);
    // SAFETY: Runs on this window's thread while the dialog is open.
    let play = Box::new(move |id: &str| unsafe { transfer_to(window, id) });
    if let Some(state) = state_mut(window) {
        state.modal_open = true;
    }
    let result = crate::spotify_devices_win32::show(window, rows, labels, play, |dialog| {
        if let Some(state) = state_mut(window) {
            state.spotify.devices_dialog = Some(dialog);
        }
    });
    let Some(state) = state_mut(window) else {
        return;
    };
    state.spotify.devices_dialog = None;
    state.modal_open = false;
    resume_deferred_window_work(window);
    if let Err(error) = result {
        let message = format!("Spotify devices did not open: {error}");
        show_error_message(window, &message);
    }
    let _ = SetFocus(Some(super::active_primary_control(state)));
}

/// Spotify moved on to the next track (end of a track, Next, Previous, the
/// queue, or another device) while Apricot plays a Spotify item: the player
/// keeps running and only the item changes, with one "Playing: title"
/// announcement as for any started item. `false` when no Spotify item
/// plays, so the track is taken over as a new item.
unsafe fn continue_spotify_item(window: HWND, track: &SpotifyTrack) -> bool {
    let Some(state) = state_mut(window) else {
        return false;
    };
    let session = state.application.player_session();
    let running = matches!(
        session.phase(),
        apricot_app::player_session::PlaybackPhase::Starting
            | apricot_app::player_session::PlaybackPhase::Playing
            | apricot_app::player_session::PlaybackPhase::Paused
    ) && session
        .current_item()
        .is_some_and(|item| item.source == apricot_core::MediaSource::Spotify);
    if !running {
        return false;
    }
    let mut item = track.media_item();
    // The context and occurrence, so Play from the start keeps the context.
    if let Some(player) = state
        .spotify
        .service
        .as_ref()
        .and_then(|service| service.playback())
        .and_then(|playback| playback.player_state())
    {
        if !player.context_uri.is_empty() && player.context_uri != track.uri {
            item.metadata.insert(
                "spotify_context".to_owned(),
                serde_json::Value::String(player.context_uri.clone()),
            );
        }
        if let Some(current) = player
            .track
            .as_ref()
            .filter(|current| current.uri == track.uri)
        {
            item.metadata.insert(
                "spotify_uid".to_owned(),
                serde_json::Value::String(current.uid.clone()),
            );
        }
    }
    if !state.application.replace_current_player_item(item.clone()) {
        return false;
    }
    let message =
        super::catalog_text(&state.application, "playing").replace("{title}", &item.title);
    set_status(state, &message, true);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64());
    if let Err(error) = state.application.record_history(item, "played", timestamp) {
        let message = format!("History was not saved: {error}");
        set_status(state, &message, true);
    }
    if state.view == MainView::Player {
        super::refresh_player(window, state, false, true);
    } else if let Some(model) = state.application.player_screen_model() {
        // The window title names the playing item on every screen.
        let title = wide(&model.window_title);
        let _ = SetWindowTextW(window, PCWSTR(title.as_ptr()));
    }
    true
}

/// Ctrl+PageUp/PageDown, Shift+S and R while a Spotify item plays: Connect
/// does them, so they follow the Spotify context and reach the phone too.
pub(super) unsafe fn transport(window: HWND, action_id: &str) -> bool {
    if !matches!(
        action_id,
        "player_next" | "player_previous" | "player_shuffle" | "player_repeat"
    ) {
        return false;
    }
    let Some(state) = state(window) else {
        return false;
    };
    let session = state.application.player_session();
    let spotify = session.is_open()
        && session
            .current_item()
            .is_some_and(|item| item.source == apricot_core::MediaSource::Spotify);
    let Some(playback) = state
        .spotify
        .service
        .as_ref()
        .and_then(|service| service.playback())
        .filter(|_| spotify)
    else {
        return false;
    };
    let player = playback.player_state().unwrap_or_default();
    let key = match action_id {
        "player_next" => {
            playback.next();
            None
        }
        "player_previous" => {
            playback.previous();
            None
        }
        "player_shuffle" => {
            let shuffle = !apricot_spotify::queue::shuffle(&player);
            playback.set_shuffle(shuffle);
            Some(if shuffle { "shuffle_on" } else { "shuffle_off" })
        }
        _ => {
            let (mode, key) = match apricot_spotify::queue::repeat_mode(&player) {
                RepeatMode::Off => (RepeatMode::Context, "spotify_repeat_context"),
                RepeatMode::Context => (RepeatMode::Track, "spotify_repeat_track"),
                RepeatMode::Track => (RepeatMode::Off, "repeat_off"),
            };
            playback.set_repeat(mode);
            Some(key)
        }
    };
    if let Some(key) = key {
        let text = catalog(state).text(key).to_owned();
        announce(window, &text);
    }
    true
}

/// Ctrl+Shift+Q on a Spotify track or episode (the active item: the
/// playing one in the player, the selected one in Spotify lists). It joins
/// the manually added tracks of the Spotify queue, which exists while
/// Spotify plays here. `false` for other targets.
pub(super) unsafe fn add_to_queue(window: HWND) -> bool {
    let Some(state) = state(window) else {
        return false;
    };
    let reference = super::spotify_browse::selected_item(state)
        .map(|item| item.uri.clone())
        .or_else(|| {
            super::active_media_item(window)
                .filter(|item| item.source == apricot_core::MediaSource::Spotify)
                .map(|item| item.id.0)
        })
        .and_then(|uri| SpotifyRef::parse(&uri));
    let Some(reference) = reference.filter(|reference| {
        matches!(
            reference.kind,
            SpotifyEntityKind::Track | SpotifyEntityKind::Episode
        )
    }) else {
        return false;
    };
    let texts = catalog(state);
    let playback = state
        .spotify
        .service
        .as_ref()
        .and_then(|service| service.playback())
        .filter(|playback| {
            playback
                .player_state()
                .is_some_and(|player| apricot_spotify::queue::snapshot(&player).active)
        });
    let key = if let Some(playback) = playback {
        playback.add_to_queue(reference.to_uri());
        "spotify_queue_added"
    } else {
        "spotify_queue_inactive"
    };
    let text = texts.text(key).to_owned();
    announce(window, &text);
    true
}

/// `spotify_queue` and the hub entry: the queue of this Connect device in a
/// dialog over the current screen, so playback goes on.
pub(super) unsafe fn show_queue(window: HWND) {
    stop_controlled_repeat(window);
    let accounts = accounts(window);
    let Some(service) = service(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.spotify.queue_dialog.is_some() {
        return;
    }
    let texts = catalog(state);
    if apricot_app::spotify::active_account(&accounts).is_none() {
        let text = texts.text("spotify_log_in_first").to_owned();
        announce(window, &text);
        return;
    }
    let Some(playback) = service.playback() else {
        let text = texts.text("spotify_queue_inactive").to_owned();
        announce(window, &text);
        return;
    };
    let labels = crate::spotify_queue_win32::SpotifyQueueDialogLabels {
        title: texts.text("spotify_queue").to_owned(),
        instructions: texts.text("playback_queue_instructions").to_owned(),
        play: texts.text("play").to_owned(),
        move_up: texts.text("move_up").to_owned(),
        move_down: texts.text("move_down").to_owned(),
        remove: texts.text("spotify_queue_remove").to_owned(),
        clear: texts.text("spotify_queue_clear").to_owned(),
        back: texts.text("back").to_owned(),
        removed: texts.text("spotify_queue_removed").to_owned(),
        moved: texts.text("spotify_queue_moved").to_owned(),
        cleared: texts.text("spotify_queue_cleared").to_owned(),
        changed: texts.text("spotify_queue_changed").to_owned(),
    };
    let rows = queue_rows(&texts, &service.queue_now());
    let actions = crate::spotify_queue_win32::SpotifyQueueActions {
        playback,
        // SAFETY: Runs on this window's thread while the dialog is open.
        reload: Box::new(move || unsafe { reload_queue(window) }),
    };
    state.modal_open = true;
    let result = crate::spotify_queue_win32::show(window, rows, labels, actions, |dialog| {
        if let Some(state) = state_mut(window) {
            state.spotify.queue_dialog = Some(dialog);
        }
        // Titles not known yet arrive with the confirmed queue.
        reload_queue(window);
    });
    let Some(state) = state_mut(window) else {
        return;
    };
    state.spotify.queue_dialog = None;
    state.spotify.queue_load = None;
    state.modal_open = false;
    resume_deferred_window_work(window);
    if let Err(error) = result {
        let message = format!("Spotify queue did not open: {error}");
        show_error_message(window, &message);
    }
    let _ = SetFocus(Some(super::active_primary_control(state)));
}

unsafe fn reload_queue(window: HWND) {
    let Some(service) = service(window) else {
        return;
    };
    if let Some(state) = state_mut(window) {
        let stamp = state.spotify.epochs.begin();
        state.spotify.queue_load = Some(stamp);
        service.load_queue(stamp);
    }
}
