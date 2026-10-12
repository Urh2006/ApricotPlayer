//! The list screens of the main window (Windows `MainView` screens with the
//! shared list, the Open button and screen buttons), and the local folder
//! screen (Windows `open_media_folder`, `poll_local_folder_scan`,
//! `refresh_local_folder` and the folder buttons).

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
};

use apricot_core::{MediaItem, Route, RouteFrame, TranslationCatalog};

use crate::{MainMenuSelection, Shell, ShellUi, View};

/// One control of a list screen, in Tab order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScreenControl {
    /// The screen's list.
    List,
    /// A push button; pressing it calls [`Shell::activate_screen_button`]
    /// with `id`.
    Button { id: &'static str, label: String },
    /// A labelled one-line text field; Return calls
    /// [`Shell::screen_text_submitted`]. `value` replaces the text, `None`
    /// keeps what was typed.
    TextField {
        id: &'static str,
        label: String,
        value: Option<String>,
    },
    /// A labelled drop-down list; a change calls
    /// [`Shell::screen_choice_changed`]. `selected` replaces the selection,
    /// `None` keeps it.
    Choice {
        id: &'static str,
        label: String,
        options: Vec<String>,
        selected: Option<usize>,
    },
}

/// What a list screen shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListScreen {
    /// The list's accessible name (Windows `set_control_name`).
    pub list_name: String,
    pub rows: Vec<String>,
    pub selected: Option<usize>,
    /// The controls in Tab order.
    pub controls: Vec<ScreenControl>,
    /// Focus moves to the list.
    pub focus_list: bool,
}

/// Windows `PendingLocalFolderScan`.
pub(crate) struct PendingFolderScan {
    generation: u64,
    path: PathBuf,
    cancelled: Arc<AtomicBool>,
    receiver: Receiver<Result<Vec<MediaItem>, String>>,
}

#[derive(Default)]
pub(crate) struct ScreenState {
    pending_scan: Option<PendingFolderScan>,
    next_scan_generation: u64,
}

