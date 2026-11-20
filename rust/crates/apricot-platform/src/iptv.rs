//! IPTV source loading, playlist parsing, provider browsing and programme data.
//! Source credentials use the same OS protection as the rest of Apricot.

use std::{collections::BTreeMap, io::Read, path::Path, time::Duration};

use apricot_core::{MediaItem, MediaKind};
use chrono::DateTime;
use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

const MAX_BYTES: u64 = 24 * 1024 * 1024;
const CREDENTIAL_ENTROPY: &[u8] = b"ApricotPlayer IPTV credentials v1";

/// Why an IPTV source could not be used; the interface words each case.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IptvError {
    /// The login could not be protected for saving.
    CredentialsSave,
    /// The saved login cannot be unlocked on this computer.
    CredentialsLocked,
    PlaylistUnreadable,
    PlaylistTooLarge,
    /// The address is one HLS stream, not a list of channels.
    HlsStream,
    TooManyChannels,
    NoChannels,
    GuideUnreadable,
    GuideTooLarge,
    GuideInvalid,
    Proxy,
    Connect,
    Http(u16),
    ResponseTooLarge,
    /// The provider sent data that is cut off or not what was asked for.
    InvalidData,
    NoSeries,
    ChooseEpisode,
    InvalidMac,
    PortalRefused,
    NoPlaybackLink,
    NeedHttpAddress,
    /// A saved channel is no longer offered by its source.
    ChannelGone,
}

fn too_large(length: usize) -> bool {
    u64::try_from(length).unwrap_or(u64::MAX) > MAX_BYTES
}

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
    /// A guide address with a login in it, kept with the system's
    /// credential protection; `epg_address` then only shows where it goes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub protected_epg: String,
    #[serde(default)]
    pub prefer_hls: bool,
}

/// Where a protected address goes, without its login: `https://host/…`.
fn masked_address(address: &str) -> String {
    Url::parse(address).map_or_else(
        |_| String::new(),
        |url| {
            format!(
                "{}://{}/…",
                url.scheme(),
                url.host_str().unwrap_or_default()
            )
        },
    )
}

/// A web address that carries a login or a token: a user name or password,
/// a query (`?token=`, `?username=`), or the Xtream form
/// `/live/USER/PASS/123.ts`. Such addresses never go into history.
#[must_use]
pub fn address_is_private(address: &str) -> bool {
    apricot_core::url_carries_login(address)
}

impl IptvSource {
    /// Protects the login with the system's credential protection.
    ///
    /// # Errors
    ///
    /// Returns an error when the system cannot protect it.
    pub fn set_credentials(
        &mut self,
        username: &str,
        password: &str,
        mac: &str,
    ) -> Result<(), IptvError> {
        let bytes = serde_json::to_vec(
            &serde_json::json!({"username":username,"password":password,"mac":mac}),
        )
        .map_err(|_| IptvError::CredentialsSave)?;
        self.protected_credentials = crate::audiovault_credentials::protect_data(
            &bytes,
            CREDENTIAL_ENTROPY,
            "ApricotPlayer IPTV",
        )
        .map_err(|_| IptvError::CredentialsSave)?;
        Ok(())
    }

    /// Sets a playlist's address. One that carries a login or a token
    /// (review 2.1) is kept with the system's credential protection, and
    /// the settings file only shows where it goes.
    ///
    /// # Errors
    ///
    /// Returns an error when the system cannot protect it.
    pub fn set_playlist_address(&mut self, address: &str) -> Result<(), IptvError> {
        if !address_is_private(address) {
            address.clone_into(&mut self.address);
            self.protected_credentials.clear();
            return Ok(());
        }
        let bytes = serde_json::to_vec(&serde_json::json!({ "address": address }))
            .map_err(|_| IptvError::CredentialsSave)?;
        self.protected_credentials = crate::audiovault_credentials::protect_data(
            &bytes,
            CREDENTIAL_ENTROPY,
            "ApricotPlayer IPTV",
        )
        .map_err(|_| IptvError::CredentialsSave)?;
        self.address = masked_address(address);
        Ok(())
    }

