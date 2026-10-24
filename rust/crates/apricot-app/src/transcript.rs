//! SRT/WebVTT cue parsing, preserving the Python player's source order.

use std::sync::OnceLock;
use std::{io::Read, path::Path};

const MAX_LOCAL_TRANSCRIPT_BYTES: u64 = 5_000_000;

/// Reads the first usable sidecar. Call on a worker, never on the UI thread.
#[must_use]
pub fn local_transcript(media: &Path, configured_languages: &str) -> Vec<TranscriptEntry> {
    let Some(stem) = media.file_stem() else {
        return Vec::new();
    };
    let mut candidates = vec![media.with_extension("vtt"), media.with_extension("srt")];
    let suffixes = [
        "captions.vtt",
        "captions.srt",
        "transcript.vtt",
        "transcript.srt",
    ];
    for suffix in suffixes {
        let mut name = stem.to_os_string();
        name.push(format!(".{suffix}"));
        candidates.push(media.with_file_name(name));
    }
    for language in language_candidates(configured_languages) {
        let safe: String = language
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            .collect();
        if safe.is_empty() {
            continue;
        }
        for extension in ["vtt", "srt"] {
            let mut name = stem.to_os_string();
            name.push(format!(".{safe}.{extension}"));
            let candidate = media.with_file_name(name);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    for candidate in candidates {
        let Ok(file) = std::fs::File::open(candidate) else {
            continue;
        };
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_LOCAL_TRANSCRIPT_BYTES {
            continue;
        }
        let mut bytes = Vec::new();
        if file
            .take(MAX_LOCAL_TRANSCRIPT_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() as u64 > MAX_LOCAL_TRANSCRIPT_BYTES
        {
            continue;
        }
        let entries = parse_transcript(&String::from_utf8_lossy(&bytes));
        if !entries.is_empty() {
            return entries;
        }
    }
    Vec::new()
}

#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptEntry {
    pub start: f64,
    pub end: Option<f64>,
    pub text: String,
}

/// A checked result, including an empty result, belongs to one playback item.
#[derive(Clone, Debug, PartialEq)]
pub struct CachedTranscript {
    pub entries: Vec<TranscriptEntry>,
    pub source_key: String,
}

/// Dialog projection: filtering retains original cue indices for copy and seek.
#[derive(Clone, Debug, Default)]
pub struct TranscriptView {
    entries: Vec<TranscriptEntry>,
    labels: Vec<String>,
    visible: Vec<usize>,
}

impl TranscriptView {
    #[must_use]
    pub fn new(entries: Vec<TranscriptEntry>) -> Self {
        let labels = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let start = display_time(entry.start);
                let text = entry.text.trim();
                if let Some(end) = entry.end {
                    format!("{}. {start} - {}. {text}", index + 1, display_time(end))
                } else {
                    format!("{}. {start}. {text}", index + 1)
                }
            })
            .collect();
        let visible = (0..entries.len()).collect();
        Self {
            entries,
            labels,
            visible,
        }
    }

    pub fn filter(&mut self, query: &str) {
        let query = query.trim().to_lowercase();
        self.visible = self
            .labels
            .iter()
            .enumerate()
            .filter_map(|(index, label)| {
                (query.is_empty() || label.to_lowercase().contains(&query)).then_some(index)
            })
            .collect();
    }

    pub fn visible_labels(&self) -> impl Iterator<Item = &str> {
        self.visible
            .iter()
            .map(|index| self.labels[*index].as_str())
    }

    #[must_use]
    pub fn selected(&self, row: usize) -> Option<(usize, &TranscriptEntry, &str)> {
        let index = *self.visible.get(row)?;
        Some((index, &self.entries[index], &self.labels[index]))
    }

    #[must_use]
    pub fn has_entries(&self) -> bool {
        !self.entries.is_empty()
    }

    #[must_use]
    pub fn full_text(&self) -> String {
        self.labels.join("\n")
    }
}

