//! Account-scoped playlist subscriptions. Snapshots are committed only after a complete read.
use crate::{CatalogItem, accounts::AccountStore};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistSnapshot {
    pub uri: String,
    pub name: String,
    pub owner_uri: String,
    pub revision: String,
    pub tracks: Vec<CatalogItem>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationOptions {
    pub added: bool,
    pub removed: bool,
    pub reordered: bool,
}
impl Default for NotificationOptions {
    fn default() -> Self {
        Self {
            added: true,
            removed: false,
            reordered: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistSubscription {
    pub uri: String,
    pub name: String,
    pub options: NotificationOptions,
    pub baseline: PlaylistSnapshot,
    pub last_checked: u64,
    #[serde(default)]
    pub local_changes: Vec<LocalPlaylistChange>,
    /// Subscribed because the account follows the playlist, not by hand;
    /// it is dropped again when the playlist is no longer followed (2.1.3).
    #[serde(default)]
    pub automatic: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPlaylistChange {
    pub added: Vec<String>,
    pub removed_uids: Vec<String>,
    pub moved_uids: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistChanges {
    pub id: String,
    pub uri: String,
    pub name: String,
    pub timestamp: u64,
    pub added: Vec<CatalogItem>,
    pub removed: Vec<CatalogItem>,
    pub reordered: bool,
    /// A new release of a followed artist instead of a playlist change; its
    /// `uri` is the album (2.1, RC 4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<ReleaseInfo>,
}
impl PlaylistChanges {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && !self.reordered
            && self.release.is_none()
    }
}

/// What a new release is: its artists, its type and its date.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub artists: Vec<String>,
    /// Spotify's album type: `ALBUM`, `SINGLE`, `EP` or `COMPILATION`.
    pub kind: String,
    /// The release date as Spotify gives it, for example `2026-11-19`.
    pub date: String,
}

/// Which followed artists announce their new releases, like playlist
/// subscriptions announce changes (2.1, RC 4). Every followed artist
/// announces until the user chooses otherwise (2.1.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtistReleaseOptions {
    /// Every followed artist announces, also one followed later, except the
    /// `excluded` ones.
    pub all: bool,
    pub included: BTreeSet<String>,
    pub excluded: BTreeSet<String>,
    /// The newest change from the settings window that was applied, so an
    /// older one arriving late never undoes it.
    pub revision: u64,
    /// Releases already seen in Spotify's feed, oldest first.
    pub seen: Vec<String>,
    /// The feed was read once since releases were turned on; until then
    /// every release in it is only remembered, not announced.
    pub seeded: bool,
    pub last_checked: u64,
}

impl Default for ArtistReleaseOptions {
    fn default() -> Self {
        Self {
            all: true,
            included: BTreeSet::new(),
            excluded: BTreeSet::new(),
            revision: 0,
            seen: Vec::new(),
            seeded: false,
            last_checked: 0,
        }
    }
}

/// How the background check works, from the application settings
/// (2.1.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubscriptionPreferences {
    /// Subscriptions are checked on their own; Check now always works.
    pub automatic_checks: bool,
    /// Playlists the account follows are subscribed on their own.
    pub follow_playlists: bool,
}

impl Default for SubscriptionPreferences {
    fn default() -> Self {
        Self {
            automatic_checks: true,
            follow_playlists: true,
        }
    }
}

/// At most this many followed playlists are subscribed in one check, so a
/// large library is read over several checks instead of at once.
pub const FOLLOWED_PER_CHECK: usize = 25;

/// The revision of a new artist choice: later than every choice before,
/// also one made in another window that read the same saved revision, so
/// the last choice the user made is the one kept (review 5, B19).
#[must_use]
pub fn next_artist_revision(current: u64) -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        });
    now.max(current.saturating_add(1))
}

impl ArtistReleaseOptions {
    /// 2.1.0 to 2.1.2 saved releases as off for every account, also one
    /// that never chose; such an account now hears every followed artist
    /// like a new one, until it chooses otherwise (2.1.3).
    pub fn default_to_all(&mut self) {
        if self.revision == 0 && !self.any_enabled() {
            self.all = true;
            self.seeded = false;
        }
    }

    /// A choice from the window; an older one than the kept choice is
    /// ignored (review 5, B19).
    pub fn apply_choice(
        &mut self,
        revision: u64,
        all: bool,
        included: BTreeSet<String>,
        excluded: BTreeSet<String>,
    ) {
        if revision <= self.revision {
            return;
        }
        let before = self.any_enabled();
        self.revision = revision;
        self.all = all;
        self.included = included;
        self.excluded = excluded;
        self.started(before);
    }

    #[must_use]
    pub fn is_enabled(&self, artist: &str) -> bool {
        if self.all {
            !self.excluded.contains(artist)
        } else {
            self.included.contains(artist)
        }
    }

    #[must_use]
    pub fn any_enabled(&self) -> bool {
        self.all || !self.included.is_empty()
    }

    /// Turns one artist on or off.
    pub fn set(&mut self, artist: &str, enabled: bool) {
        let before = self.any_enabled();
        // On for all keeps only the exceptions; otherwise only the chosen.
        let list = if self.all {
            &mut self.excluded
        } else {
            &mut self.included
        };
        if enabled == self.all {
            list.remove(artist);
        } else {
            list.insert(artist.to_owned());
        }
        self.started(before);
    }

    /// Turns every followed artist on or off.
    pub fn set_all(&mut self, enabled: bool) {
        let before = self.any_enabled();
        self.all = enabled;
        self.included.clear();
        self.excluded.clear();
        self.started(before);
    }

    /// Releases turned on again are read once quietly, so the feed's older
    /// releases are not all announced at once.
    fn started(&mut self, before: bool) {
        if !before && self.any_enabled() {
            self.seeded = false;
        }
    }

    #[must_use]
    pub fn due(&self, now: u64, interval_seconds: u64, force: bool) -> bool {
        self.any_enabled()
            && (force
                || now.saturating_sub(self.last_checked) >= interval_seconds.clamp(60, 604_800))
    }
}