    /// Sets the guide address; one with a login or a token is protected like
    /// the playlist address (review 2.1).
    ///
    /// # Errors
    ///
    /// Returns an error when the system cannot protect it.
    pub fn set_epg_address(&mut self, address: &str) -> Result<(), IptvError> {
        let address = address.trim();
        if !address_is_private(address) {
            address.clone_into(&mut self.epg_address);
            self.protected_epg.clear();
            return Ok(());
        }
        self.protected_epg = crate::audiovault_credentials::protect_data(
            address.as_bytes(),
            CREDENTIAL_ENTROPY,
            "ApricotPlayer IPTV",
        )
        .map_err(|_| IptvError::CredentialsSave)?;
        self.epg_address = masked_address(address);
        Ok(())
    }

    /// The address the guide is read from, unprotected when needed.
    #[must_use]
    pub fn epg_url(&self) -> String {
        if self.protected_epg.is_empty() {
            return self.epg_address.clone();
        }
        crate::audiovault_credentials::unprotect_data(&self.protected_epg, CREDENTIAL_ENTROPY)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default()
    }

    /// The address a playlist is read from, unprotected when needed.
    #[must_use]
    pub fn playlist_address(&self) -> String {
        self.credentials()
            .ok()
            .and_then(|value| value["address"].as_str().map(str::to_owned))
            .unwrap_or_else(|| self.address.clone())
    }

