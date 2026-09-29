//! `AudioVault` pages and items, Python `apricot/network/audiovault.py`
//! without the network: the page parser, the result and episode items, their
//! list rows and the cache and download locations.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource, TranslationCatalog};
use serde_json::Value;

pub const AUDIOVAULT_BASE_URL: &str = "https://direct.audiovault.net";

/// Python `audiovault_mode`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AudiovaultMode {
    #[default]
    Movies,
    Shows,
}

impl AudiovaultMode {
    const fn catalog(self) -> &'static str {
        match self {
            Self::Movies => "movies",
            Self::Shows => "shows",
        }
    }

    /// Python's section heading of the recently added titles.
    pub const fn recent_section(self) -> &'static str {
        match self {
            Self::Movies => "recent movies",
            Self::Shows => "recent shows",
        }
    }

    pub const fn recent_title_key(self) -> &'static str {
        match self {
            Self::Movies => "audiovault_recent_movies",
            Self::Shows => "audiovault_recent_tv_shows",
        }
    }
}

/// One table row of an `AudioVault` page, Python `_VaultPageParser.records`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VaultRecord {
    pub section: String,
    pub row: Vec<String>,
    pub link: String,
}

/// Python `_VaultPageParser`: the login token and the table rows with the
/// heading they follow.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VaultPage {
    pub token: String,
    pub records: Vec<VaultRecord>,
}

#[derive(Default)]
struct PageParser {
    page: VaultPage,
    section: String,
    row: Option<Vec<String>>,
    cell: Option<String>,
    row_links: Vec<String>,
    heading: Option<String>,
}

impl PageParser {
    fn start_tag(&mut self, tag: &str, attributes: &[(String, Option<String>)]) {
        let value = |name: &str| {
            attributes
                .iter()
                .find(|(attribute, _)| attribute == name)
                .and_then(|(_, value)| value.clone())
        };
        match tag {
            "tr" => {
                self.row = Some(Vec::new());
                self.row_links.clear();
            }
            "td" if self.row.is_some() => self.cell = Some(String::new()),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.heading = Some(String::new()),
            "a" => {
                if let Some(href) = value("href").filter(|href| !href.is_empty())
                    && self.row.is_some()
                {
                    self.row_links.push(href);
                }
            }
            "input" if value("name").as_deref() == Some("_token") => {
                self.page.token = value("value").unwrap_or_default();
            }
            _ => {}
        }
    }

    fn data(&mut self, text: &str) {
        if let Some(cell) = &mut self.cell {
            cell.push_str(text);
        }
        if let Some(heading) = &mut self.heading {
            heading.push_str(text);
        }
    }

