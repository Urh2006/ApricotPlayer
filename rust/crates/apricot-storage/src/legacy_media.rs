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
    let item = MediaItem {
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
    };
    // A private IPTV channel saved before review 4 loses its login here,
    // and the next save writes the file without it (A1).
    Some(item.without_private_address().unwrap_or(item))
}

pub fn media_item_to_python_value(item: &MediaItem) -> Value {
    // A private IPTV channel is written as a reference to its source and
    // channel, without address, login or portal session (review 4, A1).
    let kept = item.without_private_address();
    let item = kept.as_ref().unwrap_or(item);
    let mut object: Map<String, Value> = item
        .metadata
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    // Stream headers carry the AudioVault session cookie; like Python's
    // `_audiovault_headers` they only live with the playing item.
    object.remove("http_headers");
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

/// Whether a saved array (or, with `nested`, the arrays under that key of
/// each element) still holds a private IPTV address or login.
pub(crate) fn file_holds_private_address(path: &std::path::Path, nested: Option<&str>) -> bool {
    let Ok(Value::Array(values)) = crate::json_file::read_json::<Value>(path) else {
        return false;
    };
    values.iter().any(|value| match nested {
        Some(key) => value
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(holds_private_address)),
        None => holds_private_address(value),
    })
}

/// A saved entry of a private IPTV channel that still holds its address,
/// login or portal session; the file is written again without them
/// (review 4, A1).
pub(crate) fn holds_private_address(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object
        .get("apricot_private_address")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return false;
    }
    let reference =
        text(object, "url").starts_with(&format!("{}:", apricot_core::SAVED_IPTV_SCHEME));
    !reference
        || [
            "stalker_token",
            "stalker_cookie",
            "stalker_command",
            "stalker_endpoint",
            "http_headers",
        ]
        .iter()
        .any(|key| object.contains_key(*key))
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
    // An IPTV film or episode saved by a 2.1 release candidate carries
    // AudioVault's kind; its IPTV source tells it apart (review 5, A3).
    let iptv = object.contains_key("iptv_source_id");
    if explicit.contains("audiovault") || (kind.starts_with("audiovault") && !iptv) {
        return MediaSource::Audiovault;
    }
    if explicit.contains("spotify")
        || kind.starts_with("spotify")
        || location.starts_with("https://open.spotify.com/")
    {
        return MediaSource::Spotify;
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
        "audiovault_movie" | "movie" | "iptv_movie" => MediaKind::Movie,
        "audiovault_show" | "audiovault_tv_show" | "tv_show" | "iptv_series" => MediaKind::TvShow,
        "audiovault_episode" | "audiovault_remote_episode" | "tv_episode" | "iptv_episode" => {
            MediaKind::TvEpisode
        }
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
    let iptv = item.metadata.contains_key("iptv_source_id");
    if let Some(kind) = item
        .metadata
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|kind| !kind.is_empty() && (!iptv || !kind.starts_with("audiovault")))
    {
        return kind.to_owned();
    }
    match item.kind {
        MediaKind::Movie if iptv => "iptv_movie",
        MediaKind::TvShow if iptv => "iptv_series",
        MediaKind::TvEpisode if iptv => "iptv_episode",
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

    /// Review 4, A1: a private IPTV channel kept in favorites, a playlist
    /// or the queue is written without its address, login or portal
    /// session, and an entry saved before is cleaned when it is read.
    #[test]
    fn private_iptv_channels_are_saved_as_a_reference() {
        let saved = json!({
            "id": "http://portal.example.test/live/USER/FAKE_SECRET/77.ts",
            "title": "News",
            "kind": "live_stream",
            "url": "http://portal.example.test/live/USER/FAKE_SECRET/77.ts",
            "apricot_private_address": true,
            "iptv_source_id": "src one",
            "iptv_channel_id": "77",
            "stalker_token": "FAKE_TOKEN",
            "stalker_cookie": "mac=FAKE_MAC",
            "stalker_command": "ffmpeg http://localhost/ch/77",
            "http_headers": {"Cookie": "mac=FAKE_MAC"}
        });
        assert!(super::holds_private_address(&saved));
        let item = media_item_from_python_value(&saved).expect("item");
        assert!(item.is_saved_iptv_reference());
        assert_eq!(item.id.0, "iptv:src one/77");
        assert_eq!(item.title, "News");
        let written = media_item_to_python_value(&item);
        let text = written.to_string();
        for secret in ["FAKE_SECRET", "FAKE_TOKEN", "FAKE_MAC", "USER", "localhost"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        assert_eq!(written["url"], json!("apricot-iptv://saved/src%20one/77"));
        assert_eq!(written["iptv_stream_extension"], json!("ts"));
        assert!(!super::holds_private_address(&written));
        let again = media_item_from_python_value(&written).expect("again");
        assert_eq!((&again.id, &again.url), (&item.id, &item.url));
        assert_eq!(again.iptv_ids(), Some(("src one".into(), "77".into())));
    }

    /// Review 5, A3: a saved IPTV film or episode is read back as IPTV,
    /// also when a release candidate saved it with the `AudioVault` kind; a
    /// real `AudioVault` entry stays `AudioVault`.
    #[test]
    fn iptv_films_and_episodes_are_not_audiovault() {
        for (kind, expected) in [
            (MediaKind::Movie, "iptv_movie"),
            (MediaKind::TvEpisode, "iptv_episode"),
        ] {
            let older = json!({"title": "Film", "kind": if kind == MediaKind::Movie { "audiovault_movie" } else { "audiovault_remote_episode" },
                "url": "apricot-iptv://saved/one/movie%3A5", "apricot_private_address": true,
                "iptv_source_id": "one", "iptv_channel_id": "movie:5"});
            let item = media_item_from_python_value(&older).expect("item");
            assert_eq!((item.source, item.kind), (MediaSource::Direct, kind));
            assert!(item.is_saved_iptv_reference());
            let written = media_item_to_python_value(&item);
            assert_eq!(written["kind"], json!(expected));
            let again = media_item_from_python_value(&written).expect("again");
            assert_eq!((again.source, again.kind), (MediaSource::Direct, kind));
        }
        let audiovault = json!({"title": "Film", "kind": "audiovault_movie", "url": "https://audiovault.net/movies/1"});
        let item = media_item_from_python_value(&audiovault).expect("item");
        assert_eq!(
            (item.source, item.kind),
            (MediaSource::Audiovault, MediaKind::Movie)
        );
    }

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
