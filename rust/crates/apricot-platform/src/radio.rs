//! Radio Browser catalog and durable station identities. Calls run on workers,
//! never on the UI or playback thread. Read retries never repeat mutations.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.

use std::{collections::BTreeMap, io::Read, time::Duration};

use apricot_core::{MediaItem, MediaKind};
use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

const MAX_BYTES: u64 = 8 * 1024 * 1024;
pub const PAGE_SIZE: usize = 100;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Station {
    pub stationuuid: String,
    pub name: String,
    pub url: String,
    pub url_resolved: String,
    pub homepage: String,
    pub favicon: String,
    pub tags: String,
    pub country: String,
    pub countrycode: String,
    pub state: String,
    pub language: String,
    pub codec: String,
    pub bitrate: u32,
    pub votes: u64,
    pub clickcount: u64,
    pub clicktrend: i64,
    pub lastcheckok: u8,
}

impl Station {
    #[must_use]
    pub fn stream_url(&self) -> Option<Url> {
        http_url(&self.url_resolved).or_else(|| http_url(&self.url))
    }

    #[must_use]
    pub fn media(&self) -> Option<MediaItem> {
        let url = self.stream_url()?;
        let mut item = MediaItem::from_direct_link(url.as_str())?;
        item.title.clone_from(&self.name);
        item.channel.clone_from(&self.country);
        item.kind = MediaKind::LiveStream;
        item.metadata
            .insert("radio_station_id".into(), self.stationuuid.clone().into());
        item.metadata
            .insert("radio_station".into(), serde_json::to_value(self).ok()?);
        Some(item)
    }