    fn end_tag(&mut self, tag: &str) {
        match tag {
            "td" if self.row.is_some() && self.cell.is_some() => {
                let cell = self.cell.take().unwrap_or_default();
                if let Some(row) = &mut self.row {
                    row.push(collapse_whitespace(&cell));
                }
            }
            "tr" if self.row.is_some() => {
                let row = self.row.take().unwrap_or_default();
                if !row.is_empty() {
                    let link = self
                        .row_links
                        .iter()
                        .find(|link| has_download_path(link))
                        .cloned()
                        .unwrap_or_default();
                    self.page.records.push(VaultRecord {
                        section: self.section.clone(),
                        row,
                        link,
                    });
                }
                self.row_links.clear();
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if self.heading.is_some() => {
                let heading = self.heading.take().unwrap_or_default();
                self.section = collapse_whitespace(&heading)
                    .trim_end_matches(':')
                    .to_lowercase();
            }
            _ => {}
        }
    }
}

/// Python `" ".join(text.split())`.
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Python `re.search(r"/download/\d+", link)`.
fn has_download_path(link: &str) -> bool {
    link.match_indices("/download/").any(|(index, matched)| {
        link[index + matched.len()..]
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    })
}

fn unescape(text: &str) -> String {
    html_escape::decode_html_entities(text).into_owned()
}

/// Python `_VaultPageParser.feed` with `convert_charrefs=True`: tags and
/// attributes are lowercased, entities in text and attribute values decoded,
/// comments and declarations skipped, and script and style text kept as is.
pub fn parse_vault_page(html: &str) -> VaultPage {
    let mut parser = PageParser::default();
    let mut index = 0;
    let lower = html.to_ascii_lowercase();
    while index < html.len() {
        let Some(offset) = html[index..].find('<') else {
            parser.data(&unescape(&html[index..]));
            break;
        };
        let start = index + offset;
        if start > index {
            parser.data(&unescape(&html[index..start]));
        }
        let rest = &html[start..];
        if rest.starts_with("<!--") {
            index = rest.find("-->").map_or(html.len(), |end| start + end + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            index = rest.find('>').map_or(html.len(), |end| start + end + 1);
            continue;
        }
        let closing = rest.starts_with("</");
        let name_start = start + if closing { 2 } else { 1 };
        let name_end = html[name_start..]
            .find(|character: char| {
                character.is_whitespace() || character == '>' || character == '/'
            })
            .map_or(html.len(), |end| name_start + end);
        let name = lower[name_start..name_end].to_owned();
        if name.is_empty() || !name.starts_with(|character: char| character.is_ascii_alphabetic()) {
            // Python passes a lone "<" through as text.
            parser.data("<");
            index = start + 1;
            continue;
        }
        let tag_end = tag_end(html, name_end);
        let inner_end = if tag_end > 0 && html.as_bytes()[tag_end - 1] == b'>' {
            tag_end - 1
        } else {
            tag_end
        };
        if closing {
            parser.end_tag(&name);
            index = tag_end;
            continue;
        }
        let attribute_text = &html[name_end..inner_end];
        let self_closing = attribute_text.trim_end().ends_with('/');
        let attributes = parse_attributes(attribute_text.trim_end().trim_end_matches('/'));
        parser.start_tag(&name, &attributes);
        if self_closing {
            parser.end_tag(&name);
        }
        index = tag_end;
        if matches!(name.as_str(), "script" | "style") {
            let closing_tag = format!("</{name}");
            let text_end = lower[index..]
                .find(&closing_tag)
                .map_or(html.len(), |end| index + end);
            parser.data(&html[index..text_end]);
            index = text_end;
        }
    }
    parser.page
}

/// The byte after the `>` that ends the tag, skipping quoted values.
fn tag_end(html: &str, from: usize) -> usize {
    let bytes = html.as_bytes();
    let mut quote = None;
    let mut index = from;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(open) if byte == open => quote = None,
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'>' => return index + 1,
            Some(_) | None => {}
        }
        index += 1;
    }
    bytes.len()
}

fn parse_attributes(text: &str) -> Vec<(String, Option<String>)> {
    let mut attributes = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/') {
            index += 1;
        }
        let name_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && !matches!(bytes[index], b'=' | b'/')
        {
            index += 1;
        }
        if name_start == index {
            index += 1;
            continue;
        }
        let name = text[name_start..index].to_ascii_lowercase();
        let mut cursor = index;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == b'=' {
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            let (value, next) = if cursor < bytes.len() && matches!(bytes[cursor], b'"' | b'\'') {
                let quote = bytes[cursor];
                let value_start = cursor + 1;
                let value_end = bytes[value_start..]
                    .iter()
                    .position(|byte| *byte == quote)
                    .map_or(bytes.len(), |end| value_start + end);
                (
                    &text[value_start..value_end],
                    (value_end + 1).min(bytes.len()),
                )
            } else {
                let value_end = bytes[cursor..]
                    .iter()
                    .position(u8::is_ascii_whitespace)
                    .map_or(bytes.len(), |end| cursor + end);
                (&text[cursor..value_end], value_end)
            };
            attributes.push((name, Some(unescape(value))));
            index = next;
        } else {
            attributes.push((name, None));
        }
    }
    attributes
}

/// Python `audiovault_catalog_url`.
pub fn catalog_url(mode: AudiovaultMode, query: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("search", query)
        .finish();
    format!("{AUDIOVAULT_BASE_URL}/{}?{query}", mode.catalog())
}

