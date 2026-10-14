//! Spotify (Windows `spotify_win32.rs`, `docs/SPOTIFY_PLAN.md` 4.1, 9.1):
//! the hub opened from the main menu, the account list, the browser login,
//! the Connect session, Spotify links, transport keys, the volume link with
//! Connect and the Spotify settings. The session runtime is
//! `apricot-spotify`; its events are polled with the other background work.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use apricot_app::spotify::{
    SpotifyAccountRow, SpotifyHubEntry, SpotifyHubModel, account_rows, error_text, named,
};
use apricot_core::{
    MediaItem, Route, RouteFrame, SpotifyEntityKind, SpotifyEpochs, SpotifyRef, SpotifyStamp,
};
use apricot_playback::PlaybackCommand;
use apricot_spotify::settings::SpotifySettings;
use apricot_spotify::{
    CallbackPage, PlaybackNotice, RepeatMode, SpotifyAccounts, SpotifyError, SpotifyEvent,
    SpotifyService, SpotifyTrack,
};

use crate::{ListScreen, PlayerFocus, ScreenControl, Shell, ShellUi, View};

/// Where a finished login returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReturnTo {
    Hub,
    Accounts,
}

/// The login dialog's texts (Windows `LoginTexts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotifyLoginTexts {
    pub title: String,
    pub status_name: String,
    pub waiting: String,
    pub open_browser: String,
    pub copy_link: String,
    pub retry: String,
    pub cancel: String,
}

/// Progress for an open login dialog (Windows `LoginUpdate`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpotifyLoginUpdate {
    /// The login failed: the text replaces the status, Try again is enabled
    /// and focused.
    Failed(String),
    /// A new attempt started.
    Waiting,
    /// Logged in; the dialog closes.
    Succeeded,
}

/// A button of the login dialog.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpotifyLoginAction {
    OpenBrowser,
    CopyLink,
    Retry,
    Cancel,
}

/// The Spotify settings dialog's texts (Windows `SettingsTexts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotifySettingsTexts {
    pub title: String,
    pub quality: String,
    pub qualities: [String; 3],
    pub normalisation: String,
    pub listening_history: String,
    pub experimental_warning: String,
    pub yes: String,
    pub no: String,
    pub autoplay: String,
    pub autoplays: [String; 3],
    pub order: String,
    pub orders: [String; 5],
    pub ok: String,
    pub cancel: String,
}

/// The selected row of the account list (Windows `SelectedRow`).
pub(crate) enum SelectedAccount {
    AddAccount,
    Account { logged_in: bool, active: bool },
}

impl SelectedAccount {
    /// The shape `spotify_accounts_context_menu` takes.
    pub(crate) const fn menu_account(&self) -> Option<(bool, bool)> {
        match self {
            Self::AddAccount => None,
            Self::Account { logged_in, active } => Some((*logged_in, *active)),
        }
    }
}

/// Windows `SpotifyState`.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools)] // As on Windows.
pub(crate) struct SpotifyState {
    service: Option<Arc<SpotifyService>>,
    receiver: Option<Receiver<SpotifyEvent>>,
    pub(crate) epochs: SpotifyEpochs,
    /// ID of the login the dialog is waiting for; older results are stale.
    login: u64,
    login_url: Option<String>,
    login_open: bool,
    login_return: Option<ReturnTo>,
    login_account: Option<String>,
    pub(crate) hub_entries: Vec<SpotifyHubEntry>,
    pub(crate) hub_pins: Vec<apricot_spotify::main_menu::MenuPin>,
    pub(crate) hub_pin_items: Vec<apricot_spotify::CatalogItem>,
    hub_labels: Vec<String>,
    pub(crate) hub_selected: Option<SpotifyHubEntry>,
    rows: Vec<SpotifyAccountRow>,
    /// The switch the user asked for; only its result is announced.
    connect: Option<SpotifyStamp>,
    /// A link waiting for the connection of the active account.
    pending_play: Option<String>,
    /// The link whose metadata is being read; only the latest one plays.
    resolve: Option<SpotifyStamp>,
    polling: bool,
    /// The last automatic reconnection after a lost session (beta 6, 4a.1).
    reconnected_at: Option<std::time::Instant>,
    /// The pending request for smart shuffle recommendations, when the
    /// last one started, and the tracks already recommended.
    smart_request: Option<SpotifyStamp>,
    smart_requested_at: Option<std::time::Instant>,
    smart_used: Vec<String>,
    /// The transfer the user asked for and the device's name.
    pub(crate) transfer: Option<(SpotifyStamp, String)>,
    /// The open queue dialog's reload.
    pub(crate) queue_open: bool,
    pub(crate) queue_load: Option<SpotifyStamp>,
    pub(crate) devices_open: bool,
    /// The open Spotify lists (Windows `BrowseState`).
    pub(crate) browse: crate::spotify_lists::BrowseState,
    /// The rows of the open queue or device dialog.
    pub(crate) dialog_rows: crate::spotify_panels::DialogRows,
}

/// Whether an automatic reconnection may start now; records it.
fn reconnect_due(last: &mut Option<std::time::Instant>) -> bool {
    let due = last.is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(60));
    if due {
        *last = Some(std::time::Instant::now());
    }
    due
}

impl SpotifyState {
    pub(crate) const fn service_running(&self) -> bool {
        self.service.is_some()
    }

    /// Windows `is_current_account`: answers of an earlier account are
    /// dropped.
    pub(crate) const fn is_current_account(&self, stamp: SpotifyStamp) -> bool {
        self.epochs.is_current_account(stamp)
    }

    /// Windows `next_account` (beta 2): another account forgets the
    /// requests, caches and lists of the previous one.
    pub(crate) fn next_account(&mut self) {
        self.epochs.next_account();
        self.resolve = None;
        self.pending_play = None;
        self.queue_load = None;
        self.transfer = None;
        self.smart_request = None;
        self.smart_requested_at = None;
        self.smart_used.clear();
        self.browse.clear_account();
    }
}