    #[must_use]
    pub fn label(&self) -> String {
        [
            self.name.clone(),
            self.country.clone(),
            self.language.clone(),
            self.codec.clone(),
            (self.bitrate > 0)
                .then(|| format!("{} kbps", self.bitrate))
                .unwrap_or_default(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
    }

    #[must_use]
    pub fn details(&self) -> String {
        format!(
            "{}\nCountry: {}\nRegion: {}\nLanguage: {}\nTags: {}\nCodec: {}\nBitrate: {} kbps\nVotes: {}\nPlayed: {}\nWorking at last check: {}\nWebsite: {}\nStream: {}",
            self.name,
            self.country,
            self.state,
            self.language,
            self.tags,
            self.codec,
            self.bitrate,
            self.votes,
            self.clickcount,
            self.lastcheckok == 1,
            self.homepage,
            self.stream_url()
                .map_or_else(String::new, |url| url.to_string())
        )
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct RadioFilters {
    pub country: String,
    pub region: String,
    pub language: String,
    pub tags: String,
    pub codec: String,
    pub minimum_bitrate: u32,
    pub maximum_bitrate: u32,
    pub only_working: bool,
    pub https_only: bool,
}

impl RadioFilters {
    #[must_use]
    pub fn matches(&self, station: &Station) -> bool {
        let same =
            |wanted: &str, actual: &str| wanted.is_empty() || wanted.eq_ignore_ascii_case(actual);
        let contains = |wanted: &str, actual: &str| {
            wanted.is_empty()
                || actual
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case(wanted.trim()))
        };
        same(&self.country, &station.countrycode)
            && same(&self.region, &station.state)
            && contains(&self.language, &station.language)
            && self
                .tags
                .split(',')
                .filter(|part| !part.trim().is_empty())
                .all(|tag| contains(tag, &station.tags))
            && same(&self.codec, &station.codec)
            && station.bitrate >= self.minimum_bitrate
            && (self.maximum_bitrate == 0 || station.bitrate <= self.maximum_bitrate)
            && (!self.only_working || station.lastcheckok == 1)
            && (!self.https_only
                || station
                    .stream_url()
                    .is_some_and(|url| url.scheme() == "https"))
    }

    fn append_query(&self, query: &mut Vec<(String, String)>) {
        for (key, value) in [
            ("countrycode", &self.country),
            ("state", &self.region),
            ("language", &self.language),
            ("tagList", &self.tags),
            ("codec", &self.codec),
        ] {
            if !value.trim().is_empty() {
                query.push((key.to_owned(), value.trim().to_owned()));
            }
        }
        query.push(("bitrateMin".into(), self.minimum_bitrate.to_string()));
        if self.maximum_bitrate > 0 {
            query.push(("bitrateMax".into(), self.maximum_bitrate.to_string()));
        }
        query.push(("hidebroken".into(), self.only_working.to_string()));
        query.push((
            "is_https".into(),
            if self.https_only { "true" } else { "" }.to_owned(),
        ));
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum RadioCategory {
    #[default]
    All,
    MostPlayed,
    Trending,
    MostVoted,
    RecentClicks,
    Updated,
    Country(String),
    Region {
        country: String,
        name: String,
    },
    Language(String),
    Tag(String),
    Codec(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DirectoryEntry {
    pub name: String,
    #[serde(default)]
    pub stationcount: u64,
    #[serde(default)]
    pub iso_3166_1: String,
    #[serde(default)]
    pub country: String,
}

#[derive(Clone, Debug)]
pub struct RadioRequest {
    pub category: RadioCategory,
    pub search: String,
    pub global_filters: RadioFilters,
    pub local_filters: RadioFilters,
    pub offset: usize,
}

#[derive(Clone, Debug)]
pub struct RadioPage {
    pub stations: Vec<Station>,
    pub next_offset: usize,
    pub has_more: bool,
}

#[derive(Clone)]
pub struct RadioClient {
    client: Client,
    servers: Vec<Url>,
}

impl RadioClient {
    /// Builds a lazily used catalog client. Construction performs no request.
    pub fn new(proxy: Option<&str>) -> Result<Self, String> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(7))
            .redirect(Policy::limited(4))
            .user_agent(format!("ApricotPlayer/{}", env!("CARGO_PKG_VERSION")));
        if let Some(proxy) = proxy.filter(|proxy| !proxy.trim().is_empty()) {
            builder = builder.proxy(
                reqwest::Proxy::all(proxy).map_err(|_| "The radio proxy is invalid.".to_owned())?,
            );
        }
        Ok(Self {
            client: builder
                .build()
                .map_err(|_| "Unable to create the radio client.".to_owned())?,
            servers: [
                "https://de1.api.radio-browser.info/",
                "https://nl1.api.radio-browser.info/",
                "https://at1.api.radio-browser.info/",
            ]
            .into_iter()
            .filter_map(|address| Url::parse(address).ok())
            .collect(),
        })
    }

    /// Fetches a raw page before applying local intersections. Advancing the raw
    /// offset prevents filters from trapping pagination on an empty page.
    pub fn stations(&self, request: &RadioRequest) -> Result<RadioPage, String> {
        let mut query = vec![
            ("limit".into(), PAGE_SIZE.to_string()),
            ("offset".into(), request.offset.to_string()),
        ];
        request.global_filters.append_query(&mut query);
        if !request.search.trim().is_empty() {
            query.push(("name".into(), request.search.trim().into()));
        }
        let mut category_filters = RadioFilters::default();
        let order = match &request.category {
            RadioCategory::All => "name",
            RadioCategory::MostPlayed => "clickcount",
            RadioCategory::Trending => "clicktrend",
            RadioCategory::MostVoted => "votes",
            RadioCategory::RecentClicks => "clicktimestamp",
            RadioCategory::Updated => "lastchangetime",
            RadioCategory::Country(value) => {
                category_filters.country.clone_from(value);
                "name"
            }
            RadioCategory::Region { country, name } => {
                category_filters.country.clone_from(country);
                category_filters.region.clone_from(name);
                "name"
            }
            RadioCategory::Language(value) => {
                category_filters.language.clone_from(value);
                "name"
            }
            RadioCategory::Tag(value) => {
                category_filters.tags.clone_from(value);
                "name"
            }
            RadioCategory::Codec(value) => {
                category_filters.codec.clone_from(value);
                "name"
            }
        };
        query.push(("order".into(), order.into()));
        query.push(("reverse".into(), (order != "name").to_string()));
        // Server-side narrowing is safe only where no global constraint would
        // be replaced; the final intersection is always checked locally.
        for (key, value, global) in [
            (
                "countrycode",
                &category_filters.country,
                &request.global_filters.country,
            ),
            (
                "state",
                &category_filters.region,
                &request.global_filters.region,
            ),
            (
                "language",
                &category_filters.language,
                &request.global_filters.language,
            ),
            (
                "tagList",
                &category_filters.tags,
                &request.global_filters.tags,
            ),
            (
                "codec",
                &category_filters.codec,
                &request.global_filters.codec,
            ),
        ] {
            if !value.is_empty() && global.is_empty() {
                query.retain(|(name, _)| name != key);
                query.push((key.into(), value.clone()));
            }
        }
        let raw: Vec<Station> =
            serde_json::from_value(self.read("json/stations/search", &query, false)?)
                .map_err(|_| "The radio catalog returned invalid station data.".to_owned())?;
        let count = raw.len();
        let stations = raw
            .into_iter()
            .filter(|station| {
                !station.stationuuid.is_empty()
                    && station.stream_url().is_some()
                    && request.global_filters.matches(station)
                    && request.local_filters.matches(station)
                    && category_filters.matches(station)
            })
            .collect();
        Ok(RadioPage {
            stations,
            next_offset: request.offset.saturating_add(count),
            has_more: count >= PAGE_SIZE,
        })
    }

    pub fn directory(
        &self,
        kind: &str,
        offset: usize,
        search: &str,
        country: &str,
    ) -> Result<Vec<DirectoryEntry>, String> {
        if !matches!(
            kind,
            "countries" | "states" | "languages" | "tags" | "codecs"
        ) {
            return Err("Unknown radio directory.".to_owned());
        }
        let mut query = vec![
            ("limit".into(), PAGE_SIZE.to_string()),
            ("offset".into(), offset.to_string()),
            ("order".into(), "name".into()),
        ];
        if !search.trim().is_empty() {
            query.push(("filter".into(), search.trim().into()));
        }
        if !country.is_empty() {
            query.push(("country".into(), country.into()));
        }
        serde_json::from_value(self.read(&format!("json/{kind}"), &query, false)?)
            .map_err(|_| "The radio catalog returned invalid directory data.".to_owned())
    }

    pub fn vote(&self, id: &str) -> Result<(), String> {
        validate_station_id(id)?;
        let result = self.read(&format!("json/vote/{id}"), &[], true)?;
        if result.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(())
        } else {
            Err(result
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("The radio catalog did not accept the vote.")
                .to_owned())
        }
    }

    pub fn clicked(&self, id: &str) -> Result<(), String> {
        validate_station_id(id)?;
        let _ = self.read(&format!("json/url/{id}"), &[], true)?;
        Ok(())
    }

    fn read(
        &self,
        path: &str,
        query: &[(String, String)],
        mutation: bool,
    ) -> Result<Value, String> {
        let mut last = "The radio catalog is unavailable.".to_owned();
        for base in self.servers.iter().take(if mutation { 1 } else { 2 }) {
            let url = base
                .join(path)
                .map_err(|_| "Invalid radio request.".to_owned())?;
            let response = self.client.get(url).query(query).send();
            match response {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        last = format!("The radio catalog returned HTTP {}.", status.as_u16());
                        if status.as_u16() == 429 || !status.is_server_error() {
                            return Err(last);
                        }
                        continue;
                    }
                    if response
                        .content_length()
                        .is_some_and(|length| length > MAX_BYTES)
                    {
                        return Err("The radio response is too large.".into());
                    }
                    let mut bytes = Vec::new();
                    response
                        .take(MAX_BYTES + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| "The radio response was interrupted.".to_owned())?;
                    if bytes.len() as u64 > MAX_BYTES {
                        return Err("The radio response is too large.".into());
                    }
                    return serde_json::from_slice(&bytes)
                        .map_err(|_| "The radio catalog returned invalid JSON.".to_owned());
                }
                Err(_) => last = "Unable to connect to the radio catalog.".into(),
            }
        }
        Err(last)
    }
}

fn validate_station_id(id: &str) -> Result<(), String> {
    if id.len() == 36
        && id
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == '-')
    {
        Ok(())
    } else {
        Err("Invalid station identity.".into())
    }
}

#[must_use]
pub fn http_url(value: &str) -> Option<Url> {
    Url::parse(value.trim()).ok().filter(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct RadioLibrary {
    pub favorites: Vec<Station>,
    pub recent: Vec<Station>,
    pub global_filters: RadioFilters,
    pub local_filters: BTreeMap<String, RadioFilters>,
    pub announce_metadata: bool,
}

impl Default for RadioLibrary {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            recent: Vec::new(),
            global_filters: RadioFilters {
                only_working: true,
                ..RadioFilters::default()
            },
            local_filters: BTreeMap::new(),
            announce_metadata: true,
        }
    }
}

impl RadioLibrary {
    pub fn toggle_favorite(&mut self, station: &Station) -> bool {
        if let Some(index) = self
            .favorites
            .iter()
            .position(|value| value.stationuuid == station.stationuuid)
        {
            self.favorites.remove(index);
            false
        } else {
            self.favorites.push(station.clone());
            true
        }
    }

