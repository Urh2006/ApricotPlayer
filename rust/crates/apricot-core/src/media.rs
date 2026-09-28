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
    pub fn from_direct_link(value: &str) -> Option<Self> {
        let url = Url::parse(value.trim()).ok()?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return None;
        }
        let durable_url = url.to_string();
        Some(Self {
            id: MediaId(durable_url.clone()),
            source: MediaSource::Direct,
            kind: MediaKind::Unknown,
            title: durable_url,
            url: Some(url),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        })
    }

    pub fn is_local_media(&self) -> bool {
        self.source == MediaSource::Local
            || self
                .local_path
                .as_ref()
                .is_some_and(|path| !path.trim().is_empty())
    }

    /// Returns the durable location users expect from Copy link/Copy path.
    /// Ephemeral component stream URLs are intentionally excluded.
    pub fn copy_location(&self) -> Option<String> {
        if self.is_local_media() {
            return self
                .local_path
                .as_deref()
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_owned);
        }
        self.url.as_ref().map(ToString::to_string)
    }

    /// Python `extract_youtube_video_id`: the video id of a `YouTube` item.
    pub fn youtube_video_id(&self) -> Option<String> {
        if self.source == MediaSource::Youtube && valid_youtube_video_id(&self.id.0) {
            return Some(self.id.0.clone());
        }
        self.url.as_ref().and_then(youtube_video_id_from_url)
    }

    /// Builds a canonical `YouTube` watch URL at the current whole second.
    /// Existing non-time query parameters, such as a playlist identity, are
    /// preserved when the durable source URL is a `YouTube` URL.
    pub fn youtube_url_at_timestamp(&self, seconds: f64) -> Option<Url> {
        let source_url = self
            .url
            .as_ref()
            .filter(|url| youtube_video_id_from_url(url).is_some());
        let video_id = if self.source == MediaSource::Youtube && valid_youtube_video_id(&self.id.0)
        {
            self.id.0.clone()
        } else {
            youtube_video_id_from_url(source_url?)?
        };
        let mut result = Url::parse("https://www.youtube.com/watch").ok()?;
        let timestamp = std::time::Duration::try_from_secs_f64(seconds.max(0.0))
            .map_or(0, |duration| duration.as_secs());
        {
            let mut query = result.query_pairs_mut();
            query.append_pair("v", &video_id);
            if let Some(source_url) = source_url {
                for (key, value) in source_url.query_pairs() {
                    if !matches!(
                        key.to_ascii_lowercase().as_str(),
                        "v" | "t" | "start" | "time_continue"
                    ) {
                        query.append_pair(&key, &value);
                    }
                }
            }
            query.append_pair("t", &format!("{timestamp}s"));
        }
        Some(result)
    }

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

    /// Merges source metadata without changing the durable identity or any
    /// ephemeral playback URL.
    pub fn merge_descriptive_metadata(&mut self, hydrated: &Self) -> bool {
        if self.stable_identity().is_none() || self.stable_identity() != hydrated.stable_identity()
        {
            return false;
        }
        let before = self.clone();
        if !hydrated.title.trim().is_empty() {
            self.title.clone_from(&hydrated.title);
        }
        if !hydrated.channel.trim().is_empty() {
            self.channel.clone_from(&hydrated.channel);
        }
        if hydrated.duration_seconds.is_some() {
            self.duration_seconds = hydrated.duration_seconds;
        }
        if matches!(hydrated.kind, MediaKind::Video | MediaKind::LiveStream) {
            self.kind = hydrated.kind;
        }
        for (key, value) in &hydrated.metadata {
            let meaningful =
                !value.is_null() && value.as_str().is_none_or(|value| !value.trim().is_empty());
            if meaningful {
                self.metadata.insert(key.clone(), value.clone());
            }
        }
        *self != before
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

fn valid_youtube_video_id(value: &str) -> bool {
    value.len() >= 8
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn youtube_video_id_from_url(url: &Url) -> Option<String> {
    let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    if host == "youtu.be" || host.ends_with(".youtu.be") {
        return url
            .path_segments()?
            .find(|segment| !segment.is_empty())
            .filter(|id| valid_youtube_video_id(id))
            .map(str::to_owned);
    }
    if host != "youtube.com" && !host.ends_with(".youtube.com") {
        return None;
    }
    if let Some((_, id)) = url
        .query_pairs()
        .find(|(key, _)| key.eq_ignore_ascii_case("v"))
    {
        return valid_youtube_video_id(&id).then(|| id.into_owned());
    }
    let mut segments = url.path_segments()?;
    let kind = segments.next()?;
    if !matches!(kind, "shorts" | "embed" | "live") {
        return None;
    }
    segments
        .next()
        .filter(|id| valid_youtube_video_id(id))
        .map(str::to_owned)
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
    fn descriptive_metadata_merge_preserves_durable_and_ephemeral_locations() {
        let mut original = item(MediaKind::Video);
        let original_url = original.url.clone();
        original.stream_url = Some("https://media.example/old".parse().expect("stream"));
        let mut hydrated = original.clone();
        hydrated.title = "Hydrated title".to_owned();
        hydrated.channel = "Hydrated channel".to_owned();
        hydrated.duration_seconds = Some(42.0);
        hydrated.url = Some("https://youtube.com/watch?v=replaced".parse().expect("URL"));
        hydrated.stream_url = Some("https://media.example/new".parse().expect("stream"));
        hydrated.metadata.insert("view_count".to_owned(), 12.into());

        assert!(original.merge_descriptive_metadata(&hydrated));
        assert_eq!(original.title, "Hydrated title");
        assert_eq!(original.channel, "Hydrated channel");
        assert_eq!(original.duration_seconds, Some(42.0));
        assert_eq!(original.metadata["view_count"], 12);
        assert_eq!(original.url, original_url);
        assert_eq!(
            original.stream_url.as_ref().map(url::Url::as_str),
            Some("https://media.example/old")
        );
    }

    #[test]
    fn collections_are_not_playable_sequence_items() {
        assert!(item(MediaKind::Video).is_playable());
        assert!(!item(MediaKind::Playlist).is_playable());
        assert!(!item(MediaKind::Channel).is_playable());
        assert!(!item(MediaKind::PodcastFeed).is_playable());
        assert!(!item(MediaKind::TvShow).is_playable());
    }

    #[test]
    fn copy_location_never_exposes_an_ephemeral_stream() {
        let mut media = item(MediaKind::Video);
        media.stream_url = Some("https://cdn.example/temporary".parse().expect("stream URL"));
        assert_eq!(
            media.copy_location().as_deref(),
            Some("https://www.youtube.com/watch?v=same")
        );

        media.source = MediaSource::Local;
        media.url = None;
        media.local_path = Some(r"C:\Music\Track.mp3".to_owned());
        assert_eq!(
            media.copy_location().as_deref(),
            Some(r"C:\Music\Track.mp3")
        );
    }

    #[test]
    fn timestamp_url_preserves_collection_context_and_replaces_old_time() {
        let mut media = item(MediaKind::Video);
        media.id = MediaId("dQw4w9WgXcQ".to_owned());
        media.url = Some(
            "https://youtu.be/dQw4w9WgXcQ?list=PL123&t=3&index=4"
                .parse()
                .expect("YouTube URL"),
        );
        assert_eq!(
            media
                .youtube_url_at_timestamp(65.9)
                .map(|url| url.to_string())
                .as_deref(),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123&index=4&t=65s")
        );
    }

    #[test]
    fn timestamp_url_supports_direct_youtube_links_but_rejects_lookalike_hosts() {
        let mut media = item(MediaKind::Video);
        media.source = MediaSource::Direct;
        media.id = MediaId(String::new());
        media.url = Some(
            "https://www.youtube.com/shorts/dQw4w9WgXcQ"
                .parse()
                .expect("YouTube URL"),
        );
        assert!(media.youtube_url_at_timestamp(2.0).is_some());

        media.url = Some(
            "https://notyoutube.com/watch?v=dQw4w9WgXcQ"
                .parse()
                .expect("lookalike URL"),
        );
        assert!(media.youtube_url_at_timestamp(2.0).is_none());
    }

    #[test]
    fn direct_links_accept_only_absolute_http_media_locations() {
        let item = MediaItem::from_direct_link(" https://media.example/track.mp3 ")
            .expect("valid direct link");
        assert_eq!(item.source, MediaSource::Direct);
        assert_eq!(
            item.copy_location().as_deref(),
            Some("https://media.example/track.mp3")
        );
        assert!(MediaItem::from_direct_link("file:///C:/private.mp3").is_none());
        assert!(MediaItem::from_direct_link("javascript:alert(1)").is_none());
        assert!(MediaItem::from_direct_link("media.example/track.mp3").is_none());
    }
}
