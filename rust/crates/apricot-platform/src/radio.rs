//! Radio Browser catalog and durable station identities. Calls run on workers,
//! never on the UI or playback thread. Read retries never repeat mutations.

use std::{collections::BTreeMap, io::Read, time::Duration};

use apricot_core::{MediaItem, MediaKind};
use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

const MAX_BYTES: u64 = 8 * 1024 * 1024;
pub const PAGE_SIZE: usize = 100;

/// Why a catalog request failed; the interface words each case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RadioError {
    /// The proxy in Settings is not a valid address.
    Proxy,
    /// No catalog server answered.
    Connect,
    /// Too many requests: the catalog asks to wait.
    Busy,
    Http(u16),
    /// The answer was cut off, too large or not the expected data.
    InvalidData,
    InvalidStation,
    /// The catalog refused the vote, with its own reason.
    VoteRejected(String),
}

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

    /// The station's name with single spaces.
    #[must_use]
    pub fn display_name(&self) -> String {
        self.name.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The rows of a station list: the name only, as the details are in
    /// Details; stations that share a name also say their codec and bit
    /// rate, so they can be told apart (review 2.1).
    #[must_use]
    pub fn list_rows(stations: &[Self]) -> Vec<String> {
        let names: Vec<String> = stations.iter().map(Self::display_name).collect();
        stations
            .iter()
            .zip(&names)
            .map(|(station, name)| {
                let shared = names
                    .iter()
                    .filter(|other| other.eq_ignore_ascii_case(name))
                    .count()
                    > 1;
                let quality = [
                    station.codec.clone(),
                    if station.bitrate > 0 {
                        format!("{} kbps", station.bitrate)
                    } else {
                        String::new()
                    },
                ]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
                if shared && !quality.is_empty() {
                    format!("{name}, {quality}")
                } else {
                    name.clone()
                }
            })
            .collect()
    }

    #[must_use]
    pub fn label(&self) -> String {
        [
            self.name.split_whitespace().collect::<Vec<_>>().join(" "),
            self.country.clone(),
            self.language.clone(),
            self.codec.clone(),
            if self.bitrate > 0 {
                format!("{} kbps", self.bitrate)
            } else {
                String::new()
            },
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
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
        // The catalog answers an empty value with a server error.
        if self.https_only {
            query.push(("is_https".into(), "true".into()));
        }
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

/// The catalog's lists of station properties.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectoryKind {
    Countries,
    Regions,
    Languages,
    Tags,
    Codecs,
}

impl DirectoryKind {
    const fn path(self) -> &'static str {
        match self {
            Self::Countries => "countries",
            Self::Regions => "states",
            Self::Languages => "languages",
            Self::Tags => "tags",
            Self::Codecs => "codecs",
        }
    }
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

/// Countries by name, as a screen reader user finds them by the first
/// letter: "The Netherlands" is listed as "Netherlands" under N.
fn sorted_countries(mut entries: Vec<DirectoryEntry>) -> Vec<DirectoryEntry> {
    for entry in &mut entries {
        if let Some(name) = entry.name.strip_prefix("The ")
            && !name.trim().is_empty()
        {
            entry.name = name.to_owned();
        }
    }
    entries.sort_by_cached_key(|entry| entry.name.to_lowercase());
    entries
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
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy is not a valid address.
    pub fn new(proxy: Option<&str>) -> Result<Self, RadioError> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(7))
            .redirect(Policy::limited(4))
            .user_agent(format!("ApricotPlayer/{}", env!("CARGO_PKG_VERSION")));
        if let Some(proxy) = proxy.filter(|proxy| !proxy.trim().is_empty()) {
            builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| RadioError::Proxy)?);
        }
        Ok(Self {
            client: builder.build().map_err(|_| RadioError::Connect)?,
            servers: [
                "https://de1.api.radio-browser.info/",
                "https://de2.api.radio-browser.info/",
                "https://nl1.api.radio-browser.info/",
            ]
            .into_iter()
            .filter_map(|address| Url::parse(address).ok())
            .collect(),
        })
    }

    /// Fetches a raw page before applying local intersections. Advancing the raw
    /// offset prevents filters from trapping pagination on an empty page.
    ///
    /// # Errors
    ///
    /// Returns an error when the catalog cannot be read.
    #[allow(clippy::too_many_lines)] // One query, its filters and the local check.
    pub fn stations(&self, request: &RadioRequest) -> Result<RadioPage, RadioError> {
        let mut query = vec![
            ("limit".into(), PAGE_SIZE.to_string()),
            ("offset".into(), request.offset.to_string()),
        ];
        request.global_filters.append_query(&mut query);
        if !request.search.trim().is_empty() {
            query.push(("name".into(), request.search.trim().into()));
        }
        let mut category_filters = RadioFilters::default();
        // A region's country as the catalog names it ("Germany"): regions of
        // the same name in other countries stay apart (review 2.1).
        let mut region_country = None;
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
                if country.len() == 2 {
                    category_filters.country.clone_from(country);
                } else if !country.trim().is_empty() {
                    region_country = Some(country.trim().to_owned());
                    query.push(("country".into(), country.trim().to_owned()));
                    query.push(("countryExact".into(), "true".into()));
                }
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
                .map_err(|_| RadioError::InvalidData)?;
        let count = raw.len();
        let stations = raw
            .into_iter()
            // Some names start with a tab or a line break, which a screen
            // reader reads before the name.
            .map(|mut station| {
                let name = station
                    .name
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                station.name = name;
                station
            })
            .filter(|station| {
                !station.stationuuid.is_empty()
                    && station.stream_url().is_some()
                    && region_country
                        .as_deref()
                        .is_none_or(|country| station.country.eq_ignore_ascii_case(country))
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

    /// One page of countries, regions, languages, tags or codecs.
    ///
    /// # Errors
    ///
    /// Returns an error when the catalog cannot be read.
    pub fn directory(
        &self,
        kind: DirectoryKind,
        offset: usize,
        search: &str,
        country: &str,
    ) -> Result<Vec<DirectoryEntry>, RadioError> {
        // About 230 countries come in one list, sorted here: the catalog
        // orders them by country code, not by the name people read.
        if kind == DirectoryKind::Countries {
            if offset > 0 {
                return Ok(Vec::new());
            }
            let mut query = Vec::new();
            if !search.trim().is_empty() {
                query.push(("filter".into(), search.trim().into()));
            }
            let entries: Vec<DirectoryEntry> =
                serde_json::from_value(self.read("json/countries", &query, false)?)
                    .map_err(|_| RadioError::InvalidData)?;
            return Ok(sorted_countries(entries));
        }
        // Thousands of tags and languages: the most used come first. Places
        // and codecs are easier to find by name.
        let by_use = matches!(kind, DirectoryKind::Languages | DirectoryKind::Tags);
        let mut query = vec![
            ("limit".into(), PAGE_SIZE.to_string()),
            ("offset".into(), offset.to_string()),
            (
                "order".into(),
                if by_use { "stationcount" } else { "name" }.into(),
            ),
            ("reverse".into(), by_use.to_string()),
        ];
        if !search.trim().is_empty() {
            query.push(("filter".into(), search.trim().into()));
        }
        if !country.is_empty() {
            query.push(("country".into(), country.into()));
        }
        serde_json::from_value(self.read(&format!("json/{}", kind.path()), &query, false)?)
            .map_err(|_| RadioError::InvalidData)
    }

    /// Votes for a station once; a vote is never retried.
    ///
    /// # Errors
    ///
    /// Returns the catalog's reason when it refuses the vote.
    pub fn vote(&self, id: &str) -> Result<(), RadioError> {
        validate_station_id(id)?;
        let result = self.read(&format!("json/vote/{id}"), &[], true)?;
        if result.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(())
        } else {
            Err(RadioError::VoteRejected(
                result
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ))
        }
    }

    /// Tells the catalog that the station was played, as its rules ask.
    ///
    /// # Errors
    ///
    /// Returns an error when the catalog cannot be reached.
    pub fn clicked(&self, id: &str) -> Result<(), RadioError> {
        validate_station_id(id)?;
        let _ = self.read(&format!("json/url/{id}"), &[], true)?;
        Ok(())
    }

    fn read(
        &self,
        path: &str,
        query: &[(String, String)],
        mutation: bool,
    ) -> Result<Value, RadioError> {
        let mut last = RadioError::Connect;
        for base in self.servers.iter().take(if mutation { 1 } else { 2 }) {
            let url = base.join(path).map_err(|_| RadioError::InvalidData)?;
            let Ok(response) = self.client.get(url).query(query).send() else {
                last = RadioError::Connect;
                continue;
            };
            let status = response.status();
            if !status.is_success() {
                if status.as_u16() == 429 {
                    return Err(RadioError::Busy);
                }
                last = RadioError::Http(status.as_u16());
                if !status.is_server_error() {
                    return Err(last);
                }
                continue;
            }
            if response
                .content_length()
                .is_some_and(|length| length > MAX_BYTES)
            {
                return Err(RadioError::InvalidData);
            }
            let mut bytes = Vec::new();
            response
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| RadioError::InvalidData)?;
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BYTES {
                return Err(RadioError::InvalidData);
            }
            return serde_json::from_slice(&bytes).map_err(|_| RadioError::InvalidData);
        }
        Err(last)
    }
}