pub(crate) const fn is_view(view: View) -> bool {
    matches!(
        view,
        View::SpotifyHub | View::SpotifyAccounts | View::SpotifyBrowse
    )
}

fn display_name(accounts: &SpotifyAccounts, key: &str) -> String {
    accounts
        .accounts
        .iter()
        .find(|account| account.key == key)
        .map(|account| account.display_name.clone())
        .unwrap_or_default()
}

/// Debug builds on a test run must never start the user's browser.
fn open_login_url(url: &str) {
    if cfg!(debug_assertions) && std::env::var_os("APRICOT_SPOTIFY_TEST_NO_BROWSER").is_some() {
        return;
    }
    let _ = apricot_platform::shell::open_url(url);
}

impl Shell {
    /// The runtime starts on first use, not with the application.
    pub(crate) fn spotify_service(&mut self) -> Option<Arc<SpotifyService>> {
        if self.spotify.service.is_none() {
            let app_data = apricot_platform::discover_app_paths().ok()?.app_data;
            // The events are polled with the other background work.
            let (service, receiver) = SpotifyService::new(&app_data, Arc::new(|| {}));
            self.spotify.service = Some(Arc::new(service));
            self.spotify.receiver = Some(receiver);
        }
        self.spotify.service.clone()
    }

    /// The runtime when it already runs.
    pub(crate) fn spotify_service_ref(&self) -> Option<&SpotifyService> {
        self.spotify.service.as_deref()
    }

    fn spotify_accounts(&mut self) -> SpotifyAccounts {
        self.spotify_service()
            .map(|service| service.accounts())
            .unwrap_or_default()
    }

    /// Windows `prepare_screen`.
    pub(crate) fn spotify_prepare_screen(
        &mut self,
        ui: &mut dyn ShellUi,
        view: View,
        route: Route,
    ) {
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != route {
            self.application.navigate_main_menu();
            if matches!(route, Route::SpotifyAccounts | Route::SpotifyBrowse) {
                self.application
                    .navigate_to(RouteFrame::new(Route::SpotifyHub));
            }
            self.application.navigate_to(RouteFrame::new(route));
        }
        self.view = view;
    }

