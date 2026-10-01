//! Platform-neutral Spotify screens: the hub opened from the main menu and
//! the account list (`docs/SPOTIFY_PLAN.md` 4.1, 9.1).
//!
//! The hub lists only entries that already work. Later phases add their
//! entries here; there are no inert rows.

use std::collections::BTreeMap;

use apricot_core::{TranslationCatalog, action::action_by_id};
use apricot_spotify::{
    CatalogItem, ItemKind, QueueSection, SearchKind, SpotifyAccount, SpotifyAccounts, SpotifyError,
    SpotifyQueue,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpotifyHubEntry {
    /// Browser login; the only way in without a saved account.
    LogIn,
    Search,
    Library,
    LikedSongs,
    Playlists,
    Home,
    DailyMixes,
    RecentlyPlayed,
    Top,
    Browse,
    /// The Spotify queue, once an account is logged in.
    Queue,
    /// Spotify Connect devices, once an account is logged in.
    Devices,
    Accounts,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyHubItem {
    pub entry: SpotifyHubEntry,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyHubModel {
    pub accessible_name: String,
    pub items: Vec<SpotifyHubItem>,
}

fn with_shortcut(
    mut label: String,
    action_id: &str,
    show_shortcuts: bool,
    shortcuts: &BTreeMap<String, String>,
) -> String {
    if show_shortcuts && let Some(action) = action_by_id(action_id) {
        label.push('\t');
        label.push_str(
            shortcuts
                .get(action_id)
                .map_or(action.default_windows_shortcut, String::as_str),
        );
    }
    label
}

/// The active account when it is still logged in.
pub fn active_account(accounts: &SpotifyAccounts) -> Option<&SpotifyAccount> {
    let key = accounts.active.as_deref()?;
    accounts
        .accounts
        .iter()
        .find(|account| account.key == key && account.is_logged_in())
}

impl SpotifyHubModel {
    pub fn build(
        catalog: &TranslationCatalog,
        accounts: &SpotifyAccounts,
        show_shortcuts: bool,
        shortcuts: &BTreeMap<String, String>,
    ) -> Self {
        let mut items = Vec::new();
        if active_account(accounts).is_none() {
            items.push(SpotifyHubItem {
                entry: SpotifyHubEntry::LogIn,
                label: catalog.text("spotify_log_in").to_owned(),
            });
        }
        if active_account(accounts).is_some() {
            for (entry, key) in [
                (SpotifyHubEntry::Search, "spotify_search"),
                (SpotifyHubEntry::Library, "spotify_library"),
                (SpotifyHubEntry::LikedSongs, "spotify_liked_songs"),
                (SpotifyHubEntry::Playlists, "spotify_playlists"),
                (SpotifyHubEntry::Home, "spotify_home"),
                (SpotifyHubEntry::DailyMixes, "spotify_daily_mixes"),
                (SpotifyHubEntry::RecentlyPlayed, "spotify_recently_played"),
                (SpotifyHubEntry::Top, "spotify_top"),
                (SpotifyHubEntry::Browse, "spotify_browse_all"),
            ] {
                items.push(SpotifyHubItem {
                    entry,
                    label: with_shortcut(
                        catalog.text(key).to_owned(),
                        key,
                        show_shortcuts,
                        shortcuts,
                    ),
                });
            }
            items.push(SpotifyHubItem {
                entry: SpotifyHubEntry::Queue,
                label: with_shortcut(
                    catalog.text("spotify_queue").to_owned(),
                    "spotify_queue",
                    show_shortcuts,
                    shortcuts,
                ),
            });
            items.push(SpotifyHubItem {
                entry: SpotifyHubEntry::Devices,
                label: with_shortcut(
                    catalog.text("spotify_devices").to_owned(),
                    "spotify_devices",
                    show_shortcuts,
                    shortcuts,
                ),
            });
        }
        if !accounts.accounts.is_empty() {
            items.push(SpotifyHubItem {
                entry: SpotifyHubEntry::Accounts,
                label: with_shortcut(
                    catalog.text("spotify_accounts").to_owned(),
                    "spotify_accounts",
                    show_shortcuts,
                    shortcuts,
                ),
            });
        }
        Self {
            accessible_name: catalog.text("spotify").to_owned(),
            items,
        }
    }
}

/// One row of the account list: an account or the Add account row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpotifyAccountRow {
    Account { key: String, label: String },
    AddAccount { label: String },
}

impl SpotifyAccountRow {
    pub fn label(&self) -> &str {
        match self {
            Self::Account { label, .. } | Self::AddAccount { label } => label,
        }
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            Self::Account { key, .. } => Some(key),
            Self::AddAccount { .. } => None,
        }
    }
}

/// "Name, Premium, active" style row text.
pub fn account_label(
    catalog: &TranslationCatalog,
    account: &SpotifyAccount,
    active: bool,
) -> String {
    let product = match account.product.as_str() {
        "premium" => catalog.text("spotify_premium").to_owned(),
        "" => String::new(),
        _ => catalog.text("spotify_not_premium").to_owned(),
    };
    let state = if !account.is_logged_in() {
        catalog.text("spotify_logged_out")
    } else if active {
        catalog.text("spotify_active_account")
    } else {
        ""
    };
    [account.display_name.as_str(), product.as_str(), state]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn account_rows(
    catalog: &TranslationCatalog,
    accounts: &SpotifyAccounts,
) -> Vec<SpotifyAccountRow> {
    let mut rows: Vec<SpotifyAccountRow> = accounts
        .accounts
        .iter()
        .map(|account| SpotifyAccountRow::Account {
            key: account.key.clone(),
            label: account_label(
                catalog,
                account,
                accounts.active.as_deref() == Some(account.key.as_str()),
            ),
        })
        .collect();
    rows.push(SpotifyAccountRow::AddAccount {
        label: catalog.text("spotify_add_account").to_owned(),
    });
    rows
}

/// One row of the Spotify queue view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpotifyQueueRow {
    /// The track that plays now.
    Current { label: String },
    /// An upcoming occurrence, identified by its UID.
    Entry {
        uid: String,
        section: QueueSection,
        label: String,
    },
    /// Nothing plays on this device, or nothing comes next.
    Message { label: String },
}

impl SpotifyQueueRow {
    pub fn label(&self) -> &str {
        match self {
            Self::Current { label } | Self::Entry { label, .. } | Self::Message { label } => label,
        }
    }

    pub fn uid(&self) -> Option<&str> {
        match self {
            Self::Entry { uid, .. } => Some(uid),
            _ => None,
        }
    }
}

/// What the queue context menu offers for the selected row.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SpotifyQueueMenu {
    pub play: bool,
    pub move_up: bool,
    pub move_down: bool,
    pub remove: bool,
    pub clear: bool,
}

