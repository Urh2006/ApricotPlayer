//! Source-neutral player information projected from durable metadata and libmpv.

use std::path::Path;

use apricot_core::{MediaItem, MediaKind, TranslationCatalog};
use apricot_playback::PlaybackMediaInfo;
use chrono::NaiveDate;

pub fn format_status(
    catalog: &TranslationCatalog,
    item: &MediaItem,
    playback: &PlaybackMediaInfo,
) -> String {
    let metadata = &item.metadata;
    let extension = metadata_string(metadata, "ext").or_else(|| {
        item.local_path
            .as_deref()
            .and_then(|path| Path::new(path).extension())
            .map(|extension| extension.to_string_lossy().into_owned())
    });
    let container = extension
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_ascii_uppercase())
        .or_else(|| playback.container.as_deref().and_then(format_container))
        .unwrap_or_else(|| "MP3".to_owned());
    let audio_codec = metadata_string(metadata, "acodec")
        .or_else(|| playback.audio_codec.clone())
        .and_then(|codec| format_audio_codec(&codec));
    let video_codec = metadata_string(metadata, "vcodec")
        .or_else(|| playback.video_codec.clone())
        .filter(|codec| !codec.eq_ignore_ascii_case("none"));
    let height = metadata_u32(metadata, "height").or(playback.height);
    let bitrate_kbps = metadata_f64(metadata, "abr")
        .or_else(|| metadata_f64(metadata, "audio_bitrate"))
        .filter(|value| value.is_finite() && *value > 0.0)
        .or_else(|| {
            playback
                .audio_bitrate_bits_per_second
                .map(|value| value / 1_000.0)
                .filter(|value| value.is_finite() && *value > 0.0)
        })
        .map(f64::round);
    let bitrate = bitrate_kbps.map(|value| format!("{value:.0} kbps"));

    if video_codec.is_some()
        && let Some(height) = height
    {
        let resolution = format!("{height}p");
        return match (audio_codec.as_deref(), bitrate.as_deref()) {
            (Some(audio), Some(bitrate)) => catalog
                .text("format_status_video_with_audio_bitrate")
                .replace("{container}", &container)
                .replace("{resolution}", &resolution)
                .replace("{audio}", audio)
                .replace("{bitrate}", bitrate),
            (Some(audio), None) => catalog
                .text("format_status_video_with_audio")
                .replace("{container}", &container)
                .replace("{resolution}", &resolution)
                .replace("{audio}", audio),
            (None, _) => catalog
                .text("format_status_video_only")
                .replace("{container}", &container)
                .replace("{resolution}", &resolution),
        };
    }
    match (audio_codec.as_deref(), bitrate.as_deref()) {
        (Some(audio), Some(bitrate)) => catalog
            .text("format_status_audio_detailed")
            .replace("{container}", &container)
            .replace("{audio}", audio)
            .replace("{bitrate}", bitrate),
        (Some(audio), None) => catalog
            .text("format_status_audio")
            .replace("{container}", &container)
            .replace("{audio}", audio),
        (None, Some(bitrate)) => catalog
            .text("format_status_audio_bitrate")
            .replace("{container}", &container)
            .replace("{bitrate}", bitrate),
        (None, None) => catalog
            .text("format_status_simple")
            .replace("{container}", &container),
    }
}

pub fn details_text(
    catalog: &TranslationCatalog,
    item: &MediaItem,
    speed: f64,
    pitch: f64,
) -> String {
    let location = item.copy_location().unwrap_or_default();
    let views = display_count(&item.metadata, "views")
        .or_else(|| display_count(&item.metadata, "view_count"))
        .unwrap_or_default();
    let uploaded = display_upload_age(catalog, item);
    let duration = item
        .duration_seconds
        .map(format_duration)
        .unwrap_or_default();
    let description = metadata_string(&item.metadata, "description").unwrap_or_default();
    [
        item.title.clone(),
        labeled(catalog.text("channel"), &item.channel),
        labeled(catalog.text("url"), &location),
        labeled(catalog.text("views"), &views),
        uploaded,
        labeled(catalog.text("type"), item_type_label(catalog, item.kind)),
        labeled("Duration", &duration),
        labeled("Playback speed", &format!("{speed:.2}x")),
        labeled(catalog.text("pitch_label"), &format!("{pitch:.2}x")),
        format!("{}:", catalog.text("description")),
        description,
    ]
    .join("\r\n")
}

