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
            Self::Section(_) => return None,
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
            Self::Album(uri) => CatalogRequest::Album(uri.clone()),
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
    /// Ctrl+A waits for the remaining pages, then marks every song.
    mark_all: bool,
}

/// A change waiting for Spotify's answer (Windows `Pending`).
enum Pending {
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
    /// The last search, offered again by the search dialog.
    last_query: String,
    last_kind: usize,
    pending: Option<(SpotifyStamp, Pending)>,
    /// Playlists the account may add to, read once per session.
    playlists: Option<Vec<CatalogItem>>,
    /// Songs the account hid, read when the first personal mix opens.
    hidden: Option<std::collections::HashSet<String>>,
    hidden_load: Option<SpotifyStamp>,
    /// When the open playlist was last checked.
    refreshed_at: Option<Instant>,
    /// Songs marked with Shift+A or Ctrl+A for "Add to Spotify playlist".
    marks: apricot_app::spotify::SpotifyMarks,
}

/// The Spotify search dialog's texts (Windows `SearchTexts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotifySearchTexts {
    pub title: String,
    pub query: String,
    pub kind: String,
    pub kinds: Vec<String>,
    pub search: String,
    pub cancel: String,
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
        if !self.spotify_ready_for_lists(ui) {
            return;
        }
        let catalog = self.catalog();
        let texts = SpotifySearchTexts {
            title: catalog.text("spotify_search_dialog").to_owned(),
            query: catalog.text("spotify_search_query").to_owned(),
            kind: catalog.text("spotify_search_type").to_owned(),
            kinds: apricot_app::spotify::search_kind_labels(&catalog),
            search: catalog.text("search").to_owned(),
            cancel: catalog.text("cancel").to_owned(),
        };
        let query = self.spotify.browse.last_query.clone();
        let kind = self.spotify.browse.last_kind;
        let Some((query, kind)) = ui.spotify_search(&texts, &query, kind) else {
            ui.focus_primary_control();
            return;
        };
        self.spotify.browse.last_query.clone_from(&query);
        self.spotify.browse.last_kind = kind;
        let title = self
            .text("spotify_search_results")
            .replace("{query}", &query);
        let searching = self.text("spotify_searching").replace("{query}", &query);
        ui.set_status(&searching, false);
        let kind = SearchKind::ALL
            .get(kind)
            .copied()
            .unwrap_or(SearchKind::All);
        self.browse_open_root(ui, Source::Search { query, kind }, title);
    }

    /// Opens a new list on top of the current one and loads its first
    /// page.
    fn browse_open(&mut self, ui: &mut dyn ShellUi, source: Source, title: String) {
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
            mark_all: false,
        };
        if let Source::Section(items) = &frame.source {
            frame.items.clone_from(items);
            frame.loaded = true;
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
        let Some(service) = self.spotify_service() else {
            return;
        };
        let stamp = self.spotify.epochs.begin();
        let Some(frame) = self.spotify.browse.frames.last_mut() else {
            return;
        };
        let Some(request) = frame.source.request(offset) else {
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
        let texts = self.catalog();
        let pending_load = matches!(
            self.spotify.browse.pending,
            Some((
                wanted,
                Pending::Playlists { .. }
                    | Pending::Describe { .. }
                    | Pending::Preview { .. }
                    | Pending::Duplicates { .. }
            ))
                if wanted == stamp
        );
        if pending_load && let Some((_, pending)) = self.spotify.browse.pending.take() {
            match (pending, result) {
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
                CatalogResult::Playlists(_)
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
            if self.spotify.browse.frames[index].mark_all {
                self.browse_continue_mark_all(ui);
            }
            self.browse_render(ui, false);
        }
    }

    /// Ctrl+A: every song of the open list, also the pages not loaded yet,
    /// is marked for "Add to Spotify playlist".
    pub(crate) fn spotify_mark_all(&mut self, ui: &mut dyn ShellUi) {
        if self.view != View::SpotifyBrowse {
            return;
        }
        let Some(frame) = self.spotify.browse.frames.last_mut() else {
            return;
        };
        frame.mark_all = true;
        let more = frame.next_offset.is_some() || frame.loading.is_some();
        if more {
            ui.set_status(&self.text("spotify_marking_all"), true);
        }
        self.browse_continue_mark_all(ui);
        self.browse_render(ui, false);
    }

    /// Loads the next page for Ctrl+A, or marks the songs when all are in.
    fn browse_continue_mark_all(&mut self, ui: &mut dyn ShellUi) {
        let Some(frame) = self.spotify.browse.frames.last() else {
            return;
        };
        if frame.loading.is_some() {
            return;
        }
        if let Some(offset) = frame.next_offset {
            self.browse_request(offset);
            return;
        }
        let songs: Vec<String> = frame
            .items
            .iter()
            .filter(|item| item.kind.is_playable_item() && item.playable)
            .map(|item| item.uri.clone())
            .collect();
        let count = songs.len();
        if let Some(frame) = self.spotify.browse.frames.last_mut() {
            frame.mark_all = false;
        }
        self.spotify.browse.marks.mark_all(songs);
        ui.set_status(
            &self
                .text("spotify_marked_all")
                .replace("{count}", &count.to_string()),
            true,
        );
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
        ui.set_status(&self.text(key).replace("{count}", &count), true);
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
        let screen = ListScreen {
            list_name: frame.title.clone(),
            rows,
            selected: Some(selected),
            controls: vec![
                ScreenControl::List,
                button("open", "open"),
                button("back", "back"),
            ],
            focus_list: focus,
        };
        if matches!(frame.source, Source::Playlist(_)) {
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
        if let Some(media) = self.browse_media_for(item, shuffle) {
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
        );
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
            collection_plays: collection && !matches!(item.kind, ItemKind::Folder | ItemKind::User),
            album: !item.album_uri.is_empty(),
            artist: item.artist_uri.starts_with("spotify:artist:"),
            link: SpotifyRef::parse(&item.uri).is_some(),
            saved: if profile_section {
                frame.following
            } else {
                item.saved
            },
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

    /// `spotify_radio`: Spotify's radio for the selected track, artist,
    /// album or playlist, or for the playing Spotify track.
    pub(crate) fn spotify_radio(&mut self, ui: &mut dyn ShellUi) {
        let target = self
            .browse_selected_item(ui)
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
            });
        let Some((seed, name)) = target else {
            return;
        };
        let title = self.text("spotify_radio_title").replace("{name}", &name);
        self.browse_open(ui, Source::Radio(seed), title);
    }

    /// `spotify_dislike`: Spotify's "Hide song" in a personal mix, or
    /// showing it again. Elsewhere it says where it works.
    pub(crate) fn spotify_toggle_hidden(&mut self, ui: &mut dyn ShellUi) {
        let frame = self
            .browse_top()
            .filter(|_| self.view == View::SpotifyBrowse);
        let target = frame.filter(|frame| frame.personalised).and_then(|frame| {
            let Source::Playlist(context) = &frame.source else {
                return None;
            };
            self.browse_selected_item(ui)
                .filter(|item| item.kind == ItemKind::Track)
                .map(|item| (item.uri.clone(), context.clone()))
        });
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
    pub(crate) fn spotify_toggle_saved(&mut self, ui: &mut dyn ShellUi) {
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
        let Some(uri) = uri.filter(|uri| uri != "spotify:collection:tracks") else {
            return;
        };
        self.browse_begin_edit(LibraryEdit::ToggleSaved { uri }, Pending::Saved);
    }

    /// Delete in an editable playlist: removes exactly the selected
    /// occurrence.
    pub(crate) fn spotify_browse_remove_selected(&mut self, ui: &mut dyn ShellUi) {
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
                        CatalogRequest::Playlist {
                            uri: playlist.uri.clone(),
                            offset: 0,
                        },
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
            (O::Saved { uri, saved }, _) => {
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
                        frame.items.retain(|item| item.uri != *uri);
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
                    frame
                        .items
                        .retain(|item| item.uid.as_deref() != Some(uid.as_str()));
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
                let songs = self.browse_songs_to_add(&item.uri);
                self.browse_add_to_playlist(ui, songs);
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
            mark_all: false,
        });
        self.view = View::SpotifyBrowse;
        self.browse_render(ui, true);
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