fn title_and_artists(catalog: &TranslationCatalog, title: &str, artists: &str) -> String {
    let title = if title.is_empty() {
        catalog.text("unknown")
    } else {
        title
    };
    if artists.is_empty() {
        title.to_owned()
    } else {
        format!("{title}, {artists}")
    }
}

/// "Now playing: title, artists", then the upcoming tracks with their
/// section ("Added manually", "Next from context", "Autoplay") last, so the
/// title is heard first.
pub fn queue_rows(catalog: &TranslationCatalog, queue: &SpotifyQueue) -> Vec<SpotifyQueueRow> {
    let Some(current) = queue.current.as_ref().filter(|_| queue.active) else {
        return vec![SpotifyQueueRow::Message {
            label: catalog.text("spotify_queue_inactive").to_owned(),
        }];
    };
    let mut rows = vec![SpotifyQueueRow::Current {
        label: catalog.text("spotify_queue_now_playing").replace(
            "{title}",
            &title_and_artists(catalog, &current.title, &current.artists),
        ),
    }];
    rows.extend(queue.entries.iter().map(|entry| {
        let section = catalog.text(match entry.section {
            QueueSection::Manual => "spotify_queue_manual",
            QueueSection::Context => "spotify_queue_context",
            QueueSection::Autoplay => "spotify_queue_autoplay",
        });
        SpotifyQueueRow::Entry {
            uid: entry.uid.clone(),
            section: entry.section,
            label: format!(
                "{}, {section}",
                title_and_artists(catalog, &entry.title, &entry.artists)
            ),
        }
    }));
    if queue.entries.is_empty() {
        rows.push(SpotifyQueueRow::Message {
            label: catalog.text("spotify_queue_nothing_next").to_owned(),
        });
    }
    rows
}

