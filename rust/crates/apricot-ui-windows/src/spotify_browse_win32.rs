//! Spotify lists in the main window (`docs/SPOTIFY_PLAN.md` 4.2, 4.3):
//! search results, the library, Liked Songs, playlists, albums, artists and
//! shows. Every opened list is a frame on a stack, so Escape goes back one
//! level to the same row. Lists load in pages; reaching the last row loads
//! the next page, as Apricot's own result lists do. Answers that arrive late
//! (another frame, another account) are dropped. Focus moves only when the
//! user opens or leaves a list.

use apricot_app::spotify::{item_label, kind_word};
use apricot_core::{Route, SpotifyStamp};
use apricot_spotify::{
    CatalogItem, CatalogRequest, CatalogResult, ItemKind, LibraryFilter, SearchKind, SpotifyError,
    SpotifyTrack,
};
use windows::Win32::{
    Foundation::{HWND, WPARAM},
    UI::{
        Input::KeyboardAndMouse::SetFocus,
        WindowsAndMessaging::{
            KillTimer, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, SendMessageW, SetTimer,
        },
    },
};

use super::{
    MainView, WindowState, add_list_string, layout_controls_state, set_status, state, state_mut,
};

/// Where a list comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Source {
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
    /// Spotify's radio for a track, artist, album or playlist; it becomes
    /// the station playlist once Spotify names it.
    Radio(String),
    /// Top tracks and artists with their six section titles.
    Top([String; 6]),
    /// Another account's profile with its section titles (public
    /// playlists, following, followers).
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

#[allow(clippy::struct_excessive_bools)]
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
}

