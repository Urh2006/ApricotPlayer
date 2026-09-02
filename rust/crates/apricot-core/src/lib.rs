//! Platform-neutral product contract for `ApricotPlayer` 2.0.

pub mod action;
pub mod audio;
pub mod context_menu;
pub mod locale;
pub mod media;
pub mod menu;
pub mod navigation;
pub mod screen;
pub mod setting;

pub use action::{ActionDefinition, ActionId, ActionScope, RepeatPolicy};
pub use audio::{EqualizerBand, FactoryEqualizerPreset};
pub use context_menu::{CONTEXT_MENUS, ContextMenuDefinition};
pub use locale::{LanguageDefinition, TranslationCatalog};
pub use media::{MediaId, MediaItem, MediaKind, MediaSource};
pub use menu::{CUSTOMIZABLE_MAIN_MENU, MainMenuDefinition, PERMANENT_MAIN_MENU_IDS};
pub use navigation::{FocusId, NavigationStack, Route, RouteFrame};
pub use screen::{PrimaryControlRole, SCREENS, ScreenDefinition, ScreenKind};
pub use setting::SettingId;
