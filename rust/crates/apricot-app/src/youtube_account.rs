//! My `YouTube` (2.2.0): the lists of the signed-in `YouTube` account, read by
//! yt-dlp with the app's cookies, and the sign-in that fills those cookies.
//! `ApricotPlayer`'s own subscriptions, history, favorites and playlists stay
//! separate; the account's lists are read-only.

use std::path::{Path, PathBuf};

use apricot_core::TranslationCatalog;
use apricot_media::YoutubeAccountFeed;
use serde_json::Value;

/// The folder of `ApricotPlayer`'s own browser profile for the `YouTube`
/// sign-in, inside the app data folder.
pub const SIGN_IN_PROFILE_FOLDER: &str = "youtube-sign-in";
const ACCOUNT_FILE: &str = "youtube_account.json";

/// A row of My `YouTube`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MyYoutubeEntry {
    SignIn,
    Feed(YoutubeAccountFeed),
    ImportSubscriptions,
    SignOut,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MyYoutubeRow {
    pub entry: MyYoutubeEntry,
    pub label: String,
}

/// What My `YouTube` shows: before a sign-in only "Sign in with Google
/// account", after it the account's lists, the import and the sign-out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MyYoutubeModel {
    pub accessible_name: String,
    pub rows: Vec<MyYoutubeRow>,
}

impl MyYoutubeModel {
    #[must_use]
    pub fn build(catalog: &TranslationCatalog, signed_in: bool, account: Option<&str>) -> Self {
        let row = |entry, label: String| MyYoutubeRow { entry, label };
        let account = account.map(str::trim).filter(|name| !name.is_empty());
        if !signed_in {
            return Self {
                accessible_name: catalog.text("my_youtube").to_owned(),
                rows: vec![row(
                    MyYoutubeEntry::SignIn,
                    catalog.text("my_youtube_sign_in").to_owned(),
                )],
            };
        }
        let mut rows: Vec<MyYoutubeRow> = YoutubeAccountFeed::ALL
            .into_iter()
            .map(|feed| {
                row(
                    MyYoutubeEntry::Feed(feed),
                    catalog.text(feed.text_key()).to_owned(),
                )
            })
            .collect();
        rows.push(row(
            MyYoutubeEntry::ImportSubscriptions,
            catalog.text("my_youtube_import").to_owned(),
        ));
        rows.push(row(
            MyYoutubeEntry::SignOut,
            account.map_or_else(
                || catalog.text("my_youtube_sign_out").to_owned(),
                |name| {
                    catalog
                        .text("my_youtube_sign_out_named")
                        .replace("{name}", name)
                },
            ),
        ));
        Self {
            accessible_name: account.map_or_else(
                || catalog.text("my_youtube").to_owned(),
                |name| catalog.text("my_youtube_named").replace("{name}", name),
            ),
            rows,
        }
    }
}

/// `ApricotPlayer`'s own sign-in profile; the browser makes its `Default`
/// profile inside, which cookie exports then read like any other profile.
#[must_use]
pub fn sign_in_user_data_dir(app_data: &Path) -> PathBuf {
    app_data.join(SIGN_IN_PROFILE_FOLDER)
}

#[must_use]
pub fn sign_in_profile(app_data: &Path) -> PathBuf {
    sign_in_user_data_dir(app_data).join("Default")
}

/// Whether the cookies of the sign-in window already hold `YouTube`'s own
/// sign-in, not only Google's.
#[must_use]
pub fn devtools_cookies_signed_in(cookies: &[Value]) -> bool {
    cookies.iter().any(|cookie| {
        let text = |key: &str| cookie.get(key).and_then(Value::as_str).unwrap_or_default();
        let domain = text("domain").trim_start_matches('.').to_ascii_lowercase();
        let youtube = domain == "youtube.com" || domain.ends_with(".youtube.com");
        youtube
            && matches!(
                text("name"),
                "LOGIN_INFO" | "SAPISID" | "__Secure-3PAPISID" | "__Secure-1PSID"
            )
            && !text("value").is_empty()
    })
}