    pub fn played(&mut self, station: &Station) {
        self.recent
            .retain(|value| value.stationuuid != station.stationuuid);
        self.recent.insert(0, station.clone());
        self.recent.truncate(200);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_resolved_url_falls_back_and_filters_are_intersected() {
        let station = Station {
            stationuuid: "station".into(),
            name: "Radio".into(),
            url: "https://example.test/live".into(),
            url_resolved: "file:///etc/passwd".into(),
            countrycode: "SI".into(),
            language: "Slovenian,English".into(),
            tags: "Rock,Pop".into(),
            bitrate: 192,
            lastcheckok: 1,
            ..Station::default()
        };
        assert_eq!(
            station.stream_url().unwrap().host_str(),
            Some("example.test")
        );
        assert!(
            RadioFilters {
                language: "english".into(),
                tags: "rock,pop".into(),
                minimum_bitrate: 128,
                https_only: true,
                ..RadioFilters::default()
            }
            .matches(&station)
        );
        assert!(
            !RadioFilters {
                country: "AT".into(),
                ..RadioFilters::default()
            }
            .matches(&station)
        );
    }

    #[test]
    fn favorites_and_history_keep_station_identity() {
        let station = Station {
            stationuuid: "one".into(),
            ..Station::default()
        };
        let mut library = RadioLibrary::default();
        assert!(library.announce_metadata);
        assert!(library.toggle_favorite(&station));
        assert!(!library.toggle_favorite(&station));
        library.played(&station);
        library.played(&station);
        assert_eq!(library.recent.len(), 1);
    }
}
