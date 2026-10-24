//! `AudioVault` screens (Windows `audiovault_win32.rs`, Python
//! `AudioVaultMixin`): the menu, search, recently added titles, TV show
//! episodes, login, playback and downloads. The network runs on worker
//! threads that report back through a channel polled with the other
//! background work.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use apricot_app::audiovault::{self as vault, ArchiveMember, AudiovaultKind, AudiovaultMode};
use apricot_core::{MediaItem, Route, RouteFrame, TranslationCatalog};
use apricot_platform::audiovault::{
    AUDIOVAULT_BASE_URL, AUDIOVAULT_REGISTER_URL, AudiovaultClient, AudiovaultError,
    RemoteMemberRequest, ShowProgress, episode_files, is_audio_file, natural_sort_key,
    show_cache_is_complete,
};
use apricot_platform::audiovault_credentials::{protect_password, unprotect_password};
use serde_json::Value;

use crate::{ListScreen, ScreenControl, Shell, ShellUi, View};

/// Python `audiovault_view`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VaultView {
    Menu,
    Search,
    Recent(AudiovaultMode),
    Episodes,
}

/// Python `audiovault_parent_results`, `_view` and `_title`.
#[derive(Clone)]
struct Parent {
    results: Vec<MediaItem>,
    view: VaultView,
    title: String,
}

/// Python `audiovault_player_return_state`.
#[derive(Clone)]
struct Snapshot {
    mode: AudiovaultMode,
    results: Vec<MediaItem>,
    view: VaultView,
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

/// P-15 (O-19): a movie or episode that finishes preparing plays only while
/// it is still what the user asked for.
#[derive(Clone, Copy)]
struct PlayRequest {
    token: u64,
    player_generation: u64,
    from_audiovault: bool,
}

/// Python `resolve_audiovault_stream` without the open response.
struct ResolvedStream {
    final_url: String,
    content_type: String,
    disposition: String,
    headers: Vec<(String, String)>,
}

type ManifestResult = Result<(u64, Vec<apricot_platform::zip_archive::ZipEntry>), AudiovaultError>;

enum Event {
    Login {
        email: String,
        remember: bool,
        after: AfterLogin,
        result: Result<Option<String>, AudiovaultError>,
    },
    Results {
        /// P-14 (O-18): only the latest search or Recent request fills the
        /// list.
        generation: u64,
        view: VaultView,
        retry: Option<AfterLogin>,
        result: Result<Vec<vault::VaultRecord>, AudiovaultError>,
        mode: AudiovaultMode,
    },
    Manifest {
        show: MediaItem,
        generation: u64,
        allow_retry: bool,
        result: ManifestResult,
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
        result: Result<(), AudiovaultError>,
    },
    EpisodeDone {
        item: MediaItem,
        download_after: bool,
        allow_retry: bool,
        task: u64,
        request: PlayRequest,
        result: Result<(), AudiovaultError>,
    },
    Stream {
        item: MediaItem,
        allow_retry: bool,
        request: PlayRequest,
        result: Result<ResolvedStream, AudiovaultError>,
    },
    Movie {
        item: MediaItem,
        allow_retry: bool,
        result: Result<PathBuf, AudiovaultError>,
    },
}

/// The `AudioVault` login dialog's texts (Windows `LoginTexts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudiovaultLogin {
    pub title: String,
    pub email_label: String,
    pub password_label: String,
    pub register: String,
    pub ok: String,
    pub cancel: String,
    pub email: String,
    /// Register opens this page.
    pub register_url: &'static str,
}

/// Windows `AudiovaultState`.
pub(crate) struct AudiovaultState {
    client: Option<Arc<AudiovaultClient>>,
    /// Python `audiovault_logged_in`, for this address.
    logged_in_email: Option<String>,
    mode: AudiovaultMode,
    view: VaultView,
    title: String,
    results: Vec<MediaItem>,
    parent: Option<Parent>,
    manifests: HashMap<String, Vec<MediaItem>>,
    manifest_loading: HashSet<String>,
    episode_loading: HashSet<String>,
    progress_task: u64,
    progress_generation: u64,
    show_generation: u64,
    results_generation: u64,
    play_request: u64,
    player_return: Option<Snapshot>,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    running: usize,
    /// A message shown for one event must not handle the next one.
    polling: bool,
}

impl Default for AudiovaultState {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            client: None,
            logged_in_email: None,
            mode: AudiovaultMode::Movies,
            view: VaultView::Menu,
            title: String::new(),
            results: Vec::new(),
            parent: None,
            manifests: HashMap::new(),
            manifest_loading: HashSet::new(),
            episode_loading: HashSet::new(),
            progress_task: 0,
            progress_generation: 0,
            show_generation: 0,
            results_generation: 0,
            play_request: 0,
            player_return: None,
            sender,
            receiver,
            running: 0,
            polling: false,
        }
    }
}

pub(crate) const fn is_view(view: View) -> bool {
    matches!(
        view,
        View::AudiovaultMenu | View::AudiovaultSearch | View::AudiovaultResults
    )
}

/// Python `friendly_error(exc)` of an `AudioVault` failure.
fn error_text(catalog: &TranslationCatalog, error: &AudiovaultError) -> String {
    let text = error
        .text_key()
        .map_or_else(|| error.to_string(), |key| catalog.text(key).to_owned());
    apricot_app::comments::friendly_error(catalog, &text)
}

impl Shell {
    fn keyed(&self, key: &str, replacements: &[(&str, &str)]) -> String {
        let mut message = self.text(key);
        for (name, value) in replacements {
            message = message.replace(&format!("{{{name}}}"), value);
        }
        message
    }

    fn vault_announce(&self, ui: &mut dyn ShellUi, key: &str, replacements: &[(&str, &str)]) {
        ui.set_status(&self.keyed(key, replacements), true);
    }