/// One release in Spotify's feed of new releases of followed artists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedRelease {
    pub uri: String,
    pub name: String,
    pub kind: String,
    pub date: String,
    /// The release's artists: URI and name.
    pub artists: Vec<(String, String)>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubscriptionDocument {
    pub version: u32,
    pub interval_seconds: u64,
    pub entries: Vec<PlaylistSubscription>,
    pub history: Vec<PlaylistChanges>,
    pub outbox: Vec<PlaylistChanges>,
    /// New releases of followed artists (2.1, RC 4).
    pub artists: ArtistReleaseOptions,
    /// Followed playlists the user unsubscribed by hand; they are not
    /// subscribed again on their own (2.1.3).
    pub ignored_followed: BTreeSet<String>,
    /// When the followed playlists were last read.
    pub followed_checked: u64,
}
impl Default for SubscriptionDocument {
    fn default() -> Self {
        Self {
            version: 1,
            interval_seconds: 3600,
            entries: Vec::new(),
            history: Vec::new(),
            outbox: Vec::new(),
            artists: ArtistReleaseOptions::default(),
            ignored_followed: BTreeSet::new(),
            followed_checked: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubscriptionCommand {
    Subscribe(String),
    Unsubscribe(String),
    Check {
        force: bool,
    },
    Options {
        uri: String,
        options: NotificationOptions,
    },
    Interval(u64),
    Acknowledge(Vec<String>),
    /// The followed artists whose new releases are announced, as the
    /// settings window shows them; an older revision is ignored.
    ArtistReleases {
        revision: u64,
        all: bool,
        included: BTreeSet<String>,
        excluded: BTreeSet<String>,
    },
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubscriptionReport {
    pub account: String,
    pub message: String,
    pub document: SubscriptionDocument,
    pub errors: Vec<String>,
    pub checked: usize,
    /// The errors came from a check that works in the background, so they
    /// are not shown in a message box (review 2.1).
    pub background: bool,
}

/// The subscriptions of one account.
///
/// # Errors
///
/// Returns a message key when the saved data cannot be used.
pub fn load(store: &AccountStore, key: &str) -> Result<SubscriptionDocument, String> {
    validate_key(key)?;
    let path = store.account_dir(key).join("playlist_subscriptions.json");
    if !path.exists() {
        return Ok(SubscriptionDocument::default());
    }
    let mut document: SubscriptionDocument =
        apricot_storage::read_json(&path).map_err(|e| e.to_string())?;
    if document.version != 1 {
        return Err("spotify_sub_data_version".into());
    }
    if document
        .entries
        .iter()
        .any(|entry| entry.uri != entry.baseline.uri || !entry.uri.starts_with("spotify:playlist:"))
    {
        return Err("spotify_sub_data_invalid".into());
    }
    document.artists.default_to_all();
    Ok(document)
}

/// Saves the subscriptions of one account.
///
/// # Errors
///
/// Returns the error when the file cannot be written.
pub fn save(
    store: &AccountStore,
    key: &str,
    document: &SubscriptionDocument,
) -> Result<(), String> {
    validate_key(key)?;
    apricot_storage::write_json_atomic(
        &store.account_dir(key).join("playlist_subscriptions.json"),
        document,
    )
    .map_err(|e| e.to_string())
}
fn validate_key(key: &str) -> Result<(), String> {
    if key.len() == 16 && key.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("spotify_sub_account_invalid".into())
    }
}
fn keys(items: &[CatalogItem]) -> Vec<String> {
    let mut counts = BTreeMap::<&str, usize>::new();
    items
        .iter()
        .map(|item| {
            if let Some(uid) = item.uid.as_deref().filter(|s| !s.is_empty()) {
                return format!("uid:{uid}");
            }
            let count = counts.entry(&item.uri).or_default();
            *count += 1;
            format!("uri:{}:{}", item.uri, count)
        })
        .collect()
}
pub fn compare(
    previous: &PlaylistSnapshot,
    next: &PlaylistSnapshot,
    timestamp: u64,
) -> PlaylistChanges {
    let before = keys(&previous.tracks);
    let after = keys(&next.tracks);
    let old_keys: BTreeSet<_> = before.iter().collect();
    let new_keys: BTreeSet<_> = after.iter().collect();
    let added = after
        .iter()
        .zip(&next.tracks)
        .filter(|(key, _)| !old_keys.contains(key))
        .map(|(_, track)| track.clone())
        .collect();
    let removed = before
        .iter()
        .zip(&previous.tracks)
        .filter(|(key, _)| !new_keys.contains(key))
        .map(|(_, track)| track.clone())
        .collect();
    let old_retained: Vec<_> = before.iter().filter(|key| new_keys.contains(key)).collect();
    let new_retained: Vec<_> = after.iter().filter(|key| old_keys.contains(key)).collect();
    PlaylistChanges {
        id: format!("{}:{}:{}", next.uri, next.revision, timestamp),
        uri: next.uri.clone(),
        name: next.name.clone(),
        timestamp,
        added,
        removed,
        reordered: old_retained != new_retained,
        release: None,
    }
}
impl SubscriptionDocument {
    /// Adds a playlist; `false` when it was already subscribed.
    ///
    /// # Errors
    ///
    /// Returns a message key for an unknown owner or the account's own list.
    pub fn subscribe(
        &mut self,
        snapshot: PlaylistSnapshot,
        own_uri: &str,
        now: u64,
    ) -> Result<bool, String> {
        if snapshot.owner_uri.is_empty() {
            return Err("spotify_sub_owner_unknown".into());
        }
        if snapshot.owner_uri == own_uri {
            return Err("spotify_sub_own_playlist".into());
        }
        self.ignored_followed.remove(&snapshot.uri);
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.uri == snapshot.uri)
        {
            // Subscribed by hand, it stays when the playlist is unfollowed.
            entry.automatic = false;
            return Ok(false);
        }
        self.entries.push(PlaylistSubscription {
            uri: snapshot.uri.clone(),
            name: snapshot.name.clone(),
            options: NotificationOptions::default(),
            baseline: snapshot,
            last_checked: now,
            local_changes: Vec::new(),
            automatic: false,
        });
        Ok(true)
    }

    /// Ends a subscription; a followed playlist is then not subscribed again
    /// on its own (2.1.3).
    pub fn unsubscribe(&mut self, uri: &str) {
        if self
            .entries
            .iter()
            .any(|entry| entry.uri == uri && entry.automatic)
        {
            self.ignored_followed.insert(uri.to_owned());
        }
        self.entries.retain(|entry| entry.uri != uri);
    }

    /// The followed playlists still to subscribe, at most
    /// [`FOLLOWED_PER_CHECK`], and drops the automatic subscriptions of
    /// playlists no longer followed. `followed` is the account's whole
    /// list of followed playlists as URI and owner URI; the account's own
    /// playlists are left out (2.1.3).
    pub fn sync_followed(&mut self, followed: &[(String, String)], own_uri: &str) -> Vec<String> {
        let followed_uris: BTreeSet<&str> = followed.iter().map(|(uri, _)| uri.as_str()).collect();
        self.entries
            .retain(|entry| !entry.automatic || followed_uris.contains(entry.uri.as_str()));
        self.ignored_followed
            .retain(|uri| followed_uris.contains(uri.as_str()));
        let mut seen = BTreeSet::new();
        followed
            .iter()
            .filter(|(uri, owner)| {
                uri.starts_with("spotify:playlist:")
                    && !owner.is_empty()
                    && owner != own_uri
                    && !self.ignored_followed.contains(uri)
                    && !self.entries.iter().any(|entry| entry.uri == *uri)
                    && seen.insert(uri.as_str())
            })
            .take(FOLLOWED_PER_CHECK)
            .map(|(uri, _)| uri.clone())
            .collect()
    }

    /// Subscribes a followed playlist on its own, with its current content
    /// as the silent starting point.
    ///
    /// # Errors
    ///
    /// Returns a message key for an unknown owner or the account's own list.
    pub fn subscribe_followed(
        &mut self,
        snapshot: PlaylistSnapshot,
        own_uri: &str,
        now: u64,
    ) -> Result<(), String> {
        let uri = snapshot.uri.clone();
        if self.ignored_followed.contains(&uri) {
            return Ok(());
        }
        if self.subscribe(snapshot, own_uri, now)?
            && let Some(entry) = self.entries.iter_mut().find(|entry| entry.uri == uri)
        {
            entry.automatic = true;
        }
        Ok(())
    }

    /// A due playlist whose revision did not change was checked without
    /// reading all of it (2.1.3).
    pub fn note_unchanged(&mut self, uri: &str, revision: &str, now: u64) -> bool {
        match self
            .entries
            .iter_mut()
            .find(|entry| entry.uri == uri && entry.baseline.revision == revision)
        {
            Some(entry) => {
                entry.last_checked = now;
                true
            }
            None => false,
        }
    }

    /// Compares a fresh snapshot with the saved one and keeps the changes
    /// the user asked to hear about.
    ///
    /// # Errors
    ///
    /// Returns a message key when the playlist owner is unknown.
    pub fn apply_snapshot(
        &mut self,
        snapshot: PlaylistSnapshot,
        own_uri: &str,
        now: u64,
        suppress: bool,
    ) -> Result<Option<PlaylistChanges>, String> {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.uri == snapshot.uri)
        else {
            return Ok(None);
        };
        if snapshot.owner_uri.is_empty() {
            return Err("spotify_sub_owner_unknown".into());
        }
        if snapshot.owner_uri == own_uri {
            let uri = snapshot.uri;
            self.entries.retain(|entry| entry.uri != uri);
            return Ok(None);
        }
        let mut changes = compare(&entry.baseline, &snapshot, now);
        if snapshot.revision != entry.baseline.revision {
            // Every own move is left out at once, then the order is compared
            // once: several own moves never look like a change by someone
            // else (review 2.1).
            let mut moved = BTreeSet::new();
            for local in entry.local_changes.drain(..) {
                for uri in local.added {
                    if let Some(index) = changes.added.iter().position(|track| track.uri == uri) {
                        changes.added.remove(index);
                    }
                }
                for uid in local.removed_uids {
                    if let Some(index) = changes
                        .removed
                        .iter()
                        .position(|track| track.uid.as_deref() == Some(uid.as_str()))
                    {
                        changes.removed.remove(index);
                    }
                }
                moved.extend(local.moved_uids.into_iter().map(|uid| format!("uid:{uid}")));
            }
            if !moved.is_empty() {
                let before = keys(&entry.baseline.tracks);
                let after = keys(&snapshot.tracks);
                let old: Vec<_> = before
                    .iter()
                    .filter(|key| !moved.contains(*key) && after.contains(key))
                    .collect();
                let new: Vec<_> = after
                    .iter()
                    .filter(|key| !moved.contains(*key) && before.contains(key))
                    .collect();
                changes.reordered = old != new;
            }
        }
        entry.name.clone_from(&snapshot.name);
        entry.baseline = snapshot;
        entry.last_checked = now;
        if !entry.options.added {
            changes.added.clear();
        }
        if !entry.options.removed {
            changes.removed.clear();
        }
        if !entry.options.reordered {
            changes.reordered = false;
        }
        if suppress || changes.is_empty() {
            return Ok(None);
        }
        self.history.insert(0, changes.clone());
        self.history.truncate(200);
        self.outbox.push(changes.clone());
        Ok(Some(changes))
    }
    /// A check that failed waits for the interval like one that worked,
    /// instead of being tried again every minute (review 2.1).
    pub fn note_failed_check(&mut self, uri: &str, now: u64) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.uri == uri) {
            entry.last_checked = now;
        }
    }

    /// Spotify's feed of new releases: releases not seen before become
    /// notifications when one of their artists is turned on. The first read
    /// after turning releases on only remembers what is there.
    /// `followed`, when known, are the account's followed artists: with
    /// exceptions, only those are a reason for a release, so turning off an
    /// artist also silences that artist's release with someone not followed
    /// (review 5, B20).
    pub fn apply_release_feed(
        &mut self,
        feed: &[FeedRelease],
        now: u64,
        followed: Option<&BTreeSet<String>>,
    ) -> Vec<PlaylistChanges> {
        let options = &mut self.artists;
        options.last_checked = now;
        let mut found = Vec::new();
        let seeded = options.seeded;
        // The feed lists the newest first; notifications keep their order.
        for release in feed.iter().rev() {
            if release.uri.is_empty() || options.seen.contains(&release.uri) {
                continue;
            }
            options.seen.push(release.uri.clone());
            if !seeded
                || !release.artists.iter().any(|(uri, _)| {
                    options.is_enabled(uri)
                        && followed.is_none_or(|followed| followed.contains(uri))
                })
            {
                continue;
            }
            found.push(PlaylistChanges {
                id: format!("release:{}", release.uri),
                uri: release.uri.clone(),
                name: release.name.clone(),
                timestamp: now,
                added: Vec::new(),
                removed: Vec::new(),
                reordered: false,
                release: Some(ReleaseInfo {
                    artists: release
                        .artists
                        .iter()
                        .map(|(_, name)| name.clone())
                        .collect(),
                    kind: release.kind.clone(),
                    date: release.date.clone(),
                }),
            });
        }
        options.seeded = true;
        let excess = options.seen.len().saturating_sub(2000);
        options.seen.drain(..excess);
        for change in &found {
            self.history.insert(0, change.clone());
            self.outbox.push(change.clone());
        }
        self.history.truncate(200);
        found
    }

    /// Whether a scheduled check has work: a due playlist, the release
    /// feed or, when they are followed on their own, the followed
    /// playlists.
    #[must_use]
    pub fn check_due(&self, now: u64, preferences: SubscriptionPreferences) -> bool {
        !self.due(now, false).is_empty()
            || self.artists.due(now, self.interval_seconds, false)
            || (preferences.follow_playlists && self.followed_due(now))
    }

    /// The followed playlists are read again after the check interval.
    #[must_use]
    pub fn followed_due(&self, now: u64) -> bool {
        now.saturating_sub(self.followed_checked) >= self.interval_seconds.clamp(60, 604_800)
    }

    pub fn due(&self, now: u64, force: bool) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| {
                force
                    || now.saturating_sub(entry.last_checked)
                        >= self.interval_seconds.clamp(60, 604_800)
            })
            .map(|entry| entry.uri.clone())
            .collect()
    }
    pub fn record_local_edit(&mut self, edit: &crate::LibraryEdit) {
        use crate::LibraryEdit as E;
        let (uri, change) = match edit {
            E::AddToPlaylist { playlist, uris } => (
                playlist,
                LocalPlaylistChange {
                    added: uris.clone(),
                    ..LocalPlaylistChange::default()
                },
            ),
            E::RemoveFromPlaylist { playlist, uids } => (
                playlist,
                LocalPlaylistChange {
                    removed_uids: uids.clone(),
                    ..LocalPlaylistChange::default()
                },
            ),
            E::MoveInPlaylist { playlist, uid, .. } => (
                playlist,
                LocalPlaylistChange {
                    moved_uids: vec![uid.clone()],
                    ..LocalPlaylistChange::default()
                },
            ),
            _ => return,
        };
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.uri == *uri) {
            entry.local_changes.push(change);
        }
    }
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

