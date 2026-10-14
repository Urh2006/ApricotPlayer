//! Spotify lists (Windows `spotify_browse_win32.rs`, `docs/SPOTIFY_PLAN.md`
//! 4.2, 4.3): search results, the library, Liked Songs, playlists, albums,
//! artists, shows, Home, Daily Mixes, Recently played, Top, Browse and
//! profiles. Every opened list is a frame on a stack, so Escape goes back one
//! level to the same row. Lists load in pages; reaching the last row loads
//! the next page. Answers that arrive late are dropped. Focus moves only
//! when the user opens or leaves a list.

use std::time::{Duration, Instant};

use apricot_app::context_menu::{ContextCommand, ContextMenuEntry};
use apricot_app::spotify::{SpotifyHubEntry, item_label, kind_word};
use apricot_core::{MediaItem, Route, SpotifyEntityKind, SpotifyRef, SpotifyStamp};
use apricot_spotify::{
    CatalogItem, CatalogRequest, CatalogResult, EditOutcome, ItemKind, LibraryEdit, LibraryFilter,
    SearchKind, SpotifyError, SpotifyTrack,
};

use crate::{ChoiceDialog, ListScreen, ScreenControl, Shell, ShellUi, View};

/// An open playlist is checked this often for changes made elsewhere.
const REFRESH_INTERVAL: Duration = Duration::from_secs(15);

/// Where a list comes from (Windows `Source`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Search {
        query: String,
        kind: SearchKind,
    },
    Library {
        filter: LibraryFilter,
        folder: Option<String>,
    },
    LikedSongs,
    MyProfile(String),
    ProfilePeople {
        uri: String,
        followers: bool,
    },
    MenuSection {
        origin: apricot_spotify::main_menu::MenuOrigin,
        uri: String,
    },
    Album(String),
    Playlist(String),
    Artist(String),
    Show(String),
    Home,
    DailyMixes,
    RecentlyPlayed,
    /// Browse: `None` lists the categories, `Some(page)` one category.
    Browse(Option<String>),
    /// A section of Home or a browse page, already loaded.
    Section(Vec<CatalogItem>),
    /// Spotify's radio; it becomes the station playlist once named.
    Radio(String),
    /// Top tracks and artists with their six section titles.
    Top([String; 6]),
    /// Another account's profile with its section titles.
    Profile {
        uri: String,
        titles: [String; 3],
    },
    /// All public playlists of a profile, page by page.
    ProfilePlaylists(String),
}

impl Source {
    fn request(&self, offset: u64) -> Option<CatalogRequest> {
        Some(match self {
            Self::ProfilePeople { uri, followers } => CatalogRequest::ProfilePeople {
                uri: uri.clone(),
                followers: *followers,
            },
            Self::MenuSection { origin, uri } => CatalogRequest::MenuSection {
                origin: origin.clone(),
                uri: uri.clone(),
            },
            Self::Home => CatalogRequest::Home,
            Self::DailyMixes => CatalogRequest::DailyMixes,
            Self::RecentlyPlayed => CatalogRequest::RecentlyPlayed,
            Self::Browse(None) => CatalogRequest::BrowseAll,
            Self::Browse(Some(uri)) => CatalogRequest::BrowsePage(uri.clone()),
            Self::Radio(seed) => CatalogRequest::Radio(seed.clone()),
            Self::Top(titles) => CatalogRequest::Top(titles.clone()),
            Self::Profile { uri, titles } => CatalogRequest::Profile {
                uri: uri.clone(),
                titles: titles.clone(),
            },
            Self::ProfilePlaylists(uri) => CatalogRequest::ProfilePlaylists {
                uri: uri.clone(),
                offset,
            },
            Self::MyProfile(_) | Self::Section(_) => return None,
            Self::Search { query, kind } => CatalogRequest::Search {
                query: query.clone(),
                kind: *kind,
                offset,
            },
            Self::Library { filter, folder } => CatalogRequest::Library {
                filter: *filter,
                folder: folder.clone(),
                offset,
            },
            Self::LikedSongs => CatalogRequest::LikedSongs { offset },
            Self::Album(uri) => CatalogRequest::Album {
                uri: uri.clone(),
                offset,
            },
            Self::Playlist(uri) => CatalogRequest::Playlist {
                uri: uri.clone(),
                offset,
            },
            Self::Artist(uri) => CatalogRequest::Artist(uri.clone()),
            Self::Show(uri) => CatalogRequest::Show {
                uri: uri.clone(),
                offset,
            },
        })
    }

    /// The Spotify context a track of this list plays in.
    fn context(&self) -> Option<&str> {
        match self {
            Self::Album(uri) | Self::Playlist(uri) | Self::Artist(uri) | Self::Show(uri) => {
                Some(uri)
            }
            _ => None,
        }
    }
}

#[allow(clippy::struct_excessive_bools)] // As on Windows.
struct Frame {
    source: Source,
    /// Accessible name of the list.
    title: String,
    items: Vec<CatalogItem>,
    next_offset: Option<u64>,
    selected: usize,
    /// The request this frame waits for.
    loading: Option<SpotifyStamp>,
    /// The first page arrived (or failed).
    loaded: bool,
    /// A background check of an open playlist for changes made elsewhere.
    refresh: Option<SpotifyStamp>,
    /// Text of a failed first page.
    error: Option<String>,
    /// Search over all types: rows say their type.
    mixed: bool,
    /// A playlist the account may edit (items) and rename.
    can_edit: bool,
    can_rename: bool,
    /// A personal mix, where Spotify offers "Hide song".
    personalised: bool,
    /// An open profile: this account follows it.
    following: Option<bool>,
    /// Home and browse pages: the sections behind the section rows.
    sections: Vec<apricot_spotify::catalog::Section>,
    /// The search screen: query field, type and Search above the results,
    /// as the search of the main menu (Urh's beta 6 request).
    search_screen: bool,
    /// The query the field shows when the screen is first drawn.
    field_value: Option<String>,
    /// The first page of a new search takes the focus, as in that search.
    focus_results: bool,
}

/// A change waiting for Spotify's answer (Windows `Pending`).
enum Pending {
    Selection {
        save: Option<bool>,
        marks: apricot_app::spotify::SpotifyMarks,
    },
    CheckSaved {
        uris: Vec<String>,
    },
    Saved,
    Add {
        name: String,
        count: usize,
    },
    /// The whole target playlist loads to find songs already in it.
    Duplicates {
        playlist: String,
        name: String,
        uris: Vec<String>,
        existing: Vec<String>,
    },
    Remove {
        uid: String,
    },
    Move {
        uid: String,
        other_uid: String,
    },
    Create {
        then_add: Option<Vec<String>>,
    },
    Rename,
    /// The editable playlists load for "Add to playlist" with these items.
    Playlists {
        uris: Vec<String>,
    },
    /// The playlist loads for "Edit description" with its description.
    Describe {
        uri: String,
    },
    /// An episode's preview address loads for "Play preview".
    Preview {
        name: String,
    },
    /// Description and visibility: the answer names what changed.
    Plain,
}

/// Windows `BrowseState`.
#[derive(Default)]
pub(crate) struct BrowseState {
    frames: Vec<Frame>,
    context_read: Option<(SpotifyStamp, String, Vec<String>)>,
    saved_states: std::collections::HashMap<String, bool>,
    /// The last search, offered again by the search dialog.
    last_query: String,
    last_kind: usize,
    pending: Option<(SpotifyStamp, Pending)>,
    /// Playlists the account may add to, read once per session.
    playlists: Option<Vec<CatalogItem>>,
    /// Songs the account hid, read when the first personal mix opens.
    hidden: Option<std::collections::HashSet<String>>,
    hidden_load: Option<SpotifyStamp>,
    /// Verified personal mix contexts remain known after their list closes
    /// (beta 5), so "Hide song" works from the player.
    personalised_contexts: std::collections::HashSet<String>,
    /// When the open playlist was last checked.
    refreshed_at: Option<Instant>,
    /// Songs marked with Shift+A or Ctrl+A for "Add to Spotify playlist".
    marks: apricot_app::spotify::SpotifyMarks,
}

impl BrowseState {
    /// Another account: nothing of the previous one stays (beta 2).
    pub(crate) fn clear_account(&mut self) {
        *self = Self::default();
    }
}

impl Frame {
    /// Removes rows after an edit; the next page then starts as many rows
    /// earlier, and answers read before the edit are dropped (beta 5).
    fn remove_items(&mut self, keep: impl FnMut(&CatalogItem) -> bool) {
        let before = self.items.len();
        self.items.retain(keep);
        let removed = before - self.items.len();
        if removed > 0 {
            self.next_offset = self
                .next_offset
                .map(|offset| offset.saturating_sub(removed as u64));
            self.loading = None;
            self.refresh = None;
            self.selected = self.selected.min(self.items.len().saturating_sub(1));
        }
    }
}

/// The context commands of a Spotify list row.
pub(crate) const fn is_browse_command(command: ContextCommand) -> bool {
    use ContextCommand as C;
    matches!(
        command,
        C::SpotifyPlay
            | C::SpotifyShufflePlay
            | C::SpotifyOpen
            | C::SpotifyAddToQueue
            | C::SpotifyGoToAlbum
            | C::SpotifyToggleMenuPin
            | C::SpotifyToggleArtistFollow
            | C::SpotifyGoToArtist
            | C::SpotifyCopyLink
            | C::SpotifyToggleSaved
            | C::SpotifyAddToPlaylist
            | C::SpotifyRemoveFromPlaylist
            | C::SpotifyMoveUp
            | C::SpotifyMoveDown
            | C::SpotifyRenamePlaylist
            | C::SpotifyCreatePlaylist
            | C::SpotifyHide
            | C::SpotifyRadio
            | C::SpotifyGoToOwner
            | C::SpotifyEditDescription
            | C::SpotifyToggleVisibility
            | C::SpotifyPlayPreview
    )
}

/// A section as a row: its title and number of entries.
fn section_row(section: &apricot_spotify::catalog::Section) -> CatalogItem {
    CatalogItem {
        kind: ItemKind::Section,
        uri: section.uri.clone(),
        name: section.title.clone(),
        subtitle: String::new(),
        album: String::new(),
        album_uri: String::new(),
        artist_uri: String::new(),
        duration_ms: None,
        playable: false,
        explicit: false,
        uid: None,
        saved: None,
        count: Some(section.items.len() as u64),
        editable: false,
        format: String::new(),
    }
}

impl Shell {
    fn browse_top(&self) -> Option<&Frame> {
        self.spotify.browse.frames.last()
    }

    /// The selected row, for the context menu and shortcuts.
    fn browse_selected_item(&self, ui: &dyn ShellUi) -> Option<&CatalogItem> {
        if ui.focused_player_control().is_some() {
            return None;
        }
        if self.view == View::SpotifyHub {
            let SpotifyHubEntry::Pinned(index) =
                self.spotify.hub_entries.get(ui.selected_row()?).copied()?
            else {
                return None;
            };
            return self.spotify.hub_pin_items.get(index);
        }
        if self.view != View::SpotifyBrowse {
            return None;
        }
        self.browse_top()?.items.get(ui.selected_row()?)
    }

