//! The platform-neutral coordinator of the main window.
//!
//! The Windows UI (`apricot-ui-windows/src/win32.rs`) decides itself which
//! screen opens, what the status line says, which row is selected and when
//! something is announced. This crate holds the same decisions without any
//! native control, so the macOS UI behaves identically. A native UI owns one
//! [`Shell`], forwards user input to it and implements [`ShellUi`] to show
//! the result. Windows moves onto this crate in unit W1
//! (`docs/MACOS_RUST_PORT_PLAN.md`).

mod keyboard;
mod main_menu;
mod player;
mod repeat;
mod type_ahead;
mod ui;

pub use keyboard::{FocusKind, KeyRoute};
pub use player::{format_duration, local_file_item};
pub use repeat::KeyRelease;

pub use main_menu::{MainMenuSelection, last_activated_index, preserved_index};
pub use type_ahead::TypeAhead;
pub use ui::{PlayerFocus, ShellUi};

use apricot_app::{Application, MainMenuModel};
use apricot_core::TranslationCatalog;

/// The screen shown in the main window (Windows `MainView`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum View {
    MainMenu,
    Player,
}

/// The main window's application state and the decisions made on it.
pub struct Shell {
    pub application: Application,
    view: View,
    model: MainMenuModel,
    /// Windows `last_activated_menu_item`: the main menu returns to it.
    last_activated_menu_item: Option<&'static str>,
    player: player::PlayerRuntime,
    key_repeat: Option<repeat::KeyRepeat>,
}

impl Shell {
    pub fn new(application: Application) -> Self {
        let model = application.main_menu_model();
        Self {
            application,
            view: View::MainMenu,
            model,
            last_activated_menu_item: None,
            player: player::PlayerRuntime::default(),
            key_repeat: None,
        }
    }

    /// Windows `remember_menu_item`.
    pub(crate) const fn remember_menu_item(&mut self, item_id: &'static str) {
        self.last_activated_menu_item = Some(item_id);
    }

    #[must_use]
    pub const fn view(&self) -> View {
        self.view
    }

    /// The main menu as it is currently shown.
    #[must_use]
    pub const fn main_menu(&self) -> &MainMenuModel {
        &self.model
    }

    #[must_use]
    pub fn catalog(&self) -> TranslationCatalog {
        apricot_app::embedded_catalog(&self.application.settings().language)
    }

    /// Windows `catalog_text`.
    #[must_use]
    pub fn text(&self, key: &str) -> String {
        self.catalog().text(key).to_owned()
    }

    /// Windows `run_win32` after the window is shown: the main menu with its
    /// first row and the one-time startup announcement
    /// (`take_startup_announcement`, spoken by `announce_player_text`).
    pub fn start(&mut self, ui: &mut dyn ShellUi) {
        self.model = self.application.main_menu_model();
        ui.show_main_menu(&self.model, &self.text("open"), 0, true);
        ui.set_status(&self.text("ready"), false);
        if let Some(key) = self.application.take_startup_announcement() {
            let message = self.text(key);
            ui.set_status(&message, true);
        }
    }

    /// Windows `show_main_menu`: the main menu with the last opened item
    /// selected, "Ready" on the status line, focus on the list.
    pub fn show_main_menu(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        self.application.navigate_main_menu();
        self.view = View::MainMenu;
        self.refresh_main_menu(ui, MainMenuSelection::LastActivated, None, true);
        ui.set_status(&self.text("ready"), false);
    }

    /// Windows `refresh_main_menu`. `current` is the row selected now, for
    /// [`MainMenuSelection::Preserve`].
    pub fn refresh_main_menu(
        &mut self,
        ui: &mut dyn ShellUi,
        selection: MainMenuSelection,
        current: Option<usize>,
        focus: bool,
    ) {
        let old_label = current
            .and_then(|index| self.model.items.get(index))
            .map(|item| item.label.clone());
        self.model = self.application.main_menu_model();
        let selected = match selection {
            MainMenuSelection::LastActivated => {
                last_activated_index(&self.model.items, self.last_activated_menu_item)
            }
            MainMenuSelection::Preserve => {
                let labels: Vec<_> = self
                    .model
                    .items
                    .iter()
                    .map(|item| item.label.as_str())
                    .collect();
                preserved_index(&labels, old_label.as_deref(), current)
            }
        };
        ui.show_main_menu(&self.model, &self.text("open"), selected, focus);
    }

    /// Windows `activate_main_menu_selection` for the row at `index`.
    pub fn activate_main_menu_row(&mut self, ui: &mut dyn ShellUi, index: usize) {
        let Some(item) = self.model.items.get(index).cloned() else {
            return;
        };
        self.last_activated_menu_item = Some(item.id);
        match item.id {
            "exit" => {
                ui.exit();
                return;
            }
            "play_file" => {
                self.open_media_file(ui);
                return;
            }
            _ => {}
        }
        // Every other destination arrives with its macOS screen; until then
        // the row says so, as Windows did for actions without a Rust route.
        let feature = item.label.split('\t').next().unwrap_or(&item.label);
        let message = apricot_app::unavailable_feature_message(&self.catalog(), feature);
        ui.set_status(&message, true);
    }

