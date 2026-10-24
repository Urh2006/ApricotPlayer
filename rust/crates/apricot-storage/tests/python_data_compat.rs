use std::{collections::BTreeMap, env, path::PathBuf};

use apricot_storage::{
    CompatibilitySnapshot, DEFAULT_MAX_ARTIFACT_BYTES, NotificationFile, RssFeedFile,
    SubscriptionFile, UserPlaylistFile,
};
use tempfile::tempdir;

#[test]
#[ignore = "run through qualify_python_data_compat.ps1 with a private temporary copy"]
fn real_python_data_round_trips_without_value_loss() {
    let source = PathBuf::from(
        env::var_os("APRICOT_PYTHON_APP_DATA")
            .expect("APRICOT_PYTHON_APP_DATA must point to a private temporary copy"),
    );
    let first = CompatibilitySnapshot::load(&source, DEFAULT_MAX_ARTIFACT_BYTES)
        .expect("load Python app data");
    assert!(!first.artifacts.is_empty(), "no compatibility data found");

    let target = tempdir().expect("round-trip directory");
    first.write_to(target.path()).expect("write Rust snapshot");
    let second = CompatibilitySnapshot::load(target.path(), DEFAULT_MAX_ARTIFACT_BYTES)
        .expect("reload Rust snapshot");

    let first_values: BTreeMap<_, _> = first
        .artifacts
        .iter()
        .map(|artifact| (artifact.id, &artifact.value))
        .collect();
    let second_values: BTreeMap<_, _> = second
        .artifacts
        .iter()
        .map(|artifact| (artifact.id, &artifact.value))
        .collect();
    assert_eq!(first_values, second_values);

    let source_playlists = source.join("playlists.json");
    if source_playlists.is_file() {
        let original: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&source_playlists).expect("read Python playlists"),
        )
        .expect("parse Python playlists");
        let playlists = UserPlaylistFile::new(&source_playlists)
            .load()
            .expect("load Python playlists through typed adapter");
        let playlist_target = target.path().join("typed-playlists.json");
        UserPlaylistFile::new(&playlist_target)
            .save(&playlists)
            .expect("write typed playlists");
        let restored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(playlist_target).expect("read typed playlists"))
                .expect("parse typed playlists");
        assert_json_equivalent(&original, &restored, "playlists");
    }

    let source_notifications = source.join("notifications.json");
    if source_notifications.is_file() {
        let original: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&source_notifications).expect("read Python notifications"),
        )
        .expect("parse Python notifications");
        let notifications = NotificationFile::new(&source_notifications)
            .load()
            .expect("load Python notifications through typed adapter");
        let notification_target = target.path().join("typed-notifications.json");
        NotificationFile::new(&notification_target)
            .save(&notifications)
            .expect("write typed notifications");
        let restored: serde_json::Value = serde_json::from_slice(
            &std::fs::read(notification_target).expect("read typed notifications"),
        )
        .expect("parse typed notifications");
        assert_json_equivalent(&original, &restored, "notifications");
    }

    let source_subscriptions = source.join("subscriptions.json");
    if source_subscriptions.is_file() {
        let original: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&source_subscriptions).expect("read Python subscriptions"),
        )
        .expect("parse Python subscriptions");
        let subscriptions = SubscriptionFile::new(&source_subscriptions)
            .load()
            .expect("load Python subscriptions through typed adapter");
        let subscription_target = target.path().join("typed-subscriptions.json");
        SubscriptionFile::new(&subscription_target)
            .save(&subscriptions)
            .expect("write typed subscriptions");
        let restored: serde_json::Value = serde_json::from_slice(
            &std::fs::read(subscription_target).expect("read typed subscriptions"),
        )
        .expect("parse typed subscriptions");
        assert_json_equivalent(&original, &restored, "subscriptions");
    }

    let source_rss_feeds = source.join("rss_feeds.json");
    if source_rss_feeds.is_file() {
        let original: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&source_rss_feeds).expect("read Python RSS feeds"),
        )
        .expect("parse Python RSS feeds");
        let feeds = RssFeedFile::new(&source_rss_feeds)
            .load()
            .expect("load Python RSS feeds through typed adapter");
        let rss_target = target.path().join("typed-rss-feeds.json");
        RssFeedFile::new(&rss_target)
            .save(&feeds)
            .expect("write typed RSS feeds");
        let restored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(rss_target).expect("read typed RSS feeds"))
                .expect("parse typed RSS feeds");
        assert_json_equivalent(&original, &restored, "rss_feeds");
    }
}

fn assert_json_equivalent(left: &serde_json::Value, right: &serde_json::Value, path: &str) {
    match (left, right) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            assert_eq!(left.as_f64(), right.as_f64(), "number changed at {path}");
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            assert_eq!(left.len(), right.len(), "array length changed at {path}");
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                assert_json_equivalent(left, right, &format!("{path}[{index}]"));
            }
        }
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            assert_eq!(left.len(), right.len(), "object keys changed at {path}");
            for (key, left) in left {
                let right = right
                    .get(key)
                    .unwrap_or_else(|| panic!("missing key {path}.{key}"));
                assert_json_equivalent(left, right, &format!("{path}.{key}"));
            }
        }
        _ => assert_eq!(left, right, "JSON value changed at {path}"),
    }
}
