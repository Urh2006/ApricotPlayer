//! Platform-neutral product contract for `ApricotPlayer` 2.0.

pub mod action;
pub mod audio;
pub mod locale;
pub mod media;
pub mod menu;
pub mod navigation;
pub mod setting;

pub use action::{ActionDefinition, ActionId, ActionScope, RepeatPolicy};
pub use audio::{EqualizerBand, FactoryEqualizerPreset};
pub use locale::LanguageDefinition;
pub use media::{MediaId, MediaItem, MediaKind, MediaSource};
pub use menu::MainMenuDefinition;
pub use navigation::{FocusId, NavigationStack, Route, RouteFrame};
pub use setting::SettingId;
