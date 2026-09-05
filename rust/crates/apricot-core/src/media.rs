//! Shared media identities. Source adapters enrich these values without changing
//! list ordering or selection identity.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MediaId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSource {
    Youtube,
    Soundcloud,
    Direct,
    Local,
    Podcast,
    Audiovault,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Audio,
    Video,
    LiveStream,
    Playlist,
    Channel,
    PodcastFeed,
    PodcastEpisode,
    Movie,
    TvShow,
    TvEpisode,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MediaItem {
    pub id: MediaId,
    pub source: MediaSource,
    pub kind: MediaKind,
    pub title: String,
    #[serde(default)]
    pub url: Option<Url>,
    /// Ephemeral resolved playback URL. Durable features must keep using
    /// `url`, because component-provided stream URLs can expire.
    #[serde(default)]
    pub stream_url: Option<Url>,
    /// Optional audio rendition paired with a video-only `stream_url`.
    #[serde(default)]
    pub external_audio_url: Option<Url>,
    #[serde(default)]
    pub local_path: Option<String>,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl MediaItem {
    pub fn stable_identity(&self) -> Option<String> {
        let source = match self.source {
            MediaSource::Youtube => "youtube",
            MediaSource::Soundcloud => "soundcloud",
            MediaSource::Direct => "direct",
            MediaSource::Local => "local",
            MediaSource::Podcast => "podcast",
            MediaSource::Audiovault => "audiovault",
        };
        if !self.id.0.trim().is_empty() {
            return Some(format!("{source}:id:{}", self.id.0));
        }
        if let Some(url) = &self.url {
            return Some(format!("{source}:url:{url}"));
        }
        self.local_path
            .as_ref()
            .filter(|path| !path.trim().is_empty())
            .map(|path| format!("{source}:path:{path}"))
    }

    pub fn is_playable(&self) -> bool {
        !matches!(
            self.kind,
            MediaKind::Playlist | MediaKind::Channel | MediaKind::PodcastFeed | MediaKind::TvShow
        ) && (self.url.is_some()
            || self
                .local_path
                .as_ref()
                .is_some_and(|path| !path.trim().is_empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaId, MediaItem, MediaKind, MediaSource};
    use std::collections::BTreeMap;

    fn item(kind: MediaKind) -> MediaItem {
        MediaItem {
            id: MediaId("same".to_owned()),
            source: MediaSource::Youtube,
            kind,
            title: "Item".to_owned(),
            url: Some("https://www.youtube.com/watch?v=same".parse().expect("URL")),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn stable_identity_ignores_ephemeral_stream_resolution() {
        let mut media = item(MediaKind::Video);
        let before = media.stable_identity();
        media.stream_url = Some("https://cdn.example/video".parse().expect("stream URL"));
        media.external_audio_url = Some("https://cdn.example/audio".parse().expect("audio URL"));
        assert_eq!(media.stable_identity(), before);
    }

    #[test]
    fn collections_are_not_playable_sequence_items() {
        assert!(item(MediaKind::Video).is_playable());
        assert!(!item(MediaKind::Playlist).is_playable());
        assert!(!item(MediaKind::Channel).is_playable());
        assert!(!item(MediaKind::PodcastFeed).is_playable());
        assert!(!item(MediaKind::TvShow).is_playable());
    }
}