    /// An action chosen by its shortcut or from a menu (Windows
    /// `activate_action`).
    pub fn activate_action(&mut self, ui: &mut dyn ShellUi, action_id: &str) {
        self.leave_player_for_global_navigation(ui, action_id);
        if self.activate_player_action(ui, action_id) {
            return;
        }
        match action_id {
            "open_main_menu" => {
                self.show_main_menu(ui);
                return;
            }
            "open_selected" => {
                if self.view == View::MainMenu
                    && let Some(row) = ui.selected_row()
                {
                    self.activate_main_menu_row(ui, row);
                }
                return;
            }
            "open_play_file" => {
                self.open_media_file(ui);
                return;
            }
            _ => {}
        }
        // Every other action arrives with its macOS screen.
        let catalog = self.catalog();
        if let Some(message) = apricot_app::unavailable_action_message(&catalog, action_id, None) {
            ui.set_status(&message, true);
        }
    }

    /// The player actions of Windows `activate_action`; `true` when
    /// `action_id` is one of them.
    fn activate_player_action(&mut self, ui: &mut dyn ShellUi, action_id: &str) -> bool {
        match action_id {
            "background_play_pause" => self.background_play_pause(ui),
            "player_play_pause" => self.toggle_player_pause(ui),
            "close_player" => self.close_current_player(ui),
            "player_back" => self.player_back(ui),
            // The player page's Back buttons (`player_model` navigation row).
            "player_back_to_results" | "player_back_to_main_menu" => self.navigate_back(ui),
            "player_back_keep_playing" => self.leave_player_to_main_menu(ui, true),
            "player_previous" => self.navigate_player_relative(ui, -1, false),
            "player_next" => self.navigate_player_relative(ui, 1, false),
            "player_time" => self.announce_player_time(ui),
            "player_volume_status" => self.announce_player_volume(ui),
            "player_format_status" => self.announce_player_format(ui),
            "player_seek_back" => self.seek_player(ui, -self.configured_seek_seconds()),
            "player_seek_forward" => self.seek_player(ui, self.configured_seek_seconds()),
            "player_seek_back_large" => self.seek_player(ui, -60.0),
            "player_seek_forward_large" => self.seek_player(ui, 60.0),
            "player_seek_back_huge" => self.seek_player(ui, -600.0),
            "player_seek_forward_huge" => self.seek_player(ui, 600.0),
            "player_seek_start" => self.seek_player_to_start(ui),
            "player_seek_end" => self.seek_player_to_end(ui),
            "player_volume_up" => self.adjust_player_volume(ui, self.configured_volume_step()),
            "player_volume_down" => self.adjust_player_volume(ui, -self.configured_volume_step()),
            "player_speed_up" => self.adjust_player_speed(ui, self.configured_speed_step()),
            "player_speed_down" => self.adjust_player_speed(ui, -self.configured_speed_step()),
            "player_pitch_up" => self.adjust_player_pitch(ui, self.configured_pitch_step()),
            "player_pitch_down" => self.adjust_player_pitch(ui, -self.configured_pitch_step()),
            "player_reset_speed_pitch" => self.reset_player_speed_pitch(ui),
            "player_repeat" => {
                self.toggle_player_session_setting(ui, apricot_app::SessionToggle::Repeat);
            }
            "player_shuffle" => {
                self.toggle_player_session_setting(ui, apricot_app::SessionToggle::Shuffle);
            }
            "player_bass_boost" => {
                self.toggle_player_session_setting(ui, apricot_app::SessionToggle::BassBoost);
            }
            "player_volume_boost" => {
                self.toggle_player_session_setting(ui, apricot_app::SessionToggle::VolumeBoost);
            }
            "session_autoplay_next" => {
                self.toggle_player_session_setting(ui, apricot_app::SessionToggle::AutoplayNext);
            }
            _ => return false,
        }
        true
    }

    /// Windows `leave_player_for_global_navigation`: leaving the player page
    /// for another screen stops playback unless background playback is on.
    fn leave_player_for_global_navigation(&mut self, ui: &mut dyn ShellUi, action_id: &str) {
        if !matches!(
            action_id,
            "open_main_menu"
                | "open_search"
                | "open_play_from_folder"
                | "open_play_file"
                | "open_direct_link"
                | "open_favorites"
                | "open_bookmarks"
                | "open_playlists"
                | "open_subscriptions"
                | "open_current_downloads"
                | "open_history"
                | "open_podcasts_rss"
                | "open_settings"
                | "new_subscription_videos"
                | "open_audiovault"
                | "open_spotify"
                | "spotify_accounts"
                | "spotify_search"
                | "spotify_library"
                | "spotify_liked_songs"
                | "spotify_playlists"
                | "spotify_daily_mixes"
        ) || self.view != View::Player
        {
            return;
        }
        if self.application.settings().enable_background_playback {
            self.application.keep_player_return_frame();
            return;
        }
        self.close_player_runtime(ui);
        if matches!(
            action_id,
            "open_settings" | "open_bookmarks" | "open_play_file" | "open_play_from_folder"
        ) {
            // These open over the current screen; without a player there is
            // no player page to return to.
            self.show_main_menu(ui);
        }
    }
}

#[cfg(test)]
mod tests;
