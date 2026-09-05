//! Application coordinator state. UI controls are projections of this state.

pub mod action_finder;
pub mod activation;
pub mod application;
pub mod main_menu;
pub mod playback_sequence;
pub mod player_model;
pub mod player_session;
pub mod search_session;
pub mod settings_controller;
pub mod settings_model;
pub mod settings_session;

use apricot_core::NavigationStack;
pub use apricot_media::YoutubeSearchKind;

pub use action_finder::{ActionFinderContext, ActionFinderItem, ActionFinderModel};
pub use activation::ActivationRequest;
pub use application::{Application, PlayerNavigationOutcome};
pub use main_menu::{
    MainMenuAvailability, MainMenuItem, MainMenuModel, MenuVisibility, embedded_catalog,
    english_catalog,
};
pub use playback_sequence::{PlaybackSequence, PlaybackSequenceSource};
pub use player_model::{
    PlayerControlModel, PlayerControlRole, PlayerScreenModel, PlayerToggle, PlayerViewState,
    TransportState,
};
pub use player_session::{
    AudioSession, EqualizerSession, PlaybackPhase, PlayerSession, PlayerSessionDefaults,
    SessionToggle,
};
pub use search_session::{
    DYNAMIC_SEARCH_PAGE_SIZE, SearchApplyOutcome, SearchPhase, SearchSession, SearchSessionError,
    SearchWork, SearchWorkKind,
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
    pub player_sequence: PlaybackSequence,
    pub search: SearchSession,
}