/// Python `audiovault_results_from_records`.
pub fn results_from_records(
    records: &[VaultRecord],
    mode: AudiovaultMode,
    section: Option<&str>,
    catalog: &TranslationCatalog,
) -> Vec<MediaItem> {
    let Ok(base) = url::Url::parse(AUDIOVAULT_BASE_URL) else {
        return Vec::new();
    };
    records
        .iter()
        .filter(|record| section.is_none_or(|section| record.section == section))
        .filter(|record| record.row.len() >= 2 && has_download_path(&record.link))
        .filter_map(|record| {
            let url = base.join(&record.link).ok()?;
            let (kind, python_kind, type_key) = match mode {
                AudiovaultMode::Shows => (MediaKind::TvShow, "audiovault_show", "tv_show"),
                AudiovaultMode::Movies => (MediaKind::Movie, "audiovault_movie", "movie"),
            };
            let id = record.row[0].clone();
            let mut metadata = BTreeMap::new();
            metadata.insert("id".to_owned(), Value::String(id.clone()));
            metadata.insert("kind".to_owned(), Value::String(python_kind.to_owned()));
            metadata.insert(
                "type".to_owned(),
                Value::String(catalog.text(type_key).to_owned()),
            );
            metadata.insert("webpage_url".to_owned(), Value::String(url.to_string()));
            Some(MediaItem {
                id: MediaId(id),
                source: MediaSource::Audiovault,
                kind,
                title: unescape(&record.row[1]),
                url: Some(url),
                stream_url: None,
                external_audio_url: None,
                local_path: None,
                channel: "AudioVault".to_owned(),
                duration_seconds: None,
                metadata,
            })
        })
        .collect()
}

/// Python's `AudioVault` item kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudiovaultKind {
    Movie,
    Show,
    /// An episode still inside the remote package.
    RemoteEpisode,
    /// An episode in the cache folder.
    Episode,
}

/// Python `item.get("kind")` of an `AudioVault` item.
pub fn item_kind(item: &MediaItem) -> Option<AudiovaultKind> {
    if item.source != MediaSource::Audiovault {
        return None;
    }
    let kind = item
        .metadata
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(match kind {
        "audiovault_show" | "audiovault_tv_show" => AudiovaultKind::Show,
        "audiovault_remote_episode" => AudiovaultKind::RemoteEpisode,
        "audiovault_episode" => AudiovaultKind::Episode,
        "audiovault_movie" => AudiovaultKind::Movie,
        _ => match item.kind {
            MediaKind::TvShow => AudiovaultKind::Show,
            MediaKind::TvEpisode if item.metadata.contains_key("archive_member") => {
                AudiovaultKind::RemoteEpisode
            }
            MediaKind::TvEpisode => AudiovaultKind::Episode,
            _ => AudiovaultKind::Movie,
        },
    })
}

/// Python `result_line` for `AudioVault` items: title and type.
pub fn result_line(item: &MediaItem, catalog: &TranslationCatalog) -> String {
    let type_label = item
        .metadata
        .get("type")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| catalog.text("video"));
    [item.title.as_str(), type_label]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Python `safe_folder_name`.
pub fn safe_folder_name(value: &str) -> String {
    let replaced = value
        .trim()
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) || u32::from(character) < 0x20
            {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut cleaned = replaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches([' ', '.'])
        .to_owned();
    let stem = cleaned.split('.').next().unwrap_or_default().to_lowercase();
    let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || stem
            .strip_prefix("com")
            .or_else(|| stem.strip_prefix("lpt"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            });
    if reserved {
        cleaned.insert(0, '_');
    }
    let cleaned = cleaned.chars().take(150).collect::<String>();
    if cleaned.is_empty() {
        "Download".to_owned()
    } else {
        cleaned
    }
}

/// Python `audiovault_show_manifest_key`.
pub fn show_key(show: &MediaItem) -> String {
    if show.id.0.is_empty() {
        show.url
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
    } else {
        show.id.0.clone()
    }
}

/// Python `audiovault_show_cache_dir`.
pub fn show_cache_dir(cache_folder: &str, show: &MediaItem) -> PathBuf {
    let id = if show.id.0.is_empty() {
        "show"
    } else {
        &show.id.0
    };
    let safe_id = id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    Path::new(cache_folder).join("audiovault").join(safe_id)
}

/// The last part of a member name and its stem and lowercase suffix, as
/// `pathlib.Path` on Windows splits them.
fn name_parts(member: &str) -> (String, String) {
    let name = member.rsplit(['/', '\\']).next().unwrap_or_default();
    match name.rfind('.') {
        Some(index) if index > 0 && index + 1 < name.len() => {
            (name[..index].to_owned(), name[index..].to_lowercase())
        }
        _ => (name.to_owned(), String::new()),
    }
}