fn validate_station_id(id: &str) -> Result<(), RadioError> {
    if id.len() == 36
        && id
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == '-')
    {
        Ok(())
    } else {
        Err(RadioError::InvalidStation)
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
    #[must_use]
    pub fn is_favorite(&self, station: &Station) -> bool {
        self.favorites
            .iter()
            .any(|value| value.stationuuid == station.stationuuid)
    }

    /// Adds or removes the station; `true` when it was added.
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

    /// Moves the station to the top of the recently played list.
    pub fn played(&mut self, station: &Station) {
        self.recent
            .retain(|value| value.stationuuid != station.stationuuid);
        self.recent.insert(0, station.clone());
        self.recent.truncate(200);
    }
}

#[cfg(test)]
mod tests {
    /// Review 2.1: rows are station names; only stations that share a name
    /// add their codec and bit rate.
    #[test]
    fn station_rows_are_short_and_tell_namesakes_apart() {
        let station = |name: &str, bitrate: u32| super::Station {
            name: name.to_owned(),
            codec: "MP3".to_owned(),
            bitrate,
            country: "Germany".to_owned(),
            ..super::Station::default()
        };
        let rows = super::Station::list_rows(&[
            station("	Rock  FM", 128),
            station("Jazz", 128),
            station("Jazz", 320),
        ]);
        assert_eq!(
            rows,
            ["Rock FM", "Jazz, MP3 128 kbps", "Jazz, MP3 320 kbps"]
        );
    }

    /// Countries read by name: sorted, with "The" not deciding the letter.
    #[test]
    fn countries_are_sorted_by_the_name_people_read() {
        let entry = |name: &str| super::DirectoryEntry {
            name: name.to_owned(),
            stationcount: 1,
            iso_3166_1: String::new(),
            country: String::new(),
        };
        let sorted = super::sorted_countries(vec![
            entry("Slovenia"),
            entry("The Netherlands"),
            entry("austria"),
            entry("Andorra"),
        ]);
        let names: Vec<_> = sorted.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["Andorra", "austria", "Netherlands", "Slovenia"]);
    }

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
    fn an_unset_https_filter_sends_no_value() {
        let mut query = Vec::new();
        RadioFilters::default().append_query(&mut query);
        assert!(query.iter().all(|(key, _)| key != "is_https"));
        let mut query = Vec::new();
        RadioFilters {
            https_only: true,
            ..RadioFilters::default()
        }
        .append_query(&mut query);
        assert!(query.contains(&("is_https".into(), "true".into())));
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
