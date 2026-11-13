//! IPTV source loading, playlist parsing, provider browsing and programme data.
//! Source credentials use the same OS protection as the rest of Apricot.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.

use std::{collections::BTreeMap, io::Read, path::Path, time::Duration};

use apricot_core::{MediaItem, MediaKind};
use chrono::DateTime;
use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

const MAX_BYTES: u64 = 24 * 1024 * 1024;
const CREDENTIAL_ENTROPY: &[u8] = b"ApricotPlayer IPTV credentials v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SourceKind {
    Playlist,
    Xtream,
    Stalker,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IptvSource {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    pub address: String,
    #[serde(default)]
    pub protected_credentials: String,
    #[serde(default)]
    pub epg_address: String,
    #[serde(default)]
    pub prefer_hls: bool,
}

impl IptvSource {
    pub fn set_credentials(
        &mut self,
        username: &str,
        password: &str,
        mac: &str,
    ) -> Result<(), String> {
        let bytes = serde_json::to_vec(
            &serde_json::json!({"username":username,"password":password,"mac":mac}),
        )
        .map_err(|_| "Unable to encode IPTV credentials.".to_owned())?;
        self.protected_credentials = crate::audiovault_credentials::protect_data(
            &bytes,
            CREDENTIAL_ENTROPY,
            "ApricotPlayer IPTV",
        )?;
        Ok(())
    }