    /// A list entry of the hub or its global shortcut.
    pub(crate) fn spotify_open_hub_list(
        &mut self,
        ui: &mut dyn ShellUi,
        entry: SpotifyHubEntry,
        _label: &str,
    ) {
        self.stop_key_repeat(ui);
        if entry == SpotifyHubEntry::Search {
            self.spotify_show_search(ui);
            return;
        }
        if !self.spotify_ready_for_lists(ui) {
            return;
        }
        if entry == SpotifyHubEntry::MyProfile {
            if let Some(uri) = self
                .spotify_service_ref()
                .and_then(apricot_spotify::SpotifyService::my_profile_uri)
            {
                self.browse_open_root(ui, Source::MyProfile(uri), self.text("spotify_my_profile"));
            } else {
                ui.set_status(
                    &apricot_app::spotify::error_text(
                        &self.catalog(),
                        &SpotifyError::NoCredentials,
                    ),
                    true,
                );
            }
            return;
        }
        if let SpotifyHubEntry::Pinned(index) = entry {
            if let Some(pin) = self.spotify.hub_pins.get(index).cloned() {
                self.spotify_open_pin(ui, &pin);
            }
            return;
        }
        let (source, title_key) = match entry {
            SpotifyHubEntry::Library => (
                Source::Library {
                    filter: LibraryFilter::All,
                    folder: None,
                },
                "spotify_library",
            ),
            SpotifyHubEntry::LikedSongs => (Source::LikedSongs, "spotify_liked_songs"),
            SpotifyHubEntry::Playlists => (
                Source::Library {
                    filter: LibraryFilter::Playlists,
                    folder: None,
                },
                "spotify_playlists",
            ),
            SpotifyHubEntry::FollowedArtists => (
                Source::Library {
                    filter: LibraryFilter::Artists,
                    folder: None,
                },
                "spotify_followed_artists",
            ),
            SpotifyHubEntry::Home => (Source::Home, "spotify_home"),
            SpotifyHubEntry::DailyMixes => (Source::DailyMixes, "spotify_daily_mixes"),
            SpotifyHubEntry::RecentlyPlayed => (Source::RecentlyPlayed, "spotify_recently_played"),
            SpotifyHubEntry::Top => {
                let titles = [
                    "spotify_top_tracks_short",
                    "spotify_top_artists_short",
                    "spotify_top_tracks_medium",
                    "spotify_top_artists_medium",
                    "spotify_top_tracks_long",
                    "spotify_top_artists_long",
                ]
                .map(|key| self.text(key));
                (Source::Top(titles), "spotify_top")
            }
            SpotifyHubEntry::Browse => (Source::Browse(None), "spotify_browse_all"),
            _ => return,
        };
        let title = self.text(title_key);
        self.browse_open_root(ui, source, title);
    }

    /// `spotify_search`: the search dialog, then the results list.
    fn spotify_show_search(&mut self, ui: &mut dyn ShellUi) {
        if self.spotify_ready_for_lists(ui) {
            self.browse_open_search_screen(ui);
        }
    }

    /// The search screen with the last query, focus in its field.
    pub(crate) fn browse_open_search_screen(&mut self, ui: &mut dyn ShellUi) {
        let kind = SearchKind::ALL
            .get(self.spotify.browse.last_kind)
            .copied()
            .unwrap_or(SearchKind::All);
        if !self
            .browse_top()
            .is_some_and(|frame| matches!(frame.source, Source::MyProfile(_)))
        {
            self.spotify.browse.frames.clear();
        }
        self.spotify_prepare_screen(ui, View::SpotifyBrowse, Route::SpotifyBrowse);
        self.spotify.browse.frames.push(Frame {
            source: Source::Search {
                query: String::new(),
                kind,
            },
            title: self.text("result_list"),
            items: Vec::new(),
            next_offset: None,
            selected: 0,
            loading: None,
            loaded: true,
            refresh: None,
            error: None,
            mixed: kind == SearchKind::All,
            can_edit: false,
            can_rename: false,
            personalised: false,
            following: None,
            sections: Vec::new(),
            search_screen: true,
            field_value: Some(self.spotify.browse.last_query.clone()),
            focus_results: false,
        });
        self.view = View::SpotifyBrowse;
        self.browse_render(ui, false);
        ui.focus_screen_control("spotify_search_query");
    }

    /// Return in the query field and Search: the results load into the
    /// list below and take the focus when they arrive.
    pub(crate) fn spotify_search_submit(&mut self, ui: &mut dyn ShellUi) {
        let query = ui.screen_text("spotify_search_query").trim().to_owned();
        if query.is_empty() {
            ui.show_error(&self.text("enter_query"));
            ui.focus_screen_control("spotify_search_query");
            return;
        }
        let kind_index = ui.screen_choice("spotify_search_type").unwrap_or(0);
        let kind = SearchKind::ALL
            .get(kind_index)
            .copied()
            .unwrap_or(SearchKind::All);
        self.spotify.browse.marks.clear();
        if matches!(
            self.spotify.browse.pending,
            Some((_, Pending::Selection { .. } | Pending::CheckSaved { .. }))
        ) {
            self.spotify.browse.pending = None;
        }
        self.spotify.browse.last_query.clone_from(&query);
        self.spotify.browse.last_kind = kind_index;
        let title = self
            .text("spotify_search_results")
            .replace("{query}", &query);
        ui.set_status(
            &self.text("spotify_searching").replace("{query}", &query),
            true,
        );
        // A search from a nested list starts over at the search screen.
        let search = self
            .spotify
            .browse
            .frames
            .iter()
            .rposition(|frame| frame.search_screen)
            .unwrap_or(0);
        self.spotify.browse.frames.truncate(search + 1);
        let Some(frame) = self.spotify.browse.frames.last_mut() else {
            return;
        };
        frame.source = Source::Search { query, kind };
        frame.title = title;
        frame.items.clear();
        frame.next_offset = None;
        frame.selected = 0;
        frame.loaded = false;
        frame.error = None;
        frame.mixed = kind == SearchKind::All;
        frame.focus_results = true;
        self.browse_request(0);
        self.browse_render(ui, false);
    }

    /// Opens a new list on top of the current one and loads its first
    /// page.
    fn browse_open(&mut self, ui: &mut dyn ShellUi, source: Source, title: String) {
        self.spotify.browse.marks.clear();
        if matches!(
            self.spotify.browse.pending,
            Some((_, Pending::Selection { .. } | Pending::CheckSaved { .. }))
        ) {
            self.spotify.browse.pending = None;
        }
        let nested = self.view == View::SpotifyBrowse;
        self.spotify_prepare_screen(ui, View::SpotifyBrowse, Route::SpotifyBrowse);
        let mixed = matches!(
            source,
            Source::Search {
                kind: SearchKind::All,
                ..
            }
        );
        // A list opened from the hub replaces the stack; from a list it
        // nests.
        if !nested {
            self.spotify.browse.frames.clear();
        }
        let mut frame = Frame {
            source,
            title,
            items: Vec::new(),
            next_offset: None,
            selected: 0,
            loading: None,
            loaded: false,
            refresh: None,
            error: None,
            mixed,
            can_edit: false,
            can_rename: false,
            personalised: false,
            following: None,
            sections: Vec::new(),
            search_screen: false,
            field_value: None,
            focus_results: false,
        };
        if let Source::Section(items) = &frame.source {
            frame.items.clone_from(items);
            frame.loaded = true;
        }
        if let Source::MyProfile(uri) = &frame.source {
            frame.loaded = true;
            frame.items = [
                ("following", "spotify_profile_following"),
                ("followers", "spotify_profile_followers"),
                ("friends", "spotify_find_friends"),
                ("playlists", "spotify_my_public_playlists"),
            ]
            .into_iter()
            .map(|(key, label)| {
                section_row(&apricot_spotify::catalog::Section {
                    uri: apricot_spotify::catalog::profile_section_uri(uri, key),
                    title: self.text(label),
                    items: Vec::new(),
                })
            })
            .collect();
        }
        self.spotify.browse.frames.push(frame);
        self.view = View::SpotifyBrowse;
        self.browse_request(0);
        self.browse_render(ui, true);
    }

    /// Opens a list from the hub or a global shortcut: the stack starts
    /// over.
    fn browse_open_root(&mut self, ui: &mut dyn ShellUi, source: Source, title: String) {
        self.spotify.browse.frames.clear();
        if self.view == View::SpotifyBrowse {
            self.view = View::SpotifyHub;
        }
        self.browse_open(ui, source, title);
    }

    fn browse_request(&mut self, offset: u64) {
        let Some(request) = self
            .browse_top()
            .and_then(|frame| frame.source.request(offset))
        else {
            return;
        };
        let Some(service) = self.spotify_service() else {
            return;
        };
        let stamp = self.spotify.epochs.begin();
        let Some(frame) = self.spotify.browse.frames.last_mut() else {
            return;
        };
        frame.loading = Some(stamp);
        service.load_catalog(stamp, request);
    }

