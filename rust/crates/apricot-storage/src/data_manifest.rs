//! Machine-readable compatibility contract for Python app-data artifacts.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataClass {
    DurableUserData,
    Secret,
    RebuildableCache,
    RuntimeComponent,
    Diagnostic,
    EphemeralSignal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataShape {
    JsonObject,
    JsonArray,
    Text,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryPolicy {
    AtomicWithBackup,
    Atomic,
    PreserveOnCorruption,
    Rebuildable,
    ReplaceTransactionally,
    RotateBounded,
    Discardable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataEntry {
    pub id: &'static str,
    pub relative_path: &'static str,
    pub class: DataClass,
    pub shape: DataShape,
    pub recovery: RecoveryPolicy,
    pub legacy_path_from_app_data_parent: Option<&'static str>,
}

impl DataEntry {
    pub fn path_in(&self, app_data_directory: &Path) -> PathBuf {
        app_data_directory.join(self.relative_path)
    }

    pub fn legacy_path_in(&self, app_data_parent: &Path) -> Option<PathBuf> {
        self.legacy_path_from_app_data_parent
            .map(|path| app_data_parent.join(path))
    }

    pub fn is_durable(&self) -> bool {
        matches!(self.class, DataClass::DurableUserData | DataClass::Secret)
    }
}

pub const DATA_ENTRIES: &[DataEntry] = &[
    DataEntry {
        id: "settings",
        relative_path: "settings.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonObject,
        recovery: RecoveryPolicy::AtomicWithBackup,
        legacy_path_from_app_data_parent: Some("UrhasaurusYouTubePlayer/settings.json"),
    },
    DataEntry {
        id: "favorites",
        relative_path: "favorites.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonArray,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: Some("UrhasaurusYouTubePlayer/favorites.json"),
    },
    DataEntry {
        id: "bookmarks",
        relative_path: "bookmarks.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonArray,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "history",
        relative_path: "history.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonArray,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "subscriptions",
        relative_path: "subscriptions.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonArray,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "rss_feeds",
        relative_path: "rss_feeds.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonArray,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "playlists",
        relative_path: "playlists.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonArray,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "notifications",
        relative_path: "notifications.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonArray,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "playback_positions",
        relative_path: "playback_positions.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonObject,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "playback_queue",
        relative_path: "playback_queue.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonArray,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "last_player_session",
        relative_path: "last_player_session.json",
        class: DataClass::DurableUserData,
        shape: DataShape::JsonObject,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "stream_url_cache",
        relative_path: "stream_url_cache.json",
        class: DataClass::RebuildableCache,
        shape: DataShape::JsonObject,
        recovery: RecoveryPolicy::Rebuildable,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "cached_cookies",
        relative_path: "cookies.txt",
        class: DataClass::Secret,
        shape: DataShape::Text,
        recovery: RecoveryPolicy::PreserveOnCorruption,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "download_archive",
        relative_path: "download-archive.txt",
        class: DataClass::DurableUserData,
        shape: DataShape::Text,
        recovery: RecoveryPolicy::Atomic,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "cache",
        relative_path: "cache",
        class: DataClass::RebuildableCache,
        shape: DataShape::Directory,
        recovery: RecoveryPolicy::Rebuildable,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "components",
        relative_path: "components",
        class: DataClass::RuntimeComponent,
        shape: DataShape::Directory,
        recovery: RecoveryPolicy::ReplaceTransactionally,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "updater_log",
        relative_path: "updater.log",
        class: DataClass::Diagnostic,
        shape: DataShape::Text,
        recovery: RecoveryPolicy::RotateBounded,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "mpv_log",
        relative_path: "mpv.log",
        class: DataClass::Diagnostic,
        shape: DataShape::Text,
        recovery: RecoveryPolicy::RotateBounded,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "update_relaunch",
        relative_path: "updated-relaunch.json",
        class: DataClass::EphemeralSignal,
        shape: DataShape::JsonObject,
        recovery: RecoveryPolicy::Discardable,
        legacy_path_from_app_data_parent: None,
    },
    DataEntry {
        id: "activation_signal",
        relative_path: "activate.json",
        class: DataClass::EphemeralSignal,
        shape: DataShape::JsonObject,
        recovery: RecoveryPolicy::Discardable,
        legacy_path_from_app_data_parent: None,
    },
];

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, path::Path};

    use super::{DATA_ENTRIES, DataClass, DataShape, RecoveryPolicy};

    #[test]
    fn python_app_data_manifest_has_20_unique_entries() {
        let ids: HashSet<_> = DATA_ENTRIES.iter().map(|entry| entry.id).collect();
        let paths: HashSet<_> = DATA_ENTRIES
            .iter()
            .map(|entry| entry.relative_path)
            .collect();
        assert_eq!(DATA_ENTRIES.len(), 20);
        assert_eq!(ids.len(), DATA_ENTRIES.len());
        assert_eq!(paths.len(), DATA_ENTRIES.len());
    }

    #[test]
    fn durable_data_is_never_marked_rebuildable_or_discardable() {
        for entry in DATA_ENTRIES.iter().filter(|entry| entry.is_durable()) {
            assert!(!matches!(
                entry.recovery,
                RecoveryPolicy::Rebuildable | RecoveryPolicy::Discardable
            ));
        }
    }

    #[test]
    fn secret_and_runtime_artifacts_have_expected_shapes() {
        let cookies = DATA_ENTRIES
            .iter()
            .find(|entry| entry.id == "cached_cookies")
            .expect("cookies");
        assert_eq!(cookies.class, DataClass::Secret);
        assert_eq!(cookies.shape, DataShape::Text);

        let components = DATA_ENTRIES
            .iter()
            .find(|entry| entry.id == "components")
            .expect("components");
        assert_eq!(components.class, DataClass::RuntimeComponent);
        assert_eq!(components.shape, DataShape::Directory);
    }

    #[test]
    fn legacy_paths_are_resolved_beside_the_current_app_directory() {
        let settings = DATA_ENTRIES
            .iter()
            .find(|entry| entry.id == "settings")
            .expect("settings");
        let parent = Path::new("app-data-parent");
        assert_eq!(
            settings.legacy_path_in(parent),
            Some(parent.join("UrhasaurusYouTubePlayer/settings.json"))
        );
    }
}