    /// The saved login, empty for a playlist source.
    ///
    /// # Errors
    ///
    /// Returns an error when the login cannot be unlocked on this computer.
    pub fn credentials(&self) -> Result<Value, IptvError> {
        if self.protected_credentials.is_empty() {
            return Ok(serde_json::json!({}));
        }
        let bytes = crate::audiovault_credentials::unprotect_data(
            &self.protected_credentials,
            CREDENTIAL_ENTROPY,
        )
        .ok_or(IptvError::CredentialsLocked)?;
        serde_json::from_slice(&bytes).map_err(|_| IptvError::CredentialsLocked)
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
        if !self.epg_id.is_empty() {
            item.metadata
                .insert("iptv_epg_id".into(), self.epg_id.clone().into());
        }
        // Provider addresses carry the login, so they stay out of history
        // and resume positions; so do playlist entries with a login or a
        // token in the address, and every entry of a protected playlist.
        if source.kind != SourceKind::Playlist
            || !source.protected_credentials.is_empty()
            || address_is_private(&self.url)
        {
            item.metadata
                .insert("apricot_private_address".into(), true.into());
            // The same id as the saved reference, so a channel kept in a
            // list is found again when it plays (review 4, A1).
            item.id = apricot_core::MediaId(apricot_core::iptv_reference_id(&source.id, &self.id));
            if item.title.contains("://") {
                item.title.clone_from(&item.id.0);
            }
        }
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

/// Reads an XMLTV guide, also gzip compressed.
///
/// # Errors
///
/// Returns an error when the guide is too large or not XMLTV.
pub fn parse_xmltv(bytes: &[u8]) -> Result<EpgGuide, IptvError> {
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .take(MAX_BYTES + 1)
            .read_to_end(&mut decoded)
            .map_err(|_| IptvError::GuideInvalid)?;
        if too_large(decoded.len()) {
            return Err(IptvError::GuideTooLarge);
        }
        decoded
    } else {
        bytes.to_vec()
    };
    if too_large(bytes.len()) {
        return Err(IptvError::GuideTooLarge);
    }
    let text = apricot_media::decode_xml(&bytes).map_err(|_| IptvError::GuideInvalid)?;
    let document = roxmltree::Document::parse(&text).map_err(|_| IptvError::GuideInvalid)?;
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
        items.sort_by_key(|item| item.start);
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
///
/// # Errors
///
/// Returns an error for an HLS stream or a list without playable channels.
pub fn parse_m3u(text: &str, base: Option<&Url>) -> Result<Vec<IptvChannel>, IptvError> {
    if too_large(text.len()) {
        return Err(IptvError::PlaylistTooLarge);
    }
    let text = text.trim_start_matches('\u{feff}');
    if text.lines().any(|line| {
        line.starts_with("#EXT-X-TARGETDURATION") || line.starts_with("#EXT-X-STREAM-INF")
    }) {
        return Err(IptvError::HlsStream);
    }
    let mut channels = Vec::new();
    let mut seen = BTreeMap::<String, usize>::new();
    let mut info = String::new();
    let mut group = String::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(value) = line.strip_prefix("#EXTINF:") {
            value.clone_into(&mut info);
            continue;
        }
        if let Some(value) = line.strip_prefix("#EXTGRP:") {
            value.clone_into(&mut group);
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
        // A lasting id within the playlist: its guide id, or its name, with
        // the occurrence for a repeat; not its row, which moves when the
        // provider adds a channel above it (review 5, A2).
        let base = if get("tvg-id").is_empty() {
            format!("name:{name}")
        } else {
            format!("tvg:{}", get("tvg-id"))
        };
        let occurrence = seen.entry(base.clone()).or_insert(0_usize);
        *occurrence += 1;
        let id = if *occurrence == 1 {
            base
        } else {
            format!("{base}#{occurrence}")
        };
        channels.push(IptvChannel {
            id,
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
            return Err(IptvError::TooManyChannels);
        }
    }
    if channels.is_empty() {
        return Err(IptvError::NoChannels);
    }
    Ok(channels)
}

fn unquoted_comma(text: &str) -> Option<usize> {
    let mut quoted = false;
    text.char_indices().find_map(|(index, character)| {
        if character == '"' {
            quoted = !quoted;
        }
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
    /// Builds the client; construction performs no request.
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy is not a valid address.
    pub fn new(proxy: Option<&str>) -> Result<Self, IptvError> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(8))
            .redirect(Policy::limited(5))
            .user_agent(format!("ApricotPlayer/{}", env!("CARGO_PKG_VERSION")));
        if let Some(proxy) = proxy.filter(|value| !value.trim().is_empty()) {
            builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| IptvError::Proxy)?);
        }
        Ok(Self {
            client: builder.build().map_err(|_| IptvError::Connect)?,
        })
    }

    /// Every channel, film and series of the source.
    ///
    /// # Errors
    ///
    /// Returns an error when the source cannot be read.
    pub fn load(&self, source: &IptvSource) -> Result<Vec<IptvChannel>, IptvError> {
        match source.kind {
            SourceKind::Playlist => {
                let address = source.playlist_address();
                let path = Path::new(&address);
                let (bytes, base) = if path.is_file() {
                    let length = path
                        .metadata()
                        .map_err(|_| IptvError::PlaylistUnreadable)?
                        .len();
                    if length > MAX_BYTES {
                        return Err(IptvError::PlaylistTooLarge);
                    }
                    (
                        std::fs::read(path).map_err(|_| IptvError::PlaylistUnreadable)?,
                        Url::from_file_path(path).ok(),
                    )
                } else {
                    let url = http_address(&address)?;
                    (self.bytes(url.clone(), &[])?, Some(url))
                };
                let text = decode_playlist(&bytes);
                parse_m3u(&text, base.as_ref())
            }
            SourceKind::Xtream => self.xtream(source),
            SourceKind::Stalker => self.stalker(source),
        }
    }

    /// The programme guide from a file or an address.
    ///
    /// # Errors
    ///
    /// Returns an error when the guide cannot be read.
    pub fn epg(&self, address: &str) -> Result<EpgGuide, IptvError> {
        let bytes = if Path::new(address).is_file() {
            let path = Path::new(address);
            if path
                .metadata()
                .map_err(|_| IptvError::GuideUnreadable)?
                .len()
                > MAX_BYTES
            {
                return Err(IptvError::GuideTooLarge);
            }
            std::fs::read(path).map_err(|_| IptvError::GuideUnreadable)?
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
    ) -> Result<Value, IptvError> {
        let login = source.credentials()?;
        let mut url = http_address(&source.address)?;
        if !url.path().ends_with("player_api.php") {
            url.set_path(&format!(
                "{}/player_api.php",
                url.path().trim_end_matches('/')
            ));
        }
        url.query_pairs_mut()
            .append_pair("username", login["username"].as_str().unwrap_or_default())
            .append_pair("password", login["password"].as_str().unwrap_or_default())
            .append_pair("action", action)
            .extend_pairs(extra.iter().map(|(key, value)| (*key, value.as_str())));
        self.json(url, &[])
    }

    fn xtream(&self, source: &IptvSource) -> Result<Vec<IptvChannel>, IptvError> {
        let mut output = Vec::new();
        for (kind, categories, streams) in [
            (ChannelKind::Live, "get_live_categories", "get_live_streams"),
            (ChannelKind::Movie, "get_vod_categories", "get_vod_streams"),
            (ChannelKind::Series, "get_series_categories", "get_series"),
        ] {
            let categories = self.xtream_request(source, categories, &[])?;
            let names: BTreeMap<String, String> = categories
                .as_array()
                .ok_or(IptvError::InvalidData)?
                .iter()
                .map(|item| {
                    (
                        value_text(&item["category_id"]),
                        value_text(&item["category_name"]),
                    )
                })
                .collect();
            let result = self.xtream_request(source, streams, &[])?;
            for item in result.as_array().ok_or(IptvError::InvalidData)? {
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
                    id: xtream_id(kind, &id),
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

    /// The episodes of one series, season by season. The group of an
    /// episode is its season number.
    ///
    /// # Errors
    ///
    /// Returns an error when the provider has no episodes for it.
    pub fn series(
        &self,
        source: &IptvSource,
        series_id: &str,
    ) -> Result<Vec<IptvChannel>, IptvError> {
        if source.kind != SourceKind::Xtream {
            return Err(IptvError::NoSeries);
        }
        let response = self.xtream_request(
            source,
            "get_series_info",
            &[("series_id", provider_id(series_id).1.to_owned())],
        )?;
        let seasons = response["episodes"]
            .as_object()
            .ok_or(IptvError::NoSeries)?;
        let mut seasons: Vec<_> = seasons.iter().collect();
        seasons.sort_by_key(|(key, _)| key.parse::<u64>().unwrap_or(u64::MAX));
        let mut channels = Vec::new();
        for (season, episodes) in seasons {
            for episode in episodes.as_array().ok_or(IptvError::InvalidData)? {
                let id = value_text(&episode["id"]);
                let extension = episode["container_extension"].as_str().unwrap_or("mp4");
                channels.push(IptvChannel {
                    id: xtream_id(ChannelKind::Episode, &id),
                    name: value_text(&episode["title"]),
                    group: season.clone(),
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

    fn stalker(&self, source: &IptvSource) -> Result<Vec<IptvChannel>, IptvError> {
        let credentials = source.credentials()?;
        let mac = credentials["mac"].as_str().unwrap_or_default();
        if mac.len() != 17
            || !mac
                .chars()
                .all(|value| value.is_ascii_hexdigit() || value == ':')
        {
            return Err(IptvError::InvalidMac);
        }
        let mut base = http_address(&source.address)?;
        if !base.path().ends_with("load.php") {
            base.set_path(&format!(
                "{}/server/load.php",
                base.path().trim_end_matches('/').trim_end_matches("/c")
            ));
        }
        let cookie = format!("mac={}; stb_lang=en; timezone=UTC", mac.replace(':', "%3A"));
        let request = |kind: &str,
                       action: &str,
                       token: Option<&str>,
                       extra: &[(&str, &str)]|
         -> Result<Value, IptvError> {
            let mut url = base.clone();
            url.query_pairs_mut()
                .append_pair("type", kind)
                .append_pair("action", action)
                .append_pair("JsHttpRequest", "1-xml")
                .extend_pairs(extra.iter().copied());
            let mut headers = vec![("Cookie", cookie.as_str())];
            let auth = token.map(|token| format!("Bearer {token}"));
            if let Some(auth) = &auth {
                headers.push(("Authorization", auth));
            }
            self.json(url, &headers)
                .map(|value| value.get("js").cloned().unwrap_or(value))
        };
        let handshake = request("stb", "handshake", None, &[])?;
        let token = handshake["token"]
            .as_str()
            .ok_or(IptvError::PortalRefused)?;
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
            .ok_or(IptvError::InvalidData)?;
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

    /// A private channel kept in favorites, a playlist or the queue, opened
    /// again from its source: an Xtream address is built from the saved
    /// login, a playlist or portal is read again for a fresh address and
    /// session (review 4, A1).
    ///
    /// # Errors
    ///
    /// Returns an error when the source cannot be read or no longer has the
    /// channel.
    pub fn reopen(&self, source: &IptvSource, item: &MediaItem) -> Result<IptvChannel, IptvError> {
        let (_, channel_id) = item.iptv_ids().ok_or(IptvError::ChannelGone)?;
        let text = |key: &str| {
            item.metadata
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        if source.kind == SourceKind::Xtream {
            let (named, raw_id) = provider_id(&channel_id);
            let kind = named.unwrap_or(match item.kind {
                MediaKind::Movie => ChannelKind::Movie,
                MediaKind::TvEpisode => ChannelKind::Episode,
                _ => ChannelKind::Live,
            });
            let saved = text("iptv_stream_extension");
            let extension = match (saved.as_str(), kind) {
                ("", ChannelKind::Live) if source.prefer_hls => "m3u8",
                ("", ChannelKind::Live) => "ts",
                ("", _) => "mp4",
                (extension, _) => extension,
            };
            return Ok(IptvChannel {
                url: provider_stream(source, kind, raw_id, extension)?,
                id: channel_id.clone(),
                name: item.title.clone(),
                group: item.channel.clone(),
                number: String::new(),
                epg_id: text("iptv_epg_id"),
                logo: String::new(),
                kind,
                metadata: BTreeMap::new(),
            });
        }
        let channels = self.load(source)?;
        let channel = channels
            .iter()
            .find(|channel| channel.id == channel_id)
            .or_else(|| older_playlist_channel(&channels, &channel_id))
            .ok_or(IptvError::ChannelGone)?;
        self.resolve_channel(channel)
    }

    /// A portal channel's playback link, asked for just before it plays.
    ///
    /// # Errors
    ///
    /// Returns an error when the portal gives no link.
    pub fn resolve_channel(&self, channel: &IptvChannel) -> Result<IptvChannel, IptvError> {
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
            .ok_or(IptvError::NoPlaybackLink)?;
        let mut result = channel.clone();
        command
            .strip_prefix("ffmpeg ")
            .unwrap_or(command)
            .clone_into(&mut result.url);
        Ok(result)
    }

    fn json(&self, url: Url, headers: &[(&str, &str)]) -> Result<Value, IptvError> {
        serde_json::from_slice(&self.bytes(url, headers)?).map_err(|_| IptvError::InvalidData)
    }

    fn bytes(&self, url: Url, headers: &[(&str, &str)]) -> Result<Vec<u8>, IptvError> {
        let mut request = self.client.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request.send().map_err(|_| IptvError::Connect)?;
        if !response.status().is_success() {
            return Err(IptvError::Http(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BYTES)
        {
            return Err(IptvError::ResponseTooLarge);
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| IptvError::InvalidData)?;
        if too_large(bytes.len()) {
            return Err(IptvError::ResponseTooLarge);
        }
        Ok(bytes)
    }
}

/// The id of an Xtream item: a film, series or episode is named with its
/// kind, since the provider numbers them apart from live channels and the
/// same number may mean different items (review 5, A2). A live channel
/// keeps its number, as saved before.
fn xtream_id(kind: ChannelKind, id: &str) -> String {
    match kind {
        ChannelKind::Live => id.to_owned(),
        ChannelKind::Movie => format!("movie:{id}"),
        ChannelKind::Series => format!("series:{id}"),
        ChannelKind::Episode => format!("episode:{id}"),
    }
}

/// The kind named in an Xtream id, if any, and the provider's own number.
#[must_use]
pub fn provider_id(id: &str) -> (Option<ChannelKind>, &str) {
    for (prefix, kind) in [
        ("movie:", ChannelKind::Movie),
        ("series:", ChannelKind::Series),
        ("episode:", ChannelKind::Episode),
    ] {
        if let Some(raw) = id.strip_prefix(prefix) {
            return (Some(kind), raw);
        }
    }
    (None, id)
}

/// A playlist channel saved by 2.1 release candidates, whose id was its
/// row and guide id ("12:news.de"): found by the guide id when exactly one
/// channel has it. Without a guide id, or with several, the channel must be
/// chosen again (review 5, A2).
fn older_playlist_channel<'a>(channels: &'a [IptvChannel], id: &str) -> Option<&'a IptvChannel> {
    let (row, guide) = id.split_once(':')?;
    if guide.is_empty() || row.is_empty() || !row.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut matching = channels.iter().filter(|channel| channel.epg_id == guide);
    let first = matching.next()?;
    matching.next().is_none().then_some(first)
}

fn provider_stream(
    source: &IptvSource,
    kind: ChannelKind,
    id: &str,
    extension: &str,
) -> Result<String, IptvError> {
    if !id.chars().all(|value| value.is_ascii_alphanumeric())
        || !extension.chars().all(|value| value.is_ascii_alphanumeric())
    {
        return Err(IptvError::InvalidData);
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
        ChannelKind::Series => return Err(IptvError::ChooseEpisode),
    };
    url.path_segments_mut()
        .map_err(|()| IptvError::NeedHttpAddress)?
        .pop_if_empty()
        .push(group)
        .push(credentials["username"].as_str().unwrap_or_default())
        .push(credentials["password"].as_str().unwrap_or_default())
        .push(&format!("{id}.{extension}"));
    Ok(url.to_string())
}

fn http_address(value: &str) -> Result<Url, IptvError> {
    Url::parse(value.trim())
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .ok_or(IptvError::NeedHttpAddress)
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
    fn addresses_with_a_login_or_token_are_private() {
        for private in [
            "https://user:pass@example.invalid/list.m3u",
            "https://example.invalid/get.php?username=u&password=p&type=m3u",
            "https://example.invalid/live/USER/PASS/123.ts",
            "http://example.invalid/movie/USER/PASS/9.mkv",
            "https://cdn.example.invalid/stream.m3u8?token=abc",
        ] {
            assert!(address_is_private(private), "{private}");
        }
        for public in [
            "https://example.invalid/free/news.m3u8",
            "https://example.invalid/live/news.m3u8",
            "/Users/me/tv.m3u",
            "",
        ] {
            assert!(!address_is_private(public), "{public}");
        }
    }

    /// Review 2.1: a guide address with a password is saved protected and
    /// still reads back for loading the guide.
    #[test]
    fn a_guide_address_with_a_login_is_saved_protected() {
        let mut source = IptvSource {
            id: "one".into(),
            name: "Home".into(),
            kind: SourceKind::Xtream,
            address: "https://example.invalid".into(),
            protected_credentials: String::new(),
            epg_address: String::new(),
            protected_epg: String::new(),
            prefer_hls: false,
        };
        let secret = "https://example.invalid/xmltv.php?username=USER&password=FAKE_SECRET";
        source.set_epg_address(secret).expect("protect");
        let saved = serde_json::to_string(&source).expect("json");
        assert!(!saved.contains("FAKE_SECRET"), "{saved}");
        assert_eq!(source.epg_address, "https://example.invalid/…");
        let restored: IptvSource = serde_json::from_str(&saved).expect("read");
        assert_eq!(restored.epg_url(), secret);
        source
            .set_epg_address("https://example.invalid/guide.xml")
            .expect("plain");
        assert!(source.protected_epg.is_empty());
        assert_eq!(source.epg_url(), "https://example.invalid/guide.xml");
    }

    /// Review 4, A1: an Xtream channel kept as a reference is opened again
    /// from the protected login, with its saved kind and extension.
    #[test]
    fn a_kept_xtream_channel_is_opened_from_its_source() {
        let mut source = IptvSource {
            id: "one".into(),
            name: "Home".into(),
            kind: SourceKind::Xtream,
            address: "https://tv.example.test/player_api.php".into(),
            protected_credentials: String::new(),
            epg_address: String::new(),
            protected_epg: String::new(),
            prefer_hls: false,
        };
        source
            .set_credentials("USER", "FAKE_SECRET", "")
            .expect("protect");
        let channel = IptvChannel {
            id: "42".into(),
            name: "Film".into(),
            group: "Films".into(),
            number: String::new(),
            epg_id: String::new(),
            logo: String::new(),
            url: provider_stream(&source, ChannelKind::Movie, "42", "mkv").expect("url"),
            kind: ChannelKind::Movie,
            metadata: BTreeMap::new(),
        };
        let item = channel.media(&source).expect("item");
        let kept = item.without_private_address().expect("kept");
        assert!(
            !serde_json::to_string(&kept)
                .unwrap()
                .contains("FAKE_SECRET")
        );
        let client = IptvClient::new(None).expect("client");
        let opened = client.reopen(&source, &kept).expect("reopen");
        assert_eq!(opened.url, channel.url);
        assert_eq!(opened.kind, ChannelKind::Movie);
        let again = opened.media(&source).expect("item");
        assert_eq!(again.id, kept.id);
    }

    /// Review 5, A2: a film and a live channel with the same provider
    /// number are different items, and a film saved by an older version
    /// (plain number, film kind) still opens as the film.
    #[test]
    fn xtream_items_of_different_kinds_keep_apart() {
        let mut source = IptvSource {
            id: "one".into(),
            name: "Home".into(),
            kind: SourceKind::Xtream,
            address: "https://tv.example.test/player_api.php".into(),
            protected_credentials: String::new(),
            epg_address: String::new(),
            protected_epg: String::new(),
            prefer_hls: false,
        };
        source
            .set_credentials("USER", "FAKE_SECRET", "")
            .expect("protect");
        let channel = |kind| IptvChannel {
            id: xtream_id(kind, "5"),
            name: "Five".into(),
            group: String::new(),
            number: String::new(),
            epg_id: String::new(),
            logo: String::new(),
            url: provider_stream(&source, kind, "5", "ts").expect("url"),
            kind,
            metadata: BTreeMap::new(),
        };
        let live = channel(ChannelKind::Live).media(&source).expect("live");
        let film = channel(ChannelKind::Movie).media(&source).expect("film");
        assert_ne!(live.id, film.id);
        let client = IptvClient::new(None).expect("client");
        let kept = film.without_private_address().expect("kept");
        let opened = client.reopen(&source, &kept).expect("film");
        assert_eq!(opened.kind, ChannelKind::Movie);
        assert!(opened.url.contains("/movie/"), "{}", opened.url);
        let mut older = kept.clone();
        older.metadata.insert("iptv_channel_id".into(), "5".into());
        let opened = client.reopen(&source, &older).expect("older film");
        assert_eq!(opened.kind, ChannelKind::Movie);
        assert_eq!(provider_id("episode:9"), (Some(ChannelKind::Episode), "9"));
    }

    /// Review 5, A2: a playlist channel keeps its id when the provider adds
    /// a channel above it; repeats are told apart by occurrence; an id of
    /// an older version is found by its guide id only when it is unique.
    #[test]
    fn playlist_ids_do_not_depend_on_rows() {
        let list = |text: &str| parse_m3u(text, None).expect("list");
        let before = list(
            "#EXTM3U
#EXTINF:-1 tvg-id=\"news.de\",News
http://a.test/1
#EXTINF:-1,Music
http://a.test/2
",
        );
        let after = list(
            "#EXTM3U
#EXTINF:-1,Sport
http://a.test/0
#EXTINF:-1 tvg-id=\"news.de\",News
http://a.test/1
#EXTINF:-1,Music
http://a.test/2
#EXTINF:-1,Music
http://a.test/3
",
        );
        assert_eq!(before[0].id, after[1].id);
        assert_eq!(before[1].id, after[2].id);
        assert_ne!(after[2].id, after[3].id);
        assert_eq!(
            older_playlist_channel(&after, "0:news.de").map(|c| c.id.as_str()),
            Some("tvg:news.de")
        );
        assert!(older_playlist_channel(&after, "1:").is_none());
        let doubled = list(
            "#EXTM3U
#EXTINF:-1 tvg-id=\"x\",A
http://a.test/1
#EXTINF:-1 tvg-id=\"x\",B
http://a.test/2
",
        );
        assert!(older_playlist_channel(&doubled, "0:x").is_none());
    }

    #[test]
    fn credentialed_playlist_entries_stay_out_of_history() {
        let source = IptvSource {
            id: "one".into(),
            name: "Home".into(),
            kind: SourceKind::Playlist,
            address: "https://example.invalid/free.m3u".into(),
            protected_credentials: String::new(),
            epg_address: String::new(),
            protected_epg: String::new(),
            prefer_hls: false,
        };
        let channel = |url: &str| IptvChannel {
            id: "c".into(),
            name: "News".into(),
            group: String::new(),
            number: String::new(),
            epg_id: String::new(),
            logo: String::new(),
            url: url.into(),
            kind: ChannelKind::Live,
            metadata: BTreeMap::new(),
        };
        let private = channel("https://example.invalid/live/USER/PASS/123.ts")
            .media(&source)
            .expect("item");
        assert!(private.has_private_address());
        let public = channel("https://example.invalid/free/news.m3u8")
            .media(&source)
            .expect("item");
        assert!(!public.has_private_address());
    }

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
