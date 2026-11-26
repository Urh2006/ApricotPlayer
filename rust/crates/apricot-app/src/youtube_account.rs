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

/// Remembers the account name, or forgets it with `None`.
///
/// # Errors
/// The write error.
pub fn save_account_name(app_data: &Path, name: Option<&str>) -> std::io::Result<()> {
    let path = app_data.join(ACCOUNT_FILE);
    match name {
        Some(name) => std::fs::write(
            path,
            serde_json::to_vec(&serde_json::json!({ "name": name }))?,
        ),
        None => match std::fs::remove_file(path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        },
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
    fn the_account_name_is_remembered_and_forgotten() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(load_account_name(folder.path()), None);
        save_account_name(folder.path(), Some("Urh")).unwrap();
        assert_eq!(load_account_name(folder.path()).as_deref(), Some("Urh"));
        save_account_name(folder.path(), None).unwrap();
        save_account_name(folder.path(), None).unwrap();
        assert_eq!(load_account_name(folder.path()), None);
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
