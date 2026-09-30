//! The beta's first start on real Python data: every Python file is read
//! through the same controllers `apricot-player` configures, and nothing is
//! dropped. Run with `APRICOT_PYTHON_APP_DATA` pointing at a private copy.

use std::{env, fs, path::Path};

use apricot_app::{Application, MainMenuAvailability, SettingsController};
use apricot_storage::{
    BookmarkFile, LastPlayerSessionFile, MediaListFile, NotificationFile, PlaybackPositionFile,
    PlaybackQueueFile, RssFeedFile, SettingsDocument, SettingsPaths, SubscriptionFile,
    UserPlaylistFile,
};
use serde_json::Value;

fn json(folder: &Path, name: &str) -> Option<Value> {
    let data = fs::read(folder.join(name)).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Python lists are top-level arrays, or arrays under one known key.
fn count(value: Option<&Value>, key: &str) -> Option<usize> {
    let value = value?;
    value
        .as_array()
        .or_else(|| value.get(key).and_then(Value::as_array))
        .map(Vec::len)
}

#[test]
#[ignore = "needs a private copy of real Python app data in APRICOT_PYTHON_APP_DATA"]
#[allow(clippy::too_many_lines)]
fn first_start_imports_every_python_file() {
    let legacy = env::var_os("APRICOT_PYTHON_APP_DATA").expect("APRICOT_PYTHON_APP_DATA");
    let legacy = Path::new(&legacy);
    let beta = tempfile::tempdir().expect("beta data");
    let app = beta.path();
    let defaults = SettingsDocument::with_platform_defaults("downloads", "cache", "beta");
    let settings = SettingsController::load(SettingsPaths::for_app_data(app, legacy), defaults);
    assert!(
        settings.load_errors().is_empty(),
        "{:?}",
        settings.load_errors()
    );
    let python_settings = json(legacy, "settings.json").expect("Python settings");
    let imported = serde_json::to_value(settings.current()).expect("settings value");
    let mut differing = Vec::new();
    for (key, value) in python_settings.as_object().expect("settings object") {
        if let Some(rust) = imported.get(key)
            && rust != value
            && !(rust.is_number() && value.is_number() && rust.as_f64() == value.as_f64())
        {
            differing.push(key.clone());
        }
    }
    println!("settings keys whose value Rust normalized: {differing:?}");
    let mut application = Application::new(settings, MainMenuAvailability::default());
    application.configure_playback_queue(
        PlaybackQueueFile::new(app.join("playback_queue.json")),
        &PlaybackQueueFile::new(legacy.join("playback_queue.json")),
    );
    application.configure_media_collections(
        MediaListFile::new(app.join("favorites.json")),
        &MediaListFile::new(legacy.join("favorites.json")),
        MediaListFile::new(app.join("history.json")),
        &MediaListFile::new(legacy.join("history.json")),
    );
    application.configure_notifications(
        NotificationFile::new(app.join("notifications.json")),
        &NotificationFile::new(legacy.join("notifications.json")),
    );
    application.configure_subscriptions(
        SubscriptionFile::new(app.join("subscriptions.json")),
        &SubscriptionFile::new(legacy.join("subscriptions.json")),
    );
    application.configure_rss_feeds(
        RssFeedFile::new(app.join("rss_feeds.json")),
        &RssFeedFile::new(legacy.join("rss_feeds.json")),
    );
    application.configure_user_playlists(
        UserPlaylistFile::new(app.join("playlists.json")),
        &UserPlaylistFile::new(legacy.join("playlists.json")),
    );
    application.configure_bookmarks(
        BookmarkFile::new(app.join("bookmarks.json")),
        &BookmarkFile::new(legacy.join("bookmarks.json")),
        0.0,
    );
    application.configure_playback_positions(
        PlaybackPositionFile::new(app.join("playback_positions.json")),
        &PlaybackPositionFile::new(legacy.join("playback_positions.json")),
    );
    application.configure_last_player_session(
        LastPlayerSessionFile::new(app.join("last_player_session.json")),
        &LastPlayerSessionFile::new(legacy.join("last_player_session.json")),
    );

    let report = [
        (
            "favorites",
            count(json(legacy, "favorites.json").as_ref(), "items"),
            application.favorites().len(),
        ),
        (
            "history",
            count(json(legacy, "history.json").as_ref(), "items"),
            application.history().len(),
        ),
        (
            "notifications",
            count(json(legacy, "notifications.json").as_ref(), "notifications"),
            application.notifications().len(),
        ),
        (
            "subscriptions",
            count(json(legacy, "subscriptions.json").as_ref(), "subscriptions"),
            application.subscriptions().len(),
        ),
        (
            "rss feeds",
            count(json(legacy, "rss_feeds.json").as_ref(), "feeds"),
            application.rss_feeds().len(),
        ),
        (
            "playlists",
            count(json(legacy, "playlists.json").as_ref(), "playlists"),
            application.user_playlists().len(),
        ),
        (
            "bookmarks",
            count(json(legacy, "bookmarks.json").as_ref(), "bookmarks"),
            application.bookmarks().len(),
        ),
        (
            "playback queue",
            count(json(legacy, "playback_queue.json").as_ref(), "items"),
            application.playback_queue().items().len(),
        ),
    ];
    for (name, python, rust) in &report {
        println!("{name}: Python {python:?}, Rust {rust}");
    }
    for (name, python, rust) in report {
        if let Some(python) = python {
            assert_eq!(python, rust, "{name}");
        }
    }
    let python_items = json(legacy, "playlists.json")
        .and_then(|value| value.as_array().cloned())
        .map(|playlists| {
            playlists
                .iter()
                .map(|playlist| count(Some(playlist), "items").unwrap_or(0))
                .sum::<usize>()
        });
    let rust_items = application
        .user_playlists()
        .iter()
        .map(|playlist| playlist.items.len())
        .sum::<usize>();
    println!("playlist items: Python {python_items:?}, Rust {rust_items}");
    if let Some(python_items) = python_items {
        assert_eq!(python_items, rust_items);
    }
    let python_episodes = json(legacy, "rss_feeds.json")
        .and_then(|value| value.as_array().cloned())
        .map(|feeds| {
            feeds
                .iter()
                .map(|feed| count(Some(feed), "items").unwrap_or(0))
                .sum::<usize>()
        });
    let rust_episodes = application
        .rss_feeds()
        .iter()
        .map(|feed| feed.items.len())
        .sum::<usize>();
    println!("rss episodes: Python {python_episodes:?}, Rust {rust_episodes}");
    println!(
        "last session: {}",
        application.last_player_session().is_some()
    );
    assert!(application.last_player_session_load_error().is_none());
    assert!(application.playback_queue_load_error().is_none());
}