    /// The hub and account lists: the list, Open and Back in the Windows
    /// creation order.
    fn spotify_list_screen(&self, name: &str, rows: Vec<String>, selected: usize) -> ListScreen {
        let button = |id: &'static str, key: &str| ScreenControl::Button {
            id,
            label: self.text(key),
        };
        ListScreen {
            list_name: name.to_owned(),
            rows,
            selected: Some(selected),
            controls: vec![
                ScreenControl::List,
                button("open", "open"),
                button("back", "back"),
            ],
            focus_list: true,
        }
    }

    /// Main menu Spotify, `open_spotify`.
    pub(crate) fn show_spotify_hub(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("spotify");
        let accounts = self.spotify_accounts();
        self.spotify_prepare_screen(ui, View::SpotifyHub, Route::SpotifyHub);
        let catalog = self.catalog();
        let settings = self.application.settings();
        let pins = self
            .spotify_service_ref()
            .and_then(|service| service.menu_pins().ok())
            .unwrap_or_default();
        self.spotify.hub_pin_items = pins
            .iter()
            .map(apricot_spotify::main_menu::MenuPin::item)
            .collect();
        self.spotify.hub_pins.clone_from(&pins);
        let model = SpotifyHubModel::build(
            &catalog,
            &accounts,
            settings.show_shortcuts_in_labels,
            &settings.keyboard_shortcuts,
        )
        .with_pins(&pins);
        self.spotify.hub_entries = model.items.iter().map(|item| item.entry).collect();
        let selected = self
            .spotify
            .hub_selected
            .and_then(|entry| self.spotify.hub_entries.iter().position(|e| *e == entry))
            .unwrap_or(0);
        let rows: Vec<String> = model.items.into_iter().map(|item| item.label).collect();
        self.spotify.hub_labels.clone_from(&rows);
        let screen = self.spotify_list_screen(&model.accessible_name, rows, selected);
        ui.show_list_screen(&screen);
    }

    /// `spotify_accounts`; `select` keeps an account selected after a
    /// change.
    pub(crate) fn show_spotify_accounts(&mut self, ui: &mut dyn ShellUi, select: Option<&str>) {
        self.spotify_prepare_screen(ui, View::SpotifyAccounts, Route::SpotifyAccounts);
        self.spotify_refresh_accounts(ui, select, true);
    }

    /// Refills the account list in place; with `focus` the list gets it.
    fn spotify_refresh_accounts(
        &mut self,
        ui: &mut dyn ShellUi,
        select: Option<&str>,
        focus: bool,
    ) {
        let accounts = self.spotify_accounts();
        let catalog = self.catalog();
        let rows = account_rows(&catalog, &accounts);
        let wanted = select.or(accounts.active.as_deref());
        let selected = wanted
            .and_then(|key| rows.iter().position(|row| row.key() == Some(key)))
            .unwrap_or(0);
        let labels = rows.iter().map(|row| row.label().to_owned()).collect();
        self.spotify.rows = rows;
        self.spotify.hub_selected = Some(SpotifyHubEntry::Accounts);
        let mut screen =
            self.spotify_list_screen(catalog.text("spotify_accounts"), labels, selected);
        screen.focus_list = focus;
        ui.show_list_screen(&screen);
    }

    fn spotify_remember_hub_entry(&mut self, entry: SpotifyHubEntry) {
        self.spotify.hub_selected = Some(entry);
    }

    /// Enter and Open on the hub and the account list.
    pub(crate) fn spotify_activate(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = ui.selected_row() else {
            return;
        };
        match self.view {
            View::SpotifyHub => match self.spotify.hub_entries.get(index).copied() {
                Some(SpotifyHubEntry::LogIn) => {
                    self.spotify.hub_selected = Some(SpotifyHubEntry::LogIn);
                    self.spotify_log_in(ui, ReturnTo::Hub);
                }
                Some(SpotifyHubEntry::Accounts) => self.show_spotify_accounts(ui, None),
                Some(SpotifyHubEntry::Settings) => {
                    self.spotify_remember_hub_entry(SpotifyHubEntry::Settings);
                    self.show_spotify_settings(ui);
                }
                Some(SpotifyHubEntry::Queue) => {
                    self.spotify.hub_selected = Some(SpotifyHubEntry::Queue);
                    self.show_spotify_queue(ui);
                }
                Some(SpotifyHubEntry::Devices) => {
                    self.spotify.hub_selected = Some(SpotifyHubEntry::Devices);
                    self.show_spotify_devices(ui);
                }
                Some(entry) => {
                    self.spotify_remember_hub_entry(entry);
                    let label = self
                        .spotify
                        .hub_labels
                        .get(index)
                        .cloned()
                        .unwrap_or_default();
                    self.spotify_open_hub_list(ui, entry, &label);
                }
                None => {}
            },
            View::SpotifyAccounts => match self.spotify.rows.get(index).cloned() {
                Some(SpotifyAccountRow::AddAccount { .. }) => {
                    self.spotify_log_in(ui, ReturnTo::Accounts);
                }
                Some(SpotifyAccountRow::Account { key, .. }) => {
                    let accounts = self.spotify_accounts();
                    let Some(account) = accounts.accounts.iter().find(|a| a.key == key) else {
                        return;
                    };
                    if !account.is_logged_in() {
                        self.spotify_log_in(ui, ReturnTo::Accounts);
                    } else if accounts.active.as_deref() != Some(key.as_str()) {
                        self.spotify_use_account(ui, &key);
                    } else {
                        let text =
                            named(&self.catalog(), "spotify_connected", &account.display_name);
                        ui.set_status(&text, true);
                    }
                }
                None => {}
            },
            _ => {}
        }
    }

    /// The selected account row, for the context menu: `None` for Add
    /// account, else whether it is logged in and active.
    pub(crate) fn spotify_selected_account(&mut self, ui: &dyn ShellUi) -> Option<SelectedAccount> {
        if self.view != View::SpotifyAccounts {
            return None;
        }
        let row = self.spotify.rows.get(ui.selected_row()?)?.clone();
        let Some(key) = row.key() else {
            return Some(SelectedAccount::AddAccount);
        };
        let accounts = self.spotify_accounts();
        let account = accounts.accounts.iter().find(|a| a.key == key)?;
        Some(SelectedAccount::Account {
            logged_in: account.is_logged_in(),
            active: accounts.active.as_deref() == Some(key),
        })
    }

    fn spotify_selected_key(&self, ui: &dyn ShellUi) -> Option<String> {
        self.spotify
            .rows
            .get(ui.selected_row()?)?
            .key()
            .map(str::to_owned)
    }

    /// Context menu Use this account.
    pub(crate) fn spotify_use_selected_account(&mut self, ui: &mut dyn ShellUi) {
        if let Some(key) = self.spotify_selected_key(ui) {
            self.spotify_use_account(ui, &key);
        }
    }

    fn spotify_use_account(&mut self, ui: &mut dyn ShellUi, key: &str) {
        let Some(service) = self.spotify_service() else {
            return;
        };
        self.spotify.next_account();
        let stamp = self.spotify.epochs.begin();
        self.spotify.connect = Some(stamp);
        ui.set_status(&self.text("spotify_connecting"), false);
        service.connect(stamp, key);
        self.spotify_refresh_accounts(ui, Some(key), false);
    }

    /// Context menu Log in / Log in again.
    pub(crate) fn spotify_log_in_from_accounts(&mut self, ui: &mut dyn ShellUi) {
        self.spotify_log_in(ui, ReturnTo::Accounts);
    }

    /// Context menu Log out.
    pub(crate) fn spotify_log_out_selected(&mut self, ui: &mut dyn ShellUi) {
        let Some(key) = self.spotify_selected_key(ui) else {
            return;
        };
        let Some(service) = self.spotify_service() else {
            return;
        };
        let name = display_name(&service.accounts(), &key);
        self.spotify.next_account();
        self.spotify.connect = None;
        match service.logout(&key) {
            Ok(_) => {
                let text = named(&self.catalog(), "spotify_logged_out_done", &name);
                self.spotify_refresh_accounts(ui, Some(&key), false);
                ui.set_status(&text, true);
            }
            Err(error) => self.spotify_report_error(ui, &error),
        }
    }

    /// Context menu Remove account and Delete on an account row.
    pub(crate) fn spotify_remove_selected(&mut self, ui: &mut dyn ShellUi) {
        let Some(key) = self.spotify_selected_key(ui) else {
            return;
        };
        let Some(service) = self.spotify_service() else {
            return;
        };
        let name = display_name(&service.accounts(), &key);
        let catalog = self.catalog();
        let question = named(&catalog, "spotify_remove_account_confirm", &name);
        if !ui.ask_yes_no(&question, &self.text("yes"), &self.text("no")) {
            return;
        }
        self.spotify.next_account();
        self.spotify.connect = None;
        match service.remove(&key) {
            Ok(remaining) => {
                let text = named(&catalog, "spotify_account_removed", &name);
                if remaining.accounts.is_empty() {
                    self.show_spotify_hub(ui);
                } else {
                    self.spotify_refresh_accounts(ui, remaining.active.as_deref(), false);
                }
                ui.set_status(&text, true);
            }
            Err(error) => self.spotify_report_error(ui, &error),
        }
    }

    pub(crate) fn spotify_report_error(&self, ui: &mut dyn ShellUi, error: &SpotifyError) {
        ui.show_error(&error_text(&self.catalog(), error));
    }

    // -----------------------------------------------------------------------
    // Login

    fn spotify_start_login(&mut self) {
        let Some(service) = self.spotify_service() else {
            return;
        };
        self.spotify.login += 1;
        self.spotify.login_url = None;
        let catalog = self.catalog();
        let page = CallbackPage::from_texts(
            catalog.selected_code(),
            catalog.text("spotify_browser_success"),
            catalog.text("spotify_browser_failure"),
        );
        service.begin_login(self.spotify.login, page);
    }

    /// Windows `log_in`: the login dialog over the window; the browser
    /// opens with the login page.
    fn spotify_log_in(&mut self, ui: &mut dyn ShellUi, return_to: ReturnTo) {
        if self.spotify_service().is_none() || self.spotify.login_open {
            return;
        }
        let catalog = self.catalog();
        let texts = SpotifyLoginTexts {
            title: catalog.text("spotify_login").to_owned(),
            status_name: catalog.text("spotify_login").to_owned(),
            waiting: catalog.text("spotify_login_waiting").to_owned(),
            open_browser: catalog.text("spotify_open_browser_again").to_owned(),
            copy_link: catalog.text("spotify_copy_login_link").to_owned(),
            retry: catalog.text("spotify_retry").to_owned(),
            cancel: catalog.text("cancel").to_owned(),
        };
        self.spotify.login_account = None;
        self.spotify.login_open = true;
        self.spotify.login_return = Some(return_to);
        ui.open_spotify_login(&texts);
        self.spotify_start_login();
        ui.set_background_polling(true);
    }

    /// A button of the login dialog (Windows `LoginActions`).
    pub fn spotify_login_action(&mut self, ui: &mut dyn ShellUi, action: SpotifyLoginAction) {
        match action {
            SpotifyLoginAction::OpenBrowser => {
                if let Some(url) = self.spotify.login_url.clone() {
                    open_login_url(&url);
                }
            }
            SpotifyLoginAction::CopyLink => {
                let Some(url) = self.spotify.login_url.clone() else {
                    return;
                };
                if ui.copy_text(&url) {
                    ui.set_status(&self.text("spotify_login_link_copied"), true);
                }
            }
            SpotifyLoginAction::Retry => {
                ui.spotify_login_update(&SpotifyLoginUpdate::Waiting);
                self.spotify_start_login();
            }
            SpotifyLoginAction::Cancel => {
                // Any late result of this login is now stale.
                self.spotify.login += 1;
                if let Some(service) = &self.spotify.service {
                    service.cancel_login();
                }
            }
        }
    }

    /// The login dialog closed; `succeeded` when it closed after a login.
    pub fn spotify_login_closed(&mut self, ui: &mut dyn ShellUi, succeeded: bool) {
        self.spotify.login_open = false;
        let name = self.spotify.login_account.take();
        let return_to = self.spotify.login_return.take().unwrap_or(ReturnTo::Hub);
        if !succeeded {
            ui.set_status(&self.text("spotify_login_cancelled"), true);
            return;
        }
        let key = self
            .spotify
            .service
            .as_ref()
            .and_then(|service| service.accounts().active);
        // The new login is the active session.
        self.spotify.next_account();
        self.spotify.connect = None;
        self.spotify.hub_selected = Some(SpotifyHubEntry::Accounts);
        match return_to {
            ReturnTo::Hub => self.show_spotify_hub(ui),
            ReturnTo::Accounts => self.show_spotify_accounts(ui, key.as_deref()),
        }
        let text = named(
            &self.catalog(),
            "spotify_logged_in",
            name.as_deref().unwrap_or(""),
        );
        ui.set_status(&text, true);
    }

    // -----------------------------------------------------------------------
    // Events

    /// Windows `WM_SPOTIFY_EVENT`; `true` while the runtime runs.
    pub(crate) fn poll_spotify(&mut self, ui: &mut dyn ShellUi) -> bool {
        if self.spotify.service.is_none() {
            return false;
        }
        if self.spotify.polling {
            return true;
        }
        self.spotify_browse_refresh_tick();
        loop {
            let event = self
                .spotify
                .receiver
                .as_ref()
                .and_then(|receiver| receiver.try_recv().ok());
            let notice = if event.is_none() {
                self.spotify
                    .service
                    .as_ref()
                    .and_then(|service| service.take_notice())
            } else {
                None
            };
            if event.is_none() && notice.is_none() {
                break;
            }
            self.spotify.polling = true;
            if let Some(event) = event {
                self.spotify_handle_event(ui, event);
            }
            if let Some(notice) = notice {
                self.spotify_handle_notice(ui, notice);
            }
            self.spotify.polling = false;
        }
        true
    }

    #[allow(clippy::too_many_lines)]
    fn spotify_handle_event(&mut self, ui: &mut dyn ShellUi, event: SpotifyEvent) {
        let texts = self.catalog();
        match event {
            SpotifyEvent::LoginUrl { login, url } => {
                if login != self.spotify.login {
                    return;
                }
                self.spotify.login_url = Some(url.clone());
                open_login_url(&url);
            }
            SpotifyEvent::LoginFinished { login, result } => {
                if login != self.spotify.login || !self.spotify.login_open {
                    return;
                }
                match result {
                    Ok(account) => {
                        self.spotify.login_account = Some(account.display_name);
                        ui.spotify_login_update(&SpotifyLoginUpdate::Succeeded);
                    }
                    Err(SpotifyError::Cancelled) => {}
                    Err(error) => {
                        let text = error_text(&texts, &error);
                        ui.spotify_login_update(&SpotifyLoginUpdate::Failed(text.clone()));
                        ui.set_status(&text, true);
                    }
                }
            }
            SpotifyEvent::Connected { stamp, result } => {
                // Every connect starts a new account epoch; later requests of
                // other kinds do not make it stale.
                if !self.spotify.epochs.is_current_account(stamp) {
                    return;
                }
                let requested = self.spotify.connect.take() == Some(stamp);
                let text = match &result {
                    Ok(account) => named(&texts, "spotify_connected", &account.display_name),
                    Err(error) => error_text(&texts, error),
                };
                if self.view == View::SpotifyAccounts {
                    // The selection stays on the row the user is on.
                    let key = self.spotify_selected_key(ui);
                    self.spotify_refresh_accounts(ui, key.as_deref(), false);
                }
                let pending = self.spotify.pending_play.take();
                if requested || result.is_err() {
                    ui.set_status(&text, true);
                }
                if let (Ok(_), Some(uri)) = (&result, pending) {
                    self.spotify_resolve(ui, &uri);
                }
            }
            SpotifyEvent::Disconnected { session_id } => {
                // The end of a replaced session is not a disconnection.
                if self
                    .spotify
                    .service
                    .as_ref()
                    .is_none_or(|service| !service.is_current_session(session_id))
                {
                    return;
                }
                ui.set_status(texts.text("spotify_disconnected"), true);
                // A session lost while idle (network change, sleep) connects
                // again on its own, at most once a minute, so the account
                // does not stay "not logged in" until a restart.
                if reconnect_due(&mut self.spotify.reconnected_at) {
                    self.spotify_autoconnect();
                }
            }
            SpotifyEvent::Resolved { stamp, result } => {
                if !self.spotify.is_current_account(stamp) || self.spotify.resolve != Some(stamp) {
                    return;
                }
                self.spotify.resolve = None;
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
                        self.spotify_play_track(ui, item);
                    }
                    Err(error) => ui.show_error(&error_text(&texts, &error)),
                }
            }
            SpotifyEvent::Catalog { stamp, result } => {
                self.spotify_browse_loaded(ui, stamp, result);
            }
            SpotifyEvent::Edited { stamp, result } => {
                self.spotify_browse_edited(ui, stamp, result);
            }
            SpotifyEvent::SmartShuffle {
                stamp,
                playlist,
                result,
            } => self.spotify_smart_tracks(ui, stamp, &playlist, result.unwrap_or_default()),
            SpotifyEvent::Transferred { stamp, result } => {
                if !self.spotify.is_current_account(stamp) {
                    return;
                }
                let Some((wanted, name)) = self.spotify.transfer.take() else {
                    return;
                };
                if wanted != stamp {
                    self.spotify.transfer = Some((wanted, name));
                    return;
                }
                let text = match result {
                    Ok(()) => named(&texts, "spotify_transferred", &name),
                    Err(error) => error_text(&texts, &error),
                };
                ui.set_status(&text, true);
            }
            SpotifyEvent::Queue { stamp, queue } => {
                if !self.spotify.is_current_account(stamp) || self.spotify.queue_load != Some(stamp)
                {
                    return;
                }
                self.spotify.queue_load = None;
                self.spotify_queue_arrived(ui, apricot_app::spotify::queue_rows(&texts, &queue));
            }
        }
    }

    fn spotify_smart_tracks(
        &mut self,
        ui: &mut dyn ShellUi,
        stamp: SpotifyStamp,
        playlist: &str,
        tracks: Vec<apricot_spotify::smart_shuffle::SmartTrack>,
    ) {
        if !self.spotify.is_current_account(stamp) || self.spotify.smart_request != Some(stamp) {
            return;
        }
        self.spotify.smart_request = None;
        let Some(playback) = self.spotify.service.as_ref().and_then(|s| s.playback()) else {
            return;
        };
        if playback.smart_shuffle().as_deref() != Some(playlist) {
            return;
        }
        let used = &self.spotify.smart_used;
        let tracks: Vec<_> = tracks
            .into_iter()
            .filter(|track| !used.contains(&track.uri))
            .collect();
        if tracks.is_empty() {
            // Nothing for this playlist at all: shuffle stays on.
            if self.spotify.smart_used.is_empty() {
                playback.set_smart_shuffle(None);
                ui.set_status(&self.text("spotify_smart_shuffle_unavailable"), true);
            }
            return;
        }
        self.spotify
            .smart_used
            .extend(tracks.iter().map(|track| track.uri.clone()));
        playback.add_smart_tracks(&tracks);
    }

    /// The Spotify actions of the action list; `true` when `action_id` is
    /// one.
    pub(crate) fn spotify_action(&mut self, ui: &mut dyn ShellUi, action_id: &str) -> bool {
        let entry = match action_id {
            "open_spotify" => {
                self.show_spotify_hub(ui);
                return true;
            }
            "spotify_accounts" => {
                self.show_spotify_accounts(ui, None);
                return true;
            }
            "spotify_queue" => {
                self.show_spotify_queue(ui);
                return true;
            }
            "spotify_devices" => {
                self.show_spotify_devices(ui);
                return true;
            }
            "spotify_mark_all" => {
                self.spotify_mark_all(ui);
                return true;
            }
            "spotify_toggle_saved" => {
                self.spotify_toggle_saved(ui);
                return true;
            }
            "spotify_dislike" => {
                self.spotify_toggle_hidden(ui);
                return true;
            }
            "spotify_radio" => {
                self.spotify_radio(ui);
                return true;
            }
            "spotify_search" => SpotifyHubEntry::Search,
            "spotify_library" => SpotifyHubEntry::Library,
            "spotify_liked_songs" => SpotifyHubEntry::LikedSongs,
            "spotify_playlists" => SpotifyHubEntry::Playlists,
            "spotify_daily_mixes" => SpotifyHubEntry::DailyMixes,
            _ => return false,
        };
        self.stop_key_repeat(ui);
        let label = self.text(action_id);
        self.spotify_open_hub_list(ui, entry, &label);
        true
    }

    /// The lists need a connected account; without one, it says what to do.
    pub(crate) fn spotify_ready_for_lists(&mut self, ui: &mut dyn ShellUi) -> bool {
        let accounts = self.spotify_accounts();
        let Some(service) = self.spotify_service() else {
            return false;
        };
        if apricot_app::spotify::active_account(&accounts).is_none() {
            ui.set_status(&self.text("spotify_log_in_first"), true);
            return false;
        }
        if service.connected_account().is_none() {
            ui.set_status(&self.text("spotify_not_connected"), true);
            return false;
        }
        true
    }

    /// Windows `show_settings`: quality, normalisation, autoplay and the
    /// library order.
    pub(crate) fn show_spotify_settings(&mut self, ui: &mut dyn ShellUi) {
        let texts = self.catalog();
        let text = |key: &str| texts.text(key).to_owned();
        let settings_texts = SpotifySettingsTexts {
            title: text("spotify_settings"),
            quality: text("spotify_quality"),
            qualities: [
                text("spotify_quality_normal"),
                text("spotify_quality_high"),
                text("spotify_quality_very_high"),
            ],
            normalisation: text("spotify_normalisation"),
            listening_history: text("spotify_listening_history"),
            experimental_warning: text("spotify_listening_history_warning"),
            yes: text("yes"),
            no: text("no"),
            autoplay: text("spotify_autoplay"),
            autoplays: [
                text("spotify_autoplay_account"),
                text("spotify_autoplay_on"),
                text("spotify_autoplay_off"),
            ],
            order: text("spotify_library_order"),
            orders: [
                text("spotify_order_recents"),
                text("spotify_order_recently_added"),
                text("spotify_order_alphabetical"),
                text("spotify_order_creator"),
                text("spotify_order_custom"),
            ],
            ok: text("ok"),
            cancel: text("cancel"),
        };
        // The service sets the settings folder when it starts.
        let _ = self.spotify_service();
        let current = apricot_spotify::settings::load();
        let chosen = ui.spotify_settings(&settings_texts, current);
        ui.focus_primary_control();
        let Some(chosen) = chosen.filter(|chosen| *chosen != current) else {
            return;
        };
        if let Err(error) = apricot_spotify::settings::save(&chosen) {
            let text = texts
                .text("spotify_error_storage")
                .replace("{error}", &error);
            ui.set_status(&text, true);
            return;
        }
        self.application.forget_spotify_settings_draft();
        if chosen.listening_history != current.listening_history
            && let Some(service) = &self.spotify.service
        {
            service.set_listening_history(chosen.listening_history);
        }
        if chosen.playback() == current.playback() {
            ui.set_status(texts.text("spotify_settings_saved"), true);
            return;
        }
        let key = if self.spotify_apply_playback_settings() {
            "spotify_settings_saved"
        } else {
            "spotify_settings_saved_later"
        };
        ui.set_status(texts.text(key), true);
    }

    /// Changed playback settings apply when the session connects: with
    /// nothing of Spotify playing the connected account reconnects now.
    fn spotify_apply_playback_settings(&mut self) -> bool {
        let session = self.application.player_session();
        let spotify_plays = session.is_open()
            && session
                .current_item()
                .is_some_and(|item| item.source == apricot_core::MediaSource::Spotify);
        if spotify_plays {
            return false;
        }
        if self
            .spotify
            .service
            .as_ref()
            .is_some_and(|service| service.connected_account().is_some())
        {
            self.spotify_autoconnect();
        }
        true
    }

    /// The Settings window saved the Spotify section silently.
    pub(crate) fn spotify_settings_window_closed(&mut self, before: SpotifySettings) {
        let after = self.application.saved_spotify_settings();
        if before.listening_history != after.listening_history
            && let Some(service) = &self.spotify.service
        {
            service.set_listening_history(after.listening_history);
        }
        if after.playback() != before.playback() {
            let _ = self.spotify_apply_playback_settings();
        }
    }

    /// Back and Escape: accounts return to the hub, the hub to the main
    /// menu.
    pub(crate) fn spotify_back(&mut self, ui: &mut dyn ShellUi) {
        match self.view {
            View::SpotifyBrowse => self.spotify_browse_back(ui),
            View::SpotifyAccounts => self.show_spotify_hub(ui),
            View::SpotifyHub => self.show_main_menu(ui),
            _ => {}
        }
    }

    /// Closes the session with the application.
    pub fn spotify_shutdown(&self) {
        if let Some(service) = &self.spotify.service {
            service.shutdown();
        }
    }

    // -----------------------------------------------------------------------
    // Playback

    /// At start-up the active account connects in the background, so its
    /// Connect device is there for the phone. Nothing is announced.
    pub(crate) fn spotify_autoconnect(&mut self) {
        let accounts = self.spotify_accounts();
        let Some(key) = apricot_app::spotify::active_account(&accounts).map(|a| a.key.clone())
        else {
            return;
        };
        let Some(service) = self.spotify_service() else {
            return;
        };
        self.spotify.next_account();
        let stamp = self.spotify.epochs.begin();
        self.spotify.connect = None;
        service.connect(stamp, &key);
    }

    /// A `spotify:` URI or an `open.spotify.com` link from Direct link;
    /// `false` when the text is not a Spotify reference.
    pub(crate) fn spotify_play_link(
        &mut self,
        ui: &mut dyn ShellUi,
        text: &str,
        action: &str,
    ) -> bool {
        let Some(reference) = SpotifyRef::parse(text) else {
            return false;
        };
        if action != "play" {
            // Spotify content is never downloaded or exported (plan 3.5).
            ui.show_error(&self.text("spotify_no_export"));
            return true;
        }
        if !matches!(
            reference.kind,
            SpotifyEntityKind::Track
                | SpotifyEntityKind::Episode
                | SpotifyEntityKind::Album
                | SpotifyEntityKind::Playlist
                | SpotifyEntityKind::Artist
                | SpotifyEntityKind::Show
                | SpotifyEntityKind::User
                | SpotifyEntityKind::LikedSongs
        ) {
            ui.show_error(&self.text("spotify_link_kind_unsupported"));
            return true;
        }
        let uri = reference.to_uri();
        let connected = self
            .spotify
            .service
            .as_ref()
            .is_some_and(|service| service.connected_account().is_some());
        if connected {
            self.spotify_resolve(ui, &uri);
            return true;
        }
        let accounts = self.spotify_accounts();
        let Some(key) = apricot_app::spotify::active_account(&accounts).map(|a| a.key.clone())
        else {
            let text = self.text("spotify_log_in_first");
            self.show_spotify_hub(ui);
            ui.set_status(&text, true);
            return true;
        };
        if let Some(service) = self.spotify_service() {
            self.spotify.next_account();
            self.spotify.pending_play = Some(uri);
            let stamp = self.spotify.epochs.begin();
            self.spotify.connect = None;
            ui.set_status(&self.text("spotify_connecting"), false);
            service.connect(stamp, &key);
            ui.set_background_polling(true);
        }
        true
    }

    /// An artist, show, profile or Liked Songs link opens its list; a
    /// track, episode, album or playlist link plays (beta 2).
    fn spotify_resolve(&mut self, ui: &mut dyn ShellUi, uri: &str) {
        if self.spotify_open_link_list(ui, uri) {
            return;
        }
        let Some(service) = self.spotify_service() else {
            return;
        };
        let stamp = self.spotify.epochs.begin();
        self.spotify.resolve = Some(stamp);
        service.resolve_track(stamp, uri);
    }

    /// Music starts at 0:00, spoken content resumes (plan D14).
    pub(crate) fn spotify_play_track(&mut self, ui: &mut dyn ShellUi, item: MediaItem) {
        let episode =
            item.metadata.get("kind").and_then(|kind| kind.as_str()) == Some("spotify_episode");
        if episode {
            self.start_media_item(ui, item, None);
        } else {
            self.start_media_item_at(ui, item, 0.0);
        }
    }

    /// Connect: another device started a track on Apricot. The previous
    /// item saves its position and stops, the new one plays and focus stays
    /// where it is (plan D08). One short announcement.
    fn spotify_handle_notice(&mut self, ui: &mut dyn ShellUi, notice: PlaybackNotice) {
        match notice {
            PlaybackNotice::Playing {
                track,
                requested: true,
                ..
            } => {
                // An album or playlist started as a whole becomes its first
                // track.
                let session = self.application.player_session();
                let collection = session
                    .current_item()
                    .and_then(|item| item.metadata.get("spotify_collection"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                // Also when Spotify started another track than the item
                // names; a relinked track (another ID, same title) stays.
                let other_track = session
                    .current_item()
                    .is_some_and(|item| item.id.0 != track.uri && item.title != track.title);
                if collection || other_track {
                    self.spotify_continue_item(ui, &track);
                }
                self.spotify_push_volume();
            }
            PlaybackNotice::Playing {
                track,
                position_ms,
                requested: false,
            } => {
                if self.spotify_continue_item(ui, &track) {
                    return;
                }
                let mut item = track.media_item();
                item.metadata
                    .insert("spotify_attach".to_owned(), serde_json::Value::Bool(true));
                if let Some(player) = self
                    .spotify_service_ref()
                    .and_then(SpotifyService::playback)
                    .and_then(|playback| playback.player_state())
                {
                    item.metadata.insert(
                        "spotify_shuffle".to_owned(),
                        apricot_spotify::queue::shuffle(&player).into(),
                    );
                }
                // The player's own "Playing: <title>" is the one
                // announcement; the page opens only from the player.
                self.start_media_item_at(ui, item, f64::from(position_ms) / 1000.0);
                self.spotify_push_volume();
            }
            PlaybackNotice::Unavailable { .. } => {
                ui.set_status(&self.text("spotify_track_unavailable"), true);
            }
            PlaybackNotice::QueueChanged => {
                if let Some(player) = self
                    .spotify_service_ref()
                    .and_then(SpotifyService::playback)
                    .and_then(|playback| playback.player_state())
                {
                    self.application
                        .sync_spotify_shuffle(apricot_spotify::queue::shuffle(&player));
                }
                if self.spotify.queue_open {
                    self.spotify_reload_queue();
                }
                self.spotify_follow_smart_shuffle();
            }
            PlaybackNotice::DevicesChanged => {
                self.spotify_devices_changed(ui);
            }
            PlaybackNotice::Volume(volume) => self.spotify_apply_connect_volume(ui, volume),
        }
    }

    /// The playing Spotify item's player volume, when a Spotify item plays.
    fn spotify_volume(&self) -> Option<f64> {
        let session = self.application.player_session();
        let spotify = session.is_open()
            && session
                .current_item()
                .is_some_and(|item| item.source == apricot_core::MediaSource::Spotify);
        spotify
            .then(|| session.audio().map(|audio| audio.volume))
            .flatten()
    }

    /// Apricot's volume changed: the Connect device shows it, so the
    /// phone's volume slider follows (SD-4).
    pub(crate) fn spotify_volume_changed(&self, volume: f64) {
        if self.spotify_volume().is_none() {
            return;
        }
        if let Some(playback) = self
            .spotify
            .service
            .as_ref()
            .and_then(|service| service.playback())
        {
            playback.set_volume(volume);
        }
    }

    /// A Spotify item started: Connect gets Apricot's volume.
    fn spotify_push_volume(&self) {
        if let Some(volume) = self.spotify_volume() {
            self.spotify_volume_changed(volume);
        }
    }

    /// Another device set the volume of Apricot's Connect device: the
    /// player volume follows without an announcement; our own change comes
    /// back the same and is ignored.
    fn spotify_apply_connect_volume(&mut self, ui: &mut dyn ShellUi, volume: u16) {
        let Some(current) = self.spotify_volume() else {
            return;
        };
        let percent = apricot_spotify::playback::percent_from_volume(volume);
        if (percent - current.min(100.0)).abs() < 0.5 {
            return;
        }
        if self.execute_player_command(ui, PlaybackCommand::SetVolume(percent)) {
            self.application.set_player_volume(percent);
            if self.view == View::Player {
                self.refresh_player(ui, PlayerFocus::Preserve);
            }
        }
    }

    /// Spotify moved on to the next track while Apricot plays a Spotify
    /// item: only the item changes, with one "Playing: title"; `false`
    /// when no Spotify item plays.
    fn spotify_continue_item(&mut self, ui: &mut dyn ShellUi, track: &SpotifyTrack) -> bool {
        let session = self.application.player_session();
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
        // An Apricot playlist has priority over Spotify's own autoplay
        // (Urh, 2. 10. 2026).
        let in_apricot_sequence = session
            .current_item()
            .is_some_and(|item| self.application.player_sequence_contains(item));
        let player = self
            .spotify
            .service
            .as_ref()
            .and_then(|service| service.playback())
            .and_then(|playback| playback.player_state());
        let spotify_autoplay = player
            .as_ref()
            .and_then(|player| player.track.as_ref().map(|track| track.provider.clone()))
            .is_some_and(|provider| provider == "autoplay");
        if in_apricot_sequence && spotify_autoplay {
            self.finish_current_item(ui);
            return true;
        }
        let mut item = track.media_item();
        // The context and occurrence, so Play from the start keeps the
        // context.
        if let Some(player) = player {
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
                if apricot_spotify::smart_shuffle::is_smart(current) {
                    item.metadata.insert(
                        "spotify_smart_shuffle".to_owned(),
                        serde_json::Value::Bool(true),
                    );
                }
            }
        }
        if !self.application.replace_current_player_item(item.clone()) {
            return false;
        }
        let message = self.playing_message(&item);
        ui.set_status(&message, true);
        if let Err(error) = self
            .application
            .record_history(item, "played", crate::unix_timestamp())
        {
            ui.set_status(&format!("History was not saved: {error}"), true);
        }
        if self.view == View::Player {
            self.refresh_player(ui, PlayerFocus::Preserve);
        } else if let Some(model) = self.application.player_screen_model() {
            // The window title names the playing item on every screen.
            ui.set_window_title(&apricot_platform::playing_window_title(&model.window_title));
        }
        true
    }

    /// Ctrl+PageUp and `PageDown`, Shift+S and R while a Spotify item plays:
    /// Connect does them, so they follow the Spotify context and reach the
    /// phone too.
    pub(crate) fn spotify_transport(&mut self, ui: &mut dyn ShellUi, action_id: &str) -> bool {
        if !matches!(
            action_id,
            "player_next" | "player_previous" | "player_shuffle" | "player_repeat"
        ) {
            return false;
        }
        let session = self.application.player_session();
        let spotify = session.is_open()
            && session
                .current_item()
                .is_some_and(|item| item.source == apricot_core::MediaSource::Spotify);
        let Some(playback) = self
            .spotify
            .service
            .as_ref()
            .and_then(|service| service.playback())
            .filter(|_| spotify)
        else {
            return false;
        };
        let player = playback.player_state().unwrap_or_default();
        let mut smart_start = None;
        let key = match action_id {
            "player_next" => {
                playback.next();
                None
            }
            "player_previous" => {
                playback.previous();
                None
            }
            // Off, on, smart shuffle (playlists only), off: the order of the
            // Spotify apps.
            "player_shuffle" => {
                let shuffle = apricot_spotify::queue::shuffle(&player);
                let smart = shuffle
                    && playback
                        .smart_shuffle()
                        .is_some_and(|playlist| playlist == player.context_uri);
                if !shuffle {
                    playback.set_shuffle(true);
                    Some("shuffle_on")
                } else if !smart && player.context_uri.starts_with("spotify:playlist:") {
                    playback.set_smart_shuffle(Some(player.context_uri.clone()));
                    smart_start = Some(player.context_uri.clone());
                    Some("spotify_smart_shuffle_on")
                } else {
                    playback.set_smart_shuffle(None);
                    playback.set_shuffle(false);
                    Some("shuffle_off")
                }
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
            if action_id == "player_shuffle" {
                self.application
                    .set_player_toggle(apricot_app::SessionToggle::Shuffle, key != "shuffle_off");
            }
            ui.set_status(&self.text(key), true);
        }
        if let Some(playlist) = smart_start {
            self.spotify.smart_used.clear();
            self.spotify.smart_request = None;
            self.spotify.smart_requested_at = None;
            self.spotify_request_smart_tracks(playlist);
        }
        true
    }

    /// Asks for smart shuffle recommendations, at most every few seconds
    /// and one request at a time.
    fn spotify_request_smart_tracks(&mut self, playlist: String) {
        let Some(service) = self.spotify.service.clone() else {
            return;
        };
        if self.spotify.smart_request.is_some()
            || self
                .spotify
                .smart_requested_at
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(5))
        {
            return;
        }
        let stamp = self.spotify.epochs.begin();
        self.spotify.smart_request = Some(stamp);
        self.spotify.smart_requested_at = Some(std::time::Instant::now());
        service.smart_shuffle(stamp, playlist, self.spotify.smart_used.clone());
    }

    /// The upcoming tracks changed: smart shuffle ends with its playlist or
    /// with shuffle, and asks for more when none is left.
    fn spotify_follow_smart_shuffle(&mut self) {
        let Some(playback) = self.spotify.service.as_ref().and_then(|s| s.playback()) else {
            return;
        };
        let Some(playlist) = playback.smart_shuffle() else {
            return;
        };
        let Some(player) = playback.player_state() else {
            return;
        };
        if player.context_uri != playlist || !apricot_spotify::queue::shuffle(&player) {
            playback.set_smart_shuffle(None);
            return;
        }
        if apricot_spotify::smart_shuffle::needs_more(&player.next_tracks) {
            self.spotify_request_smart_tracks(playlist);
        }
    }

    /// The Spotify queue reloads; the open dialog gets the confirmed rows.
    pub(crate) fn spotify_reload_queue(&mut self) {
        let Some(service) = self.spotify_service() else {
            return;
        };
        let stamp = self.spotify.epochs.begin();
        self.spotify.queue_load = Some(stamp);
        service.load_queue(stamp);
    }
}
