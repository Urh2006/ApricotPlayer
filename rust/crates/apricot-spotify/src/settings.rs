//! Spotify settings of this installation (`docs/SPOTIFY_PLAN.md` 9.2):
//! streaming quality, volume normalisation and autoplay, in
//! `spotify/settings.json` of the Apricot data folder. They apply when the
//! session connects; changing them reconnects the active account.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// 96 kbps.
    Normal,
    /// 160 kbps.
    High,
    /// 320 kbps (Premium).
    VeryHigh,
}

impl Quality {
    pub const ALL: [Self; 3] = [Self::Normal, Self::High, Self::VeryHigh];

    pub const fn kbps(self) -> u32 {
        match self {
            Self::Normal => 96,
            Self::High => 160,
            Self::VeryHigh => 320,
        }
    }
}

/// What happens when an album or playlist ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Autoplay {
    /// The account's own Spotify setting.
    Account,
    On,
    Off,
}

impl Autoplay {
    pub const ALL: [Self; 3] = [Self::Account, Self::On, Self::Off];

    pub const fn session_value(self) -> Option<bool> {
        match self {
            Self::Account => None,
            Self::On => Some(true),
            Self::Off => Some(false),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpotifySettings {
    pub quality: Quality,
    pub normalisation: bool,
    pub autoplay: Autoplay,
}

impl Default for SpotifySettings {
    fn default() -> Self {
        Self {
            quality: Quality::VeryHigh,
            normalisation: false,
            autoplay: Autoplay::Account,
        }
    }
}

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// Where the settings live; set once when the Spotify service starts.
pub fn set_folder(app_data: &Path) {
    let _ = FILE.set(app_data.join("spotify").join("settings.json"));
}

/// The saved settings, or the defaults.
pub fn load() -> SpotifySettings {
    FILE.get()
        .and_then(|file| std::fs::read(file).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Saves the settings.
///
/// # Errors
///
/// Returns the reason when the file cannot be written.
pub fn save(settings: &SpotifySettings) -> Result<(), String> {
    let file = FILE.get().ok_or("Spotify is not started")?;
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
    std::fs::write(file, bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_partial_files_use_the_defaults() {
        let partial: SpotifySettings = serde_json::from_str(r#"{"quality":"high"}"#).unwrap();
        assert_eq!(partial.quality, Quality::High);
        assert_eq!(partial.autoplay, Autoplay::Account);
        assert!(!partial.normalisation);
        assert_eq!(Quality::VeryHigh.kbps(), 320);
        assert_eq!(Autoplay::Off.session_value(), Some(false));
    }
}
