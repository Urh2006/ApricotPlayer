//! Metadata fields of the selected list row, read one at a time with
//! Ctrl+Alt+Left and Ctrl+Alt+Right, matching Python's
//! `result_metadata_columns` and `announce_active_media_column`.

use apricot_core::{MediaItem, MediaKind, TranslationCatalog};

use crate::player_information::{display_count, display_upload_age};

/// Python `result_metadata_columns`: labelled non-empty fields in Python's order.
pub fn result_metadata_columns(
    item: &MediaItem,
    catalog: &TranslationCatalog,
) -> Vec<(String, String)> {
    let duration = metadata_text(item, "duration").or_else(|| {
        item.duration_seconds
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
            .map(format_duration)
    });
    let mut columns = vec![
        ("media_field_title", Some(item.title.trim().to_owned())),
        ("media_field_type", Some(type_label(item, catalog))),
        ("media_field_channel", Some(item.channel.trim().to_owned())),
        ("media_field_duration", duration),
        (
            "media_field_views",
            display_count(&item.metadata, "views")
                .or_else(|| display_count(&item.metadata, "view_count")),
        ),
        (
            "media_field_uploaded",
            Some(display_upload_age(catalog, item)),
        ),
        ("media_field_album", metadata_text(item, "album")),
        (
            "media_field_playlist_count",
            metadata_text(item, "playlist_count"),
        ),
    ];
    if item.is_local_media() {
        columns.push(("media_field_path", item.local_path.clone()));
    }
    columns
        .into_iter()
        .filter_map(|(key, value)| {
            let value = value?.trim().to_owned();
            (!value.is_empty()).then(|| (catalog.text(key).to_owned(), value))
        })
        .collect()
}

/// The announcement for one step, or the Python message when there is
/// nothing to read.
pub fn result_column_announcement(
    cursor: &mut ResultColumnCursor,
    item: &MediaItem,
    catalog: &TranslationCatalog,
    forward: bool,
) -> String {
    let columns = result_metadata_columns(item, catalog);
    let Some(index) = cursor.step(item, columns.len(), forward) else {
        return catalog.text("result_column_unavailable").to_owned();
    };
    let (label, value) = &columns[index];
    catalog
        .text("result_column_value")
        .replace("{label}", label)
        .replace("{value}", value)
        .replace("{current}", &(index + 1).to_string())
        .replace("{total}", &columns.len().to_string())
}

/// Which field was read last, and for which row. A new row starts again at the
/// first field going forward or at the last going back.
#[derive(Debug, Default)]
pub struct ResultColumnCursor {
    identity: String,
    index: Option<usize>,
}

impl ResultColumnCursor {
    pub fn step(&mut self, item: &MediaItem, total: usize, forward: bool) -> Option<usize> {
        if total == 0 {
            return None;
        }
        let identity = row_identity(item);
        if identity != self.identity {
            self.identity = identity;
            self.index = None;
        }
        let index = match (self.index, forward) {
            (None, true) => 0,
            (None, false) => total - 1,
            (Some(index), true) => (index + 1) % total,
            (Some(index), false) => (index + total - 1) % total,
        };
        self.index = Some(index);
        Some(index)
    }
}

/// Python `result_metadata_identity`.
fn row_identity(item: &MediaItem) -> String {
    [
        item.id.0.clone(),
        item.url
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        item.local_path.clone().unwrap_or_default(),
        item.title.clone(),
    ]
    .join("\u{1f}")
}

fn type_label(item: &MediaItem, catalog: &TranslationCatalog) -> String {
    if let Some(label) = metadata_text(item, "type") {
        return label;
    }
    if item.is_local_media() {
        return catalog.text("local_media").to_owned();
    }
    let key = match item.kind {
        MediaKind::Playlist => "playlist",
        MediaKind::Channel => "channel",
        MediaKind::LiveStream => "live_stream",
        MediaKind::PodcastEpisode => "podcast_episode",
        _ => "video",
    };
    catalog.text(key).to_owned()
}

fn metadata_text(item: &MediaItem, key: &str) -> Option<String> {
    match item.metadata.get(key)? {
        serde_json::Value::String(text) => Some(text.trim().to_owned()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
    .filter(|text| !text.is_empty())
}

fn format_duration(seconds: f64) -> String {
    let seconds = std::time::Duration::try_from_secs_f64(seconds.max(0.0))
        .map_or(0, |duration| duration.as_secs());
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};

    use super::{ResultColumnCursor, result_column_announcement, result_metadata_columns};
    use crate::embedded_catalog;

    fn video() -> MediaItem {
        let mut metadata = BTreeMap::new();
        metadata.insert("views".to_owned(), 1_500.into());
        metadata.insert("age".to_owned(), "Uploaded 2 days ago".into());
        MediaItem {
            id: MediaId("abc".to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: "Song".to_owned(),
            url: Some("https://www.youtube.com/watch?v=abc".parse().unwrap()),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: "Artist".to_owned(),
            duration_seconds: Some(65.0),
            metadata,
        }
    }

    #[test]
    fn columns_follow_python_order_and_skip_empty_fields() {
        let catalog = embedded_catalog("en");
        let columns = result_metadata_columns(&video(), &catalog);
        let labels: Vec<_> = columns.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(
            labels,
            ["Title", "Type", "Channel", "Duration", "Views", "Uploaded"]
        );
        assert_eq!(columns[3].1, "1:05");
        assert_eq!(columns[4].1, "1.5K");
    }

    #[test]
    fn stepping_starts_at_first_forward_last_backward_and_wraps() {
        let catalog = embedded_catalog("en");
        let item = video();
        let mut cursor = ResultColumnCursor::default();
        assert_eq!(
            result_column_announcement(&mut cursor, &item, &catalog, true),
            "Title: Song. Field 1 of 6."
        );
        assert_eq!(
            result_column_announcement(&mut cursor, &item, &catalog, false),
            "Uploaded: Uploaded 2 days ago. Field 6 of 6."
        );
        assert_eq!(
            result_column_announcement(&mut cursor, &item, &catalog, true),
            "Title: Song. Field 1 of 6."
        );

        let mut other = item.clone();
        other.title = "Other".to_owned();
        assert_eq!(
            result_column_announcement(&mut cursor, &other, &catalog, false),
            "Uploaded: Uploaded 2 days ago. Field 6 of 6."
        );
    }

    #[test]
    fn local_files_add_the_path() {
        let catalog = embedded_catalog("en");
        let mut item = video();
        item.source = MediaSource::Local;
        item.kind = MediaKind::Audio;
        item.url = None;
        item.local_path = Some(r"C:\Music\song.mp3".to_owned());
        item.metadata.clear();
        let columns = result_metadata_columns(&item, &catalog);
        assert_eq!(columns[1].1, "Local media file");
        assert_eq!(
            columns
                .last()
                .map(|(label, value)| (label.as_str(), value.as_str())),
            Some(("Path", r"C:\Music\song.mp3"))
        );
    }
}