fn labeled(label: &str, value: &str) -> String {
    format!("{label}: {value}")
}

fn item_type_label(catalog: &TranslationCatalog, kind: MediaKind) -> &str {
    catalog.text(match kind {
        MediaKind::Audio => "download_audio_mode",
        MediaKind::Video => "video",
        MediaKind::LiveStream => "live_stream",
        MediaKind::Playlist => "playlist",
        MediaKind::Channel => "channel",
        MediaKind::PodcastFeed => "rss_feeds",
        MediaKind::PodcastEpisode => "podcast_episode",
        MediaKind::Movie => "movie",
        MediaKind::TvShow => "tv_show",
        MediaKind::TvEpisode => "episode",
        MediaKind::Unknown => "unknown",
    })
}

fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return String::new();
    }
    let Ok(duration) = std::time::Duration::try_from_secs_f64(seconds) else {
        return String::new();
    };
    let total = duration.as_secs();
    let hours = total / 3_600;
    let minutes = (total % 3_600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_count(value: u64) -> String {
    if value >= 1_000_000_000 {
        compact_count(value, 1_000_000_000, 'B')
    } else if value >= 1_000_000 {
        compact_count(value, 1_000_000, 'M')
    } else if value >= 1_000 {
        compact_count(value, 1_000, 'K')
    } else {
        value.to_string()
    }
}

fn compact_count(value: u64, unit: u64, suffix: char) -> String {
    let whole = value / unit;
    let decimal = (value % unit).saturating_mul(10) / unit;
    format!("{whole}.{decimal}{suffix}")
}

fn display_count(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    metadata_u64(metadata, key)
        .map(format_count)
        .or_else(|| metadata_string(metadata, key))
}

fn display_upload_age(catalog: &TranslationCatalog, item: &MediaItem) -> String {
    if let Some(age) = metadata_string(&item.metadata, "age") {
        return age;
    }
    let timestamp = metadata_i64(&item.metadata, "timestamp")
        .or_else(|| metadata_i64(&item.metadata, "release_timestamp"))
        .or_else(|| {
            metadata_string(&item.metadata, "upload_date")
                .or_else(|| metadata_string(&item.metadata, "uploaded_at"))
                .and_then(|date| upload_date_timestamp(&date))
        });
    if let Some(timestamp) = timestamp {
        return format!("{} {}", catalog.text("uploaded"), format_ago(timestamp));
    }
    metadata_string(&item.metadata, "uploaded_at").unwrap_or_default()
}

fn upload_date_timestamp(value: &str) -> Option<i64> {
    NaiveDate::parse_from_str(value.trim(), "%Y%m%d")
        .ok()?
        .and_hms_opt(0, 0, 0)
        .map(|date| date.and_utc().timestamp())
}

fn format_ago(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(timestamp);
    let difference = now.saturating_sub(timestamp).max(0);
    for (name, seconds) in [
        ("year", 31_536_000),
        ("month", 2_592_000),
        ("day", 86_400),
        ("hour", 3_600),
        ("minute", 60),
    ] {
        if difference >= seconds {
            let amount = difference / seconds;
            return format!("{amount} {name}{} ago", if amount == 1 { "" } else { "s" });
        }
    }
    "just now".to_owned()
}

fn metadata_string(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    metadata
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn metadata_f64(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<f64> {
    metadata.get(key).and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str()?.trim().parse().ok())
    })
}

fn metadata_u32(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<u32> {
    metadata.get(key).and_then(|value| {
        value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .or_else(|| value.as_str()?.trim().parse().ok())
    })
}

fn metadata_u64(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<u64> {
    metadata.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str()?.trim().parse().ok())
    })
}

fn metadata_i64(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<i64> {
    metadata.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
            .or_else(|| value.as_str()?.trim().parse().ok())
    })
}

