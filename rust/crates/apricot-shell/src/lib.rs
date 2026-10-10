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
mod type_ahead;
mod ui;

pub use keyboard::KeyRoute;

pub use main_menu::{MainMenuSelection, last_activated_index, preserved_index};
pub use type_ahead::TypeAhead;
pub use ui::ShellUi;

use apricot_app::{Application, MainMenuModel};
use apricot_core::TranslationCatalog;

/// The screen shown in the main window (Windows `MainView`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum View {
    MainMenu,
}

/// The main window's application state and the decisions made on it.
pub struct Shell {
    pub application: Application,
    view: View,
    model: MainMenuModel,
    /// Windows `last_activated_menu_item`: the main menu returns to it.
    last_activated_menu_item: Option<&'static str>,
}

impl Shell {
    pub fn new(application: Application) -> Self {
        let model = application.main_menu_model();
        Self {
            application,
            view: View::MainMenu,
            model,
            last_activated_menu_item: None,
        }
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
        if item.id == "exit" {
            ui.exit();
            return;
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
            _ => {}
        }
        // Every other action arrives with its macOS screen.
        let catalog = self.catalog();
        if let Some(message) = apricot_app::unavailable_action_message(&catalog, action_id, None) {
            ui.set_status(&message, true);
        }
    }
}

#[cfg(test)]
mod tests;