/// Runs one subscription command for the active account.
///
/// # Errors
///
/// Returns a message key or the error when the data cannot be loaded or
/// saved.
pub async fn execute(
    store: &AccountStore,
    account: &str,
    api: &crate::api::Api,
    session: &librespot_core::Session,
    command: SubscriptionCommand,
    preferences: SubscriptionPreferences,
) -> Result<SubscriptionReport, String> {
    let mut document = load(store, account)?;
    let own_uri = format!("spotify:user:{}", session.username());
    let now = now();
    let mut errors = Vec::new();
    let mut checked = 0;
    let background = matches!(command, SubscriptionCommand::Check { force: false });
    let message = match command {
        SubscriptionCommand::Subscribe(uri) => {
            let snapshot = snapshot(api, session, &uri)
                .await
                .map_err(|e| e.to_string())?;
            if document.subscribe(snapshot, &own_uri, now)? {
                "spotify_sub_subscribed"
            } else {
                "spotify_sub_already"
            }
        }
        SubscriptionCommand::Unsubscribe(uri) => {
            document.unsubscribe(&uri);
            "spotify_sub_unsubscribed"
        }
        SubscriptionCommand::Options { uri, options } => {
            let entry = document
                .entries
                .iter_mut()
                .find(|entry| entry.uri == uri)
                .ok_or("spotify_sub_missing")?;
            entry.options = options;
            "spotify_sub_options_saved"
        }
        SubscriptionCommand::Interval(seconds) => {
            document.interval_seconds = seconds.clamp(60, 604_800);
            "spotify_sub_interval_saved"
        }
        SubscriptionCommand::Acknowledge(ids) => {
            document.outbox.retain(|change| !ids.contains(&change.id));
            ""
        }
        SubscriptionCommand::ArtistReleases {
            revision,
            all,
            included,
            excluded,
        } => {
            document
                .artists
                .apply_choice(revision, all, included, excluded);
            ""
        }
        SubscriptionCommand::Check { force } => {
            // With automatic checks off a scheduled check only hands over
            // waiting notifications (2.1.3).
            if force || preferences.automatic_checks {
                if preferences.follow_playlists && (force || document.followed_due(now)) {
                    follow_playlists(&mut document, api, session, &own_uri, now, &mut errors).await;
                }
                for uri in document.due(now, force) {
                    // An unchanged revision needs no full read (2.1.3).
                    if let Ok(current) = crate::catalog::playlist(api, session, &uri, 0).await
                        && document.note_unchanged(&uri, &current.revision, now)
                    {
                        checked += 1;
                        continue;
                    }
                    match snapshot(api, session, &uri).await {
                        Ok(snapshot) => {
                            match document.apply_snapshot(snapshot, &own_uri, now, false) {
                                Ok(_) => checked += 1,
                                Err(error) => {
                                    document.note_failed_check(&uri, now);
                                    errors.push(error);
                                }
                            }
                        }
                        Err(error) => {
                            document.note_failed_check(&uri, now);
                            errors.push(format!("{uri}: {error}"));
                        }
                    }
                }
                if check_releases(&mut document, api, session, now, force, &mut errors).await {
                    checked += 1;
                }
            }
            // A scheduled check works in the background and says nothing;
            // Check now reports that it finished.
            if force {
                "spotify_sub_check_finished"
            } else {
                ""
            }
        }
    }
    .to_owned();
    save(store, account, &document)?;
    Ok(SubscriptionReport {
        account: account.to_owned(),
        message,
        document,
        errors,
        checked,
        background,
    })
}

