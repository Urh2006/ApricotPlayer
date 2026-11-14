//! 2.1 additions around `YouTube`: one download's own options, a link that
//! names both a video and its playlist, and search suggestions.

use std::path::PathBuf;

use apricot_core::{MediaItem, MediaKind};
use apricot_storage::SettingsDocument;
use serde::{Deserialize, Serialize};
use url::Url;

/// Item metadata key of [`ItemDownloadOptions`].
pub const DOWNLOAD_OPTIONS_KEY: &str = "apricot_download_options";

/// The audio quality values of Settings, best first.
pub const AUDIO_QUALITY_VALUES: [&str; 18] = [
    "0", "320", "256", "192", "160", "128", "96", "64", "1", "2", "3", "4", "5", "6", "7", "8",
    "9", "10",
];

/// How Settings names an audio quality value.
#[must_use]
pub fn audio_quality_label(value: &str) -> String {
    if value == "0" {
        "Best variable quality (VBR 0)".to_owned()
    } else if value.parse::<i64>().is_ok_and(|number| number <= 10) {
        format!("Variable quality (VBR {value})")
    } else {
        format!("{value} kbps")
    }
}

/// The formats and folder chosen for one download; empty values keep the
/// settings.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ItemDownloadOptions {
    pub audio_format: String,
    pub audio_quality: String,
    pub video_format: String,
    pub max_video_height: Option<i64>,
    /// The exact folder the files go to.
    pub folder: String,
}

impl ItemDownloadOptions {
    #[must_use]
    pub fn from_item(item: &MediaItem) -> Option<Self> {
        item.metadata
            .get(DOWNLOAD_OPTIONS_KEY)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    pub fn store(&self, item: &mut MediaItem) {
        if let Ok(value) = serde_json::to_value(self) {
            item.metadata.insert(DOWNLOAD_OPTIONS_KEY.to_owned(), value);
        }
    }

    /// The settings this download uses.
    #[must_use]
    pub fn apply(&self, settings: &SettingsDocument) -> SettingsDocument {
        let mut settings = settings.clone();
        let set = |target: &mut String, value: &str| {
            if !value.trim().is_empty() {
                value.trim().clone_into(target);
            }
        };
        set(&mut settings.audio_format, &self.audio_format);
        set(&mut settings.audio_quality, &self.audio_quality);
        set(&mut settings.video_format, &self.video_format);
        if let Some(height) = self.max_video_height {
            settings.max_video_height = height.max(0);
        }
        // A folder chosen for this download is not asked for again.
        if self.folder().is_some() {
            settings.ask_download_location_each_time = false;
        }
        settings
    }

    #[must_use]
    pub fn folder(&self) -> Option<PathBuf> {
        Some(PathBuf::from(self.folder.trim())).filter(|folder| folder.is_absolute())
    }
}

/// The video alone and the whole playlist of a `YouTube` link that names
/// both, such as `watch?v=…&list=…`.
#[must_use]
pub fn video_and_playlist(link: &str) -> Option<(MediaItem, MediaItem)> {
    let url = Url::parse(link.trim()).ok()?;
    let host = url
        .host_str()?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    let query = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
            .filter(|value| !value.trim().is_empty())
    };
    let video = match host.as_str() {
        "youtube.com" | "m.youtube.com" | "music.youtube.com" if url.path() == "/watch" => {
            query("v")?
        }
        "youtu.be" => url.path().trim_start_matches('/').to_owned(),
        _ => return None,
    };
    let list = query("list")?;
    if video.is_empty() {
        return None;
    }
    let video_item =
        MediaItem::from_direct_link(&format!("https://www.youtube.com/watch?v={video}"))?;
    let mut playlist_item =
        MediaItem::from_direct_link(&format!("https://www.youtube.com/playlist?list={list}"))?;
    playlist_item.kind = MediaKind::Playlist;
    Some((video_item, playlist_item))
}

/// The suggestions of the `YouTube` search suggestion service: `[query,
/// [suggestion, …]]`.
#[must_use]
pub fn parse_suggestions(body: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get(1).and_then(serde_json::Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::trim).map(str::to_owned))
        .filter(|value| !value.is_empty())
        .take(10)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watch_link_with_a_list_names_both() {
        let (video, playlist) =
            video_and_playlist("https://www.youtube.com/watch?v=abc123&list=PL42&index=3")
                .expect("both");
        assert_eq!(
            video.url.expect("video").as_str(),
            "https://www.youtube.com/watch?v=abc123"
        );
        assert_eq!(
            playlist.url.expect("playlist").as_str(),
            "https://www.youtube.com/playlist?list=PL42"
        );
        assert_eq!(playlist.kind, MediaKind::Playlist);
        assert!(video_and_playlist("https://youtu.be/abc123?list=PL42").is_some());
        assert!(video_and_playlist("https://www.youtube.com/watch?v=abc123").is_none());
        assert!(video_and_playlist("https://example.com/watch?v=a&list=b").is_none());
    }

    #[test]
    fn item_options_override_only_what_was_chosen() {
        let settings = SettingsDocument::default();
        let options = ItemDownloadOptions {
            audio_format: "flac".into(),
            max_video_height: Some(720),
            ..ItemDownloadOptions::default()
        };
        let applied = options.apply(&settings);
        assert_eq!(applied.audio_format, "flac");
        assert_eq!(applied.max_video_height, 720);
        assert_eq!(applied.audio_quality, settings.audio_quality);
        let mut item = MediaItem::from_direct_link("https://example.test/a").expect("item");
        options.store(&mut item);
        assert_eq!(ItemDownloadOptions::from_item(&item), Some(options));
    }

    #[test]
    fn suggestions_are_read_from_the_second_entry() {
        assert_eq!(
            parse_suggestions(r#"["cats",["cats song"," cat videos ",""],[]]"#),
            ["cats song", "cat videos"]
        );
        assert!(parse_suggestions("not json").is_empty());
    }
}