fn display_time(seconds: f64) -> String {
    let total = std::time::Duration::try_from_secs_f64(seconds.max(0.0).floor())
        .map_or(0, |duration| duration.as_secs());
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptSource {
    Subtitles,
    AutomaticCaptions,
}

#[must_use]
pub fn language_candidates(configured: &str) -> Vec<String> {
    let mut languages: Vec<String> = Vec::new();
    for value in configured.split(',').chain(["en", "sl"]).map(str::trim) {
        if !value.is_empty()
            && !languages
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(value))
        {
            languages.push(value.to_owned());
        }
    }
    languages
}

fn language_matches(available: &str, requested: &str) -> bool {
    let available = available.to_lowercase();
    let requested = requested.to_lowercase();
    !available.is_empty()
        && !requested.is_empty()
        && (available == requested
            || available.starts_with(&format!("{requested}-"))
            || requested.starts_with(&format!("{available}-")))
}

/// Borrows the full track object so request headers survive track selection.
#[must_use]
pub fn select_track<'a>(
    info: &'a serde_json::Value,
    configured: &str,
) -> Option<(&'a serde_json::Value, TranscriptSource)> {
    let languages = language_candidates(configured);
    for (key, source) in [
        ("requested_subtitles", TranscriptSource::Subtitles),
        ("subtitles", TranscriptSource::Subtitles),
        ("automatic_captions", TranscriptSource::AutomaticCaptions),
    ] {
        let Some(group) = info.get(key).and_then(serde_json::Value::as_object) else {
            continue;
        };
        let mut ordered = Vec::new();
        for requested in &languages {
            for language in group.keys() {
                if language_matches(language, requested) && !ordered.contains(&language) {
                    ordered.push(language);
                }
            }
        }
        for language in group.keys() {
            if !ordered.contains(&language) {
                ordered.push(language);
            }
        }
        for language in ordered {
            let value = &group[language];
            let tracks: Vec<_> = if let Some(array) = value.as_array() {
                array.iter().collect()
            } else {
                vec![value]
            };
            let tracks: Vec<_> = tracks
                .into_iter()
                .filter(|track| {
                    track.is_object()
                        && track
                            .get("url")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|url| !url.is_empty())
                })
                .collect();
            for extension in ["vtt", "srt"] {
                if let Some(track) = tracks.iter().find(|track| {
                    track
                        .get("ext")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
                }) {
                    return Some((track, source));
                }
            }
            if let Some(track) = tracks.first() {
                return Some((track, source));
            }
        }
    }
    None
}

/// Parses subtitle text without sorting cues or merging distinct repeated speech.
#[must_use]
pub fn parse_transcript(text: &str) -> Vec<TranscriptEntry> {
    let normalized = text
        .replace('\u{feff}', "")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let lines: Vec<_> = normalized.lines().map(str::trim).collect();
    let mut entries: Vec<TranscriptEntry> = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let mut line = lines[index];
        let upper = line.to_uppercase();
        if line.is_empty()
            || upper == "WEBVTT"
            || upper.starts_with("KIND:")
            || upper.starts_with("LANGUAGE:")
        {
            index += 1;
            continue;
        }
        if ["NOTE", "STYLE", "REGION"]
            .iter()
            .any(|prefix| upper.starts_with(prefix))
        {
            index += 1;
            while index < lines.len() && !lines[index].is_empty() {
                index += 1;
            }
            continue;
        }
        if !line.contains("-->")
            && lines
                .get(index + 1)
                .is_some_and(|line| line.contains("-->"))
        {
            index += 1;
            line = lines[index];
        }
        let Some((left, right)) = line.split_once("-->") else {
            index += 1;
            continue;
        };
        let start = timestamp(left.trim());
        let end = timestamp(right.trim().split(' ').next().unwrap_or_default());
        index += 1;
        let mut body = Vec::new();
        while index < lines.len() && !lines[index].is_empty() {
            if !lines[index].chars().all(char::is_numeric) {
                body.push(lines[index]);
            }
            index += 1;
        }
        let body = clean_text(&body.join(" "));
        let Some(start) = start.filter(|_| !body.is_empty()) else {
            continue;
        };
        if entries
            .last()
            .is_some_and(|last| last.text == body && (last.start - start).abs() <= 0.2)
        {
            continue;
        }
        entries.push(TranscriptEntry {
            start: rounded(start),
            end: end.filter(|end| *end > start).map(rounded),
            text: body,
        });
    }
    entries
}