/// Subscribes the playlists the account follows and drops the automatic
/// subscriptions of playlists no longer followed (2.1.3). A library that
/// cannot be read changes nothing.
async fn follow_playlists(
    document: &mut SubscriptionDocument,
    api: &crate::api::Api,
    session: &librespot_core::Session,
    own_uri: &str,
    now: u64,
    errors: &mut Vec<String>,
) {
    document.followed_checked = now;
    let followed = match followed_playlists(api, session).await {
        Ok(followed) => followed,
        Err(error) => {
            errors.push(format!("spotify_sub_followed_failed: {error}"));
            return;
        }
    };
    for uri in document.sync_followed(&followed, own_uri) {
        match snapshot(api, session, &uri).await {
            Ok(snapshot) => {
                // Another owner than the library said is only skipped.
                let _ = document.subscribe_followed(snapshot, own_uri, now);
            }
            Err(error) => errors.push(format!("{uri}: {error}")),
        }
    }
}

/// The playlists in the account's library, from all folders, as URI and
/// owner URI.
async fn followed_playlists(
    api: &crate::api::Api,
    session: &librespot_core::Session,
) -> Result<Vec<(String, String)>, crate::api::ApiError> {
    Ok(crate::catalog::library_playlists(api, session)
        .await?
        .into_iter()
        .map(|item| (item.uri, item.artist_uri))
        .collect())
}