    /// The answer to a list request (Windows `loaded`).
    #[allow(clippy::too_many_lines)]
    pub(crate) fn spotify_browse_loaded(
        &mut self,
        ui: &mut dyn ShellUi,
        stamp: SpotifyStamp,
        result: Result<CatalogResult, SpotifyError>,
    ) {
        if !self.spotify.is_current_account(stamp) {
            return;
        }
        if self
            .spotify
            .browse
            .context_read
            .as_ref()
            .is_some_and(|(wanted, _, _)| *wanted == stamp)
        {
            let (_, target, uris) = self.spotify.browse.context_read.take().unwrap();
            if self.spotify_context_identity(ui) != Some(target) {
                return;
            }
            match &result {
                Ok(CatalogResult::SavedStates(states)) if states.len() == uris.len() => {
                    for (uri, state) in uris.iter().zip(states) {
                        self.spotify.browse.saved_states.insert(uri.clone(), *state);
                    }
                }
                Err(error) => ui.set_status(
                    &apricot_app::spotify::error_text(&self.catalog(), error),
                    true,
                ),
                _ => {}
            }
            self.show_context_menu_for_active_view_ready(ui);
            return;
        }
        let texts = self.catalog();
        if let Ok(CatalogResult::SelectionProgress(count)) = &result {
            if self
                .spotify
                .browse
                .pending
                .as_ref()
                .is_some_and(|(wanted, pending)| {
                    *wanted == stamp && matches!(pending, Pending::Selection { .. })
                })
            {
                ui.set_status(
                    &format!("{} ({count})", self.text("spotify_loading")),
                    false,
                );
            }
            return;
        }
        let pending_load = matches!(
            self.spotify.browse.pending,
            Some((
                wanted,
                Pending::Playlists { .. }
                    | Pending::Describe { .. }
                    | Pending::Preview { .. }
                    | Pending::Duplicates { .. }
                    | Pending::Selection { .. }
                    | Pending::CheckSaved { .. }
            ))
                if wanted == stamp
        );
        if pending_load && let Some((_, pending)) = self.spotify.browse.pending.take() {
            match (pending, result) {
                (Pending::Selection { save, marks }, Ok(CatalogResult::TrackUris(uris))) => {
                    let uris = marks.resolve(uris);
                    self.browse_bulk_action(ui, uris, save);
                }
                (Pending::CheckSaved { uris }, Ok(CatalogResult::SavedStates(states))) => {
                    self.browse_save_checked(ui, uris, &states);
                }
                (
                    Pending::Duplicates {
                        playlist,
                        name,
                        uris,
                        ..
                    },
                    Ok(CatalogResult::TrackUris(existing)),
                ) => self.browse_add_without_duplicates(ui, playlist, &name, &uris, &existing),
                (Pending::Playlists { uris }, Ok(CatalogResult::Playlists(playlists))) => {
                    self.spotify.browse.playlists = Some(playlists);
                    self.browse_choose_playlist(ui, uris);
                }
                (Pending::Describe { uri }, Ok(CatalogResult::Collection(collection))) => {
                    self.browse_describe(ui, uri, &collection.description);
                }
                (Pending::Preview { name }, Ok(CatalogResult::Preview(url))) => {
                    self.browse_play_preview(ui, &name, url.as_deref());
                }
                (
                    Pending::Duplicates {
                        playlist,
                        name,
                        uris,
                        mut existing,
                    },
                    Ok(CatalogResult::Collection(collection)),
                ) => {
                    existing.extend(collection.page.items.into_iter().map(|item| item.uri));
                    match collection.page.next_offset {
                        Some(offset) => self.browse_load_for(
                            ui,
                            CatalogRequest::Playlist {
                                uri: playlist.clone(),
                                offset,
                            },
                            Pending::Duplicates {
                                playlist,
                                name,
                                uris,
                                existing,
                            },
                        ),
                        None => self
                            .browse_add_without_duplicates(ui, playlist, &name, &uris, &existing),
                    }
                }
                (_, Err(error)) => {
                    ui.set_status(&apricot_app::spotify::error_text(&texts, &error), true);
                }
                _ => {}
            }
            return;
        }
        if self.browse_refreshed(ui, stamp, &result) {
            return;
        }
        if self.spotify.browse.hidden_load == Some(stamp) {
            self.spotify.browse.hidden_load = None;
            if let Ok(CatalogResult::HiddenSongs(uris)) = result {
                self.spotify.browse.hidden = Some(uris.into_iter().collect());
                if self.view == View::SpotifyBrowse {
                    self.browse_render(ui, false);
                }
            }
            return;
        }
        let Some(index) = self
            .spotify
            .browse
            .frames
            .iter()
            .position(|frame| frame.loading == Some(stamp))
        else {
            return;
        };
        let is_top = index + 1 == self.spotify.browse.frames.len();
        let frame = &mut self.spotify.browse.frames[index];
        frame.loading = None;
        let first_page = !frame.loaded;
        frame.loaded = true;
        let focus_results = std::mem::take(&mut frame.focus_results) && first_page;
        match result {
            Ok(CatalogResult::Page(page)) => {
                frame.items.extend(page.items);
                frame.next_offset = page.next_offset;
            }
            Ok(CatalogResult::Collection(collection)) => {
                if first_page && !collection.name.is_empty() {
                    let kind = match frame.source {
                        Source::Album(_) => ItemKind::Album,
                        Source::Playlist(_) => ItemKind::Playlist,
                        Source::Artist(_) => ItemKind::Artist,
                        _ => ItemKind::Show,
                    };
                    frame.title = [
                        collection.name.clone(),
                        kind_word(&texts, kind),
                        collection.subtitle.clone(),
                    ]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ");
                }
                frame.can_edit = collection.can_edit_items;
                frame.can_rename = collection.can_edit_metadata;
                frame.personalised = apricot_spotify::catalog::PERSONALISED_FORMATS
                    .contains(&collection.format.as_str());
                if let Source::Playlist(uri) = &frame.source {
                    if frame.personalised {
                        self.spotify
                            .browse
                            .personalised_contexts
                            .insert(uri.clone());
                    } else {
                        self.spotify.browse.personalised_contexts.remove(uri);
                    }
                }
                frame.items.extend(collection.page.items);
                frame.next_offset = collection.page.next_offset;
            }
            Ok(CatalogResult::Sections { title, sections }) => {
                if !title.is_empty() {
                    frame.title = title;
                }
                frame.items = sections.iter().map(section_row).collect();
                frame.sections = sections;
            }
            Ok(CatalogResult::Profile(profile)) => {
                let followers = profile.followers.map(|count| {
                    texts
                        .text("spotify_followers_count")
                        .replace("{count}", &count.to_string())
                });
                frame.title = [
                    Some(profile.name.clone()),
                    Some(kind_word(&texts, ItemKind::User)),
                    followers,
                ]
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
                let playlists =
                    apricot_spotify::catalog::profile_section_uri(&profile.uri, "playlists");
                frame.items = profile
                    .sections
                    .iter()
                    .map(|section| {
                        let mut row = section_row(section);
                        if section.uri == playlists {
                            row.count = Some(profile.playlists_total);
                        }
                        row
                    })
                    .collect();
                frame.sections = profile.sections;
                frame.following = profile.following;
            }
            Ok(CatalogResult::Radio(playlist)) => {
                // The station playlist loads in the same frame.
                frame.source = Source::Playlist(playlist);
                frame.loaded = false;
                if is_top {
                    self.browse_request(0);
                }
                return;
            }
            Ok(
                CatalogResult::SelectionProgress(_)
                | CatalogResult::TrackUris(_)
                | CatalogResult::SavedStates(_)
                | CatalogResult::Playlists(_)
                | CatalogResult::HiddenSongs(_)
                | CatalogResult::Preview(_),
            ) => {}
            Err(error) => {
                let text = apricot_app::spotify::error_text(&texts, &error);
                if first_page {
                    frame.error = Some(text.clone());
                }
                ui.set_status(&text, true);
            }
        }
        let needs_hidden = self.spotify.browse.frames[index].personalised
            && self.spotify.browse.hidden.is_none()
            && self.spotify.browse.hidden_load.is_none();
        if needs_hidden && let Some(service) = self.spotify_service() {
            let stamp = self.spotify.epochs.begin();
            self.spotify.browse.hidden_load = Some(stamp);
            service.load_catalog(stamp, CatalogRequest::HiddenSongs);
        }
        if is_top && self.view == View::SpotifyBrowse {
            self.browse_render(ui, focus_results);
        }
    }

    /// Ctrl+A records the entire current list without loading its remaining rows.
    pub(crate) fn spotify_mark_all(&mut self, ui: &mut dyn ShellUi) {
        if self.view != View::SpotifyBrowse {
            return;
        }
        let Some(frame) = self.browse_top() else {
            return;
        };
        let more = frame.next_offset.is_some() || frame.loading.is_some();
        let count = frame
            .items
            .iter()
            .filter(|item| item.kind.is_playable_item() && item.playable)
            .count();
        if more {
            self.spotify.browse.marks.mark_entire_list();
        } else {
            let uris = frame
                .items
                .iter()
                .filter(|item| item.kind.is_playable_item() && item.playable)
                .map(|item| item.uri.clone())
                .collect::<Vec<_>>();
            self.spotify.browse.marks.clear();
            self.spotify.browse.marks.mark_all(uris);
        }
        let text = if more {
            self.text("spotify_selected_entire_list")
        } else {
            self.text("spotify_marked_all")
                .replace("{count}", &count.to_string())
        };
        ui.set_status(&text, true);
        self.browse_render(ui, false);
    }

    fn browse_selection_saved(&self) -> Option<bool> {
        let frame = self.browse_top()?;
        let marks = &self.spotify.browse.marks;
        if marks.is_empty() {
            return None;
        }
        if matches!(frame.source, Source::LikedSongs) {
            return Some(true);
        }
        if marks.is_all() && (frame.next_offset.is_some() || frame.loading.is_some()) {
            return Some(false);
        }
        let selected: Vec<_> = frame
            .items
            .iter()
            .filter(|item| marks.contains(&item.uri) && item.kind.is_playable_item())
            .collect();
        Some(
            !selected.is_empty()
                && selected.len() >= marks.len()
                && selected.iter().all(|item| item.saved == Some(true)),
        )
    }

    fn browse_begin_selection(&mut self, ui: &mut dyn ShellUi, selected: &str, save: Option<bool>) {
        if self.spotify.browse.pending.is_some() {
            ui.set_status(&self.text("spotify_loading"), true);
            return;
        }
        if self.spotify.browse.marks.is_all()
            && let Some(request) = self.browse_top().and_then(|frame| frame.source.request(0))
        {
            self.browse_load_for(
                ui,
                CatalogRequest::CollectTracks(Box::new(request)),
                Pending::Selection {
                    save,
                    marks: self.spotify.browse.marks.clone(),
                },
            );
            return;
        }
        let uris = self.browse_songs_to_add(selected);
        self.browse_bulk_action(ui, uris, save);
    }

    fn browse_bulk_action(&mut self, ui: &mut dyn ShellUi, uris: Vec<String>, save: Option<bool>) {
        if uris.is_empty() {
            ui.set_status(&self.text("no_selection"), true);
            return;
        }
        match save {
            None => self.browse_add_to_playlist(ui, uris),
            Some(true) => self.browse_load_for(
                ui,
                CatalogRequest::SavedStates(uris.clone()),
                Pending::CheckSaved { uris },
            ),
            Some(false) => {
                let question = self.text("spotify_unlike") + &format!(" ({})?", uris.len());
                if ui.ask_yes_no(&question, &self.text("yes"), &self.text("no")) {
                    self.browse_begin_edit(
                        LibraryEdit::SetSaved { uris, saved: false },
                        Pending::Saved,
                    );
                }
                ui.focus_primary_control();
            }
        }
    }

    fn browse_save_checked(&mut self, ui: &mut dyn ShellUi, uris: Vec<String>, states: &[bool]) {
        if states.len() != uris.len() {
            ui.set_status(
                &apricot_app::spotify::error_text(
                    &self.catalog(),
                    &SpotifyError::Service("incomplete saved states".into()),
                ),
                true,
            );
            return;
        }
        let duplicates = states.iter().filter(|saved| **saved).count();
        let uris = if duplicates == 0 {
            uris
        } else {
            let question = self
                .text("spotify_duplicates_question")
                .replace("{count}", &duplicates.to_string())
                .replace("{total}", &uris.len().to_string())
                .replace("{name}", &self.text("spotify_liked_songs"));
            let choice = ui.choose(&ChoiceDialog {
                title: self.text("spotify_like"),
                list_name: question.clone(),
                prompt: question,
                choices: vec![
                    self.text("spotify_add_new_only"),
                    self.text("spotify_add_all_anyway"),
                ],
                initial: 0,
                ok: self.text("ok"),
                cancel: self.text("cancel"),
            });
            ui.focus_primary_control();
            match choice {
                Some(0) => uris
                    .into_iter()
                    .zip(states)
                    .filter_map(|(uri, saved)| (!saved).then_some(uri))
                    .collect(),
                Some(_) => uris,
                None => return,
            }
        };
        if uris.is_empty() {
            ui.set_status(
                &self
                    .text("spotify_nothing_new")
                    .replace("{name}", &self.text("spotify_liked_songs")),
                true,
            );
            return;
        }
        self.browse_begin_edit(LibraryEdit::SetSaved { uris, saved: true }, Pending::Saved);
    }

    /// Shift+A in a Spotify list: marks or unmarks the selected song.
    pub(crate) fn spotify_toggle_mark(&mut self, ui: &mut dyn ShellUi) {
        let Some(uri) = self
            .browse_selected_item(ui)
            .filter(|item| item.kind.is_playable_item() && item.playable)
            .map(|item| item.uri.clone())
        else {
            ui.set_status(&self.text("no_selection"), true);
            return;
        };
        let marked = self.spotify.browse.marks.toggle(&uri);
        let key = if marked {
            "spotify_marked"
        } else {
            "spotify_unmarked"
        };
        let count = self.spotify.browse.marks.len().to_string();
        let text = if self.spotify.browse.marks.is_all() {
            self.text(if marked {
                "spotify_marked_row"
            } else {
                "spotify_unmarked_row"
            })
        } else {
            self.text(key).replace("{count}", &count)
        };
        ui.set_status(&text, true);
        self.browse_render(ui, false);
    }

