//! Spotify catalog and library reads (`docs/SPOTIFY_PLAN.md` 3.2, 4.2):
//! search, library, Liked Songs, albums, playlists, artists and shows, from
//! pathfinder answers. Parsing is tolerant: an unknown or restricted entry
//! becomes an unavailable row, never a crash or a silently empty list.

#![allow(clippy::missing_errors_doc)] // Every read fails only with `ApiError`.

use librespot_core::Session;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::{Api, ApiError};

/// What a row is.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ItemKind {
    Track,
    Episode,
    Album,
    Artist,
    Playlist,
    Show,
    Audiobook,
    User,
    Folder,
    LikedSongs,
    Genre,
    /// A browse category page (`spotify:page:`).
    Page,
    /// A section of Home or a browse page, listing its items.
    Section,
    /// Spotify did not send the item (restricted, removed, unknown type).
    Unavailable,
}

impl ItemKind {
    /// Rows that play when chosen; the others open a collection.
    pub const fn is_playable_item(self) -> bool {
        matches!(self, Self::Track | Self::Episode)
    }

    /// The kind a Spotify URI names.
    pub fn from_uri_public(uri: &str) -> Option<Self> {
        Self::from_uri(uri)
    }

    /// The kind a URI names, for answers without `__typename`.
    fn from_uri(uri: &str) -> Option<Self> {
        let mut parts = uri.split(':');
        if parts.next() != Some("spotify") {
            return None;
        }
        let kind = parts.next()?;
        Some(match kind {
            "track" => Self::Track,
            "episode" => Self::Episode,
            "album" => Self::Album,
            "artist" => Self::Artist,
            "playlist" => Self::Playlist,
            "show" => Self::Show,
            "audiobook" => Self::Audiobook,
            "genre" => Self::Genre,
            "page" => Self::Page,
            "section" => Self::Section,
            "collection" => Self::LikedSongs,
            "user" if uri.contains(":folder:") => Self::Folder,
            "user" if uri.ends_with(":collection") => Self::LikedSongs,
            "user" => Self::User,
            _ => return None,
        })
    }

    fn from_typename(typename: &str) -> Option<Self> {
        Some(match typename {
            "Track" => Self::Track,
            "Episode" => Self::Episode,
            "Album" => Self::Album,
            "Artist" => Self::Artist,
            "Playlist" => Self::Playlist,
            "Podcast" => Self::Show,
            "Audiobook" => Self::Audiobook,
            "User" => Self::User,
            "Folder" => Self::Folder,
            "PseudoPlaylist" => Self::LikedSongs,
            "Genre" => Self::Genre,
            _ => return None,
        })
    }
}

/// One row of a Spotify list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogItem {
    pub kind: ItemKind,
    pub uri: String,
    pub name: String,
    /// Artists, owner or publisher.
    pub subtitle: String,
    pub album: String,
    pub album_uri: String,
    /// The first artist; for a playlist its owner (`spotify:user:`).
    pub artist_uri: String,
    pub duration_ms: Option<u64>,
    pub playable: bool,
    pub explicit: bool,
    /// The occurrence in its playlist or album (`uid`), for playing and
    /// editing exactly this row.
    pub uid: Option<String>,
    /// In the library of the account (Liked Songs for tracks), when known.
    pub saved: Option<bool>,
    /// Number of items (Liked Songs, folders), when known.
    pub count: Option<u64>,
    /// A playlist the account may add to and remove from.
    pub editable: bool,
    /// Spotify's playlist format (`daily-mix`, ...), empty for others.
    pub format: String,
}

impl CatalogItem {
    fn unavailable(uri: String) -> Self {
        Self {
            kind: ItemKind::Unavailable,
            uri,
            name: String::new(),
            subtitle: String::new(),
            album: String::new(),
            album_uri: String::new(),
            artist_uri: String::new(),
            duration_ms: None,
            playable: false,
            explicit: false,
            uid: None,
            saved: None,
            count: None,
            editable: false,
            format: String::new(),
        }
    }
}

/// A page of rows and where the next page starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogPage {
    pub items: Vec<CatalogItem>,
    pub total: Option<u64>,
    /// Offset of the next page; `None` at the end.
    pub next_offset: Option<u64>,
}

/// Header of an opened album, playlist, artist or show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Collection {
    pub uri: String,
    pub name: String,
    /// Artist, owner or publisher.
    pub subtitle: String,
    pub description: String,
    /// Playlist edit rights of the account.
    pub can_edit_items: bool,
    pub can_edit_metadata: bool,
    /// The account saved (follows) it.
    pub saved: Option<bool>,
    /// Spotify's playlist format (`daily-mix`, `discover-weekly`, ...).
    pub format: String,
    pub revision: String,
    pub page: CatalogPage,
}

fn text<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or("")
}