    /// Python `set_status`: shown on the status line, not spoken.
    fn vault_status(&self, ui: &mut dyn ShellUi, key: &str, replacements: &[(&str, &str)]) {
        ui.set_status(&self.keyed(key, replacements), false);
    }

    fn vault_message(&self, ui: &mut dyn ShellUi, key: &str, error: &AudiovaultError) {
        let catalog = self.catalog();
        let text = catalog
            .text(key)
            .replace("{error}", &error_text(&catalog, error));
        ui.show_error(&text);
    }

    fn vault_client(&mut self) -> Option<Arc<AudiovaultClient>> {
        if self.audiovault.client.is_none() {
            self.audiovault.client = AudiovaultClient::new(env!("CARGO_PKG_VERSION"))
                .ok()
                .map(Arc::new);
        }
        self.audiovault.client.clone()
    }

    /// Runs `work` on a worker thread; its event comes back on the poll.
    fn vault_spawn(
        &mut self,
        ui: &mut dyn ShellUi,
        work: impl FnOnce(&AudiovaultClient, &Sender<Event>) -> Option<Event> + Send + 'static,
    ) {
        let Some(client) = self.vault_client() else {
            return;
        };
        let sender = self.audiovault.sender.clone();
        self.audiovault.running += 1;
        ui.set_background_polling(true);
        std::thread::spawn(move || {
            if let Some(event) = work(&client, &sender) {
                let _ = sender.send(event);
            }
        });
    }

    // -----------------------------------------------------------------------
    // Login

    fn vault_settings_email(&self) -> String {
        self.application
            .settings()
            .audiovault_email
            .trim()
            .to_owned()
    }

    /// Python `audiovault_logged_in`. A changed address in the settings
    /// logs out.
    fn vault_logged_in(&self) -> bool {
        self.audiovault
            .logged_in_email
            .as_deref()
            .is_some_and(|email| email == self.application.settings().audiovault_email)
    }

    /// Python `ensure_audiovault_login`.
    fn vault_ensure_login(&mut self, ui: &mut dyn ShellUi, after: AfterLogin) -> bool {
        if self.vault_logged_in() {
            return true;
        }
        if self.audiovault.logged_in_email.take().is_some()
            && let Some(client) = &self.audiovault.client
        {
            client.clear_cookies();
        }
        let email = self.vault_settings_email();
        let password =
            unprotect_password(&self.application.settings().audiovault_password_protected);
        if !email.is_empty() && !password.is_empty() {
            ui.set_status(&self.text("audiovault_logging_in"), false);
            self.vault_start_login(ui, email, password, after, false);
        } else {
            self.vault_show_login(ui, after);
        }
        false
    }

    /// Python `retry_audiovault_after_login`.
    fn vault_retry_after_login(&mut self, ui: &mut dyn ShellUi, after: AfterLogin) {
        self.audiovault.logged_in_email = None;
        if let Some(client) = &self.audiovault.client {
            client.clear_cookies();
        }
        self.vault_ensure_login(ui, after);
    }

    /// Python `show_audiovault_login`.
    fn vault_show_login(&mut self, ui: &mut dyn ShellUi, after: AfterLogin) -> bool {
        let login = AudiovaultLogin {
            title: self.text("audiovault_login"),
            email_label: self.text("email"),
            password_label: self.text("password"),
            register: self.text("register"),
            ok: self.text("ok"),
            cancel: self.text("cancel"),
            email: self.application.settings().audiovault_email.clone(),
            register_url: AUDIOVAULT_REGISTER_URL,
        };
        let Some((address, secret)) = ui.audiovault_login(&login) else {
            return false;
        };
        let address = address.trim().to_owned();
        if address.is_empty() || secret.is_empty() {
            ui.show_error(&self.text("audiovault_credentials_required"));
            return false;
        }
        self.vault_status(ui, "audiovault_logging_in", &[]);
        self.vault_start_login(ui, address, secret, after, true);
        true
    }

