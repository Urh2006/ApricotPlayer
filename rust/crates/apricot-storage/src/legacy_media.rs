//! Loss-aware conversion of Python media dictionaries into typed Rust items.

use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
use serde_json::{Map, Value};

pub fn media_item_from_python_value(value: &Value) -> Option<MediaItem> {
    let object = value.as_object()?;
    let kind_text = text(object, "kind").to_ascii_lowercase();
    let location = first_text(object, &["url", "webpage_url", "local_path", "path"]);
    let local_path = local_path(object, &kind_text, &location);
    let source = media_source(object, &kind_text, local_path.as_deref(), &location);
    let url = if local_path.is_some() {
        None
    } else {
        location.parse().ok()
    };
    if url.is_none()
        && local_path.is_none()
        && !matches!(kind_text.as_str(), "rss_item" | "podcast_episode")
    {
        return None;
    }
    let kind = media_kind(&kind_text, local_path.as_deref(), object);
    let id = first_text(object, &["id", "video_id", "episode_id"]);
    let id = if id.is_empty() { location.clone() } else { id };
    let title = {
        let title = text(object, "title");
        if title.is_empty() {
            location.clone()
        } else {
            title
        }
    };
    let duration_seconds = object
        .get("duration_seconds")
        .or_else(|| object.get("duration"))
        .and_then(Value::as_f64)
        .filter(|duration| duration.is_finite() && *duration >= 0.0);
    let metadata = object
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Some(MediaItem {
        id: MediaId(id),
        source,
        kind,
        title,
        url,
        stream_url: None,
        external_audio_url: None,
        local_path,
        channel: text(object, "channel"),
        duration_seconds,
        metadata,
    })
}

pub fn media_item_to_python_value(item: &MediaItem) -> Value {
    let mut object: Map<String, Value> = item
        .metadata
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    object.insert("title".to_owned(), Value::String(item.title.clone()));
    object.insert("channel".to_owned(), Value::String(item.channel.clone()));
    object.insert("kind".to_owned(), Value::String(python_kind(item)));
    if let Some(path) = &item.local_path {
        object.insert("url".to_owned(), Value::String(path.clone()));
        object.insert("local_path".to_owned(), Value::String(path.clone()));
    } else if let Some(url) = &item.url {
        object.insert("url".to_owned(), Value::String(url.to_string()));
    }
    if let Some(duration) = item.duration_seconds {
        object.insert("duration_seconds".to_owned(), Value::from(duration));
    }
    Value::Object(object)
}

