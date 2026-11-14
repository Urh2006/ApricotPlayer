//! The 2.1 integrations: the system media controls, on unless turned off,
//! and Discord Rich Presence, off until the user turns it on, with a choice
//! for every shared detail.

use apricot_core::MediaItem;
use apricot_storage::SettingsDocument;
use serde::{Deserialize, Serialize};

/// What Discord shows; every field already follows the user's choices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresenceActivity {
    pub title: String,
    pub artist: String,
    /// An https address of the artwork.
    pub artwork: String,
    /// Unix milliseconds of the start and the end.
    pub started_ms: Option<i64>,
    pub ends_ms: Option<i64>,
}

/// Settings key of [`IntegrationSettings`].
pub const INTEGRATIONS_KEY: &str = "integrations_v1";

/// The Discord application that names Apricot in a profile. Empty
/// until one is registered for Apricot; Discord stays unavailable
/// without it.
pub const DISCORD_APPLICATION_ID: &str = "";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)] // One switch for every shared detail.
pub struct IntegrationSettings {
    pub media_controls: bool,
    pub discord: bool,
    pub discord_title: bool,
    pub discord_artist: bool,
    pub discord_artwork: bool,
    pub discord_timing: bool,
    pub discord_when_paused: bool,
}

impl Default for IntegrationSettings {
    fn default() -> Self {
        Self {
            media_controls: true,
            discord: false,
            discord_title: true,
            discord_artist: true,
            discord_artwork: true,
            discord_timing: true,
            discord_when_paused: false,
        }
    }
}

impl IntegrationSettings {
    #[must_use]
    pub fn from_settings(settings: &SettingsDocument) -> Self {
        settings
            .preserved
            .get(INTEGRATIONS_KEY)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }

    /// Discord can be used: it is turned on and Apricot has its
    /// Discord application.
    #[must_use]
    pub fn discord_active(&self) -> bool {
        self.discord && !DISCORD_APPLICATION_ID.is_empty()
    }

    /// What Discord shows for the playing item; `None` shows nothing.
    #[must_use]
    pub fn presence(
        &self,
        item: &MediaItem,
        position: f64,
        duration: Option<f64>,
        paused: bool,
        now_ms: i64,
    ) -> Option<PresenceActivity> {
        if !self.discord_active() || (paused && !self.discord_when_paused) {
            return None;
        }
        let title = if self.discord_title && !item.title.trim().is_empty() {
            item.title.clone()
        } else {
            "ApricotPlayer".to_owned()
        };
        let artist = if self.discord_artist {
            item.channel.clone()
        } else {
            String::new()
        };
        let artwork = if self.discord_artwork {
            item.metadata
                .get("thumbnail")
                .and_then(serde_json::Value::as_str)
                .filter(|url| url.starts_with("https://"))
                .unwrap_or_default()
                .to_owned()
        } else {
            String::new()
        };
        let (started_ms, ends_ms) = if self.discord_timing && !paused && position.is_finite() {
            #[allow(clippy::cast_possible_truncation)] // Milliseconds of a play.
            let started = now_ms - (position.max(0.0) * 1000.0) as i64;
            #[allow(clippy::cast_possible_truncation)]
            let ends = duration
                .filter(|duration| duration.is_finite() && *duration > 0.0)
                .map(|duration| started + (duration * 1000.0) as i64);
            (Some(started), ends)
        } else {
            (None, None)
        };
        Some(PresenceActivity {
            title,
            artist,
            artwork,
            started_ms,
            ends_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_controls_are_on_and_discord_is_off_by_default() {
        let settings = IntegrationSettings::default();
        assert!(settings.media_controls);
        assert!(!settings.discord);
        let item = MediaItem::from_direct_link("https://example.test/a").expect("item");
        assert_eq!(
            settings.presence(&item, 10.0, Some(60.0), false, 100_000),
            None
        );
    }

    #[test]
    fn presence_shares_only_the_chosen_details() {
        let settings = IntegrationSettings {
            discord: true,
            discord_artist: false,
            ..IntegrationSettings::default()
        };
        let mut item = MediaItem::from_direct_link("https://example.test/a").expect("item");
        item.title = "Song".into();
        item.channel = "Band".into();
        let shown = PresenceActivity {
            title: "Song".into(),
            artist: String::new(),
            artwork: String::new(),
            started_ms: Some(90_000),
            ends_ms: Some(150_000),
        };
        // Without the Discord application nothing is shown at all.
        let expected = (!DISCORD_APPLICATION_ID.is_empty()).then_some(shown);
        assert_eq!(
            settings.presence(&item, 10.0, Some(60.0), false, 100_000),
            expected
        );
        assert_eq!(
            settings.presence(&item, 10.0, Some(60.0), true, 100_000),
            None
        );
    }
}