fn format_container(value: &str) -> Option<String> {
    let first = value.split(',').next()?.trim().to_ascii_lowercase();
    if first.is_empty() {
        return None;
    }
    Some(if first.contains("mp4") || first.contains("mov") {
        "MP4".to_owned()
    } else {
        first.to_ascii_uppercase()
    })
}

fn format_audio_codec(value: &str) -> Option<String> {
    let codec = value.trim().to_ascii_lowercase();
    if codec.is_empty() || codec == "none" {
        return None;
    }
    Some(if codec.contains("mp4a") || codec.contains("aac") {
        "AAC".to_owned()
    } else if codec.contains("opus") {
        "Opus".to_owned()
    } else if codec.contains("mp3") {
        "MP3".to_owned()
    } else if codec.contains("flac") {
        "FLAC".to_owned()
    } else if codec.contains("vorbis") || codec.contains("ogg") {
        "Vorbis".to_owned()
    } else if codec.contains("pcm") || codec.contains("wav") {
        "WAV".to_owned()
    } else {
        codec.to_ascii_uppercase()
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaKind, MediaSource};
    use apricot_playback::PlaybackMediaInfo;

    use super::{details_text, format_status};
    use crate::english_catalog;

    fn item() -> apricot_core::MediaItem {
        apricot_core::MediaItem {
            id: MediaId("video".to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: "Video".to_owned(),
            url: Some("https://youtube.com/watch?v=video123".parse().expect("URL")),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn runtime_video_information_is_formatted_like_the_python_player() {
        let status = format_status(
            &english_catalog(),
            &item(),
            &PlaybackMediaInfo {
                container: Some("matroska,webm".to_owned()),
                video_codec: Some("vp9".to_owned()),
                height: Some(1080),
                audio_codec: Some("opus".to_owned()),
                audio_bitrate_bits_per_second: Some(129_500.0),
                ..PlaybackMediaInfo::default()
            },
        );
        assert_eq!(
            status,
            "Format: MATROSKA 1080p video, Opus audio (130 kbps)"
        );
    }

    #[test]
    fn local_extension_and_metadata_take_priority_without_blocking_probe() {
        let mut media = item();
        media.source = MediaSource::Local;
        media.local_path = Some(r"C:\Music\song.flac".to_owned());
        media.metadata.insert("acodec".to_owned(), "flac".into());
        assert_eq!(
            format_status(&english_catalog(), &media, &PlaybackMediaInfo::default()),
            "Format: FLAC audio (FLAC)"
        );
    }

    #[test]
    fn details_are_source_neutral_and_use_durable_locations() {
        let mut media = item();
        media.channel = "Channel name".to_owned();
        media.duration_seconds = Some(3_661.0);
        media
            .metadata
            .insert("view_count".to_owned(), 1_234_567_u64.into());
        media
            .metadata
            .insert("age".to_owned(), "uploaded 2 days ago".into());
        media
            .metadata
            .insert("description".to_owned(), "Description text".into());
        media.stream_url = Some("https://cdn.example/temporary".parse().expect("stream URL"));

        let details = details_text(&english_catalog(), &media, 1.25, 0.95);
        assert!(details.starts_with("Video\r\nChannel: Channel name\r\n"));
        assert!(details.contains("URL: https://youtube.com/watch?v=video123"));
        assert!(details.contains("Views: 1.2M"));
        assert!(details.contains("Duration: 1:01:01"));
        assert!(details.contains("Playback speed: 1.25x"));
        assert!(details.contains("Pitch: 0.95x"));
        assert!(!details.contains("cdn.example"));
    }

    #[test]
    fn local_details_show_the_path_and_media_type() {
        let mut media = item();
        media.source = MediaSource::Local;
        media.kind = MediaKind::Audio;
        media.url = None;
        media.local_path = Some(r"C:\Music\song.flac".to_owned());
        let details = details_text(&english_catalog(), &media, 1.0, 1.0);
        assert!(details.contains(r"URL: C:\Music\song.flac"));
        assert!(details.contains("Type: Audio"));
    }
}
