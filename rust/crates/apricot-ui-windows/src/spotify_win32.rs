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
};
use apricot_core::{
    Route, RouteFrame, SpotifyEntityKind, SpotifyEpochs, SpotifyRef, SpotifyStamp,
    TranslationCatalog,
};
use apricot_spotify::{
    CallbackPage, PlaybackNotice, SpotifyAccounts, SpotifyError, SpotifyEvent, SpotifyService,
    SpotifyTrack,
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::{
            Input::KeyboardAndMouse::{GetFocus, SetFocus},
            Shell::ShellExecuteW,
            WindowsAndMessaging::{
                IDYES, IsWindow, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, MB_ICONQUESTION,
                MB_YESNO, MessageBoxW, PostMessageW, SW_SHOWNORMAL, SendMessageW,
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
    polling: bool,
}

pub(super) const fn is_view(view: MainView) -> bool {
    matches!(view, MainView::SpotifyHub | MainView::SpotifyAccounts)
}

fn catalog(state: &WindowState) -> TranslationCatalog {
    apricot_app::embedded_catalog(&state.application.settings().language)
}

unsafe fn announce(window: HWND, text: &str) {
    if let Some(state) = state(window) {
        set_status(state, text, true);
    }
}

/// The runtime starts on first use, not with the application.
unsafe fn service(window: HWND) -> Option<Arc<SpotifyService>> {
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

unsafe fn prepare_screen(window: HWND, main_view: MainView, route: Route) {
    restore_from_tray(window);
    stop_controlled_repeat(window);
    let Some(state) = state_mut(window) else {
        return;
    };
    cancel_youtube_work(window, state);
    cancel_local_folder_scan(window, state);
    if state.application.current_route() != route {
        state.application.navigate_main_menu();
        if route == Route::SpotifyAccounts {
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
            if !state.spotify.epochs.is_latest(stamp) {
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
                Ok(track) => play_track(window, &track),
                Err(error) => {
                    let text = error_text(&texts, &error);
                    show_error_message(window, &text);
                }
            }
        }
    }
}

/// Back and Escape: accounts return to the hub, the hub to the main menu.
pub(super) unsafe fn back(window: HWND) {
    match state(window).map(|state| state.view) {
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
        SpotifyEntityKind::Track | SpotifyEntityKind::Episode
    ) {
        let text = texts
            .text("rust_feature_unavailable")
            .replace("{feature}", texts.text("spotify"));
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
unsafe fn play_track(window: HWND, track: &SpotifyTrack) {
    let item = track.media_item();
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
            requested: true, ..
        } => {}
        PlaybackNotice::Playing {
            track,
            position_ms,
            requested: false,
        } => {
            let mut item = track.media_item();
            item.metadata
                .insert("spotify_attach".to_owned(), serde_json::Value::Bool(true));
            state.background_start = state.view != MainView::Player;
            // The player's own "Playing: <title>" is the one announcement.
            start_media_item_at(window, item, f64::from(position_ms) / 1000.0);
        }
        PlaybackNotice::Unavailable { .. } => {
            let text = texts.text("spotify_track_unavailable").to_owned();
            set_status(state, &text, true);
        }
    }
}