/// Python `audiovault_remote_episode_cache_path`.
pub fn remote_episode_cache_path(show_cache_dir: &Path, member: &str, index: usize) -> PathBuf {
    let normalized = member.replace('\\', "/");
    let (stem, suffix) = name_parts(&normalized);
    let safe_stem = safe_folder_name(&stem)
        .chars()
        .take(120)
        .collect::<String>();
    show_cache_dir
        .join("_episodes")
        .join(format!("{:04} - {safe_stem}{suffix}", index + 1))
}

/// Archive fields of one member, Python's `ZipInfo`.
pub struct ArchiveMember<'a> {
    pub name: &'a str,
    pub crc: u32,
    pub file_size: u64,
    pub compress_size: u64,
}

fn episode_base(
    show: &MediaItem,
    title: String,
    path: &Path,
    python_kind: &str,
    catalog: &TranslationCatalog,
) -> MediaItem {
    let path_text = path.to_string_lossy().into_owned();
    let mut metadata = BTreeMap::new();
    metadata.insert("kind".to_owned(), Value::String(python_kind.to_owned()));
    metadata.insert(
        "type".to_owned(),
        Value::String(catalog.text("episode").to_owned()),
    );
    metadata.insert("path".to_owned(), Value::String(path_text.clone()));
    metadata.insert("local_path".to_owned(), Value::String(path_text.clone()));
    metadata.insert("audiovault_show".to_owned(), show_value(show));
    MediaItem {
        id: MediaId(path_text.clone()),
        source: MediaSource::Audiovault,
        kind: MediaKind::TvEpisode,
        title,
        url: None,
        stream_url: None,
        external_audio_url: None,
        local_path: Some(path_text),
        channel: if show.title.is_empty() {
            "AudioVault".to_owned()
        } else {
            show.title.clone()
        },
        duration_seconds: None,
        metadata,
    }
}

/// The show a Python episode keeps in `audiovault_show`.
fn show_value(show: &MediaItem) -> Value {
    let mut value = serde_json::Map::new();
    for (key, entry) in &show.metadata {
        value.insert(key.clone(), entry.clone());
    }
    value.insert("id".to_owned(), Value::String(show.id.0.clone()));
    value.insert("title".to_owned(), Value::String(show.title.clone()));
    if let Some(url) = &show.url {
        value.insert("url".to_owned(), Value::String(url.to_string()));
    }
    value.insert("channel".to_owned(), Value::String(show.channel.clone()));
    Value::Object(value)
}

/// The show an episode came from, Python `item.get("audiovault_show")`.
pub fn episode_show(item: &MediaItem) -> Option<MediaItem> {
    let object = item.metadata.get("audiovault_show")?.as_object()?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Some(MediaItem {
        id: MediaId(text("id")),
        source: MediaSource::Audiovault,
        kind: MediaKind::TvShow,
        title: text("title"),
        url: text("url").parse().ok(),
        stream_url: None,
        external_audio_url: None,
        local_path: None,
        channel: text("channel"),
        duration_seconds: None,
        metadata: object
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    })
}

