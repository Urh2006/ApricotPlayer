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
}
impl PlaylistChanges {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && !self.reordered
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubscriptionDocument {
    pub version: u32,
    pub interval_seconds: u64,
    pub entries: Vec<PlaylistSubscription>,
    pub history: Vec<PlaylistChanges>,
    pub outbox: Vec<PlaylistChanges>,
}
impl Default for SubscriptionDocument {
    fn default() -> Self {
        Self {
            version: 1,
            interval_seconds: 3600,
            entries: Vec::new(),
            history: Vec::new(),
            outbox: Vec::new(),
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
    let document: SubscriptionDocument =
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
        if self.entries.iter().any(|entry| entry.uri == snapshot.uri) {
            return Ok(false);
        }
        self.entries.push(PlaylistSubscription {
            uri: snapshot.uri.clone(),
            name: snapshot.name.clone(),
            options: NotificationOptions::default(),
            baseline: snapshot,
            last_checked: now,
            local_changes: Vec::new(),
        });
        Ok(true)
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
            document.entries.retain(|entry| entry.uri != uri);
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
        SubscriptionCommand::Check { force } => {
            for uri in document.due(now, force) {
                match snapshot(api, session, &uri).await {
                    Ok(snapshot) => match document.apply_snapshot(snapshot, &own_uri, now, false) {
                        Ok(_) => checked += 1,
                        Err(error) => {
                            document.note_failed_check(&uri, now);
                            errors.push(error);
                        }
                    },
                    Err(error) => {
                        document.note_failed_check(&uri, now);
                        errors.push(format!("{uri}: {error}"));
                    }
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