fn timestamp(text: &str) -> Option<f64> {
    crate::chapters::chapter_seconds(&serde_json::Value::String(text.to_owned()))
}

fn rounded(value: f64) -> f64 {
    (value * 1000.0).round_ties_even() / 1000.0
}

fn clean_text(text: &str) -> String {
    static TAGS: OnceLock<regex::Regex> = OnceLock::new();
    let tags =
        TAGS.get_or_init(|| regex::Regex::new(r"<[^>]+>").expect("static subtitle tag expression"));
    let stripped = tags.replace_all(text, " ");
    html_escape::decode_html_entities(&stripped)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_filter_keeps_original_indices_and_full_copy() {
        let entries = parse_transcript(
            "00:01.500 --> 00:02.500\nFirst\n\n01:02:03.999 --> invalid\nSecond\n",
        );
        let mut view = TranscriptView::new(entries);
        let all = "1. 0:01 - 0:02. First\n2. 1:02:03. Second";
        assert_eq!(view.full_text(), all);
        view.filter(" second ");
        assert_eq!(
            view.visible_labels().collect::<Vec<_>>(),
            ["2. 1:02:03. Second"]
        );
        let (index, entry, label) = view.selected(0).unwrap();
        assert_eq!(index, 1);
        assert!(entry.start > 3723.0);
        assert_eq!(label, "2. 1:02:03. Second");
        assert_eq!(view.full_text(), all);
        view.filter("absent");
        assert!(view.has_entries());
        assert!(view.selected(0).is_none());
        view.filter("0:02");
        assert_eq!(view.selected(0).unwrap().0, 0);
        view.filter("");
        assert_eq!(view.visible_labels().count(), 2);
        assert!(!TranscriptView::default().has_entries());
    }

    #[test]
    fn local_sidecars_fall_through_invalid_content_and_prefer_plain_files() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("Episode.mp3");
        std::fs::write(media.with_extension("vtt"), "Invalid").unwrap();
        std::fs::write(
            media.with_extension("srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nPlain\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("Episode.sl.vtt"),
            "WEBVTT\n\n00:01.000 --> 00:02.000\nTranslated\n",
        )
        .unwrap();
        assert_eq!(local_transcript(&media, "sl")[0].text, "Plain");
        std::fs::remove_file(media.with_extension("srt")).unwrap();
        assert_eq!(local_transcript(&media, "sl")[0].text, "Translated");
    }

    #[test]
    fn local_sidecars_skip_oversized_files_without_reading_them() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("Episode.mp3");
        let oversized = std::fs::File::create(media.with_extension("vtt")).unwrap();
        oversized.set_len(MAX_LOCAL_TRANSCRIPT_BYTES + 1).unwrap();
        std::fs::write(
            temp.path().join("Episode.captions.srt"),
            "00:01.000 --> 00:02.000\nAvailable\n",
        )
        .unwrap();
        assert_eq!(local_transcript(&media, "../../sl")[0].text, "Available");
        assert!(local_transcript(&temp.path().join("Absent.mp3"), "").is_empty());
    }

    #[test]
    fn configured_languages_keep_priority_and_unique_fallbacks() {
        assert_eq!(
            language_candidates(" sl, EN, sl, de ,,"),
            ["sl", "EN", "de"]
        );
        assert_eq!(language_candidates("de"), ["de", "en", "sl"]);
    }

    #[test]
    fn fallback_language_and_regional_ties_follow_source_order() {
        let info: serde_json::Value = serde_json::from_str(
            r#"{"subtitles":{"zz":{"url":"https://example.test/first"},"aa":{"url":"https://example.test/second"}}}"#,
        ).unwrap();
        assert_eq!(
            select_track(&info, "").unwrap().0["url"],
            "https://example.test/first"
        );
        let regional: serde_json::Value = serde_json::from_str(
            r#"{"subtitles":{"en-US":{"url":"https://example.test/us"},"en-GB":{"url":"https://example.test/gb"}}}"#,
        ).unwrap();
        assert_eq!(
            select_track(&regional, "en").unwrap().0["url"],
            "https://example.test/us"
        );
        assert_eq!(
            select_track(&regional, "en-GB").unwrap().0["url"],
            "https://example.test/gb"
        );
    }

    #[test]
    fn track_selection_preserves_source_language_format_and_headers() {
        let info = serde_json::json!({
            "subtitles": {"sl-SI": [
                {"ext":"srt", "url":"https://example.test/srt"},
                {"ext":"VTT", "url":"https://example.test/vtt", "http_headers":{"Referer":"https://example.test/"}}
            ]},
            "automatic_captions": {"en":{"ext":"vtt", "url":"https://example.test/auto"}}
        });
        let (track, source) = select_track(&info, "sl").unwrap();
        assert_eq!(source, TranscriptSource::Subtitles);
        assert_eq!(track["url"], "https://example.test/vtt");
        assert!(track["http_headers"].is_object());
        let requested = serde_json::json!({"requested_subtitles":{"en":{"url":"https://example.test/requested"}}, "subtitles":info["subtitles"]});
        assert_eq!(
            select_track(&requested, "sl").unwrap().0["url"],
            "https://example.test/requested"
        );
    }

    #[test]
    fn invalid_tracks_do_not_hide_automatic_captions() {
        let info = serde_json::json!({"subtitles":{"en":[null, {"url":""}]}, "automatic_captions":{"en-US":[{"url":"https://example.test/auto", "ext":"vtt"}]}});
        assert_eq!(
            select_track(&info, "en").unwrap().1,
            TranscriptSource::AutomaticCaptions
        );
        assert!(select_track(&serde_json::Value::Null, "en").is_none());
    }

    #[test]
    fn srt_cues_preserve_source_order_and_decode_text() {
        let entries = parse_transcript(
            "\u{feff}1\r\n00:01:02,500 --> 00:01:04,000\r\n<b>Hello</b> &amp; &#x17e;\r\nworld\r\n\r\n2\r\n00:00:01,000 --> 00:00:02,000\r\nEarlier\r\n",
        );
        assert_eq!(entries.len(), 2);
        assert!((entries[0].start - 62.5).abs() < f64::EPSILON);
        assert_eq!(entries[0].text, "Hello & \u{17e} world");
        assert!(entries[1].start < entries[0].start);
    }

    #[test]
    fn webvtt_metadata_and_near_duplicates_are_not_spoken() {
        let entries = parse_transcript(
            "WEBVTT\nKind: captions\nLanguage: en\n\nNOTE hidden\ncomment\n\nSTYLE\n::cue {color:red}\n\nidentifier\n00:01.000 --> 00:02.000 align:start\n<v Speaker>Hello <00:01.500>there</v>\n\n00:01.100 --> 00:02.100\nHello there\n\n00:03.000 --> 00:04.000\nHello there\n",
        );
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry.text == "Hello there"));
        assert!(entries.iter().all(|entry| entry.end.is_some()));
    }

    #[test]
    fn invalid_start_is_skipped_and_invalid_end_is_optional() {
        let entries = parse_transcript(
            "bad --> 00:02.000\nIgnore\n\n00:03.000 --> invalid\nKeep\n\n00:04.000 --> 00:02.000\nKeep too\n",
        );
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry.end.is_none()));
        assert!(parse_transcript("NaN --> inf\nInvalid\n").is_empty());
    }
}