    fn vault_start_login(
        &mut self,
        ui: &mut dyn ShellUi,
        email: String,
        password: String,
        after: AfterLogin,
        remember: bool,
    ) {
        self.vault_spawn(ui, move |client, _| {
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

    fn vault_finish_login(
        &mut self,
        ui: &mut dyn ShellUi,
        email: &str,
        remember: bool,
        after: AfterLogin,
        result: Result<Option<String>, AudiovaultError>,
    ) {
        match result {
            Ok(protected) => {
                self.audiovault.logged_in_email = Some(email.to_owned());
                if let Err(error) = self
                    .application
                    .store_audiovault_login(email, protected.filter(|_| remember))
                {
                    ui.show_error(&error.to_string());
                }
                self.vault_status(ui, "audiovault_logged_in", &[]);
                self.vault_run_after_login(ui, after);
            }
            Err(error) => {
                self.audiovault.logged_in_email = None;
                let retry_interactively = !remember;
                if retry_interactively {
                    let _ = self.application.forget_audiovault_login(false);
                }
                let catalog = self.catalog();
                let text = catalog
                    .text("audiovault_login_error")
                    .replace("{error}", &error_text(&catalog, &error));
                ui.show_error(&text);
                if retry_interactively {
                    self.vault_show_login(ui, after);
                }
            }
        }
    }

    fn vault_run_after_login(&mut self, ui: &mut dyn ShellUi, after: AfterLogin) {
        match after {
            AfterLogin::Nothing => {}
            AfterLogin::Menu => self.show_audiovault_menu(ui),
            AfterLogin::Search => self.show_audiovault_search(ui),
            AfterLogin::Recent(mode) => self.vault_show_recent(ui, mode, false),
            AfterLogin::SearchQuery(query, mode) => self.vault_start_search(ui, query, mode, false),
            AfterLogin::PlayRemote(item) => self.vault_play_remote(ui, item, false),
            AfterLogin::PrepareShow {
                item,
                download_after,
            } => self.vault_prepare_show(ui, item, download_after, false),
            AfterLogin::PrepareEpisode {
                item,
                download_after,
            } => self.vault_prepare_remote_episode(ui, item, download_after, false),
            AfterLogin::Download(item) => self.vault_download_item(ui, item, false),
        }
    }

    /// Settings: Python `login_audiovault_from_settings`.
    pub fn audiovault_settings_login(&mut self, ui: &mut dyn ShellUi) {
        self.vault_show_login(ui, AfterLogin::Nothing);
    }

    /// Settings: Python `logout_audiovault`.
    pub fn audiovault_settings_logout(&mut self, ui: &mut dyn ShellUi) {
        if let Some(client) = &self.audiovault.client {
            client.clear_cookies();
        }
        self.audiovault.logged_in_email = None;
        if let Err(error) = self.application.forget_audiovault_login(true) {
            ui.show_error(&error.to_string());
            return;
        }
        self.vault_status(ui, "audiovault_logged_out", &[]);
    }

    // -----------------------------------------------------------------------
    // Screens

    /// Python `prepare_audiovault_screen`.
    fn vault_prepare_screen(
        &mut self,
        ui: &mut dyn ShellUi,
        view: VaultView,
        main_view: View,
        route: Route,
    ) {
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != route {
            self.application.navigate_main_menu();
            self.application.navigate_to(RouteFrame::new(route));
        }
        self.audiovault.view = view;
        self.view = main_view;
    }

    fn vault_button(&self, id: &'static str, key: &str) -> ScreenControl {
        ScreenControl::Button {
            id,
            label: self.text(key),
        }
    }

    /// Python `show_audiovault_menu`.
    pub(crate) fn show_audiovault_menu(&mut self, ui: &mut dyn ShellUi) {
        if !self.vault_ensure_login(ui, AfterLogin::Menu) {
            return;
        }
        self.remember_menu_item("audiovault");
        self.audiovault.parent = None;
        self.vault_prepare_screen(
            ui,
            VaultView::Menu,
            View::AudiovaultMenu,
            Route::AudiovaultMenu,
        );
        let catalog = self.catalog();
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text("audiovault").to_owned(),
            rows: [
                "search",
                "audiovault_recent_tv_shows",
                "audiovault_recent_movies",
            ]
            .iter()
            .map(|key| catalog.text(key).to_owned())
            .collect(),
            selected: Some(0),
            controls: vec![
                self.vault_button("back", "back"),
                self.vault_button("open", "open"),
                ScreenControl::List,
            ],
            focus_list: true,
        });
    }

    /// Python `activate_audiovault_menu_item`.
    fn vault_activate_menu_item(&mut self, ui: &mut dyn ShellUi) {
        match ui.selected_row() {
            Some(0) => self.show_audiovault_search(ui),
            Some(1) => self.vault_show_recent(ui, AudiovaultMode::Shows, true),
            Some(2) => self.vault_show_recent(ui, AudiovaultMode::Movies, true),
            _ => {}
        }
    }

    /// The search screen with `rows` (Windows creation order: Back, the
    /// query, the type, Search, Play, Download audio, the list).
    fn vault_search_screen(&self, rows: Vec<String>, reset: bool) -> ListScreen {
        let catalog = self.catalog();
        ListScreen {
            list_name: catalog.text("result_list").to_owned(),
            rows,
            selected: Some(0),
            controls: vec![
                self.vault_button("back", "back"),
                ScreenControl::TextField {
                    id: "audiovault_query",
                    label: catalog.text("search_query").to_owned(),
                    value: reset.then(String::new),
                },
                ScreenControl::Choice {
                    id: "audiovault_type",
                    label: catalog.text("type").to_owned(),
                    options: vec![
                        catalog.text("movies").to_owned(),
                        catalog.text("tv_shows").to_owned(),
                    ],
                    selected: reset
                        .then_some(usize::from(self.audiovault.mode == AudiovaultMode::Shows)),
                },
                self.vault_button("search", "search"),
                self.vault_button("search_play", "play"),
                self.vault_button("search_download_audio", "download_audio"),
                ScreenControl::List,
            ],
            focus_list: false,
        }
    }

    /// Python `show_audiovault_search`.
    pub(crate) fn show_audiovault_search(&mut self, ui: &mut dyn ShellUi) {
        if !self.vault_ensure_login(ui, AfterLogin::Search) {
            return;
        }
        // Python remembers `show_audiovault_search`, which is no main menu
        // item.
        self.remember_menu_item("audiovault_search");
        self.vault_prepare_screen(
            ui,
            VaultView::Search,
            View::AudiovaultSearch,
            Route::AudiovaultSearch,
        );
        self.audiovault.title = self.text("search");
        self.audiovault.results.clear();
        let screen = self.vault_search_screen(vec![self.text("search_results_empty")], true);
        ui.show_list_screen(&screen);
        ui.focus_screen_control("audiovault_query");
    }

    /// The results screen (Windows `MainView::AudiovaultResults`): Back,
    /// Play, Download audio, the list named by the title.
    fn vault_results_screen(&self, title: &str, rows: Vec<String>) -> ListScreen {
        ListScreen {
            list_name: title.to_owned(),
            rows,
            selected: Some(0),
            controls: vec![
                self.vault_button("back", "back"),
                self.vault_button("search_play", "play"),
                self.vault_button("search_download_audio", "download_audio"),
                ScreenControl::List,
            ],
            focus_list: true,
        }
    }

    /// Python `show_audiovault_results_screen`.
    fn vault_show_results_screen(&mut self, ui: &mut dyn ShellUi, title: &str, view: VaultView) {
        let route = if view == VaultView::Episodes {
            Route::AudiovaultEpisodes
        } else {
            Route::AudiovaultResults
        };
        self.vault_prepare_screen(ui, view, View::AudiovaultResults, route);
        title.clone_into(&mut self.audiovault.title);
        self.audiovault.results.clear();
        let screen = self.vault_results_screen(title, vec![self.text("search_results_empty")]);
        ui.show_list_screen(&screen);
    }

    /// Python `show_audiovault_recent`.
    fn vault_show_recent(&mut self, ui: &mut dyn ShellUi, mode: AudiovaultMode, allow_retry: bool) {
        self.audiovault.mode = mode;
        let title = self.text(mode.recent_title_key());
        self.audiovault.results_generation += 1;
        let generation = self.audiovault.results_generation;
        self.vault_show_results_screen(ui, &title, VaultView::Recent(mode));
        self.vault_status(ui, "audiovault_loading_recent", &[]);
        self.vault_spawn(ui, move |client, _| {
            let result = client
                .fetch_page(&format!("{AUDIOVAULT_BASE_URL}/"))
                .map(|page| vault::parse_vault_page(&page).records);
            Some(Event::Results {
                generation,
                view: VaultView::Recent(mode),
                retry: allow_retry.then_some(AfterLogin::Recent(mode)),
                result,
                mode,
            })
        });
    }

    /// Python `back_from_audiovault`.
    pub(crate) fn audiovault_back(&mut self, ui: &mut dyn ShellUi) {
        self.audiovault.show_generation += 1;
        if let Some(parent) = self
            .audiovault
            .parent
            .take()
            .filter(|parent| !parent.results.is_empty())
        {
            if parent.view == VaultView::Search {
                self.show_audiovault_search(ui);
            } else {
                self.vault_show_results_screen(ui, &parent.title, parent.view);
            }
            self.vault_show_results(ui, parent.results);
            return;
        }
        if self.audiovault.view == VaultView::Menu {
            self.show_main_menu(ui);
        } else {
            self.show_audiovault_menu(ui);
        }
    }

    /// Python `search_audiovault`.
    pub(crate) fn audiovault_search(&mut self, ui: &mut dyn ShellUi) {
        let query = ui.screen_text("audiovault_query").trim().to_owned();
        if query.is_empty() {
            ui.show_error(&self.text("enter_query"));
            return;
        }
        let mode = if ui.screen_choice("audiovault_type") == Some(0) {
            AudiovaultMode::Movies
        } else {
            AudiovaultMode::Shows
        };
        self.audiovault.mode = mode;
        self.vault_status(ui, "searching", &[("query", &query)]);
        self.vault_start_search(ui, query, mode, true);
    }

    fn vault_start_search(
        &mut self,
        ui: &mut dyn ShellUi,
        query: String,
        mode: AudiovaultMode,
        allow_retry: bool,
    ) {
        self.audiovault.results_generation += 1;
        let generation = self.audiovault.results_generation;
        self.vault_spawn(ui, move |client, _| {
            let result = client
                .fetch_page(&vault::catalog_url(mode, &query))
                .map(|page| vault::parse_vault_page(&page).records);
            Some(Event::Results {
                generation,
                view: VaultView::Search,
                retry: allow_retry.then_some(AfterLogin::SearchQuery(query, mode)),
                result,
                mode,
            })
        });
    }

    /// Python `show_audiovault_results`: the list rows change in place.
    fn vault_show_results(&mut self, ui: &mut dyn ShellUi, results: Vec<MediaItem>) {
        let catalog = self.catalog();
        let mut rows: Vec<String> = results
            .iter()
            .map(|item| vault::result_line(item, &catalog))
            .collect();
        if rows.is_empty() {
            rows.push(catalog.text("no_results").to_owned());
        }
        let found = catalog
            .text("found")
            .replace("{count}", &results.len().to_string());
        self.audiovault.results = results;
        ui.set_list_rows(rows, 0);
        ui.set_status(&found, false);
        ui.focus_list();
    }

    /// Python `selected_audiovault_item`.
    pub(crate) fn audiovault_selected_item(&self, ui: &dyn ShellUi) -> Option<MediaItem> {
        if !matches!(self.view, View::AudiovaultSearch | View::AudiovaultResults) {
            return None;
        }
        self.audiovault.results.get(ui.selected_row()?).cloned()
    }

    /// Enter, Open and Play on an `AudioVault` screen.
    pub(crate) fn audiovault_activate(&mut self, ui: &mut dyn ShellUi) {
        if self.view == View::AudiovaultMenu {
            self.vault_activate_menu_item(ui);
        } else {
            self.vault_activate_item(ui);
        }
    }

    /// Python `activate_audiovault_item`.
    fn vault_activate_item(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self.audiovault_selected_item(ui) else {
            return;
        };
        match vault::item_kind(&item) {
            Some(AudiovaultKind::Show) => self.vault_prepare_show(ui, item, false, true),
            Some(AudiovaultKind::RemoteEpisode) => {
                self.vault_prepare_remote_episode(ui, item, false, true);
            }
            Some(AudiovaultKind::Episode) => self.vault_play_local(ui, item),
            _ => self.vault_play_remote(ui, item, true),
        }
    }

    /// Button commands of the `AudioVault` screens; `true` when handled.
    pub(crate) fn audiovault_command(&mut self, ui: &mut dyn ShellUi, id: &str) -> bool {
        if !is_view(self.view) {
            return false;
        }
        match id {
            "back" => self.audiovault_back(ui),
            "open" | "search_play" => self.audiovault_activate(ui),
            "search" => self.audiovault_search(ui),
            "search_download_audio" => self.vault_download_selected(ui),
            _ => return false,
        }
        true
    }

    // -----------------------------------------------------------------------
    // TV shows

    /// Python `prepare_audiovault_show`.
    fn vault_prepare_show(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        download_after: bool,
        allow_retry: bool,
    ) {
        let catalog = self.catalog();
        let cache_dir = vault::show_cache_dir(&self.application.settings().cache_folder, &item);
        let episodes = vault::local_episode_items(&item, &episode_files(&cache_dir), &catalog);
        if !episodes.is_empty() && show_cache_is_complete(&cache_dir) {
            if download_after {
                self.vault_copy_show_to_downloads(ui, &item, &cache_dir);
            } else {
                self.vault_show_episodes(ui, &item, episodes);
            }
            return;
        }
        if download_after {
            self.vault_start_full_show_job(ui, item, cache_dir, true, allow_retry, 0);
            return;
        }
        let key = vault::show_key(&item);
        if let Some(cached) = self
            .audiovault
            .manifests
            .get(&key)
            .filter(|cached| !cached.is_empty())
            .cloned()
        {
            self.vault_show_episodes(ui, &item, cached);
            return;
        }
        self.vault_status(ui, "audiovault_loading_episodes", &[("title", &item.title)]);
        if self.audiovault.manifest_loading.contains(&key) {
            return;
        }
        self.audiovault.manifest_loading.insert(key);
        self.audiovault.show_generation += 1;
        let generation = self.audiovault.show_generation;
        self.vault_spawn(ui, move |client, _| {
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

    /// Python `load_audiovault_show_manifest_worker` back on the UI.
    fn vault_finish_manifest(
        &mut self,
        ui: &mut dyn ShellUi,
        show: MediaItem,
        generation: u64,
        allow_retry: bool,
        result: ManifestResult,
    ) {
        let key = vault::show_key(&show);
        self.audiovault.manifest_loading.remove(&key);
        let catalog = self.catalog();
        let cache_dir = vault::show_cache_dir(&self.application.settings().cache_folder, &show);
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
                    self.vault_message(ui, "audiovault_show_failed", &AudiovaultError::NoEpisodes);
                    return;
                }
                self.audiovault.manifests.insert(key, episodes.clone());
                self.vault_show_episodes_if_current(ui, generation, &show, episodes);
            }
            Err(AudiovaultError::RangeUnsupported) => {
                self.vault_start_full_show_job(ui, show, cache_dir, false, allow_retry, generation);
            }
            Err(AudiovaultError::SessionExpired) if allow_retry => {
                self.vault_retry_after_login(
                    ui,
                    AfterLogin::PrepareShow {
                        item: show,
                        download_after: false,
                    },
                );
            }
            Err(error) => self.vault_message(ui, "audiovault_show_failed", &error),
        }
    }

    /// Python `show_audiovault_episodes_if_current`.
    fn vault_show_episodes_if_current(
        &mut self,
        ui: &mut dyn ShellUi,
        generation: u64,
        show: &MediaItem,
        episodes: Vec<MediaItem>,
    ) {
        if generation != 0 && generation != self.audiovault.show_generation {
            return;
        }
        if !is_view(self.view) {
            return;
        }
        self.vault_show_episodes(ui, show, episodes);
    }

    /// Python `show_audiovault_episodes`: the episodes replace the list on
    /// the same screen, which keeps its fields and title.
    fn vault_show_episodes(
        &mut self,
        ui: &mut dyn ShellUi,
        show: &MediaItem,
        episodes: Vec<MediaItem>,
    ) {
        let catalog = self.catalog();
        self.audiovault.parent = Some(Parent {
            results: self.audiovault.results.clone(),
            view: self.audiovault.view,
            title: self.audiovault.title.clone(),
        });
        self.audiovault.view = VaultView::Episodes;
        self.audiovault.title = if show.title.is_empty() {
            catalog.text("tv_show").to_owned()
        } else {
            show.title.clone()
        };
        let rows = episodes
            .iter()
            .map(|item| vault::result_line(item, &catalog))
            .collect();
        let loaded = catalog
            .text("audiovault_episodes_loaded")
            .replace("{count}", &episodes.len().to_string())
            .replace("{title}", &show.title);
        self.audiovault.results = episodes;
        ui.set_list_rows(rows, 0);
        ui.set_status(&loaded, false);
        ui.focus_list();
    }

    /// Python `next_audiovault_progress_task_id` and
    /// `show_audiovault_progress`.
    fn vault_start_progress(&mut self, ui: &mut dyn ShellUi, title: &str) -> u64 {
        self.audiovault.progress_generation += 1;
        let task = self.audiovault.progress_generation;
        self.audiovault.progress_task = task;
        self.vault_status(
            ui,
            "audiovault_progress_downloading",
            &[("title", title), ("percent", "0")],
        );
        task
    }

    /// Python `close_audiovault_progress`.
    const fn vault_close_progress(&mut self, task: u64) {
        if task != 0 && self.audiovault.progress_task == task {
            self.audiovault.progress_task = 0;
        }
    }

    /// Python `start_audiovault_full_show_job`.
    fn vault_start_full_show_job(
        &mut self,
        ui: &mut dyn ShellUi,
        show: MediaItem,
        cache_dir: PathBuf,
        download_after: bool,
        allow_retry: bool,
        generation: u64,
    ) {
        if generation != 0 && (generation != self.audiovault.show_generation || !is_view(self.view))
        {
            return;
        }
        let task = self.vault_start_progress(ui, &show.title);
        self.vault_spawn(ui, move |client, sender| {
            let url = show
                .url
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();
            let title = show.title.clone();
            let result = client.download_show(&url, &cache_dir, &mut |progress| {
                let (message_key, percent) = match progress {
                    ShowProgress::Downloading(percent) => {
                        ("audiovault_progress_downloading", percent)
                    }
                    ShowProgress::Extracting(percent) => {
                        ("audiovault_progress_extracting", percent)
                    }
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

    #[allow(clippy::too_many_arguments)]
    fn vault_finish_show(
        &mut self,
        ui: &mut dyn ShellUi,
        show: MediaItem,
        download_after: bool,
        allow_retry: bool,
        task: u64,
        generation: u64,
        result: Result<(), AudiovaultError>,
    ) {
        self.vault_close_progress(task);
        let cache_dir = vault::show_cache_dir(&self.application.settings().cache_folder, &show);
        match result {
            Ok(()) => {
                if download_after {
                    self.vault_copy_show_to_downloads(ui, &show, &cache_dir);
                } else {
                    let episodes = vault::local_episode_items(
                        &show,
                        &episode_files(&cache_dir),
                        &self.catalog(),
                    );
                    self.vault_show_episodes_if_current(ui, generation, &show, episodes);
                }
            }
            Err(AudiovaultError::SessionExpired) if allow_retry => self.vault_retry_after_login(
                ui,
                AfterLogin::PrepareShow {
                    item: show,
                    download_after,
                },
            ),
            Err(error) => self.vault_message(ui, "audiovault_show_failed", &error),
        }
    }

    // -----------------------------------------------------------------------
    // Episodes

    /// Python `prepare_audiovault_remote_episode`.
    #[allow(clippy::too_many_lines)] // As on Windows.
    fn vault_prepare_remote_episode(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        download_after: bool,
        allow_retry: bool,
    ) {
        let target = PathBuf::from(item.local_path.clone().unwrap_or_default());
        let expected = vault::metadata_u64(&item, "archive_file_size");
        if target.is_file()
            && (expected == 0
                || std::fs::metadata(&target).is_ok_and(|meta| meta.len() == expected))
        {
            apricot_platform::audiovault::touch(&target);
            self.vault_finish_remote_episode(ui, &item, download_after);
            return;
        }
        let _ = std::fs::remove_file(&target);
        let key = target.to_string_lossy().into_owned();
        if self.audiovault.episode_loading.contains(&key) {
            self.vault_status(
                ui,
                "audiovault_episode_preparing",
                &[("title", &item.title)],
            );
            return;
        }
        self.audiovault.episode_loading.insert(key);
        let play_request = self.vault_new_play_request();
        let task = if download_after {
            self.vault_start_progress(ui, &item.title)
        } else {
            self.vault_announce(
                ui,
                "audiovault_episode_cache_notice",
                &[("title", &item.title)],
            );
            0
        };
        let settings = self.application.settings();
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
        let playing = self
            .application
            .player_session()
            .current_item()
            .and_then(|current| current.local_path.clone());
        self.vault_spawn(ui, move |client, sender| {
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
                    // Python speaks the cache progress in steps of five
                    // percent.
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
                apricot_platform::audiovault::trim_episode_cache(
                    &cache_root,
                    cache_limit,
                    &protected,
                );
            }
            Some(Event::EpisodeDone {
                item,
                download_after,
                allow_retry,
                task,
                request: play_request,
                result,
            })
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn vault_finish_episode(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        download_after: bool,
        allow_retry: bool,
        task: u64,
        request: PlayRequest,
        result: Result<(), AudiovaultError>,
    ) {
        let key = item.local_path.clone().unwrap_or_default();
        self.audiovault.episode_loading.remove(&key);
        self.vault_close_progress(task);
        match result {
            Ok(()) if !download_after && !self.vault_play_request_current(request) => {}
            Ok(()) => self.vault_finish_remote_episode(ui, &item, download_after),
            Err(AudiovaultError::SessionExpired) if allow_retry => self.vault_retry_after_login(
                ui,
                AfterLogin::PrepareEpisode {
                    item,
                    download_after,
                },
            ),
            Err(error) => {
                let key = if download_after {
                    "download_failed"
                } else {
                    "player_failed"
                };
                self.vault_message(ui, key, &error);
            }
        }
    }

    /// Python `finish_audiovault_remote_episode`.
    fn vault_finish_remote_episode(
        &mut self,
        ui: &mut dyn ShellUi,
        item: &MediaItem,
        download_after: bool,
    ) {
        let playable = vault::cached_episode(item);
        if download_after {
            self.vault_copy_episode_to_downloads(ui, &playable);
        } else {
            self.vault_play_local(ui, playable);
        }
    }

    /// Python `audiovault_player_return_state`.
    fn vault_snapshot(&self) -> Snapshot {
        Snapshot {
            mode: self.audiovault.mode,
            results: self.audiovault.results.clone(),
            view: self.audiovault.view,
            title: self.audiovault.title.clone(),
            parent: self.audiovault.parent.clone(),
        }
    }

    /// Python `play_audiovault_local_item`.
    fn vault_play_local(&mut self, ui: &mut dyn ShellUi, item: MediaItem) {
        self.audiovault.player_return = Some(self.vault_snapshot());
        let sequence = self.audiovault.results.clone();
        self.application
            .prepare_audiovault_playback(&sequence, &item);
        if self.application.player_sequence_source()
            == Some(apricot_app::PlaybackSequenceSource::Audiovault)
        {
            self.start_sequence_media_item(ui, item, None);
        } else {
            self.start_media_item(ui, item, None);
        }
    }

    /// Python `play_audiovault_remote_item`.
    fn vault_play_remote(&mut self, ui: &mut dyn ShellUi, item: MediaItem, allow_retry: bool) {
        let request = self.vault_new_play_request();
        // Python `set_status`: shown, not spoken.
        self.vault_status(ui, "preparing_stream", &[("title", &item.title)]);
        self.vault_spawn(ui, move |client, _| {
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
                request,
                result,
            })
        });
    }

    fn vault_finish_stream(
        &mut self,
        ui: &mut dyn ShellUi,
        mut item: MediaItem,
        allow_retry: bool,
        request: PlayRequest,
        result: Result<ResolvedStream, AudiovaultError>,
    ) {
        if result.is_ok() && !self.vault_play_request_current(request) {
            return;
        }
        match result {
            Ok(ResolvedStream {
                final_url,
                content_type,
                disposition,
                headers,
            }) => {
                if content_type.contains("zip") || disposition.to_lowercase().contains(".zip") {
                    self.vault_prepare_show(ui, item, false, true);
                    return;
                }
                let Ok(stream_url) = final_url.parse() else {
                    self.vault_message(ui, "player_failed", &AudiovaultError::Message(final_url));
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
                self.audiovault.player_return = Some(self.vault_snapshot());
                self.start_media_item(ui, item, None);
            }
            Err(AudiovaultError::SessionExpired) if allow_retry => {
                self.vault_retry_after_login(ui, AfterLogin::PlayRemote(item));
            }
            Err(error) => self.vault_message(ui, "player_failed", &error),
        }
    }

    fn vault_new_play_request(&mut self) -> PlayRequest {
        self.audiovault.play_request += 1;
        PlayRequest {
            token: self.audiovault.play_request,
            player_generation: self.application.player_session().generation(),
            from_audiovault: is_view(self.view),
        }
    }

    /// A newer request, another item started in the player, or leaving
    /// the `AudioVault` screens the request came from makes it stale.
    fn vault_play_request_current(&self, request: PlayRequest) -> bool {
        request.token == self.audiovault.play_request
            && request.player_generation == self.application.player_session().generation()
            && (!request.from_audiovault || is_view(self.view))
    }

    /// Starting an `AudioVault` item from anywhere: a movie needs its stream
    /// and a TV show episode its cached file. Returns whether it was taken
    /// over (Windows `intercept_start`).
    pub(crate) fn audiovault_intercept_start(
        &mut self,
        ui: &mut dyn ShellUi,
        item: &MediaItem,
    ) -> bool {
        match vault::item_kind(item) {
            Some(AudiovaultKind::Movie) if item.stream_url.is_none() => {
                self.vault_play_remote(ui, item.clone(), true);
                true
            }
            Some(AudiovaultKind::RemoteEpisode) => {
                self.vault_prepare_remote_episode(ui, item.clone(), false, true);
                true
            }
            _ => false,
        }
    }

    /// Back from the player to an `AudioVault` screen: Python
    /// `restore_audiovault_player_results`.
    pub(crate) fn audiovault_restore_after_player(&mut self, ui: &mut dyn ShellUi) {
        let snapshot = self
            .audiovault
            .player_return
            .clone()
            .unwrap_or_else(|| self.vault_snapshot());
        self.audiovault.mode = snapshot.mode;
        if snapshot.view == VaultView::Search {
            self.show_audiovault_search(ui);
        } else if snapshot.view == VaultView::Menu {
            self.show_audiovault_menu(ui);
            return;
        } else {
            self.vault_show_results_screen(ui, &snapshot.title, snapshot.view);
        }
        self.audiovault.parent = snapshot.parent;
        self.vault_show_results(ui, snapshot.results);
    }

    // -----------------------------------------------------------------------
    // Downloads

    /// Python `download_audiovault_selected`.
    fn vault_download_selected(&mut self, ui: &mut dyn ShellUi) {
        if let Some(item) = self.audiovault_selected_item(ui) {
            self.vault_download_item(ui, item, true);
        }
    }

    /// Python `start_download` for `AudioVault` items; `true` when taken
    /// over.
    pub(crate) fn audiovault_start_download(
        &mut self,
        ui: &mut dyn ShellUi,
        item: &MediaItem,
        audio: bool,
    ) -> bool {
        if vault::item_kind(item).is_none() {
            return false;
        }
        if audio {
            self.vault_download_item(ui, item.clone(), true);
        } else {
            self.vault_announce(ui, "audiovault_video_unavailable", &[]);
        }
        true
    }

    /// Python's location prompt of `download_audiovault_item`; `false` when
    /// cancelled.
    fn vault_choose_download_target(&self, ui: &mut dyn ShellUi, item: &mut MediaItem) -> bool {
        let settings = self.application.settings();
        if !settings.ask_download_location_each_time {
            return true;
        }
        let show = vault::item_kind(item) == Some(AudiovaultKind::Show);
        let key = if show {
            "_audiovault_download_target_folder"
        } else {
            "_audiovault_download_target_path"
        };
        if !vault::metadata_text(item, key).is_empty() {
            return true;
        }
        let folder = vault::download_folder(&settings.download_folder, item, show);
        let _ = std::fs::create_dir_all(&folder);
        let chosen = if show {
            ui.choose_download_folder(&self.text("choose_save_folder"), &folder)
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
            ui.choose_save_file(
                &self.text("choose_save_path"),
                Some(&folder),
                &default_name,
                &[extension.as_str()],
            )
            .map(|path| {
                if path.extension().is_none() {
                    path.with_extension(&extension)
                } else {
                    path
                }
            })
        };
        let Some(chosen) = chosen else {
            self.vault_status(ui, "download_cancelled", &[]);
            return false;
        };
        item.metadata.insert(
            key.to_owned(),
            Value::String(chosen.to_string_lossy().into_owned()),
        );
        true
    }

    /// Python `download_audiovault_item`.
    fn vault_download_item(
        &mut self,
        ui: &mut dyn ShellUi,
        mut item: MediaItem,
        allow_retry: bool,
    ) {
        if !self.vault_choose_download_target(ui, &mut item) {
            return;
        }
        match vault::item_kind(&item) {
            Some(AudiovaultKind::Show) => {
                self.vault_prepare_show(ui, item, true, allow_retry);
                self.vault_announce(ui, "audiovault_show_cache_notice", &[]);
            }
            Some(AudiovaultKind::RemoteEpisode) => {
                self.vault_prepare_remote_episode(ui, item, true, allow_retry);
            }
            Some(AudiovaultKind::Episode) => self.vault_copy_episode_to_downloads(ui, &item),
            _ => {
                let folder =
                    Path::new(&self.application.settings().download_folder).join("AudioVault");
                let _ = std::fs::create_dir_all(&folder);
                self.vault_spawn(ui, move |client, _| {
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
                    let result =
                        client.download_movie(&url, &folder, &fallback, selected.as_deref());
                    Some(Event::Movie {
                        item,
                        allow_retry,
                        result,
                    })
                });
            }
        }
    }

    fn vault_finish_movie(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        allow_retry: bool,
        result: Result<PathBuf, AudiovaultError>,
    ) {
        match result {
            Ok(path) => self.vault_announce_complete(ui, &item.title, &path),
            Err(AudiovaultError::SessionExpired) if allow_retry => {
                self.vault_retry_after_login(ui, AfterLogin::Download(item));
            }
            Err(error) => self.vault_message(ui, "download_failed", &error),
        }
    }

    fn vault_announce_complete(&self, ui: &mut dyn ShellUi, title: &str, path: &Path) {
        self.vault_announce(
            ui,
            "audiovault_download_complete_path",
            &[("title", title), ("path", &path.to_string_lossy())],
        );
    }

    /// Python `copy_audiovault_episode_to_downloads`.
    fn vault_copy_episode_to_downloads(&self, ui: &mut dyn ShellUi, item: &MediaItem) {
        let selected = vault::metadata_text(item, "_audiovault_download_target_path");
        let target = if selected.trim().is_empty() {
            vault::episode_download_target(&self.application.settings().download_folder, item)
        } else {
            PathBuf::from(selected.trim())
        };
        let source = PathBuf::from(item.local_path.clone().unwrap_or_default());
        // Python lets a copy failure end the handler without a message.
        if apricot_platform::audiovault::copy_episode(&source, &target).is_ok() {
            self.vault_announce_complete(ui, &item.title, &target);
        }
    }

    /// Python `copy_audiovault_show_to_downloads`.
    fn vault_copy_show_to_downloads(
        &self,
        ui: &mut dyn ShellUi,
        show: &MediaItem,
        cache_dir: &Path,
    ) {
        let selected = vault::metadata_text(show, "_audiovault_download_target_folder");
        let merge = !selected.trim().is_empty();
        let target = if merge {
            PathBuf::from(selected.trim())
        } else {
            vault::show_download_target(&self.application.settings().download_folder, show)
        };
        if apricot_platform::audiovault::copy_show(cache_dir, &target, merge).is_ok() {
            self.vault_announce_complete(ui, &show.title, &target);
        }
    }

    // -----------------------------------------------------------------------
    // Worker events

    /// Windows `AUDIOVAULT_TIMER_ID`: every finished worker's result, in
    /// order; `true` while workers run.
    pub(crate) fn poll_audiovault(&mut self, ui: &mut dyn ShellUi) -> bool {
        if self.audiovault.polling {
            return true;
        }
        while let Ok(event) = self.audiovault.receiver.try_recv() {
            if !matches!(event, Event::Progress { .. }) {
                self.audiovault.running = self.audiovault.running.saturating_sub(1);
            }
            self.audiovault.polling = true;
            self.vault_handle_event(ui, event);
            self.audiovault.polling = false;
        }
        self.audiovault.running > 0
    }

    fn vault_handle_event(&mut self, ui: &mut dyn ShellUi, event: Event) {
        match event {
            Event::Login {
                email,
                remember,
                after,
                result,
            } => self.vault_finish_login(ui, &email, remember, after, result),
            Event::Results {
                generation,
                view,
                retry,
                result,
                mode,
            } => {
                if self.audiovault.results_generation == generation {
                    self.vault_finish_results(ui, view, retry, result, mode);
                }
            }
            Event::Manifest {
                show,
                generation,
                allow_retry,
                result,
            } => self.vault_finish_manifest(ui, show, generation, allow_retry, result),
            Event::Progress {
                task,
                message_key,
                title,
                percent,
            } => {
                if task != 0 && task != self.audiovault.progress_task {
                    return;
                }
                let percent = percent.to_string();
                self.vault_status(ui, message_key, &[("title", &title), ("percent", &percent)]);
            }
            Event::ShowDone {
                show,
                download_after,
                allow_retry,
                task,
                generation,
                result,
            } => self.vault_finish_show(
                ui,
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
                request,
                result,
            } => self.vault_finish_episode(
                ui,
                item,
                download_after,
                allow_retry,
                task,
                request,
                result,
            ),
            Event::Stream {
                item,
                allow_retry,
                request,
                result,
            } => self.vault_finish_stream(ui, item, allow_retry, request, result),
            Event::Movie {
                item,
                allow_retry,
                result,
            } => self.vault_finish_movie(ui, item, allow_retry, result),
        }
    }

    /// Python `load_audiovault_recent_worker` and `search_audiovault_worker`
    /// back on the UI.
    fn vault_finish_results(
        &mut self,
        ui: &mut dyn ShellUi,
        view: VaultView,
        retry: Option<AfterLogin>,
        result: Result<Vec<vault::VaultRecord>, AudiovaultError>,
        mode: AudiovaultMode,
    ) {
        match result {
            Ok(records) => {
                // The list belongs to the screen that asked for the results.
                let current = matches!(
                    (view, self.view, self.audiovault.view),
                    (VaultView::Search, View::AudiovaultSearch, VaultView::Search)
                        | (
                            VaultView::Recent(_),
                            View::AudiovaultResults,
                            VaultView::Recent(_)
                        )
                );
                if !current {
                    return;
                }
                let section = match view {
                    VaultView::Recent(mode) => Some(mode.recent_section()),
                    _ => None,
                };
                let results = vault::results_from_records(&records, mode, section, &self.catalog());
                self.vault_show_results(ui, results);
            }
            Err(AudiovaultError::SessionExpired) if retry.is_some() => {
                self.vault_retry_after_login(ui, retry.unwrap_or(AfterLogin::Nothing));
            }
            Err(error) => self.vault_message(ui, "audiovault_search_failed", &error),
        }
    }
}