/// The release feed when it is due; `true` when it was read.
async fn check_releases(
    document: &mut SubscriptionDocument,
    api: &crate::api::Api,
    session: &librespot_core::Session,
    now: u64,
    force: bool,
    errors: &mut Vec<String>,
) -> bool {
    if !document.artists.due(now, document.interval_seconds, force) {
        return false;
    }
    match release_feed(api, session).await {
        Ok(feed) => {
            // Exceptions need the followed artists (review 5, B20); without
            // them every artist of a release counts, as before.
            let followed = if document.artists.all && !document.artists.excluded.is_empty() {
                followed_artists(api, session).await.ok()
            } else {
                None
            };
            document.apply_release_feed(&feed, now, followed.as_ref());
            true
        }
        Err(error) => {
            document.artists.last_checked = now;
            errors.push(format!("spotify_sub_releases_failed: {error}"));
            false
        }
    }
}

/// The account's followed artists, at most 2000.
async fn followed_artists(
    api: &crate::api::Api,
    session: &librespot_core::Session,
) -> Result<BTreeSet<String>, crate::api::ApiError> {
    let mut artists = BTreeSet::new();
    let mut offset = Some(0);
    while let Some(next) = offset.filter(|_| artists.len() < 2000) {
        let page = crate::catalog::library(api, session, crate::LibraryFilter::Artists, None, next)
            .await?;
        artists.extend(page.items.into_iter().map(|item| item.uri));
        offset = page.next_offset;
    }
    Ok(artists)
}

/// Spotify's feed of new releases of the account's followed artists (the
/// "What's new" list of the Spotify apps), newest first, at most 300.
///
/// # Errors
///
/// Returns the API error when the feed cannot be read.
pub async fn release_feed(
    api: &crate::api::Api,
    session: &librespot_core::Session,
) -> Result<Vec<FeedRelease>, crate::api::ApiError> {
    let mut releases = Vec::new();
    let mut offset = 0_u64;
    loop {
        let data = api
            .pathfinder(
                session,
                "queryWhatsNewFeed",
                serde_json::json!({
                    "offset": offset,
                    "limit": 50,
                    "onlyUnPlayedItems": false,
                    "includedContentTypes": ["ALBUM"],
                    "includeEpisodeContentRatingsV2": false,
                }),
            )
            .await?;
        let feed = data
            .get("whatsNewFeedItems")
            .ok_or_else(|| crate::api::ApiError::Shape("whatsNewFeedItems".into()))?;
        let items = feed
            .get("items")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| crate::api::ApiError::Shape("whatsNewFeedItems items".into()))?;
        releases.extend(items.iter().filter_map(parse_feed_release));
        let next = feed
            .pointer("/pagingInfo/nextOffset")
            .and_then(serde_json::Value::as_u64);
        match next {
            Some(next) if next > offset && next < 300 && !items.is_empty() => offset = next,
            _ => break,
        }
    }
    Ok(releases)
}

