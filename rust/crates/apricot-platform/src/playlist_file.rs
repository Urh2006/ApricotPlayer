//! Bounded playlist parsing shared by library import and source adapters.
use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
use url::Url;
const MAX_BYTES: u64 = 16 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedPlaylist {
    pub title: String,
    pub items: Vec<MediaItem>,
    pub hls: bool,
}
pub fn is_hls(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim_start().starts_with("#EXT-X-"))
}
/// Reads a playlist file from disk.
///
/// # Errors
///
/// Returns a message when the file cannot be read or parsed.
pub fn read_local(path: &Path) -> Result<ParsedPlaylist, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The playlist exceeds 16 MiB".into());
    }
    let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
    let base = Url::from_file_path(&absolute).map_err(|()| "The playlist path is invalid")?;
    parse(&decode(&bytes), &base)
}
/// Downloads and parses a playlist from the web.
///
/// # Errors
///
/// Returns a message for a non-web address, a failed download or a bad playlist.
pub fn read_url(url: &Url) -> Result<ParsedPlaylist, String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Use an HTTP or HTTPS playlist address".into());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(4))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url.clone())
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| e.to_string())?;
    let final_url = response.url().clone();
    let mut bytes = Vec::new();
    response
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The playlist exceeds 16 MiB".into());
    }
    parse(&decode(&bytes), &final_url)
}
fn decode(bytes: &[u8]) -> String {
    if let Some((encoding, length)) = encoding_rs::Encoding::for_bom(bytes) {
        return encoding.decode(&bytes[length..]).0.into_owned();
    }
    String::from_utf8(bytes.to_vec())
        .unwrap_or_else(|_| encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned())
}
/// Parses playlist text; relative entries resolve against `base`.
///
/// # Errors
///
/// Returns a message when the text is not a playlist of a known format.
pub fn parse(text: &str, base: &Url) -> Result<ParsedPlaylist, String> {
    let ext = Path::new(base.path())
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if is_hls(text) {
        return Ok(ParsedPlaylist {
            title: String::new(),
            items: vec![location(base.as_str(), "", None, base)?],
            hls: true,
        });
    }
    let mut title = String::new();
    let items = match ext.as_str() {
        "pls" => parse_pls(text, base)?,
        "xspf" | "asx" => parse_xml(text, base, &ext)?,
        "cue" => parse_cue(text, base)?,
        "json" => {
            let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
            value
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .clone_into(&mut title);
            let entries = value
                .get("items")
                .and_then(serde_json::Value::as_array)
                .ok_or("The playlist JSON has no items array")?;
            entries
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    serde_json::from_value::<MediaItem>(item.clone())
                        .map_err(|e| format!("Playlist entry {}: {e}", i + 1))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => {
            let mut result = Vec::new();
            let mut name = String::new();
            let mut duration = None;
            for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                if let Some(value) = line.strip_prefix("#PLAYLIST:") {
                    value.trim().clone_into(&mut title);
                    continue;
                }
                if let Some(value) = line.strip_prefix("#EXTINF:") {
                    if let Some((seconds, label)) = value.split_once(',') {
                        duration = seconds
                            .split_whitespace()
                            .next()
                            .and_then(|v| v.parse::<f64>().ok())
                            .filter(|n| n.is_finite() && *n >= 0.0);
                        label.trim().clone_into(&mut name);
                    }
                } else if !line.starts_with('#') {
                    result.push(location(line, &name, duration, base)?);
                    name.clear();
                    duration = None;
                }
            }
            result
        }
    };
    Ok(ParsedPlaylist {
        title,
        items,
        hls: false,
    })
}
fn location(
    value: &str,
    title: &str,
    duration: Option<f64>,
    base: &Url,
) -> Result<MediaItem, String> {
    let value = value.trim().trim_matches('"');
    if value.is_empty() {
        return Err("A playlist entry has an empty location".into());
    }
    if let Some(reference) = apricot_core::SpotifyRef::parse(value) {
        return Ok(MediaItem {
            id: MediaId(reference.to_uri()),
            source: MediaSource::Spotify,
            kind: if reference.kind == apricot_core::SpotifyEntityKind::Playlist {
                MediaKind::Playlist
            } else {
                MediaKind::Audio
            },
            title: if title.is_empty() {
                value.into()
            } else {
                title.into()
            },
            url: reference.to_url().and_then(|v| v.parse().ok()),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: duration,
            metadata: BTreeMap::new(),
        });
    }
    let windows_absolute = value.as_bytes().get(1) == Some(&b':')
        && value
            .as_bytes()
            .get(2)
            .is_some_and(|b| matches!(b, b'/' | b'\\'));
    let unresolved = "A playlist entry could not be resolved";
    // Local references stay filesystem paths so `#`, `?` and `%` in file names
    // are kept; only real URLs and web-relative entries go through `Url`.
    let mut web = None;
    let local = if Path::new(value).is_absolute() {
        Some(PathBuf::from(value))
    } else if windows_absolute {
        return Err(unresolved.into());
    } else if let Ok(url) = Url::parse(value) {
        if url.scheme() == "file" {
            Some(
                url.to_file_path()
                    .map_err(|()| "A local playlist path is invalid")?,
            )
        } else {
            web = Some(url);
            None
        }
    } else {
        let relative = value.replace('\\', "/");
        if base.scheme() == "file" {
            let folder = base
                .to_file_path()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .ok_or(unresolved)?;
            Some(folder.join(relative))
        } else {
            web = Some(base.join(&relative).map_err(|_| unresolved)?);
            None
        }
    };
    let mut item = if let Some(path) = local {
        let name = path
            .file_stem()
            .map_or_else(|| value.into(), |v| v.to_string_lossy().into_owned());
        MediaItem {
            id: MediaId(path.to_string_lossy().into_owned()),
            source: MediaSource::Local,
            kind: if is_video(&path) {
                MediaKind::Video
            } else {
                MediaKind::Audio
            },
            title: name,
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: Some(path.to_string_lossy().into_owned()),
            channel: String::new(),
            duration_seconds: duration,
            metadata: BTreeMap::new(),
        }
    } else {
        let url = web.ok_or(unresolved)?;
        MediaItem::from_direct_link(url.as_str())
            .ok_or("The playlist contains an unsupported address")?
    };
    if !title.is_empty() {
        title.clone_into(&mut item.title);
    }
    item.duration_seconds = duration;
    Ok(item)
}
fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            crate::local_media::VIDEO_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn parse_pls(text: &str, base: &Url) -> Result<Vec<MediaItem>, String> {
    let mut entries = BTreeMap::<u32, (String, String, Option<f64>)>::new();
    for line in text.lines().map(str::trim) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let lower = key.trim().to_ascii_lowercase();
        for prefix in ["file", "title", "length"] {
            if let Some(index) = lower
                .strip_prefix(prefix)
                .and_then(|s| s.parse::<u32>().ok())
            {
                let entry = entries.entry(index).or_default();
                match prefix {
                    "file" => value.trim().clone_into(&mut entry.0),
                    "title" => value.trim().clone_into(&mut entry.1),
                    _ => {
                        entry.2 = value
                            .trim()
                            .parse::<f64>()
                            .ok()
                            .filter(|n| n.is_finite() && *n >= 0.0);
                    }
                }
            }
        }
    }
    entries
        .values()
        .map(|(path, title, duration)| location(path, title, *duration, base))
        .collect()
}
fn parse_xml(text: &str, base: &Url, kind: &str) -> Result<Vec<MediaItem>, String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let tag = if kind == "xspf" { "track" } else { "entry" };
    let mut items = Vec::new();
    for entry in doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name().eq_ignore_ascii_case(tag))
    {
        let name = entry
            .children()
            .find(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("title"))
            .and_then(|n| n.text())
            .unwrap_or("");
        let source = if kind == "xspf" {
            entry
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "location")
                .and_then(|n| n.text())
        } else {
            entry
                .children()
                .find(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("ref"))
                .and_then(|n| n.attribute("href").or_else(|| n.attribute("HREF")))
        };
        let source = source.ok_or("An XML playlist entry has no location")?;
        let duration = entry
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "duration")
            .and_then(|n| n.text())
            .and_then(|n| n.parse::<f64>().ok())
            .map(|v| v / 1000.0);
        items.push(location(source, name, duration, base)?);
    }
    Ok(items)
}
fn cue_time(value: &str) -> Option<f64> {
    let p = value
        .split(':')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (p.len() == 3 && p[1] < 60 && p[2] < 75)
        .then(|| f64::from(p[0]) * 60.0 + f64::from(p[1]) + f64::from(p[2]) / 75.0)
}
fn parse_cue(text: &str, base: &Url) -> Result<Vec<MediaItem>, String> {
    let mut items: Vec<MediaItem> = Vec::new();
    let mut file = String::new();
    let mut track = 0;
    let mut title = String::new();
    let mut artist = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("FILE ") {
            file = if let Some(quoted) = value.strip_prefix('"') {
                quoted.split('"').next().unwrap_or("").into()
            } else {
                value.split_whitespace().next().unwrap_or("").into()
            };
        } else if let Some(value) = line.strip_prefix("TRACK ") {
            track = value
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .ok_or("Invalid CUE track")?;
            title = format!("Track {track}");
        } else if let Some(value) = line.strip_prefix("TITLE ") {
            value.trim_matches('"').clone_into(&mut title);
        } else if let Some(value) = line.strip_prefix("PERFORMER ") {
            value.trim_matches('"').clone_into(&mut artist);
        } else if let Some(value) = line.strip_prefix("INDEX 01 ") {
            if track == 0 || file.is_empty() {
                return Err("CUE index has no file or track".into());
            }
            let start = cue_time(value).ok_or("Invalid CUE time")?;
            let mut item = location(&file, &title, None, base)?;
            item.channel.clone_from(&artist);
            item.id.0 = format!("{}#cue:{track}", item.id.0);
            item.metadata
                .insert("apricot_segment_start".into(), serde_json::json!(start));
            if let Some(previous) = items
                .last_mut()
                .filter(|previous| previous.copy_location() == item.copy_location())
            {
                let old = previous
                    .metadata
                    .get("apricot_segment_start")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(0.0);
                if start <= old {
                    return Err("CUE track times are not increasing".into());
                }
                previous
                    .metadata
                    .insert("apricot_segment_end".into(), serde_json::json!(start));
                previous.duration_seconds = Some(start - old);
            }
            items.push(item);
        }
    }
    Ok(items)
}
/// A playlist entry that stands for a folder, read again on every open.
///
/// # Errors
///
/// Returns a message when the path is not an existing folder.
pub fn folder_reference(path: &Path) -> Result<MediaItem, String> {
    let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
    if !absolute.is_dir() {
        return Err("Choose an existing folder".into());
    }
    Ok(MediaItem {
        id: MediaId(format!("folder:{}", absolute.display())),
        source: MediaSource::Local,
        kind: MediaKind::Unknown,
        title: format!("Folder: {}", absolute.display()),
        url: None,
        stream_url: None,
        external_audio_url: None,
        local_path: Some(absolute.to_string_lossy().into_owned()),
        channel: String::new(),
        duration_seconds: None,
        metadata: BTreeMap::from([("apricot_folder_reference".into(), serde_json::json!(true))]),
    })
}
/// Replaces folder entries with the media files they currently contain.
///
/// # Errors
///
/// Returns a message when cancelled or when a folder cannot be read.
pub fn resolve_folders(
    items: &[MediaItem],
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<MediaItem>, String> {
    let mut result = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Playlist loading cancelled".into());
        }
        let folder = item
            .metadata
            .get("apricot_folder_reference")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let entries = if folder {
            match item
                .local_path
                .as_deref()
                .ok_or_else(|| "Folder reference has no path".to_owned())
                .and_then(|path| {
                    crate::local_media::scan_local_media_folder_with_cancel(
                        Path::new(path),
                        cancelled,
                    )
                    .map_err(|e| e.to_string())
                }) {
                Ok(items) => items,
                Err(error) => {
                    let mut missing = item.clone();
                    missing
                        .metadata
                        .insert("apricot_missing".into(), serde_json::json!(error));
                    vec![missing]
                }
            }
        } else {
            vec![item.clone()]
        };
        for mut entry in entries {
            if folder
                && item
                    .metadata
                    .get("apricot_folder_exclusions")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|paths| {
                        entry.local_path.as_ref().is_some_and(|path| {
                            paths.iter().any(|entry| entry.as_str() == Some(path))
                        })
                    })
            {
                continue;
            }
            let base_id = item
                .metadata
                .get("apricot_playlist_entry_id")
                .and_then(serde_json::Value::as_str)
                .map_or_else(|| format!("{index}:{}", item.id.0), str::to_owned);
            let entry_id = if folder {
                format!("{base_id}:{}", entry.id.0)
            } else {
                base_id
            };
            entry.metadata.insert(
                "apricot_playlist_entry_id".into(),
                serde_json::json!(entry_id),
            );
            entry.metadata.insert(
                "apricot_playlist_source_index".into(),
                serde_json::json!(index),
            );
            result.push(entry);
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_preserve_duplicates_relative_paths_and_titles() {
        let base = Url::parse("https://example.org/music/list.m3u").unwrap();
        let list = parse("#EXTM3U\n#EXTINF:12,Song\na.mp3\na.mp3", &base).unwrap();
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[0].title, "Song");
        assert_eq!(
            list.items[0].url.as_ref().unwrap().as_str(),
            "https://example.org/music/a.mp3"
        );
        assert!(
            parse("#EXTM3U\n#EXT-X-TARGETDURATION:10\na.ts", &base)
                .unwrap()
                .hls
        );
    }
    #[test]
    fn supported_formats_resolve_entries() {
        let cases = [
            ("pls", "[playlist]\nFile1=a.mp3\nTitle1=A"),
            (
                "xspf",
                "<playlist xmlns=\"http://xspf.org/ns/0/\"><trackList><track><location>a.mp3</location><title>A</title></track></trackList></playlist>",
            ),
            (
                "asx",
                "<ASX><ENTRY><TITLE>A</TITLE><REF HREF=\"a.mp3\"/></ENTRY></ASX>",
            ),
        ];
        for (ext, text) in cases {
            let base = Url::parse(&format!("https://example.org/list.{ext}")).unwrap();
            let list = parse(text, &base).unwrap();
            assert_eq!(list.items.len(), 1);
            assert_eq!(list.items[0].title, "A");
        }
        let base = Url::parse("https://example.org/book.cue").unwrap();
        let list=parse("FILE \"book.flac\" WAVE\n TRACK 01 AUDIO\n TITLE \"One\"\n INDEX 01 00:00:00\n TRACK 02 AUDIO\n TITLE \"Two\"\n INDEX 01 02:30:37",&base).unwrap();
        assert_eq!(list.items.len(), 2);
        assert!((list.items[0].duration_seconds.unwrap() - 150.493_333).abs() < 0.00001);
        assert_ne!(
            list.items[0].stable_identity(),
            list.items[1].stable_identity()
        );
    }
    #[test]
    fn live_folder_scan_is_a_snapshot_and_missing_reference_is_kept() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.mp3"), b"").unwrap();
        let reference = folder_reference(root.path()).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let first = resolve_folders(std::slice::from_ref(&reference), &cancel).unwrap();
        assert_eq!(first.len(), 1);
        std::fs::write(root.path().join("b.mp3"), b"").unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(resolve_folders(&[reference], &cancel).unwrap().len(), 2);
    }

    #[test]
    fn local_entries_keep_special_characters_and_detect_video() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("review");
        let base = Url::from_file_path(folder.join("list.m3u")).unwrap();
        let text = "music/Track#1.mp3\na?b.mp3\n100%.mp3\nmovie.mp4\nsub\\clip.MKV\nhttps://example.org/a.mp3";
        let list = parse(text, &base).unwrap();
        let path = |item: &MediaItem| item.local_path.clone().unwrap();
        let expected = |relative: &str| folder.join(relative).to_string_lossy().into_owned();
        assert_eq!(path(&list.items[0]), expected("music/Track#1.mp3"));
        assert_eq!(path(&list.items[1]), expected("a?b.mp3"));
        assert_eq!(path(&list.items[2]), expected("100%.mp3"));
        assert_eq!(list.items[0].title, "Track#1");
        assert_eq!(list.items[0].kind, MediaKind::Audio);
        assert_eq!(list.items[3].kind, MediaKind::Video);
        assert_eq!(path(&list.items[4]), expected("sub/clip.MKV"));
        assert_eq!(list.items[4].kind, MediaKind::Video);
        assert!(list.items[5].local_path.is_none());
        assert_eq!(
            list.items[5].url.as_ref().unwrap().as_str(),
            "https://example.org/a.mp3"
        );
    }
}