    /// The songs "Add to Spotify playlist" adds: the marked ones, or the
    /// selected row.
    fn browse_songs_to_add(&self, selected: &str) -> Vec<String> {
        if self.spotify.browse.marks.is_empty() {
            vec![selected.to_owned()]
        } else {
            self.spotify.browse.marks.uris().to_vec()
        }
    }

    /// The whole playlist is in: songs already there are left out, or
    /// added anyway, as the user chooses.
    fn browse_add_without_duplicates(
        &mut self,
        ui: &mut dyn ShellUi,
        playlist: String,
        name: &str,
        uris: &[String],
        existing: &[String],
    ) {
        let (new, duplicates) = apricot_app::spotify::split_duplicates(uris, existing);
        let uris = if duplicates == 0 {
            uris.to_vec()
        } else {
            let question = self
                .text("spotify_duplicates_question")
                .replace("{count}", &duplicates.to_string())
                .replace("{total}", &uris.len().to_string())
                .replace("{name}", name);
            let title = self.text("spotify_add_to_playlist");
            let chosen = ui.choose(&ChoiceDialog {
                title,
                list_name: question.clone(),
                prompt: question,
                choices: vec![
                    self.text("spotify_add_new_only"),
                    self.text("spotify_add_all_anyway"),
                ],
                initial: 0,
                ok: self.text("ok"),
                cancel: self.text("cancel"),
            });
            ui.focus_primary_control();
            match chosen {
                Some(0) if new.is_empty() => {
                    ui.set_status(
                        &self.text("spotify_nothing_new").replace("{name}", name),
                        true,
                    );
                    self.spotify.browse.marks.clear();
                    self.browse_render(ui, false);
                    return;
                }
                Some(0) => new,
                Some(_) => uris.to_vec(),
                None => return,
            }
        };
        let count = uris.len();
        self.browse_begin_edit(
            LibraryEdit::AddToPlaylist { playlist, uris },
            Pending::Add {
                name: name.to_owned(),
                count,
            },
        );
    }

