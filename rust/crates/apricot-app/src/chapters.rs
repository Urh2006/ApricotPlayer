//! Chapter ordering and keyboard navigation shared by native player surfaces.

use apricot_core::MediaItem;

#[derive(Clone, Debug, PartialEq)]
pub struct Chapter {
    pub title: String,
    pub start_seconds: f64,
    pub end_seconds: Option<f64>,
}

#[must_use]
pub fn session_chapters(session: &crate::PlayerSession) -> Vec<Chapter> {
    let chapters = session
        .current_item()
        .map(item_chapters)
        .unwrap_or_default();
    if !chapters.is_empty() {
        return chapters;
    }
    normalized_chapters(&serde_json::Value::Array(
        session.media_info().chapters.clone(),
    ))
}

#[must_use]
pub fn item_chapters(item: &MediaItem) -> Vec<Chapter> {
    normalized_chapters(
        item.metadata
            .get("chapters")
            .unwrap_or(&serde_json::Value::Null),
    )
}

#[must_use]
pub fn normalized_chapters(value: &serde_json::Value) -> Vec<Chapter> {
    let mut chapters = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| {
            let start = first_field(value, &["start_time", "time", "start", "startTime"])
                .and_then(chapter_seconds)?;
            if !start.is_finite() || start < 0.0 {
                return None;
            }
            Some(Chapter {
                end_seconds: first_field(value, &["end_time", "end", "endTime"])
                    .and_then(chapter_seconds)
                    .filter(|end| end.is_finite() && *end > start),
                title: value
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .filter(|title| !title.is_empty())
                    .or_else(|| value.get("name").and_then(serde_json::Value::as_str))
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                start_seconds: start,
            })
        })
        .collect::<Vec<_>>();
    chapters.sort_by(|left, right| left.start_seconds.total_cmp(&right.start_seconds));
    chapters
}

fn first_field<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a serde_json::Value> {
    keys.iter().find_map(|key| value.get(*key))
}

fn chapter_seconds(value: &serde_json::Value) -> Option<f64> {
    let seconds = if let Some(number) = value.as_f64() {
        number
    } else {
        let text = value.as_str()?.trim();
        let mut seconds = 0.0;
        for part in text.split(':') {
            seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
        }
        seconds
    };
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds)
}

/// Retains Python's next/previous tolerances, including restarting the current
/// chapter when its beginning is more than 1.5 seconds behind the playhead.
#[must_use]
pub fn relative_chapter(chapters: &[Chapter], position: f64, next: bool) -> Option<&Chapter> {
    let position = if position.is_finite() {
        position.max(0.0)
    } else {
        0.0
    };
    if next {
        chapters
            .iter()
            .find(|chapter| chapter.start_seconds > position + 0.75)
    } else {
        chapters
            .iter()
            .rev()
            .find(|chapter| chapter.start_seconds < position - 1.5)
            .or_else(|| chapters.first())
    }
}

#[cfg(test)]
// Fixtures use exactly representable integers and halves; navigation returns them unchanged.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn chapters() -> Vec<Chapter> {
        [0.0, 30.0, 60.0]
            .into_iter()
            .map(|start_seconds| Chapter {
                end_seconds: None,
                title: String::new(),
                start_seconds,
            })
            .collect()
    }

    #[test]
    fn source_aliases_and_clock_strings_normalize_in_time_order() {
        let result = normalized_chapters(&serde_json::json!([
            {"startTime":"01:02:03.5", "endTime":"01:03:00", "name":"Late"},
            {"time":0, "title":"Intro"},
            {"start":"00:30", "end":"00:20", "title":"Topic"},
            {"start_time":"NaN"},
            {"start_time":-3},
            null
        ]));
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].title, "Intro");
        assert_eq!(result[1].start_seconds, 30.0);
        assert_eq!(result[1].end_seconds, None);
        assert_eq!(result[2].start_seconds, 3723.5);
        assert_eq!(result[2].end_seconds, Some(3780.0));
        assert_eq!(result[2].title, "Late");
    }

    #[test]
    fn previous_restarts_current_or_selects_prior_with_python_tolerance() {
        let chapters = chapters();
        assert_eq!(
            relative_chapter(&chapters, 33.0, false)
                .unwrap()
                .start_seconds,
            30.0
        );
        assert_eq!(
            relative_chapter(&chapters, 31.0, false)
                .unwrap()
                .start_seconds,
            0.0
        );
        assert_eq!(
            relative_chapter(&chapters, 0.0, false)
                .unwrap()
                .start_seconds,
            0.0
        );
    }

    #[test]
    fn next_skips_near_current_boundary_and_never_wraps() {
        let chapters = chapters();
        assert_eq!(
            relative_chapter(&chapters, 29.5, true)
                .unwrap()
                .start_seconds,
            60.0
        );
        assert!(relative_chapter(&chapters, 60.0, true).is_none());
        assert!(relative_chapter(&[], 0.0, false).is_none());
    }
}
