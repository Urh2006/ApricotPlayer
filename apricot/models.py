from dataclasses import dataclass, field
from apricot.constants import *


@dataclass
class Settings:
    language: str = "en"
    download_folder: str = str(DEFAULT_DOWNLOAD_ROOT)
    results_limit: int = 0
    audio_format: str = "mp3"
    video_format: str = VIDEO_FORMAT_MP4
    max_video_height: int = 1080
    player_command: str = ""
    autoplay_next: bool = False
    autoplay_related: bool = False
    prefer_browser_playback: bool = False
    player_fullscreen: bool = False
    player_start_paused: bool = False
    announce_play_pause: bool = True
    announce_playback_finished: bool = True
    enable_background_playback: bool = False
    player_speed: str = "1.0"
    speed_audio_mode: str = SPEED_AUDIO_MODE_RUBBERBAND
    show_video_details_by_default: bool = False
    direct_link_enter_action: str = DIRECT_LINK_ENTER_PLAY
    enable_age_restricted_videos: bool = False
    enable_stream_cache: bool = True
    enable_stream_url_cache: bool = True
    stream_url_cache_minutes: int = 20
    prefetch_next_stream_url: bool = True
    gapless_playback: bool = True
    replaygain_mode: str = REPLAYGAIN_MODE_OFF
    enable_online_lyrics: bool = True
    cache_folder: str = str(DEFAULT_CACHE_DIR)
    cache_size_mb: int = 512
    resume_playback: bool = True
    audio_output_device: str = "auto"
    speed_step: float = 0.01
    pitch_step: float = 0.01
    pitch_mode: str = PITCH_MODE_MPV
    global_equalizer_enabled: bool = False
    global_equalizer_preset: str = EQ_PRESET_FLAT
    global_equalizer_gains: dict[str, float] = field(default_factory=default_equalizer_gains)
    equalizer_preset_gains: dict[str, dict[str, float]] = field(default_factory=default_equalizer_preset_gains)
    equalizer_custom_names: dict[str, str] = field(default_factory=default_equalizer_custom_names)
    equalizer_db_range: int = 12
    equalizer_clipping_protection: bool = False
    ask_download_location_each_time: bool = False
    quiet_downloads: bool = False
    keep_playlist_order: bool = True
    filename_template: str = DEFAULT_FILENAME_TEMPLATE
    audio_quality: str = "0"
    seek_seconds: float = 5.0
    volume_step: int = 5
    default_volume: int = 100
    volume_boost_by_default: bool = False
    write_thumbnail: bool = False
    write_description: bool = False
    write_info_json: bool = False
    write_subtitles: bool = False
    auto_subtitles: bool = False
    subtitle_languages: str = "sl,en"
    embed_metadata: bool = True
    embed_thumbnail: bool = False
    restrict_filenames: bool = False
    open_folder_after_download: bool = False
    popup_when_download_complete: bool = True
    popup_when_conversion_complete: bool = True
    auto_update_ytdlp: bool = True
    auto_update_app: bool = True
    app_update_interval_hours: float = 6.0
    app_update_notifications: bool = True
    skipped_update_version: str = ""
    update_channel: str = "stable"
    confirm_before_download: bool = False
    download_archive: bool = False
    rate_limit: str = ""
    proxy: str = ""
    youtube_data_api_key: str = ""
    cookies_file: str = ""
    cookies_from_browser: str = "none"
    cookies_browser_profile: str = COOKIE_PROFILE_AUTO
    show_advanced_network_settings: bool = False
    cookie_user_agent: str = ""
    ffmpeg_location: str = ""
    concurrent_fragments: int = 4
    retries: int = 10
    socket_timeout: int = 20
    close_to_tray: bool = False
    start_with_windows: bool = False
    tray_notification: bool = True
    subscription_check_enabled: bool = True
    subscription_check_interval_hours: float = 6.0
    windows_notifications: bool = True
    download_notifications: bool = True
    subscription_notifications: bool = True
    last_subscription_check: float = 0.0
    enable_trending: bool = False
    enable_history: bool = True
    enable_podcasts_rss: bool = True
    podcast_search_provider: str = PODCAST_DIRECTORY_PROVIDER_APPLE
    podcast_search_country: str = "US"
    podcast_search_limit: int = 20
    rss_max_items: int = 100
    rss_refresh_on_startup: bool = False
    rss_auto_refresh_enabled: bool = False
    rss_refresh_interval_hours: float = 12.0
    history_limit: int = 500
    keyboard_shortcuts: dict[str, str] = field(default_factory=lambda: dict(DEFAULT_KEYBOARD_SHORTCUTS))
    media_association_prompted_version: str = ""
    language_prompted: bool = False






