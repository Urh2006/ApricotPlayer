//! Spotify lists in the main window (`docs/SPOTIFY_PLAN.md` 4.2, 4.3):
//! search results, the library, Liked Songs, playlists, albums, artists and
//! shows. Every opened list is a frame on a stack, so Escape goes back one
//! level to the same row. Lists load in pages; reaching the last row loads
//! the next page, as Apricot's own result lists do. Answers that arrive late
//! (another frame, another account) are dropped. Focus moves only when the
//! user opens or leaves a list.

use apricot_app::spotify::{item_label, kind_word};
use apricot_core::{Route, SpotifyStamp};
use apricot_spotify::main_menu::{MenuOrigin, MenuPin, MenuTarget};
use apricot_spotify::{
    CatalogItem, CatalogRequest, CatalogResult, ItemKind, LibraryFilter, SearchKind, SpotifyError,
    SpotifyTrack,
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
        UI::{
            Input::KeyboardAndMouse::{GetFocus, SetFocus},
            WindowsAndMessaging::{
                CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL, CBS_DROPDOWNLIST,
                ES_AUTOHSCROLL, GetWindowTextLengthW, GetWindowTextW, KillTimer, LB_GETCURSEL,
                LB_RESETCONTENT, LB_SETCURSEL, MoveWindow, SW_HIDE, SW_SHOW, SendMessageW,
                SetTimer, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WS_CHILD,
                WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{
    MainView, WindowState, add_list_string, create_control, layout_button_row,
    layout_controls_state, set_status, show_error_message, state, state_mut, wide,
};

const ID_SPOTIFY_QUERY: usize = 1062;
const ID_SPOTIFY_TYPE: usize = 1063;

/// The fields of the Spotify search screen (Urh's beta 6 request: a screen
/// like the `YouTube` search instead of a dialog).
pub(super) struct SearchControls {
    query_label: HWND,
    query: HWND,
    type_label: HWND,
    kind: HWND,
}

impl SearchControls {
    pub(super) const fn windows(&self) -> [HWND; 4] {
        [self.query_label, self.query, self.type_label, self.kind]
    }
}

pub(super) unsafe fn create_search_controls(
    parent: HWND,
    instance: HINSTANCE,
    catalog: &apricot_core::TranslationCatalog,
) -> windows::core::Result<SearchControls> {
    let query_text = wide(catalog.text("spotify_search_query"));
    let query_label = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(query_text.as_ptr()),
        WS_CHILD,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let query = create_control(
        parent,
        instance,
        w!("EDIT"),
        PCWSTR::null(),
        WS_CHILD | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        WS_EX_CLIENTEDGE,
        ID_SPOTIFY_QUERY,
    )?;
    crate::accessibility_win32::annotate_control_name(query, catalog.text("spotify_search_query"));
    let type_text = wide(catalog.text("spotify_search_type"));
    let type_label = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(type_text.as_ptr()),
        WS_CHILD,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let kind = create_control(
        parent,
        instance,
        w!("COMBOBOX"),
        PCWSTR::null(),
        WS_CHILD | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL,
        WINDOW_EX_STYLE::default(),
        ID_SPOTIFY_TYPE,
    )?;
    crate::accessibility_win32::annotate_control_name(kind, catalog.text("spotify_search_type"));
    Ok(SearchControls {
        query_label,
        query,
        type_label,
        kind,
    })
}

/// The open list is the search screen.
pub(super) fn search_screen(state: &WindowState) -> bool {
    state.view == MainView::SpotifyBrowse && top(state).is_some_and(|frame| frame.search_screen)
}

/// The search screen with the last query, focus in its field.
pub(super) unsafe fn show_search_screen(window: HWND) {
    super::spotify::prepare_screen(window, MainView::SpotifyBrowse, Route::SpotifyBrowse);
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let kind_index = state.spotify_browse.last_kind;
    let kind = SearchKind::ALL
        .get(kind_index)
        .copied()
        .unwrap_or(SearchKind::All);
    state.spotify_browse.frames.clear();
    state.spotify_browse.frames.push(Frame {
        source: Source::Search {
            query: String::new(),
            kind,
        },
        title: texts.text("result_list").to_owned(),
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
        focus_results: false,
    });
    state.view = MainView::SpotifyBrowse;
    // The labels follow the language; the field offers the last query.
    let controls = &state.spotify_search_controls;
    let label = |control: HWND, key: &str| {
        let text = wide(texts.text(key));
        let _ = SetWindowTextW(control, PCWSTR(text.as_ptr()));
    };
    label(controls.query_label, "spotify_search_query");
    label(controls.type_label, "spotify_search_type");
    let query = wide(&state.spotify_browse.last_query);
    let _ = SetWindowTextW(controls.query, PCWSTR(query.as_ptr()));
    SendMessageW(controls.kind, CB_RESETCONTENT, None, None);
    for kind in apricot_app::spotify::search_kind_labels(&texts) {
        let kind = wide(&kind);
        SendMessageW(
            controls.kind,
            CB_ADDSTRING,
            None,
            Some(LPARAM(kind.as_ptr() as isize)),
        );
    }
    SendMessageW(controls.kind, CB_SETCURSEL, Some(WPARAM(kind_index)), None);
    let query_control = controls.query;
    render(window, false);
    let _ = SetFocus(Some(query_control));
    // The whole last query is selected, so typing replaces it.
    SendMessageW(query_control, 0x00B1, Some(WPARAM(0)), Some(LPARAM(-1)));
}

/// Enter in the query field or on the type, and Search.
pub(super) fn handles_enter(state: &WindowState, target: HWND) -> bool {
    search_screen(state)
        && (target == state.spotify_search_controls.query
            || target == state.spotify_search_controls.kind)
}

/// Runs the search: the results load into the list below and take the
/// focus when they arrive, as on the `YouTube` search screen.
pub(super) unsafe fn submit_search(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let controls = &state.spotify_search_controls;
    let query = window_text(controls.query).trim().to_owned();
    if query.is_empty() {
        let query_control = controls.query;
        show_error_message(window, texts.text("enter_query"));
        let _ = SetFocus(Some(query_control));
        return;
    }
    let kind_index =
        usize::try_from(SendMessageW(controls.kind, CB_GETCURSEL, None, None).0).unwrap_or(0);
    let kind = SearchKind::ALL
        .get(kind_index)
        .copied()
        .unwrap_or(SearchKind::All);
    state.spotify_browse.cancel_context();
    state.spotify_browse.marks.clear();
    if matches!(
        state.spotify_browse.pending,
        Some((_, Pending::Selection { .. } | Pending::CheckSaved { .. }))
    ) {
        state.spotify_browse.pending = None;
    }
    state.spotify_browse.last_query.clone_from(&query);
    state.spotify_browse.last_kind = kind_index;
    let title = search_title(state, &query);
    let searching = texts.text("spotify_searching").replace("{query}", &query);
    set_status(state, &searching, true);
    // A search from a nested list starts over at the search screen.
    let search = state
        .spotify_browse
        .frames
        .iter()
        .rposition(|frame| frame.search_screen)
        .unwrap_or(0);
    state.spotify_browse.frames.truncate(search + 1);
    let Some(frame) = state.spotify_browse.frames.last_mut() else {
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
    request(window, 0);
    render(window, false);
}

unsafe fn window_text(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    let mut buffer = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let copied = GetWindowTextW(control, &mut buffer);
    String::from_utf16_lossy(&buffer[..usize::try_from(copied).unwrap_or_default()])
}

/// Visibility of the search fields and the shared buttons they use.
pub(super) unsafe fn sync_search_visibility(state: &WindowState) {
    let search = search_screen(state);
    for control in state.spotify_search_controls.windows() {
        let _ = ShowWindow(control, if search { SW_SHOW } else { SW_HIDE });
    }
    if search {
        let _ = ShowWindow(state.search, SW_SHOW);
    }
}

/// The `YouTube` search screen's order: Back, the query, the type, Search
/// and Open, then the results.
pub(super) unsafe fn layout_search(
    state: &WindowState,
    width: i32,
    height: i32,
    margin: i32,
    button_height: i32,
    status_height: i32,
) {
    if !search_screen(state) {
        return;
    }
    let label_height = 22;
    let field_height = 30;
    let field_width = width - margin * 2;
    let mut y = margin;
    let _ = MoveWindow(state.back, margin, y, 180, button_height, true);
    y += button_height + margin;
    let controls = &state.spotify_search_controls;
    for (label, field, box_height) in [
        (controls.query_label, controls.query, field_height),
        (controls.type_label, controls.kind, 200),
    ] {
        let _ = MoveWindow(label, margin, y, field_width, label_height, true);
        let _ = MoveWindow(
            field,
            margin,
            y + label_height,
            field_width,
            box_height,
            true,
        );
        y += label_height + field_height + margin;
    }
    layout_button_row(&[state.search, state.open], width, y, margin, button_height);
    y += button_height + margin;
    let status_y = height - status_height - margin;
    let _ = MoveWindow(
        state.status,
        margin,
        status_y,
        field_width,
        status_height,
        true,
    );
    let _ = MoveWindow(
        state.list,
        margin,
        y,
        field_width,
        (status_y - margin - y).max(40),
        true,
    );
}

/// The Tab order of the search screen.
pub(super) fn search_tab_controls(state: &WindowState) -> Option<Vec<HWND>> {
    search_screen(state).then(|| {
        vec![
            state.back,
            state.spotify_search_controls.query,
            state.spotify_search_controls.kind,
            state.search,
            state.open,
            state.list,
        ]
    })
}

/// The query field until results arrive, then the list.
pub(super) fn primary_control(state: &WindowState) -> HWND {
    if search_screen(state)
        && top(state).is_some_and(|frame| {
            frame.items.is_empty()
                && matches!(&frame.source, Source::Search { query, .. } if query.is_empty())
        })
    {
        state.spotify_search_controls.query
    } else {
        state.list
    }
}

/// Where a list comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Source {
    MyProfile(String),
    ProfilePeople {
        uri: String,
        followers: bool,
    },
    MenuSection {
        origin: MenuOrigin,
        uri: String,
    },
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
    /// The search screen: query, type and Search above the results.
    search_screen: bool,
    /// The first page of a new search takes the focus.
    focus_results: bool,
}

/// A change waiting for Spotify's answer.
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

#[derive(Default)]
pub(super) struct BrowseState {
    pub(super) pins: Vec<MenuPin>,
    pin_items: std::collections::HashMap<String, CatalogItem>,
    artist_states: std::collections::HashMap<String, bool>,
    profile_states: std::collections::HashMap<String, bool>,
    context_read: Option<ContextRead>,
    context_ready: Option<ContextIdentity>,
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
    /// Songs marked with Shift+A or Ctrl+A for "Add to Spotify playlist".
    marks: apricot_app::spotify::SpotifyMarks,
    /// Verified personal mix contexts remain known after their list closes.
    personalised_contexts: std::collections::HashSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ContextIdentity {
    view: MainView,
    uri: String,
    player: bool,
    generation: u64,
    focus: isize,
    depth: usize,
    row: isize,
}

struct ContextRead {
    stamp: SpotifyStamp,
    identity: ContextIdentity,
    location: LPARAM,
    uris: Vec<String>,
    playlist: bool,
    publication: bool,
    profile_uri: Option<String>,
}

impl BrowseState {
    pub(super) fn cancel_context(&mut self) {
        self.context_read = None;
        self.context_ready = None;
    }
    pub(super) fn clear_account(&mut self) {
        *self = Self::default();
    }
}

impl Frame {
    fn remove_items(&mut self, keep: impl FnMut(&CatalogItem) -> bool) {
        let before = self.items.len();
        self.items.retain(keep);
        let removed = before - self.items.len();
        if removed > 0 {
            self.next_offset = self
                .next_offset
                .map(|offset| offset.saturating_sub(removed as u64));
            // Responses read before the edit use the old server offsets.
            self.loading = None;
            self.refresh = None;
            self.selected = self.selected.min(self.items.len().saturating_sub(1));
        }
    }
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
    state.spotify_browse.cancel_context();
    state.spotify_browse.marks.clear();
    if matches!(
        state.spotify_browse.pending,
        Some((_, Pending::Selection { .. } | Pending::CheckSaved { .. }))
    ) {
        state.spotify_browse.pending = None;
    }
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
        search_screen: false,
        focus_results: false,
    });
    let texts = super::spotify::catalog(state);
    if let Some(frame) = state.spotify_browse.frames.last_mut()
        && let Source::Section(items) = &frame.source
    {
        frame.items = items.clone();
        frame.loaded = true;
    }
    if let Some(frame) = state.spotify_browse.frames.last_mut()
        && let Source::MyProfile(uri) = &frame.source
    {
        frame.items = my_profile_rows(uri, &texts);
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
    if context_loaded(window, stamp, &result) {
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    if !state.spotify.is_current_account(stamp) {
        return;
    }
    let texts = super::spotify::catalog(state);
    if let Ok(CatalogResult::SelectionProgress(count)) = &result {
        if state
            .spotify_browse
            .pending
            .as_ref()
            .is_some_and(|(wanted, pending)| {
                *wanted == stamp && matches!(pending, Pending::Selection { .. })
            })
        {
            set_status(
                state,
                &format!("{} ({count})", texts.text("spotify_loading")),
                false,
            );
        }
        return;
    }
    let pending_load = matches!(
        state.spotify_browse.pending,
        Some((
            wanted,
            Pending::Playlists { .. }
                | Pending::Describe { .. }
                | Pending::Preview { .. }
                | Pending::Duplicates { .. }
                | Pending::Selection { .. }
                | Pending::CheckSaved { .. }
        )) if wanted == stamp
    );
    if pending_load && let Some((_, pending)) = state.spotify_browse.pending.take() {
        match (pending, result) {
            (Pending::Selection { save, marks }, Ok(CatalogResult::TrackUris(uris))) => {
                let uris = marks.resolve(uris);
                bulk_action(window, uris, save);
            }
            (Pending::CheckSaved { uris }, Ok(CatalogResult::SavedStates(states))) => {
                save_checked(window, uris, &states);
            }
            (
                Pending::Duplicates {
                    playlist,
                    name,
                    uris,
                    ..
                },
                Ok(CatalogResult::TrackUris(existing)),
            ) => add_without_duplicates(window, playlist, &name, &uris, &existing),
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
                    Some(offset) => load_for(
                        window,
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
                    None => add_without_duplicates(window, playlist, &name, &uris, &existing),
                }
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
                    state
                        .spotify_browse
                        .personalised_contexts
                        .insert(uri.clone());
                } else {
                    state.spotify_browse.personalised_contexts.remove(uri);
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
                request(window, 0);
            }
            return;
        }
        Ok(
            CatalogResult::PlaylistProfileState { .. }
            | CatalogResult::SelectionProgress(_)
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
        render(window, focus_results);
    }
}

/// Ctrl+A selects the list without materializing its remaining rows.
pub(super) unsafe fn mark_all(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.view != MainView::SpotifyBrowse {
        return;
    }
    let texts = super::spotify::catalog(state);
    let Some(frame) = top(state) else {
        return;
    };
    let more = frame.next_offset.is_some() || frame.loading.is_some();
    let uris: Vec<String> = frame
        .items
        .iter()
        .filter(|item| item.kind.is_playable_item() && item.playable)
        .map(|item| item.uri.clone())
        .collect();
    let count = uris.len();
    if more {
        state.spotify_browse.marks.mark_entire_list();
    } else {
        state.spotify_browse.cancel_context();
        state.spotify_browse.marks.clear();
        state.spotify_browse.marks.mark_all(uris);
    }
    let text = if more {
        texts.text("spotify_selected_entire_list").to_owned()
    } else {
        texts
            .text("spotify_marked_all")
            .replace("{count}", &count.to_string())
    };
    super::spotify::announce(window, &text);
    render(window, false);
}

fn selection_saved(state: &WindowState) -> Option<bool> {
    let frame = top(state)?;
    let marks = &state.spotify_browse.marks;
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

unsafe fn begin_selection(window: HWND, selected: &str, save: Option<bool>) {
    let Some(state) = state(window) else {
        return;
    };
    if state.spotify_browse.pending.is_some() {
        return;
    }
    if state.spotify_browse.marks.is_all()
        && let Some(request) = top(state).and_then(|frame| frame.source.request(0))
    {
        load_for(
            window,
            CatalogRequest::CollectTracks(Box::new(request)),
            Pending::Selection {
                save,
                marks: state.spotify_browse.marks.clone(),
            },
        );
        return;
    }
    bulk_action(window, songs_to_add(state, selected), save);
}

unsafe fn bulk_choice(
    window: HWND,
    title: &str,
    prompt: &str,
    choices: &[String],
) -> Option<usize> {
    let state = state_mut(window)?;
    let texts = super::spotify::catalog(state);
    state.modal_open = true;
    let chosen = crate::playlist_dialog_win32::choose(
        window,
        title,
        prompt,
        choices,
        texts.text("ok"),
        texts.text("cancel"),
    );
    if let Some(state) = state_mut(window) {
        state.modal_open = false;
    }
    super::resume_deferred_window_work(window);
    restore_focus(window);
    chosen.ok().flatten()
}

unsafe fn bulk_action(window: HWND, uris: Vec<String>, save: Option<bool>) {
    if uris.is_empty() {
        return;
    }
    match save {
        None => add_to_playlist(window, uris),
        Some(true) => load_for(
            window,
            CatalogRequest::SavedStates(uris.clone()),
            Pending::CheckSaved { uris },
        ),
        Some(false) => {
            let Some(state) = state(window) else {
                return;
            };
            let texts = super::spotify::catalog(state);
            let question = format!("{} ({})?", texts.text("spotify_unlike"), uris.len());
            if bulk_choice(
                window,
                texts.text("spotify_unlike"),
                &question,
                &[texts.text("yes").to_owned()],
            ) == Some(0)
            {
                begin_edit(
                    window,
                    apricot_spotify::LibraryEdit::SetSaved { uris, saved: false },
                    Pending::Saved,
                );
            }
        }
    }
}

unsafe fn save_checked(window: HWND, uris: Vec<String>, states: &[bool]) {
    if states.len() != uris.len() {
        return;
    }
    let Some(state) = state(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let duplicates = states.iter().filter(|saved| **saved).count();
    let uris = if duplicates == 0 {
        uris
    } else {
        let question = texts
            .text("spotify_duplicates_question")
            .replace("{count}", &duplicates.to_string())
            .replace("{total}", &uris.len().to_string())
            .replace("{name}", texts.text("spotify_liked_songs"));
        let choice = bulk_choice(
            window,
            texts.text("spotify_like"),
            &question,
            &[
                texts.text("spotify_add_new_only").to_owned(),
                texts.text("spotify_add_all_anyway").to_owned(),
            ],
        );
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
        super::spotify::announce(
            window,
            &texts
                .text("spotify_nothing_new")
                .replace("{name}", texts.text("spotify_liked_songs")),
        );
        return;
    }
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::SetSaved { uris, saved: true },
        Pending::Saved,
    );
}

/// Shift+A in a Spotify list: marks or unmarks the selected song; Spotify
/// songs never go into the download queue.
pub(super) unsafe fn toggle_mark(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let Some(uri) = selected_item(state)
        .filter(|item| item.kind.is_playable_item() && item.playable)
        .map(|item| item.uri.clone())
    else {
        super::spotify::announce(window, texts.text("no_selection"));
        return;
    };
    let marked = state.spotify_browse.marks.toggle(&uri);
    let key = if marked {
        "spotify_marked"
    } else {
        "spotify_unmarked"
    };
    let text = if state.spotify_browse.marks.is_all() {
        texts
            .text(if marked {
                "spotify_marked_row"
            } else {
                "spotify_unmarked_row"
            })
            .to_owned()
    } else {
        texts
            .text(key)
            .replace("{count}", &state.spotify_browse.marks.len().to_string())
    };
    super::spotify::announce(window, &text);
    render(window, false);
}

/// The songs "Add to Spotify playlist" adds: the marked ones, or the
/// selected row.
fn songs_to_add(state: &WindowState, selected: &str) -> Vec<String> {
    if state.spotify_browse.marks.is_empty() {
        vec![selected.to_owned()]
    } else {
        state.spotify_browse.marks.uris().to_vec()
    }
}

/// The whole playlist is in: songs already there are left out, or added
/// anyway, as the user chooses.
unsafe fn add_without_duplicates(
    window: HWND,
    playlist: String,
    name: &str,
    uris: &[String],
    existing: &[String],
) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let texts = super::spotify::catalog(state);
    let (new, duplicates) = apricot_app::spotify::split_duplicates(uris, existing);
    let uris = if duplicates == 0 {
        uris.to_vec()
    } else {
        let question = texts
            .text("spotify_duplicates_question")
            .replace("{count}", &duplicates.to_string())
            .replace("{total}", &uris.len().to_string())
            .replace("{name}", name);
        let choices = [
            texts.text("spotify_add_new_only").to_owned(),
            texts.text("spotify_add_all_anyway").to_owned(),
        ];
        state.modal_open = true;
        let chosen = crate::playlist_dialog_win32::choose(
            window,
            texts.text("spotify_add_to_playlist"),
            &question,
            &choices,
            texts.text("ok"),
            texts.text("cancel"),
        );
        if let Some(state) = state_mut(window) {
            state.modal_open = false;
        }
        super::resume_deferred_window_work(window);
        restore_focus(window);
        match chosen.ok().flatten() {
            Some(0) if new.is_empty() => {
                let text = texts.text("spotify_nothing_new").replace("{name}", name);
                if let Some(state) = state_mut(window) {
                    state.spotify_browse.cancel_context();
                    state.spotify_browse.marks.clear();
                }
                super::spotify::announce(window, &text);
                render(window, false);
                return;
            }
            Some(0) => new,
            Some(_) => uris.to_vec(),
            None => return,
        }
    };
    let count = uris.len();
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::AddToPlaylist { playlist, uris },
        Pending::Add {
            name: name.to_owned(),
            count,
        },
    );
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
                if state.spotify_browse.marks.contains(&item.uri) {
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

unsafe fn open_my_profile_item(window: HWND, item: &CatalogItem) -> bool {
    if let Some(uri) = state(window)
        .and_then(top)
        .and_then(|frame| match &frame.source {
            Source::MyProfile(uri) => Some(uri.clone()),
            _ => None,
        })
    {
        if item.uri == apricot_spotify::catalog::profile_section_uri(&uri, "friends") {
            let Some(state) = state_mut(window) else {
                return true;
            };
            let parent = std::mem::take(&mut state.spotify_browse.frames);
            state.spotify_browse.last_query.clear();
            state.spotify_browse.last_kind = SearchKind::ALL
                .iter()
                .position(|kind| *kind == SearchKind::Profiles)
                .unwrap_or(0);
            show_search_screen(window);
            if let Some(state) = state_mut(window) {
                let search = std::mem::take(&mut state.spotify_browse.frames);
                state.spotify_browse.frames = parent;
                state.spotify_browse.frames.extend(search);
            }
            return true;
        }
        let source = if item.uri == apricot_spotify::catalog::profile_section_uri(&uri, "playlists")
        {
            Source::ProfilePlaylists(uri)
        } else {
            Source::ProfilePeople {
                uri: uri.clone(),
                followers: item.uri
                    == apricot_spotify::catalog::profile_section_uri(&uri, "followers"),
            }
        };
        open(window, source, item.name.clone());
        return true;
    }
    false
}

/// Opens an album, playlist, artist, show, folder or Liked Songs row.
pub(super) unsafe fn open_item(window: HWND, item: &CatalogItem) {
    if let Some(pin) = state(window).and_then(super::spotify::selected_pin) {
        open_root(window, pin_source(&pin), pin.label);
        return;
    }
    if open_my_profile_item(window, item) {
        return;
    }
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
    if let Some(mut media) = media_for(state, item, shuffle) {
        if shuffle {
            media
                .metadata
                .insert("spotify_shuffle_explicit".to_owned(), true.into());
        }
        super::spotify::play_track(window, media);
    }
}

/// The selected row as an Apricot item, for Favorites, Apricot playlists
/// and the other shared actions: a track or episode in the context of its
/// list, or an album, playlist, artist or show that plays as a whole.
pub(super) fn selected_media(state: &WindowState) -> Option<apricot_core::MediaItem> {
    let item = context_item(state)?;
    if item.kind == ItemKind::Unavailable || apricot_core::SpotifyRef::parse(&item.uri).is_none() {
        return None;
    }
    media_for(state, &item, false)
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
    state.spotify_browse.cancel_context();
    state.spotify_browse.marks.clear();
    if matches!(
        state.spotify_browse.pending,
        Some((_, Pending::Selection { .. } | Pending::CheckSaved { .. }))
    ) {
        state.spotify_browse.pending = None;
    }
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
fn my_profile_rows(uri: &str, texts: &apricot_core::TranslationCatalog) -> Vec<CatalogItem> {
    [
        ("following", "spotify_profile_following"),
        ("followers", "spotify_profile_followers"),
        ("friends", "spotify_find_friends"),
        ("playlists", "spotify_my_public_playlists"),
    ]
    .into_iter()
    .map(|(key, label)| {
        section_row(&apricot_spotify::catalog::Section {
            uri: apricot_spotify::catalog::profile_section_uri(uri, key),
            title: texts.text(label).to_owned(),
            items: Vec::new(),
        })
    })
    .collect()
}

pub(super) fn pin_source(pin: &MenuPin) -> Source {
    match &pin.target {
        MenuTarget::Playlist(uri) => Source::Playlist(uri.clone()),
        MenuTarget::Artist(uri) => Source::Artist(uri.clone()),
        MenuTarget::Category(uri) => Source::Browse(Some(uri.clone())),
        MenuTarget::Section { origin, uri } => Source::MenuSection {
            origin: origin.clone(),
            uri: uri.clone(),
        },
    }
}

fn menu_origin(state: &WindowState) -> Option<MenuOrigin> {
    for frame in state.spotify_browse.frames.iter().rev() {
        match &frame.source {
            Source::Home => return Some(MenuOrigin::Home),
            Source::Browse(None) => return Some(MenuOrigin::BrowseAll),
            Source::Browse(Some(uri)) => return Some(MenuOrigin::BrowsePage(uri.clone())),
            Source::MenuSection { origin, .. } => return Some(origin.clone()),
            Source::Section(_) => {}
            _ => return None,
        }
    }
    None
}

pub(super) fn context_item(state: &WindowState) -> Option<CatalogItem> {
    if state.view == MainView::SpotifyHub {
        let pin = super::spotify::selected_pin(state)?;
        Some(
            state
                .spotify_browse
                .pin_items
                .get(&pin.item().uri)
                .cloned()
                .unwrap_or_else(|| pin.item()),
        )
    } else {
        selected_item(state).cloned()
    }
}

fn current_pin(state: &WindowState) -> Option<MenuPin> {
    if state.view == MainView::SpotifyHub {
        super::spotify::selected_pin(state)
    } else {
        MenuPin::from_item(&context_item(state)?, menu_origin(state))
    }
}

pub(super) unsafe fn toggle_menu_pin(window: HWND) {
    let Some((pin, selected, hub)) = state(window).and_then(|state| {
        Some((
            current_pin(state)?,
            SendMessageW(state.list, LB_GETCURSEL, None, None).0,
            state.view == MainView::SpotifyHub,
        ))
    }) else {
        return;
    };
    let Some(service) = super::spotify::service(window) else {
        return;
    };
    match service
        .toggle_menu_pin(pin)
        .and_then(|_| service.menu_pins())
    {
        Ok(pins) => {
            if let Some(state) = state_mut(window) {
                state.spotify_browse.pins = pins;
            }
            if hub {
                super::spotify::show_hub(window);
                if let Some(state) = state(window) {
                    let count = SendMessageW(
                        state.list,
                        windows::Win32::UI::WindowsAndMessaging::LB_GETCOUNT,
                        None,
                        None,
                    )
                    .0;
                    SendMessageW(
                        state.list,
                        LB_SETCURSEL,
                        Some(WPARAM(
                            usize::try_from(selected.min(count - 1).max(0)).unwrap_or(0),
                        )),
                        None,
                    );
                }
            }
            if let Some(state) = state(window) {
                super::spotify::announce(
                    window,
                    super::spotify::catalog(state).text("settings_saved"),
                );
            }
        }
        Err(error) => {
            if let Some(state) = state(window) {
                set_status(state, &error, true);
            }
        }
    }
}

fn artist_uri(item: &CatalogItem) -> Option<String> {
    if item.kind == ItemKind::Artist {
        Some(item.uri.clone())
    } else {
        item.artist_uri
            .starts_with("spotify:artist:")
            .then(|| item.artist_uri.clone())
    }
}

pub(super) fn playing_artist(item: &apricot_core::MediaItem) -> Option<String> {
    (item.source == apricot_core::MediaSource::Spotify)
        .then(|| {
            item.metadata
                .get("spotify_artist_uri")?
                .as_str()
                .map(str::to_owned)
        })
        .flatten()
        .filter(|uri| uri.starts_with("spotify:artist:"))
}

fn context_identity(state: &WindowState, player: bool) -> Option<ContextIdentity> {
    Some(ContextIdentity {
        view: state.view,
        uri: if player {
            state
                .application
                .player_session()
                .current_item()?
                .id
                .0
                .clone()
        } else {
            context_item(state)?.uri
        },
        player,
        generation: if player {
            state.application.player_session().generation()
        } else {
            0
        },
        focus: unsafe { GetFocus().0 as isize },
        depth: if player {
            0
        } else {
            state.spotify_browse.frames.len()
        },
        row: if player {
            -1
        } else {
            unsafe { SendMessageW(state.list, LB_GETCURSEL, None, None).0 }
        },
    })
}

/// Reads current Spotify follow state before showing a menu, without blocking the UI.
pub(super) unsafe fn prepare_context(window: HWND, location: LPARAM, player: bool) -> bool {
    let Some(state) = state_mut(window) else {
        return false;
    };
    let Some(identity) = context_identity(state, player) else {
        state.spotify_browse.cancel_context();
        return false;
    };
    if state.spotify_browse.context_ready.take().as_ref() == Some(&identity) {
        return false;
    }
    if state
        .spotify_browse
        .context_read
        .as_ref()
        .is_some_and(|read| read.identity == identity)
    {
        return true;
    }
    state.spotify_browse.context_read = None;
    let item = (!player).then(|| context_item(state)).flatten();
    let artist = if player {
        state
            .application
            .player_session()
            .current_item()
            .and_then(playing_artist)
    } else {
        item.as_ref().and_then(artist_uri)
    };
    let profile_uri = item
        .as_ref()
        .filter(|item| item.kind == ItemKind::Playlist)
        .map(|item| item.uri.clone());
    let playlist = state.view == MainView::SpotifyHub && profile_uri.is_some();
    let uris: Vec<_> = artist.into_iter().chain(profile_uri.clone()).collect();
    if uris.is_empty() {
        return false;
    }
    if let Some(uri) = &profile_uri {
        state.spotify_browse.profile_states.remove(uri);
    }
    for uri in &uris {
        state.spotify_browse.artist_states.remove(uri);
        for frame in &mut state.spotify_browse.frames {
            for item in &mut frame.items {
                if item.kind == ItemKind::Artist && item.uri == *uri {
                    item.saved = None;
                }
            }
        }
        if let Some(item) = state.spotify_browse.pin_items.get_mut(uri) {
            item.saved = None;
        }
    }
    let Some(service) = super::spotify::service(window) else {
        return false;
    };
    let Some(state) = state_mut(window) else {
        return false;
    };
    let stamp = state.spotify.epochs_begin();
    let request = if playlist {
        let mut cached = item.unwrap();
        cached.editable = false;
        cached.saved = None;
        state
            .spotify_browse
            .pin_items
            .insert(cached.uri.clone(), cached);
        CatalogRequest::Playlist {
            uri: identity.uri.clone(),
            offset: 0,
        }
    } else {
        CatalogRequest::SavedStates(uris.clone())
    };
    state.spotify_browse.context_read = Some(ContextRead {
        stamp,
        identity,
        location,
        uris,
        playlist,
        publication: false,
        profile_uri,
    });
    set_status(
        state,
        super::spotify::catalog(state).text("spotify_loading"),
        false,
    );
    service.load_catalog(stamp, request);
    true
}

unsafe fn context_loaded(
    window: HWND,
    stamp: SpotifyStamp,
    result: &Result<CatalogResult, SpotifyError>,
) -> bool {
    let Some(state) = state_mut(window) else {
        return false;
    };
    if !state
        .spotify_browse
        .context_read
        .as_ref()
        .is_some_and(|read| read.stamp == stamp)
    {
        return false;
    }
    let mut read = state.spotify_browse.context_read.take().unwrap();
    if !state.spotify.is_current_account(stamp)
        || context_identity(state, read.identity.player).as_ref() != Some(&read.identity)
    {
        return true;
    }
    if read.playlist
        && let Ok(CatalogResult::Collection(collection)) = result
    {
        if let Some(item) = state.spotify_browse.pin_items.get_mut(&read.identity.uri) {
            item.editable = collection.can_edit_metadata;
            item.saved = collection.saved;
        }
        read.playlist = false;
        let uris = read.uris.clone();
        state.spotify_browse.context_read = Some(read);
        if let Some(service) = super::spotify::service(window) {
            service.load_catalog(stamp, CatalogRequest::SavedStates(uris));
        }
        return true;
    }
    match result {
        Ok(CatalogResult::PlaylistProfileState {
            uri,
            published: Some(published),
        }) if read.publication && read.profile_uri.as_ref() == Some(uri) => {
            state
                .spotify_browse
                .profile_states
                .insert(uri.clone(), *published);
        }
        Ok(CatalogResult::SavedStates(states)) if states.len() == read.uris.len() => {
            for (uri, followed) in read.uris.iter().zip(states) {
                state
                    .spotify_browse
                    .artist_states
                    .insert(uri.clone(), *followed);
                for frame in &mut state.spotify_browse.frames {
                    for item in &mut frame.items {
                        if item.uri == *uri {
                            item.saved = Some(*followed);
                        }
                    }
                }
                if let Some(item) = state.spotify_browse.pin_items.get_mut(uri) {
                    item.saved = Some(*followed);
                }
            }
        }
        Err(error) => set_status(
            state,
            &apricot_app::spotify::error_text(&super::spotify::catalog(state), error),
            true,
        ),
        _ => {}
    }
    if !read.publication
        && let Some(uri) = &read.profile_uri
    {
        let request = CatalogRequest::PlaylistProfileState(uri.clone());
        read.publication = true;
        state.spotify_browse.context_read = Some(read);
        if let Some(service) = super::spotify::service(window) {
            service.load_catalog(stamp, request);
        }
        return true;
    }
    let player = read.identity.player;
    state.spotify_browse.context_ready = Some(read.identity);
    if player {
        super::show_player_context_menu(window, read.location);
    } else {
        super::show_list_context_menu(window, read.location);
    }
    true
}

pub(super) fn artist_followed(state: &WindowState, item: &apricot_core::MediaItem) -> Option<bool> {
    state
        .spotify_browse
        .artist_states
        .get(&playing_artist(item)?)
        .copied()
}

pub(super) unsafe fn toggle_profile_publication(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let Some(item) = context_item(state).filter(|item| item.kind == ItemKind::Playlist) else {
        return;
    };
    if !state.spotify_browse.profile_states.contains_key(&item.uri) {
        set_status(
            state,
            super::spotify::catalog(state).text("spotify_profile_state_unavailable"),
            true,
        );
        return;
    }
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::ToggleProfilePublication { uri: item.uri },
        Pending::Plain,
    );
}

pub(super) unsafe fn toggle_artist_follow(window: HWND, active: Option<&apricot_core::MediaItem>) {
    let Some(state) = state(window) else {
        return;
    };
    let uri = if state.view == MainView::Player || super::focus_in_background_player(state) {
        active.and_then(playing_artist)
    } else {
        context_item(state).as_ref().and_then(artist_uri)
    };
    let Some(uri) = uri else {
        return;
    };
    if !state.spotify_browse.artist_states.contains_key(&uri) {
        set_status(
            state,
            super::spotify::catalog(state).text("spotify_artist_state_unavailable"),
            true,
        );
        return;
    }
    begin_edit(
        window,
        apricot_spotify::LibraryEdit::ToggleSaved { uri },
        Pending::Saved,
    );
}

/// The Spotify link of the selected row, for Copy link.
pub(super) fn selected_link(state: &WindowState) -> Option<String> {
    context_item(state)
        .and_then(|item| apricot_core::SpotifyRef::parse(&item.uri))
        .and_then(|reference| reference.to_url())
}

/// What the context menu offers for the selected row.
pub(super) fn row_menu(state: &WindowState) -> Option<apricot_app::context_menu::SpotifyRowMenu> {
    let item = context_item(state)?;
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
    let frame = top(state).filter(|_| state.view == MainView::SpotifyBrowse);
    let profile_section = item.kind == ItemKind::Section
        && frame.is_some_and(|frame| matches!(frame.source, Source::Profile { .. }));
    let editable =
        frame.is_some_and(|frame| frame.can_edit && matches!(frame.source, Source::Playlist(_)));
    // SAFETY: Reads the selection of this thread's list.
    let index = usize::try_from(unsafe { SendMessageW(state.list, LB_GETCURSEL, None, None).0 })
        .unwrap_or(0);
    let has_uid = |at: Option<usize>| {
        at.and_then(|at| frame?.items.get(at))
            .is_some_and(|item| item.uid.is_some())
    };
    Some(apricot_app::context_menu::SpotifyRowMenu {
        playable_item: item.kind.is_playable_item() && item.playable,
        collection: collection || matches!(item.kind, ItemKind::Page | ItemKind::Section),
        collection_plays: collection && !matches!(item.kind, ItemKind::Folder | ItemKind::User),
        album: !item.album_uri.is_empty(),
        artist: item.artist_uri.starts_with("spotify:artist:"),
        link: apricot_core::SpotifyRef::parse(&item.uri).is_some(),
        saved: if profile_section {
            frame.and_then(|frame| frame.following)
        } else if item.kind.is_playable_item() {
            selection_saved(state).or(item.saved)
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
        create_playlist: frame.is_some_and(|frame| matches!(frame.source, Source::Library { .. })),
        hide: frame.is_some_and(|frame| frame.personalised) && item.kind == ItemKind::Track,
        hidden: state
            .spotify_browse
            .hidden
            .as_ref()
            .is_some_and(|hidden| hidden.contains(&item.uri)),
        radio: matches!(
            item.kind,
            ItemKind::Track | ItemKind::Artist | ItemKind::Album | ItemKind::Playlist
        ),
        main_menu_pin: current_pin(state).map(|pin| {
            state
                .spotify_browse
                .pins
                .iter()
                .any(|existing| existing.target == pin.target)
        }),
        artist_followed: artist_uri(&item)
            .and_then(|uri| state.spotify_browse.artist_states.get(&uri).copied()),
        playlist_profile: item.kind == ItemKind::Playlist,
        playlist_published: state.spotify_browse.profile_states.get(&item.uri).copied(),
    })
}

/// `spotify_radio` (Ctrl+Alt+Shift+R): Spotify's radio for the selected
/// track, artist, album or playlist, or for the playing Spotify track. The
/// station opens as a list; Enter plays it.
pub(super) unsafe fn radio(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let selected = context_item(state)
        .filter(|item| {
            matches!(
                item.kind,
                ItemKind::Track | ItemKind::Artist | ItemKind::Album | ItemKind::Playlist
            )
        })
        .map(|item| (item.uri.clone(), item.name.clone()));
    let target = shortcut_target(super::focus_in_background_player(state), selected, || {
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
    let selected = frame.filter(|frame| frame.personalised).and_then(|frame| {
        let context = match &frame.source {
            Source::Playlist(uri) => uri.clone(),
            _ => return None,
        };
        selected_item(state)
            .filter(|item| item.kind == ItemKind::Track)
            .map(|item| (item.uri.clone(), context))
    });
    let target = shortcut_target(super::focus_in_background_player(state), selected, || {
        let item = state.application.player_session().current_item()?;
        if item.source != apricot_core::MediaSource::Spotify
            || !item.id.0.starts_with("spotify:track:")
        {
            return None;
        }
        let context = item.metadata.get("spotify_context")?.as_str()?;
        state
            .spotify_browse
            .personalised_contexts
            .contains(context)
            .then(|| (item.id.0.clone(), context.to_owned()))
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
    let selected = context_item(state)
        .filter(|item| item.kind != ItemKind::Unavailable && item.kind != ItemKind::Folder)
        .and_then(|item| {
            if item.kind == ItemKind::Section {
                // A section of an open profile follows the profile.
                profile.clone()
            } else {
                Some(item.uri.clone())
            }
        });
    let uri = shortcut_target(super::focus_in_background_player(state), selected, || {
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
    if state.view == MainView::SpotifyBrowse
        && !super::focus_in_background_player(state)
        && !state.spotify_browse.marks.is_empty()
    {
        begin_selection(window, &uri, Some(selection_saved(state) != Some(true)));
    } else {
        begin_edit(
            window,
            apricot_spotify::LibraryEdit::ToggleSaved { uri },
            Pending::Saved,
        );
    }
}

/// Player focus must never edit a row selected in the list behind it.
fn shortcut_target<T>(
    player_focus: bool,
    selected: Option<T>,
    playing: impl FnOnce() -> Option<T>,
) -> Option<T> {
    if player_focus {
        playing()
    } else {
        selected.or_else(playing)
    }
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
    let Some(item) = state(window).and_then(context_item) else {
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
            restore_focus(window);
            if let Some(playlist) = playlists.get(index - 1) {
                // The whole playlist loads first, to find duplicates.
                if let Some(state) = super::state(window) {
                    let text = texts
                        .text("spotify_checking_duplicates")
                        .replace("{name}", &playlist.name);
                    set_status(state, &text, false);
                }
                load_for(
                    window,
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
    if !state.spotify.is_current_account(stamp) {
        return;
    }
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
        (O::ProfilePublication { uri, published }, _) => {
            state
                .spotify_browse
                .profile_states
                .insert(uri.clone(), *published);
            texts
                .text(if *published {
                    "spotify_profile_published"
                } else {
                    "spotify_profile_unpublished"
                })
                .to_owned()
        }
        (O::SavedBatch { uris, saved }, _) => {
            let changed: std::collections::HashSet<_> = uris.iter().collect();
            for frame in &mut state.spotify_browse.frames {
                for item in &mut frame.items {
                    if changed.contains(&item.uri) {
                        item.saved = Some(*saved);
                    }
                }
                if !saved && matches!(frame.source, Source::LikedSongs) {
                    frame.remove_items(|item| !changed.contains(&item.uri));
                }
            }
            state.spotify_browse.cancel_context();
            state.spotify_browse.marks.clear();
            texts
                .text(if *saved {
                    "spotify_liked_done"
                } else {
                    "spotify_unliked_done"
                })
                .replace("{count}", &uris.len().to_string())
        }
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
            state.spotify_browse.cancel_context();
            state.spotify_browse.marks.clear();
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
            if let Some(frame) = state.spotify_browse.frames.last_mut() {
                frame.remove_items(|item| item.uid.as_deref() != Some(uid.as_str()));
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
                let count = uris.len();
                follow_up = Some((
                    apricot_spotify::LibraryEdit::AddToPlaylist {
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
    let Some(item) = state(window).and_then(context_item) else {
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
        C::SpotifyToggleMenuPin => toggle_menu_pin(window),
        C::SpotifyToggleProfilePublication => toggle_profile_publication(window),
        C::SpotifyToggleArtistFollow => toggle_artist_follow(window, None),
        C::SpotifyAddToPlaylist => {
            begin_selection(window, &item.uri, None);
        }
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
#[path = "spotify_native_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotify_player_actions_target_playing_track_instead_of_background_selection() {
        let selected = "spotify:album:selected";
        let playing = "spotify:track:recommended";
        assert_eq!(
            shortcut_target(true, Some(selected), || Some(playing)),
            Some(playing)
        );
        assert_eq!(
            shortcut_target(false, Some(selected), || Some(playing)),
            Some(selected)
        );
        assert_eq!(
            shortcut_target(true, Some(selected), || None),
            None,
            "a non-Spotify player must not edit the background Spotify row"
        );
        assert_eq!(
            shortcut_target(false, None, || Some(playing)),
            Some(playing)
        );
    }

    #[test]
    fn account_change_drops_private_caches_and_pending_answers() {
        let mut browse = BrowseState {
            playlists: Some(Vec::new()),
            hidden: Some(["spotify:track:a".to_owned()].into()),
            hidden_load: Some(SpotifyStamp::default()),
            pending: Some((SpotifyStamp::default(), Pending::Saved)),
            last_query: "private search".to_owned(),
            personalised_contexts: ["spotify:playlist:personal".to_owned()].into(),
            ..BrowseState::default()
        };
        browse.clear_account();
        assert!(browse.playlists.is_none());
        assert!(browse.hidden.is_none());
        assert!(browse.hidden_load.is_none());
        assert!(browse.pending.is_none());
        assert!(browse.frames.is_empty());
        assert!(browse.last_query.is_empty());
        assert!(browse.personalised_contexts.is_empty());
    }

    #[test]
    fn deleting_loaded_track_keeps_first_unread_track_in_next_page() {
        let row = |index: usize| {
            section_row(&apricot_spotify::catalog::Section {
                uri: format!("track-{index}"),
                title: index.to_string(),
                items: Vec::new(),
            })
        };
        let mut frame = Frame {
            source: Source::LikedSongs,
            title: String::new(),
            items: (0..50).map(row).collect(),
            next_offset: Some(50),
            selected: 49,
            loading: Some(SpotifyStamp::default()),
            refresh: Some(SpotifyStamp::default()),
            loaded: true,
            error: None,
            mixed: false,
            can_edit: false,
            can_rename: false,
            personalised: false,
            following: None,
            sections: Vec::new(),
            search_screen: false,
            focus_results: false,
        };
        frame.remove_items(|item| item.uri != "track-0");
        // The same server deletion shifts track 50 to index 49.
        let server_rows: Vec<_> = (1..100).map(row).collect();
        let offset = usize::try_from(frame.next_offset.unwrap()).unwrap();
        assert_eq!(server_rows[offset].uri, "track-50");
        assert_eq!(
            frame.source.request(offset as u64),
            Some(CatalogRequest::LikedSongs { offset: 49 })
        );
        assert!(
            frame.loading.is_none(),
            "page response before deletion must be discarded"
        );
        assert!(frame.refresh.is_none());
        assert_eq!(frame.selected, 48);
    }

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
            Source::Album("spotify:album:a".into()).request(300),
            Some(CatalogRequest::Album {
                uri: "spotify:album:a".into(),
                offset: 300
            })
        );
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