/// The account name remembered after the last sign-in.
#[must_use]
pub fn load_account_name(app_data: &Path) -> Option<String> {
    let text = std::fs::read_to_string(app_data.join(ACCOUNT_FILE)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// Whether a sign-in through My `YouTube` was confirmed by `YouTube`. Cookies
/// that only carry sign-in names (for example an old `cookies.txt`) are not
/// a sign-in (Urh, 2.2.0).
/// The confirmation belongs to the cookies it was made with: other cookies
/// (another account imported in Settings, Cookies) are not confirmed.
#[must_use]
pub fn account_confirmed(app_data: &Path, fingerprint: Option<&str>) -> bool {
    let Some(fingerprint) = fingerprint else {
        return false;
    };
    std::fs::read_to_string(app_data.join(ACCOUNT_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|value| {
            value.get("signed_in").and_then(Value::as_bool) == Some(true)
                && value.get("cookie").and_then(Value::as_str) == Some(fingerprint)
        })
}

/// A short digest of the `YouTube` session in a `cookies.txt` (its
/// `SAPISID`), so a confirmation stays tied to the cookies that made it.
#[must_use]
pub fn sign_in_fingerprint(cookies_text: &str) -> Option<String> {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    let session = cookies_text.lines().find_map(|line| {
        let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        let fields: Vec<&str> = line.split('\t').collect();
        let [domain, _, _, _, _, name, value] = fields.as_slice() else {
            return None;
        };
        let domain = domain.trim_start_matches('.').to_ascii_lowercase();
        let youtube = domain == "youtube.com" || domain.ends_with(".youtube.com");
        (youtube && matches!(*name, "SAPISID" | "__Secure-3PAPISID") && !value.is_empty())
            .then_some(*value)
    })?;
    let digest = sha2::Sha256::digest(session.as_bytes());
    Some(
        digest
            .iter()
            .take(12)
            .fold(String::new(), |mut text, byte| {
                let _ = write!(text, "{byte:02x}");
                text
            }),
    )
}

/// Records the confirmed sign-in with its name, when `YouTube` gave one,
/// and the cookies it belongs to.
///
/// # Errors
/// The write error.
pub fn save_account(app_data: &Path, name: Option<&str>, fingerprint: &str) -> std::io::Result<()> {
    std::fs::write(
        app_data.join(ACCOUNT_FILE),
        serde_json::to_vec(&serde_json::json!({
            "signed_in": true,
            "name": name,
            "cookie": fingerprint,
        }))?,
    )
}

/// Forgets the sign-in.
///
/// # Errors
/// The delete error.
pub fn forget_account(app_data: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(app_data.join(ACCOUNT_FILE)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// The channels of the account that are not in `ApricotPlayer`'s
/// subscriptions yet, in the account's order.
#[must_use]
pub fn channels_to_import(
    channels: Vec<apricot_core::MediaItem>,
    subscribed: impl Fn(&apricot_core::MediaItem) -> bool,
) -> Vec<apricot_core::MediaItem> {
    let mut seen = std::collections::HashSet::new();
    channels
        .into_iter()
        .filter(|item| item.kind == apricot_core::MediaKind::Channel)
        .filter(|item| seen.insert(item.id.0.clone()))
        .filter(|item| !subscribed(item))
        .collect()
}

/// The loading text of a `YouTube` list: My `YouTube` lists say only
/// "Loading.", since the user has just heard the list's name (Urh, 2.2.0);
/// playlists and channels keep Python's `loading_playlist`.
#[must_use]
pub fn collection_loading_text(
    catalog: &TranslationCatalog,
    kind: apricot_media::YoutubeCollectionKind,
    title: &str,
) -> String {
    if matches!(kind, apricot_media::YoutubeCollectionKind::Account(_)) {
        catalog.text("my_youtube_loading").to_owned()
    } else {
        catalog.text("loading_playlist").replace("{title}", title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::english_catalog;

    #[test]
    fn my_youtube_offers_the_sign_in_first_and_then_the_lists() {
        let catalog = english_catalog();
        let signed_out = MyYoutubeModel::build(&catalog, false, None);
        assert_eq!(signed_out.accessible_name, "My YouTube");
        assert_eq!(
            signed_out.rows,
            [MyYoutubeRow {
                entry: MyYoutubeEntry::SignIn,
                label: "Sign in with Google account".to_owned()
            }]
        );
        let signed_in = MyYoutubeModel::build(&catalog, true, Some("Urh"));
        assert_eq!(signed_in.accessible_name, "My YouTube, signed in as Urh");
        let labels: Vec<_> = signed_in
            .rows
            .iter()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "Home, recommended for you",
                "Subscriptions, new videos",
                "Subscribed channels",
                "My playlists",
                "Watch later",
                "Liked videos",
                "YouTube history",
                "Import YouTube subscriptions",
                "Sign out of YouTube (Urh)"
            ]
        );
        assert_eq!(
            MyYoutubeModel::build(&catalog, true, None)
                .rows
                .last()
                .map(|row| row.label.as_str()),
            Some("Sign out of YouTube")
        );
    }

    #[test]
    fn only_youtubes_own_sign_in_ends_the_sign_in_window() {
        let google =
            serde_json::json!([{ "name": "SAPISID", "domain": ".google.com", "value": "x" }]);
        assert!(!devtools_cookies_signed_in(google.as_array().unwrap()));
        let youtube = serde_json::json!([
            { "name": "VISITOR_INFO1_LIVE", "domain": ".youtube.com", "value": "x" },
            { "name": "LOGIN_INFO", "domain": ".youtube.com", "value": "y" }
        ]);
        assert!(devtools_cookies_signed_in(youtube.as_array().unwrap()));
    }

    #[test]
    fn only_a_confirmed_sign_in_counts_and_it_is_forgotten() {
        let folder = tempfile::tempdir().unwrap();
        let cookies = ".youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tsession-a\n";
        let a = sign_in_fingerprint(cookies).unwrap();
        let b = sign_in_fingerprint(&cookies.replace("session-a", "session-b")).unwrap();
        assert_ne!(a, b);
        assert_eq!(
            sign_in_fingerprint(".google.com\tTRUE\t/\tTRUE\t0\tSAPISID\tx\n"),
            None
        );
        assert!(!account_confirmed(folder.path(), Some(&a)));
        assert_eq!(load_account_name(folder.path()), None);
        save_account(folder.path(), Some("Urh"), &a).unwrap();
        assert!(account_confirmed(folder.path(), Some(&a)));
        // Other cookies (another account) are not confirmed.
        assert!(!account_confirmed(folder.path(), Some(&b)));
        assert!(!account_confirmed(folder.path(), None));
        assert_eq!(load_account_name(folder.path()).as_deref(), Some("Urh"));
        save_account(folder.path(), None, &a).unwrap();
        assert!(account_confirmed(folder.path(), Some(&a)));
        assert_eq!(load_account_name(folder.path()), None);
        forget_account(folder.path()).unwrap();
        forget_account(folder.path()).unwrap();
        assert!(!account_confirmed(folder.path(), Some(&a)));
        // 2.2.0 test builds wrote only the name.
        std::fs::write(folder.path().join(ACCOUNT_FILE), r#"{"name":"Urh"}"#).unwrap();
        assert!(!account_confirmed(folder.path(), Some(&a)));
    }

    #[test]
    fn the_import_adds_each_new_channel_once() {
        let channel = |id: &str| apricot_core::MediaItem {
            id: apricot_core::MediaId(id.to_owned()),
            source: apricot_core::MediaSource::Youtube,
            kind: apricot_core::MediaKind::Channel,
            title: id.to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: std::collections::BTreeMap::new(),
        };
        let mut video = channel("video");
        video.kind = apricot_core::MediaKind::Video;
        let new = channels_to_import(
            vec![
                channel("a"),
                channel("b"),
                channel("a"),
                video,
                channel("c"),
            ],
            |item| item.id.0 == "b",
        );
        let ids: Vec<_> = new.iter().map(|item| item.id.0.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
    }
}