/// Menu of the row at `index`: moves stay within the manually added tracks,
/// Clear only when there are any.
pub fn queue_menu(rows: &[SpotifyQueueRow], index: usize) -> SpotifyQueueMenu {
    let manual = |at: Option<usize>| {
        at.and_then(|at| rows.get(at)).is_some_and(|row| {
            matches!(
                row,
                SpotifyQueueRow::Entry {
                    section: QueueSection::Manual,
                    ..
                }
            )
        })
    };
    let clear = (0..rows.len()).any(|at| manual(Some(at)));
    match rows.get(index) {
        Some(SpotifyQueueRow::Entry { section, .. }) => SpotifyQueueMenu {
            play: true,
            move_up: *section == QueueSection::Manual && manual(index.checked_sub(1)),
            move_down: *section == QueueSection::Manual && manual(Some(index + 1)),
            remove: true,
            clear,
        },
        _ => SpotifyQueueMenu {
            clear,
            ..SpotifyQueueMenu::default()
        },
    }
}

/// "Name, this computer, playing" rows of the device list.
pub fn device_rows(
    catalog: &TranslationCatalog,
    devices: &[apricot_spotify::SpotifyDevice],
) -> Vec<String> {
    devices
        .iter()
        .map(|device| {
            let mut parts = vec![device.name.clone()];
            if device.this_device {
                parts.push(catalog.text("spotify_device_this").to_owned());
            }
            if device.active {
                parts.push(catalog.text("spotify_device_playing").to_owned());
            }
            parts.join(", ")
        })
        .collect()
}

/// The spoken type of a row ("album", "playlist").
pub fn kind_word(catalog: &TranslationCatalog, kind: ItemKind) -> String {
    catalog
        .text(match kind {
            ItemKind::Track => "spotify_kind_track",
            ItemKind::Episode => "spotify_kind_episode",
            ItemKind::Album => "spotify_kind_album",
            ItemKind::Artist => "spotify_kind_artist",
            ItemKind::Playlist => "spotify_kind_playlist",
            ItemKind::Show => "spotify_kind_show",
            ItemKind::Audiobook => "spotify_kind_audiobook",
            ItemKind::User => "spotify_kind_user",
            ItemKind::Folder => "spotify_kind_folder",
            ItemKind::LikedSongs => "spotify_kind_liked_songs",
            ItemKind::Genre | ItemKind::Page => "spotify_kind_genre",
            ItemKind::Section => "spotify_kind_section",
            ItemKind::Unavailable => "spotify_unavailable",
        })
        .to_owned()
}

