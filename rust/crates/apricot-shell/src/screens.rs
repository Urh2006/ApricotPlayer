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
        match id {
            "open" => self.activate_selection(ui),
            "back" => self.navigate_back(ui),
            "play_folder" => self.play_current_local_folder(ui, false),
            "shuffle_folder" => self.play_current_local_folder(ui, true),
            "add_folder_to_queue" => self.add_current_local_folder_to_queue(ui),
            "folder_playback_queue" => self.activate_action(ui, "open_playback_queue"),
            _ => {}
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
            View::Player => {}
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
        let folder = self.poll_folder_scan(ui);
        let device = self.poll_saved_audio_device_check(ui);
        let chapters = self.poll_external_chapters(ui);
        let bpm = self.poll_bpm_analyses(ui);
        let edits = self.poll_edit_saves(ui);
        folder || device || chapters || bpm || edits
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
    pub(crate) fn navigate_back(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        self.cancel_local_folder_scan();
        if self.view == View::Player {
            self.close_player_runtime(ui);
        }
        let frame = self
            .application
            .navigate_back()
            .unwrap_or_else(|| RouteFrame::new(Route::MainMenu));
        match frame.route {
            Route::LocalFolder => {
                self.view = View::LocalFolder;
                self.refresh_local_folder(ui, true, false);
            }
            Route::Player => {
                self.view = View::Player;
                self.refresh_player(ui, crate::PlayerFocus::Preserve);
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