/// A change waiting for Spotify's answer.
enum Pending {
    Saved,
    Add {
        name: String,
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

#[derive(Default)]
pub(super) struct BrowseState {
    frames: Vec<Frame>,
    /// The last search, offered again by the search dialog.
    pub(super) last_query: String,
    pub(super) last_kind: usize,
    pending: Option<(SpotifyStamp, Pending)>,
    /// Playlists the account may add to, read once per session.
    playlists: Option<Vec<CatalogItem>>,
    /// Songs the account hid, read when the first personal mix opens.
    hidden: Option<std::collections::HashSet<String>>,
    hidden_load: Option<SpotifyStamp>,
}

pub(super) const fn is_view(view: MainView) -> bool {
    matches!(view, MainView::SpotifyBrowse)
}

fn top(state: &WindowState) -> Option<&Frame> {
    state.spotify_browse.frames.last()
}

/// The selected row, for the context menu and shortcuts.
pub(super) fn selected_item(state: &WindowState) -> Option<&CatalogItem> {
    if state.view != MainView::SpotifyBrowse {
        return None;
    }
    let frame = top(state)?;
    // SAFETY: Reads the selection of this thread's list.
    let selected = unsafe { SendMessageW(state.list, LB_GETCURSEL, None, None).0 };
    frame.items.get(usize::try_from(selected).ok()?)
}

/// Opens a new list on top of the current one and loads its first page.
pub(super) unsafe fn open(window: HWND, source: Source, title: String) {
    super::spotify::prepare_screen(window, MainView::SpotifyBrowse, Route::SpotifyBrowse);
    let Some(state) = state_mut(window) else {
        return;
    };
    let mixed = matches!(
        source,
        Source::Search {
            kind: SearchKind::All,
            ..
        }
    );
    // A list opened from the hub replaces the stack; from a list it nests.
    if !is_view(state.view) {
        state.spotify_browse.frames.clear();
    }
    state.spotify_browse.frames.push(Frame {
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
    });
    if let Some(frame) = state.spotify_browse.frames.last_mut()
        && let Source::Section(items) = &frame.source
    {
        frame.items = items.clone();
        frame.loaded = true;
    }
    state.view = MainView::SpotifyBrowse;
    request(window, 0);
    render(window, true);
}

/// Opens a list from the hub or a global shortcut: the stack starts over.
pub(super) unsafe fn open_root(window: HWND, source: Source, title: String) {
    if let Some(state) = state_mut(window) {
        state.spotify_browse.frames.clear();
        if state.view == MainView::SpotifyBrowse {
            state.view = MainView::SpotifyHub;
        }
    }
    open(window, source, title);
}

unsafe fn request(window: HWND, offset: u64) {
    let Some(service) = super::spotify::service(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let stamp = state.spotify.epochs_begin();
    let Some(frame) = state.spotify_browse.frames.last_mut() else {
        return;
    };
    let Some(request) = frame.source.request(offset) else {
        return;
    };
    frame.loading = Some(stamp);
    service.load_catalog(stamp, request);
}

/// The answer to a list request.
#[allow(clippy::too_many_lines)]
pub(super) unsafe fn loaded(
    window: HWND,
    stamp: SpotifyStamp,
    result: Result<CatalogResult, SpotifyError>,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let pending_load = matches!(
        state.spotify_browse.pending,
        Some((wanted, Pending::Playlists { .. } | Pending::Describe { .. } | Pending::Preview { .. }))
            if wanted == stamp
    );
    if pending_load && let Some((_, pending)) = state.spotify_browse.pending.take() {
        match (pending, result) {
            (Pending::Playlists { uris }, Ok(CatalogResult::Playlists(playlists))) => {
                state.spotify_browse.playlists = Some(playlists);
                choose_playlist(window, uris);
            }
            (Pending::Describe { uri }, Ok(CatalogResult::Collection(collection))) => {
                describe(window, uri, &collection.description);
            }
            (Pending::Preview { name }, Ok(CatalogResult::Preview(url))) => {
                play_preview(window, &name, url.as_deref());
            }
            (_, Err(error)) => {
                let text = apricot_app::spotify::error_text(&texts, &error);
                set_status(state, &text, true);
            }
            _ => {}
        }
        return;
    }
    if refreshed(window, stamp, &result) {
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.spotify_browse.hidden_load == Some(stamp) {
        state.spotify_browse.hidden_load = None;
        if let Ok(CatalogResult::HiddenSongs(uris)) = result {
            state.spotify_browse.hidden = Some(uris.into_iter().collect());
            if state.view == MainView::SpotifyBrowse {
                render(window, false);
            }
        }
        return;
    }
    let Some(index) = state
        .spotify_browse
        .frames
        .iter()
        .position(|frame| frame.loading == Some(stamp))
    else {
        return;
    };
    let is_top = index + 1 == state.spotify_browse.frames.len();
    let frame = &mut state.spotify_browse.frames[index];
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
                request(window, 0);
            }
            return;
        }
        Ok(
            CatalogResult::Playlists(_) | CatalogResult::HiddenSongs(_) | CatalogResult::Preview(_),
        ) => {}
        Err(error) => {
            let text = apricot_app::spotify::error_text(&texts, &error);
            if first_page {
                frame.error = Some(text.clone());
            }
            set_status(state, &text, true);
        }
    }
    let needs_hidden = state.spotify_browse.frames[index].personalised
        && state.spotify_browse.hidden.is_none()
        && state.spotify_browse.hidden_load.is_none();
    if needs_hidden && let Some(service) = super::spotify::service(window) {
        let stamp = state.spotify.epochs_begin();
        state.spotify_browse.hidden_load = Some(stamp);
        service.load_catalog(stamp, CatalogRequest::HiddenSongs);
    }
    if is_top && state.view == MainView::SpotifyBrowse {
        render(window, false);
    }
}

/// Refills the list. `focus` puts the focus on it (opening, going back);
/// otherwise the focus stays where it is.
unsafe fn render(window: HWND, focus: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let Some(frame) = state.spotify_browse.frames.last() else {
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
                if state
                    .spotify_browse
                    .hidden
                    .as_ref()
                    .is_some_and(|hidden| hidden.contains(&item.uri))
                {
                    label.push_str(", ");
                    label.push_str(texts.text("spotify_hidden"));
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
    let title = frame.title.clone();
    let selected = frame.selected.min(rows.len().saturating_sub(1));
    super::set_open_button_label(state, "open");
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    crate::accessibility_win32::set_control_name(state.list, &title);
    for row in &rows {
        add_list_string(state.list, row);
    }
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    let watch = matches!(frame.source, Source::Playlist(_));
    layout_controls_state(window, state);
    if focus {
        let _ = SetFocus(Some(state.list));
    }
    if watch {
        let _ = SetTimer(Some(window), REFRESH_TIMER_ID, REFRESH_INTERVAL_MS, None);
    } else {
        let _ = KillTimer(Some(window), REFRESH_TIMER_ID);
    }
}

/// `WM_TIMER` id of the open playlist check.
pub(super) const REFRESH_TIMER_ID: usize = 21;
/// An open playlist is checked this often for changes made elsewhere.
const REFRESH_INTERVAL_MS: u32 = 15_000;

/// Timer: the open playlist asks Spotify for its current state. Nothing
/// happens while a dialog is open, another screen shows or a page loads.
pub(super) unsafe fn refresh_tick(window: HWND) {
    let Some(service) = super::spotify::service(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let watching = state.view == MainView::SpotifyBrowse
        && !state.modal_open
        && state.spotify_browse.frames.last().is_some_and(|frame| {
            matches!(frame.source, Source::Playlist(_))
                && frame.loaded
                && frame.loading.is_none()
                && frame.refresh.is_none()
        });
    if !watching {
        if state.view != MainView::SpotifyBrowse {
            let _ = KillTimer(Some(window), REFRESH_TIMER_ID);
        }
        return;
    }
    let stamp = state.spotify.epochs_begin();
    let Some(frame) = state.spotify_browse.frames.last_mut() else {
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

/// The answer to a background check: a playlist changed elsewhere (phone,
/// another device) shows its new rows; the same occurrence stays selected,
/// or the same position, and the focus does not move. `false` when the
/// answer was not a check.
unsafe fn refreshed(
    window: HWND,
    stamp: SpotifyStamp,
    result: &Result<CatalogResult, SpotifyError>,
) -> bool {
    let Some(state) = state_mut(window) else {
        return false;
    };
    let Some(index) = state
        .spotify_browse
        .frames
        .iter()
        .position(|frame| frame.refresh == Some(stamp))
    else {
        return false;
    };
    let is_top = index + 1 == state.spotify_browse.frames.len();
    let selected = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok();
    let frame = &mut state.spotify_browse.frames[index];
    frame.refresh = None;
    let Ok(CatalogResult::Collection(collection)) = result else {
        return true;
    };
    // Spotify's `revisionId` can lag behind a change, so the rows decide:
    // the occurrences of the first page, and where the list ends.
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
    if is_top && state.view == MainView::SpotifyBrowse {
        render(window, false);
    }
    true
}

/// The selection moved: it is remembered, and the last row loads the next
/// page with Apricot's "Loading more results." announcement.
pub(super) unsafe fn selection_changed(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let selected = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok();
    let Some(frame) = state.spotify_browse.frames.last_mut() else {
        return;
    };
    let Some(selected) = selected else {
        return;
    };
    frame.selected = selected;
    let at_end = !frame.items.is_empty() && selected + 1 == frame.items.len();
    if let Some(offset) = frame
        .next_offset
        .filter(|_| at_end && frame.loading.is_none())
    {
        let message = super::catalog_text(&state.application, "loading_more_results");
        set_status(state, &message, false);
        request(window, offset);
    }
}

/// The row to come back to from the player or a nested list.
unsafe fn remember_selection(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let selected = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok();
    if let (Some(frame), Some(selected)) = (state.spotify_browse.frames.last_mut(), selected) {
        frame.selected = selected;
    }
}

/// Enter: a track or episode plays in its list's context, a collection opens.
pub(super) unsafe fn activate(window: HWND) {
    remember_selection(window);
    let Some(item) = state(window).and_then(|state| selected_item(state).cloned()) else {
        return;
    };
    if item.kind.is_playable_item() {
        play(window, &item, false);
    } else {
        open_item(window, &item);
    }
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

/// Opens an album, playlist, artist, show, folder or Liked Songs row.
pub(super) unsafe fn open_item(window: HWND, item: &CatalogItem) {
    let source = match item.kind {
        ItemKind::Page => Source::Browse(Some(item.uri.clone())),
        ItemKind::Section
            if state(window).and_then(top).is_some_and(|frame| {
                matches!(&frame.source, Source::Profile { uri, .. }
                    if item.uri == apricot_spotify::catalog::profile_section_uri(uri, "playlists"))
            }) =>
        {
            let Some(Source::Profile { uri, .. }) =
                state(window).and_then(top).map(|frame| &frame.source)
            else {
                return;
            };
            Source::ProfilePlaylists(uri.clone())
        }
        ItemKind::User => match state(window) {
            Some(state) => profile_source(state, &item.uri),
            None => return,
        },
        ItemKind::Section => {
            let items = state(window)
                .and_then(top)
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
            if let Some(state) = state(window) {
                let text = super::spotify::catalog(state)
                    .text("spotify_unplayable")
                    .to_owned();
                set_status(state, &text, true);
            }
            return;
        }
        _ => {
            if let Some(state) = state(window) {
                let texts = super::spotify::catalog(state);
                let text = texts
                    .text("rust_feature_unavailable")
                    .replace("{feature}", &kind_word(&texts, item.kind));
                set_status(state, &text, true);
            }
            return;
        }
    };
    let title = if item.name.is_empty() {
        item.uri.clone()
    } else {
        item.name.clone()
    };
    open(window, source, title);
}

/// Plays `item`: a track or episode in the context of the open list, at its
/// exact occurrence; a collection as its whole context. `shuffle` starts
/// the context shuffled.
pub(super) unsafe fn play(window: HWND, item: &CatalogItem, shuffle: bool) {
    let Some(state) = state(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    if !item.playable && item.kind.is_playable_item() || item.kind == ItemKind::Unavailable {
        let text = texts.text("spotify_unplayable").to_owned();
        set_status(state, &text, true);
        return;
    }
    if let Some(media) = media_for(state, item, shuffle) {
        super::spotify::play_track(window, media);
    }
}

/// The selected row as an Apricot item, for Favorites, Apricot playlists
/// and the other shared actions: a track or episode in the context of its
/// list, or an album, playlist, artist or show that plays as a whole.
pub(super) fn selected_media(state: &WindowState) -> Option<apricot_core::MediaItem> {
    let item = selected_item(state)?;
    if item.kind == ItemKind::Unavailable || apricot_core::SpotifyRef::parse(&item.uri).is_none() {
        return None;
    }
    media_for(state, item, false)
}

fn media_for(
    state: &WindowState,
    item: &CatalogItem,
    shuffle: bool,
) -> Option<apricot_core::MediaItem> {
    let liked_songs = || {
        state
            .spotify
            .service_ref()
            .and_then(|service| service.liked_songs_context())
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
        let context = match top(state).map(|frame| &frame.source) {
            Some(Source::LikedSongs) => liked_songs(),
            Some(source) => source.context().map(str::to_owned),
            None => None,
        };
        if let Some(context) = context {
            insert("spotify_context", serde_json::Value::String(context));
            // Playlist uids are Connect's occurrence ids (P0); album and
            // show uids from pathfinder are not, so those play by URI. A
            // personal mix is made anew for every request, so Connect's
            // copy has other uids: it plays by URI as well.
            let by_uid = top(state).is_some_and(|frame| {
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
        let context = context?;
        insert("spotify_context", serde_json::Value::String(context));
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
pub(super) unsafe fn back(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    state.spotify_browse.frames.pop();
    if state.spotify_browse.frames.is_empty() {
        super::spotify::show_hub(window);
    } else {
        render(window, true);
    }
}

/// Back from the player: the list it was started from, at the same row.
pub(super) unsafe fn restore(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.spotify_browse.frames.is_empty() {
        super::spotify::show_hub(window);
        return;
    }
    state.view = MainView::SpotifyBrowse;
    render(window, true);
}

/// The title of a search list.
pub(super) fn search_title(state: &WindowState, query: &str) -> String {
    super::spotify::catalog(state)
        .text("spotify_search_results")
        .replace("{query}", query)
}

/// The Spotify link of the selected row, for Copy link.
pub(super) fn selected_link(state: &WindowState) -> Option<String> {
    selected_item(state)
        .and_then(|item| apricot_core::SpotifyRef::parse(&item.uri))
        .and_then(|reference| reference.to_url())
}

/// What the context menu offers for the selected row.
pub(super) fn row_menu(state: &WindowState) -> Option<apricot_app::context_menu::SpotifyRowMenu> {
    let item = selected_item(state)?;
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
    let frame = top(state)?;
    let profile_section =
        item.kind == ItemKind::Section && matches!(frame.source, Source::Profile { .. });
    let editable = frame.can_edit && matches!(frame.source, Source::Playlist(_));
    // SAFETY: Reads the selection of this thread's list.
    let index = usize::try_from(unsafe { SendMessageW(state.list, LB_GETCURSEL, None, None).0 })
        .unwrap_or(0);
    let has_uid = |at: Option<usize>| {
        at.and_then(|at| frame.items.get(at))
            .is_some_and(|item| item.uid.is_some())
    };
    Some(apricot_app::context_menu::SpotifyRowMenu {
        playable_item: item.kind.is_playable_item() && item.playable,
        collection,
        collection_plays: collection && !matches!(item.kind, ItemKind::Folder | ItemKind::User),
        album: !item.album_uri.is_empty(),
        artist: item.artist_uri.starts_with("spotify:artist:"),
        link: apricot_core::SpotifyRef::parse(&item.uri).is_some(),
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
        hidden: state
            .spotify_browse
            .hidden
            .as_ref()
            .is_some_and(|hidden| hidden.contains(&item.uri)),
        radio: matches!(
            item.kind,
            ItemKind::Track | ItemKind::Artist | ItemKind::Album | ItemKind::Playlist
        ),
    })
}

/// `spotify_radio` (Ctrl+Alt+Shift+R): Spotify's radio for the selected
/// track, artist, album or playlist, or for the playing Spotify track. The
/// station opens as a list; Enter plays it.
pub(super) unsafe fn radio(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let target = selected_item(state)
        .filter(|item| {
            matches!(
                item.kind,
                ItemKind::Track | ItemKind::Artist | ItemKind::Album | ItemKind::Playlist
            )
        })
        .map(|item| (item.uri.clone(), item.name.clone()))
        .or_else(|| {
            state
                .application
                .player_session()
                .current_item()
                .filter(|item| item.source == apricot_core::MediaSource::Spotify)
                .filter(|item| item.id.0.starts_with("spotify:track:"))
                .map(|item| (item.id.0.clone(), item.title.clone()))
        });
    let Some((seed, name)) = target else {
        return;
    };
    let title = super::spotify::catalog(state)
        .text("spotify_radio_title")
        .replace("{name}", &name);
    open(window, Source::Radio(seed), title);
}

/// `spotify_dislike` (Ctrl+Shift+H): Spotify's "Hide song" in a personal
/// mix, or showing the song again. Elsewhere it says where it works.
pub(super) unsafe fn toggle_hidden(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let frame = top(state).filter(|_| state.view == MainView::SpotifyBrowse);
    let target = frame.filter(|frame| frame.personalised).and_then(|frame| {
        let context = match &frame.source {
            Source::Playlist(uri) => uri.clone(),
            _ => return None,
        };
        selected_item(state)
            .filter(|item| item.kind == ItemKind::Track)
            .map(|item| (item.uri.clone(), context))
    });
    let Some((uri, context)) = target else {
        let text = super::spotify::catalog(state)
            .text("spotify_hide_unavailable")
            .to_owned();
        set_status(state, &text, true);
        return;
    };
    let hidden = state
        .spotify_browse
        .hidden
        .as_ref()
        .is_some_and(|hidden| hidden.contains(&uri));
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::Hide {
            uri,
            context,
            hidden: !hidden,
        },
        Pending::Saved,
    );
}

unsafe fn begin_edit(window: HWND, edit: apricot_spotify::LibraryEdit, pending: Pending) {
    let Some(service) = super::spotify::service(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let stamp = state.spotify.epochs_begin();
    state.spotify_browse.pending = Some((stamp, pending));
    service.edit(stamp, edit);
}

/// `spotify_toggle_saved` (Ctrl+Shift+I): like or unlike the selected track
/// or the one that plays; save or remove a collection; follow an artist.
pub(super) unsafe fn toggle_saved(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let profile = top(state).and_then(|frame| match &frame.source {
        Source::Profile { uri, .. } => Some(uri.clone()),
        _ => None,
    });
    let uri = selected_item(state)
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
            let session = state.application.player_session();
            session
                .current_item()
                .filter(|item| item.source == apricot_core::MediaSource::Spotify)
                .map(|item| item.id.0.clone())
                .filter(|uri| !uri.contains(":album:") && !uri.contains(":playlist:"))
        });
    let Some(uri) = uri else {
        return;
    };
    let uri = if uri == "spotify:collection:tracks" {
        return;
    } else {
        uri
    };
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::ToggleSaved { uri },
        Pending::Saved,
    );
}

/// Delete in an editable playlist: removes exactly the selected occurrence.
pub(super) unsafe fn remove_selected(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let Some(frame) = top(state) else {
        return;
    };
    let Source::Playlist(playlist) = &frame.source else {
        return;
    };
    if !frame.can_edit {
        return;
    }
    let Some(uid) = selected_item(state).and_then(|item| item.uid.clone()) else {
        return;
    };
    let playlist = playlist.clone();
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::RemoveFromPlaylist {
            playlist,
            uids: vec![uid.clone()],
        },
        Pending::Remove { uid },
    );
}

unsafe fn move_selected(window: HWND, up: bool) {
    let Some(state) = state(window) else {
        return;
    };
    let Some(frame) = top(state) else {
        return;
    };
    let Source::Playlist(playlist) = &frame.source else {
        return;
    };
    let index = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).unwrap_or(0);
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
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::MoveInPlaylist {
            playlist,
            uid: uid.clone(),
            before: up,
            other_uid: other_uid.clone(),
        },
        Pending::Move { uid, other_uid },
    );
}

/// Asks for a name; `None` after Cancel or an empty name.
unsafe fn ask_name(window: HWND, title_key: &str, initial: &str) -> Option<String> {
    ask_text(window, title_key, "playlist_name", initial, false)
}

/// Asks for a text labelled `label_key`; `None` after Cancel, and after an
/// empty answer unless `allow_empty`.
unsafe fn ask_text(
    window: HWND,
    title_key: &str,
    label_key: &str,
    initial: &str,
    allow_empty: bool,
) -> Option<String> {
    let state = state_mut(window)?;
    let texts = super::spotify::catalog(state);
    state.modal_open = true;
    let result = crate::playlist_dialog_win32::prompt_name_with_initial(
        window,
        texts.text(title_key),
        texts.text(label_key),
        initial,
        texts.text("ok"),
        texts.text("cancel"),
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    super::resume_deferred_window_work(window);
    result
        .ok()
        .flatten()
        .map(|text| text.trim().to_owned())
        .filter(|text| allow_empty || !text.is_empty())
}

/// `create_playlist` in Spotify lists: a new playlist, first in the library.
pub(super) unsafe fn create_playlist(window: HWND) {
    create_playlist_then_add(window, None);
}

unsafe fn create_playlist_then_add(window: HWND, then_add: Option<Vec<String>>) {
    let Some(name) = ask_name(window, "spotify_create_playlist", "") else {
        restore_focus(window);
        return;
    };
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::CreatePlaylist { name: name.clone() },
        Pending::Create { then_add },
    );
    restore_focus(window);
}

unsafe fn rename_selected(window: HWND) {
    let Some(item) = state(window).and_then(|state| selected_item(state).cloned()) else {
        return;
    };
    let Some(name) = ask_name(window, "spotify_rename_playlist", &item.name) else {
        restore_focus(window);
        return;
    };
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::RenamePlaylist {
            uri: item.uri.clone(),
            name: name.clone(),
        },
        Pending::Rename,
    );
    restore_focus(window);
}

unsafe fn restore_focus(window: HWND) {
    if let Some(state) = state(window) {
        let _ = SetFocus(Some(super::active_primary_control(state)));
    }
}

/// "Add to Spotify playlist": the playlists the account may edit, with
/// New playlist first. They are read once and then kept for the session.
unsafe fn add_to_playlist(window: HWND, uris: Vec<String>) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.spotify_browse.playlists.is_some() {
        choose_playlist(window, uris);
        return;
    }
    let Some(service) = super::spotify::service(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let text = texts.text("spotify_loading").to_owned();
    set_status(state, &text, false);
    let stamp = state.spotify.epochs_begin();
    state.spotify_browse.pending = Some((stamp, Pending::Playlists { uris }));
    service.load_catalog(stamp, CatalogRequest::EditablePlaylists);
}

unsafe fn choose_playlist(window: HWND, uris: Vec<String>) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let playlists = state.spotify_browse.playlists.clone().unwrap_or_default();
    let mut choices = vec![texts.text("spotify_new_playlist").to_owned()];
    choices.extend(playlists.iter().map(|playlist| playlist.name.clone()));
    state.modal_open = true;
    let chosen = crate::playlist_dialog_win32::choose(
        window,
        texts.text("spotify_add_to_playlist"),
        texts.text("spotify_add_to_playlist"),
        &choices,
        texts.text("ok"),
        texts.text("cancel"),
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    super::resume_deferred_window_work(window);
    match chosen.ok().flatten() {
        Some(0) => create_playlist_then_add(window, Some(uris)),
        Some(index) => {
            if let Some(playlist) = playlists.get(index - 1) {
                begin_edit(
                    window,
                    apricot_spotify::LibraryEdit::AddToPlaylist {
                        playlist: playlist.uri.clone(),
                        uris,
                    },
                    Pending::Add {
                        name: playlist.name.clone(),
                    },
                );
            }
            restore_focus(window);
        }
        None => restore_focus(window),
    }
}

/// Spotify's answer to a change: one announcement, and the open lists show
/// the confirmed state without moving the focus.
#[allow(clippy::too_many_lines)]
pub(super) unsafe fn edited(
    window: HWND,
    stamp: SpotifyStamp,
    result: Result<apricot_spotify::EditOutcome, SpotifyError>,
) {
    use apricot_spotify::EditOutcome as O;
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let Some((wanted, pending)) = state.spotify_browse.pending.take() else {
        return;
    };
    if wanted != stamp {
        state.spotify_browse.pending = Some((wanted, pending));
        return;
    }
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            let text = apricot_app::spotify::error_text(&texts, &error);
            set_status(state, &text, true);
            return;
        }
    };
    let mut follow_up = None;
    let text = match (&outcome, pending) {
        (O::Saved { uri, saved }, _) => {
            let kind = ItemKind::from_uri_public(uri);
            for frame in &mut state.spotify_browse.frames {
                for item in frame.items.iter_mut().filter(|item| item.uri == *uri) {
                    item.saved = Some(*saved);
                }
                if matches!(&frame.source, Source::Profile { uri: profile, .. } if profile == uri) {
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
        (O::AddedToPlaylist { .. }, Pending::Add { name }) => texts
            .text("spotify_added_to_playlist")
            .replace("{name}", &name),
        (O::RemovedFromPlaylist { .. }, Pending::Remove { uid }) => {
            if let Some(frame) = state.spotify_browse.frames.last_mut() {
                frame
                    .items
                    .retain(|item| item.uid.as_deref() != Some(uid.as_str()));
            }
            texts.text("spotify_removed_from_playlist").to_owned()
        }
        (O::Moved { .. }, Pending::Move { uid, other_uid }) => {
            if let Some(frame) = state.spotify_browse.frames.last_mut() {
                let position = |wanted: &str| {
                    frame
                        .items
                        .iter()
                        .position(|item| item.uid.as_deref() == Some(wanted))
                };
                if let (Some(from), Some(to)) = (position(&uid), position(&other_uid)) {
                    frame.items.swap(from, to);
                    frame.selected = to;
                    // SAFETY: Moves the selection of this thread's list.
                    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(to)), None);
                }
            }
            texts.text("spotify_playlist_updated").to_owned()
        }
        (O::Created { uri, name }, Pending::Create { then_add, .. }) => {
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
            if let Some(playlists) = &mut state.spotify_browse.playlists {
                playlists.insert(0, created.clone());
            }
            for frame in &mut state.spotify_browse.frames {
                if matches!(frame.source, Source::Library { folder: None, .. }) && frame.loaded {
                    frame.items.insert(0, created.clone());
                }
            }
            if let Some(uris) = then_add {
                follow_up = Some((
                    apricot_spotify::LibraryEdit::AddToPlaylist {
                        playlist: uri.clone(),
                        uris,
                    },
                    Pending::Add { name: name.clone() },
                ));
            }
            texts
                .text("spotify_playlist_created")
                .replace("{name}", name)
        }
        (O::Hidden { uri, hidden }, _) => {
            let set = state.spotify_browse.hidden.get_or_insert_default();
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
            for frame in &mut state.spotify_browse.frames {
                for item in frame.items.iter_mut().filter(|item| item.uri == *uri) {
                    item.name.clone_from(name);
                }
            }
            if let Some(playlists) = &mut state.spotify_browse.playlists {
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
            let name = state
                .spotify_browse
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
    set_status(state, &text, true);
    if state.view == MainView::SpotifyBrowse {
        render(window, false);
    }
    if let Some((edit, pending)) = follow_up {
        begin_edit(window, edit, pending);
    }
}

/// A context menu command on the selected row.
pub(super) unsafe fn command(window: HWND, command: apricot_app::context_menu::ContextCommand) {
    use apricot_app::context_menu::ContextCommand as C;
    remember_selection(window);
    if command == C::SpotifyCreatePlaylist {
        create_playlist(window);
        return;
    }
    let Some(item) = state(window).and_then(|state| selected_item(state).cloned()) else {
        return;
    };
    match command {
        C::SpotifyPlay => play(window, &item, false),
        C::SpotifyShufflePlay => play(window, &item, true),
        C::SpotifyOpen => open_item(window, &item),
        C::SpotifyAddToQueue => {
            super::spotify::add_to_queue(window);
        }
        C::SpotifyGoToAlbum => open(
            window,
            Source::Album(item.album_uri.clone()),
            item.album.clone(),
        ),
        C::SpotifyGoToArtist => {
            let name = item
                .subtitle
                .split(", ")
                .next()
                .unwrap_or_default()
                .to_owned();
            open(window, Source::Artist(item.artist_uri.clone()), name);
        }
        C::SpotifyCopyLink => {
            if let Some(link) = state(window).and_then(selected_link) {
                super::copy_text_and_announce(window, &link, "url_copied");
            }
        }
        C::SpotifyToggleSaved => toggle_saved(window),
        C::SpotifyAddToPlaylist => add_to_playlist(window, vec![item.uri.clone()]),
        C::SpotifyRemoveFromPlaylist => remove_selected(window),
        C::SpotifyMoveUp => move_selected(window, true),
        C::SpotifyMoveDown => move_selected(window, false),
        C::SpotifyRenamePlaylist => rename_selected(window),
        C::SpotifyHide => toggle_hidden(window),
        C::SpotifyRadio => radio(window),
        C::SpotifyGoToOwner => {
            if let Some(source) = state(window).map(|state| profile_source(state, &item.artist_uri))
            {
                open(window, source, item.subtitle.clone());
            }
        }
        C::SpotifyEditDescription => {
            load_for(
                window,
                CatalogRequest::Playlist {
                    uri: item.uri.clone(),
                    offset: 0,
                },
                Pending::Describe {
                    uri: item.uri.clone(),
                },
            );
        }
        C::SpotifyToggleVisibility => begin_edit(
            window,
            apricot_spotify::LibraryEdit::ToggleVisibility {
                uri: item.uri.clone(),
            },
            Pending::Plain,
        ),
        C::SpotifyPlayPreview => {
            load_for(
                window,
                CatalogRequest::Preview(item.uri.clone()),
                Pending::Preview {
                    name: item.name.clone(),
                },
            );
        }
        _ => {}
    }
}

/// A profile's source with its localized section titles.
pub(super) fn profile_source(state: &WindowState, uri: &str) -> Source {
    let texts = super::spotify::catalog(state);
    Source::Profile {
        uri: uri.to_owned(),
        titles: [
            "spotify_profile_playlists",
            "spotify_profile_following",
            "spotify_profile_followers",
        ]
        .map(|key| texts.text(key).to_owned()),
    }
}

/// Loads what a command needs first ("Loading."), then goes on in
/// `loaded` with `pending`.
unsafe fn load_for(window: HWND, request: CatalogRequest, pending: Pending) {
    let Some(service) = super::spotify::service(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let text = texts.text("spotify_loading").to_owned();
    set_status(state, &text, false);
    let stamp = state.spotify.epochs_begin();
    state.spotify_browse.pending = Some((stamp, pending));
    service.load_catalog(stamp, request);
}

/// "Edit description": the current description to change; an empty one
/// removes it.
unsafe fn describe(window: HWND, uri: String, current: &str) {
    let Some(description) = ask_text(
        window,
        "spotify_edit_description",
        "spotify_description",
        current,
        true,
    ) else {
        restore_focus(window);
        return;
    };
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::DescribePlaylist { uri, description },
        Pending::Plain,
    );
    restore_focus(window);
}

/// "Play preview": Spotify's short preview plays as its own item, named
/// as a preview, so it never passes for the whole episode.
unsafe fn play_preview(window: HWND, name: &str, url: Option<&str>) {
    let Some(state) = state(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let Some(url) = url.and_then(|url| url.parse::<url::Url>().ok()) else {
        let text = texts.text("spotify_no_preview").to_owned();
        set_status(state, &text, true);
        return;
    };
    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("spotify_preview".to_owned(), serde_json::Value::Bool(true));
    let item = apricot_core::MediaItem {
        id: apricot_core::MediaId(url.to_string()),
        source: apricot_core::MediaSource::Podcast,
        kind: apricot_core::MediaKind::Audio,
        title: texts.text("spotify_preview_title").replace("{name}", name),
        url: Some(url.clone()),
        stream_url: Some(url),
        external_audio_url: None,
        local_path: None,
        channel: String::new(),
        duration_seconds: None,
        metadata,
    };
    super::start_media_item(window, item, None);
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
        assert_eq!(
            Source::Search {
                query: "q".into(),
                kind: SearchKind::Tracks
            }
            .context(),
            None
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
        assert_eq!(
            Source::LikedSongs.request(100),
            Some(CatalogRequest::LikedSongs { offset: 100 })
        );
        assert_eq!(Source::Section(Vec::new()).request(0), None);
        assert_eq!(
            Source::Browse(None).request(0),
            Some(CatalogRequest::BrowseAll)
        );
    }
}