    /// Refills the list. `focus` puts the focus on it (opening, going
    /// back); otherwise focus stays where it is. The Tab order is the
    /// list, Open and Back, as Windows creates them.
    #[allow(clippy::too_many_lines)] // One complete native list screen and its controls.
    fn browse_render(&mut self, ui: &mut dyn ShellUi, focus: bool) {
        let texts = self.catalog();
        let Some(frame) = self.spotify.browse.frames.last() else {
            return;
        };
        let rows: Vec<String> = if !frame.items.is_empty() {
            frame
                .items
                .iter()
                .map(|item| {
                    if matches!(frame.source, Source::MyProfile(_)) {
                        return item.name.clone();
                    }
                    let mut label = item_label(
                        &texts,
                        item,
                        frame.mixed,
                        !matches!(frame.source, Source::LikedSongs),
                    );
                    if self
                        .spotify
                        .browse
                        .hidden
                        .as_ref()
                        .is_some_and(|hidden| hidden.contains(&item.uri))
                    {
                        label.push_str(", ");
                        label.push_str(texts.text("spotify_hidden"));
                    }
                    if self.spotify.browse.marks.contains(&item.uri) {
                        label.push_str(", ");
                        label.push_str(texts.text("spotify_marked_row"));
                    }
                    label
                })
                .collect()
        } else if let Some(error) = &frame.error {
            vec![error.clone()]
        } else if !frame.loaded {
            vec![texts.text("spotify_loading").to_owned()]
        } else if matches!(&frame.source, Source::Search { query, .. } if query.is_empty()) {
            vec![texts.text("search_results_empty").to_owned()]
        } else if matches!(frame.source, Source::Search { .. }) {
            vec![texts.text("spotify_no_results").to_owned()]
        } else {
            vec![texts.text("spotify_empty").to_owned()]
        };
        let selected = frame.selected.min(rows.len().saturating_sub(1));
        let button = |id: &'static str, key: &str| ScreenControl::Button {
            id,
            label: texts.text(key).to_owned(),
        };
        let controls = if frame.search_screen {
            // The YouTube search screen's order: Back, the query, the type,
            // Search, Open and the results.
            let kind = match &frame.source {
                Source::Search { kind, .. } => {
                    SearchKind::ALL.iter().position(|known| known == kind)
                }
                _ => None,
            };
            vec![
                button("back", "back"),
                ScreenControl::TextField {
                    id: "spotify_search_query",
                    label: texts.text("spotify_search_query").to_owned(),
                    value: frame.field_value.clone(),
                },
                ScreenControl::Choice {
                    id: "spotify_search_type",
                    label: texts.text("spotify_search_type").to_owned(),
                    options: apricot_app::spotify::search_kind_labels(&texts),
                    selected: frame.field_value.as_ref().and(kind),
                },
                button("spotify_search_run", "search"),
                button("open", "open"),
                ScreenControl::List,
            ]
        } else {
            vec![
                ScreenControl::List,
                button("open", "open"),
                button("back", "back"),
            ]
        };
        let screen = ListScreen {
            list_name: frame.title.clone(),
            rows,
            selected: Some(selected),
            controls,
            focus_list: focus,
        };
        let playlist = matches!(frame.source, Source::Playlist(_));
        // The field keeps what the user typed after the first drawing.
        if let Some(frame) = self.spotify.browse.frames.last_mut() {
            frame.field_value = None;
        }
        if playlist {
            self.spotify
                .browse
                .refreshed_at
                .get_or_insert_with(Instant::now);
        } else {
            self.spotify.browse.refreshed_at = None;
        }
        ui.show_list_screen(&screen);
    }

    /// Windows `refresh_tick`: the open playlist asks Spotify for its
    /// current state every 15 seconds, while nothing else loads.
    pub(crate) fn spotify_browse_refresh_tick(&mut self) {
        let Some(at) = self.spotify.browse.refreshed_at else {
            return;
        };
        if at.elapsed() < REFRESH_INTERVAL {
            return;
        }
        self.spotify.browse.refreshed_at = Some(Instant::now());
        let watching = self.view == View::SpotifyBrowse
            && self.spotify.browse.frames.last().is_some_and(|frame| {
                matches!(frame.source, Source::Playlist(_))
                    && frame.loaded
                    && frame.loading.is_none()
                    && frame.refresh.is_none()
            });
        if !watching {
            return;
        }
        let Some(service) = self.spotify_service() else {
            return;
        };
        let stamp = self.spotify.epochs.begin();
        let Some(frame) = self.spotify.browse.frames.last_mut() else {
            return;
        };
        let Source::Playlist(uri) = &frame.source else {
            return;
        };
        frame.refresh = Some(stamp);
        service.load_catalog(
            stamp,
            CatalogRequest::Playlist {
                uri: uri.clone(),
                offset: 0,
            },
        );
    }

    /// The answer to a background check: a playlist changed elsewhere shows
    /// its new rows; the same occurrence stays selected and focus does not
    /// move. `false` when the answer was not a check.
    fn browse_refreshed(
        &mut self,
        ui: &mut dyn ShellUi,
        stamp: SpotifyStamp,
        result: &Result<CatalogResult, SpotifyError>,
    ) -> bool {
        let Some(index) = self
            .spotify
            .browse
            .frames
            .iter()
            .position(|frame| frame.refresh == Some(stamp))
        else {
            return false;
        };
        let is_top = index + 1 == self.spotify.browse.frames.len();
        let selected = ui.selected_row();
        let frame = &mut self.spotify.browse.frames[index];
        frame.refresh = None;
        let Ok(CatalogResult::Collection(collection)) = result else {
            return true;
        };
        // Spotify's `revisionId` can lag behind a change, so the rows
        // decide.
        let occurrence = |item: &CatalogItem| (item.uid.clone(), item.uri.clone());
        let page = &collection.page.items;
        let unchanged = frame
            .items
            .iter()
            .map(occurrence)
            .take(page.len())
            .eq(page.iter().map(occurrence))
            && if collection.page.next_offset.is_some() {
                frame.items.len() >= page.len()
            } else {
                frame.items.len() == page.len()
            };
        if unchanged {
            return true;
        }
        let selected_uid = selected
            .filter(|_| is_top)
            .and_then(|at| frame.items.get(at))
            .and_then(|item| item.uid.clone());
        frame.items.clone_from(&collection.page.items);
        frame.next_offset = collection.page.next_offset;
        frame.can_edit = collection.can_edit_items;
        frame.can_rename = collection.can_edit_metadata;
        let position = selected_uid
            .and_then(|uid| {
                frame
                    .items
                    .iter()
                    .position(|item| item.uid.as_deref() == Some(uid.as_str()))
            })
            .or(selected)
            .unwrap_or(0);
        frame.selected = position.min(frame.items.len().saturating_sub(1));
        if is_top && self.view == View::SpotifyBrowse {
            self.browse_render(ui, false);
        }
        true
    }

    /// The selection moved: it is remembered, and the last row loads the
    /// next page with "Loading more results.".
    pub(crate) fn spotify_browse_selection_changed(&mut self, ui: &mut dyn ShellUi) {
        let Some(selected) = ui.selected_row() else {
            return;
        };
        let Some(frame) = self.spotify.browse.frames.last_mut() else {
            return;
        };
        frame.selected = selected;
        let at_end = !frame.items.is_empty() && selected + 1 == frame.items.len();
        if let Some(offset) = frame
            .next_offset
            .filter(|_| at_end && frame.loading.is_none())
        {
            ui.set_status(&self.text("loading_more_results"), false);
            self.browse_request(offset);
        }
    }

    /// The row to come back to from the player or a nested list.
    fn browse_remember_selection(&mut self, ui: &dyn ShellUi) {
        if let (Some(frame), Some(selected)) =
            (self.spotify.browse.frames.last_mut(), ui.selected_row())
        {
            frame.selected = selected;
        }
    }

    /// Enter: a track or episode plays in its list's context, a collection
    /// opens.
    pub(crate) fn spotify_browse_activate(&mut self, ui: &mut dyn ShellUi) {
        self.browse_remember_selection(ui);
        let Some(item) = self.browse_selected_item(ui).cloned() else {
            return;
        };
        if item.kind.is_playable_item() {
            self.browse_play(ui, &item, false);
        } else {
            self.browse_open_item(ui, &item);
        }
    }

    /// An artist, show, profile or Liked Songs link opens its list (Windows
    /// `resolve`, beta 2); `false` for links that play.
    pub(crate) fn spotify_open_link_list(&mut self, ui: &mut dyn ShellUi, uri: &str) -> bool {
        let Some(reference) = SpotifyRef::parse(uri) else {
            return false;
        };
        let source = match reference.kind {
            SpotifyEntityKind::Artist => Source::Artist(uri.to_owned()),
            SpotifyEntityKind::Show => Source::Show(uri.to_owned()),
            SpotifyEntityKind::User => self.browse_profile_source(uri),
            SpotifyEntityKind::LikedSongs => Source::LikedSongs,
            _ => return false,
        };
        self.browse_open_root(ui, source, uri.to_owned());
        true
    }

    /// A profile's source with its localized section titles.
    fn browse_profile_source(&self, uri: &str) -> Source {
        Source::Profile {
            uri: uri.to_owned(),
            titles: [
                "spotify_profile_playlists",
                "spotify_profile_following",
                "spotify_profile_followers",
            ]
            .map(|key| self.text(key)),
        }
    }

    /// Opens an album, playlist, artist, show, folder or Liked Songs row.
    fn browse_open_item(&mut self, ui: &mut dyn ShellUi, item: &CatalogItem) {
        if let Some(Source::MyProfile(uri)) = self.browse_top().map(|frame| &frame.source) {
            let uri = uri.clone();
            if item.uri == apricot_spotify::catalog::profile_section_uri(&uri, "friends") {
                self.spotify.browse.last_kind = SearchKind::ALL
                    .iter()
                    .position(|kind| *kind == SearchKind::Profiles)
                    .unwrap();
                self.spotify.browse.last_query.clear();
                self.browse_open_search_screen(ui);
            } else {
                let source = if item.uri
                    == apricot_spotify::catalog::profile_section_uri(&uri, "playlists")
                {
                    Source::ProfilePlaylists(uri)
                } else {
                    Source::ProfilePeople {
                        followers: item.uri
                            == apricot_spotify::catalog::profile_section_uri(&uri, "followers"),
                        uri,
                    }
                };
                self.browse_open(ui, source, item.name.clone());
            }
            return;
        }
        let source = match item.kind {
            ItemKind::Page => Source::Browse(Some(item.uri.clone())),
            ItemKind::Section
                if self.browse_top().is_some_and(|frame| {
                    matches!(&frame.source, Source::Profile { uri, .. }
                    if item.uri == apricot_spotify::catalog::profile_section_uri(uri, "playlists"))
                }) =>
            {
                let Some(Source::Profile { uri, .. }) = self.browse_top().map(|f| &f.source) else {
                    return;
                };
                Source::ProfilePlaylists(uri.clone())
            }
            ItemKind::User => self.browse_profile_source(&item.uri),
            ItemKind::Section => {
                let items = self
                    .browse_top()
                    .and_then(|frame| {
                        frame
                            .sections
                            .iter()
                            .find(|section| section.uri == item.uri)
                    })
                    .map(|section| section.items.clone())
                    .unwrap_or_default();
                Source::Section(items)
            }
            ItemKind::Album => Source::Album(item.uri.clone()),
            ItemKind::Playlist => Source::Playlist(item.uri.clone()),
            ItemKind::Artist => Source::Artist(item.uri.clone()),
            ItemKind::Show => Source::Show(item.uri.clone()),
            ItemKind::LikedSongs => Source::LikedSongs,
            ItemKind::Folder => Source::Library {
                filter: LibraryFilter::All,
                folder: Some(item.uri.clone()),
            },
            ItemKind::Track | ItemKind::Episode => return,
            ItemKind::Unavailable => {
                ui.set_status(&self.text("spotify_unplayable"), true);
                return;
            }
            _ => {
                let texts = self.catalog();
                let text = texts
                    .text("rust_feature_unavailable")
                    .replace("{feature}", &kind_word(&texts, item.kind));
                ui.set_status(&text, true);
                return;
            }
        };
        let title = if item.name.is_empty() {
            item.uri.clone()
        } else {
            item.name.clone()
        };
        self.browse_open(ui, source, title);
    }

    /// Plays `item`: a track or episode in the context of the open list, a
    /// collection as its whole context; `shuffle` starts it shuffled.
    fn browse_play(&mut self, ui: &mut dyn ShellUi, item: &CatalogItem, shuffle: bool) {
        if !item.playable && item.kind.is_playable_item() || item.kind == ItemKind::Unavailable {
            ui.set_status(&self.text("spotify_unplayable"), true);
            return;
        }
        if let Some(mut media) = self.browse_media_for(item, shuffle) {
            if shuffle {
                media
                    .metadata
                    .insert("spotify_shuffle_explicit".to_owned(), true.into());
            }
            self.spotify_play_track(ui, media);
        }
    }

    /// The selected row as an Apricot item, for Favorites, Apricot
    /// playlists and the other shared actions.
    pub(crate) fn spotify_selected_media(&self, ui: &dyn ShellUi) -> Option<MediaItem> {
        let item = self.browse_selected_item(ui)?;
        if item.kind == ItemKind::Unavailable || SpotifyRef::parse(&item.uri).is_none() {
            return None;
        }
        self.browse_media_for(item, false)
    }

    fn browse_media_for(&self, item: &CatalogItem, shuffle: bool) -> Option<MediaItem> {
        let liked_songs = || {
            self.spotify_service_ref()
                .and_then(apricot_spotify::SpotifyService::liked_songs_context)
        };
        let track = SpotifyTrack {
            uri: item.uri.clone(),
            title: item.name.clone(),
            artists: item.subtitle.clone(),
            artist_uri: item.artist_uri.clone(),
            album: item.album.clone(),
            duration_ms: item
                .duration_ms
                .and_then(|duration| u32::try_from(duration).ok())
                .unwrap_or(0),
        };
        let mut media = track.media_item();
        let mut insert = |key: &str, value: serde_json::Value| {
            media.metadata.insert(key.to_owned(), value);
        };
        if item.kind.is_playable_item() {
            let context = match self.browse_top().map(|frame| &frame.source) {
                Some(Source::LikedSongs) => liked_songs(),
                Some(source) => source.context().map(str::to_owned),
                None => None,
            };
            if let Some(context) = context {
                insert("spotify_context", serde_json::Value::String(context));
                // Playlist uids are Connect's occurrence ids; album, show and
                // personal mix uids are not, so those play by URI.
                let by_uid = self.browse_top().is_some_and(|frame| {
                    matches!(frame.source, Source::Playlist(_)) && !frame.personalised
                });
                if let (Some(uid), true) = (&item.uid, by_uid) {
                    insert("spotify_uid", serde_json::Value::String(uid.clone()));
                }
            }
        } else {
            let context = if item.kind == ItemKind::LikedSongs {
                liked_songs()
            } else {
                Some(item.uri.clone())
            };
            insert("spotify_context", serde_json::Value::String(context?));
            // The first track is not known yet: the item becomes the track
            // Spotify starts, with its own "Playing" announcement.
            insert("spotify_collection", serde_json::Value::Bool(true));
        }
        if shuffle {
            insert("spotify_shuffle", serde_json::Value::Bool(true));
        }
        Some(media)
    }

    /// Escape and Back: one level up, to the row the user came from.
    pub(crate) fn spotify_browse_back(&mut self, ui: &mut dyn ShellUi) {
        self.spotify.browse.marks.clear();
        if matches!(
            self.spotify.browse.pending,
            Some((_, Pending::Selection { .. } | Pending::CheckSaved { .. }))
        ) {
            self.spotify.browse.pending = None;
        }
        self.spotify.browse.frames.pop();
        if self.spotify.browse.frames.is_empty() {
            self.show_spotify_hub(ui);
        } else {
            self.browse_render(ui, true);
        }
    }

    /// Back from the player: the list it was started from, at the same row.
    pub(crate) fn spotify_browse_restore(&mut self, ui: &mut dyn ShellUi) {
        if self.spotify.browse.frames.is_empty() {
            self.show_spotify_hub(ui);
            return;
        }
        self.view = View::SpotifyBrowse;
        self.browse_render(ui, true);
    }

    /// The Spotify link of the selected row, for Copy link.
    fn browse_selected_link(&self, ui: &dyn ShellUi) -> Option<String> {
        self.browse_selected_item(ui)
            .and_then(|item| SpotifyRef::parse(&item.uri))
            .and_then(|reference| reference.to_url())
    }

    /// The context menu of the selected row (Windows `row_menu`).
    pub(crate) fn spotify_browse_context_entries(
        &self,
        ui: &dyn ShellUi,
        context: &apricot_app::context_menu::ContextMenuContext<'_>,
    ) -> Option<Vec<ContextMenuEntry>> {
        let item = self.browse_selected_item(ui)?;
        let collection = matches!(
            item.kind,
            ItemKind::Album
                | ItemKind::Playlist
                | ItemKind::Artist
                | ItemKind::Show
                | ItemKind::Folder
                | ItemKind::LikedSongs
                | ItemKind::User
                | ItemKind::Page
                | ItemKind::Section
        );
        if self.view == View::SpotifyHub {
            return self.spotify_hub_pin_context_entries(ui, context);
        }
        let frame = self.browse_top()?;
        let profile_section =
            item.kind == ItemKind::Section && matches!(frame.source, Source::Profile { .. });
        let editable = frame.can_edit && matches!(frame.source, Source::Playlist(_));
        let index = ui.selected_row().unwrap_or(0);
        let has_uid = |at: Option<usize>| {
            at.and_then(|at| frame.items.get(at))
                .is_some_and(|item| item.uid.is_some())
        };
        let menu = apricot_app::context_menu::SpotifyRowMenu {
            playable_item: item.kind.is_playable_item() && item.playable,
            collection,
            collection_plays: collection
                && !matches!(
                    item.kind,
                    ItemKind::Folder | ItemKind::User | ItemKind::Page | ItemKind::Section
                ),
            album: !item.album_uri.is_empty(),
            artist: item.artist_uri.starts_with("spotify:artist:"),
            link: SpotifyRef::parse(&item.uri).is_some(),
            saved: if profile_section {
                frame.following
            } else if item.kind.is_playable_item() {
                self.browse_selection_saved().or(item.saved)
            } else {
                self.spotify
                    .browse
                    .saved_states
                    .get(&item.uri)
                    .copied()
                    .or(item.saved)
            },
            main_menu_pin: self.spotify_pin_for_item(item).map(|pin| {
                self.spotify
                    .hub_pins
                    .iter()
                    .any(|entry| entry.target == pin.target)
            }),
            artist_followed: self
                .spotify
                .browse
                .saved_states
                .get(&item.artist_uri)
                .copied(),
            is_artist: item.kind == ItemKind::Artist,
            profile: item.kind == ItemKind::User || profile_section,
            owner: item.kind == ItemKind::Playlist && item.artist_uri.starts_with("spotify:user:"),
            preview: item.kind == ItemKind::Episode,
            in_editable_playlist: editable && item.uid.is_some(),
            move_up: editable && has_uid(index.checked_sub(1)),
            move_down: editable && has_uid(Some(index + 1)),
            rename: item.kind == ItemKind::Playlist && item.editable,
            create_playlist: matches!(frame.source, Source::Library { .. }),
            hide: frame.personalised && item.kind == ItemKind::Track,
            hidden: self
                .spotify
                .browse
                .hidden
                .as_ref()
                .is_some_and(|hidden| hidden.contains(&item.uri)),
            radio: matches!(
                item.kind,
                ItemKind::Track | ItemKind::Artist | ItemKind::Album | ItemKind::Playlist
            ),
        };
        Some(apricot_app::context_menu::spotify_browse_context_menu(
            context, menu,
        ))
    }

    fn spotify_hub_pin_context_entries(
        &self,
        ui: &dyn ShellUi,
        context: &apricot_app::context_menu::ContextMenuContext<'_>,
    ) -> Option<Vec<ContextMenuEntry>> {
        let item = self.browse_selected_item(ui)?;
        let _pin = self.spotify_selected_pin(ui)?;
        let saved = self.spotify.browse.saved_states.get(&item.uri).copied();
        Some(apricot_app::context_menu::spotify_browse_context_menu(
            context,
            apricot_app::context_menu::SpotifyRowMenu {
                collection: true,
                collection_plays: item.playable,
                link: SpotifyRef::parse(&item.uri).is_some(),
                is_artist: item.kind == ItemKind::Artist,
                saved,
                main_menu_pin: Some(true),
                radio: item.playable,
                owner: item.kind == ItemKind::Playlist
                    && item.artist_uri.starts_with("spotify:user:"),
                rename: item.kind == ItemKind::Playlist && item.editable,
                ..Default::default()
            },
        ))
    }

    /// `spotify_radio`: Spotify's radio for the selected track, artist,
    /// album or playlist, or for the playing Spotify track.
    pub(crate) fn spotify_radio_target(&self, ui: &dyn ShellUi) -> Option<(String, String)> {
        self.browse_selected_item(ui)
            .filter(|item| {
                matches!(
                    item.kind,
                    ItemKind::Track | ItemKind::Artist | ItemKind::Album | ItemKind::Playlist
                )
            })
            .map(|item| (item.uri.clone(), item.name.clone()))
            .or_else(|| {
                self.application
                    .player_session()
                    .current_item()
                    .filter(|item| item.source == apricot_core::MediaSource::Spotify)
                    .filter(|item| item.id.0.starts_with("spotify:track:"))
                    .map(|item| (item.id.0.clone(), item.title.clone()))
            })
    }

    pub(crate) fn spotify_radio(&mut self, ui: &mut dyn ShellUi) {
        let target = self.spotify_radio_target(ui);
        let Some((seed, name)) = target else {
            return;
        };
        let title = self.text("spotify_radio_title").replace("{name}", &name);
        self.browse_open(ui, Source::Radio(seed), title);
    }

    /// `spotify_dislike`: Spotify's "Hide song" in a personal mix, or
    /// showing it again. Elsewhere it says where it works.
    pub(crate) fn spotify_hide_target(&self, ui: &dyn ShellUi) -> Option<(String, String)> {
        let frame = self
            .browse_top()
            .filter(|_| self.view == View::SpotifyBrowse);
        frame
            .filter(|frame| frame.personalised)
            .and_then(|frame| {
                let Source::Playlist(context) = &frame.source else {
                    return None;
                };
                self.browse_selected_item(ui)
                    .filter(|item| item.kind == ItemKind::Track)
                    .map(|item| (item.uri.clone(), context.clone()))
            })
            .or_else(|| {
                // The playing track of a personal mix that was opened
                // before (beta 5).
                let item = self.application.player_session().current_item()?;
                if item.source != apricot_core::MediaSource::Spotify
                    || !item.id.0.starts_with("spotify:track:")
                {
                    return None;
                }
                let context = item.metadata.get("spotify_context")?.as_str()?;
                self.spotify
                    .browse
                    .personalised_contexts
                    .contains(context)
                    .then(|| (item.id.0.clone(), context.to_owned()))
            })
    }

    pub(crate) fn spotify_toggle_hidden(&mut self, ui: &mut dyn ShellUi) {
        let target = self.spotify_hide_target(ui);
        let Some((uri, context)) = target else {
            ui.set_status(&self.text("spotify_hide_unavailable"), true);
            return;
        };
        let hidden = self
            .spotify
            .browse
            .hidden
            .as_ref()
            .is_some_and(|hidden| hidden.contains(&uri));
        self.browse_begin_edit(
            LibraryEdit::Hide {
                uri,
                context,
                hidden: !hidden,
            },
            Pending::Saved,
        );
    }

    fn browse_begin_edit(&mut self, edit: LibraryEdit, pending: Pending) {
        let Some(service) = self.spotify_service() else {
            return;
        };
        let stamp = self.spotify.epochs.begin();
        self.spotify.browse.pending = Some((stamp, pending));
        service.edit(stamp, edit);
    }

    /// `spotify_toggle_saved`: like or unlike the selected track or the
    /// one that plays; save or remove a collection; follow an artist.
    pub(crate) fn spotify_saved_target(&self, ui: &dyn ShellUi) -> Option<String> {
        let profile = self.browse_top().and_then(|frame| match &frame.source {
            Source::Profile { uri, .. } => Some(uri.clone()),
            _ => None,
        });
        let uri = self
            .browse_selected_item(ui)
            .filter(|item| item.kind != ItemKind::Unavailable && item.kind != ItemKind::Folder)
            .and_then(|item| {
                if item.kind == ItemKind::Section {
                    // A section of an open profile follows the profile.
                    profile.clone()
                } else {
                    Some(item.uri.clone())
                }
            })
            .or_else(|| {
                self.application
                    .player_session()
                    .current_item()
                    .filter(|item| item.source == apricot_core::MediaSource::Spotify)
                    .map(|item| item.id.0.clone())
                    .filter(|uri| !uri.contains(":album:") && !uri.contains(":playlist:"))
            });
        uri.filter(|uri| uri != "spotify:collection:tracks")
    }

    pub(crate) fn spotify_toggle_saved(&mut self, ui: &mut dyn ShellUi) {
        let Some(uri) = self.spotify_saved_target(ui) else {
            return;
        };
        if self.view == View::SpotifyBrowse
            && ui.focused_player_control().is_none()
            && !self.spotify.browse.marks.is_empty()
        {
            let save = self.browse_selection_saved() != Some(true);
            self.browse_begin_selection(ui, &uri, Some(save));
        } else {
            self.browse_begin_edit(LibraryEdit::ToggleSaved { uri }, Pending::Saved);
        }
    }

    /// Delete in an editable playlist: removes exactly the selected
    /// occurrence.
    pub(crate) fn spotify_browse_remove_selected(&mut self, ui: &mut dyn ShellUi) {
        if ui.focused_player_control().is_some() {
            return;
        }
        let Some(frame) = self.browse_top() else {
            return;
        };
        let Source::Playlist(playlist) = &frame.source else {
            return;
        };
        if !frame.can_edit {
            return;
        }
        let Some(uid) = self
            .browse_selected_item(ui)
            .and_then(|item| item.uid.clone())
        else {
            return;
        };
        let playlist = playlist.clone();
        self.browse_begin_edit(
            LibraryEdit::RemoveFromPlaylist {
                playlist,
                uids: vec![uid.clone()],
            },
            Pending::Remove { uid },
        );
    }

    fn browse_move_selected(&mut self, ui: &mut dyn ShellUi, up: bool) {
        let Some(frame) = self.browse_top() else {
            return;
        };
        let Source::Playlist(playlist) = &frame.source else {
            return;
        };
        let index = ui.selected_row().unwrap_or(0);
        let other = if up {
            index.checked_sub(1)
        } else {
            Some(index + 1)
        };
        let (Some(uid), Some(other_uid)) = (
            frame.items.get(index).and_then(|item| item.uid.clone()),
            other
                .and_then(|other| frame.items.get(other))
                .and_then(|item| item.uid.clone()),
        ) else {
            return;
        };
        let playlist = playlist.clone();
        self.browse_begin_edit(
            LibraryEdit::MoveInPlaylist {
                playlist,
                uid: uid.clone(),
                before: up,
                other_uid: other_uid.clone(),
            },
            Pending::Move { uid, other_uid },
        );
    }

    /// Asks for a text; `None` after Cancel, and after an empty answer
    /// unless `allow_empty`.
    fn browse_ask_text(
        &self,
        ui: &mut dyn ShellUi,
        title_key: &str,
        label_key: &str,
        initial: &str,
        allow_empty: bool,
    ) -> Option<String> {
        ui.prompt_text(
            &self.text(title_key),
            &self.text(label_key),
            initial,
            &self.text("ok"),
            &self.text("cancel"),
        )
        .map(|text| text.trim().to_owned())
        .filter(|text| allow_empty || !text.is_empty())
    }

    /// `create_playlist` in Spotify lists: a new playlist, first in the
    /// library.
    pub(crate) fn spotify_create_playlist(&mut self, ui: &mut dyn ShellUi) {
        self.browse_create_playlist_then_add(ui, None);
    }

    fn browse_create_playlist_then_add(
        &mut self,
        ui: &mut dyn ShellUi,
        then_add: Option<Vec<String>>,
    ) {
        let name = self.browse_ask_text(ui, "spotify_create_playlist", "playlist_name", "", false);
        ui.focus_primary_control();
        let Some(name) = name else {
            return;
        };
        self.browse_begin_edit(
            LibraryEdit::CreatePlaylist { name },
            Pending::Create { then_add },
        );
    }

    fn browse_rename_selected(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self.browse_selected_item(ui).cloned() else {
            return;
        };
        let name = self.browse_ask_text(
            ui,
            "spotify_rename_playlist",
            "playlist_name",
            &item.name,
            false,
        );
        ui.focus_primary_control();
        let Some(name) = name else {
            return;
        };
        self.browse_begin_edit(
            LibraryEdit::RenamePlaylist {
                uri: item.uri.clone(),
                name,
            },
            Pending::Rename,
        );
    }

    /// "Add to Spotify playlist": the playlists the account may edit, with
    /// New playlist first, read once per session.
    fn browse_add_to_playlist(&mut self, ui: &mut dyn ShellUi, uris: Vec<String>) {
        if self.spotify.browse.playlists.is_some() {
            self.browse_choose_playlist(ui, uris);
            return;
        }
        let Some(service) = self.spotify_service() else {
            return;
        };
        ui.set_status(&self.text("spotify_loading"), false);
        let stamp = self.spotify.epochs.begin();
        self.spotify.browse.pending = Some((stamp, Pending::Playlists { uris }));
        service.load_catalog(stamp, CatalogRequest::EditablePlaylists);
    }

    fn browse_choose_playlist(&mut self, ui: &mut dyn ShellUi, uris: Vec<String>) {
        let playlists = self.spotify.browse.playlists.clone().unwrap_or_default();
        let mut choices = vec![self.text("spotify_new_playlist")];
        choices.extend(playlists.iter().map(|playlist| playlist.name.clone()));
        let title = self.text("spotify_add_to_playlist");
        let chosen = ui.choose(&ChoiceDialog {
            title: title.clone(),
            list_name: title.clone(),
            prompt: title,
            choices,
            initial: 0,
            ok: self.text("ok"),
            cancel: self.text("cancel"),
        });
        match chosen {
            Some(0) => self.browse_create_playlist_then_add(ui, Some(uris)),
            Some(index) => {
                ui.focus_primary_control();
                if let Some(playlist) = playlists.get(index - 1) {
                    // The whole playlist loads first, to find duplicates.
                    ui.set_status(
                        &self
                            .text("spotify_checking_duplicates")
                            .replace("{name}", &playlist.name),
                        false,
                    );
                    self.browse_load_for(
                        ui,
                        CatalogRequest::CollectTracks(Box::new(CatalogRequest::Playlist {
                            uri: playlist.uri.clone(),
                            offset: 0,
                        })),
                        Pending::Duplicates {
                            playlist: playlist.uri.clone(),
                            name: playlist.name.clone(),
                            uris,
                            existing: Vec::new(),
                        },
                    );
                }
            }
            None => ui.focus_primary_control(),
        }
    }

    /// Spotify's answer to a change: one announcement, and the open lists
    /// show the confirmed state without moving focus.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn spotify_browse_edited(
        &mut self,
        ui: &mut dyn ShellUi,
        stamp: SpotifyStamp,
        result: Result<EditOutcome, SpotifyError>,
    ) {
        use EditOutcome as O;
        if !self.spotify.is_current_account(stamp) {
            return;
        }
        let texts = self.catalog();
        let Some((wanted, pending)) = self.spotify.browse.pending.take() else {
            return;
        };
        if wanted != stamp {
            self.spotify.browse.pending = Some((wanted, pending));
            return;
        }
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                ui.set_status(&apricot_app::spotify::error_text(&texts, &error), true);
                return;
            }
        };
        let mut follow_up = None;
        let mut select = None;
        let text = match (&outcome, pending) {
            (O::SavedBatch { uris, saved }, _) => {
                let changed: std::collections::HashSet<_> = uris.iter().collect();
                for frame in &mut self.spotify.browse.frames {
                    for item in &mut frame.items {
                        if changed.contains(&item.uri) {
                            item.saved = Some(*saved);
                        }
                    }
                    if !saved && matches!(frame.source, Source::LikedSongs) {
                        frame.remove_items(|item| !changed.contains(&item.uri));
                    }
                }
                self.spotify.browse.marks.clear();
                texts
                    .text(if *saved {
                        "spotify_liked_done"
                    } else {
                        "spotify_unliked_done"
                    })
                    .replace("{count}", &uris.len().to_string())
            }
            (O::Saved { uri, saved }, _) => {
                self.spotify.browse.saved_states.insert(uri.clone(), *saved);
                let kind = ItemKind::from_uri_public(uri);
                for frame in &mut self.spotify.browse.frames {
                    for item in frame.items.iter_mut().filter(|item| item.uri == *uri) {
                        item.saved = Some(*saved);
                    }
                    if matches!(&frame.source, Source::Profile { uri: profile, .. } if profile == uri)
                    {
                        frame.following = Some(*saved);
                    }
                    if !*saved && matches!(frame.source, Source::LikedSongs) {
                        frame.remove_items(|item| item.uri != *uri);
                    }
                }
                let key = match (kind, *saved) {
                    (Some(ItemKind::Track | ItemKind::Episode), true) => "spotify_liked_done",
                    (Some(ItemKind::Track | ItemKind::Episode), false) => "spotify_unliked_done",
                    (Some(ItemKind::Artist | ItemKind::User), true) => "spotify_followed_done",
                    (Some(ItemKind::Artist | ItemKind::User), false) => "spotify_unfollowed_done",
                    (_, true) => "spotify_saved_done",
                    (_, false) => "spotify_unsaved_done",
                };
                texts.text(key).to_owned()
            }
            (O::AddedToPlaylist { .. }, Pending::Add { name, count }) => {
                // The marks are used up once Spotify confirmed them.
                self.spotify.browse.marks.clear();
                if count > 1 {
                    texts
                        .text("spotify_added_count")
                        .replace("{count}", &count.to_string())
                        .replace("{name}", &name)
                } else {
                    texts
                        .text("spotify_added_to_playlist")
                        .replace("{name}", &name)
                }
            }
            (O::RemovedFromPlaylist { .. }, Pending::Remove { uid }) => {
                if let Some(frame) = self.spotify.browse.frames.last_mut() {
                    frame.remove_items(|item| item.uid.as_deref() != Some(uid.as_str()));
                }
                texts.text("spotify_removed_from_playlist").to_owned()
            }
            (O::Moved { .. }, Pending::Move { uid, other_uid }) => {
                if let Some(frame) = self.spotify.browse.frames.last_mut() {
                    let position = |wanted: &str| {
                        frame
                            .items
                            .iter()
                            .position(|item| item.uid.as_deref() == Some(wanted))
                    };
                    if let (Some(from), Some(to)) = (position(&uid), position(&other_uid)) {
                        frame.items.swap(from, to);
                        frame.selected = to;
                        select = Some(to);
                    }
                }
                texts.text("spotify_playlist_updated").to_owned()
            }
            (O::Created { uri, name }, Pending::Create { then_add }) => {
                let created = CatalogItem {
                    kind: ItemKind::Playlist,
                    uri: uri.clone(),
                    name: name.clone(),
                    subtitle: String::new(),
                    album: String::new(),
                    album_uri: String::new(),
                    artist_uri: String::new(),
                    duration_ms: None,
                    playable: true,
                    explicit: false,
                    uid: None,
                    saved: Some(true),
                    count: None,
                    editable: true,
                    format: String::new(),
                };
                if let Some(playlists) = &mut self.spotify.browse.playlists {
                    playlists.insert(0, created.clone());
                }
                for frame in &mut self.spotify.browse.frames {
                    if matches!(frame.source, Source::Library { folder: None, .. }) && frame.loaded
                    {
                        frame.items.insert(0, created.clone());
                    }
                }
                if let Some(uris) = then_add {
                    let count = uris.len();
                    follow_up = Some((
                        LibraryEdit::AddToPlaylist {
                            playlist: uri.clone(),
                            uris,
                        },
                        Pending::Add {
                            name: name.clone(),
                            count,
                        },
                    ));
                }
                texts
                    .text("spotify_playlist_created")
                    .replace("{name}", name)
            }
            (O::Hidden { uri, hidden }, _) => {
                let set = self.spotify.browse.hidden.get_or_insert_default();
                if *hidden {
                    set.insert(uri.clone());
                } else {
                    set.remove(uri);
                }
                texts
                    .text(if *hidden {
                        "spotify_hidden_done"
                    } else {
                        "spotify_unhidden_done"
                    })
                    .to_owned()
            }
            (O::Renamed { uri, name }, _) => {
                for frame in &mut self.spotify.browse.frames {
                    for item in frame.items.iter_mut().filter(|item| item.uri == *uri) {
                        item.name.clone_from(name);
                    }
                }
                if let Some(playlists) = &mut self.spotify.browse.playlists {
                    for playlist in playlists.iter_mut().filter(|playlist| playlist.uri == *uri) {
                        playlist.name.clone_from(name);
                    }
                }
                texts
                    .text("spotify_playlist_renamed")
                    .replace("{name}", name)
            }
            (O::Described { .. }, _) => texts.text("spotify_description_saved").to_owned(),
            (O::Visibility { uri, private }, _) => {
                let name = self
                    .spotify
                    .browse
                    .frames
                    .iter()
                    .flat_map(|frame| frame.items.iter())
                    .find(|item| item.uri == *uri)
                    .map(|item| item.name.clone())
                    .unwrap_or_default();
                texts
                    .text(if *private {
                        "spotify_now_private"
                    } else {
                        "spotify_now_public"
                    })
                    .replace("{name}", &name)
            }
            _ => texts.text("spotify_playlist_updated").to_owned(),
        };
        ui.set_status(&text, true);
        if self.view == View::SpotifyBrowse {
            self.browse_render(ui, false);
        }
        let _ = select;
        if let Some((edit, pending)) = follow_up {
            self.browse_begin_edit(edit, pending);
        }
    }

    /// A context menu command on the selected row (Windows `command`).
    pub(crate) fn spotify_browse_command(&mut self, ui: &mut dyn ShellUi, command: ContextCommand) {
        use ContextCommand as C;
        if command == C::SpotifyToggleMenuPin {
            self.spotify_toggle_pin(ui);
            return;
        }
        if command == C::SpotifyToggleArtistFollow {
            if let Some(uri) = self.spotify_context_artist(ui) {
                self.browse_begin_edit(LibraryEdit::ToggleSaved { uri }, Pending::Saved);
            }
            return;
        }
        if self.view == View::SpotifyHub
            && let Some(pin) = self.spotify_selected_pin(ui).cloned()
        {
            if command == C::SpotifyOpen {
                self.spotify_open_pin(ui, &pin);
                return;
            }
            if matches!(command, C::SpotifyPlay | C::SpotifyShufflePlay) {
                self.browse_play(ui, &pin.item(), command == C::SpotifyShufflePlay);
                return;
            }
        }
        self.browse_remember_selection(ui);
        if command == C::SpotifyCreatePlaylist {
            self.spotify_create_playlist(ui);
            return;
        }
        let Some(item) = self.browse_selected_item(ui).cloned() else {
            return;
        };
        match command {
            C::SpotifyPlay => self.browse_play(ui, &item, false),
            C::SpotifyShufflePlay => self.browse_play(ui, &item, true),
            C::SpotifyOpen => self.browse_open_item(ui, &item),
            C::SpotifyAddToQueue => {
                self.spotify_add_to_queue(ui);
            }
            C::SpotifyGoToAlbum => {
                self.browse_open(
                    ui,
                    Source::Album(item.album_uri.clone()),
                    item.album.clone(),
                );
            }
            C::SpotifyGoToArtist => {
                let name = item
                    .subtitle
                    .split(", ")
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                self.browse_open(ui, Source::Artist(item.artist_uri.clone()), name);
            }
            C::SpotifyCopyLink => {
                if let Some(link) = self.browse_selected_link(ui) {
                    self.copy_text_and_announce(ui, &link, "url_copied");
                }
            }
            C::SpotifyToggleSaved => self.spotify_toggle_saved(ui),
            C::SpotifyAddToPlaylist => {
                self.browse_begin_selection(ui, &item.uri, None);
            }
            C::SpotifyRemoveFromPlaylist => self.spotify_browse_remove_selected(ui),
            C::SpotifyMoveUp => self.browse_move_selected(ui, true),
            C::SpotifyMoveDown => self.browse_move_selected(ui, false),
            C::SpotifyRenamePlaylist => self.browse_rename_selected(ui),
            C::SpotifyHide => self.spotify_toggle_hidden(ui),
            C::SpotifyRadio => self.spotify_radio(ui),
            C::SpotifyGoToOwner => {
                let source = self.browse_profile_source(&item.artist_uri);
                self.browse_open(ui, source, item.subtitle.clone());
            }
            C::SpotifyEditDescription => self.browse_load_for(
                ui,
                CatalogRequest::Playlist {
                    uri: item.uri.clone(),
                    offset: 0,
                },
                Pending::Describe {
                    uri: item.uri.clone(),
                },
            ),
            C::SpotifyToggleVisibility => self.browse_begin_edit(
                LibraryEdit::ToggleVisibility {
                    uri: item.uri.clone(),
                },
                Pending::Plain,
            ),
            C::SpotifyPlayPreview => self.browse_load_for(
                ui,
                CatalogRequest::Preview(item.uri.clone()),
                Pending::Preview {
                    name: item.name.clone(),
                },
            ),
            _ => {}
        }
    }

    fn spotify_selected_pin(
        &self,
        ui: &dyn ShellUi,
    ) -> Option<&apricot_spotify::main_menu::MenuPin> {
        let SpotifyHubEntry::Pinned(index) =
            self.spotify.hub_entries.get(ui.selected_row()?).copied()?
        else {
            return None;
        };
        self.spotify.hub_pins.get(index)
    }

    fn spotify_pin_for_item(
        &self,
        item: &CatalogItem,
    ) -> Option<apricot_spotify::main_menu::MenuPin> {
        use apricot_spotify::main_menu::{MenuOrigin, MenuPin};
        let origin = self.browse_top().and_then(|frame| match &frame.source {
            Source::Home => Some(MenuOrigin::Home),
            Source::Browse(None) => Some(MenuOrigin::BrowseAll),
            Source::Browse(Some(uri)) => Some(MenuOrigin::BrowsePage(uri.clone())),
            _ => None,
        });
        MenuPin::from_item(item, origin)
    }

    fn spotify_open_pin(
        &mut self,
        ui: &mut dyn ShellUi,
        pin: &apricot_spotify::main_menu::MenuPin,
    ) {
        use apricot_spotify::main_menu::MenuTarget;
        let source = match &pin.target {
            MenuTarget::Playlist(uri) => Source::Playlist(uri.clone()),
            MenuTarget::Artist(uri) => Source::Artist(uri.clone()),
            MenuTarget::Category(uri) => Source::Browse(Some(uri.clone())),
            MenuTarget::Section { origin, uri } => Source::MenuSection {
                origin: origin.clone(),
                uri: uri.clone(),
            },
        };
        self.browse_open(ui, source, pin.label.clone());
    }

    fn spotify_toggle_pin(&mut self, ui: &mut dyn ShellUi) {
        let pin = if self.view == View::SpotifyHub {
            self.spotify_selected_pin(ui).cloned()
        } else {
            self.browse_selected_item(ui)
                .and_then(|item| self.spotify_pin_for_item(item))
        };
        let Some(pin) = pin else {
            return;
        };
        let selected_pin = if self.view == View::SpotifyHub {
            ui.selected_row()
                .and_then(|row| self.spotify.hub_entries.get(row))
                .and_then(|entry| {
                    if let SpotifyHubEntry::Pinned(index) = entry {
                        Some(*index)
                    } else {
                        None
                    }
                })
        } else {
            None
        };
        let Some(service) = self.spotify_service_ref() else {
            return;
        };
        match service.toggle_menu_pin(pin) {
            Ok(added) => {
                self.spotify.hub_pins = service.menu_pins().unwrap_or_default();
                ui.set_status(
                    &self.text(if added {
                        "spotify_add_main_menu"
                    } else {
                        "spotify_remove_main_menu"
                    }),
                    true,
                );
                if self.view == View::SpotifyHub {
                    self.spotify.hub_selected = Some(if self.spotify.hub_pins.is_empty() {
                        SpotifyHubEntry::MyProfile
                    } else {
                        SpotifyHubEntry::Pinned(
                            selected_pin
                                .unwrap_or(0)
                                .min(self.spotify.hub_pins.len() - 1),
                        )
                    });
                    self.show_spotify_hub(ui);
                } else {
                    self.browse_render(ui, false);
                }
            }
            Err(error) => ui.show_error(&error),
        }
    }

    fn spotify_context_identity(&self, ui: &dyn ShellUi) -> Option<String> {
        self.browse_selected_item(ui)
            .map(|item| item.uri.clone())
            .or_else(|| {
                self.application
                    .player_session()
                    .current_item()
                    .filter(|item| {
                        item.source == apricot_core::MediaSource::Spotify
                            && (self.view == View::Player || ui.focused_player_control().is_some())
                    })
                    .map(|item| item.id.0.clone())
            })
            .map(|uri| {
                format!(
                    "{:?}:{}:{:?}:{}:{uri}",
                    self.view,
                    self.spotify.browse.frames.len(),
                    ui.selected_row(),
                    ui.focused_player_control().unwrap_or("")
                )
            })
    }

    fn spotify_context_artist(&self, ui: &dyn ShellUi) -> Option<String> {
        self.browse_selected_item(ui)
            .map(|item| {
                if item.kind == ItemKind::Artist {
                    item.uri.clone()
                } else {
                    item.artist_uri.clone()
                }
            })
            .filter(|uri| uri.starts_with("spotify:artist:"))
            .or_else(|| {
                self.application
                    .player_session()
                    .current_item()
                    .filter(|_| self.view == View::Player || ui.focused_player_control().is_some())
                    .and_then(|item| {
                        item.metadata
                            .get("spotify_artist_uri")?
                            .as_str()
                            .map(str::to_owned)
                    })
            })
    }

    pub(crate) fn spotify_prepare_context_menu(&mut self, ui: &mut dyn ShellUi) -> bool {
        let Some(target) = self.spotify_context_identity(ui) else {
            return false;
        };
        let mut uris = Vec::new();
        if let Some(artist) = self.spotify_context_artist(ui) {
            uris.push(artist);
        }
        if let Some(item) = self.browse_selected_item(ui)
            && matches!(item.kind, ItemKind::Artist | ItemKind::Playlist)
            && !uris.contains(&item.uri)
        {
            uris.push(item.uri.clone());
        }
        if uris.is_empty() {
            return false;
        }
        // Never guess the state after a failed read.
        for uri in &uris {
            self.spotify.browse.saved_states.remove(uri);
            for frame in &mut self.spotify.browse.frames {
                for item in &mut frame.items {
                    if item.uri == *uri {
                        item.saved = None;
                    }
                }
            }
        }
        if self.spotify_service_ref().is_none() {
            return false;
        }
        let stamp = self.spotify.epochs.begin();
        self.spotify.browse.context_read = Some((stamp, target, uris.clone()));
        self.spotify_service_ref()
            .unwrap()
            .load_catalog(stamp, CatalogRequest::SavedStates(uris));
        ui.set_status(&self.text("spotify_loading"), false);
        true
    }

    pub(crate) fn spotify_extend_player_context(
        &self,
        ui: &dyn ShellUi,
        entries: &mut Vec<ContextMenuEntry>,
    ) {
        let Some(uri) = self.spotify_context_artist(ui) else {
            return;
        };
        if let Some(followed) = self.spotify.browse.saved_states.get(&uri) {
            entries.push(ContextMenuEntry::Command {
                label: self.text(if *followed {
                    "spotify_unfollow_artist"
                } else {
                    "spotify_follow_artist"
                }),
                command: ContextCommand::SpotifyToggleArtistFollow,
            });
        } else {
            entries.push(ContextMenuEntry::Disabled {
                label: self.text("spotify_artist_state_unavailable"),
            });
        }
    }

    /// Loads what a command needs first ("Loading."), then goes on in
    /// `spotify_browse_loaded` with `pending`.
    fn browse_load_for(&mut self, ui: &mut dyn ShellUi, request: CatalogRequest, pending: Pending) {
        let Some(service) = self.spotify_service() else {
            return;
        };
        ui.set_status(&self.text("spotify_loading"), false);
        let stamp = self.spotify.epochs.begin();
        self.spotify.browse.pending = Some((stamp, pending));
        service.load_catalog(stamp, request);
    }

    /// "Edit description": the current one to change; empty removes it.
    fn browse_describe(&mut self, ui: &mut dyn ShellUi, uri: String, current: &str) {
        let description = self.browse_ask_text(
            ui,
            "spotify_edit_description",
            "spotify_description",
            current,
            true,
        );
        ui.focus_primary_control();
        let Some(description) = description else {
            return;
        };
        self.browse_begin_edit(
            LibraryEdit::DescribePlaylist { uri, description },
            Pending::Plain,
        );
    }

    /// "Play preview": Spotify's short preview plays as its own item.
    fn browse_play_preview(&mut self, ui: &mut dyn ShellUi, name: &str, url: Option<&str>) {
        let Some(url) = url.and_then(|url| url.parse::<url::Url>().ok()) else {
            ui.set_status(&self.text("spotify_no_preview"), true);
            return;
        };
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("spotify_preview".to_owned(), serde_json::Value::Bool(true));
        let item = MediaItem {
            id: apricot_core::MediaId(url.to_string()),
            source: apricot_core::MediaSource::Podcast,
            kind: apricot_core::MediaKind::Audio,
            title: self.text("spotify_preview_title").replace("{name}", name),
            url: Some(url.clone()),
            stream_url: Some(url),
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata,
        };
        self.start_media_item(ui, item, None);
    }

    /// Ctrl+Shift+Q on a Spotify track or episode joins the Spotify queue
    /// while Spotify plays here; `false` for other targets.
    pub(crate) fn spotify_add_to_queue(&mut self, ui: &mut dyn ShellUi) -> bool {
        let reference = self
            .browse_selected_item(ui)
            .map(|item| item.uri.clone())
            .or_else(|| {
                self.active_media_item(ui)
                    .filter(|item| item.source == apricot_core::MediaSource::Spotify)
                    .map(|item| item.id.0)
            })
            .and_then(|uri| SpotifyRef::parse(&uri));
        let Some(reference) = reference.filter(|reference| {
            matches!(
                reference.kind,
                SpotifyEntityKind::Track | SpotifyEntityKind::Episode
            )
        }) else {
            return false;
        };
        let playback = self
            .spotify_service_ref()
            .and_then(apricot_spotify::SpotifyService::playback)
            .filter(|playback| {
                playback
                    .player_state()
                    .is_some_and(|player| apricot_spotify::queue::snapshot(&player).active)
            });
        let key = if let Some(playback) = playback {
            playback.add_to_queue(reference.to_uri());
            "spotify_queue_added"
        } else {
            "spotify_queue_inactive"
        };
        ui.set_status(&self.text(key), true);
        true
    }
}

