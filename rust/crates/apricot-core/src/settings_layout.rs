//! Canonical Settings sections and reset ownership inherited from Python 1.0.21.

use crate::SettingId;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SettingsSection {
    General,
    MainMenu,
    Playback,
    Equalizer,
    Downloads,
    Library,
    Podcasts,
    Notifications,
    Cookies,
    Audiovault,
    /// Rust only (Urh, 2026-10-02): the Spotify settings, which live in
    /// `spotify/settings.json` and not in `settings.json`.
    Spotify,
    Shortcuts,
}

impl SettingsSection {
    pub const ALL: &[Self] = &[
        Self::General,
        Self::MainMenu,
        Self::Playback,
        Self::Equalizer,
        Self::Downloads,
        Self::Library,
        Self::Podcasts,
        Self::Notifications,
        Self::Cookies,
        Self::Audiovault,
        Self::Spotify,
        Self::Shortcuts,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::MainMenu => "main_menu",
            Self::Playback => "playback",
            Self::Equalizer => "equalizer",
            Self::Downloads => "downloads",
            Self::Library => "library",
            Self::Podcasts => "podcasts",
            Self::Notifications => "notifications",
            Self::Cookies => "cookies",
            Self::Audiovault => "audiovault",
            Self::Spotify => "spotify",
            Self::Shortcuts => "shortcuts",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsSectionDefinition {
    pub section: SettingsSection,
    pub label_key: &'static str,
    pub reset_fields: &'static [SettingId],
}

const GENERAL: &[SettingId] = &[
    SettingId::Language,
    SettingId::DownloadFolder,
    SettingId::ResultsLimit,
    SettingId::DirectLinkEnterAction,
    SettingId::ShowShortcutsInLabels,
    SettingId::AutoUpdateYtdlp,
    SettingId::AutoUpdateApp,
    SettingId::AppUpdateIntervalHours,
    SettingId::AppUpdateNotifications,
    SettingId::CloseToTray,
    SettingId::StartWithWindows,
    SettingId::TrayNotification,
    SettingId::SkippedUpdateVersion,
    SettingId::UpdateChannel,
];

const MAIN_MENU: &[SettingId] = &[SettingId::MainMenuHiddenActions];

const PLAYBACK: &[SettingId] = &[
    SettingId::AutoplayNext,
    SettingId::AutoplayRelated,
    SettingId::PreferBrowserPlayback,
    SettingId::PlayerFullscreen,
    SettingId::PlayerStartPaused,
    SettingId::AnnouncePlayPause,
    SettingId::AnnouncePlaybackFinished,
    SettingId::EnableBackgroundPlayback,
    SettingId::PlayerSpeed,
    SettingId::SpeedAudioMode,
    SettingId::ShowVideoDetailsByDefault,
    SettingId::EnableAgeRestrictedVideos,
    SettingId::EnableStreamCache,
    SettingId::EnableStreamUrlCache,
    SettingId::StreamUrlCacheMinutes,
    SettingId::StreamFormatPreference,
    SettingId::PrefetchNextStreamUrl,
    SettingId::GaplessPlayback,
    SettingId::ReplaygainMode,
    SettingId::EnableOnlineLyrics,
    SettingId::CacheFolder,
    SettingId::CacheSizeMb,
    SettingId::ResumePlayback,
    SettingId::ShowResumeInMenu,
    SettingId::AudioOutputDevice,
    SettingId::SpeedStep,
    SettingId::PitchStep,
    SettingId::SpeedPitchHoldDelayMs,
    SettingId::SpeedPitchHoldIntervalMs,
    SettingId::PitchMode,
    SettingId::SeekSeconds,
    SettingId::VolumeStep,
    SettingId::DefaultVolume,
    SettingId::VolumeBoostByDefault,
];

const EQUALIZER: &[SettingId] = &[
    SettingId::GlobalEqualizerEnabled,
    SettingId::GlobalEqualizerPreset,
    SettingId::GlobalEqualizerGains,
    SettingId::EqualizerPresetGains,
    SettingId::EqualizerCustomNames,
    SettingId::EqualizerDevicePresets,
    SettingId::EqualizerDbRange,
    SettingId::EqualizerClippingProtection,
];

const DOWNLOADS: &[SettingId] = &[
    SettingId::AudioFormat,
    SettingId::VideoFormat,
    SettingId::MaxVideoHeight,
    SettingId::AskDownloadLocationEachTime,
    SettingId::QuietDownloads,
    SettingId::KeepPlaylistOrder,
    SettingId::FilenameTemplate,
    SettingId::AudioQuality,
    SettingId::WriteThumbnail,
    SettingId::WriteDescription,
    SettingId::WriteInfoJson,
    SettingId::WriteSubtitles,
    SettingId::AutoSubtitles,
    SettingId::SubtitleLanguages,
    SettingId::EmbedMetadata,
    SettingId::EmbedThumbnail,
    SettingId::RestrictFilenames,
    SettingId::OpenFolderAfterDownload,
    SettingId::PopupWhenDownloadComplete,
    SettingId::PopupWhenConversionComplete,
    SettingId::ConfirmBeforeDownload,
    SettingId::DownloadArchive,
];

const LIBRARY: &[SettingId] = &[
    SettingId::SubscriptionCheckEnabled,
    SettingId::SubscriptionCheckIntervalHours,
    SettingId::LastSubscriptionCheck,
    SettingId::EnableTrending,
    SettingId::EnableHistory,
    SettingId::HistoryLimit,
];

const PODCASTS: &[SettingId] = &[
    SettingId::EnablePodcastsRss,
    SettingId::PodcastSearchProvider,
    SettingId::PodcastSearchCountry,
    SettingId::PodcastSearchLimit,
    SettingId::RssMaxItems,
    SettingId::RssRefreshOnStartup,
    SettingId::RssAutoRefreshEnabled,
    SettingId::RssRefreshIntervalHours,
];

const NOTIFICATIONS: &[SettingId] = &[
    SettingId::WindowsNotifications,
    SettingId::DownloadNotifications,
    SettingId::SubscriptionNotifications,
    SettingId::AppUpdateNotifications,
];

const COOKIES: &[SettingId] = &[
    SettingId::RateLimit,
    SettingId::Proxy,
    SettingId::YoutubeDataApiKey,
    SettingId::CookiesFile,
    SettingId::CookiesFromBrowser,
    SettingId::CookiesBrowserProfile,
    SettingId::ShowAdvancedNetworkSettings,
    SettingId::CookieUserAgent,
    SettingId::FfmpegLocation,
    SettingId::ConcurrentFragments,
    SettingId::Retries,
    SettingId::SocketTimeout,
];

const AUDIOVAULT: &[SettingId] = &[
    SettingId::AudiovaultEmail,
    SettingId::AudiovaultPasswordProtected,
];

const SHORTCUTS: &[SettingId] = &[SettingId::KeyboardShortcuts];

pub const SETTINGS_SECTIONS: &[SettingsSectionDefinition] = &[
    SettingsSectionDefinition {
        section: SettingsSection::General,
        label_key: "general_section",
        reset_fields: GENERAL,
    },
    SettingsSectionDefinition {
        section: SettingsSection::MainMenu,
        label_key: "customize_main_menu_section",
        reset_fields: MAIN_MENU,
    },
    SettingsSectionDefinition {
        section: SettingsSection::Playback,
        label_key: "playback_section",
        reset_fields: PLAYBACK,
    },
    SettingsSectionDefinition {
        section: SettingsSection::Equalizer,
        label_key: "equalizer_section",
        reset_fields: EQUALIZER,
    },
    SettingsSectionDefinition {
        section: SettingsSection::Downloads,
        label_key: "downloads_section",
        reset_fields: DOWNLOADS,
    },
    SettingsSectionDefinition {
        section: SettingsSection::Library,
        label_key: "library_section",
        reset_fields: LIBRARY,
    },
    SettingsSectionDefinition {
        section: SettingsSection::Podcasts,
        label_key: "podcasts_section",
        reset_fields: PODCASTS,
    },
    SettingsSectionDefinition {
        section: SettingsSection::Notifications,
        label_key: "notifications_section",
        reset_fields: NOTIFICATIONS,
    },
    SettingsSectionDefinition {
        section: SettingsSection::Cookies,
        label_key: "cookies_network_section",
        reset_fields: COOKIES,
    },
    SettingsSectionDefinition {
        section: SettingsSection::Audiovault,
        label_key: "audiovault_section",
        reset_fields: AUDIOVAULT,
    },
    SettingsSectionDefinition {
        section: SettingsSection::Spotify,
        label_key: "spotify",
        reset_fields: &[],
    },
    SettingsSectionDefinition {
        section: SettingsSection::Shortcuts,
        label_key: "keyboard_shortcuts_section",
        reset_fields: SHORTCUTS,
    },
];

/// Persisted implementation details that are deliberately not user-editable.
pub const INTERNAL_SETTINGS: &[SettingId] = &[
    SettingId::PlayerCommand,
    SettingId::CookiesSourceFile,
    SettingId::CookiesSourceSignature,
    SettingId::MediaAssociationPromptedVersion,
    SettingId::LanguagePrompted,
];

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::{INTERNAL_SETTINGS, SETTINGS_SECTIONS, SettingsSection};
    use crate::SettingId;

    #[test]
    fn sections_match_python_order_and_reset_placement_count() {
        assert_eq!(SETTINGS_SECTIONS.len(), 12);
        assert_eq!(SettingsSection::ALL.len(), SETTINGS_SECTIONS.len());
        assert_eq!(
            SETTINGS_SECTIONS
                .iter()
                .flat_map(|section| section.reset_fields)
                .count(),
            112
        );
        assert!(
            SETTINGS_SECTIONS
                .iter()
                .zip(SettingsSection::ALL)
                .all(|(definition, expected)| definition.section == *expected)
        );
    }

    #[test]
    fn every_persisted_setting_has_exactly_one_ownership_class() {
        let mut placement_counts = HashMap::<SettingId, usize>::new();
        for id in SETTINGS_SECTIONS
            .iter()
            .flat_map(|section| section.reset_fields)
        {
            *placement_counts.entry(*id).or_default() += 1;
        }
        assert_eq!(placement_counts.len(), 111);
        assert_eq!(
            placement_counts.get(&SettingId::AppUpdateNotifications),
            Some(&2)
        );
        assert!(placement_counts.iter().all(|(id, count)| {
            *id == SettingId::AppUpdateNotifications && *count == 2 || *count == 1
        }));

        let internals: HashSet<_> = INTERNAL_SETTINGS.iter().copied().collect();
        assert_eq!(internals.len(), 5);
        assert!(
            internals
                .iter()
                .all(|id| !placement_counts.contains_key(id))
        );

        let covered: HashSet<_> = placement_counts.keys().copied().chain(internals).collect();
        assert_eq!(covered.len(), SettingId::ALL.len());
        assert!(SettingId::ALL.iter().all(|id| covered.contains(id)));
    }

    #[test]
    fn section_ids_and_labels_are_unique_and_nonempty() {
        let ids: HashSet<_> = SETTINGS_SECTIONS
            .iter()
            .map(|definition| definition.section.id())
            .collect();
        let labels: HashSet<_> = SETTINGS_SECTIONS
            .iter()
            .map(|definition| definition.label_key)
            .collect();
        assert_eq!(ids.len(), SETTINGS_SECTIONS.len());
        assert_eq!(labels.len(), SETTINGS_SECTIONS.len());
        assert!(ids.iter().all(|id| !id.is_empty()));
        assert!(labels.iter().all(|label| !label.is_empty()));
    }
}
