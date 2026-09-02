//! Platform-neutral product contract for `ApricotPlayer` 2.0.

pub mod action;
pub mod announcement;
pub mod audio;
pub mod context_menu;
pub mod error;
pub mod locale;
pub mod media;
pub mod menu;
pub mod navigation;
pub mod screen;
pub mod setting;
pub mod settings_layout;

pub use action::{ActionDefinition, ActionId, ActionScope, RepeatPolicy};
pub use announcement::{
    AnnouncementBroker, AnnouncementPriority, AnnouncementRequest, GenerationToken,
};
pub use audio::{EqualizerBand, FactoryEqualizerPreset};
pub use context_menu::{CONTEXT_MENUS, ContextMenuDefinition};
pub use error::{AppError, ErrorDomain, RecoveryAction};
pub use locale::{LanguageDefinition, TranslationCatalog};
pub use media::{MediaId, MediaItem, MediaKind, MediaSource};
pub use menu::{CUSTOMIZABLE_MAIN_MENU, MainMenuDefinition, PERMANENT_MAIN_MENU_IDS};
pub use navigation::{FocusId, NavigationStack, Route, RouteFrame};
pub use screen::{PrimaryControlRole, SCREENS, ScreenDefinition, ScreenKind};
pub use setting::SettingId;
pub use settings_layout::{
    INTERNAL_SETTINGS, SETTINGS_SECTIONS, SettingsSection, SettingsSectionDefinition,
};
