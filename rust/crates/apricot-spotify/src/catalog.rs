//! Spotify catalog and library reads (`docs/SPOTIFY_PLAN.md` 3.2, 4.2):
//! search, library, Liked Songs, albums, playlists, artists and shows, from
//! pathfinder answers. Parsing is tolerant: an unknown or restricted entry
//! becomes an unavailable row, never a crash or a silently empty list.

#![allow(clippy::missing_errors_doc)] // Every read fails only with `ApiError`.

use librespot_core::Session;
use serde_json::{Value, json};

use crate::api::{Api, ApiError};

/// What a row is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogItem {
    pub kind: ItemKind,
    pub uri: String,
    pub name: String,
    /// Artists, owner or publisher.
    pub subtitle: String,
    pub album: String,
    pub album_uri: String,
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
    }
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
    Audiobooks,
}

impl SearchKind {
    pub const ALL: [Self; 8] = [
        Self::All,
        Self::Tracks,
        Self::Artists,
        Self::Albums,
        Self::Playlists,
        Self::Shows,
        Self::Episodes,
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
                "order": null,
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
    Ok(page)
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

pub async fn album(api: &Api, session: &Session, uri: &str) -> Result<Collection, ApiError> {
    let data = api
        .pathfinder(
            session,
            "getAlbum",
            json!({ "uri": uri, "locale": "", "offset": 0, "limit": 300 }),
        )
        .await?;
    let album = data
        .get("albumUnion")
        .ok_or_else(|| ApiError::Shape("getAlbum".into()))?;
    let mut page = parse_page(&album["tracksV2"], 0);
    page.next_offset = None;
    Ok(Collection {
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
    })
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

/// An artist's overview: popular tracks, then albums, singles and
/// compilations (each opens its release).
pub async fn artist(api: &Api, session: &Session, uri: &str) -> Result<Collection, ApiError> {
    let data = api
        .pathfinder(
            session,
            "queryArtistOverview",
            json!({ "uri": uri, "locale": "", "includePrerelease": true }),
        )
        .await?;
    let artist = data
        .get("artistUnion")
        .ok_or_else(|| ApiError::Shape("queryArtistOverview".into()))?;
    let mut items = parse_page(&artist["discography"]["topTracks"], 0).items;
    for section in ["albums", "singles", "compilations"] {
        let releases = artist
            .pointer(&format!("/discography/{section}/items"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for release in releases {
            // Each release group lists its first release.
            let release = release
                .pointer("/releases/items/0")
                .cloned()
                .unwrap_or(release);
            items.push(parse_item(&release));
        }
    }
    if let Some(related) = artist
        .pointer("/relatedContent/relatedArtists/items")
        .and_then(Value::as_array)
    {
        items.extend(related.iter().map(parse_item));
    }
    items.retain(|item| item.kind != ItemKind::Unavailable || !item.uri.is_empty());
    Ok(Collection {
        uri: uri.to_owned(),
        name: text(artist, "/profile/name").to_owned(),
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
    let data = api
        .pathfinder(session, "areEntitiesInLibrary", json!({ "uris": uris }))
        .await?;
    Ok(data
        .get("lookup")
        .and_then(Value::as_array)
        .map(|lookup| {
            lookup
                .iter()
                .map(|entry| {
                    entry
                        .pointer("/data/saved")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