fn names(list: &Value) -> String {
    list.pointer("/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|artist| {
                    artist
                        .pointer("/profile/name")
                        .or_else(|| artist.pointer("/data/profile/name"))
                        .and_then(Value::as_str)
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// The entity object inside the wrappers Spotify uses (`data`, `item`,
/// `itemV2`, `track`, `entity`).
fn entity(value: &Value) -> Option<&Value> {
    if value
        .get("__typename")
        .and_then(Value::as_str)
        .and_then(ItemKind::from_typename)
        .is_some()
    {
        return Some(value);
    }
    // Some answers (artist pages, album tracks) leave out `__typename`.
    if value.get("__typename").is_none()
        && value
            .get("uri")
            .and_then(Value::as_str)
            .and_then(ItemKind::from_uri)
            .is_some()
        && (value.get("name").is_some() || value.get("profile").is_some())
    {
        return Some(value);
    }
    for key in ["data", "itemV2", "item", "track", "entity"] {
        if let Some(inner) = value.get(key).and_then(entity) {
            return Some(inner);
        }
    }
    None
}

/// The URI Spotify sent next to the entity (`_uri`), also for restricted
/// entries without data.
fn wrapper_uri(value: &Value) -> String {
    for pointer in [
        "/_uri",
        "/item/_uri",
        "/itemV2/_uri",
        "/track/_uri",
        "/entity/_uri",
        "/uri",
    ] {
        if let Some(uri) = value.pointer(pointer).and_then(Value::as_str) {
            return uri.to_owned();
        }
    }
    String::new()
}

/// One row from any search, library, album, playlist or show entry.
pub fn parse_item(value: &Value) -> CatalogItem {
    let Some(node) = entity(value) else {
        return CatalogItem::unavailable(wrapper_uri(value));
    };
    let typename = text(node, "/__typename");
    let kind = ItemKind::from_typename(typename)
        .or_else(|| {
            typename
                .is_empty()
                .then(|| ItemKind::from_uri(text(node, "/uri")))
                .flatten()
        })
        .unwrap_or(ItemKind::Unavailable);
    let mut uri = text(node, "/uri").to_owned();
    if uri.is_empty() {
        uri = wrapper_uri(value);
    }
    let name = match kind {
        ItemKind::Artist => text(node, "/profile/name"),
        ItemKind::User => {
            let display = text(node, "/displayName");
            if display.is_empty() {
                text(node, "/name")
            } else {
                display
            }
        }
        _ => text(node, "/name"),
    }
    .to_owned();
    let subtitle = match kind {
        ItemKind::Track => names(&node["artists"]),
        // Releases on an artist page have a year instead of artists.
        ItemKind::Album => {
            let artists = names(&node["artists"]);
            if artists.is_empty() {
                node.pointer("/date/year")
                    .and_then(Value::as_u64)
                    .map(|year| year.to_string())
                    .unwrap_or_default()
            } else {
                artists
            }
        }
        ItemKind::Playlist => {
            let owner = text(node, "/ownerV2/data/name");
            owner.to_owned()
        }
        ItemKind::Show => text(node, "/publisher/name").to_owned(),
        ItemKind::Episode => {
            let show = text(node, "/podcastV2/data/name");
            show.to_owned()
        }
        ItemKind::Audiobook => names(&node["authors"]),
        _ => String::new(),
    };
    let duration_ms = node
        .pointer("/duration/totalMilliseconds")
        .or_else(|| node.pointer("/trackDuration/totalMilliseconds"))
        .and_then(Value::as_u64);
    let playable = node
        .pointer("/playability/playable")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| text(node, "/playability/reason") != "NOT_PLAYABLE");
    let uid = value
        .get("uid")
        .and_then(Value::as_str)
        .filter(|uid| !uid.is_empty())
        .map(str::to_owned);
    CatalogItem {
        kind,
        uri,
        name,
        subtitle,
        album: text(node, "/albumOfTrack/name").to_owned(),
        album_uri: text(node, "/albumOfTrack/uri").to_owned(),
        artist_uri: node
            .pointer("/artists/items/0/uri")
            .or_else(|| node.pointer("/ownerV2/data/uri"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        duration_ms,
        playable: playable && kind != ItemKind::Unavailable,
        explicit: text(node, "/contentRating/label") == "EXPLICIT",
        uid,
        saved: node.get("saved").and_then(Value::as_bool),
        count: node.get("count").and_then(Value::as_u64),
        editable: node
            .pointer("/currentUserCapabilities/canEditItems")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        // A playlist's format, or a release's type (ALBUM, SINGLE, EP,
        // COMPILATION) so rows can say single or EP instead of album.
        format: match text(node, "/format") {
            "" if kind == ItemKind::Album => text(node, "/type").to_owned(),
            format => format.to_owned(),
        },
    }
}

/// A titled group of Home or a browse page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub uri: String,
    pub title: String,
    pub items: Vec<CatalogItem>,
}

/// One entry of a section: a playlist, album, artist, show, or a browse
/// category page. Links to other Spotify features are left out.
fn parse_section_item(value: &Value) -> Option<CatalogItem> {
    let content = value.get("content")?;
    let uri = value.get("uri").and_then(Value::as_str).unwrap_or("");
    match text(content, "/__typename") {
        "BrowseSectionContainerWrapper" => {
            let name = text(
                content,
                "/data/data/cardRepresentation/title/transformedLabel",
            );
            let mut item = CatalogItem::unavailable(uri.to_owned());
            item.kind = ItemKind::Page;
            name.clone_into(&mut item.name);
            (!name.is_empty() && uri.starts_with("spotify:page:")).then_some(item)
        }
        "BrowseXlinkResponseWrapper" => None,
        _ => {
            let mut item = parse_item(content);
            if item.uri.is_empty() {
                uri.clone_into(&mut item.uri);
            }
            (item.kind != ItemKind::Unavailable && !item.name.is_empty()).then_some(item)
        }
    }
}

/// Titled sections with at least one usable entry, in Spotify's order.
pub fn parse_sections(list: &Value) -> Vec<Section> {
    list.get("items")
        .and_then(Value::as_array)
        .map(|sections| {
            sections
                .iter()
                .filter_map(|section| {
                    let title = text(section, "/data/title/transformedLabel");
                    let items: Vec<CatalogItem> = section
                        .pointer("/sectionItems/items")
                        .and_then(Value::as_array)
                        .map(|items| items.iter().filter_map(parse_section_item).collect())
                        .unwrap_or_default();
                    (!title.is_empty() && !items.is_empty()).then(|| Section {
                        uri: text(section, "/uri").to_owned(),
                        title: title.to_owned(),
                        items,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Home: Spotify's personal sections (Made for you, mixes, recently
/// played, recommendations), as the web player gets them.
pub async fn home(api: &Api, session: &Session) -> Result<Vec<Section>, ApiError> {
    let data = api
        .pathfinder(
            session,
            "home",
            json!({
                "timeZone": "UTC",
                "sp_t": "",
                "facet": "",
                "sectionItemsLimit": 50,
                "homeEndUserIntegration": "INTEGRATION_WEB_PLAYER",
            }),
        )
        .await?;
    let sections = data
        .pointer("/home/sectionContainer/sections")
        .ok_or_else(|| ApiError::Shape("home without sections".into()))?;
    Ok(parse_sections(sections))
}

/// Daily Mixes of the account, found by their format on Home (not by
/// title), each once, in Spotify's order.
pub async fn daily_mixes(api: &Api, session: &Session) -> Result<Vec<CatalogItem>, ApiError> {
    let mut mixes: Vec<CatalogItem> = Vec::new();
    for section in home(api, session).await? {
        for item in section.items {
            if item.format == "daily-mix" && !mixes.iter().any(|mix| mix.uri == item.uri) {
                mixes.push(item);
            }
        }
    }
    Ok(mixes)
}

/// Browse: the category pages (genres, moods, podcasts, charts).
pub async fn browse_all(api: &Api, session: &Session) -> Result<Vec<Section>, ApiError> {
    let data = api
        .pathfinder(
            session,
            "browseAll",
            json!({
                "pagePagination": { "offset": 0, "limit": 10 },
                "sectionPagination": { "offset": 0, "limit": 99 },
                "browseEndUserIntegration": "INTEGRATION_WEB_PLAYER",
            }),
        )
        .await?;
    let sections = data
        .pointer("/browseStart/sections")
        .ok_or_else(|| ApiError::Shape("browseAll without sections".into()))?;
    Ok(parse_sections(sections))
}

/// One browse category page: its title and sections.
pub async fn browse_page(
    api: &Api,
    session: &Session,
    uri: &str,
) -> Result<(String, Vec<Section>), ApiError> {
    let data = api
        .pathfinder(
            session,
            "browsePage",
            json!({
                "uri": uri,
                "pagePagination": { "offset": 0, "limit": 20 },
                "sectionPagination": { "offset": 0, "limit": 99 },
                "browseEndUserIntegration": "INTEGRATION_WEB_PLAYER",
            }),
        )
        .await?;
    let browse = data
        .get("browse")
        .ok_or_else(|| ApiError::Shape("browsePage".into()))?;
    Ok((
        text(browse, "/header/title/transformedLabel").to_owned(),
        parse_sections(&browse["sections"]),
    ))
}

/// What the account played recently (albums, playlists, artists, shows,
/// Liked Songs), newest first.
pub async fn recently_played(api: &Api, session: &Session) -> Result<CatalogPage, ApiError> {
    let played = api
        .spclient(
            session,
            reqwest::Method::GET,
            &format!(
                "/recently-played/v3/user/{}/recently-played?format=json&offset=0&limit=50&filter=default,collection-new-episodes&market=from_token",
                session.username()
            ),
            None,
        )
        .await?;
    let uris: Vec<String> = played
        .get("playContexts")
        .and_then(Value::as_array)
        .map(|contexts| {
            contexts
                .iter()
                .filter_map(|context| context.get("uri").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if uris.is_empty() {
        return Ok(CatalogPage::default());
    }
    let data = api
        .pathfinder(
            session,
            "fetchEntitiesForRecentlyPlayed",
            json!({ "uris": uris }),
        )
        .await?;
    let items = data
        .get("lookup")
        .and_then(Value::as_array)
        .map(|lookup| {
            lookup
                .iter()
                .map(parse_item)
                .filter(|item| item.kind != ItemKind::Unavailable && !item.name.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Ok(CatalogPage {
        items,
        total: None,
        next_offset: None,
    })
}

/// The account's most played tracks and artists over the last four weeks,
/// six months and all time; `titles` name them in that order (tracks, then
/// artists, for each period).
pub async fn top_content(
    api: &Api,
    session: &Session,
    titles: [&str; 6],
) -> Result<Vec<Section>, ApiError> {
    let mut sections = Vec::new();
    for (period, range) in ["SHORT_TERM", "MID_TERM", "LONG_TERM"].iter().enumerate() {
        let input = json!({ "offset": 0, "limit": 50, "sortBy": "AFFINITY", "timeRange": range });
        let data = api
            .pathfinder(
                session,
                "userTopContent",
                json!({
                    "includeTopArtists": true,
                    "topArtistsInput": input,
                    "includeTopTracks": true,
                    "topTracksInput": input,
                }),
            )
            .await?;
        for (offset, pointer) in ["/me/profile/topTracks", "/me/profile/topArtists"]
            .iter()
            .enumerate()
        {
            let items: Vec<CatalogItem> = data
                .pointer(pointer)
                .map(|list| parse_page(list, 0).items)
                .unwrap_or_default()
                .into_iter()
                .filter(|item| item.kind != ItemKind::Unavailable)
                .collect();
            if !items.is_empty() {
                sections.push(Section {
                    uri: format!("apricot:top:{range}:{offset}"),
                    title: titles[period * 2 + offset].to_owned(),
                    items,
                });
            }
        }
    }
    Ok(sections)
}

/// Spotify's lyrics of a track as LRC text (`[mm:ss.cc]line` when Spotify
/// times the lines) and the lyrics provider; `None` when Spotify has none.
pub async fn lyrics(
    api: &Api,
    session: &Session,
    uri: &str,
) -> Result<Option<(String, String)>, ApiError> {
    if let Some(id) = uri.strip_prefix("spotify:episode:") {
        return transcript(api, session, id).await;
    }
    let Some(id) = uri.strip_prefix("spotify:track:") else {
        return Ok(None);
    };
    let answer = match api
        .spclient(
            session,
            reqwest::Method::GET,
            &format!(
                "/color-lyrics/v2/track/{id}?format=json&vocalRemoval=false&market=from_token"
            ),
            None,
        )
        .await
    {
        Err(ApiError::Status(404)) => return Ok(None),
        other => other?,
    };
    Ok(lyrics_text(&answer))
}

/// LRC text and provider of a `color-lyrics` answer.
pub fn lyrics_text(answer: &Value) -> Option<(String, String)> {
    let lyrics = answer.get("lyrics")?;
    let synced = text(lyrics, "/syncType") == "LINE_SYNCED";
    let lines: Vec<String> = lyrics
        .get("lines")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|line| {
            let words = text(line, "/words").trim();
            if words.is_empty() || words == "\u{266a}" {
                return None;
            }
            if !synced {
                return Some(words.to_owned());
            }
            let start: u64 = text(line, "/startTimeMs").parse().ok()?;
            let (minutes, seconds, hundredths) =
                (start / 60_000, (start / 1000) % 60, (start % 1000) / 10);
            Some(format!(
                "[{minutes:02}:{seconds:02}.{hundredths:02}]{words}"
            ))
        })
        .collect();
    (!lines.is_empty()).then(|| {
        let provider = lyrics
            .get("providerDisplayName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        (lines.join("\n"), provider)
    })
}

/// Spotify's transcript of an episode as LRC text, one timed line per
/// sentence, the speaker before the first sentence of each turn; the
/// provider is empty. `None` when the episode has none.
async fn transcript(
    api: &Api,
    session: &Session,
    id: &str,
) -> Result<Option<(String, String)>, ApiError> {
    let answer = match api
        .spclient(
            session,
            reqwest::Method::GET,
            &format!("/transcript-read-along/v2/episode/{id}?format=json"),
            None,
        )
        .await
    {
        Err(ApiError::Status(404)) => return Ok(None),
        other => other?,
    };
    Ok(transcript_text(&answer).map(|text| (text, String::new())))
}

/// LRC text of a `transcript-read-along` answer.
pub fn transcript_text(answer: &Value) -> Option<String> {
    let mut lines = Vec::new();
    let mut speaker = String::new();
    for section in answer.get("section").and_then(Value::as_array)? {
        if let Some(title) = section.pointer("/title/title").and_then(Value::as_str) {
            title.clone_into(&mut speaker);
            continue;
        }
        let (words, closed_caption) = match section
            .pointer("/text/sentence/text")
            .and_then(Value::as_str)
        {
            Some(words) => (words, false),
            None => match section
                .pointer("/musicClosedCaption/text")
                .or_else(|| section.pointer("/fallback/sentence/text"))
                .and_then(Value::as_str)
            {
                Some(words) => (words, true),
                None => continue,
            },
        };
        let words = words.trim();
        if words.is_empty() {
            continue;
        }
        let start = section.get("startMs").and_then(Value::as_u64).unwrap_or(0);
        let (minutes, seconds, hundredths) =
            (start / 60_000, (start / 1000) % 60, (start % 1000) / 10);
        let words = if closed_caption {
            format!("({words})")
        } else if speaker.is_empty() {
            words.to_owned()
        } else {
            format!("{}: {words}", std::mem::take(&mut speaker))
        };
        lines.push(format!(
            "[{minutes:02}:{seconds:02}.{hundredths:02}]{words}"
        ));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// The address of Spotify's short audio preview of an episode, when it has
/// one.
pub async fn preview(api: &Api, session: &Session, uri: &str) -> Result<Option<String>, ApiError> {
    if !uri.starts_with("spotify:episode:") {
        return Ok(None);
    }
    let data = api
        .pathfinder(session, "getEpisodeOrChapter", json!({ "uri": uri }))
        .await?;
    Ok(data
        .pointer("/episodeUnionV2/previewPlayback/audioPreview/cdnUrl")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://"))
        .map(str::to_owned))
}

/// Another account's profile: name, followers, whether this account
/// follows it, and its public playlists, the profiles and artists it
/// follows and its followers as sections.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub uri: String,
    pub name: String,
    pub followers: Option<u64>,
    pub following: Option<bool>,
    /// All public playlists; the first section lists only the first ones.
    pub playlists_total: u64,
    pub sections: Vec<Section>,
}

/// A playlist, profile or artist of a profile answer, which has no
/// `__typename`.
fn profile_row(value: &Value) -> Option<CatalogItem> {
    let uri = text(value, "/uri");
    let kind = ItemKind::from_uri(uri)?;
    let playlist = kind == ItemKind::Playlist;
    let owner = |pointer| {
        if playlist {
            text(value, pointer).to_owned()
        } else {
            String::new()
        }
    };
    Some(CatalogItem {
        kind,
        name: text(value, "/name").to_owned(),
        subtitle: owner("/owner_name"),
        artist_uri: owner("/owner_uri"),
        playable: true,
        saved: value.get("is_following").and_then(Value::as_bool),
        ..CatalogItem::unavailable(uri.to_owned())
    })
}

fn profile_rows(answer: &Value, key: &str) -> Vec<CatalogItem> {
    answer
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(profile_row).collect())
        .unwrap_or_default()
}

fn user_name(uri: &str) -> Result<&str, ApiError> {
    uri.strip_prefix("spotify:user:")
        .filter(|name| !name.is_empty() && !name.contains(':'))
        .ok_or_else(|| ApiError::Shape(format!("not a profile: {uri}")))
}

/// An explicit profile section request; failures are not presented as an empty list.
pub async fn profile_people(
    api: &Api,
    session: &Session,
    uri: &str,
    followers: bool,
) -> Result<CatalogPage, ApiError> {
    let name = user_name(uri)?;
    let section = if followers { "followers" } else { "following" };
    let answer = api
        .spclient(
            session,
            reqwest::Method::GET,
            &format!("/user-profile-view/v3/profile/{name}/{section}?market=from_token"),
            None,
        )
        .await?;
    if answer.get("profiles").and_then(Value::as_array).is_none() {
        return Err(ApiError::Shape(
            "Spotify did not return the requested profile list".into(),
        ));
    }
    let items = profile_rows(&answer, "profiles");
    Ok(CatalogPage {
        total: Some(items.len() as u64),
        items,
        next_offset: None,
    })
}

/// A profile and its sections, titled `titles` (public playlists,
/// following, followers); empty sections are left out.
pub async fn profile(
    api: &Api,
    session: &Session,
    uri: &str,
    titles: [&str; 3],
) -> Result<Profile, ApiError> {
    let name = user_name(uri)?;
    let base = format!("/user-profile-view/v3/profile/{name}");
    let answer = api
        .spclient(
            session,
            reqwest::Method::GET,
            &format!(
                "{base}?playlist_limit={PAGE}&artist_limit=0&episode_limit=0&market=from_token"
            ),
            None,
        )
        .await?;
    let lists = |path: &'static str| {
        let path = format!("{base}/{path}?market=from_token");
        async move {
            api.spclient(session, reqwest::Method::GET, &path, None)
                .await
                .map(|answer| profile_rows(&answer, "profiles"))
                .unwrap_or_default()
        }
    };
    let following_list = lists("following").await;
    let followers_list = lists("followers").await;
    let following = api
        .pathfinder(session, "isFollowingUsers", json!({ "uris": [uri] }))
        .await
        .ok()
        .and_then(|data| data.pointer("/users/0/following").and_then(Value::as_bool));
    let playlists = profile_rows(&answer, "public_playlists");
    let playlists_total = answer
        .get("total_public_playlists_count")
        .and_then(Value::as_u64)
        .unwrap_or(playlists.len() as u64);
    let sections = [
        ("playlists", playlists),
        ("following", following_list),
        ("followers", followers_list),
    ]
    .into_iter()
    .zip(titles)
    .filter(|((_, items), _)| !items.is_empty())
    .map(|((key, items), title)| Section {
        uri: profile_section_uri(uri, key),
        title: title.to_owned(),
        items,
    })
    .collect();
    Ok(Profile {
        uri: uri.to_owned(),
        name: text(&answer, "/name").to_owned(),
        followers: answer.get("followers_count").and_then(Value::as_u64),
        following,
        playlists_total,
        sections,
    })
}

/// The URI of a profile's section row (`playlists`, `following`,
/// `followers`).
pub fn profile_section_uri(profile: &str, key: &str) -> String {
    format!("apricot:{profile}:{key}")
}

/// A page of a profile's public playlists.
pub async fn profile_playlists(
    api: &Api,
    session: &Session,
    uri: &str,
    offset: u64,
) -> Result<CatalogPage, ApiError> {
    let name = user_name(uri)?;
    let answer = api
        .spclient(
            session,
            reqwest::Method::GET,
            &format!(
                "/user-profile-view/v3/profile/{name}/playlists?offset={offset}&limit={PAGE}&market=from_token"
            ),
            None,
        )
        .await?;
    let items = profile_rows(&answer, "public_playlists");
    let total = answer
        .get("total_public_playlists_count")
        .and_then(Value::as_u64);
    let next = offset + items.len() as u64;
    Ok(CatalogPage {
        next_offset: (!items.is_empty() && total.is_some_and(|total| next < total)).then_some(next),
        total,
        items,
    })
}

/// Spotify's radio for a track, artist, album or playlist: the URI of the
/// station playlist Spotify makes for it.
pub async fn radio(api: &Api, session: &Session, seed: &str) -> Result<String, ApiError> {
    let answer = api
        .spclient(
            session,
            reqwest::Method::GET,
            &format!("/inspiredby-mix/v2/seed_to_playlist/{seed}?response-format=json"),
            None,
        )
        .await?;
    answer
        .pointer("/mediaItems/0/uri")
        .and_then(Value::as_str)
        .filter(|uri| uri.starts_with("spotify:playlist:"))
        .map(str::to_owned)
        .ok_or_else(|| ApiError::Shape("no radio for this item".into()))
}

/// A page from `{items, totalCount}` at `offset`.
pub fn parse_page(list: &Value, offset: u64) -> CatalogPage {
    let items: Vec<CatalogItem> = list
        .get("items")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_item).collect())
        .unwrap_or_default();
    let total = list.get("totalCount").and_then(Value::as_u64);
    let next = list
        .pointer("/pagingInfo/nextOffset")
        .and_then(Value::as_u64)
        .or_else(|| {
            let end = offset + items.len() as u64;
            total.filter(|total| end < *total).map(|_| end)
        })
        .filter(|_| !items.is_empty());
    CatalogPage {
        items,
        total,
        next_offset: next,
    }
}

/// Search types; `All` is Spotify's overview with each section's first
/// results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchKind {
    All,
    Tracks,
    Artists,
    Albums,
    Playlists,
    Shows,
    Episodes,
    Profiles,
    Audiobooks,
}

impl SearchKind {
    pub const ALL: [Self; 9] = [
        Self::All,
        Self::Tracks,
        Self::Artists,
        Self::Albums,
        Self::Playlists,
        Self::Shows,
        Self::Episodes,
        Self::Profiles,
        Self::Audiobooks,
    ];

    const fn operation(self) -> (&'static str, &'static str) {
        match self {
            Self::All => ("searchDesktop", ""),
            Self::Tracks => ("searchTracks", "/searchV2/tracksV2"),
            Self::Artists => ("searchArtists", "/searchV2/artists"),
            Self::Albums => ("searchAlbums", "/searchV2/albumsV2"),
            Self::Playlists => ("searchPlaylists", "/searchV2/playlists"),
            Self::Shows => ("searchPodcasts", "/searchV2/podcasts"),
            Self::Episodes => ("searchFullEpisodes", "/searchV2/episodes"),
            Self::Profiles => ("searchUsers", "/searchV2/users"),
            Self::Audiobooks => ("searchAudiobooks", "/searchV2/audiobooks"),
        }
    }
}

/// The overview of `searchDesktop`: tracks, artists, albums, playlists,
/// shows, episodes and audiobooks in that order, each row with its type.
pub fn parse_search_overview(data: &Value) -> CatalogPage {
    let mut items = Vec::new();
    for pointer in [
        "/searchV2/tracksV2",
        "/searchV2/artists",
        "/searchV2/albumsV2",
        "/searchV2/playlists",
        "/searchV2/podcasts",
        "/searchV2/episodes",
        "/searchV2/audiobooks",
    ] {
        if let Some(list) = data.pointer(pointer) {
            items.extend(
                parse_page(list, 0)
                    .items
                    .into_iter()
                    .filter(|item| item.kind != ItemKind::Unavailable),
            );
        }
    }
    CatalogPage {
        items,
        total: None,
        next_offset: None,
    }
}

pub const PAGE: u64 = 50;

fn search_variables(query: &str, offset: u64, limit: u64) -> Value {
    json!({
        "searchTerm": query,
        "offset": offset,
        "limit": limit,
        "numberOfTopResults": 5,
        "includeAudiobooks": true,
        "includeArtistHasConcertsField": false,
        "includePreReleases": true,
        "includeLocalConcertsField": false,
        "includeAuthors": false,
    })
}

pub async fn search(
    api: &Api,
    session: &Session,
    query: &str,
    kind: SearchKind,
    offset: u64,
) -> Result<CatalogPage, ApiError> {
    let (operation, pointer) = kind.operation();
    let limit = if kind == SearchKind::All { 10 } else { PAGE };
    let data = api
        .pathfinder(session, operation, search_variables(query, offset, limit))
        .await?;
    if kind == SearchKind::All {
        return Ok(parse_search_overview(&data));
    }
    let list = data
        .pointer(pointer)
        .ok_or_else(|| ApiError::Shape(format!("{operation} without results")))?;
    Ok(parse_page(list, offset))
}

/// Library filters Spotify offers (`libraryV3` `availableFilters`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryFilter {
    All,
    Playlists,
    Artists,
    Albums,
    Shows,
}

impl LibraryFilter {
    const fn id(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Playlists => Some("Playlists"),
            Self::Artists => Some("Artists"),
            Self::Albums => Some("Albums"),
            Self::Shows => Some("Podcasts & Shows"),
        }
    }
}

/// The library (or one folder of it), most recent first, as Spotify sorts it.
pub async fn library(
    api: &Api,
    session: &Session,
    filter: LibraryFilter,
    folder: Option<&str>,
    offset: u64,
) -> Result<CatalogPage, ApiError> {
    library_page(api, session, filter, folder, offset, false).await
}

/// Every playlist in the library, from all folders, at most 2000; its
/// `artist_uri` is the owner. `true` when that is the whole library
/// (2.1.3).
pub async fn library_playlists(
    api: &Api,
    session: &Session,
) -> Result<(Vec<CatalogItem>, bool), ApiError> {
    let mut playlists = Vec::new();
    let mut offset = Some(0);
    while let Some(start) = offset {
        if start >= 2000 {
            return Ok((playlists, false));
        }
        let page = library_page(api, session, LibraryFilter::Playlists, None, start, true).await?;
        playlists.extend(
            page.items
                .into_iter()
                .filter(|item| item.kind == ItemKind::Playlist),
        );
        offset = page.next_offset;
    }
    Ok((playlists, true))
}

/// Playlists the account may add tracks to, from all folders, newest first.
pub async fn editable_playlists(
    api: &Api,
    session: &Session,
) -> Result<Vec<CatalogItem>, ApiError> {
    let mut playlists = Vec::new();
    let mut offset = Some(0);
    while let Some(start) = offset.filter(|start| *start < 2000) {
        let page = library_page(api, session, LibraryFilter::Playlists, None, start, true).await?;
        playlists.extend(
            page.items
                .into_iter()
                .filter(|item| item.kind == ItemKind::Playlist && item.editable),
        );
        offset = page.next_offset;
    }
    Ok(playlists)
}

async fn library_page(
    api: &Api,
    session: &Session,
    filter: LibraryFilter,
    folder: Option<&str>,
    offset: u64,
    flatten: bool,
) -> Result<CatalogPage, ApiError> {
    let filters: Vec<&str> = filter.id().into_iter().collect();
    let data = api
        .pathfinder(
            session,
            "libraryV3",
            json!({
                "filters": filters,
                "order": (!flatten).then(|| crate::settings::load().library_order.id()),
                "textFilter": "",
                "features": ["LIKED_SONGS", "YOUR_EPISODES", "PRERELEASES"],
                "limit": PAGE,
                "offset": offset,
                "flatten": flatten,
                "expandedFolders": [],
                "folderUri": folder,
                "includeFoldersWhenFlattening": true,
            }),
        )
        .await?;
    let list = data
        .pointer("/me/libraryV3")
        .ok_or_else(|| ApiError::Shape("libraryV3".into()))?;
    Ok(parse_library_page(list, offset))
}

fn parse_library_page(list: &Value, offset: u64) -> CatalogPage {
    let mut page = parse_page(list, offset);
    // Everything listed is in the library.
    for item in &mut page.items {
        if item.saved.is_none() && !item.kind.is_playable_item() {
            item.saved = Some(true);
        }
    }
    // Folders carry their URI in the wrapper only.
    if let Some(items) = list.get("items").and_then(Value::as_array) {
        for (row, raw) in page.items.iter_mut().zip(items) {
            if row.kind == ItemKind::Unavailable && wrapper_uri(raw).contains(":folder:") {
                row.kind = ItemKind::Folder;
                row.playable = false;
                text(raw, "/item/data/name").clone_into(&mut row.name);
            }
        }
    }
    // Repair each folder against its original raw entry before filtering;
    // otherwise an expired unnamed item shifts the zip onto different rows.
    page.items
        .retain(|item| !item.name.is_empty() || item.kind == ItemKind::Unavailable);
    page
}

pub async fn liked_songs(
    api: &Api,
    session: &Session,
    offset: u64,
) -> Result<CatalogPage, ApiError> {
    let data = api
        .pathfinder(
            session,
            "fetchLibraryTracks",
            json!({ "uri": "spotify:collection:tracks", "offset": offset, "limit": PAGE }),
        )
        .await?;
    let list = data
        .pointer("/me/library/tracks")
        .ok_or_else(|| ApiError::Shape("fetchLibraryTracks".into()))?;
    let mut page = parse_page(list, offset);
    for item in &mut page.items {
        item.saved = Some(true);
    }
    Ok(page)
}

pub async fn album(
    api: &Api,
    session: &Session,
    uri: &str,
    offset: u64,
) -> Result<Collection, ApiError> {
    let data = api
        .pathfinder(
            session,
            "getAlbum",
            json!({ "uri": uri, "locale": "", "offset": offset, "limit": 300 }),
        )
        .await?;
    let album = data
        .get("albumUnion")
        .ok_or_else(|| ApiError::Shape("getAlbum".into()))?;
    Ok(parse_album_collection(album, uri, offset))
}

fn parse_album_collection(album: &Value, uri: &str, offset: u64) -> Collection {
    let page = parse_page(&album["tracksV2"], offset);
    Collection {
        uri: uri.to_owned(),
        name: text(album, "/name").to_owned(),
        subtitle: names(&album["artists"]),
        description: String::new(),
        can_edit_items: false,
        can_edit_metadata: false,
        saved: album.get("saved").and_then(Value::as_bool),
        revision: String::new(),
        format: String::new(),
        page,
    }
}

pub async fn playlist(
    api: &Api,
    session: &Session,
    uri: &str,
    offset: u64,
) -> Result<Collection, ApiError> {
    let data = api
        .pathfinder(
            session,
            "fetchPlaylist",
            json!({ "uri": uri, "offset": offset, "limit": PAGE, "enableWatchFeedEntrypoint": false }),
        )
        .await?;
    let playlist = data
        .get("playlistV2")
        .filter(|playlist| text(playlist, "/__typename") == "Playlist")
        .ok_or_else(|| ApiError::Shape("fetchPlaylist".into()))?;
    let capability = |name: &str| {
        playlist
            .pointer(&format!("/currentUserCapabilities/{name}"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    Ok(Collection {
        uri: uri.to_owned(),
        name: text(playlist, "/name").to_owned(),
        subtitle: text(playlist, "/ownerV2/data/name").to_owned(),
        description: text(playlist, "/description").to_owned(),
        can_edit_items: capability("canEditItems"),
        can_edit_metadata: capability("canEditMetadata"),
        saved: playlist.get("following").and_then(Value::as_bool),
        format: text(playlist, "/format").to_owned(),
        revision: text(playlist, "/revisionId").to_owned(),
        page: parse_page(&playlist["content"], offset),
    })
}

/// The parts of an artist page (Urh, 2.1.7): Spotify's overview shows only
/// the first page of each, so every part opens as its own complete list,
/// and All songs gathers the songs of every release.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArtistSection {
    Popular,
    Songs,
    Albums,
    Singles,
    Compilations,
    AppearsOn,
    Playlists,
    Related,
    /// The artist's biography, shown as text (2.2.0).
    About,
}

impl ArtistSection {
    /// Page order.
    pub const ALL: [Self; 9] = [
        Self::Popular,
        Self::Songs,
        Self::Albums,
        Self::Singles,
        Self::Compilations,
        Self::AppearsOn,
        Self::Playlists,
        Self::Related,
        Self::About,
    ];

    const fn key(self) -> &'static str {
        match self {
            Self::Popular => "popular",
            Self::Songs => "songs",
            Self::Albums => "albums",
            Self::Singles => "singles",
            Self::Compilations => "compilations",
            Self::AppearsOn => "appears_on",
            Self::Playlists => "playlists",
            Self::Related => "related",
            Self::About => "about",
        }
    }

    /// The translation key of the row and list name.
    #[must_use]
    pub const fn text_key(self) -> &'static str {
        match self {
            Self::Popular => "spotify_artist_popular",
            Self::Songs => "spotify_artist_songs",
            Self::Albums => "spotify_artist_albums",
            Self::Singles => "spotify_artist_singles",
            Self::Compilations => "spotify_artist_compilations",
            Self::AppearsOn => "spotify_artist_appears_on",
            Self::Playlists => "spotify_artist_playlists",
            Self::Related => "spotify_artist_related",
            Self::About => "spotify_artist_about",
        }
    }
}

/// The row of an artist page that opens `section`.
#[must_use]
pub fn artist_section_uri(artist: &str, section: ArtistSection) -> String {
    format!("apricot:{artist}:{}", section.key())
}

/// The artist and part an artist page row opens.
#[must_use]
pub fn parse_artist_section_uri(uri: &str) -> Option<(String, ArtistSection)> {
    let rest = uri.strip_prefix("apricot:spotify:artist:")?;
    let (id, key) = rest.split_once(':')?;
    let section = ArtistSection::ALL
        .into_iter()
        .find(|section| section.key() == key)?;
    (!id.is_empty()).then(|| (format!("spotify:artist:{id}"), section))
}

/// The artist a row of an artist page belongs to, so the context menu and
/// shortcuts (play, follow, radio, copy link) act on the artist there.
/// Section rows carry the artist's name in `album`.
#[must_use]
pub fn artist_of_section_row(row: &CatalogItem) -> Option<CatalogItem> {
    if row.kind != ItemKind::Section {
        return None;
    }
    let (artist, _) = parse_artist_section_uri(&row.uri)?;
    Some(CatalogItem {
        kind: ItemKind::Artist,
        uri: artist,
        name: row.album.clone(),
        subtitle: String::new(),
        album: String::new(),
        album_uri: String::new(),
        artist_uri: String::new(),
        duration_ms: None,
        playable: true,
        explicit: false,
        uid: None,
        saved: row.saved,
        count: None,
        editable: false,
        format: String::new(),
    })
}

fn artist_section_row(
    artist: &str,
    name: &str,
    section: ArtistSection,
    count: Option<u64>,
) -> CatalogItem {
    CatalogItem {
        kind: ItemKind::Section,
        uri: artist_section_uri(artist, section),
        // The window names the row in the user's language.
        name: String::new(),
        subtitle: String::new(),
        album: name.to_owned(),
        album_uri: String::new(),
        artist_uri: artist.to_owned(),
        duration_ms: None,
        playable: false,
        explicit: false,
        uid: None,
        saved: None,
        count,
        editable: false,
        format: String::new(),
    }
}

async fn artist_overview(api: &Api, session: &Session, uri: &str) -> Result<Value, ApiError> {
    let mut data = api
        .pathfinder(
            session,
            "queryArtistOverview",
            json!({ "uri": uri, "locale": "", "includePrerelease": true }),
        )
        .await?;
    data.get_mut("artistUnion")
        .map(Value::take)
        .ok_or_else(|| ApiError::Shape("queryArtistOverview".into()))
}

/// An artist page: one row for each part that has entries, with its size.
pub async fn artist(api: &Api, session: &Session, uri: &str) -> Result<Collection, ApiError> {
    let artist = artist_overview(api, session, uri).await?;
    let name = text(&artist, "/profile/name").to_owned();
    let total = |pointer: &str| {
        artist
            .pointer(&format!("{pointer}/totalCount"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let popular = artist
        .pointer("/discography/topTracks/items")
        .and_then(Value::as_array)
        .map_or(0, Vec::len) as u64;
    let releases = total("/discography/albums")
        + total("/discography/singles")
        + total("/discography/compilations");
    let about = artist_about_of(&artist);
    let has_about = !about.is_empty();
    if let Ok(mut texts) = ARTIST_ABOUT.lock() {
        texts.insert(uri.to_owned(), about);
    }
    let items = ArtistSection::ALL
        .into_iter()
        .filter_map(|section| {
            let count = match section {
                ArtistSection::Popular => Some(popular),
                // The number of songs is known only once all are read.
                ArtistSection::Songs => return (releases > 0).then_some((section, None)),
                ArtistSection::Albums => Some(total("/discography/albums")),
                ArtistSection::Singles => Some(total("/discography/singles")),
                ArtistSection::Compilations => Some(total("/discography/compilations")),
                ArtistSection::AppearsOn => Some(total("/relatedContent/appearsOn")),
                ArtistSection::Playlists => Some(total("/profile/playlistsV2")),
                ArtistSection::Related => Some(total("/relatedContent/relatedArtists")),
                // A text, not a list: no count.
                ArtistSection::About => return has_about.then_some((section, None)),
            };
            count
                .filter(|count| *count > 0)
                .map(|count| (section, Some(count)))
        })
        .map(|(section, count)| artist_section_row(uri, &name, section, count))
        .collect();
    Ok(Collection {
        uri: uri.to_owned(),
        name,
        subtitle: String::new(),
        description: String::new(),
        can_edit_items: false,
        can_edit_metadata: false,
        saved: artist.pointer("/saved").and_then(Value::as_bool),
        revision: String::new(),
        format: String::new(),
        page: CatalogPage {
            items,
            total: None,
            next_offset: None,
        },
    })
}

/// What Spotify's About shows for an artist (2.2.0): the listeners,
/// followers and world rank, where people listen, the biography and the
/// artist's links.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ArtistAbout {
    pub monthly_listeners: Option<u64>,
    pub followers: Option<u64>,
    pub world_rank: Option<u64>,
    /// City, country and listeners, most listeners first.
    pub top_cities: Vec<(String, String, u64)>,
    pub biography: String,
    /// Name and address.
    pub links: Vec<(String, String)>,
}

impl ArtistAbout {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.monthly_listeners.is_none()
            && self.followers.is_none()
            && self.world_rank.is_none()
            && self.top_cities.is_empty()
            && self.biography.is_empty()
            && self.links.is_empty()
    }
}

/// `artistUnion` of `queryArtistOverview` as About.
fn artist_about_of(artist: &Value) -> ArtistAbout {
    let number = |pointer: &str| {
        artist
            .pointer(pointer)
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
    };
    let top_cities = artist
        .pointer("/stats/topCities/items")
        .and_then(Value::as_array)
        .map(|cities| {
            cities
                .iter()
                .filter_map(|city| {
                    let name = text(city, "/city").trim().to_owned();
                    let listeners = city.get("numberOfListeners").and_then(Value::as_u64)?;
                    (!name.is_empty())
                        .then(|| (name, text(city, "/country").trim().to_owned(), listeners))
                })
                .collect()
        })
        .unwrap_or_default();
    let links = artist
        .pointer("/profile/externalLinks/items")
        .and_then(Value::as_array)
        .map(|links| {
            links
                .iter()
                .filter_map(|link| {
                    let url = text(link, "/url").trim().to_owned();
                    (!url.is_empty()).then(|| (text(link, "/name").trim().to_owned(), url))
                })
                .collect()
        })
        .unwrap_or_default();
    ArtistAbout {
        monthly_listeners: number("/stats/monthlyListeners"),
        followers: number("/stats/followers"),
        world_rank: number("/stats/worldRank"),
        top_cities,
        biography: plain_text(text(artist, "/profile/biography/text")),
        links,
    }
}

/// About of artist pages opened in this run.
static ARTIST_ABOUT: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, ArtistAbout>>,
> = std::sync::LazyLock::new(Default::default);

/// About of an artist whose page was opened, when Spotify gave anything.
#[must_use]
pub fn artist_about(artist: &str) -> Option<ArtistAbout> {
    ARTIST_ABOUT
        .lock()
        .ok()?
        .get(artist)
        .filter(|about| !about.is_empty())
        .cloned()
}

/// Spotify's biography markup as plain text: links keep their words, tags
/// go, the usual entities become characters.
fn plain_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    let text = [
        ("&nbsp;", " "),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&#x27;", "'"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&amp;", "&"),
    ]
    .into_iter()
    .fold(text, |text, (entity, character)| {
        text.replace(entity, character)
    });
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A page of release groups (`{items: [{releases: {items}}], totalCount}`):
/// each group shows its first release.
fn release_page(list: &Value, offset: u64) -> CatalogPage {
    let items: Vec<CatalogItem> = list
        .get("items")
        .and_then(Value::as_array)
        .map(|groups| {
            groups
                .iter()
                .map(|group| parse_item(group.pointer("/releases/items/0").unwrap_or(group)))
                .collect()
        })
        .unwrap_or_default();
    let total = list.get("totalCount").and_then(Value::as_u64);
    let end = offset + items.len() as u64;
    CatalogPage {
        next_offset: total
            .filter(|total| end < *total && !items.is_empty())
            .map(|_| end),
        items,
        total,
    }
}

/// One part of an artist page, page by page; All songs comes whole.
pub async fn artist_section(
    api: &std::sync::Arc<Api>,
    session: &Session,
    artist: &str,
    section: ArtistSection,
    offset: u64,
) -> Result<CatalogPage, ApiError> {
    let paged = |operation: &'static str| {
        api.pathfinder(
            session,
            operation,
            json!({ "uri": artist, "offset": offset, "limit": PAGE, "order": "DATE_DESC" }),
        )
    };
    let mut page = match section {
        ArtistSection::Popular => {
            let overview = artist_overview(api, session, artist).await?;
            let mut page = parse_page(&overview["discography"]["topTracks"], 0);
            page.next_offset = None;
            page
        }
        ArtistSection::Songs => {
            let items = artist_songs(api, session, artist).await?;
            CatalogPage {
                total: Some(items.len() as u64),
                items,
                next_offset: None,
            }
        }
        ArtistSection::Albums => release_page(
            &paged("queryArtistDiscographyAlbums").await?["artistUnion"]["discography"]["albums"],
            offset,
        ),
        ArtistSection::Singles => release_page(
            &paged("queryArtistDiscographySingles").await?["artistUnion"]["discography"]["singles"],
            offset,
        ),
        ArtistSection::Compilations => release_page(
            &paged("queryArtistDiscographyCompilations").await?["artistUnion"]["discography"]["compilations"],
            offset,
        ),
        ArtistSection::AppearsOn => release_page(
            &paged("queryArtistAppearsOn").await?["artistUnion"]["relatedContent"]["appearsOn"],
            offset,
        ),
        ArtistSection::Playlists => parse_page(
            &paged("queryArtistPlaylists").await?["artistUnion"]["profile"]["playlistsV2"],
            offset,
        ),
        // About is shown from the page's text and has no list.
        ArtistSection::About => CatalogPage {
            items: Vec::new(),
            total: Some(0),
            next_offset: None,
        },
        ArtistSection::Related => {
            let data = api
                .pathfinder(
                    session,
                    "queryArtistRelated",
                    json!({ "uri": artist, "locale": "" }),
                )
                .await?;
            let mut page = parse_page(&data["artistUnion"]["relatedContent"]["relatedArtists"], 0);
            page.next_offset = None;
            page
        }
    };
    page.items
        .retain(|item| item.kind != ItemKind::Unavailable || !item.uri.is_empty());
    Ok(page)
}

/// All songs of artists read in this run of the application, newest first:
/// reading them takes one request for every release, so they are read once.
static ARTIST_SONGS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Vec<CatalogItem>>>,
> = std::sync::LazyLock::new(Default::default);

/// Releases read at the same time for All songs.
const RELEASES_AT_ONCE: usize = 6;

/// The songs of the artist's All songs list, when it was read: playing one
/// of them continues with the next one of that list.
#[must_use]
pub fn artist_song_uris(artist: &str) -> Option<Vec<String>> {
    let lists = ARTIST_SONGS.lock().ok()?;
    let items = lists.get(artist)?;
    Some(items.iter().map(|item| item.uri.clone()).collect())
}

/// Keeps an artist's All songs list as if it had been read (tests).
#[cfg(test)]
pub(crate) fn remember_artist_songs(artist: &str, items: Vec<CatalogItem>) {
    ARTIST_SONGS
        .lock()
        .unwrap()
        .insert(artist.to_owned(), items);
}

/// Every song of every album, single, EP and compilation of the artist,
/// newest release first and in release order; a song that came out again
/// with the same title and length is listed once, at its newest release.
pub async fn artist_songs(
    api: &std::sync::Arc<Api>,
    session: &Session,
    artist: &str,
) -> Result<Vec<CatalogItem>, ApiError> {
    if let Some(items) = ARTIST_SONGS
        .lock()
        .ok()
        .and_then(|lists| lists.get(artist).cloned())
    {
        return Ok(items);
    }
    let mut releases: Vec<(String, String)> = Vec::new();
    let mut offset = 0;
    loop {
        let data = api
            .pathfinder(
                session,
                "queryArtistDiscographyAll",
                json!({ "uri": artist, "offset": offset, "limit": PAGE, "order": "DATE_DESC" }),
            )
            .await?;
        let page = release_page(&data["artistUnion"]["discography"]["all"], offset);
        releases.extend(
            page.items
                .into_iter()
                .filter(|release| !release.uri.is_empty())
                .map(|release| (release.uri, release.name)),
        );
        match page.next_offset {
            Some(next) if next > offset && releases.len() < 5_000 => offset = next,
            _ => break,
        }
    }
    let limit = std::sync::Arc::new(tokio::sync::Semaphore::new(RELEASES_AT_ONCE));
    let mut reads = tokio::task::JoinSet::new();
    for (index, (uri, name)) in releases.into_iter().enumerate() {
        let api = api.clone();
        let session = session.clone();
        let limit = limit.clone();
        reads.spawn(async move {
            let _permit = limit.acquire_owned().await;
            let tracks = release_tracks(&api, &session, &uri, &name).await;
            (index, tracks)
        });
    }
    let mut read = Vec::new();
    while let Some(joined) = reads.join_next().await {
        let (index, tracks) =
            joined.map_err(|error| ApiError::Shape(format!("release read stopped: {error}")))?;
        read.push((index, tracks?));
    }
    read.sort_by_key(|(index, _)| *index);
    let items = unique_songs(read.into_iter().flat_map(|(_, tracks)| tracks));
    if let Ok(mut lists) = ARTIST_SONGS.lock() {
        lists.insert(artist.to_owned(), items.clone());
    }
    Ok(items)
}

/// The tracks of one release, with the release as their album.
async fn release_tracks(
    api: &Api,
    session: &Session,
    uri: &str,
    name: &str,
) -> Result<Vec<CatalogItem>, ApiError> {
    let mut tracks = Vec::new();
    let mut offset = 0;
    loop {
        let data = api
            .pathfinder(
                session,
                "queryAlbumTracks",
                json!({ "uri": uri, "offset": offset, "limit": 300 }),
            )
            .await?;
        let page = parse_page(&data["albumUnion"]["tracksV2"], offset);
        for mut track in page.items {
            if track.album_uri.is_empty() {
                uri.clone_into(&mut track.album_uri);
                name.clone_into(&mut track.album);
            }
            // Album uids are not Connect's: these songs play by URI.
            track.uid = None;
            tracks.push(track);
        }
        match page.next_offset {
            Some(next) if next > offset => offset = next,
            _ => return Ok(tracks),
        }
    }
}

/// Keeps the first of songs with the same URI, or with the same title
/// (ignoring case) and a length within a second.
fn unique_songs(songs: impl IntoIterator<Item = CatalogItem>) -> Vec<CatalogItem> {
    let mut kept = Vec::new();
    let mut uris = std::collections::HashSet::new();
    let mut lengths: std::collections::HashMap<String, Vec<u64>> = std::collections::HashMap::new();
    for song in songs {
        if !song.kind.is_playable_item() || !uris.insert(song.uri.clone()) {
            continue;
        }
        let known = lengths.entry(song.name.to_lowercase()).or_default();
        if let Some(length) = song.duration_ms {
            if known.iter().any(|other| other.abs_diff(length) <= 1_000) {
                continue;
            }
            known.push(length);
        }
        kept.push(song);
    }
    kept
}

pub async fn show(
    api: &Api,
    session: &Session,
    uri: &str,
    offset: u64,
) -> Result<Collection, ApiError> {
    let data = api
        .pathfinder(
            session,
            "queryPodcastEpisodes",
            json!({ "uri": uri, "offset": offset, "limit": PAGE }),
        )
        .await?;
    let show = data
        .get("podcastUnionV2")
        .ok_or_else(|| ApiError::Shape("queryPodcastEpisodes".into()))?;
    Ok(Collection {
        uri: uri.to_owned(),
        name: text(show, "/name").to_owned(),
        subtitle: text(show, "/publisher/name").to_owned(),
        description: String::new(),
        can_edit_items: false,
        can_edit_metadata: false,
        saved: None,
        revision: String::new(),
        format: String::new(),
        page: parse_page(&show["episodesV2"], offset),
    })
}

/// Formats of personalised playlists, where Spotify offers "Hide song"
/// (P0 evidence 7).
pub const PERSONALISED_FORMATS: [&str; 12] = [
    "artistsets",
    "artist-mix-reader",
    "blend",
    "daily-mix",
    "daylist",
    "discover-weekly",
    "descripto",
    "inspiredby-mix",
    "on-repeat",
    "release-radar",
    "repeat-rewind",
    "topic-mix",
];

/// Songs the account hid ("ban" collection, kept without context).
pub async fn hidden_songs(api: &Api, session: &Session) -> Result<Vec<String>, ApiError> {
    let mut body = Vec::new();
    crate::api::proto::bytes(&mut body, 1, session.username().as_bytes());
    crate::api::proto::bytes(&mut body, 2, b"ban");
    crate::api::proto::int(&mut body, 4, 2000);
    let answer = api
        .collection(session, "/collection/v2/paging", body)
        .await?;
    Ok(answer
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    !item
                        .get("isRemoved")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .filter_map(|item| item.get("uri").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default())
}

/// Saved state of `uris` (Liked Songs for tracks, the library for others).
pub async fn saved(api: &Api, session: &Session, uris: &[String]) -> Result<Vec<bool>, ApiError> {
    let playlists: Vec<String> = uris
        .iter()
        .filter(|uri| uri.starts_with("spotify:playlist:"))
        .cloned()
        .collect();
    let other: Vec<String> = uris
        .iter()
        .filter(|uri| !uri.starts_with("spotify:playlist:"))
        .cloned()
        .collect();
    let mut states = std::collections::HashMap::new();
    if !playlists.is_empty() {
        // Playlists belong to the rootlist. The entity lookup does not supply
        // their saved state; publication/public attributes are separate.
        let root = api
            .spclient(
                session,
                reqwest::Method::GET,
                &format!(
                    "/playlist/v2/user/{}/rootlist?decorate=revision&from=0&length=10000",
                    session.username()
                ),
                None,
            )
            .await?;
        states.extend(
            playlists
                .iter()
                .cloned()
                .zip(parse_playlist_saved_states(&root, &playlists)?),
        );
    }
    if !other.is_empty() {
        let data = api
            .pathfinder(session, "areEntitiesInLibrary", json!({ "uris": other }))
            .await?;
        states.extend(
            other
                .iter()
                .cloned()
                .zip(parse_saved_states(&data, &other)?),
        );
    }
    Ok(uris.iter().map(|uri| states[uri]).collect())
}

fn parse_playlist_saved_states(root: &Value, uris: &[String]) -> Result<Vec<bool>, ApiError> {
    if root.pointer("/contents/truncated").and_then(Value::as_bool) == Some(true) {
        return Err(ApiError::Shape(
            "Incomplete playlist library response".into(),
        ));
    }
    let items = root
        .pointer("/contents/items")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::Shape("Playlist library items missing".into()))?;
    let entries: std::collections::HashSet<&str> = items
        .iter()
        .map(|item| {
            item.get("uri")
                .and_then(Value::as_str)
                .filter(|uri| !uri.is_empty())
                .ok_or_else(|| ApiError::Shape("Playlist library entry URI missing".into()))
        })
        .collect::<Result<_, _>>()?;
    Ok(uris
        .iter()
        .map(|uri| entries.contains(uri.as_str()))
        .collect())
}

pub fn parse_saved_states(data: &Value, uris: &[String]) -> Result<Vec<bool>, ApiError> {
    let lookup = data
        .get("lookup")
        .and_then(Value::as_array)
        .filter(|entries| entries.len() == uris.len())
        .ok_or_else(|| ApiError::Shape("Incomplete saved state response".into()))?;
    lookup
        .iter()
        .zip(uris)
        .map(|(entry, uri)| {
            if let Some(actual) = entry.pointer("/data/uri").and_then(Value::as_str)
                && actual != uri
            {
                return Err(ApiError::Shape(
                    "Saved state response order mismatch".into(),
                ));
            }
            entry
                .pointer("/data/saved")
                .or_else(|| entry.pointer("/data/following"))
                .or_else(|| entry.pointer("/data/isFollowing"))
                .and_then(Value::as_bool)
                .ok_or_else(|| ApiError::Shape("Unknown saved state".into()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artist_page_rows_open_their_part_and_stand_for_the_artist() {
        let artist = "spotify:artist:6UOk7DmvqlzWmo6gjhZvn6";
        for section in ArtistSection::ALL {
            let uri = artist_section_uri(artist, section);
            assert_eq!(
                parse_artist_section_uri(&uri),
                Some((artist.to_owned(), section))
            );
        }
        assert_eq!(
            parse_artist_section_uri("apricot:spotify:user:x:playlists"),
            None
        );
        assert_eq!(
            parse_artist_section_uri("apricot:spotify:artist::songs"),
            None
        );
        assert_eq!(
            parse_artist_section_uri("apricot:spotify:artist:a:other"),
            None
        );
        let row = artist_section_row(artist, "Da Tweekaz", ArtistSection::Singles, Some(157));
        assert_eq!((row.kind, row.count), (ItemKind::Section, Some(157)));
        let owner = artist_of_section_row(&row).unwrap();
        assert_eq!(
            (owner.kind, owner.uri.as_str(), owner.name.as_str()),
            (ItemKind::Artist, artist, "Da Tweekaz")
        );
        let mut other = row.clone();
        other.uri = "spotify:section:home".into();
        assert!(artist_of_section_row(&other).is_none());
    }

    #[test]
    fn a_biography_reads_as_plain_text() {
        assert_eq!(
            plain_text(
                "<p>Da Tweekaz &amp; friends</p>\n  <a href=\"spotify:artist:x\">Sub Zero</a> &quot;live&quot;  "
            ),
            "Da Tweekaz & friends\nSub Zero \"live\""
        );
        assert_eq!(plain_text(""), "");
    }

    #[test]
    fn release_pages_show_each_first_release_with_its_type_and_page_on() {
        let list = json!({ "totalCount": 157, "items": [
            { "releases": { "items": [{ "uri": "spotify:album:a", "name": "FUEGO",
                "type": "SINGLE", "date": { "year": 2026 } }] } },
            { "releases": { "items": [{ "uri": "spotify:album:b", "name": "Shine",
                "type": "EP", "date": { "year": 2025 } }] } }
        ]});
        let page = release_page(&list, 50);
        let rows: Vec<_> = page
            .items
            .iter()
            .map(|item| {
                (
                    item.kind,
                    item.name.as_str(),
                    item.format.as_str(),
                    item.subtitle.as_str(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (ItemKind::Album, "FUEGO", "SINGLE", "2026"),
                (ItemKind::Album, "Shine", "EP", "2025")
            ]
        );
        assert_eq!((page.total, page.next_offset), (Some(157), Some(52)));
        let final_page = release_page(&json!({ "totalCount": 52, "items": list["items"] }), 50);
        assert_eq!(final_page.next_offset, None);
    }

    #[test]
    fn all_songs_keep_the_newest_release_of_a_song_that_came_out_again() {
        let song = |uri: &str, name: &str, length: u64| CatalogItem {
            kind: ItemKind::Track,
            uri: uri.into(),
            name: name.into(),
            subtitle: String::new(),
            album: String::new(),
            album_uri: String::new(),
            artist_uri: String::new(),
            duration_ms: Some(length),
            playable: true,
            explicit: false,
            uid: None,
            saved: None,
            count: None,
            editable: false,
            format: String::new(),
        };
        let kept = unique_songs([
            song("spotify:track:new", "Sun Goes Down", 180_000),
            // The same recording on an older album.
            song("spotify:track:old", "sun goes down", 180_600),
            // Another mix has another length.
            song("spotify:track:mix", "Sun Goes Down", 260_000),
            song("spotify:track:new", "Sun Goes Down", 180_000),
            song("spotify:track:other", "Fuego", 170_000),
        ]);
        let uris: Vec<_> = kept.iter().map(|song| song.uri.as_str()).collect();
        assert_eq!(
            uris,
            [
                "spotify:track:new",
                "spotify:track:mix",
                "spotify:track:other"
            ]
        );
    }

    #[test]
    fn playlist_library_membership_is_distinct_from_profile_publication() {
        let root = json!({"contents":{"truncated":false,"items":[
            {"uri":"spotify:playlist:private","attributes":{"public":false}},
            {"uri":"spotify:playlist:published","attributes":{"public":true}},
            {"uri":"spotify:playlist:unknown_public","attributes":{}},
            {"uri":"spotify:user:u:folder:f"}
        ]}});
        let uris = [
            "spotify:playlist:unknown_public",
            "spotify:playlist:missing",
            "spotify:playlist:private",
            "spotify:playlist:published",
            "spotify:playlist:private",
        ]
        .map(str::to_owned);
        assert_eq!(
            parse_playlist_saved_states(&root, &uris),
            Ok(vec![true, false, true, true, true])
        );
        assert_eq!(
            parse_playlist_saved_states(&json!({"contents":{"items":[]}}), &uris),
            Ok(vec![false; 5])
        );
        assert!(
            parse_playlist_saved_states(&json!({"contents":{"truncated":true,"items":[]}}), &uris)
                .is_err()
        );
        assert!(parse_playlist_saved_states(&json!({}), &uris).is_err());
        assert!(parse_playlist_saved_states(&json!({"contents":{"items":[{}]}}), &uris).is_err());
    }

    #[test]
    fn missing_follow_state_is_not_interpreted_as_not_followed() {
        let uris = vec!["spotify:artist:a".to_owned()];
        assert!(parse_saved_states(&json!({"lookup":[{"data":{}}]}), &uris).is_err());
        assert!(parse_saved_states(&json!({"lookup":[]}), &uris).is_err());
        assert_eq!(
            parse_saved_states(
                &json!({"lookup":[{"data":{"uri":"spotify:artist:a","following":true}}]}),
                &uris
            )
            .unwrap(),
            [true]
        );
        assert_eq!(
            parse_saved_states(&json!({"lookup":[{"data":{"saved":false}}]}), &uris).unwrap(),
            [false]
        );
    }

    #[test]
    fn unnamed_library_entry_does_not_shift_folder_identity() {
        let list = json!({ "totalCount": 3, "items": [
            { "item": { "data": { "__typename": "Album", "uri": "spotify:album:expired", "name": "" } } },
            { "item": { "_uri": "spotify:user:owner:folder:one", "data": { "__typename": "Unknown", "name": "My folder" } } },
            { "item": { "data": { "__typename": "Playlist", "uri": "spotify:playlist:p", "name": "Playlist" } } }
        ] });
        let page = parse_library_page(&list, 0);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].kind, ItemKind::Folder);
        assert_eq!(page.items[0].uri, "spotify:user:owner:folder:one");
        assert_eq!(page.items[0].name, "My folder");
        assert_eq!(page.items[1].kind, ItemKind::Playlist);
        assert_eq!(page.items[1].name, "Playlist");
    }

    fn track(uri: &str, name: &str) -> Value {
        json!({
            "__typename": "TrackResponseWrapper",
            "data": {
                "__typename": "Track",
                "uri": uri,
                "name": name,
                "artists": { "items": [
                    { "profile": { "name": "Queen" }, "uri": "spotify:artist:q" },
                    { "profile": { "name": "Bowie" }, "uri": "spotify:artist:b" }
                ]},
                "albumOfTrack": { "name": "Hot Space", "uri": "spotify:album:h" },
                "duration": { "totalMilliseconds": 248_000 },
                "contentRating": { "label": "EXPLICIT" },
                "playability": { "playable": true, "reason": "PLAYABLE" }
            }
        })
    }

    #[test]
    fn a_track_row_has_artists_album_length_and_occurrence() {
        let mut entry =
            json!({ "uid": "ab12", "itemV2": track("spotify:track:1", "Under Pressure") });
        entry["addedAt"] = json!({ "isoString": "2020-01-01T00:00:00Z" });
        let item = parse_item(&entry);
        assert_eq!(item.kind, ItemKind::Track);
        assert_eq!(item.name, "Under Pressure");
        assert_eq!(item.subtitle, "Queen, Bowie");
        assert_eq!(item.album, "Hot Space");
        assert_eq!(item.artist_uri, "spotify:artist:q");
        assert_eq!(item.duration_ms, Some(248_000));
        assert!(item.explicit && item.playable);
        assert_eq!(item.uid.as_deref(), Some("ab12"));
    }

    #[test]
    fn restricted_entries_are_unavailable_rows_with_their_uri() {
        let entry = json!({
            "entity": { "_uri": "spotify:episode:x", "data": { "__typename": "RestrictedContent" } },
            "uid": "u1"
        });
        let item = parse_item(&entry);
        assert_eq!(item.kind, ItemKind::Unavailable);
        assert_eq!(item.uri, "spotify:episode:x");
        assert!(!item.playable);
    }

    #[test]
    fn collections_and_people_get_their_subtitles() {
        let playlist = parse_item(&json!({ "data": {
            "__typename": "Playlist", "uri": "spotify:playlist:p", "name": "Mix",
            "ownerV2": { "data": { "__typename": "User", "name": "Spotify" } }
        }}));
        assert_eq!(
            (playlist.kind, playlist.subtitle.as_str()),
            (ItemKind::Playlist, "Spotify")
        );
        let show = parse_item(&json!({ "data": {
            "__typename": "Podcast", "uri": "spotify:show:s", "name": "History",
            "publisher": { "name": "Goalhanger" }
        }}));
        assert_eq!(
            (show.kind, show.subtitle.as_str()),
            (ItemKind::Show, "Goalhanger")
        );
        let artist = parse_item(&json!({ "data": {
            "__typename": "Artist", "uri": "spotify:artist:a", "profile": { "name": "Queen" }
        }}));
        assert_eq!(
            (artist.kind, artist.name.as_str()),
            (ItemKind::Artist, "Queen")
        );
        let liked = parse_item(&json!({ "item": {
            "_uri": "spotify:collection:tracks",
            "data": { "__typename": "PseudoPlaylist", "name": "Liked Songs", "count": 12 }
        }}));
        assert_eq!((liked.kind, liked.count), (ItemKind::LikedSongs, Some(12)));
    }

    #[test]
    fn artist_page_entries_without_typename_use_their_uri() {
        let top = json!({ "uid": "u", "track": {
            "uri": "spotify:track:t", "name": "Don't Stop Me Now",
            "artists": { "items": [{ "profile": { "name": "Queen" }, "uri": "spotify:artist:q" }] },
            "duration": { "totalMilliseconds": 209_413 },
            "playability": { "playable": true }
        }});
        let item = parse_item(&top);
        assert_eq!(
            (item.kind, item.subtitle.as_str()),
            (ItemKind::Track, "Queen")
        );
        assert_eq!(item.uid.as_deref(), Some("u"));
        let release = json!({ "uri": "spotify:album:a", "name": "Queen II", "type": "ALBUM", "date": { "year": 2026 } });
        let album = parse_item(&release);
        assert_eq!(
            (album.kind, album.subtitle.as_str()),
            (ItemKind::Album, "2026")
        );
        let related = json!({ "uri": "spotify:artist:b", "profile": { "name": "Bowie" } });
        assert_eq!(parse_item(&related).name, "Bowie");
        assert_eq!(
            ItemKind::from_uri("spotify:user:x:folder:1"),
            Some(ItemKind::Folder)
        );
        assert_eq!(
            ItemKind::from_uri("spotify:collection:tracks"),
            Some(ItemKind::LikedSongs)
        );
    }

    #[test]
    fn sections_keep_titles_entries_and_category_pages() {
        let list = json!({ "items": [
            { "uri": "spotify:section:a", "data": { "title": { "transformedLabel": "Made For You" } },
              "sectionItems": { "items": [
                { "uri": "spotify:playlist:m", "content": { "__typename": "PlaylistResponseWrapper", "data": {
                    "__typename": "Playlist", "uri": "spotify:playlist:m", "name": "Daily Mix 1", "format": "daily-mix" } } },
                { "uri": "spotify:xlink:x", "content": { "__typename": "BrowseXlinkResponseWrapper", "data": {} } }
              ] } },
            { "uri": "spotify:section:b", "data": { "title": { "transformedLabel": "Browse all" } },
              "sectionItems": { "items": [
                { "uri": "spotify:page:p", "content": { "__typename": "BrowseSectionContainerWrapper", "data": {
                    "__typename": "BrowseSectionContainer", "data": { "cardRepresentation": { "title": { "transformedLabel": "Music" } } } } } }
              ] } },
            { "uri": "spotify:section:c", "data": { "title": null }, "sectionItems": { "items": [] } }
        ]});
        let sections = parse_sections(&list);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "Made For You");
        assert_eq!(sections[0].items.len(), 1);
        assert_eq!(sections[0].items[0].format, "daily-mix");
        assert_eq!(sections[1].items[0].kind, ItemKind::Page);
        assert_eq!(sections[1].items[0].name, "Music");
    }

    #[test]
    fn synced_lyrics_become_lrc_lines() {
        let answer = json!({ "lyrics": {
            "syncType": "LINE_SYNCED",
            "providerDisplayName": "Musixmatch",
            "lines": [
                { "startTimeMs": "140", "words": "Is this the real life?" },
                { "startTimeMs": "65230", "words": "\u{266a}" },
                { "startTimeMs": "125990", "words": "Mama" }
            ]
        }});
        let (text, provider) = lyrics_text(&answer).unwrap();
        assert_eq!(text, "[00:00.14]Is this the real life?\n[02:05.99]Mama");
        assert_eq!(provider, "Musixmatch");
        let plain = json!({ "lyrics": { "syncType": "UNSYNCED", "lines": [{ "words": "Line" }] } });
        assert_eq!(lyrics_text(&plain).unwrap().0, "Line");
        assert_eq!(lyrics_text(&json!({})), None);
    }

    #[test]
    fn transcripts_are_timed_lines_with_the_speaker_of_each_turn() {
        let answer = json!({ "section": [
            { "startMs": 0, "fallback": { "sentence": { "text": "Music playing" } }, "musicClosedCaption": { "text": "Music playing" } },
            { "startMs": 13840, "title": { "title": "Speaker 1" } },
            { "startMs": 13840, "text": { "sentence": { "text": "What's going on?" } } },
            { "startMs": 75200, "text": { "sentence": { "text": "Good." } } }
        ]});
        assert_eq!(
            transcript_text(&answer).as_deref(),
            Some(
                "[00:00.00](Music playing)
[00:13.84]Speaker 1: What's going on?
[01:15.20]Good."
            )
        );
        assert_eq!(transcript_text(&json!({ "section": [] })), None);
    }

    #[test]
    fn profile_rows_are_playlists_with_owner_and_followed_accounts() {
        let answer = json!({ "public_playlists": [
            { "uri": "spotify:playlist:p", "name": "Mix", "owner_name": "Ana", "owner_uri": "spotify:user:ana", "is_following": true }
        ], "profiles": [
            { "uri": "spotify:artist:a", "name": "Queen" },
            { "uri": "spotify:user:bob", "name": "Bob" },
            { "uri": "spotify:unknown:x", "name": "?" }
        ]});
        let playlists = profile_rows(&answer, "public_playlists");
        assert_eq!(playlists[0].kind, ItemKind::Playlist);
        assert_eq!(playlists[0].subtitle, "Ana");
        assert_eq!(playlists[0].artist_uri, "spotify:user:ana");
        assert_eq!(playlists[0].saved, Some(true));
        let following = profile_rows(&answer, "profiles");
        let kinds: Vec<_> = following
            .iter()
            .map(|item| (item.kind, item.name.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [(ItemKind::Artist, "Queen"), (ItemKind::User, "Bob")]
        );
        assert_eq!(user_name("spotify:user:bob").unwrap(), "bob");
        assert!(user_name("spotify:user:bob:collection").is_err());
    }

    #[test]
    fn playlist_rows_name_their_owner() {
        let row = parse_item(&json!({ "data": {
            "__typename": "Playlist", "uri": "spotify:playlist:p", "name": "P",
            "ownerV2": { "data": { "__typename": "User", "name": "Ana", "uri": "spotify:user:ana" } }
        }}));
        assert_eq!(row.subtitle, "Ana");
        assert_eq!(row.artist_uri, "spotify:user:ana");
    }

    #[test]
    fn pages_know_where_the_next_page_starts() {
        let list = json!({ "items": [track("spotify:track:1", "A"), track("spotify:track:2", "B")], "totalCount": 5 });
        assert_eq!(parse_page(&list, 0).next_offset, Some(2));
        assert_eq!(parse_page(&list, 3).next_offset, None);
        let next =
            json!({ "items": [track("spotify:track:1", "A")], "pagingInfo": { "nextOffset": 40 } });
        assert_eq!(parse_page(&next, 0).next_offset, Some(40));
        let empty = json!({ "items": [], "totalCount": 9 });
        assert_eq!(parse_page(&empty, 0).next_offset, None);
    }

    #[test]
    fn the_overview_lists_sections_in_order_without_unavailable_rows() {
        let data = json!({ "searchV2": {
            "tracksV2": { "items": [{ "item": track("spotify:track:1", "A") }] },
            "artists": { "items": [{ "data": { "__typename": "Artist", "uri": "spotify:artist:a", "profile": { "name": "Queen" } } }] },
            "albumsV2": { "items": [{ "data": { "__typename": "Album", "uri": "spotify:album:b", "name": "B", "artists": { "items": [] } } }] },
            "episodes": { "items": [{ "data": { "__typename": "RestrictedContent" } }] }
        }});
        let kinds: Vec<_> = parse_search_overview(&data)
            .items
            .iter()
            .map(|item| item.kind)
            .collect();
        assert_eq!(kinds, [ItemKind::Track, ItemKind::Artist, ItemKind::Album]);
    }
    #[test]
    fn large_albums_keep_the_offset_of_unread_tracks() {
        let rows: Vec<_> = (0..300).map(|i| json!({"track":{"uri":format!("spotify:track:t{i}"),"name":format!("Track {i}")}})).collect();
        let album = json!({"name":"Large album","tracksV2":{"items":rows,"totalCount":350}});
        let first = parse_album_collection(&album, "spotify:album:a", 0);
        assert_eq!(first.page.items.len(), 300);
        assert_eq!(first.page.next_offset, Some(300));
        let last = json!({"tracksV2":{"items":[{"track":{"uri":"spotify:track:last","name":"Last"}}],"totalCount":301}});
        assert_eq!(
            parse_album_collection(&last, "spotify:album:a", 300)
                .page
                .next_offset,
            None
        );
    }
}