/// Python `audiovault_remote_episode_items`, one item per audio member in
/// the order the caller sorted them.
pub fn remote_episode_items(
    show: &MediaItem,
    show_cache_dir: &Path,
    archive_url: &str,
    archive_size: u64,
    members: &[ArchiveMember<'_>],
    catalog: &TranslationCatalog,
) -> Vec<MediaItem> {
    members
        .iter()
        .enumerate()
        .map(|(index, member)| {
            let target = remote_episode_cache_path(show_cache_dir, member.name, index);
            let (stem, _) = name_parts(member.name);
            let mut item = episode_base(show, stem, &target, "audiovault_remote_episode", catalog);
            let metadata = &mut item.metadata;
            metadata.insert(
                "webpage_url".to_owned(),
                Value::String(archive_url.to_owned()),
            );
            metadata.insert(
                "archive_url".to_owned(),
                Value::String(archive_url.to_owned()),
            );
            metadata.insert("archive_size".to_owned(), Value::from(archive_size));
            metadata.insert(
                "archive_member".to_owned(),
                Value::String(member.name.to_owned()),
            );
            metadata.insert("archive_crc".to_owned(), Value::from(member.crc));
            metadata.insert(
                "archive_file_size".to_owned(),
                Value::from(member.file_size),
            );
            metadata.insert(
                "archive_compress_size".to_owned(),
                Value::from(member.compress_size),
            );
            item
        })
        .collect()
}

/// Python `audiovault_episode_items` for the cached files, already sorted.
pub fn local_episode_items(
    show: &MediaItem,
    files: &[PathBuf],
    catalog: &TranslationCatalog,
) -> Vec<MediaItem> {
    files
        .iter()
        .map(|path| {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            episode_base(show, stem, path, "audiovault_episode", catalog)
        })
        .collect()
}

/// Python `finish_audiovault_remote_episode`: the cached episode plays as a
/// local one.
pub fn cached_episode(item: &MediaItem) -> MediaItem {
    let mut playable = item.clone();
    playable.metadata.insert(
        "kind".to_owned(),
        Value::String("audiovault_episode".to_owned()),
    );
    playable
}

pub fn metadata_text(item: &MediaItem, key: &str) -> String {
    item.metadata
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

pub fn metadata_u64(item: &MediaItem, key: &str) -> u64 {
    item.metadata
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or_default()
}

/// Python `download_folder_for_item` for `AudioVault` kinds.
pub fn download_folder(download_folder: &str, item: &MediaItem, collection: bool) -> PathBuf {
    let folder = Path::new(download_folder).join("AudioVault");
    let kind = item_kind(item);
    if kind == Some(AudiovaultKind::Show) || collection {
        let title = if item.title.is_empty() {
            "TV Show"
        } else {
            &item.title
        };
        return folder.join(safe_folder_name(title));
    }
    if matches!(
        kind,
        Some(AudiovaultKind::Episode | AudiovaultKind::RemoteEpisode)
    ) {
        let show_title = if item.channel.is_empty() {
            episode_show(item)
                .map(|show| show.title)
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| "TV Shows".to_owned())
        } else {
            item.channel.clone()
        };
        return folder.join(safe_folder_name(&show_title));
    }
    folder
}

/// Python `copy_audiovault_episode_to_downloads` without a chosen path.
pub fn episode_download_target(download_folder: &str, item: &MediaItem) -> PathBuf {
    let channel = if item.channel.is_empty() {
        "TV Shows"
    } else {
        &item.channel
    };
    let name = item
        .local_path
        .as_deref()
        .map(Path::new)
        .and_then(Path::file_name)
        .map_or_else(|| "episode.mp3".into(), ToOwned::to_owned);
    Path::new(download_folder)
        .join("AudioVault")
        .join(safe_folder_name(channel))
        .join(name)
}

/// Python `copy_audiovault_show_to_downloads` without a chosen folder.
pub fn show_download_target(download_folder: &str, show: &MediaItem) -> PathBuf {
    let title = if show.title.is_empty() {
        "TV Show"
    } else {
        &show.title
    };
    Path::new(download_folder)
        .join("AudioVault")
        .join(safe_folder_name(title))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use apricot_core::{MediaKind, MediaSource};

    use super::{
        ArchiveMember, AudiovaultKind, AudiovaultMode, catalog_url, download_folder,
        episode_download_target, episode_show, item_kind, local_episode_items, parse_vault_page,
        remote_episode_cache_path, remote_episode_items, result_line, results_from_records,
        safe_folder_name, show_cache_dir,
    };
    use crate::english_catalog;

    const HOME: &str = r#"<!DOCTYPE html><html><head><script>var x = "<td>no</td>";</script></head>
<body><form><input type="hidden" name="_token" value="tok&amp;en"></form>
<h2>Recent Movies:</h2><table>
<tr><th>ID</th><th>Name</th></tr>
<tr><td>101</td><td>  The   Movie &amp;amp; Friends </td><td><a href="/movies/101">Info</a> <A HREF="/download/101">Download</A></td></tr>
<tr><td>102</td><td>No link</td></tr>
</table>
<h3>Recent  Shows</h3><table>
<tr><td>7</td><td>A Show</td><td><a href='https://direct.audiovault.net/download/7'>Download</a></td></tr>
</table><!-- <tr><td>9</td><td>Hidden</td><td><a href="/download/9">x</a></td></tr> --></body></html>"#;

    #[test]
    fn parses_pages_like_python_html_parser() {
        let page = parse_vault_page(HOME);
        assert_eq!(page.token, "tok&en");
        assert_eq!(page.records.len(), 3);
        assert_eq!(page.records[0].section, "recent movies");
        assert_eq!(
            page.records[0].row,
            ["101", "The Movie &amp; Friends", "Info Download"]
        );
        assert_eq!(page.records[0].link, "/download/101");
        assert_eq!(page.records[1].link, "");
        assert_eq!(page.records[2].section, "recent shows");
    }

    #[test]
    fn results_keep_python_fields() {
        let catalog = english_catalog();
        let page = parse_vault_page(HOME);
        let movies = results_from_records(
            &page.records,
            AudiovaultMode::Movies,
            Some(AudiovaultMode::Movies.recent_section()),
            &catalog,
        );
        assert_eq!(movies.len(), 1);
        let movie = &movies[0];
        assert_eq!(movie.title, "The Movie & Friends");
        assert_eq!(movie.id.0, "101");
        assert_eq!(
            movie.url.as_ref().unwrap().as_str(),
            "https://direct.audiovault.net/download/101"
        );
        assert_eq!(movie.source, MediaSource::Audiovault);
        assert_eq!(movie.kind, MediaKind::Movie);
        assert_eq!(item_kind(movie), Some(AudiovaultKind::Movie));
        assert_eq!(result_line(movie, &catalog), "The Movie & Friends | Movie");
        let shows = results_from_records(&page.records, AudiovaultMode::Shows, None, &catalog);
        assert_eq!(shows.len(), 2);
        assert_eq!(item_kind(&shows[1]), Some(AudiovaultKind::Show));
        assert_eq!(result_line(&shows[1], &catalog), "A Show | TV show");
    }

    #[test]
    fn builds_python_addresses_and_names() {
        assert_eq!(
            catalog_url(AudiovaultMode::Shows, "star trek & co"),
            "https://direct.audiovault.net/shows?search=star+trek+%26+co"
        );
        assert_eq!(safe_folder_name("  a:b?  c.  "), "a b c");
        assert_eq!(safe_folder_name("con.mp3"), "_con.mp3");
        assert_eq!(safe_folder_name("..."), "Download");
        let catalog = english_catalog();
        let page = parse_vault_page(HOME);
        let show =
            results_from_records(&page.records, AudiovaultMode::Shows, None, &catalog).remove(1);
        let cache = show_cache_dir(r"C:\cache", &show);
        assert_eq!(cache, Path::new(r"C:\cache\audiovault\7"));
        assert_eq!(
            remote_episode_cache_path(&cache, r"Season 1\Ep: 1.MP3", 0),
            Path::new(r"C:\cache\audiovault\7\_episodes\0001 - Ep 1.mp3")
        );
        let episodes = remote_episode_items(
            &show,
            &cache,
            "https://direct.audiovault.net/download/7",
            1000,
            &[ArchiveMember {
                name: "Season 1/Pilot.mp3",
                crc: 5,
                file_size: 10,
                compress_size: 9,
            }],
            &catalog,
        );
        let episode = &episodes[0];
        assert_eq!(episode.title, "Pilot");
        assert_eq!(episode.channel, "A Show");
        assert_eq!(item_kind(episode), Some(AudiovaultKind::RemoteEpisode));
        assert_eq!(result_line(episode, &catalog), "Pilot | Episode");
        assert_eq!(episode_show(episode).unwrap().id.0, "7");
        assert_eq!(
            episode_download_target(r"D:\Downloads", episode),
            Path::new(r"D:\Downloads\AudioVault\A Show\0001 - Pilot.mp3")
        );
        assert_eq!(
            download_folder(r"D:\Downloads", &show, false),
            Path::new(r"D:\Downloads\AudioVault\A Show")
        );
        let local = local_episode_items(
            &show,
            &[cache.join("Season 1").join("02 Two.mp3")],
            &catalog,
        );
        assert_eq!(item_kind(&local[0]), Some(AudiovaultKind::Episode));
        assert_eq!(local[0].title, "02 Two");
    }
}