/// m:ss or h:mm:ss.
pub fn duration_text(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Row text: the name first, then the type (always for collections, for
/// tracks and episodes only in mixed lists), artists or owner, length,
/// "liked" (outside Liked Songs) and "unavailable" when it cannot be played.
pub fn item_label(
    catalog: &TranslationCatalog,
    item: &CatalogItem,
    mixed: bool,
    show_liked: bool,
) -> String {
    if item.kind == ItemKind::Unavailable {
        return if item.name.is_empty() {
            catalog.text("spotify_unavailable_item").to_owned()
        } else {
            format!("{}, {}", item.name, catalog.text("spotify_unavailable"))
        };
    }
    let mut parts = vec![item.name.clone()];
    if mixed || !item.kind.is_playable_item() {
        parts.push(kind_word(catalog, item.kind));
    }
    if !item.subtitle.is_empty() {
        parts.push(item.subtitle.clone());
    }
    if let Some(count) = item.count {
        let key = if item.kind == ItemKind::Section {
            "spotify_item_count"
        } else {
            "spotify_song_count"
        };
        parts.push(catalog.text(key).replace("{count}", &count.to_string()));
    }
    if let Some(duration) = item.duration_ms.filter(|_| item.kind.is_playable_item()) {
        parts.push(duration_text(duration));
    }
    if show_liked && item.kind.is_playable_item() && item.saved == Some(true) {
        parts.push(catalog.text("spotify_liked").to_owned());
    }
    if !item.playable && item.kind.is_playable_item() {
        parts.push(catalog.text("spotify_unavailable").to_owned());
    }
    parts.retain(|part| !part.is_empty());
    parts.join(", ")
}

/// Labels of the search type choice, in `SearchKind::ALL` order.
pub fn search_kind_labels(catalog: &TranslationCatalog) -> Vec<String> {
    SearchKind::ALL
        .iter()
        .map(|kind| {
            catalog
                .text(match kind {
                    SearchKind::All => "spotify_search_all",
                    SearchKind::Tracks => "spotify_search_tracks",
                    SearchKind::Artists => "spotify_search_artists",
                    SearchKind::Albums => "spotify_search_albums",
                    SearchKind::Playlists => "spotify_search_playlists",
                    SearchKind::Shows => "spotify_search_shows",
                    SearchKind::Episodes => "spotify_search_episodes",
                    SearchKind::Audiobooks => "spotify_search_audiobooks",
                })
                .to_owned()
        })
        .collect()
}

/// Localized error text with `{error}` filled in when there is a detail.
pub fn error_text(catalog: &TranslationCatalog, error: &SpotifyError) -> String {
    catalog
        .text(error.text_key())
        .replace("{error}", error.detail())
        .trim_end_matches([' ', ':'])
        .to_owned()
}

pub fn named(catalog: &TranslationCatalog, key: &str, name: &str) -> String {
    catalog.text(key).replace("{name}", name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::main_menu::english_catalog;

    fn account(key: &str, product: &str, logged_in: bool) -> SpotifyAccount {
        SpotifyAccount {
            key: key.into(),
            display_name: format!("User {key}"),
            product: product.into(),
            country: "SI".into(),
            credentials: if logged_in {
                "blob".into()
            } else {
                String::new()
            },
        }
    }

    #[test]
    fn hub_without_accounts_offers_only_login() {
        let model = SpotifyHubModel::build(
            &english_catalog(),
            &SpotifyAccounts::default(),
            true,
            &BTreeMap::new(),
        );
        assert_eq!(model.accessible_name, "Spotify");
        assert_eq!(model.items.len(), 1);
        assert_eq!(model.items[0].entry, SpotifyHubEntry::LogIn);
    }

    #[test]
    fn hub_with_active_login_shows_accounts_with_shortcut() {
        let accounts = SpotifyAccounts {
            active: Some("a".into()),
            accounts: vec![account("a", "premium", true)],
            ..SpotifyAccounts::default()
        };
        let model = SpotifyHubModel::build(&english_catalog(), &accounts, true, &BTreeMap::new());
        let entries: Vec<_> = model.items.iter().map(|item| item.entry).collect();
        assert_eq!(
            entries,
            [
                SpotifyHubEntry::Search,
                SpotifyHubEntry::Library,
                SpotifyHubEntry::LikedSongs,
                SpotifyHubEntry::Playlists,
                SpotifyHubEntry::Home,
                SpotifyHubEntry::DailyMixes,
                SpotifyHubEntry::RecentlyPlayed,
                SpotifyHubEntry::Top,
                SpotifyHubEntry::Browse,
                SpotifyHubEntry::Queue,
                SpotifyHubEntry::Devices,
                SpotifyHubEntry::Accounts
            ]
        );
        assert_eq!(model.items[0].label, "Search\tCtrl+Alt+Shift+Y");
        assert_eq!(model.items[2].label, "Liked Songs\tCtrl+Alt+Shift+F");
        assert_eq!(model.items[4].label, "Home");
        assert_eq!(model.items[5].label, "Daily Mixes\tCtrl+Alt+Shift+M");
        assert_eq!(model.items[9].label, "Spotify queue\tCtrl+Alt+Shift+Q");
        assert_eq!(model.items[10].label, "Spotify devices\tCtrl+Alt+Shift+O");
        assert!(model.items[11].label.ends_with("\tCtrl+Alt+Shift+C"));
    }

    fn catalog_item(kind: ItemKind, name: &str, subtitle: &str) -> CatalogItem {
        CatalogItem {
            kind,
            uri: "spotify:x:1".into(),
            name: name.into(),
            subtitle: subtitle.into(),
            album: String::new(),
            album_uri: String::new(),
            artist_uri: String::new(),
            duration_ms: Some(355_000),
            playable: true,
            explicit: false,
            uid: None,
            saved: None,
            count: None,
            editable: false,
            format: String::new(),
        }
    }

    #[test]
    fn rows_say_name_type_people_and_length() {
        let catalog = english_catalog();
        let track = catalog_item(ItemKind::Track, "Bohemian Rhapsody", "Queen");
        assert_eq!(
            item_label(&catalog, &track, false, true),
            "Bohemian Rhapsody, Queen, 5:55"
        );
        assert_eq!(
            item_label(&catalog, &track, true, true),
            "Bohemian Rhapsody, track, Queen, 5:55"
        );
        let mut liked_track = track.clone();
        liked_track.saved = Some(true);
        assert_eq!(
            item_label(&catalog, &liked_track, false, true),
            "Bohemian Rhapsody, Queen, 5:55, liked"
        );
        assert_eq!(
            item_label(&catalog, &liked_track, false, false),
            "Bohemian Rhapsody, Queen, 5:55"
        );
        let album = catalog_item(ItemKind::Album, "A Night at the Opera", "Queen");
        assert_eq!(
            item_label(&catalog, &album, false, true),
            "A Night at the Opera, album, Queen"
        );
        let mut liked = catalog_item(ItemKind::LikedSongs, "Liked Songs", "");
        liked.count = Some(12);
        assert_eq!(
            item_label(&catalog, &liked, false, true),
            "Liked Songs, playlist, 12 songs"
        );
        let mut gone = catalog_item(ItemKind::Track, "Old", "X");
        gone.playable = false;
        assert!(item_label(&catalog, &gone, false, true).ends_with(", unavailable"));
        let restricted = catalog_item(ItemKind::Unavailable, "", "");
        assert_eq!(
            item_label(&catalog, &restricted, false, true),
            "Unavailable item"
        );
        assert_eq!(duration_text(3_725_000), "1:02:05");
    }

    #[test]
    fn device_rows_name_this_computer_and_the_playing_device() {
        let device = |name: &str, active: bool, this_device: bool| apricot_spotify::SpotifyDevice {
            id: name.into(),
            name: name.into(),
            active,
            this_device,
        };
        let rows = device_rows(
            &english_catalog(),
            &[
                device("ApricotPlayer (PC)", false, true),
                device("iPhone", true, false),
            ],
        );
        assert_eq!(
            rows,
            ["ApricotPlayer (PC), this computer", "iPhone, playing"]
        );
    }

    fn queue_entry(uid: &str, section: QueueSection, title: &str) -> apricot_spotify::QueueEntry {
        apricot_spotify::QueueEntry {
            uid: uid.into(),
            uri: format!("spotify:track:{uid}"),
            section,
            title: title.into(),
            artists: "Artist".into(),
        }
    }

    fn queue() -> SpotifyQueue {
        SpotifyQueue {
            active: true,
            revision: "1".into(),
            context_uri: "spotify:album:x".into(),
            current: Some(queue_entry("c0", QueueSection::Context, "Now")),
            entries: vec![
                queue_entry("q0", QueueSection::Manual, "First"),
                queue_entry("q1", QueueSection::Manual, ""),
                queue_entry("c1", QueueSection::Context, "Next"),
            ],
            shuffle: false,
            repeat: apricot_spotify::RepeatMode::Off,
        }
    }

    #[test]
    fn queue_rows_name_the_title_first_and_the_section_last() {
        let rows = queue_rows(&english_catalog(), &queue());
        let labels: Vec<_> = rows.iter().map(SpotifyQueueRow::label).collect();
        assert_eq!(
            labels,
            [
                "Now playing: Now, Artist",
                "First, Artist, added manually",
                "unknown, Artist, added manually",
                "Next, Artist, next from the album or playlist",
            ]
        );
        assert_eq!(rows[1].uid(), Some("q0"));
        assert_eq!(rows[0].uid(), None);
    }

    #[test]
    fn inactive_or_empty_queue_says_so() {
        let mut inactive = queue();
        inactive.active = false;
        let rows = queue_rows(&english_catalog(), &inactive);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].uid().is_none());
        let mut empty = queue();
        empty.entries.clear();
        let rows = queue_rows(&english_catalog(), &empty);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].label(), "Nothing comes next.");
    }

    #[test]
    fn queue_menu_moves_only_among_manual_tracks() {
        let rows = queue_rows(&english_catalog(), &queue());
        let first = queue_menu(&rows, 1);
        assert!(first.play && first.remove && first.clear);
        assert!(!first.move_up && first.move_down);
        let second = queue_menu(&rows, 2);
        assert!(second.move_up && !second.move_down);
        let context = queue_menu(&rows, 3);
        assert!(context.play && context.remove && !context.move_up && !context.move_down);
        let current = queue_menu(&rows, 0);
        assert!(!current.play && !current.remove && current.clear);
        let mut no_manual = queue();
        no_manual.entries.clear();
        let rows = queue_rows(&english_catalog(), &no_manual);
        assert_eq!(queue_menu(&rows, 0), SpotifyQueueMenu::default());
    }

    #[test]
    fn logged_out_active_account_offers_login_and_accounts() {
        let accounts = SpotifyAccounts {
            active: Some("a".into()),
            accounts: vec![account("a", "premium", false)],
            ..SpotifyAccounts::default()
        };
        let model = SpotifyHubModel::build(&english_catalog(), &accounts, false, &BTreeMap::new());
        let entries: Vec<_> = model.items.iter().map(|item| item.entry).collect();
        assert_eq!(entries, [SpotifyHubEntry::LogIn, SpotifyHubEntry::Accounts]);
        assert!(!model.items[1].label.contains('\t'));
    }

    #[test]
    fn account_rows_describe_product_and_state_and_end_with_add() {
        let catalog = english_catalog();
        let accounts = SpotifyAccounts {
            active: Some("a".into()),
            accounts: vec![
                account("a", "premium", true),
                account("b", "free", true),
                account("c", "premium", false),
            ],
            ..SpotifyAccounts::default()
        };
        let rows = account_rows(&catalog, &accounts);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].label(), "User a, Premium, active");
        assert_eq!(rows[1].label(), "User b, Free, playback needs Premium");
        assert_eq!(rows[2].label(), "User c, Premium, logged out");
        assert_eq!(rows[3].key(), None);
        assert_eq!(rows[3].label(), "Add account");
    }

    #[test]
    fn errors_fill_or_drop_the_detail() {
        let catalog = english_catalog();
        assert_eq!(
            error_text(&catalog, &SpotifyError::Network("timeout".into())),
            "Could not reach Spotify: timeout"
        );
        assert_eq!(
            error_text(&catalog, &SpotifyError::Network(String::new())),
            "Could not reach Spotify"
        );
        assert_eq!(
            error_text(&catalog, &SpotifyError::Cancelled),
            "Spotify login cancelled."
        );
    }
}
