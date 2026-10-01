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
        WindowsAndMessaging::{LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL, SendMessageW},
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
}

impl Source {
    fn request(&self, offset: u64) -> CatalogRequest {
        match self {
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
        }
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
    /// Text of a failed first page.
    error: Option<String>,
    /// Search over all types: rows say their type.
    mixed: bool,
}

#[derive(Default)]
pub(super) struct BrowseState {
    frames: Vec<Frame>,
    /// The last search, offered again by the search dialog.
    pub(super) last_query: String,
    pub(super) last_kind: usize,
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
        error: None,
        mixed,
    });
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
    frame.loading = Some(stamp);
    service.load_catalog(stamp, frame.source.request(offset));
}

/// The answer to a list request.
pub(super) unsafe fn loaded(
    window: HWND,
    stamp: SpotifyStamp,
    result: Result<CatalogResult, SpotifyError>,
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
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
            frame.items.extend(collection.page.items);
            frame.next_offset = collection.page.next_offset;
        }
        Err(error) => {
            let text = apricot_app::spotify::error_text(&texts, &error);
            if first_page {
                frame.error = Some(text.clone());
            }
            set_status(state, &text, true);
        }
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
            .map(|item| item_label(&texts, item, frame.mixed))
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
    layout_controls_state(window, state);
    if focus {
        let _ = SetFocus(Some(state.list));
    }
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
        set_status(state, &message, true);
        request(window, offset);
    }
}

/// Enter: a track or episode plays in its list's context, a collection opens.
pub(super) unsafe fn activate(window: HWND) {
    let Some(item) = state(window).and_then(|state| selected_item(state).cloned()) else {
        return;
    };
    if item.kind.is_playable_item() {
        play(window, &item, false);
    } else {
        open_item(window, &item);
    }
}

/// Opens an album, playlist, artist, show, folder or Liked Songs row.
pub(super) unsafe fn open_item(window: HWND, item: &CatalogItem) {
    let source = match item.kind {
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
            Some(Source::LikedSongs) => {
                super::spotify::service(window).and_then(|service| service.liked_songs_context())
            }
            Some(source) => source.context().map(str::to_owned),
            None => None,
        };
        if let Some(context) = context {
            insert("spotify_context", serde_json::Value::String(context));
            // Playlist uids are Connect's occurrence ids (P0); album and
            // show uids from pathfinder are not, so those play by URI.
            if let (Some(uid), Some(Source::Playlist(_))) =
                (&item.uid, top(state).map(|frame| &frame.source))
            {
                insert("spotify_uid", serde_json::Value::String(uid.clone()));
            }
        }
    } else {
        let context = if item.kind == ItemKind::LikedSongs {
            super::spotify::service(window).and_then(|service| service.liked_songs_context())
        } else {
            Some(item.uri.clone())
        };
        let Some(context) = context else {
            return;
        };
        insert("spotify_context", serde_json::Value::String(context));
        // The first track is not known yet: the item becomes the track
        // Spotify starts, with its own "Playing" announcement.
        insert("spotify_collection", serde_json::Value::Bool(true));
    }
    if shuffle {
        insert("spotify_shuffle", serde_json::Value::Bool(true));
    }
    super::spotify::play_track(window, media);
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
    );
    Some(apricot_app::context_menu::SpotifyRowMenu {
        playable_item: item.kind.is_playable_item() && item.playable,
        collection,
        collection_plays: collection && item.kind != ItemKind::Folder,
        album: !item.album_uri.is_empty(),
        artist: !item.artist_uri.is_empty(),
        link: apricot_core::SpotifyRef::parse(&item.uri).is_some(),
    })
}

/// A context menu command on the selected row.
pub(super) unsafe fn command(window: HWND, command: apricot_app::context_menu::ContextCommand) {
    use apricot_app::context_menu::ContextCommand as C;
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
        _ => {}
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
            CatalogRequest::Playlist {
                uri: "spotify:playlist:p".into(),
                offset: 50
            }
        );
        assert_eq!(
            Source::LikedSongs.request(100),
            CatalogRequest::LikedSongs { offset: 100 }
        );
    }
}