    pub fn credentials(&self) -> Result<Value, String> {
        if self.protected_credentials.is_empty() {
            return Ok(serde_json::json!({}));
        }
        let bytes = crate::audiovault_credentials::unprotect_data(&self.protected_credentials, CREDENTIAL_ENTROPY).ok_or_else(|| "The saved IPTV login cannot be unlocked on this computer. Edit the source and sign in again.".to_owned())?;
        serde_json::from_slice(&bytes).map_err(|_| "The saved IPTV login is invalid.".to_owned())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ChannelKind {
    Live,
    Movie,
    Series,
    Episode,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct IptvChannel {
    pub id: String,
    pub name: String,
    pub group: String,
    pub number: String,
    pub epg_id: String,
    pub logo: String,
    pub url: String,
    pub kind: ChannelKind,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}

impl IptvChannel {
    #[must_use]
    pub fn media(&self, source: &IptvSource) -> Option<MediaItem> {
        let mut item = MediaItem::from_direct_link(&self.url)?;
        item.title.clone_from(&self.name);
        item.channel.clone_from(&self.group);
        item.kind = match self.kind {
            ChannelKind::Live => MediaKind::LiveStream,
            ChannelKind::Movie => MediaKind::Movie,
            ChannelKind::Series => MediaKind::TvShow,
            ChannelKind::Episode => MediaKind::TvEpisode,
        };
        item.metadata.extend(self.metadata.clone());
        item.metadata
            .insert("iptv_source_id".into(), source.id.clone().into());
        item.metadata
            .insert("iptv_channel_id".into(), self.id.clone().into());
        Some(item)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Programme {
    pub channel: String,
    pub title: String,
    pub description: String,
    pub start: i64,
    pub end: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EpgGuide {
    pub programmes: BTreeMap<String, Vec<Programme>>,
}

impl EpgGuide {
    #[must_use]
    pub fn now_next(&self, channel: &str, now: i64) -> (Option<&Programme>, Option<&Programme>) {
        let Some(items) = self.programmes.get(channel) else {
            return (None, None);
        };
        let current = items
            .iter()
            .find(|item| item.start <= now && now < item.end);
        let next = items.iter().find(|item| item.start > now);
        (current, next)
    }
}

pub fn parse_xmltv(bytes: &[u8]) -> Result<EpgGuide, String> {
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(MAX_BYTES + 1)
            .read_to_end(&mut decoded)
            .map_err(|_| "Unable to unpack the programme guide.".to_owned())?;
        if decoded.len() as u64 > MAX_BYTES {
            return Err("The programme guide is too large.".into());
        }
        decoded
    } else {
        bytes.to_vec()
    };
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The programme guide is too large.".into());
    }
    let text = apricot_media::decode_xml(&bytes)
        .map_err(|_| "The programme guide has invalid text encoding.".to_owned())?;
    let document = roxmltree::Document::parse(&text)
        .map_err(|_| "The programme guide is not valid XMLTV.".to_owned())?;
    let mut guide = EpgGuide::default();
    for node in document
        .descendants()
        .filter(|node| node.has_tag_name("programme"))
    {
        let (Some(channel), Some(start), Some(end)) = (
            node.attribute("channel"),
            node.attribute("start").and_then(xmltv_time),
            node.attribute("stop").and_then(xmltv_time),
        ) else {
            continue;
        };
        if end <= start {
            continue;
        }
        let content = |tag| {
            node.children()
                .find(|node| node.has_tag_name(tag))
                .and_then(|node| node.text())
                .unwrap_or_default()
                .trim()
                .to_owned()
        };
        guide
            .programmes
            .entry(channel.to_owned())
            .or_default()
            .push(Programme {
                channel: channel.to_owned(),
                title: content("title"),
                description: content("desc"),
                start,
                end,
            });
    }
    for items in guide.programmes.values_mut() {
        items.sort_by_key(|item| item.start)
    }
    Ok(guide)
}

fn xmltv_time(value: &str) -> Option<i64> {
    let value = value.trim();
    DateTime::parse_from_str(value, "%Y%m%d%H%M%S %z")
        .or_else(|_| DateTime::parse_from_str(&format!("{value} +0000"), "%Y%m%d%H%M%S %z"))
        .ok()
        .map(|time| time.timestamp())
}

/// Attribute parsing respects quotes, including commas and spaces inside a
/// channel title or group. Playlist rows retain their order and duplicates.
pub fn parse_m3u(text: &str, base: Option<&Url>) -> Result<Vec<IptvChannel>, String> {
    if text.len() as u64 > MAX_BYTES {
        return Err("The playlist is too large.".into());
    }
    let text = text.trim_start_matches('\u{feff}');
    if text.lines().any(|line| {
        line.starts_with("#EXT-X-TARGETDURATION") || line.starts_with("#EXT-X-STREAM-INF")
    }) {
        return Err("This address is an HLS media stream, not a channel playlist. Open it with Direct link.".into());
    }
    let mut channels = Vec::new();
    let mut info = String::new();
    let mut group = String::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(value) = line.strip_prefix("#EXTINF:") {
            info = value.to_owned();
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXTGRP:") {
            group = value.to_owned();
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let url = Url::parse(line)
            .ok()
            .or_else(|| base.and_then(|base| base.join(line).ok()));
        let Some(url) =
            url.filter(|url| matches!(url.scheme(), "http" | "https" | "rtsp" | "rtmp" | "mms"))
        else {
            continue;
        };
        let comma = unquoted_comma(&info);
        let attributes = parse_attributes(comma.map_or(info.as_str(), |index| &info[..index]));
        let name = comma
            .map(|index| info[index + 1..].trim())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| url.as_str())
            .to_owned();
        let get = |key: &str| attributes.get(key).cloned().unwrap_or_default();
        channels.push(IptvChannel {
            id: format!("{}:{}", channels.len(), get("tvg-id")),
            name,
            group: attributes
                .get("group-title")
                .cloned()
                .unwrap_or_else(|| group.clone()),
            number: get("tvg-chno"),
            epg_id: get("tvg-id"),
            logo: get("tvg-logo"),
            url: url.into(),
            kind: ChannelKind::Live,
            metadata: BTreeMap::new(),
        });
        info.clear();
        group.clear();
        if channels.len() > 150_000 {
            return Err("The playlist contains too many channels.".into());
        }
    }
    if channels.is_empty() {
        return Err("The playlist does not contain playable channels.".into());
    }
    Ok(channels)
}

fn unquoted_comma(text: &str) -> Option<usize> {
    let mut quoted = false;
    text.char_indices().find_map(|(index, character)| {
        if character == '"' {
            quoted = !quoted
        };
        (character == ',' && !quoted).then_some(index)
    })
}

fn parse_attributes(text: &str) -> BTreeMap<String, String> {
    let mut attributes = BTreeMap::new();
    let regex = regex::Regex::new(r#"([A-Za-z0-9_-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s]+))"#)
        .expect("constant attribute expression");
    for captures in regex.captures_iter(text) {
        attributes.insert(
            captures[1].to_ascii_lowercase(),
            captures
                .get(2)
                .or_else(|| captures.get(3))
                .or_else(|| captures.get(4))
                .map_or("", |value| value.as_str())
                .to_owned(),
        );
    }
    attributes
}

#[derive(Clone)]
pub struct IptvClient {
    client: Client,
}

impl IptvClient {
    pub fn new(proxy: Option<&str>) -> Result<Self, String> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(8))
            .redirect(Policy::limited(5))
            .user_agent(format!("ApricotPlayer/{}", env!("CARGO_PKG_VERSION")));
        if let Some(proxy) = proxy.filter(|value| !value.trim().is_empty()) {
            builder = builder
                .proxy(reqwest::Proxy::all(proxy).map_err(|_| "Invalid IPTV proxy.".to_owned())?)
        }
        Ok(Self {
            client: builder
                .build()
                .map_err(|_| "Unable to create the IPTV client.".to_owned())?,
        })
    }

    pub fn load(&self, source: &IptvSource) -> Result<Vec<IptvChannel>, String> {
        match source.kind {
            SourceKind::Playlist => {
                let path = Path::new(&source.address);
                let (bytes, base) = if path.is_file() {
                    let length = path
                        .metadata()
                        .map_err(|_| "Unable to read the playlist.".to_owned())?
                        .len();
                    if length > MAX_BYTES {
                        return Err("The playlist is too large.".into());
                    }
                    (
                        std::fs::read(path)
                            .map_err(|_| "Unable to read the playlist.".to_owned())?,
                        Url::from_file_path(path).ok(),
                    )
                } else {
                    let url = http_address(&source.address)?;
                    (self.bytes(url.clone(), &[])?, Some(url))
                };
                let text = decode_playlist(&bytes);
                parse_m3u(&text, base.as_ref())
            }
            SourceKind::Xtream => self.xtream(source),
            SourceKind::Stalker => self.stalker(source),
        }
    }

    pub fn epg(&self, address: &str) -> Result<EpgGuide, String> {
        let bytes = if Path::new(address).is_file() {
            let path = Path::new(address);
            if path
                .metadata()
                .map_err(|_| "Unable to read the programme guide.".to_owned())?
                .len()
                > MAX_BYTES
            {
                return Err("The programme guide is too large.".into());
            }
            std::fs::read(path).map_err(|_| "Unable to read the programme guide.".to_owned())?
        } else {
            self.bytes(http_address(address)?, &[])?
        };
        parse_xmltv(&bytes)
    }

    fn xtream_request(
        &self,
        source: &IptvSource,
        action: &str,
        extra: &[(&str, String)],
    ) -> Result<Value, String> {
        let login = source.credentials()?;
        let mut url = http_address(&source.address)?;
        if !url.path().ends_with("player_api.php") {
            url.set_path(&format!(
                "{}/player_api.php",
                url.path().trim_end_matches('/')
            ))
        }
        url.query_pairs_mut()
            .append_pair("username", login["username"].as_str().unwrap_or_default())
            .append_pair("password", login["password"].as_str().unwrap_or_default())
            .append_pair("action", action)
            .extend_pairs(extra.iter().map(|(key, value)| (*key, value.as_str())));
        self.json(url, &[])
    }

    fn xtream(&self, source: &IptvSource) -> Result<Vec<IptvChannel>, String> {
        let mut output = Vec::new();
        for (kind, categories, streams) in [
            (ChannelKind::Live, "get_live_categories", "get_live_streams"),
            (ChannelKind::Movie, "get_vod_categories", "get_vod_streams"),
            (ChannelKind::Series, "get_series_categories", "get_series"),
        ] {
            let categories = self.xtream_request(source, categories, &[])?;
            let names: BTreeMap<String, String> = categories
                .as_array()
                .ok_or("The IPTV provider returned invalid categories.")?
                .iter()
                .map(|item| {
                    (
                        value_text(&item["category_id"]),
                        value_text(&item["category_name"]),
                    )
                })
                .collect();
            let result = self.xtream_request(source, streams, &[])?;
            for item in result
                .as_array()
                .ok_or("The IPTV provider returned invalid channel data.")?
            {
                let id = value_text(
                    &item[if kind == ChannelKind::Series {
                        "series_id"
                    } else {
                        "stream_id"
                    }],
                );
                if id.is_empty() {
                    continue;
                }
                let extension = if kind == ChannelKind::Live {
                    if source.prefer_hls { "m3u8" } else { "ts" }
                } else {
                    item["container_extension"].as_str().unwrap_or("mp4")
                };
                let url = if kind == ChannelKind::Series {
                    String::new()
                } else {
                    provider_stream(source, kind, &id, extension)?
                };
                output.push(IptvChannel {
                    id: id.clone(),
                    name: value_text(&item["name"]),
                    group: names
                        .get(&value_text(&item["category_id"]))
                        .cloned()
                        .unwrap_or_default(),
                    number: value_text(&item["num"]),
                    epg_id: value_text(&item["epg_channel_id"]),
                    logo: value_text(&item["stream_icon"]),
                    url,
                    kind,
                    metadata: BTreeMap::from([("iptv_series_id".into(), id.into())]),
                });
            }
        }
        Ok(output)
    }

    pub fn series(&self, source: &IptvSource, series_id: &str) -> Result<Vec<IptvChannel>, String> {
        if source.kind != SourceKind::Xtream {
            return Err("This source does not provide a series catalog.".into());
        }
        let response = self.xtream_request(
            source,
            "get_series_info",
            &[("series_id", series_id.to_owned())],
        )?;
        let seasons = response["episodes"]
            .as_object()
            .ok_or("The IPTV provider did not return episodes.")?;
        let mut seasons: Vec<_> = seasons.iter().collect();
        seasons.sort_by_key(|(key, _)| key.parse::<u64>().unwrap_or(u64::MAX));
        let mut channels = Vec::new();
        for (season, episodes) in seasons {
            for episode in episodes
                .as_array()
                .ok_or("The IPTV provider returned invalid episode data.")?
            {
                let id = value_text(&episode["id"]);
                let extension = episode["container_extension"].as_str().unwrap_or("mp4");
                channels.push(IptvChannel {
                    id: id.clone(),
                    name: value_text(&episode["title"]),
                    group: format!("Season {season}"),
                    number: value_text(&episode["episode_num"]),
                    epg_id: String::new(),
                    logo: String::new(),
                    url: provider_stream(source, ChannelKind::Episode, &id, extension)?,
                    kind: ChannelKind::Episode,
                    metadata: BTreeMap::new(),
                });
            }
        }
        Ok(channels)
    }

    fn stalker(&self, source: &IptvSource) -> Result<Vec<IptvChannel>, String> {
        let credentials = source.credentials()?;
        let mac = credentials["mac"].as_str().unwrap_or_default();
        if mac.len() != 17
            || !mac
                .chars()
                .all(|value| value.is_ascii_hexdigit() || value == ':')
        {
            return Err("Enter a valid MAC address for this portal.".into());
        }
        let mut base = http_address(&source.address)?;
        if !base.path().ends_with("load.php") {
            base.set_path(&format!(
                "{}/server/load.php",
                base.path().trim_end_matches('/').trim_end_matches("/c")
            ))
        }
        let cookie = format!("mac={}; stb_lang=en; timezone=UTC", mac.replace(':', "%3A"));
        let request = |kind: &str,
                       action: &str,
                       token: Option<&str>,
                       extra: &[(&str, &str)]|
         -> Result<Value, String> {
            let mut url = base.clone();
            url.query_pairs_mut()
                .append_pair("type", kind)
                .append_pair("action", action)
                .append_pair("JsHttpRequest", "1-xml")
                .extend_pairs(extra.iter().copied());
            let mut headers = vec![("Cookie", cookie.as_str())];
            let auth = token.map(|token| format!("Bearer {token}"));
            if let Some(auth) = &auth {
                headers.push(("Authorization", auth))
            }
            self.json(url, &headers)
                .map(|value| value.get("js").cloned().unwrap_or(value))
        };
        let handshake = request("stb", "handshake", None, &[])?;
        let token = handshake["token"]
            .as_str()
            .ok_or("The IPTV portal did not accept the connection.")?;
        let _profile = request(
            "stb",
            "get_profile",
            Some(token),
            &[
                ("stb_type", "MAG254"),
                ("hd", "1"),
                ("auth_second_step", "1"),
            ],
        )?;
        let genres = request("itv", "get_genres", Some(token), &[])?;
        let names: BTreeMap<_, _> = genres
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|genre| (value_text(&genre["id"]), value_text(&genre["title"])))
            .collect();
        let data = request("itv", "get_all_channels", Some(token), &[])?;
        let values = data
            .get("data")
            .and_then(Value::as_array)
            .or_else(|| data.as_array())
            .ok_or("The IPTV portal returned invalid channels.")?;
        let mut channels = Vec::new();
        for item in values {
            let id = value_text(&item["id"]);
            let command = value_text(&item["cmd"]);
            let raw = command.strip_prefix("ffmpeg ").unwrap_or(&command);
            let mut metadata = BTreeMap::from([
                ("stalker_command".into(), command.clone().into()),
                ("stalker_token".into(), token.into()),
                ("stalker_cookie".into(), cookie.clone().into()),
                ("stalker_endpoint".into(), base.as_str().into()),
            ]);
            // Portal create_link is performed immediately before playback, not
            // while listing hundreds of channels with expiring URLs.
            metadata.insert(
                "http_headers".into(),
                serde_json::json!({"Cookie": cookie,"Authorization":format!("Bearer {token}")}),
            );
            channels.push(IptvChannel {
                id,
                name: value_text(&item["name"]),
                group: names
                    .get(&value_text(&item["tv_genre_id"]))
                    .cloned()
                    .unwrap_or_default(),
                number: value_text(&item["number"]),
                epg_id: value_text(&item["xmltv_id"]),
                logo: value_text(&item["logo"]),
                url: raw.into(),
                kind: ChannelKind::Live,
                metadata,
            });
        }
        Ok(channels)
    }

    pub fn resolve_channel(&self, channel: &IptvChannel) -> Result<IptvChannel, String> {
        let Some(endpoint) = channel
            .metadata
            .get("stalker_endpoint")
            .and_then(Value::as_str)
        else {
            return Ok(channel.clone());
        };
        let text = |key: &str| {
            channel
                .metadata
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        let mut url = http_address(endpoint)?;
        url.query_pairs_mut()
            .append_pair("type", "itv")
            .append_pair("action", "create_link")
            .append_pair("cmd", text("stalker_command"))
            .append_pair("JsHttpRequest", "1-xml");
        let auth = format!("Bearer {}", text("stalker_token"));
        let response = self.json(
            url,
            &[("Cookie", text("stalker_cookie")), ("Authorization", &auth)],
        )?;
        let command = response["js"]["cmd"]
            .as_str()
            .ok_or("The IPTV portal did not provide a playback link.")?;
        let mut result = channel.clone();
        result.url = command
            .strip_prefix("ffmpeg ")
            .unwrap_or(command)
            .to_owned();
        Ok(result)
    }

    fn json(&self, url: Url, headers: &[(&str, &str)]) -> Result<Value, String> {
        serde_json::from_slice(&self.bytes(url, headers)?)
            .map_err(|_| "The IPTV provider returned invalid JSON.".into())
    }

    fn bytes(&self, url: Url, headers: &[(&str, &str)]) -> Result<Vec<u8>, String> {
        let mut request = self.client.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value)
        }
        let response = request
            .send()
            .map_err(|_| "Unable to connect to the IPTV provider.".to_owned())?;
        if !response.status().is_success() {
            return Err(format!(
                "The IPTV provider returned HTTP {}.",
                response.status().as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BYTES)
        {
            return Err("The IPTV response is too large.".into());
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "The IPTV download was interrupted.".to_owned())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("The IPTV response is too large.".into());
        }
        Ok(bytes)
    }
}

fn provider_stream(
    source: &IptvSource,
    kind: ChannelKind,
    id: &str,
    extension: &str,
) -> Result<String, String> {
    if !id.chars().all(|value| value.is_ascii_alphanumeric())
        || !extension.chars().all(|value| value.is_ascii_alphanumeric())
    {
        return Err("The IPTV provider returned an invalid stream identity.".into());
    }
    let credentials = source.credentials()?;
    let mut url = http_address(&source.address)?;
    let path = url
        .path()
        .trim_end_matches("/player_api.php")
        .trim_end_matches('/')
        .to_owned();
    url.set_path(&path);
    url.set_query(None);
    let group = match kind {
        ChannelKind::Live => "live",
        ChannelKind::Movie => "movie",
        ChannelKind::Episode => "series",
        ChannelKind::Series => return Err("Choose an episode to play.".into()),
    };
    url.path_segments_mut()
        .map_err(|()| "Invalid IPTV server address.".to_owned())?
        .pop_if_empty()
        .push(group)
        .push(credentials["username"].as_str().unwrap_or_default())
        .push(credentials["password"].as_str().unwrap_or_default())
        .push(&format!("{id}.{extension}"));
    Ok(url.to_string())
}

fn http_address(value: &str) -> Result<Url, String> {
    Url::parse(value.trim())
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .ok_or_else(|| "Enter an HTTP or HTTPS IPTV address.".into())
}

fn decode_playlist(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let encoding = if bytes[0] == 0xff {
            encoding_rs::UTF_16LE
        } else {
            encoding_rs::UTF_16BE
        };
        return encoding.decode(&bytes[2..]).0.into_owned();
    }
    String::from_utf8(bytes.to_vec())
        .unwrap_or_else(|_| encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned())
}

