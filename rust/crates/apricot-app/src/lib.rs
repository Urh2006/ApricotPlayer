//! Application coordinator state. UI controls are projections of this state.

pub mod action_finder;
pub mod activation;
pub mod application;
pub mod local_folder;
pub mod main_menu;
pub mod media_collection_controller;
pub mod playback_queue;
pub mod playback_queue_controller;
pub mod playback_sequence;
pub mod player_information;
pub mod player_model;
pub mod player_session;
pub mod search_session;
pub mod settings_controller;
pub mod settings_model;
pub mod settings_session;
pub mod user_playlist_controller;

use apricot_core::NavigationStack;
pub use apricot_media::YoutubeSearchKind;

pub use action_finder::{ActionFinderContext, ActionFinderItem, ActionFinderModel};
pub use activation::ActivationRequest;
pub use application::{Application, PlayerNavigationOrigin, PlayerNavigationOutcome};
pub use local_folder::{DEFAULT_FOLDER_BATCH_SIZE, LocalFolderSession};
pub use main_menu::{
    MainMenuAvailability, MainMenuItem, MainMenuModel, MenuVisibility, embedded_catalog,
    english_catalog,
};
pub use media_collection_controller::{
    CollectionAddOutcome, MediaCollectionController, MediaCollectionControllerError,
};
pub use playback_queue::{PlaybackQueue, QueueAddOutcome, QueueBatchAddOutcome};
pub use playback_queue_controller::{PlaybackQueueController, PlaybackQueueControllerError};
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
pub use user_playlist_controller::{
    PlaylistAddOutcome, PlaylistCreateOutcome, UserPlaylistController, UserPlaylistControllerError,
};

#[derive(Debug, Default)]
pub struct AppState {
    pub favorites: MediaCollectionController,
    pub history: MediaCollectionController,
    pub navigation: NavigationStack,
    pub local_folder: LocalFolderSession,
    pub playback_queue: PlaybackQueueController,
    pub player: PlayerSession,
    pub player_sequence: PlaybackSequence,
    pub search: SearchSession,
    pub user_playlists: UserPlaylistController,
}