/// Windows `local_folder_result_label`.
#[must_use]
pub fn local_folder_result_label(item: &MediaItem, catalog: &TranslationCatalog) -> String {
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

impl Shell {
    fn button(&self, id: &'static str, label_key: &str) -> ScreenControl {
        ScreenControl::Button {
            id,
            label: self.text(label_key),
        }
    }

    /// A button of a list screen was pressed (Windows `handle_window_command`).
    pub fn activate_screen_button(&mut self, ui: &mut dyn ShellUi, id: &str) {
        if self.audiovault_command(ui, id) {
            return;
        }
        match id {
            "open" => self.activate_selection(ui),
            "back" => self.navigate_back(ui),
            "play_folder" => self.play_current_local_folder(ui, false),
            "shuffle_folder" => self.play_current_local_folder(ui, true),
            "add_folder_to_queue" => self.add_current_local_folder_to_queue(ui),
            "folder_playback_queue" => self.activate_action(ui, "open_playback_queue"),
            "load_trending" => self.load_trending_results(ui),
            "collection_remove" => self.remove_selected_collection_item(ui),
            "history_clear" => self.clear_history(ui),
            "playlist_create" => self.create_user_playlist(ui, None),
            "direct_play" => self.activate_direct_link(ui, "play"),
            "direct_download_audio" => self.activate_direct_link(ui, "download_audio"),
            "direct_download_video" => self.activate_direct_link(ui, "download_video"),
            "direct_copy_stream" => self.activate_direct_link(ui, "copy_stream_url"),
            "search"
            | "search_play"
            | "search_download_audio"
            | "search_download_video"
            | "search_add_favorite" => self.activate_search_button(ui, id),
            _ => {
                if !self.download_command(ui, id) {
                    let _ = self.podcast_command(ui, id);
                }
            }
        }
    }

    /// Windows `activate_selection`: Enter or Open on the screen's list.
    pub(crate) fn activate_selection(&mut self, ui: &mut dyn ShellUi) {
        let Some(row) = ui.selected_row() else {
            return;
        };
        match self.view {
            View::MainMenu => self.activate_main_menu_row(ui, row),
            View::LocalFolder => self.activate_local_folder_row(ui, row),
            View::Results | View::Trending => self.activate_result_selection(ui),
            View::Collection => self.activate_collection_selection(ui),
            View::Favorites | View::History => self.activate_library_selection(ui),
            View::UserPlaylists | View::UserPlaylistItems => self.activate_playlist_selection(ui),
            View::RssFeeds
            | View::RssItems
            | View::PodcastSearchResults
            | View::PodcastCategories => self.activate_podcast_selection(ui),
            View::DownloadQueue => self.start_selected_queued_download(ui, None),
            View::AudiovaultMenu | View::AudiovaultSearch | View::AudiovaultResults => {
                self.audiovault_activate(ui);
            }
            View::SpotifyHub | View::SpotifyAccounts => self.spotify_activate(ui),
            View::SpotifyBrowse => self.spotify_browse_activate(ui),
            // Python `play_selected` on the empty result list.
            View::Search => ui.show_error(&self.text("no_selection")),
            View::DirectLink | View::Player => {}
        }
    }

    /// Windows `open_media_folder`: Play folder in the main menu.
    pub fn open_media_folder(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("play_folder");
        self.stop_key_repeat(ui);
        let title = self.text("select_media_folder");
        let Some(path) = ui.choose_folder(&title) else {
            ui.focus_primary_control();
            return;
        };
        self.start_local_folder_scan(ui, path);
    }

    /// Windows `start_local_folder_scan`.
    pub fn start_local_folder_scan(&mut self, ui: &mut dyn ShellUi, path: PathBuf) {
        self.cancel_local_folder_scan();
        self.screens.next_scan_generation =
            self.screens.next_scan_generation.wrapping_add(1).max(1);
        let generation = self.screens.next_scan_generation;
        let worker_path = path.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = apricot_platform::scan_local_media_folder_with_cancel(
                &worker_path,
                &worker_cancelled,
            )
            .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.screens.pending_scan = Some(PendingFolderScan {
            generation,
            path,
            cancelled,
            receiver,
        });
        ui.set_status("Loading folder...", true);
        ui.set_background_polling(true);
    }

    /// Windows `cancel_local_folder_scan`.
    pub(crate) fn cancel_local_folder_scan(&mut self) {
        if let Some(pending) = self.screens.pending_scan.take() {
            pending.cancelled.store(true, Ordering::Release);
        }
    }

    /// Windows `poll_local_folder_scan` and the other background timers;
    /// returns `false` when nothing is left to poll.
    pub fn poll_background(&mut self, ui: &mut dyn ShellUi) -> bool {
        let youtube = self.poll_youtube(ui);
        let related = self.poll_related_videos(ui);
        let folder = self.poll_folder_scan(ui);
        let device = self.poll_saved_audio_device_check(ui);
        let chapters = self.poll_external_chapters(ui);
        let bpm = self.poll_bpm_analyses(ui);
        let edits = self.poll_edit_saves(ui);
        let podcasts = self.poll_podcast_work(ui);
        let downloads = self.poll_downloads(ui) | self.poll_conversions(ui);
        let cookies =
            self.poll_cookie_events(ui) | self.poll_audiovault(ui) | self.poll_spotify(ui);
        youtube
            || related
            || folder
            || device
            || chapters
            || bpm
            || edits
            || podcasts
            || downloads
            || cookies
    }

    fn poll_folder_scan(&mut self, ui: &mut dyn ShellUi) -> bool {
        let Some(pending) = self.screens.pending_scan.as_ref() else {
            return false;
        };
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => {
                Err("Local folder scan stopped unexpectedly".to_owned())
            }
        };
        let Some(pending) = self.screens.pending_scan.take() else {
            return false;
        };
        let _ = pending.generation;
        match result {
            Ok(items) if !items.is_empty() => {
                self.application.load_local_folder(pending.path, items);
                self.application.navigate_main_menu();
                self.application
                    .navigate_to(RouteFrame::new(Route::LocalFolder));
                self.view = View::LocalFolder;
                self.refresh_local_folder(ui, true, false);
            }
            Ok(_) => {
                let message = self.text("folder_no_media");
                ui.set_status(&message, true);
                ui.show_error(&message);
                ui.focus_primary_control();
            }
            Err(error) => {
                ui.set_status(&error, true);
                ui.show_error(&error);
                ui.focus_primary_control();
            }
        }
        false
    }

    /// Windows `refresh_local_folder`.
    pub(crate) fn refresh_local_folder(
        &mut self,
        ui: &mut dyn ShellUi,
        focus: bool,
        announce_status: bool,
    ) {
        self.sync_window_fullscreen(ui);
        let catalog = self.catalog();
        let session = self.application.local_folder_session();
        let (rows, selected, message) = if session.is_empty() {
            (
                vec![catalog.text("folder_no_media").to_owned()],
                0,
                catalog.text("folder_no_media").to_owned(),
            )
        } else {
            (
                session
                    .items()
                    .iter()
                    .map(|item| local_folder_result_label(item, &catalog))
                    .collect(),
                session.selected_index().min(session.items().len() - 1),
                catalog
                    .text("folder_loaded")
                    .replace("{count}", &session.items().len().to_string()),
            )
        };
        // Windows creation order: list, Open (Play), Back, then the folder
        // buttons.
        let screen = ListScreen {
            list_name: catalog.text("play_from_folder").to_owned(),
            rows,
            selected: Some(selected),
            controls: vec![
                ScreenControl::List,
                self.button("open", "play"),
                self.button("back", "back"),
                self.button("play_folder", "play_folder"),
                self.button("shuffle_folder", "shuffle_folder"),
                self.button("add_folder_to_queue", "add_folder_to_queue"),
                self.button("folder_playback_queue", "playback_queue"),
            ],
            focus_list: focus,
        };
        ui.show_list_screen(&screen);
        ui.set_status(&message, announce_status);
    }

    /// Windows `activate_local_folder_selection`.
    fn activate_local_folder_row(&mut self, ui: &mut dyn ShellUi, index: usize) {
        if let Some(item) = self.application.prepare_local_folder_playback(index, false) {
            self.start_sequence_media_item(ui, item, None);
        }
    }

    /// Windows `play_current_local_folder`.
    fn play_current_local_folder(&mut self, ui: &mut dyn ShellUi, shuffle: bool) {
        let selected = ui.selected_row().unwrap_or_default();
        let Some(item) = self
            .application
            .prepare_local_folder_playback(selected, shuffle)
        else {
            ui.set_status(&self.text("folder_no_media"), true);
            return;
        };
        self.start_sequence_media_item(ui, item, Some(shuffle));
    }

    /// Windows `add_current_local_folder_to_queue`.
    fn add_current_local_folder_to_queue(&mut self, ui: &mut dyn ShellUi) {
        match self.application.add_local_folder_to_playback_queue() {
            Ok(outcome) => {
                let message = self
                    .text("folder_queue_added")
                    .replace("{count}", &outcome.added.to_string());
                ui.set_status(&message, true);
            }
            Err(error) => {
                let message = format!("Playback queue was not updated: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    /// Windows `navigate_back`: playback stops when leaving the player and
    /// the previous screen returns with focus on its list.
    #[allow(clippy::too_many_lines)] // One arm per screen, as on Windows.
    pub(crate) fn navigate_back(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        if crate::audiovault::is_view(self.view) {
            // Python `back_from_audiovault`.
            self.audiovault_back(ui);
            return;
        }
        if crate::spotify::is_view(self.view) {
            self.spotify_back(ui);
            return;
        }
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        let leaving_collection = self.view == View::Collection;
        if self.view == View::Player {
            self.close_player_runtime(ui);
        }
        if leaving_collection {
            let _ = self.application.pop_youtube_collection();
        }
        let frame = self
            .application
            .navigate_back()
            .unwrap_or_else(|| RouteFrame::new(Route::MainMenu));
        match frame.route {
            Route::Search => {
                self.view = View::Search;
                let screen = self.search_screen(vec![self.text("search_results_empty")], 0, false);
                ui.show_list_screen(&screen);
                ui.focus_screen_control("search_query");
            }
            Route::Results => {
                self.view = View::Results;
                self.refresh_results(ui, true);
            }
            Route::Trending => self.restore_trending_view(ui, &frame),
            Route::UserPlaylists => {
                self.view = View::UserPlaylists;
                self.refresh_user_playlists(ui, true);
            }
            Route::UserPlaylistItems => {
                self.view = View::UserPlaylistItems;
                self.refresh_user_playlist_items(ui, true, false);
            }
            Route::Favorites | Route::History => {
                self.view = if frame.route == Route::History {
                    View::History
                } else {
                    View::Favorites
                };
                let saved = frame
                    .parameters
                    .get("index")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok());
                self.refresh_media_collection(ui, true, saved);
            }
            Route::DirectLink => self.restore_direct_link(ui),
            Route::ChannelResults | Route::PlaylistResults => {
                self.view = View::Collection;
                self.refresh_youtube_collection(ui, true);
            }
            Route::LocalFolder => {
                self.view = View::LocalFolder;
                self.refresh_local_folder(ui, true, false);
            }
            Route::Player => {
                self.view = View::Player;
                self.refresh_player(ui, crate::PlayerFocus::Preserve);
            }
            Route::RssFeeds
            | Route::RssItems
            | Route::PodcastSearchResults
            | Route::PodcastCategories => {
                let saved = frame
                    .parameters
                    .get("index")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok());
                self.restore_podcast_view(ui, frame.route, saved);
            }
            Route::AudiovaultMenu
            | Route::AudiovaultSearch
            | Route::AudiovaultResults
            | Route::AudiovaultEpisodes => self.audiovault_restore_after_player(ui),
            Route::SpotifyHub => self.show_spotify_hub(ui),
            Route::SpotifyBrowse => self.spotify_browse_restore(ui),
            Route::SpotifyAccounts => self.show_spotify_accounts(ui, None),
            Route::DownloadQueue => {
                self.view = View::DownloadQueue;
                self.refresh_download_queue(ui, true, false);
            }
            Route::Bookmarks => {
                self.view = View::MainMenu;
                self.refresh_main_menu(ui, MainMenuSelection::LastActivated, None, true);
                self.show_bookmarks_dialog(ui, false, true);
            }
            _ => {
                self.application.navigate_main_menu();
                self.view = View::MainMenu;
                self.refresh_main_menu(ui, MainMenuSelection::LastActivated, None, true);
            }
        }
    }
}