fn value_text(value: &Value) -> String {
    value.as_str().map_or_else(
        || {
            if value.is_number() {
                value.to_string()
            } else {
                String::new()
            }
        },
        ToOwned::to_owned,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_keeps_quoted_commas_and_relative_urls() {
        let channels = parse_m3u("#EXTM3U\n#EXTINF:-1 tvg-id=\"channel\" group-title=\"News, local\",News One\nstreams/live.m3u8\n", Some(&Url::parse("https://example.test/list.m3u").unwrap())).unwrap();
        assert_eq!(channels[0].group, "News, local");
        assert_eq!(channels[0].url, "https://example.test/streams/live.m3u8");
        assert_eq!(channels[0].epg_id, "channel");
        assert!(parse_m3u("#EXTM3U\n#EXT-X-TARGETDURATION:10\npart.ts", None).is_err());
    }

    #[test]
    fn programme_boundaries_use_timezone_and_exclusive_end() {
        let guide = parse_xmltv(br#"<tv><programme channel="one" start="20261113090000 +0100" stop="20261113100000 +0100"><title>Morning</title><desc>News</desc></programme><programme channel="one" start="20261113100000 +0100" stop="20261113110000 +0100"><title>Next</title></programme></tv>"#).unwrap();
        let now = xmltv_time("20261113100000 +0100").unwrap();
        let (current, next) = guide.now_next("one", now);
        assert_eq!(current.unwrap().title, "Next");
        assert!(next.is_none());
        assert_eq!(
            DateTime::parse_from_str("20261113090000 +0100", "%Y%m%d%H%M%S %z")
                .unwrap()
                .timestamp(),
            xmltv_time("20261113080000 +0000").unwrap()
        );
    }
}
