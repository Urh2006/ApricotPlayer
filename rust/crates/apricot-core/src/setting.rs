//! Stable setting identifiers inherited from the Python 1.0.21 data format.

macro_rules! define_setting_ids {
    ($($variant:ident => $key:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub enum SettingId {
            $($variant),+
        }

        impl SettingId {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn key(self) -> &'static str {
                match self {
                    $(Self::$variant => $key),+
                }
            }
        }
    };
}

define_setting_ids! {
    Language => "language",
    DownloadFolder => "download_folder",
    ResultsLimit => "results_limit",
    AudioFormat => "audio_format",
    VideoFormat => "video_format",
    MaxVideoHeight => "max_video_height",
    PlayerCommand => "player_command",
    AutoplayNext => "autoplay_next",
    AutoplayRelated => "autoplay_related",
    PreferBrowserPlayback => "prefer_browser_playback",
    PlayerFullscreen => "player_fullscreen",
    PlayerStartPaused => "player_start_paused",
    AnnouncePlayPause => "announce_play_pause",
    AnnouncePlaybackFinished => "announce_playback_finished",
    EnableBackgroundPlayback => "enable_background_playback",
    PlayerSpeed => "player_speed",
    SpeedAudioMode => "speed_audio_mode",
    ShowVideoDetailsByDefault => "show_video_details_by_default",
    DirectLinkEnterAction => "direct_link_enter_action",
    EnableAgeRestrictedVideos => "enable_age_restricted_videos",
    EnableStreamCache => "enable_stream_cache",
    EnableStreamUrlCache => "enable_stream_url_cache",
    StreamUrlCacheMinutes => "stream_url_cache_minutes",
    StreamFormatPreference => "stream_format_preference",
    PrefetchNextStreamUrl => "prefetch_next_stream_url",
    GaplessPlayback => "gapless_playback",
    ReplaygainMode => "replaygain_mode",
    EnableOnlineLyrics => "enable_online_lyrics",
    CacheFolder => "cache_folder",
    CacheSizeMb => "cache_size_mb",
    ResumePlayback => "resume_playback",
    ShowResumeInMenu => "show_resume_in_menu",
    AudioOutputDevice => "audio_output_device",
    SpeedStep => "speed_step",
    PitchStep => "pitch_step",
    SpeedPitchHoldDelayMs => "speed_pitch_hold_delay_ms",
    SpeedPitchHoldIntervalMs => "speed_pitch_hold_interval_ms",
    PitchMode => "pitch_mode",
    GlobalEqualizerEnabled => "global_equalizer_enabled",
    GlobalEqualizerPreset => "global_equalizer_preset",
    GlobalEqualizerGains => "global_equalizer_gains",
    EqualizerPresetGains => "equalizer_preset_gains",
    EqualizerCustomNames => "equalizer_custom_names",
    EqualizerDevicePresets => "equalizer_device_presets",
    EqualizerDbRange => "equalizer_db_range",
    EqualizerClippingProtection => "equalizer_clipping_protection",
    AskDownloadLocationEachTime => "ask_download_location_each_time",
    QuietDownloads => "quiet_downloads",
    KeepPlaylistOrder => "keep_playlist_order",
    FilenameTemplate => "filename_template",
    AudioQuality => "audio_quality",
    SeekSeconds => "seek_seconds",
    VolumeStep => "volume_step",
    DefaultVolume => "default_volume",
    VolumeBoostByDefault => "volume_boost_by_default",
    WriteThumbnail => "write_thumbnail",
    WriteDescription => "write_description",
    WriteInfoJson => "write_info_json",
    WriteSubtitles => "write_subtitles",
    AutoSubtitles => "auto_subtitles",
    SubtitleLanguages => "subtitle_languages",
    EmbedMetadata => "embed_metadata",
    EmbedThumbnail => "embed_thumbnail",
    RestrictFilenames => "restrict_filenames",
    OpenFolderAfterDownload => "open_folder_after_download",
    PopupWhenDownloadComplete => "popup_when_download_complete",
    PopupWhenConversionComplete => "popup_when_conversion_complete",
    AutoUpdateYtdlp => "auto_update_ytdlp",
    AutoUpdateApp => "auto_update_app",
    AppUpdateIntervalHours => "app_update_interval_hours",
    AppUpdateNotifications => "app_update_notifications",
    SkippedUpdateVersion => "skipped_update_version",
    UpdateChannel => "update_channel",
    ConfirmBeforeDownload => "confirm_before_download",
    DownloadArchive => "download_archive",
    RateLimit => "rate_limit",
    Proxy => "proxy",
    YoutubeDataApiKey => "youtube_data_api_key",
    AudiovaultEmail => "audiovault_email",
    AudiovaultPasswordProtected => "audiovault_password_protected",
    CookiesFile => "cookies_file",
    CookiesSourceFile => "cookies_source_file",
    CookiesSourceSignature => "cookies_source_signature",
    CookiesFromBrowser => "cookies_from_browser",
    CookiesBrowserProfile => "cookies_browser_profile",
    ShowAdvancedNetworkSettings => "show_advanced_network_settings",
    CookieUserAgent => "cookie_user_agent",
    FfmpegLocation => "ffmpeg_location",
    ConcurrentFragments => "concurrent_fragments",
    Retries => "retries",
    SocketTimeout => "socket_timeout",
    CloseToTray => "close_to_tray",
    StartWithWindows => "start_with_windows",
    TrayNotification => "tray_notification",
    SubscriptionCheckEnabled => "subscription_check_enabled",
    SubscriptionCheckIntervalHours => "subscription_check_interval_hours",
    WindowsNotifications => "windows_notifications",
    DownloadNotifications => "download_notifications",
    SubscriptionNotifications => "subscription_notifications",
    LastSubscriptionCheck => "last_subscription_check",
    EnableTrending => "enable_trending",
    EnableHistory => "enable_history",
    EnablePodcastsRss => "enable_podcasts_rss",
    ShowShortcutsInLabels => "show_shortcuts_in_labels",
    MainMenuHiddenActions => "main_menu_hidden_actions",
    PodcastSearchProvider => "podcast_search_provider",
    PodcastSearchCountry => "podcast_search_country",
    PodcastSearchLimit => "podcast_search_limit",
    RssMaxItems => "rss_max_items",
    RssRefreshOnStartup => "rss_refresh_on_startup",
    RssAutoRefreshEnabled => "rss_auto_refresh_enabled",
    RssRefreshIntervalHours => "rss_refresh_interval_hours",
    HistoryLimit => "history_limit",
    KeyboardShortcuts => "keyboard_shortcuts",
    MediaAssociationPromptedVersion => "media_association_prompted_version",
    LanguagePrompted => "language_prompted",
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::SettingId;

    #[test]
    fn baseline_contains_116_unique_settings() {
        let keys: HashSet<_> = SettingId::ALL.iter().map(|id| id.key()).collect();
        assert_eq!(SettingId::ALL.len(), 116);
        assert_eq!(keys.len(), SettingId::ALL.len());
    }
}