fn text(object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn first_text(object: &Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .map(|key| text(object, key))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn local_path(object: &Map<String, Value>, kind: &str, location: &str) -> Option<String> {
    let explicit = first_text(object, &["local_path", "path"]);
    if !explicit.is_empty() {
        return Some(explicit);
    }
    (kind == "local_file" || looks_like_windows_path(location)).then(|| location.to_owned())
}

fn looks_like_windows_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    (bytes.len() >= 3 && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/'))
        || value.starts_with(r"\\")
}

fn media_source(
    object: &Map<String, Value>,
    kind: &str,
    local_path: Option<&str>,
    location: &str,
) -> MediaSource {
    let explicit = first_text(object, &["source", "provider"]).to_ascii_lowercase();
    if explicit.contains("soundcloud") {
        return MediaSource::Soundcloud;
    }
    if explicit.contains("audiovault") || kind.starts_with("audiovault") {
        return MediaSource::Audiovault;
    }
    if local_path.is_some() || kind == "local_file" {
        return MediaSource::Local;
    }
    if explicit.contains("podcast") || kind.starts_with("rss") {
        return MediaSource::Podcast;
    }
    if explicit.contains("youtube")
        || location.contains("youtube.com/")
        || location.contains("youtu.be/")
    {
        return MediaSource::Youtube;
    }
    MediaSource::Direct
}

fn media_kind(kind: &str, local_path: Option<&str>, object: &Map<String, Value>) -> MediaKind {
    match kind {
        "audio" => MediaKind::Audio,
        "video" | "short" => MediaKind::Video,
        "live" | "live_stream" | "livestream" => MediaKind::LiveStream,
        "playlist" => MediaKind::Playlist,
        "channel" | "user" => MediaKind::Channel,
        "rss_feed" | "podcast_feed" => MediaKind::PodcastFeed,
        "rss_item" | "podcast_episode" => MediaKind::PodcastEpisode,
        "audiovault_movie" | "movie" => MediaKind::Movie,
        "audiovault_show" | "audiovault_tv_show" | "tv_show" => MediaKind::TvShow,
        "audiovault_episode" | "audiovault_remote_episode" | "tv_episode" => MediaKind::TvEpisode,
        "local_file" => local_media_kind(local_path.unwrap_or_default()),
        _ if object.get("is_live").and_then(Value::as_bool) == Some(true) => MediaKind::LiveStream,
        _ if local_path.is_some() => local_media_kind(local_path.unwrap_or_default()),
        _ => MediaKind::Unknown,
    }
}

fn local_media_kind(path: &str) -> MediaKind {
    let extension = path
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(
        extension.as_str(),
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "wmv"
    ) {
        MediaKind::Video
    } else {
        MediaKind::Audio
    }
}

fn python_kind(item: &MediaItem) -> String {
    if let Some(kind) = item
        .metadata
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
    {
        return kind.to_owned();
    }
    match item.kind {
        MediaKind::Audio | MediaKind::Video if item.source == MediaSource::Local => "local_file",
        MediaKind::Audio => "audio",
        MediaKind::Video => "video",
        MediaKind::LiveStream => "live_stream",
        MediaKind::Playlist => "playlist",
        MediaKind::Channel => "channel",
        MediaKind::PodcastFeed => "rss_feed",
        MediaKind::PodcastEpisode => "rss_item",
        MediaKind::Movie => "audiovault_movie",
        MediaKind::TvShow => "audiovault_tv_show",
        MediaKind::TvEpisode => "audiovault_remote_episode",
        MediaKind::Unknown => "unknown",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{media_item_from_python_value, media_item_to_python_value};
    use apricot_core::{MediaKind, MediaSource};

    #[test]
    fn youtube_dictionary_round_trips_unknown_metadata() {
        let source = json!({
            "id": "abc",
            "title": "Video",
            "channel": "Channel",
            "kind": "video",
            "url": "https://www.youtube.com/watch?v=abc",
            "duration_seconds": 42.5,
            "future_python_field": {"kept": true}
        });
        let item = media_item_from_python_value(&source).expect("media item");
        assert_eq!(item.source, MediaSource::Youtube);
        assert_eq!(item.kind, MediaKind::Video);
        let restored = media_item_to_python_value(&item);
        assert_eq!(restored["future_python_field"], json!({"kept": true}));
        assert_eq!(restored["url"], source["url"]);
    }

    #[test]
    fn python_local_url_becomes_a_path_not_an_invalid_remote_url() {
        let source = json!({
            "title": "Track",
            "kind": "local_file",
            "url": "C:\\Music\\Track.mp3"
        });
        let item = media_item_from_python_value(&source).expect("local item");
        assert_eq!(item.source, MediaSource::Local);
        assert_eq!(item.kind, MediaKind::Audio);
        assert_eq!(item.local_path.as_deref(), Some(r"C:\Music\Track.mp3"));
        assert!(item.url.is_none());
        assert_eq!(media_item_to_python_value(&item)["url"], source["url"]);
    }

    #[test]
    fn audiovault_python_kind_aliases_round_trip_without_becoming_local() {
        for source in [
            json!({
                "title": "Series",
                "kind": "audiovault_show",
                "webpage_url": "https://audiovault.example/show/series"
            }),
            json!({
                "title": "Remote episode",
                "kind": "audiovault_remote_episode",
                "url": "C:\\Cache\\episode.mp3",
                "local_path": "C:\\Cache\\episode.mp3",
                "archive_url": "https://audiovault.example/archive.zip"
            }),
            json!({
                "title": "Cached episode",
                "kind": "audiovault_episode",
                "url": "C:\\Cache\\cached.mp3",
                "local_path": "C:\\Cache\\cached.mp3"
            }),
        ] {
            let item = media_item_from_python_value(&source).expect("AudioVault item");
            assert_eq!(item.source, MediaSource::Audiovault);
            assert_eq!(media_item_to_python_value(&item)["kind"], source["kind"]);
        }
    }
}