#[cfg(test)]
impl Shell {
    /// Tests: an open list of `items` from `source`, as if loaded.
    pub(crate) fn spotify_browse_test_open(
        &mut self,
        ui: &mut dyn ShellUi,
        source: Source,
        title: &str,
        items: Vec<CatalogItem>,
        can_edit: bool,
    ) {
        self.spotify.browse.frames.push(Frame {
            source,
            title: title.to_owned(),
            items,
            next_offset: None,
            selected: 0,
            loading: None,
            loaded: true,
            refresh: None,
            error: None,
            mixed: false,
            can_edit,
            can_rename: false,
            personalised: false,
            following: None,
            sections: Vec::new(),
            search_screen: false,
            field_value: None,
            focus_results: false,
        });
        self.view = View::SpotifyBrowse;
        self.browse_render(ui, true);
    }

    pub(crate) fn spotify_browse_test_profile(&mut self, ui: &mut dyn ShellUi) {
        self.browse_open(
            ui,
            Source::MyProfile("spotify:user:me".into()),
            "My profile".into(),
        );
    }
    pub(crate) fn spotify_browse_test_activate(&mut self, ui: &mut dyn ShellUi) {
        self.browse_remember_selection(ui);
        let item = self.browse_selected_item(ui).cloned().unwrap();
        self.browse_open_item(ui, &item);
    }