fn parse_feed_release(item: &serde_json::Value) -> Option<FeedRelease> {
    let album = item.pointer("/content/data")?;
    if album.get("__typename").and_then(serde_json::Value::as_str) != Some("Album") {
        return None;
    }
    let text = |pointer: &str| {
        album
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let uri = text("/uri");
    if !uri.starts_with("spotify:album:") {
        return None;
    }
    let artists = album
        .pointer("/artists/items")
        .and_then(serde_json::Value::as_array)
        .map(|artists| {
            artists
                .iter()
                .filter_map(|artist| {
                    let uri = artist.get("uri")?.as_str()?.to_owned();
                    let name = artist
                        .pointer("/profile/name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    Some((uri, name))
                })
                .collect()
        })
        .unwrap_or_default();
    let date = text("/date/isoString");
    Some(FeedRelease {
        uri,
        name: text("/name"),
        kind: text("/albumType"),
        date: date.split('T').next().unwrap_or_default().to_owned(),
        artists,
    })
}

/// Reads each page and rejects mixed revisions, missing pages and inconsistent totals.
///
/// # Errors
///
/// Returns the API error when a page is missing or inconsistent.
pub async fn snapshot(
    api: &crate::api::Api,
    session: &librespot_core::Session,
    uri: &str,
) -> Result<PlaylistSnapshot, crate::api::ApiError> {
    use crate::api::ApiError;
    if !uri.starts_with("spotify:playlist:") {
        return Err(ApiError::Shape("invalid playlist URI".into()));
    }
    let mut result: Option<PlaylistSnapshot> = None;
    let mut offset = 0_u64;
    let mut total_expected = None;
    loop {
        let data = api.pathfinder(session, "fetchPlaylist", serde_json::json!({"uri":uri,"offset":offset,"limit":crate::catalog::PAGE,"enableWatchFeedEntrypoint":false})).await?;
        let playlist = data
            .get("playlistV2")
            .filter(|v| v.get("__typename").and_then(serde_json::Value::as_str) == Some("Playlist"))
            .ok_or_else(|| ApiError::Shape("fetchPlaylist".into()))?;
        let text = |pointer| {
            playlist
                .pointer(pointer)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        let revision = text("/revisionId");
        let owner = text("/ownerV2/data/uri");
        if revision.is_empty() || owner.is_empty() {
            return Err(ApiError::Shape(
                "incomplete playlist revision or owner".into(),
            ));
        }
        let content = playlist
            .get("content")
            .ok_or_else(|| ApiError::Shape("missing playlist content".into()))?;
        let raw_items = content
            .get("items")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| ApiError::Shape("missing playlist items".into()))?;
        let page = crate::catalog::parse_page(content, offset);
        let total = page
            .total
            .ok_or_else(|| ApiError::Shape("missing playlist count".into()))?;
        if total > 100_000
            || page.items.len() != raw_items.len()
            || total_expected.is_some_and(|expected| expected != total)
        {
            return Err(ApiError::Shape("inconsistent playlist count".into()));
        }
        total_expected = Some(total);
        let snapshot = result.get_or_insert_with(|| PlaylistSnapshot {
            uri: uri.to_owned(),
            name: text("/name"),
            owner_uri: owner.clone(),
            revision: revision.clone(),
            tracks: Vec::new(),
        });
        if snapshot.revision != revision || snapshot.owner_uri != owner {
            return Err(ApiError::Shape(
                "playlist changed while it was read; try again".into(),
            ));
        }
        snapshot.tracks.extend(page.items);
        let read = snapshot.tracks.len() as u64;
        if read == total {
            break;
        }
        if raw_items.is_empty() || read > total || page.next_offset != Some(read) || read <= offset
        {
            return Err(ApiError::Shape("incomplete playlist page".into()));
        }
        offset = read;
    }
    // Confirm that the source did not change after the last page was fetched.
    if let Some(snapshot) = result {
        let check = crate::catalog::playlist(api, session, uri, 0).await?;
        if check.revision != snapshot.revision
            || check.page.total != Some(snapshot.tracks.len() as u64)
        {
            return Err(ApiError::Shape(
                "playlist changed while it was read; try again".into(),
            ));
        }
        Ok(snapshot)
    } else {
        Err(ApiError::Shape("empty playlist response".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(uri: &str) -> CatalogItem {
        serde_json::from_value(serde_json::json!({"kind":"Track","uri":uri,"name":uri,"subtitle":"","album":"","album_uri":"","artist_uri":"","duration_ms":null,"playable":true,"explicit":false,"uid":null,"saved":null,"count":null,"editable":false,"format":""})).unwrap()
    }
    fn list(owner: &str, tracks: &[&str], revision: &str) -> PlaylistSnapshot {
        PlaylistSnapshot {
            uri: "spotify:playlist:list".into(),
            name: "Mix".into(),
            owner_uri: owner.into(),
            revision: revision.into(),
            tracks: tracks.iter().map(|s| item(s)).collect(),
        }
    }
    fn release(uri: &str, artists: &[&str]) -> FeedRelease {
        FeedRelease {
            uri: uri.into(),
            name: uri.into(),
            kind: "SINGLE".into(),
            date: "2026-11-19".into(),
            artists: artists
                .iter()
                .map(|artist| ((*artist).to_owned(), format!("Name {artist}")))
                .collect(),
        }
    }

    /// RC 4: the first read only remembers the feed; later releases of
    /// turned-on artists become notifications, others are only remembered.
    #[test]
    fn new_releases_of_turned_on_artists_are_announced_once() {
        const T: u64 = 1_800_000_000;
        let mut doc = SubscriptionDocument::default();
        doc.artists.set_all(false);
        doc.artists.set("a", true);
        assert!(doc.artists.due(T, doc.interval_seconds, false));
        assert!(
            doc.apply_release_feed(&[release("old", &["a"])], T, None)
                .is_empty()
        );
        let found = doc.apply_release_feed(
            &[
                release("new2", &["b"]),
                release("new1", &["x", "a"]),
                release("old", &["a"]),
            ],
            T + 5000,
            None,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].uri, "new1");
        assert_eq!(found[0].id, "release:new1");
        assert_eq!(
            found[0].release.as_ref().unwrap().artists,
            ["Name x", "Name a"]
        );
        assert!(!found[0].is_empty());
        assert_eq!(doc.outbox.len(), 1);
        // Seen once, never again; an artist turned on later does not bring
        // back what was already in the feed.
        doc.artists.set("b", true);
        assert!(
            doc.apply_release_feed(&[release("new2", &["b"])], T + 9000, None)
                .is_empty()
        );
        assert!(!doc.artists.due(T + 9001, doc.interval_seconds, false));
    }

    /// 2.1.3: an account that never chose hears every followed artist, also
    /// one saved as off by 2.1.2; a choice of off is kept.
    #[test]
    fn releases_of_all_followed_artists_are_on_until_turned_off() {
        let root = tempfile::tempdir().unwrap();
        let store = AccountStore::new(root.path());
        let key = crate::accounts::account_key("a");
        let path = store.account_dir(&key).join("playlist_subscriptions.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // The shape 2.1.2 saved for an account that never chose.
        std::fs::write(
            &path,
            br#"{"version":1,"interval_seconds":3600,"entries":[],"history":[],"outbox":[],
            "artists":{"all":false,"included":[],"excluded":[],"revision":0,"seen":[],"seeded":false,"last_checked":0}}"#,
        )
        .unwrap();
        let mut document = load(&store, &key).unwrap();
        assert!(document.artists.all && !document.artists.seeded);
        assert!(document.artists.due(1_800_000_000, 3600, false));
        assert!(document.ignored_followed.is_empty());
        // Turned off in the settings, it stays off.
        let revision = next_artist_revision(document.artists.revision);
        document
            .artists
            .apply_choice(revision, false, BTreeSet::new(), BTreeSet::new());
        save(&store, &key, &document).unwrap();
        assert!(!load(&store, &key).unwrap().artists.any_enabled());
        assert!(SubscriptionDocument::default().artists.all);
    }

    /// 2.1.3: followed playlists of others are subscribed on their own, in
    /// portions; one unsubscribed by hand stays unsubscribed, and an
    /// automatic one goes when the playlist is unfollowed.
    #[test]
    fn followed_playlists_are_subscribed_on_their_own() {
        const ME: &str = "spotify:user:me";
        let followed = |uris: &[&str]| -> Vec<(String, String)> {
            uris.iter()
                .map(|uri| {
                    let owner = if uri.ends_with("mine") {
                        ME
                    } else {
                        "spotify:user:spotify"
                    };
                    ((*uri).to_owned(), owner.to_owned())
                })
                .collect()
        };
        let snapshot = |uri: &str| PlaylistSnapshot {
            uri: uri.into(),
            name: uri.into(),
            owner_uri: "spotify:user:spotify".into(),
            revision: "1".into(),
            tracks: vec![item("a")],
        };
        let mut doc = SubscriptionDocument::default();
        let all = followed(&[
            "spotify:playlist:radar",
            "spotify:playlist:mine",
            "spotify:playlist:mix",
            "spotify:album:not",
        ]);
        let wanted = doc.sync_followed(&all, ME);
        assert_eq!(wanted, ["spotify:playlist:radar", "spotify:playlist:mix"]);
        for uri in &wanted {
            doc.subscribe_followed(snapshot(uri), ME, 1).unwrap();
        }
        assert!(doc.entries.iter().all(|entry| entry.automatic));
        assert!(doc.sync_followed(&all, ME).is_empty());
        assert!(doc.outbox.is_empty(), "the first content is silent");
        // Unsubscribed by hand, it is not subscribed again.
        doc.unsubscribe("spotify:playlist:mix");
        assert!(doc.sync_followed(&all, ME).is_empty());
        // Subscribed by hand, it stays after unfollowing.
        doc.subscribe(snapshot("spotify:playlist:mix"), ME, 2)
            .unwrap();
        assert!(doc.ignored_followed.is_empty());
        let fewer = followed(&["spotify:playlist:other"]);
        assert_eq!(doc.sync_followed(&fewer, ME), ["spotify:playlist:other"]);
        assert_eq!(
            doc.entries
                .iter()
                .map(|entry| entry.uri.as_str())
                .collect::<Vec<_>>(),
            ["spotify:playlist:mix"]
        );
        // A large library is subscribed over several checks.
        let many: Vec<_> = (0..60)
            .map(|index| {
                (
                    format!("spotify:playlist:{index}"),
                    "spotify:user:x".to_owned(),
                )
            })
            .collect();
        assert_eq!(doc.sync_followed(&many, ME).len(), FOLLOWED_PER_CHECK);
    }

    /// 2.1.3: the followed playlists make a check due on their own, and an
    /// unchanged revision counts as checked.
    #[test]
    fn followed_playlists_make_a_check_due() {
        const T: u64 = 1_800_000_000;
        let mut doc = SubscriptionDocument::default();
        doc.artists.set_all(false);
        let on = SubscriptionPreferences::default();
        let off = SubscriptionPreferences {
            follow_playlists: false,
            ..on
        };
        assert!(doc.check_due(T, on));
        assert!(!doc.check_due(T, off));
        doc.followed_checked = T;
        assert!(!doc.check_due(T + 10, on));
        doc.subscribe(
            list("spotify:user:other", &["a"], "r1"),
            "spotify:user:me",
            T,
        )
        .unwrap();
        assert!(!doc.note_unchanged("spotify:playlist:list", "r2", T + 4000));
        assert!(doc.note_unchanged("spotify:playlist:list", "r1", T + 4000));
        assert!(doc.due(T + 4001, false).is_empty());
    }

    /// Review 5, B20: with all on and A turned off, A's release with B,
    /// whom the account does not follow, stays silent; with B followed and
    /// on, it is announced.
    #[test]
    fn a_turned_off_artist_silences_a_release_with_someone_not_followed() {
        const T: u64 = 1_800_000_000;
        let mut doc = SubscriptionDocument::default();
        doc.artists.set_all(true);
        doc.artists.set("a", false);
        doc.apply_release_feed(&[], T, None);
        let followed: BTreeSet<String> = ["a".to_owned()].into();
        assert!(
            doc.apply_release_feed(&[release("one", &["a", "b"])], T + 1, Some(&followed))
                .is_empty()
        );
        let followed: BTreeSet<String> = ["a".to_owned(), "b".to_owned()].into();
        let found = doc.apply_release_feed(&[release("two", &["a", "b"])], T + 2, Some(&followed));
        assert_eq!(found.len(), 1);
    }

    /// Review 5, B19: the last artist choice is kept, also when two windows
    /// read the same saved choice and their saves arrive in either order.
    #[test]
    fn the_last_artist_choice_wins_over_two_windows() {
        let mut saved = ArtistReleaseOptions::default();
        let read = saved.revision;
        // Two windows read the same choice; the first turns all on, the
        // second, later, all off; their saves may arrive in either order.
        let first = next_artist_revision(read);
        std::thread::sleep(std::time::Duration::from_millis(3));
        let second = next_artist_revision(read);
        assert!(second > first);
        saved.apply_choice(second, false, BTreeSet::new(), BTreeSet::new());
        saved.apply_choice(first, true, BTreeSet::new(), BTreeSet::new());
        assert!(!saved.all);
        // In one window quick changes keep their order.
        assert!(next_artist_revision(u64::MAX - 1) > u64::MAX - 1);
    }

    /// RC 4: on for all keeps exceptions; turning everything off and on
    /// again reads the feed quietly once more.
    #[test]
    fn artist_options_follow_all_and_exceptions() {
        let mut options = ArtistReleaseOptions::default();
        assert!(options.any_enabled());
        options.set_all(false);
        assert!(!options.any_enabled());
        options.set_all(true);
        assert!(options.is_enabled("anyone"));
        options.set("a", false);
        assert!(!options.is_enabled("a"));
        options.set("a", true);
        assert!(options.excluded.is_empty() && options.is_enabled("a"));
        options.seeded = true;
        options.set_all(false);
        assert!(!options.is_enabled("a") && !options.any_enabled());
        options.set("b", true);
        assert!(options.is_enabled("b") && !options.seeded);
        options.set("b", false);
        assert!(options.included.is_empty());
    }

    #[test]
    fn the_feed_item_shape_is_read() {
        let item = serde_json::json!({"content": {"__typename": "AlbumResponseWrapper", "data": {
            "__typename": "Album", "uri": "spotify:album:1", "name": "Far Away", "albumType": "SINGLE",
            "date": {"isoString": "2026-11-19T00:00:00Z"},
            "artists": {"items": [{"uri": "spotify:artist:a", "profile": {"name": "A"}}]}}}});
        let parsed = parse_feed_release(&item).unwrap();
        assert_eq!(parsed.uri, "spotify:album:1");
        assert_eq!(parsed.date, "2026-11-19");
        assert_eq!(
            parsed.artists,
            [("spotify:artist:a".to_owned(), "A".to_owned())]
        );
        assert!(
            parse_feed_release(
                &serde_json::json!({"content": {"data": {"__typename": "Episode"}}})
            )
            .is_none()
        );
    }

    /// Review 2.1: two own moves together are no change by someone else.
    #[test]
    fn several_own_moves_are_not_a_reorder() {
        let with_uids = |tracks: &[&str], revision: &str| {
            let mut snapshot = list("spotify:user:other", tracks, revision);
            for track in &mut snapshot.tracks {
                track.uid = Some(format!("u{}", track.uri));
            }
            snapshot
        };
        let mut doc = SubscriptionDocument::default();
        assert!(
            doc.subscribe(with_uids(&["a", "b", "c", "d"], "1"), "spotify:user:me", 1)
                .unwrap()
        );
        for uid in ["ua", "ub"] {
            doc.record_local_edit(&crate::LibraryEdit::MoveInPlaylist {
                playlist: "spotify:playlist:list".into(),
                uid: uid.into(),
                before: false,
                other_uid: String::new(),
            });
        }
        let changes = doc
            .apply_snapshot(
                with_uids(&["c", "a", "d", "b"], "2"),
                "spotify:user:me",
                2,
                true,
            )
            .unwrap();
        assert!(changes.is_none_or(|changes| !changes.reordered));
    }

    #[test]
    fn duplicate_occurrences_and_reordering_are_distinct_changes() {
        let a = list("spotify:user:other", &["a", "b", "a"], "1");
        let b = list("spotify:user:other", &["a", "a", "c"], "2");
        let changes = compare(&a, &b, 1);
        assert_eq!(
            changes
                .added
                .iter()
                .map(|t| t.uri.as_str())
                .collect::<Vec<_>>(),
            ["c"]
        );
        assert_eq!(
            changes
                .removed
                .iter()
                .map(|t| t.uri.as_str())
                .collect::<Vec<_>>(),
            ["b"]
        );
        assert!(!changes.reordered);
        assert!(compare(&a, &list("spotify:user:other", &["b", "a", "a"], "3"), 2).reordered);
    }
    #[test]
    fn first_baseline_is_silent_and_own_author_is_excluded_but_collaborator_is_allowed() {
        let mut doc = SubscriptionDocument::default();
        assert!(
            doc.subscribe(list("spotify:user:me", &["a"], "1"), "spotify:user:me", 1)
                .is_err()
        );
        assert!(
            doc.subscribe(
                list("spotify:user:other", &["a"], "1"),
                "spotify:user:me",
                1
            )
            .unwrap()
        );
        assert!(doc.outbox.is_empty());
        let changes = doc
            .apply_snapshot(
                list("spotify:user:other", &["a", "a"], "2"),
                "spotify:user:me",
                2,
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(changes.added.len(), 1);
        assert_eq!(doc.outbox.len(), 1);
        assert!(
            doc.apply_snapshot(
                list("spotify:user:other", &["a", "a", "b"], "3"),
                "spotify:user:me",
                3,
                true
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(doc.outbox.len(), 1);
    }
    #[test]
    fn account_files_are_isolated_and_bad_data_is_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let store = AccountStore::new(root.path());
        let a = crate::accounts::account_key("a");
        let b = crate::accounts::account_key("b");
        let mut doc = SubscriptionDocument::default();
        doc.subscribe(
            list("spotify:user:other", &["a"], "1"),
            "spotify:user:me",
            1,
        )
        .unwrap();
        save(&store, &a, &doc).unwrap();
        assert!(load(&store, &b).unwrap().entries.is_empty());
        let path = store.account_dir(&a).join("playlist_subscriptions.json");
        std::fs::write(&path, b"broken").unwrap();
        assert!(load(&store, &a).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"broken");
        assert!(load(&store, "../bad").is_err());
    }
    #[test]
    fn own_confirmed_addition_is_suppressed_without_hiding_other_additions() {
        let mut doc = SubscriptionDocument::default();
        doc.subscribe(
            list("spotify:user:other", &["a"], "1"),
            "spotify:user:me",
            1,
        )
        .unwrap();
        doc.record_local_edit(&crate::LibraryEdit::AddToPlaylist {
            playlist: "spotify:playlist:list".into(),
            uris: vec!["b".into()],
        });
        let change = doc
            .apply_snapshot(
                list("spotify:user:other", &["a", "b", "c"], "2"),
                "spotify:user:me",
                2,
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            change
                .added
                .iter()
                .map(|item| item.uri.as_str())
                .collect::<Vec<_>>(),
            ["c"]
        );
        let later = doc
            .apply_snapshot(
                list("spotify:user:other", &["a", "b", "c", "b"], "3"),
                "spotify:user:me",
                3,
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            later
                .added
                .iter()
                .map(|item| item.uri.as_str())
                .collect::<Vec<_>>(),
            ["b"]
        );
    }
}
