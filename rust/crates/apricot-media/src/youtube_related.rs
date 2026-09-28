//! Related videos from a `YouTube` watch page, as Python
//! `fetch_related_and_play_next` reads them from `ytInitialData`.

use std::collections::BTreeMap;

use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
use serde_json::{Map, Value};
use url::Url;

/// Python tries these assignments in order.
const INITIAL_DATA_MARKERS: [&str; 3] = [
    "var ytInitialData",
    "window[\"ytInitialData\"]",
    "ytInitialData",
];

/// Parses the related videos of a watch page in page order. Items keep
/// Python's "Unknown" channel when the page has none.
pub fn parse_related_videos(html: &str) -> Vec<MediaItem> {
    let Some(data) = initial_data(html) else {
        return Vec::new();
    };
    let mut videos = Vec::new();
    collect(&data, &mut videos);
    videos
}

fn initial_data(html: &str) -> Option<Value> {
    INITIAL_DATA_MARKERS.iter().find_map(|marker| {
        html.match_indices(marker).find_map(|(index, _)| {
            let rest = html[index + marker.len()..].trim_start();
            let rest = rest.strip_prefix('=')?.trim_start();
            if !rest.starts_with('{') {
                return None;
            }
            serde_json::Deserializer::from_str(rest)
                .into_iter::<Value>()
                .next()?
                .ok()
                .filter(Value::is_object)
        })
    })
}

fn collect(value: &Value, videos: &mut Vec<MediaItem>) {
    match value {
        Value::Object(object) => {
            if let Some(lockup) = object.get("lockupViewModel") {
                if let Some(video) = lockup_video(lockup) {
                    videos.push(video);
                }
                return;
            }
            if let Some(renderer) = object
                .get("compactVideoRenderer")
                .and_then(Value::as_object)
            {
                videos.push(compact_video(renderer));
            }
            for child in object.values() {
                collect(child, videos);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect(child, videos);
            }
        }
        _ => {}
    }
}

fn lockup_video(lockup: &Value) -> Option<MediaItem> {
    let mut video_id = None;
    let overlays =
        path(lockup, &["contentImage", "thumbnailViewModel", "overlays"]).and_then(Value::as_array);
    for overlay in overlays.into_iter().flatten() {
        let badges =
            path(overlay, &["thumbnailBottomOverlayViewModel", "badges"]).and_then(Value::as_array);
        for badge in badges.into_iter().flatten() {
            if let Some(target) = path(
                badge,
                &["thumbnailBadgeViewModel", "animationActivationTargetId"],
            ) {
                video_id = target.as_str().map(str::to_owned);
            }
        }
    }
    let video_id = video_id
        .filter(|id| !id.is_empty())
        .or_else(|| lockup.as_object().and_then(find_video_id))?;
    let metadata = path(lockup, &["metadata", "lockupMetadataViewModel"]);
    let title = metadata
        .and_then(|metadata| path(metadata, &["title", "content"]))
        .and_then(Value::as_str)
        .filter(|title| !title.is_empty())?;
    let channel = match metadata.and_then(|metadata| metadata.get("byline")) {
        Some(Value::Object(byline)) => byline.get("content").and_then(Value::as_str),
        Some(Value::Array(bylines)) => bylines
            .first()
            .and_then(|byline| byline.get("content"))
            .and_then(Value::as_str),
        _ => None,
    };
    Some(video_item(&video_id, title, channel))
}

/// Python `find_vid_in_dict`: the first `videoId` in document order.
fn find_video_id(object: &Map<String, Value>) -> Option<String> {
    for (key, value) in object {
        if key == "videoId" {
            return value
                .as_str()
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
        }
        if key == "watchEndpoint" && value.is_object() {
            if let Some(id) = value.get("videoId") {
                return id.as_str().filter(|id| !id.is_empty()).map(str::to_owned);
            }
            continue;
        }
        let found = match value {
            Value::Object(child) => find_video_id(child),
            Value::Array(children) => children
                .iter()
                .filter_map(Value::as_object)
                .find_map(find_video_id),
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn compact_video(renderer: &Map<String, Value>) -> MediaItem {
    let title = renderer.get("title").map_or("", |title| {
        first_run_text(title)
            .or_else(|| title.get("simpleText").and_then(Value::as_str))
            .unwrap_or_default()
    });
    let channel = renderer.get("longBylineText").and_then(first_run_text);
    let video_id = renderer
        .get("videoId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    video_item(video_id, title, channel)
}

fn first_run_text(value: &Value) -> Option<&str> {
    value
        .get("runs")?
        .as_array()?
        .first()?
        .get("text")
        .and_then(Value::as_str)
}

fn path<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().try_fold(value, |value, key| value.get(key))
}

/// Python `normalize_entry` for `{webpage_url, id, title, uploader}` as a
/// video.
fn video_item(video_id: &str, title: &str, channel: Option<&str>) -> MediaItem {
    MediaItem {
        id: MediaId(video_id.to_owned()),
        source: MediaSource::Youtube,
        kind: MediaKind::Video,
        title: title.to_owned(),
        url: Url::parse(&format!("https://www.youtube.com/watch?v={video_id}")).ok(),
        stream_url: None,
        external_audio_url: None,
        local_path: None,
        channel: channel
            .filter(|channel| !channel.is_empty())
            .unwrap_or("Unknown")
            .to_owned(),
        duration_seconds: None,
        metadata: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_related_videos;

    #[test]
    fn reads_lockup_and_compact_videos_in_page_order() {
        let html = r#"<script>var ytInitialData = {"contents":{"results":[
            {"lockupViewModel":{
                "contentImage":{"thumbnailViewModel":{"overlays":[
                    {"thumbnailBottomOverlayViewModel":{"badges":[
                        {"thumbnailBadgeViewModel":{"animationActivationTargetId":"lockupAAAA1"}}]}}]}},
                "metadata":{"lockupMetadataViewModel":{
                    "title":{"content":"First related"},
                    "byline":[{"content":"First channel"}]}}}},
            {"lockupViewModel":{
                "rendererContext":{"commandContext":{"onTap":{"innertubeCommand":{
                    "watchEndpoint":{"videoId":"lockupBBBB2"}}}}},
                "metadata":{"lockupMetadataViewModel":{"title":{"content":"Second related"}}}}},
            {"lockupViewModel":{"metadata":{"lockupMetadataViewModel":{"title":{"content":"No id"}}}}},
            {"compactVideoRenderer":{"videoId":"compactCCC3",
                "title":{"simpleText":"Third related"},
                "longBylineText":{"runs":[{"text":"Third channel"}]}}}
        ]}};</script>"#;
        let videos = parse_related_videos(html);
        let summary: Vec<_> = videos
            .iter()
            .map(|video| {
                (
                    video.id.0.as_str(),
                    video.title.as_str(),
                    video.channel.as_str(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("lockupAAAA1", "First related", "First channel"),
                ("lockupBBBB2", "Second related", "Unknown"),
                ("compactCCC3", "Third related", "Third channel"),
            ]
        );
        assert_eq!(
            videos[0].url.as_ref().map(url::Url::as_str),
            Some("https://www.youtube.com/watch?v=lockupAAAA1")
        );
    }

    #[test]
    fn falls_back_to_the_window_assignment_and_ignores_pages_without_data() {
        let html = r#"window["ytInitialData"] = {"a":{"compactVideoRenderer":{"videoId":"windowDDD4","title":{"runs":[{"text":"From window"}]}}}};"#;
        let videos = parse_related_videos(html);
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].title, "From window");
        assert!(parse_related_videos("<html>no data</html>").is_empty());
    }
}
