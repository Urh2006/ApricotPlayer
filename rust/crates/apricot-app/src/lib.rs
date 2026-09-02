//! Application coordinator state. UI controls are projections of this state.

pub mod action_finder;
pub mod activation;
pub mod application;
pub mod main_menu;
pub mod settings_controller;
pub mod settings_model;
pub mod settings_session;

use apricot_core::{MediaItem, NavigationStack};

pub use action_finder::{ActionFinderContext, ActionFinderItem, ActionFinderModel};
pub use activation::ActivationRequest;
pub use application::Application;
pub use main_menu::{
    MainMenuAvailability, MainMenuItem, MainMenuModel, MenuVisibility, embedded_catalog,
    english_catalog,
};
pub use settings_controller::{SettingsController, SettingsControllerError};
pub use settings_model::{
    SettingsChoiceOption, SettingsCommand, SettingsControl, SettingsScreenModel,
    SettingsSectionItem, SettingsValueType, ShortcutActionItem,
};
pub use settings_session::{SettingsDraft, SettingsDraftError};

#[derive(Debug, Default)]
pub struct AppState {
    pub navigation: NavigationStack,
    pub player: PlayerSession,
}

#[derive(Debug, Default)]
pub struct PlayerSession {
    pub open: bool,
    pub current_item: Option<MediaItem>,
    pub volume: Option<f64>,
    pub output_device: Option<String>,
    pub autoplay_next: bool,
}

impl PlayerSession {
    pub fn close(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::PlayerSession;

    #[test]
    fn closing_player_resets_session_values() {
        let mut session = PlayerSession {
            open: true,
            current_item: None,
            volume: Some(80.0),
            output_device: Some("speakers".to_owned()),
            autoplay_next: true,
        };
        session.close();
        assert_eq!(session.volume, None);
        assert_eq!(session.output_device, None);
        assert!(!session.autoplay_next);
    }
}