    pub(crate) fn spotify_browse_test_hub_pins(
        &mut self,
        pins: Vec<apricot_spotify::main_menu::MenuPin>,
    ) {
        self.spotify.hub_entries = (0..pins.len()).map(SpotifyHubEntry::Pinned).collect();
        self.spotify.hub_pin_items = pins
            .iter()
            .map(apricot_spotify::main_menu::MenuPin::item)
            .collect();
        self.spotify.hub_pins = pins;
        self.view = View::SpotifyHub;
    }
    pub(crate) fn spotify_browse_test_source(&self) -> Option<Source> {
        self.browse_top().map(|frame| frame.source.clone())
    }
    pub(crate) fn spotify_browse_test_artist(&self, ui: &dyn ShellUi) -> Option<String> {
        self.spotify_context_artist(ui)
    }

    pub(crate) fn spotify_browse_test_next_page(&mut self, offset: u64) {
        self.spotify.browse.frames.last_mut().unwrap().next_offset = Some(offset);
    }

    pub(crate) fn spotify_browse_test_personalised(&mut self) {
        if let Some(frame) = self.spotify.browse.frames.last_mut()
            && let Source::Playlist(context) = &frame.source
        {
            frame.personalised = true;
            self.spotify
                .browse
                .personalised_contexts
                .insert(context.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_play_in_the_context_of_their_collection() {
        assert_eq!(
            Source::Album("spotify:album:a".into()).context(),
            Some("spotify:album:a")
        );
        assert_eq!(Source::LikedSongs.context(), None);
    }

    #[test]
    fn requests_continue_at_the_offset() {
        assert_eq!(
            Source::Playlist("spotify:playlist:p".into()).request(50),
            Some(CatalogRequest::Playlist {
                uri: "spotify:playlist:p".into(),
                offset: 50
            })
        );
        assert_eq!(Source::Section(Vec::new()).request(0), None);
    }
}
