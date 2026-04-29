from __future__ import annotations

import json
import os
import queue
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import webbrowser
import zipfile
import ctypes
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from importlib import import_module
from pathlib import Path
from urllib.parse import urlencode
from urllib.request import Request, urlopen
from urllib.error import HTTPError, URLError

import wx

from locales import EXTRA_TEXT, SL_TRANSLATION_FIXES

yt_dlp = None
yt_dlp_import_error: Exception | None = None


def get_yt_dlp():
    global yt_dlp, yt_dlp_import_error
    if yt_dlp is not None:
        return yt_dlp
    if yt_dlp_import_error is not None:
        return None
    try:
        yt_dlp = import_module("yt_dlp")
    except ImportError as exc:
        yt_dlp_import_error = exc
        return None
    return yt_dlp

try:
    import winsound
except ImportError:
    winsound = None


class NullTextStream:
    encoding = "utf-8"

    def write(self, _text: str) -> int:
        return len(str(_text))

    def flush(self) -> None:
        pass

    def isatty(self) -> bool:
        return False


if sys.stdout is None:
    sys.stdout = NullTextStream()
if sys.stderr is None:
    sys.stderr = NullTextStream()


class QuietYtdlpLogger:
    def debug(self, _message: str) -> None:
        pass

    def warning(self, _message: str) -> None:
        pass

    def error(self, _message: str) -> None:
        pass


YTDLP_LOGGER = QuietYtdlpLogger()
APP_NAME = "ApricotPlayer"
APP_VERSION = "0.4.4"
APP_VERSION_LABEL = "0.4.4"
WINDOW_TITLE = f"{APP_NAME} {APP_VERSION_LABEL}"
LEGACY_APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "UrhasaurusYouTubePlayer"
APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "ApricotPlayer"
SETTINGS_FILE = APP_DIR / "settings.json"
FAVORITES_FILE = APP_DIR / "favorites.json"
CACHED_COOKIES_FILE = APP_DIR / "cookies.txt"
LEGACY_SETTINGS_FILE = LEGACY_APP_DIR / "settings.json"
LEGACY_FAVORITES_FILE = LEGACY_APP_DIR / "favorites.json"
DEFAULT_FILENAME_TEMPLATE = "%(title)s.%(ext)s"
OLD_FILENAME_TEMPLATE = "%(title)s [%(id)s].%(ext)s"
RESULTS_PAGE_SIZE = 20
DEFAULT_GITHUB_OWNER = "Urh2006"
DEFAULT_GITHUB_REPO = "ApricotPlayer"
INSTALLER_ASSET_NAME = "ApricotPlayerSetup.exe"
PORTABLE_ZIP_ASSET_NAME = "ApricotPlayerPortable.zip"
UPDATE_LOG_FILE = APP_DIR / "updater.log"
PLAYBACK_SPEED_STEPS = [0.25, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 1.0, 1.1, 1.2, 1.25, 1.3, 1.4, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0]
PITCH_STEPS = [0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95, 1.0, 1.05, 1.1, 1.15, 1.2, 1.25, 1.3, 1.35, 1.4, 1.45, 1.5, 1.6, 1.7, 1.8, 1.9, 2.0]
DEFAULT_REACHED_SOUND = "default_reached.wav"
PITCH_MODE_RUBBERBAND = "Independent pitch - best quality (Rubberband)"
PITCH_MODE_MPV = "Independent pitch - basic (mpv built-in)"
PITCH_MODE_LINKED_SPEED = "Linked pitch and speed - pitch keys change both"
PITCH_MODE_OPTIONS = [PITCH_MODE_RUBBERBAND, PITCH_MODE_MPV, PITCH_MODE_LINKED_SPEED]
LEGACY_PITCH_MODE_RUBBERBAND = "rubberband"
LEGACY_PITCH_MODE_MPV = "mpv pitch"
LEGACY_PITCH_MODE_LINKED_SPEED = "linked speed"
RUBBERBAND_FILTER_LABEL = "apricot_pitch"
RUBBERBAND_FILTER_REF = f"@{RUBBERBAND_FILTER_LABEL}"
RATE_STEP_OPTIONS = ["0.01", "0.02", "0.05", "0.10", "0.25"]
COOKIES_BROWSER_OPTIONS = ["none", "chrome", "edge", "firefox", "brave", "chromium", "opera", "vivaldi"]
VIDEO_FORMAT_MP4 = "mp4"
VIDEO_FORMAT_BEST_ANY = "best-any"
VIDEO_FORMAT_MP4_SINGLE = "mp4-single"
VIDEO_FORMAT_SMALLEST = "smallest"
VIDEO_FORMAT_OPTIONS = [VIDEO_FORMAT_MP4, VIDEO_FORMAT_BEST_ANY, VIDEO_FORMAT_MP4_SINGLE, VIDEO_FORMAT_SMALLEST]
LEGACY_VIDEO_FORMAT_MAP = {
    "bestvideo+bestaudio/best": VIDEO_FORMAT_MP4,
    "best": VIDEO_FORMAT_BEST_ANY,
    "best[ext=mp4]": VIDEO_FORMAT_MP4_SINGLE,
    "best[ext=mp4]/best": VIDEO_FORMAT_MP4_SINGLE,
    "worst": VIDEO_FORMAT_SMALLEST,
}
MPV_IPC_TIMEOUT_SECONDS = 2.5
MPV_PITCH_RETRY_ATTEMPTS = 8
MPV_PITCH_RETRY_DELAY_SECONDS = 0.12
LANGUAGES = [
    ("en", "English"),
    ("sl", "Slovenščina"),
    ("de", "Deutsch"),
    ("fr", "Français"),
    ("es", "Español"),
    ("pt", "Português"),
    ("it", "Italiano"),
    ("pl", "Polski"),
    ("nl", "Nederlands"),
    ("sv", "Svenska"),
    ("hr", "Hrvatski"),
    ("sr", "Srpski"),
]
LANGUAGE_CODES = [code for code, _name in LANGUAGES]
LANGUAGE_NAMES = {code: name for code, name in LANGUAGES}


TEXT = {
    "sl": {
        "ready": "Pripravljen.",
        "main_menu": "Glavni meni",
        "download_all": "Download all",
        "queued_videos_for_download": "Queued videos for download",
        "queued_downloads": "Queued videos for download",
        "no_queued_downloads": "No queued downloads.",
        "queued_download_instructions": "Use Enter to download with the queued format, Ctrl+Shift+A for audio, Ctrl+Shift+D for video, or the context menu.",
        "download_selected_queued": "Download selected queued item",
        "remove_from_queue": "Remove from queue",
        "search_youtube": "Iskanje po YouTube",
        "choose_download_folder": "Izbor mape za prenose",
        "favorites": "Priljubljeni",
        "settings": "Nastavitve",
        "settings_sections": "Razdelki nastavitev",
        "general_section": "Splošno",
        "playback_section": "Predvajanje",
        "downloads_section": "Prenosi",
        "cookies_network_section": "Piškotki in omrežje",
        "updates_advanced_section": "Posodobitve in napredno",
        "exit": "Izhod",
        "open": "Odpri",
        "back": "Nazaj v glavni meni",
        "back_results": "Nazaj na rezultate",
        "internal_player": "Predvajalnik",
        "player_missing": "Notranji predvajalnik mpv ni najden. Program ne bo odpiral YouTube strani.",
        "player_announcement": "Obvestilo predvajalnika",
        "video_details": "Podrobnosti videa",
        "details_button": "View video details",
        "details_closed": "Video details closed.",
        "timing_unavailable": "Timing is not available yet.",
        "pitch_unavailable": "Pitch control is not available yet.",
        "time_announcement": "Elapsed {elapsed}, remaining {remaining}, total {total}.",
        "speed_announcement": "Playback speed {speed}x.",
        "pitch_announcement": "Pitch {pitch}x.",
        "download_started": "Download started.",
        "download_audio_start": "Downloading audio...",
        "download_video_start": "Downloading video...",
        "batch_download_start": "Starting batch download of {count} items.",
        "batch_download_done": "Batch download complete.",
        "audio_selected_download": "Audio download queued: {title}",
        "video_selected_download": "Video download queued: {title}",
        "download_deselected": "Removed from download queue: {title}",
        "download_queue_empty": "Download queue is empty.",
        "audio_queued_marker": "audio queued",
        "video_queued_marker": "video queued",
        "details_unavailable": "Video details are not available yet.",
        "version": "Verzija",
        "description": "Description",
        "url": "URL",
        "uploaded": "uploaded",
        "dynamic_results": "0 (dinamično, po 20)",
        "url_copied": "Povezava je kopirana.",
        "download_audio_done": "Audio downloaded: {title}",
        "download_video_done": "Video downloaded: {title}",
        "download_progress": "{mode}: {percent}% - {title}",
        "download_processing": "{mode}: obdelujem - {title}",
        "download_audio_mode": "Audio",
        "download_video_mode": "Video",
        "details_title": "Podrobnosti videa",
        "search_more_loaded": "Naloženih rezultatov: {count}.",
        "copy_link": "Copy link",
        "settings_file": "Settings file",
        "restore_defaults": "Restore to defaults",
        "defaults_restored": "Default settings restored.",
        "loading_more_results": "Loading more results.",
        "no_more_results": "No more results.",
        "auto_update_app": "Ob zagonu preveri posodobitve programa",
        "check_app_updates_now": "Preveri posodobitve",
        "github_owner": "GitHub owner",
        "github_repo": "GitHub repo",
        "github_token": "GitHub token za private updates",
        "checking_app_updates": "Preverjam posodobitve programa.",
        "app_up_to_date": "Program je posodobljen.",
        "app_update_available": "Na voljo je nova verzija {version}. Želiš prenos in namestitev zdaj?",
        "app_update_disabled": "Samodejne posodobitve programa so izključene.",
        "app_update_failed": "Posodobitve programa ni bilo mogoče preveriti: {error}",
        "downloading_update": "Prenašam posodobitev {version}.",
        "update_progress_title": "Updating ApricotPlayer",
        "update_download_percent": "Downloading update {version}: {percent}%",
        "update_download_unknown": "Downloading update {version}",
        "update_download_complete": "Update downloaded. Preparing install.",
        "update_install_started": "Update started. ApricotPlayer will close and reopen.",
        "update_install_log": "Updater log: {path}",
        "installing_update": "Nameščam posodobitev {version}.",
        "update_ready_restart": "Posodobitev je pripravljena. Program se bo zaprl in znova zagnal.",
        "update_source_only": "Samodejna namestitev deluje samo v .exe verziji. Na voljo je nova izdaja: {version}",
        "pitch_label": "Pitch",
        "update_available_title": "Update available",
        "update_version_heading": "Version {version}",
        "whats_new": "What's new?",
        "update_now": "Do you want to update now?",
        "update_now_button": "Update now",
        "skip_version_button": "Skip this version",
        "update_skipped": "Posodobitev {version} je preskočena.",
        "update_skip_status": "Preskočena posodobitev {version}.",
        "no_changelog": "Za to izdajo ni opisa sprememb.",
        "search_query": "Iskalni niz",
        "type": "Vrsta",
        "search": "Search",
        "video": "Video",
        "playlist": "Playlist",
        "channel": "Kanal",
        "all": "Vse",
        "views": "Ogledi",
        "play": "Play",
        "download_audio": "Download audio",
        "download_video": "Download video",
        "download_playlist": "Download playlist",
        "download_channel": "Download channel",
        "download_playlist_start": "Downloading playlist...",
        "download_channel_start": "Downloading channel...",
        "download_playlist_done": "Playlist downloaded: {title}",
        "download_channel_done": "Channel downloaded: {title}",
        "add_favorite": "Add to favorites",
        "open_browser": "Open in browser",
        "copy_url": "Copy URL",
        "remove": "Remove",
        "refresh": "Refresh list",
        "download_folder": "Mapa za prenose",
        "browse": "Prebrskaj",
        "save": "Shrani",
        "language": "Jezik",
        "results_limit": "Število rezultatov",
        "seek_seconds": "Seek v sekundah",
        "volume_step": "Korak glasnosti",
        "speed_step": "Korak hitrosti predvajanja",
        "pitch_step": "Korak pitcha",
        "pitch_mode": "Nacin pitch kontrole",
        "pitch_mode_rubberband": "Neodvisna visina tona - najboljsa kakovost (Rubberband)",
        "pitch_mode_mpv": "Neodvisna visina tona - osnovno (mpv)",
        "pitch_mode_linked_speed": "Povezana visina tona in hitrost - tipke za visino tona spremenijo oboje",
        "auto_update": "Ob vsakem zagonu preveri posodobitve yt-dlp",
        "autoplay_next": "Po koncu posnetka samodejno predvajaj naslednjega",
        "confirm_download": "Pred prenosom vprašaj za potrditev",
        "open_after_download": "Po prenosu odpri mapo za prenose",
        "download_complete_popup": "Pokazi popup, ko je prenos koncan",
        "audio_format": "Audio format",
        "audio_quality": "Audio kvaliteta (0 najboljše)",
        "video_format": "Format prenosa videa",
        "video_format_mp4_recommended": "MP4 (priporočeno)",
        "video_format_best_available": "Najboljša razpoložljiva kakovost (lahko je WebM)",
        "video_format_mp4_single": "MP4 ena datoteka (hitro)",
        "video_format_smallest": "Najmanjša datoteka",
        "max_height": "Največja višina videa",
        "filename_template": "Predloga imena datoteke",
        "subtitle_langs": "Jeziki podnapisov",
        "quiet_downloads": "Tišji prenosi",
        "playlist_order": "Ohrani vrstni red playlist",
        "write_thumbnail": "Shrani thumbnail",
        "write_description": "Shrani opis",
        "write_info_json": "Shrani info JSON",
        "write_subtitles": "Prenesi ročne podnapise",
        "auto_subtitles": "Prenesi samodejne podnapise",
        "embed_metadata": "Vgradi metapodatke",
        "embed_thumbnail": "Vgradi thumbnail",
        "restrict_filenames": "Varna ASCII imena datotek",
        "download_archive": "Preskoči že prenesene z download archive",
        "player_command": "Ukaz ali pot do playerja",
        "player_speed": "Hitrost predvajanja",
        "browser_playback": "Vedno predvajaj v brskalniku",
        "fullscreen": "Začni v full screen",
        "start_paused": "Začni pavzirano",
        "rate_limit": "Rate limit",
        "proxy": "Proxy URL",
        "cookies": "Cookies file",
        "cookies_from_browser": "Cookies from browser",
        "export_browser_cookies": "Export browser cookies to cookies.txt",
        "exporting_browser_cookies": "Exporting browser cookies.",
        "browser_cookies_exported": "Browser cookies exported to {path}. Cookies from browser is now set to none.",
        "browser_cookies_export_failed": "Browser cookies export failed: {error}",
        "select_cookies_browser": "Najprej izberi browser pri Cookies from browser.",
        "ffmpeg": "FFmpeg pot",
        "fragments": "Sočasni fragmenti",
        "retries": "Število ponovitev",
        "timeout": "Socket timeout",
        "enter_query": "Vpiši iskalni niz.",
        "searching": "Iščem: {query}",
        "loading_channel": "Nalagam videe kanala: {title}",
        "loading_playlist": "Nalagam videe playliste: {title}",
        "found": "Najdenih rezultatov: {count}.",
        "no_selection": "Izberi element.",
        "playing": "Predvajam: {title}",
        "preparing_stream": "Pripravljam predvajanje: {title}",
        "opened_browser": "Odprto v brskalniku: {title}",
        "no_player": "Player ni najden.",
        "player_failed": "Player se ni zagnal: {error}",
        "stopped": "Predvajanje ustavljeno.",
        "volume_boost_on": "Volume boost on.",
        "volume_boost_off": "Volume boost off.",
        "queued": "V čakalni vrsti",
        "done": "Končano",
        "download_confirm": "Prenesem {action}: {title}?",
        "download_cancelled": "Prenos preklican.",
        "download_done": "Prenos končan: {title}",
        "download_failed": "Prenos ni uspel: {error}",
        "youtube_auth_hint": "YouTube zahteva prijavo ali bot potrditev. V nastavitvah nastavi Cookies from browser na brskalnik, kjer si prijavljen v YouTube, na primer Chrome, Edge ali Firefox.",
        "cookie_copy_hint": "ApricotPlayer ne more prebrati Chrome cookie baze. Zapri Chrome v celoti, v nastavitvah izberi Edge ali Firefox, ali uporabi izvozen cookies.txt file.",
        "favorite_added": "Dodano med priljubljene.",
        "favorite_exists": "Ta element je že med priljubljenimi.",
        "favorite_removed": "Odstranjeno iz priljubljenih.",
        "settings_saved": "Nastavitve shranjene.",
        "checking_updates": "Preverjam posodobitve za YouTube podporo.",
        "updates_ok": "YouTube podpora je posodobljena.",
        "updates_failed": "Posodobitve YouTube podpore ni bilo mogoče preveriti: {error}",
        "missing_ytdlp": "Manjka yt-dlp.",
    },
    "en": {
        "ready": "Ready.",
        "main_menu": "Main menu",
        "download_all": "Download all",
        "queued_videos_for_download": "Queued videos for download",
        "queued_downloads": "Queued videos for download",
        "no_queued_downloads": "No queued downloads.",
        "queued_download_instructions": "Use Enter to download with the queued format, Ctrl+Shift+A for audio, Ctrl+Shift+D for video, or the context menu.",
        "download_selected_queued": "Download selected queued item",
        "remove_from_queue": "Remove from queue",
        "search_youtube": "Search YouTube",
        "choose_download_folder": "Choose download folder",
        "favorites": "Favorites",
        "settings": "Settings",
        "settings_sections": "Settings sections",
        "general_section": "General",
        "playback_section": "Playback",
        "downloads_section": "Downloads",
        "cookies_network_section": "Cookies and network",
        "updates_advanced_section": "Updates and advanced",
        "exit": "Exit",
        "open": "Open",
        "back": "Back to main menu",
        "back_results": "Back to results",
        "internal_player": "Player",
        "player_missing": "Internal mpv player was not found. The program will not open the YouTube page.",
        "player_announcement": "Player announcement",
        "video_details": "Video details",
        "details_button": "View video details",
        "details_closed": "Video details closed.",
        "timing_unavailable": "Timing is not available yet.",
        "pitch_unavailable": "Pitch control is not available yet.",
        "time_announcement": "Elapsed {elapsed}, remaining {remaining}, total {total}.",
        "speed_announcement": "Playback speed {speed}x.",
        "pitch_announcement": "Pitch {pitch}x.",
        "download_started": "Download started.",
        "download_audio_start": "Downloading audio...",
        "download_video_start": "Downloading video...",
        "batch_download_start": "Starting batch download of {count} items.",
        "batch_download_done": "Batch download complete.",
        "audio_selected_download": "Audio download queued: {title}",
        "video_selected_download": "Video download queued: {title}",
        "download_deselected": "Removed from download queue: {title}",
        "download_queue_empty": "Download queue is empty.",
        "audio_queued_marker": "audio queued",
        "video_queued_marker": "video queued",
        "details_unavailable": "Video details are not available yet.",
        "version": "Version",
        "description": "Description",
        "url": "URL",
        "uploaded": "uploaded",
        "dynamic_results": "0 (dynamic, by 20)",
        "url_copied": "Link copied.",
        "download_audio_done": "Audio downloaded: {title}",
        "download_video_done": "Video downloaded: {title}",
        "download_progress": "{mode}: {percent}% - {title}",
        "download_processing": "{mode}: processing - {title}",
        "download_audio_mode": "Audio",
        "download_video_mode": "Video",
        "details_title": "Video details",
        "search_more_loaded": "Loaded results: {count}.",
        "copy_link": "Copy link",
        "settings_file": "Settings file",
        "restore_defaults": "Restore to defaults",
        "defaults_restored": "Default settings restored.",
        "loading_more_results": "Loading more results.",
        "no_more_results": "No more results.",
        "auto_update_app": "Check for updates at startup",
        "check_app_updates_now": "Check for updates",
        "github_owner": "GitHub owner",
        "github_repo": "GitHub repo",
        "github_token": "GitHub token for private updates",
        "checking_app_updates": "Checking app updates.",
        "app_up_to_date": "The app is up to date.",
        "app_update_available": "Version {version} is available. Download and install now?",
        "app_update_disabled": "Automatic app updates are disabled.",
        "app_update_failed": "Could not check app updates: {error}",
        "downloading_update": "Downloading update {version}.",
        "update_progress_title": "Updating ApricotPlayer",
        "update_download_percent": "Downloading update {version}: {percent}%",
        "update_download_unknown": "Downloading update {version}",
        "update_download_complete": "Update downloaded. Preparing install.",
        "update_install_started": "Update started. ApricotPlayer will close and reopen.",
        "update_install_log": "Updater log: {path}",
        "installing_update": "Installing update {version}.",
        "update_ready_restart": "The update is ready. The app will close and restart.",
        "update_source_only": "Automatic install works only in the .exe build. New release available: {version}",
        "pitch_label": "Pitch",
        "update_available_title": "Update available",
        "update_version_heading": "Version {version}",
        "whats_new": "What's new?",
        "update_now": "Would you like to update now?",
        "update_now_button": "Update now",
        "skip_version_button": "Skip this version",
        "update_skipped": "Update {version} was skipped.",
        "update_skip_status": "Skipped update {version}.",
        "no_changelog": "No changelog was provided for this release.",
        "search_query": "Search query",
        "type": "Type",
        "search": "Search",
        "video": "Video",
        "playlist": "Playlist",
        "channel": "Channel",
        "all": "All",
        "views": "Views",
        "play": "Play",
        "download_audio": "Download audio",
        "download_video": "Download video",
        "download_playlist": "Download playlist",
        "download_channel": "Download channel",
        "download_playlist_start": "Downloading playlist...",
        "download_channel_start": "Downloading channel...",
        "download_playlist_done": "Playlist downloaded: {title}",
        "download_channel_done": "Channel downloaded: {title}",
        "add_favorite": "Add to favorites",
        "open_browser": "Open in browser",
        "copy_url": "Copy URL",
        "remove": "Remove",
        "refresh": "Refresh list",
        "download_folder": "Download folder",
        "browse": "Browse",
        "save": "Save",
        "language": "Language",
        "results_limit": "Number of results",
        "seek_seconds": "Seek seconds",
        "volume_step": "Volume step",
        "speed_step": "Playback speed key step",
        "pitch_step": "Pitch key step",
        "pitch_mode": "Pitch control mode",
        "pitch_mode_rubberband": "Independent pitch - best quality (Rubberband)",
        "pitch_mode_mpv": "Independent pitch - basic (mpv built-in)",
        "pitch_mode_linked_speed": "Linked pitch and speed - pitch keys change both",
        "auto_update": "Check yt-dlp updates on every startup",
        "autoplay_next": "Automatically play next item",
        "confirm_download": "Ask before starting a download",
        "open_after_download": "Open download folder after download",
        "download_complete_popup": "Show popup when download completes",
        "audio_format": "Audio format",
        "audio_quality": "Audio quality (0 is best)",
        "video_format": "Video download format",
        "video_format_mp4_recommended": "MP4 (recommended)",
        "video_format_best_available": "Best available quality (may be WebM)",
        "video_format_mp4_single": "MP4 single file (fast)",
        "video_format_smallest": "Smallest file",
        "max_height": "Maximum video height",
        "filename_template": "Filename template",
        "subtitle_langs": "Subtitle languages",
        "quiet_downloads": "Quieter downloads",
        "playlist_order": "Keep playlist order",
        "write_thumbnail": "Save thumbnail",
        "write_description": "Save description",
        "write_info_json": "Save info JSON",
        "write_subtitles": "Download manual subtitles",
        "auto_subtitles": "Download automatic subtitles",
        "embed_metadata": "Embed metadata",
        "embed_thumbnail": "Embed thumbnail",
        "restrict_filenames": "Safe ASCII filenames",
        "download_archive": "Skip already downloaded items with archive",
        "player_command": "Player command or path",
        "player_speed": "Playback speed",
        "browser_playback": "Always play in browser",
        "fullscreen": "Start full screen",
        "start_paused": "Start paused",
        "rate_limit": "Rate limit",
        "proxy": "Proxy URL",
        "cookies": "Cookies file",
        "cookies_from_browser": "Cookies from browser",
        "export_browser_cookies": "Export browser cookies to cookies.txt",
        "exporting_browser_cookies": "Exporting browser cookies.",
        "browser_cookies_exported": "Browser cookies exported to {path}. Cookies from browser is now set to none.",
        "browser_cookies_export_failed": "Browser cookies export failed: {error}",
        "select_cookies_browser": "Choose a browser in Cookies from browser first.",
        "ffmpeg": "FFmpeg path",
        "fragments": "Concurrent fragments",
        "retries": "Retries",
        "timeout": "Socket timeout",
        "enter_query": "Enter a search query.",
        "searching": "Searching: {query}",
        "loading_channel": "Loading channel videos: {title}",
        "loading_playlist": "Loading playlist videos: {title}",
        "found": "Found results: {count}.",
        "no_selection": "Select an item.",
        "playing": "Playing: {title}",
        "preparing_stream": "Preparing playback: {title}",
        "opened_browser": "Opened in browser: {title}",
        "no_player": "Player not found.",
        "player_failed": "Player did not start: {error}",
        "stopped": "Playback stopped.",
        "volume_boost_on": "Volume boost on.",
        "volume_boost_off": "Volume boost off.",
        "queued": "Queued",
        "done": "Done",
        "download_confirm": "Download {action}: {title}?",
        "download_cancelled": "Download cancelled.",
        "download_done": "Download finished: {title}",
        "download_failed": "Download failed: {error}",
        "youtube_auth_hint": "YouTube asks for sign-in or bot confirmation. Open Settings and set Cookies from browser to the browser where you are signed in to YouTube, for example Chrome, Edge, or Firefox.",
        "cookie_copy_hint": "ApricotPlayer could not read the Chrome cookie database. Close Chrome completely, choose Edge or Firefox in Settings, or use an exported cookies.txt file.",
        "favorite_added": "Added to favorites.",
        "favorite_exists": "This item is already in favorites.",
        "favorite_removed": "Removed from favorites.",
        "settings_saved": "Settings saved.",
        "checking_updates": "Checking updates for YouTube support.",
        "updates_ok": "YouTube support is up to date.",
        "updates_failed": "Could not check YouTube support updates: {error}",
        "missing_ytdlp": "yt-dlp is missing.",
    },
}

TEXT["sl"].update(SL_TRANSLATION_FIXES)
TEXT.update(EXTRA_TEXT)
for language_code in LANGUAGE_CODES:
    TEXT[language_code] = {**TEXT["en"], **TEXT.get(language_code, {})}


@dataclass
class Settings:
    language: str = "en"
    download_folder: str = str(Path.home() / "Downloads")
    results_limit: int = 20
    audio_format: str = "mp3"
    video_format: str = VIDEO_FORMAT_MP4
    max_video_height: int = 1080
    player_command: str = ""
    autoplay_next: bool = False
    prefer_browser_playback: bool = False
    player_fullscreen: bool = False
    player_start_paused: bool = False
    player_speed: str = "1.0"
    speed_step: float = 0.01
    pitch_step: float = 0.01
    pitch_mode: str = PITCH_MODE_RUBBERBAND
    quiet_downloads: bool = False
    keep_playlist_order: bool = True
    filename_template: str = DEFAULT_FILENAME_TEMPLATE
    audio_quality: str = "0"
    seek_seconds: int = 5
    volume_step: int = 5
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
    auto_update_ytdlp: bool = True
    auto_update_app: bool = True
    skipped_update_version: str = ""
    confirm_before_download: bool = False
    download_archive: bool = False
    rate_limit: str = ""
    proxy: str = ""
    cookies_file: str = ""
    cookies_from_browser: str = "none"
    ffmpeg_location: str = ""
    github_owner: str = DEFAULT_GITHUB_OWNER
    github_repo: str = DEFAULT_GITHUB_REPO
    github_token: str = ""
    concurrent_fragments: int = 4
    retries: int = 10
    socket_timeout: int = 20


class MainFrame(wx.Frame):
    def __init__(self) -> None:
        super().__init__(None, title=WINDOW_TITLE, size=(950, 680))
        APP_DIR.mkdir(parents=True, exist_ok=True)
        self.settings = self.load_settings()
        self.save_settings()
        self.favorites = self.load_favorites()
        self.results: list[dict] = []
        self.all_results: list[dict] = []
        self.return_results: list[dict] = []
        self.return_all_results: list[dict] = []
        self.return_index = 0
        self.return_visible_count = 0
        self.last_search_query = ""
        self.last_search_type_index = 0
        self.last_visible_count = 0
        self.settings_section_index = 0
        self.current_index = -1
        self.player_process: subprocess.Popen | None = None
        self.player_log_handle = None
        self.player_kind = ""
        self.player_control_mode = False
        self.volume_boost_enabled = False
        self.rubberband_pitch_filter_active = False
        self.in_player_screen = False
        self.in_queue_screen = False
        self.current_video_item: dict | None = None
        self.current_video_info: dict = {}
        self.details_label: wx.StaticText | None = None
        self.video_details: wx.TextCtrl | None = None
        self.download_queue: dict[str, dict] = {}
        self.queue_items: list[dict] = []
        self.last_download_shortcut: tuple[str, str, float] = ("", "", 0.0)
        self.ipc_path: str | None = None
        self.mpv_ipc_lock = threading.Lock()
        self.ui_queue: queue.Queue[tuple[str, object]] = queue.Queue()
        self.loading_more_results = False
        self.current_search_type_code = "Video"
        self.collection_url = ""
        self.collection_result_type = ""
        self.nvda_client = self.load_nvda_client()
        self.update_progress_dialog: wx.ProgressDialog | None = None

        self.panel = wx.Panel(self)
        self.root_sizer = wx.BoxSizer(wx.VERTICAL)
        self.panel.SetSizer(self.root_sizer)
        self.status = self.CreateStatusBar()
        self.status.SetStatusText(self.t("ready"))

        self.Bind(wx.EVT_CHAR_HOOK, self.on_char_hook)
        self.install_download_accelerators()
        self.show_main_menu()
        self.timer = wx.Timer(self)
        self.Bind(wx.EVT_TIMER, self.process_queue, self.timer)
        self.timer.Start(100)
        if self.settings.auto_update_ytdlp:
            wx.CallLater(3500, self.start_ytdlp_update_check)
        if self.settings.auto_update_app:
            wx.CallLater(5500, self.start_app_update_check)

    def install_download_accelerators(self) -> None:
        self.download_audio_accelerator_id = wx.NewIdRef()
        self.download_video_accelerator_id = wx.NewIdRef()
        self.Bind(wx.EVT_MENU, lambda _evt: self.download_audio_shortcut(), id=int(self.download_audio_accelerator_id))
        self.Bind(wx.EVT_MENU, lambda _evt: self.download_video_shortcut(), id=int(self.download_video_accelerator_id))
        entries = [
            (wx.ACCEL_CTRL | wx.ACCEL_SHIFT, ord("A"), int(self.download_audio_accelerator_id)),
            (wx.ACCEL_CTRL | wx.ACCEL_SHIFT, ord("D"), int(self.download_video_accelerator_id)),
        ]
        self.SetAcceleratorTable(wx.AcceleratorTable(entries))

    def t(self, key: str, **kwargs) -> str:
        language = self.settings.language if self.settings.language in TEXT else "en"
        text = TEXT[language].get(key, TEXT["en"].get(key, key))
        return text.format(**kwargs) if kwargs else text

    def ydl_options(self, options: dict | None = None) -> dict:
        merged = {"logger": YTDLP_LOGGER, "no_warnings": True}
        if options:
            merged.update(options)
        cookiefile = str(merged.get("cookiefile") or self.settings.cookies_file).strip()
        if cookiefile:
            merged["cookiefile"] = cookiefile
        cookies_browser = self.normalized_cookies_browser()
        if cookies_browser and not cookiefile and "cookiesfrombrowser" not in merged:
            merged["cookiesfrombrowser"] = (cookies_browser,)
        return merged

    def normalized_cookies_browser(self) -> str:
        browser = str(getattr(self.settings, "cookies_from_browser", "none") or "none").strip().lower()
        return "" if browser == "none" else browser

    def friendly_error(self, exc: Exception | str) -> str:
        text = str(exc)
        lowered = text.lower()
        if "could not copy" in lowered and "cookie" in lowered and "database" in lowered:
            return f"{text}\n\n{self.t('cookie_copy_hint')}"
        if "sign in to confirm" in lowered or "not a bot" in lowered or "cookies-from-browser" in lowered:
            return f"{text}\n\n{self.t('youtube_auth_hint')}"
        return text

    def clear(self) -> None:
        self.root_sizer.Clear(delete_windows=True)

    def focus_later(self, control: wx.Window) -> None:
        wx.CallAfter(self.safe_set_focus, control)

    @staticmethod
    def safe_set_focus(control: wx.Window) -> None:
        try:
            if control and not getattr(control, "IsBeingDeleted", lambda: False)():
                control.SetFocus()
        except RuntimeError:
            pass

    def speak_text(self, text: str) -> None:
        if not text:
            return
        if self.nvda_client:
            try:
                if hasattr(self.nvda_client, "nvdaController_cancelSpeech"):
                    self.nvda_client.nvdaController_cancelSpeech()
                result = self.nvda_client.nvdaController_speakText(str(text))
                if result == 0:
                    return
            except Exception:
                self.nvda_client = None
        self.raise_accessibility_alert(text)

    def raise_accessibility_alert(self, text: str) -> None:
        self.SetName(text)
        try:
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_NAMECHANGE, self, wx.OBJID_CLIENT, 0)
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_SYSTEM_ALERT, self, wx.OBJID_ALERT, 0)
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_VALUECHANGE, self.status, wx.OBJID_CLIENT, 0)
        except Exception:
            pass

    def load_nvda_client(self):
        for path in self.nvda_client_candidates():
            try:
                if path.exists():
                    client = ctypes.WinDLL(str(path))
                    client.nvdaController_speakText.argtypes = [ctypes.c_wchar_p]
                    client.nvdaController_speakText.restype = ctypes.c_int
                    if hasattr(client, "nvdaController_cancelSpeech"):
                        client.nvdaController_cancelSpeech.argtypes = []
                        client.nvdaController_cancelSpeech.restype = ctypes.c_int
                    return client
            except Exception:
                continue
        return None

    def nvda_client_candidates(self) -> list[Path]:
        names = ["nvdaControllerClient64.dll", "nvdaControllerClient.dll"]
        roots = [
            self.bundled_path("nvda"),
            Path(__file__).resolve().parent / "vendor" / "nvda",
            Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "NVDA",
            Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "Bookworm" / "accessible_output2" / "lib",
            Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "TeamTalk5",
            Path(os.environ.get("ProgramFiles", r"C:\Program Files")) / "twblue" / "accessible_output2" / "lib",
            Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "RS Games Client" / "accessible_output2" / "lib",
        ]
        candidates = []
        for root in roots:
            for name in names:
                candidates.append(root / name)
        return candidates

    def add_button_row(self, buttons: list[tuple[str, callable]]) -> None:
        row = wx.BoxSizer(wx.HORIZONTAL)
        for label, handler in buttons:
            button = wx.Button(self.panel, label=label)
            button.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            row.Add(button, 0, wx.RIGHT, 6)
        self.root_sizer.Add(row, 0, wx.ALL, 4)

    def show_main_menu(self) -> None:
        self.in_queue_screen = False
        self.clear()
        title = wx.StaticText(self.panel, label=self.t("main_menu"))
        self.root_sizer.Add(title, 0, wx.ALL, 4)
        self.menu_actions = []
        if self.download_queue:
            self.menu_actions.append((f"{self.t('queued_videos_for_download')} ({len(self.download_queue)})", self.show_download_queue))
        self.menu_actions.extend([
            (self.t("search_youtube"), self.show_search),
            (self.t("choose_download_folder"), self.choose_download_folder),
            (self.t("favorites"), self.show_favorites),
            (self.t("settings"), self.show_settings),
            (self.t("exit"), self.Close),
        ])
        self.menu_list = wx.ListBox(self.panel, choices=[item[0] for item in self.menu_actions])
        self.menu_list.SetName(self.t("main_menu"))
        self.menu_list.SetSelection(0)
        self.menu_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.activate_menu())
        self.menu_list.Bind(wx.EVT_KEY_DOWN, self.on_menu_key)
        self.root_sizer.Add(self.menu_list, 1, wx.EXPAND | wx.ALL, 4)
        self.add_button_row([(self.t("open"), self.activate_menu)])
        self.panel.Layout()
        self.focus_later(self.menu_list)

    def on_menu_key(self, event: wx.KeyEvent) -> None:
        if event.GetKeyCode() == wx.WXK_RETURN:
            self.activate_menu()
            return
        event.Skip()

    def activate_menu(self) -> None:
        index = self.menu_list.GetSelection()
        if index != wx.NOT_FOUND:
            self.menu_actions[index][1]()

    def show_download_queue(self) -> None:
        self.in_queue_screen = True
        self.clear()
        buttons = [(self.t("back"), self.show_main_menu)]
        if self.download_queue:
            buttons.append((self.t("download_all"), self.download_all_queued))
        self.add_button_row(buttons)
        title = wx.StaticText(self.panel, label=self.t("queued_downloads"))
        self.root_sizer.Add(title, 0, wx.ALL, 4)
        instructions = wx.StaticText(self.panel, label=self.t("queued_download_instructions"))
        self.root_sizer.Add(instructions, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.queue_items = list(self.download_queue.values())
        self.queue_list = wx.ListBox(self.panel, choices=[self.queue_line(item) for item in self.queue_items])
        self.queue_list.SetName(self.t("queued_downloads"))
        self.queue_list.Bind(wx.EVT_CONTEXT_MENU, self.open_queue_context_menu)
        self.queue_list.Bind(wx.EVT_KEY_DOWN, self.on_queue_key)
        self.root_sizer.Add(self.queue_list, 1, wx.EXPAND | wx.ALL, 4)
        if self.queue_items:
            self.queue_list.SetSelection(0)
        else:
            empty = wx.StaticText(self.panel, label=self.t("no_queued_downloads"))
            self.root_sizer.Add(empty, 0, wx.ALL, 4)
        self.panel.Layout()
        self.focus_later(self.queue_list)

    def queue_line(self, item: dict) -> str:
        mode = self.t("audio_queued_marker" if item.get("audio_only") else "video_queued_marker")
        parts = [
            item.get("title", ""),
            f"{self.t('channel')}: {item.get('channel', '')}",
            mode,
        ]
        return " | ".join(part for part in parts if part)

    def selected_queue_item(self) -> dict | None:
        if not hasattr(self, "queue_list"):
            return None
        try:
            index = self.queue_list.GetSelection()
        except RuntimeError:
            return None
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.queue_items):
            return None
        return self.queue_items[index]

    def on_queue_key(self, event: wx.KeyEvent) -> None:
        key = event.GetKeyCode()
        if self.is_ctrl_shift_letter(event, "A"):
            self.download_selected_queue_item(True)
        elif self.is_ctrl_shift_letter(event, "D"):
            self.download_selected_queue_item(False)
        elif key == wx.WXK_RETURN:
            self.download_selected_queue_item()
        elif key == getattr(wx, "WXK_APPS", -1) or (key == wx.WXK_F10 and event.ShiftDown()):
            self.open_queue_context_menu()
        else:
            event.Skip()

    def open_queue_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("download_selected_queued"), lambda: self.download_selected_queue_item()),
            (f"{self.t('download_audio')}\tCtrl+Shift+A", lambda: self.download_selected_queue_item(True)),
            (f"{self.t('download_video')}\tCtrl+Shift+D", lambda: self.download_selected_queue_item(False)),
            (self.t("download_all"), self.download_all_queued),
            (self.t("remove_from_queue"), self.remove_selected_queue_item),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def show_search(self, restore_search: bool = False) -> None:
        self.in_queue_screen = False
        self.clear()
        self.add_button_row([(self.t("back"), self.show_main_menu)])
        grid = wx.FlexGridSizer(2, 2, 6, 6)
        grid.AddGrowableCol(1, 1)
        grid.Add(wx.StaticText(self.panel, label=self.t("search_query")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.query = wx.TextCtrl(self.panel, style=wx.TE_PROCESS_ENTER)
        self.query.SetName(self.t("search_query"))
        if restore_search:
            self.query.SetValue(self.last_search_query)
        self.query.Bind(wx.EVT_TEXT_ENTER, lambda _evt: self.search())
        grid.Add(self.query, 1, wx.EXPAND)
        grid.Add(wx.StaticText(self.panel, label=self.t("type")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.search_type = wx.Choice(
            self.panel,
            choices=[self.t("video"), self.t("playlist"), self.t("channel"), self.t("all")],
        )
        self.search_type.SetName(self.t("type"))
        self.search_type.SetSelection(self.last_search_type_index if restore_search else 0)
        grid.Add(self.search_type, 1, wx.EXPAND)
        self.root_sizer.Add(grid, 0, wx.EXPAND | wx.ALL, 4)
        self.add_button_row(
            [
                (self.t("search"), self.search),
                (self.t("play"), self.play_selected),
                (self.t("download_audio"), self.download_audio),
                (self.t("download_video"), self.download_video),
                (self.t("add_favorite"), self.add_selected_favorite),
            ]
        )
        self.results_list = wx.ListBox(self.panel, choices=[])
        self.results_list.SetName(self.t("search_youtube"))
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        if not restore_search:
            self.focus_later(self.query)

    def on_results_key(self, event: wx.KeyEvent) -> None:
        key = event.GetKeyCode()
        if self.is_shift_letter(event, "A") and not event.ControlDown():
            self.toggle_download_queue(True)
        elif self.is_shift_letter(event, "D") and not event.ControlDown():
            self.toggle_download_queue(False)
        elif self.is_ctrl_shift_letter(event, "A"):
            self.download_audio_shortcut()
        elif self.is_ctrl_shift_letter(event, "D"):
            self.download_video_shortcut()
        elif key == wx.WXK_RETURN:
            self.play_selected()
        elif key == getattr(wx, "WXK_APPS", -1) or (key == wx.WXK_F10 and event.ShiftDown()):
            self.open_context_menu()
        else:
            event.Skip()
            wx.CallAfter(self.maybe_extend_results)

    def on_results_selection(self, event) -> None:
        event.Skip()
        self.maybe_extend_results()

    def show_favorites(self) -> None:
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("play"), self.play_favorite),
                (self.t("remove"), self.remove_favorite),
                (self.t("refresh"), self.refresh_favorites),
            ]
        )
        self.favorites_list = wx.ListBox(self.panel, choices=[])
        self.favorites_list.SetName(self.t("favorites"))
        self.favorites_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_favorite())
        self.favorites_list.Bind(wx.EVT_KEY_DOWN, self.on_favorites_key)
        self.root_sizer.Add(self.favorites_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_favorites()
        self.panel.Layout()
        self.focus_later(self.favorites_list)

    def on_favorites_key(self, event: wx.KeyEvent) -> None:
        if event.GetKeyCode() == wx.WXK_RETURN:
            self.play_favorite()
        else:
            event.Skip()

    def show_settings(self) -> None:
        self.clear()
        self.add_button_row([(self.t("back"), self.show_main_menu), (self.t("save"), self.save_settings_from_ui), (self.t("restore_defaults"), self.restore_default_settings)])
        self.controls = {}
        self.settings_control_order = []
        sections = self.settings_sections()
        self.settings_section_index = min(max(0, self.settings_section_index), len(sections) - 1)
        body = wx.BoxSizer(wx.HORIZONTAL)
        self.settings_section_list = wx.ListBox(self.panel, choices=[label for label, _name in sections], style=wx.LB_SINGLE)
        self.settings_section_list.SetName(self.t("settings_sections"))
        self.settings_section_list.SetSelection(self.settings_section_index)
        self.settings_section_list.Bind(wx.EVT_LISTBOX, self.on_settings_section_changed)
        self.settings_section_list.Bind(wx.EVT_KEY_DOWN, self.on_settings_section_key)
        body.Add(self.settings_section_list, 0, wx.EXPAND | wx.ALL, 4)
        self.settings_scroller = wx.ScrolledWindow(self.panel, style=wx.VSCROLL | wx.WANTS_CHARS)
        self.settings_scroller.SetName(self.t("settings"))
        self.settings_scroller.SetScrollRate(10, 10)
        body.Add(self.settings_scroller, 1, wx.EXPAND | wx.ALL, 4)
        self.root_sizer.Add(body, 1, wx.EXPAND)
        self.render_settings_section()
        self.panel.Layout()
        self.focus_later(self.settings_section_list)

    def settings_sections(self) -> list[tuple[str, str]]:
        return [
            (self.t("general_section"), "general"),
            (self.t("playback_section"), "playback"),
            (self.t("downloads_section"), "downloads"),
            (self.t("cookies_network_section"), "cookies"),
            (self.t("updates_advanced_section"), "advanced"),
        ]

    def on_settings_section_changed(self, event) -> None:
        event.Skip()
        self.apply_settings_from_visible_controls()
        self.settings_section_index = self.settings_section_list.GetSelection()
        self.render_settings_section()

    def on_settings_section_key(self, event: wx.KeyEvent) -> None:
        if event.GetKeyCode() == wx.WXK_RETURN:
            self.focus_first_settings_control()
        else:
            event.Skip()

    def focus_first_settings_control(self) -> None:
        if self.settings_control_order:
            self.safe_set_focus(self.settings_control_order[0])

    def render_settings_section(self) -> None:
        if not hasattr(self, "settings_scroller"):
            return
        old_sizer = self.settings_scroller.GetSizer()
        if old_sizer:
            old_sizer.Clear(delete_windows=True)
        self.controls = {}
        self.choice_values = {}
        self.settings_control_order = []
        form = wx.FlexGridSizer(0, 2, 6, 6)
        form.AddGrowableCol(1, 1)
        section_name = self.settings_sections()[self.settings_section_index][1]

        def remember(key: str, ctrl: wx.Window) -> None:
            self.controls[key] = ctrl
            self.settings_control_order.append(ctrl)

        def text(key: str, value: str, style: int = 0):
            form.Add(wx.StaticText(self.settings_scroller, label=self.t(key)), 0, wx.ALIGN_CENTER_VERTICAL)
            ctrl = wx.TextCtrl(self.settings_scroller, value=value, style=style)
            ctrl.SetName(self.t(key))
            form.Add(ctrl, 1, wx.EXPAND)
            remember(key, ctrl)

        def choice(key: str, value: str, options: list[str], labels: list[str] | None = None):
            form.Add(wx.StaticText(self.settings_scroller, label=self.t(key)), 0, wx.ALIGN_CENTER_VERTICAL)
            visible_options = labels or options
            ctrl = wx.Choice(self.settings_scroller, choices=visible_options)
            ctrl.SetName(self.t(key))
            selected = options.index(value) if value in options else 0
            ctrl.SetSelection(selected)
            form.Add(ctrl, 1, wx.EXPAND)
            if labels:
                self.choice_values[key] = options
            remember(key, ctrl)

        def check(key: str, value: bool):
            form.AddSpacer(1)
            ctrl = wx.CheckBox(self.settings_scroller, label=self.t(key))
            ctrl.SetName(self.t(key))
            ctrl.SetValue(value)
            form.Add(ctrl, 1, wx.EXPAND)
            remember(key, ctrl)

        def button(key: str, handler):
            form.AddSpacer(1)
            ctrl = wx.Button(self.settings_scroller, label=self.t(key))
            ctrl.SetName(self.t(key))
            ctrl.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            form.Add(ctrl, 0)
            self.settings_control_order.append(ctrl)

        if section_name == "general":
            form.Add(wx.StaticText(self.settings_scroller, label=self.t("language")), 0, wx.ALIGN_CENTER_VERTICAL)
            language_code = self.settings.language if self.settings.language in LANGUAGE_CODES else "en"
            lang = wx.Choice(self.settings_scroller, choices=[name for _code, name in LANGUAGES])
            lang.SetSelection(LANGUAGE_CODES.index(language_code))
            lang.SetName(self.t("language"))
            form.Add(lang, 1, wx.EXPAND)
            remember("language", lang)
            text("settings_file", str(SETTINGS_FILE), wx.TE_READONLY)
            text("download_folder", self.settings.download_folder)
            button("browse", self.choose_download_folder)
            results_limit_value = "0" if self.settings.results_limit == 0 else str(min(250, self.settings.results_limit))
            choice("results_limit", results_limit_value, ["0", "10", "20", "50", "100", "150", "200", "250"])
            check("auto_update", self.settings.auto_update_ytdlp)
            check("auto_update_app", self.settings.auto_update_app)
            button("check_app_updates_now", self.manual_app_update_check)
        elif section_name == "playback":
            text("player_command", self.settings.player_command)
            choice("player_speed", self.settings.player_speed, [self.format_playback_rate(step) for step in PLAYBACK_SPEED_STEPS if step <= 2.0])
            choice("pitch_mode", self.normalized_pitch_mode(), PITCH_MODE_OPTIONS, self.pitch_mode_labels())
            choice("speed_step", self.format_step_value(self.settings.speed_step), RATE_STEP_OPTIONS)
            choice("pitch_step", self.format_step_value(self.settings.pitch_step), RATE_STEP_OPTIONS)
            choice("seek_seconds", str(self.settings.seek_seconds), ["5", "10", "15", "30"])
            choice("volume_step", str(self.settings.volume_step), ["1", "2", "5", "10"])
            check("autoplay_next", self.settings.autoplay_next)
            check("browser_playback", self.settings.prefer_browser_playback)
            check("fullscreen", self.settings.player_fullscreen)
            check("start_paused", self.settings.player_start_paused)
        elif section_name == "downloads":
            check("confirm_download", self.settings.confirm_before_download)
            check("open_after_download", self.settings.open_folder_after_download)
            check("download_complete_popup", self.settings.popup_when_download_complete)
            choice("audio_format", self.settings.audio_format, ["mp3", "m4a", "opus", "wav", "flac"])
            choice("audio_quality", self.settings.audio_quality, ["0", "1", "2", "3", "4", "5", "128", "192", "256", "320"])
            choice("video_format", self.normalized_video_format(), VIDEO_FORMAT_OPTIONS, self.video_format_labels())
            choice("max_height", str(self.settings.max_video_height), ["0", "360", "480", "720", "1080", "1440", "2160"])
            text("filename_template", self.settings.filename_template or DEFAULT_FILENAME_TEMPLATE)
            text("subtitle_langs", self.settings.subtitle_languages)
            check("quiet_downloads", self.settings.quiet_downloads)
            check("playlist_order", self.settings.keep_playlist_order)
            check("write_thumbnail", self.settings.write_thumbnail)
            check("write_description", self.settings.write_description)
            check("write_info_json", self.settings.write_info_json)
            check("write_subtitles", self.settings.write_subtitles)
            check("auto_subtitles", self.settings.auto_subtitles)
            check("embed_metadata", self.settings.embed_metadata)
            check("embed_thumbnail", self.settings.embed_thumbnail)
            check("restrict_filenames", self.settings.restrict_filenames)
            check("download_archive", self.settings.download_archive)
        elif section_name == "cookies":
            text("cookies", self.settings.cookies_file)
            choice("cookies_from_browser", self.settings.cookies_from_browser or "none", COOKIES_BROWSER_OPTIONS)
            button("export_browser_cookies", self.export_browser_cookies_from_settings)
            text("rate_limit", self.settings.rate_limit)
            text("proxy", self.settings.proxy)
            text("ffmpeg", self.settings.ffmpeg_location)
            choice("fragments", str(self.settings.concurrent_fragments), ["1", "2", "4", "8", "16"])
            choice("retries", str(self.settings.retries), ["0", "3", "5", "10", "20"])
            choice("timeout", str(self.settings.socket_timeout), ["5", "10", "20", "30", "60"])
        else:
            text("github_owner", self.settings.github_owner)
            text("github_repo", self.settings.github_repo)
            text("github_token", self.settings.github_token, wx.TE_PASSWORD)

        self.settings_scroller.SetSizer(form, True)
        self.settings_scroller.Layout()
        self.settings_scroller.FitInside()
        self.panel.Layout()

    def search_type_code(self) -> str:
        index = self.search_type.GetSelection()
        return ("Video", "Playlist", "Kanal", "All")[index if index != wx.NOT_FOUND else 0]

    def search(self) -> None:
        if get_yt_dlp() is None:
            self.message(self.t("missing_ytdlp"), wx.ICON_ERROR)
            return
        query = self.query.GetValue().strip()
        if not query:
            self.message(self.t("enter_query"))
            return
        self.last_search_query = query
        self.last_search_type_index = self.search_type.GetSelection()
        self.current_search_type_code = self.search_type_code()
        self.collection_url = ""
        self.collection_result_type = ""
        self.loading_more_results = False
        self.set_status(self.t("searching", query=query))
        threading.Thread(target=self.search_worker, args=(query, self.current_search_type_code, self.initial_results_limit()), daemon=True).start()

    def effective_results_limit(self) -> int:
        return min(250, max(1, self.settings.results_limit))

    def initial_results_limit(self) -> int:
        return RESULTS_PAGE_SIZE if self.settings.results_limit == 0 else self.effective_results_limit()

    def max_results_limit(self) -> int:
        return 250 if self.settings.results_limit == 0 else self.effective_results_limit()

    def search_worker(self, query: str, search_type: str, limit: int) -> None:
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": limit}
            with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
                if search_type == "Video":
                    info = ydl.extract_info(f"ytsearch{limit}:{query}", download=False)
                else:
                    info = ydl.extract_info(self.youtube_search_url(query, search_type), download=False)
            entries = list(info.get("entries") or [])[:limit]
            wx.CallAfter(self.show_results, [self.normalize_entry(entry, search_type) for entry in entries])
        except Exception as exc:
            wx.CallAfter(self.message, self.friendly_error(exc), wx.ICON_ERROR)

    def normalize_entry(self, entry: dict, search_type: str) -> dict:
        url = entry.get("webpage_url") or entry.get("url") or ""
        ie_key = str(entry.get("ie_key") or "").lower()
        entry_type = str(entry.get("_type") or entry.get("result_type") or "").lower()
        url_text = str(url)
        is_playlist = search_type == "Playlist" or "playlist" in ie_key or "playlist" in entry_type or "list=" in url_text
        is_channel = (
            search_type == "Kanal"
            or "channel" in ie_key
            or "channel" in entry_type
            or ("tab" in ie_key and not is_playlist)
            or "/channel/" in url_text
            or url_text.startswith("@")
            or url_text.startswith("/@")
        )
        if is_channel:
            kind = "channel"
            display_type = self.t("channel")
        elif is_playlist:
            kind = "playlist"
            display_type = self.t("playlist")
        else:
            kind = "video"
            display_type = self.t("video")
        if url and not url.startswith("http"):
            if kind == "playlist":
                clean_url = url.lstrip("/")
                if url.startswith("/") or "list=" in clean_url:
                    url = f"https://www.youtube.com/{clean_url}"
                else:
                    url = f"https://www.youtube.com/playlist?list={clean_url}"
            elif kind == "channel":
                url = f"https://www.youtube.com/{url.lstrip('/')}"
            else:
                url = f"https://www.youtube.com/watch?v={url}"
        return {
            "title": entry.get("title") or "",
            "channel": entry.get("uploader") or entry.get("channel") or "",
            "views": self.format_count(entry.get("view_count")),
            "view_count": entry.get("view_count"),
            "age": self.format_age(entry),
            "duration": self.format_duration(entry.get("duration")),
            "duration_seconds": entry.get("duration"),
            "timestamp": entry.get("timestamp"),
            "upload_date": entry.get("upload_date"),
            "description": entry.get("description") or "",
            "type": display_type,
            "kind": kind,
            "url": url,
        }

    def show_results(self, results: list[dict], selection: int = 0, visible_count: int | None = None) -> None:
        self.all_results = list(results)
        if self.settings.results_limit == 0:
            count = visible_count if visible_count is not None else min(RESULTS_PAGE_SIZE, len(self.all_results))
            self.last_visible_count = min(len(self.all_results), max(0, count))
            self.results = self.all_results[: self.last_visible_count]
        else:
            self.last_visible_count = len(self.all_results)
            self.results = list(self.all_results)
        self.results_list.Clear()
        for index, item in enumerate(self.results):
            self.results_list.Append(self.result_line(index, item))
        if self.results:
            selected_index = min(max(0, selection), len(self.results) - 1)
            self.results_list.SetSelection(selected_index)
            self.results_list.SetFocus()
        self.set_status(self.t("found", count=len(self.results)))

    def maybe_extend_results(self) -> None:
        if self.settings.results_limit != 0 or not hasattr(self, "results_list"):
            return
        selection = self.results_list.GetSelection()
        if selection == wx.NOT_FOUND:
            return
        if selection < len(self.results) - 1:
            return
        if len(self.results) >= len(self.all_results):
            self.fetch_more_dynamic_results(selection)
            return
        next_count = min(len(self.all_results), len(self.results) + RESULTS_PAGE_SIZE)
        self.show_results(self.all_results, selection=selection, visible_count=next_count)
        self.set_status(self.t("search_more_loaded", count=len(self.results)))

    def fetch_more_dynamic_results(self, selection: int) -> None:
        if self.loading_more_results:
            return
        current_count = len(self.all_results)
        if current_count >= self.max_results_limit():
            self.set_status(self.t("no_more_results"))
            return
        next_limit = min(self.max_results_limit(), current_count + RESULTS_PAGE_SIZE)
        self.loading_more_results = True
        self.set_status(self.t("loading_more_results"))
        if self.collection_url:
            threading.Thread(target=self.load_collection_worker, args=(self.collection_url, self.collection_result_type or "Video", next_limit, selection), daemon=True).start()
        else:
            threading.Thread(target=self.search_more_worker, args=(self.last_search_query, self.current_search_type_code, next_limit, selection), daemon=True).start()

    def search_more_worker(self, query: str, search_type: str, limit: int, selection: int) -> None:
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": limit}
            with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
                if search_type == "Video":
                    info = ydl.extract_info(f"ytsearch{limit}:{query}", download=False)
                else:
                    info = ydl.extract_info(self.youtube_search_url(query, search_type), download=False)
            entries = list(info.get("entries") or [])[:limit]
            wx.CallAfter(self.show_more_results, [self.normalize_entry(entry, search_type) for entry in entries], selection)
        except Exception as exc:
            wx.CallAfter(self.dynamic_fetch_failed, self.friendly_error(exc))

    def show_more_results(self, results: list[dict], selection: int) -> None:
        self.loading_more_results = False
        if len(results) <= len(self.all_results):
            self.set_status(self.t("no_more_results"))
            return
        self.show_results(results, selection=selection, visible_count=min(len(results), len(self.results) + RESULTS_PAGE_SIZE))
        self.set_status(self.t("search_more_loaded", count=len(self.results)))

    def dynamic_fetch_failed(self, error: str) -> None:
        self.loading_more_results = False
        self.message(error, wx.ICON_ERROR)

    def result_line(self, index: int, item: dict) -> str:
        parts = [
            item["title"],
            f"{self.t('channel')}: {item['channel']}",
            f"{self.t('views')}: {item['views']}",
            item.get("age", ""),
            item.get("duration", ""),
            item["type"],
        ]
        queued = self.download_queue.get(item.get("url", ""))
        if queued:
            parts.append(self.t("audio_queued_marker" if queued.get("audio_only") else "video_queued_marker"))
        return " | ".join(part for part in parts if part)

    def selected_result(self) -> dict | None:
        if not hasattr(self, "results_list"):
            return None
        try:
            index = self.results_list.GetSelection()
        except RuntimeError:
            return None
        if index == wx.NOT_FOUND:
            return None
        self.current_index = index
        return self.results[index]

    def play_selected(self) -> None:
        item = self.selected_result()
        if not item:
            self.message(self.t("no_selection"))
            return
        if item.get("kind") == "channel":
            self.open_channel_videos(item)
            return
        if item.get("kind") == "playlist":
            self.open_playlist_videos(item)
            return
        self.return_results = list(self.results)
        self.return_all_results = list(self.all_results or self.results)
        self.return_index = self.current_index
        self.return_visible_count = self.last_visible_count or len(self.results)
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(item["url"], item["title"])

    def open_channel_videos(self, item: dict) -> None:
        self.set_status(self.t("loading_channel", title=item["title"]))
        url = item["url"].rstrip("/")
        if not url.endswith("/videos"):
            url = f"{url}/videos"
        self.collection_url = url
        self.collection_result_type = "Video"
        self.loading_more_results = False
        threading.Thread(target=self.load_collection_worker, args=(url, "Video", self.initial_results_limit(), 0), daemon=True).start()

    def open_playlist_videos(self, item: dict) -> None:
        self.set_status(self.t("loading_playlist", title=item["title"]))
        self.collection_url = item["url"]
        self.collection_result_type = "Video"
        self.loading_more_results = False
        threading.Thread(target=self.load_collection_worker, args=(item["url"], "Video", self.initial_results_limit(), 0), daemon=True).start()

    def load_collection_worker(self, url: str, result_type: str, limit: int | None = None, selection: int = 0) -> None:
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            limit = limit or self.initial_results_limit()
            options = {
                "quiet": True,
                "extract_flat": True,
                "skip_download": True,
                "playlistend": limit,
            }
            with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
                info = ydl.extract_info(url, download=False)
            entries = list(info.get("entries") or [])[:limit]
            normalized = [self.normalize_entry(entry, result_type) for entry in entries]
            if self.settings.results_limit == 0 and selection:
                wx.CallAfter(self.show_more_results, normalized, selection)
            else:
                wx.CallAfter(self.show_results, normalized)
                wx.CallAfter(setattr, self, "loading_more_results", False)
        except Exception as exc:
            wx.CallAfter(self.dynamic_fetch_failed, self.friendly_error(exc))

    def play_url(self, url: str, title: str = "") -> None:
        player = self.resolve_player()
        if not player:
            self.message(self.t("player_missing"), wx.ICON_ERROR)
            return
        self.current_index = max(0, self.current_index)
        self.stop_player(silent=True)
        command, kind = player
        if kind != "mpv":
            self.message(self.t("player_missing"), wx.ICON_ERROR)
            return
        self.show_player_page(title)
        self.set_status(self.t("preparing_stream", title=title or url))
        threading.Thread(target=self.resolve_and_start_player, args=(command, url, title), daemon=True).start()

    def resolve_and_start_player(self, command: str, url: str, title: str) -> None:
        try:
            stream_url, headers, info = self.resolve_stream_url(url)
            wx.CallAfter(self.merge_current_video_info, info)
            wx.CallAfter(self.start_mpv, command, stream_url, title or url, headers)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("player_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def resolve_stream_url(self, url: str) -> tuple[str, dict, dict]:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            raise RuntimeError(self.t("missing_ytdlp"))
        options = {
            "quiet": True,
            "skip_download": True,
            "format": "best[ext=mp4]/best",
            "noplaylist": True,
        }
        with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
            info = ydl.extract_info(url, download=False)
        stream_url = info.get("url")
        if not stream_url and info.get("formats"):
            formats = [fmt for fmt in info["formats"] if fmt.get("url") and fmt.get("vcodec") != "none" and fmt.get("acodec") != "none"]
            if formats:
                stream_url = formats[-1]["url"]
        if not stream_url:
            raise RuntimeError("No playable stream URL found")
        return stream_url, info.get("http_headers") or {}, info

    def merge_current_video_info(self, info: dict) -> None:
        if not info:
            return
        self.current_video_info.update(
            {
                "title": info.get("title") or self.current_video_info.get("title", ""),
                "channel": info.get("uploader") or info.get("channel") or self.current_video_info.get("channel", ""),
                "url": info.get("webpage_url") or self.current_video_info.get("url", ""),
                "view_count": info.get("view_count", self.current_video_info.get("view_count")),
                "views": self.format_count(info.get("view_count", self.current_video_info.get("view_count"))),
                "timestamp": info.get("timestamp", self.current_video_info.get("timestamp")),
                "upload_date": info.get("upload_date", self.current_video_info.get("upload_date")),
                "age": self.format_age(info) or self.current_video_info.get("age", ""),
                "duration_seconds": info.get("duration", self.current_video_info.get("duration_seconds")),
                "duration": self.format_duration(info.get("duration", self.current_video_info.get("duration_seconds"))),
                "description": info.get("description") or self.current_video_info.get("description", ""),
            }
        )
        if self.current_video_item is not None:
            self.current_video_item.update(self.current_video_info)
        self.update_details_text()

    def start_mpv(self, command: str, stream_url: str, title: str, headers: dict) -> None:
        try:
            self.ipc_path = self.make_ipc_path()
            self.player_panel.Update()
            hwnd = self.player_panel.GetHandle()
            args = [
                command,
                f"--wid={hwnd}",
                "--force-window=yes",
                f"--input-ipc-server={self.ipc_path}",
                "--idle=no",
                "--volume-max=300",
                "--audio-pitch-correction=yes",
                "--pitch=1.0",
                f"--speed={self.settings.player_speed}",
                "--term-playing-msg=",
                "--msg-level=all=warn",
            ]
            if headers.get("User-Agent"):
                args.append(f"--user-agent={headers['User-Agent']}")
            if headers.get("Referer"):
                args.append(f"--referrer={headers['Referer']}")
            for name, value in headers.items():
                if name.lower() not in {"user-agent", "referer"} and value:
                    args.append(f"--http-header-fields-append={name}: {value}")
            if self.settings.player_fullscreen:
                args.append("--fullscreen=yes")
            if self.settings.player_start_paused:
                args.append("--pause=yes")
            args.append(stream_url)
            log_file = APP_DIR / "mpv.log"
            if self.player_log_handle:
                self.player_log_handle.close()
            self.player_log_handle = log_file.open("w", encoding="utf-8", errors="replace")
            creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
            self.player_process = subprocess.Popen(
                args,
                cwd=str(Path(command).parent),
                stdout=self.player_log_handle,
                stderr=subprocess.STDOUT,
                creationflags=creationflags,
            )
            self.player_kind = "mpv"
            self.player_control_mode = True
            self.volume_boost_enabled = False
            self.rubberband_pitch_filter_active = False
            self.current_video_info["speed"] = self.format_playback_rate(float(self.settings.player_speed))
            self.current_video_info["pitch"] = self.format_playback_rate(1.0)
            self.update_details_text()
            self.set_status(self.t("playing", title=title))
        except Exception as exc:
            self.message(self.t("player_failed", error=exc), wx.ICON_ERROR)

    def show_player_page(self, title: str) -> None:
        self.in_queue_screen = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back_results"), self.back_to_results),
                (self.t("play"), lambda: self.player_command("cycle pause")),
                (self.t("copy_link"), self.copy_active_url),
            ]
        )
        label = wx.StaticText(self.panel, label=f"{self.t('internal_player')}: {title}")
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.player_panel = wx.Panel(self.panel, style=wx.BORDER_SIMPLE | wx.WANTS_CHARS)
        self.player_panel.SetName(self.t("internal_player"))
        self.player_panel.SetBackgroundColour(wx.BLACK)
        self.player_panel.Bind(wx.EVT_KEY_DOWN, self.on_player_key)
        self.player_panel.Bind(wx.EVT_CHAR_HOOK, self.on_char_hook)
        self.root_sizer.Add(self.player_panel, 1, wx.EXPAND | wx.ALL, 4)
        self.details_label = None
        self.video_details = None
        self.in_player_screen = True
        self.player_control_mode = True
        self.panel.Layout()
        self.player_panel.SetFocus()

    def on_player_key(self, event: wx.KeyEvent) -> None:
        self.on_char_hook(event)

    def back_to_results(self) -> None:
        self.stop_player(silent=True)
        self.in_player_screen = False
        results = self.return_all_results or self.all_results or self.return_results or self.results
        if results:
            self.show_search(restore_search=True)
            self.show_results(results, selection=self.return_index, visible_count=self.return_visible_count)
            if results:
                index = min(max(0, self.return_index), len(results) - 1)
                self.current_index = index
                wx.CallAfter(self.focus_results_list, index)
        else:
            self.show_main_menu()

    def focus_results_list(self, index: int | None = None) -> None:
        if not hasattr(self, "results_list"):
            return
        try:
            if index is not None and self.results_list.GetCount():
                self.results_list.SetSelection(min(max(0, index), self.results_list.GetCount() - 1))
            self.safe_set_focus(self.results_list)
        except RuntimeError:
            pass

    def announce_player(self, text: str) -> None:
        self.set_status(text)
        self.speak_text(text)

    def show_video_details(self) -> None:
        if not self.in_player_screen:
            return
        if self.video_details is None:
            self.details_label = wx.StaticText(self.panel, label=self.t("video_details"))
            self.video_details = wx.TextCtrl(
                self.panel,
                style=wx.TE_MULTILINE | wx.TE_READONLY | wx.TE_RICH2 | wx.TE_DONTWRAP | wx.VSCROLL | wx.HSCROLL | wx.WANTS_CHARS,
            )
            self.video_details.SetName(self.t("video_details"))
            self.video_details.SetMinSize((-1, 160))
            self.root_sizer.Add(self.details_label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 4)
            self.root_sizer.Add(self.video_details, 0, wx.EXPAND | wx.ALL, 4)
        self.update_details_text()
        if self.details_label:
            self.details_label.Show()
        if self.video_details:
            self.video_details.Show()
        self.panel.Layout()
        self.video_details.SetInsertionPoint(0)
        self.safe_set_focus(self.video_details)
        self.announce_player(self.t("video_details"))

    def hide_video_details(self) -> None:
        if not self.video_details:
            return
        if self.details_label:
            self.details_label.Hide()
        self.video_details.Hide()
        self.panel.Layout()
        if hasattr(self, "player_panel"):
            self.safe_set_focus(self.player_panel)
        self.announce_player(self.t("details_closed"))

    def video_details_visible(self) -> bool:
        try:
            return bool(self.video_details and self.video_details.IsShown())
        except RuntimeError:
            return False

    def update_details_text(self) -> None:
        if not self.video_details:
            return
        details = self.build_video_details_text()
        self.video_details.Freeze()
        self.video_details.SetValue(details)
        self.video_details.SetInsertionPoint(0)
        self.video_details.Thaw()

    def build_video_details_text(self) -> str:
        info = self.current_video_info or {}
        if not info:
            return self.t("details_unavailable")
        lines = [
            info.get("title") or "",
            f"{self.t('channel')}: {info.get('channel') or ''}",
            f"{self.t('url')}: {info.get('url') or ''}",
            f"{self.t('views')}: {info.get('views') or self.format_count(info.get('view_count'))}",
            info.get("age") or self.format_age(info),
            f"{self.t('type')}: {info.get('type') or 'Video'}",
            f"Duration: {info.get('duration') or self.format_duration(info.get('duration_seconds'))}",
            f"Playback speed: {info.get('speed') or self.settings.player_speed}x",
            f"{self.t('pitch_label')}: {info.get('pitch') or '1.00'}x",
            f"{self.t('description')}:",
            info.get("description") or "",
        ]
        return "\n".join(line for line in lines if line is not None)

    def active_item(self) -> dict | None:
        if self.in_player_screen and self.current_video_item:
            return self.current_video_item
        if self.in_queue_screen:
            item = self.selected_queue_item()
            if item:
                return item
        return self.selected_result()

    def copy_url_to_clipboard(self, url: str) -> None:
        if not url:
            return
        if wx.TheClipboard.Open():
            try:
                wx.TheClipboard.SetData(wx.TextDataObject(url))
            finally:
                wx.TheClipboard.Close()
        self.announce_player(self.t("url_copied"))

    def copy_active_url(self) -> None:
        item = self.active_item()
        if item:
            self.copy_url_to_clipboard(item.get("url", ""))

    def player_command(self, command: str) -> None:
        if self.player_kind != "mpv" or not self.ipc_path:
            return
        try:
            self.mpv_send(shlex.split(command), timeout=0.5)
        except Exception:
            pass

    def mpv_process_alive(self) -> bool:
        return bool(self.player_process and self.player_process.poll() is None)

    def open_mpv_pipe(self, mode: str, timeout: float = MPV_IPC_TIMEOUT_SECONDS, buffering: int = -1, encoding: str | None = None):
        if self.player_kind != "mpv" or not self.ipc_path:
            raise RuntimeError("mpv is not running")
        deadline = time.monotonic() + max(0.0, timeout)
        last_error: Exception | None = None
        while True:
            if not self.mpv_process_alive():
                raise RuntimeError("mpv process is not running")
            try:
                if encoding is None:
                    return open(self.ipc_path, mode, buffering=buffering)
                return open(self.ipc_path, mode, buffering=buffering, encoding=encoding)
            except OSError as exc:
                last_error = exc
                if time.monotonic() >= deadline:
                    raise last_error
                time.sleep(0.04)

    def mpv_send(self, command: list, timeout: float = MPV_IPC_TIMEOUT_SECONDS) -> None:
        payload = json.dumps({"command": command}) + "\n"
        with self.mpv_ipc_lock:
            with self.open_mpv_pipe("w", timeout=timeout, encoding="utf-8") as pipe:
                pipe.write(payload)

    def mpv_request(self, command: list, timeout: float = MPV_IPC_TIMEOUT_SECONDS) -> dict:
        if self.player_kind != "mpv" or not self.ipc_path:
            return {}
        request_id = int(time.time() * 1000000)
        payload = (json.dumps({"command": command, "request_id": request_id}) + "\n").encode("utf-8")
        with self.mpv_ipc_lock:
            with self.open_mpv_pipe("r+b", timeout=timeout, buffering=0) as pipe:
                deadline = time.monotonic() + max(0.0, timeout)
                pipe.write(payload)
                while time.monotonic() < deadline:
                    raw = pipe.readline()
                    if not raw:
                        time.sleep(0.01)
                        continue
                    try:
                        response = json.loads(raw.decode("utf-8", errors="replace"))
                    except json.JSONDecodeError:
                        continue
                    if response.get("request_id") == request_id:
                        return response
        return {}

    def mpv_get_property(self, name: str, timeout: float = MPV_IPC_TIMEOUT_SECONDS):
        response = self.mpv_request(["get_property", name], timeout=timeout)
        return response.get("data")

    def mpv_set_property(self, name: str, value, timeout: float = MPV_IPC_TIMEOUT_SECONDS) -> None:
        response = self.mpv_request(["set_property", name, value], timeout=timeout)
        if not response or response.get("error") != "success":
            raise RuntimeError(str(response.get("error")))

    def announce_time_async(self) -> None:
        threading.Thread(target=self.announce_time_worker, daemon=True).start()

    def announce_time_worker(self) -> None:
        try:
            elapsed = self.mpv_get_property("time-pos")
            duration = self.mpv_get_property("duration")
            if elapsed is None or duration is None:
                wx.CallAfter(self.announce_player, self.t("timing_unavailable"))
                return
            remaining = max(0, float(duration) - float(elapsed))
            text = self.t(
                "time_announcement",
                elapsed=self.format_seconds(float(elapsed)),
                remaining=self.format_seconds(remaining),
                total=self.format_seconds(float(duration)),
            )
            wx.CallAfter(self.announce_player, text)
        except Exception:
            wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

    def change_speed_async(self, delta: float) -> None:
        threading.Thread(target=self.change_speed_worker, args=(delta,), daemon=True).start()

    def change_speed_worker(self, delta: float) -> None:
        try:
            current = self.mpv_get_property("speed")
            speed = float(current if current is not None else 1.0)
            speed = self.next_playback_speed(speed, delta)
            self.mpv_set_property("audio-pitch-correction", True)
            self.mpv_set_property("speed", speed)
            speed_text = self.format_playback_rate(speed)
            self.current_video_info["speed"] = speed_text
            wx.CallAfter(self.announce_player, self.t("speed_announcement", speed=self.format_rate_for_speech(speed)))
            if self.is_default_rate(speed):
                wx.CallAfter(self.play_default_sound)
            wx.CallAfter(self.update_details_text)
        except Exception:
            wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

    def change_pitch_async(self, delta: float) -> None:
        threading.Thread(target=self.change_pitch_worker, args=(delta,), daemon=True).start()

    def change_pitch_worker(self, delta: float) -> None:
        pitch = self.next_pitch_value(self.current_pitch_value(), delta)
        speed_delta = delta if self.normalized_pitch_mode() == PITCH_MODE_LINKED_SPEED else None
        for _attempt in range(MPV_PITCH_RETRY_ATTEMPTS):
            try:
                self.apply_pitch_value(pitch, speed_delta=speed_delta)
                wx.CallAfter(self.announce_player, self.t("pitch_announcement", pitch=self.format_rate_for_speech(pitch)))
                if self.is_default_rate(pitch):
                    wx.CallAfter(self.play_default_sound)
                wx.CallAfter(self.update_details_text)
                return
            except Exception:
                if not self.mpv_process_alive():
                    return
                time.sleep(MPV_PITCH_RETRY_DELAY_SECONDS)

    def current_pitch_value(self) -> float:
        stored = self.current_video_info.get("pitch", "1.0")
        try:
            return float(stored)
        except (TypeError, ValueError):
            return 1.0

    def apply_pitch_value(self, pitch: float, speed_delta: float | None = None) -> None:
        mode = self.normalized_pitch_mode()
        pitch_text = self.format_playback_rate(pitch)
        if mode == PITCH_MODE_MPV:
            self.clear_rubberband_pitch_filter()
            self.mpv_set_property("audio-pitch-correction", True)
            self.mpv_set_property("pitch", pitch)
        else:
            self.mpv_set_property("audio-pitch-correction", True)
            self.mpv_set_property("pitch", 1.0)
            if self.is_default_rate(pitch):
                self.clear_rubberband_pitch_filter()
            else:
                self.apply_rubberband_pitch_filter(pitch)
            if mode == PITCH_MODE_LINKED_SPEED and speed_delta is not None:
                current_speed = self.mpv_get_property("speed")
                speed = float(current_speed if current_speed is not None else 1.0)
                speed = self.next_playback_speed(speed, speed_delta)
                self.mpv_set_property("speed", speed)
                self.current_video_info["speed"] = self.format_playback_rate(speed)
        self.current_video_info["pitch"] = pitch_text

    def apply_rubberband_pitch_filter(self, pitch: float) -> None:
        if self.rubberband_pitch_filter_active:
            response = self.mpv_request(["af-command", RUBBERBAND_FILTER_LABEL, "set-pitch", f"{pitch:.4f}"])
            if response.get("error") == "success":
                return
            self.rubberband_pitch_filter_active = False
        self.clear_rubberband_pitch_filter()
        response = self.mpv_request(["af", "add", self.rubberband_pitch_filter(pitch)])
        if response.get("error") != "success":
            raise RuntimeError(str(response.get("error") or "rubberband filter not ready"))
        self.rubberband_pitch_filter_active = True

    @staticmethod
    def rubberband_pitch_filter(pitch: float) -> str:
        return f"{RUBBERBAND_FILTER_REF}:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer:pitch-scale={pitch:.4f}"

    def clear_rubberband_pitch_filter(self) -> None:
        try:
            self.mpv_request(["af", "remove", RUBBERBAND_FILTER_REF], timeout=0.8)
        finally:
            self.rubberband_pitch_filter_active = False

    def change_volume_async(self, delta: int) -> None:
        threading.Thread(target=self.change_volume_worker, args=(delta,), daemon=True).start()

    def change_volume_worker(self, delta: int) -> None:
        try:
            current = self.mpv_get_property("volume")
            volume = float(current if current is not None else 100.0)
            maximum = 300.0 if self.volume_boost_enabled else 100.0
            volume = min(max(0.0, volume + float(delta)), maximum)
            self.mpv_set_property("volume", volume)
        except Exception:
            pass

    def toggle_volume_boost(self) -> None:
        self.volume_boost_enabled = not self.volume_boost_enabled
        if self.volume_boost_enabled:
            self.announce_player(self.t("volume_boost_on"))
        else:
            threading.Thread(target=self.disable_volume_boost_worker, daemon=True).start()

    def disable_volume_boost_worker(self) -> None:
        try:
            current = self.mpv_get_property("volume")
            if current is not None and float(current) > 100.0:
                self.mpv_set_property("volume", 100.0)
        except Exception:
            pass
        wx.CallAfter(self.announce_player, self.t("volume_boost_off"))

    @staticmethod
    def next_playback_speed(current: float, delta: float) -> float:
        return MainFrame.clamp_rate(current + delta, 0.25, 4.0)

    @staticmethod
    def next_pitch_value(current: float, delta: float) -> float:
        return MainFrame.clamp_rate(current + delta, 0.5, 2.0)

    @staticmethod
    def clamp_rate(value: float, minimum: float, maximum: float) -> float:
        return round(min(max(value, minimum), maximum), 2)

    @staticmethod
    def next_step_value(current: float, delta: float, steps: list[float]) -> float:
        if delta < 0:
            for step in reversed(steps):
                if step < current - 0.001:
                    return step
            return steps[0]
        for step in steps:
            if step > current + 0.001:
                return step
        return steps[-1]

    @staticmethod
    def format_playback_rate(value: float) -> str:
        if abs(value - round(value)) < 0.001:
            return f"{value:.1f}"
        return f"{value:.2f}".rstrip("0").rstrip(".")

    @staticmethod
    def format_rate_for_speech(value: float) -> str:
        return f"{value:.2f}"

    @staticmethod
    def format_step_value(value: float) -> str:
        try:
            return f"{float(value):.2f}"
        except (TypeError, ValueError):
            return "0.01"

    def speed_step_value(self) -> float:
        return self.to_float(str(getattr(self.settings, "speed_step", 0.01)), 0.01, 0.01, 0.25)

    def pitch_step_value(self) -> float:
        return self.to_float(str(getattr(self.settings, "pitch_step", 0.01)), 0.01, 0.01, 0.25)

    def normalized_pitch_mode(self) -> str:
        mode = str(getattr(self.settings, "pitch_mode", PITCH_MODE_RUBBERBAND) or PITCH_MODE_RUBBERBAND)
        return self.normalize_pitch_mode_value(mode)

    def normalized_video_format(self) -> str:
        return self.normalize_video_format_value(getattr(self.settings, "video_format", VIDEO_FORMAT_MP4))

    def pitch_mode_labels(self) -> list[str]:
        return [
            self.t("pitch_mode_rubberband"),
            self.t("pitch_mode_mpv"),
            self.t("pitch_mode_linked_speed"),
        ]

    def video_format_labels(self) -> list[str]:
        return [
            self.t("video_format_mp4_recommended"),
            self.t("video_format_best_available"),
            self.t("video_format_mp4_single"),
            self.t("video_format_smallest"),
        ]

    @staticmethod
    def normalize_pitch_mode_value(mode: str) -> str:
        normalized = str(mode or "").strip()
        lowered = normalized.lower()
        if normalized in PITCH_MODE_OPTIONS:
            return normalized
        if lowered == LEGACY_PITCH_MODE_MPV:
            return PITCH_MODE_MPV
        if lowered == LEGACY_PITCH_MODE_LINKED_SPEED:
            return PITCH_MODE_LINKED_SPEED
        if lowered == LEGACY_PITCH_MODE_RUBBERBAND:
            return PITCH_MODE_RUBBERBAND
        return PITCH_MODE_RUBBERBAND

    @staticmethod
    def normalize_video_format_value(value: str) -> str:
        normalized = str(value or "").strip()
        if normalized in VIDEO_FORMAT_OPTIONS:
            return normalized
        return LEGACY_VIDEO_FORMAT_MAP.get(normalized, VIDEO_FORMAT_MP4)

    @staticmethod
    def is_default_rate(value: float) -> bool:
        return abs(value - 1.0) < 0.001

    def play_default_sound(self) -> None:
        if winsound is None:
            return
        sound_path = self.bundled_path("assets", DEFAULT_REACHED_SOUND)
        try:
            if sound_path.exists():
                winsound.PlaySound(str(sound_path), winsound.SND_FILENAME | winsound.SND_ASYNC)
            else:
                winsound.MessageBeep(winsound.MB_OK)
        except Exception:
            pass

    def stop_player(self, silent: bool = False) -> None:
        if self.player_process and self.player_process.poll() is None:
            self.player_process.terminate()
            try:
                self.player_process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.player_process.kill()
        self.player_process = None
        if self.player_log_handle:
            self.player_log_handle.close()
            self.player_log_handle = None
        self.player_kind = ""
        self.player_control_mode = False
        self.rubberband_pitch_filter_active = False
        if not self.in_player_screen:
            self.in_player_screen = False
        if not silent:
            self.set_status(self.t("stopped"))

    @staticmethod
    def key_event_codes(event: wx.KeyEvent) -> set[int]:
        codes: set[int] = set()
        for getter_name in ("GetKeyCode", "GetUnicodeKey", "GetRawKeyCode"):
            getter = getattr(event, getter_name, None)
            if not getter:
                continue
            try:
                code = int(getter())
            except Exception:
                continue
            if code not in (-1, 0, wx.WXK_NONE):
                codes.add(code)
        return codes

    @classmethod
    def key_event_matches_letter(cls, event: wx.KeyEvent, letter: str) -> bool:
        upper = letter.upper()
        lower = letter.lower()
        control_code = ord(upper) - ord("A") + 1
        wanted = {ord(upper), ord(lower), control_code}
        for code in cls.key_event_codes(event):
            if code in wanted:
                return True
            if 65 <= code <= 90 and chr(code) == upper:
                return True
            if 97 <= code <= 122 and chr(code) == lower:
                return True
        return False

    @staticmethod
    def is_shift_letter(event: wx.KeyEvent, letter: str) -> bool:
        if not event.ShiftDown():
            return False
        return MainFrame.key_event_matches_letter(event, letter)

    @staticmethod
    def is_ctrl_shift_letter(event: wx.KeyEvent, letter: str) -> bool:
        if not (event.ControlDown() and event.ShiftDown()):
            return False
        return MainFrame.key_event_matches_letter(event, letter)

    def on_char_hook(self, event: wx.KeyEvent) -> None:
        key = event.GetKeyCode()
        focus = wx.Window.FindFocus()
        details_has_focus = focus is self.video_details
        if key == wx.WXK_RETURN and focus is getattr(self, "menu_list", None):
            self.activate_menu()
            return
        if focus is getattr(self, "queue_list", None) and self.is_ctrl_shift_letter(event, "A"):
            self.download_selected_queue_item(True)
            return
        if focus is getattr(self, "queue_list", None) and self.is_ctrl_shift_letter(event, "D"):
            self.download_selected_queue_item(False)
            return
        if focus is getattr(self, "queue_list", None) and key == wx.WXK_RETURN:
            self.download_selected_queue_item()
            return
        if focus is getattr(self, "results_list", None) and self.is_shift_letter(event, "A") and not event.ControlDown():
            self.toggle_download_queue(True)
            return
        if focus is getattr(self, "results_list", None) and self.is_shift_letter(event, "D") and not event.ControlDown():
            self.toggle_download_queue(False)
            return
        if key == wx.WXK_RETURN and focus is getattr(self, "results_list", None):
            self.play_selected()
            return
        if self.is_ctrl_shift_letter(event, "A"):
            self.download_audio_shortcut()
            return
        if self.is_ctrl_shift_letter(event, "D"):
            self.download_video_shortcut()
            return
        if key == wx.WXK_ESCAPE:
            if self.in_player_screen and self.video_details_visible():
                self.hide_video_details()
                return
            if self.in_player_screen:
                self.back_to_results()
                return
            self.show_main_menu()
            return
        if self.in_player_screen and key in (ord("L"), ord("l")):
            self.copy_active_url()
            return
        if self.player_control_mode and not details_has_focus:
            if key == wx.WXK_F2:
                self.toggle_volume_boost()
                return
            if key == wx.WXK_SPACE:
                self.player_command("cycle pause")
                return
            if key in (ord("T"), ord("t")):
                self.announce_time_async()
                return
            if key in (ord("S"), ord("s")):
                self.change_speed_async(-self.speed_step_value())
                return
            if key in (ord("D"), ord("d")):
                self.change_speed_async(self.speed_step_value())
                return
            if key == wx.WXK_UP and event.ControlDown():
                self.change_pitch_async(self.pitch_step_value())
                return
            if key == wx.WXK_DOWN and event.ControlDown():
                self.change_pitch_async(-self.pitch_step_value())
                return
            if key in (ord("V"), ord("v")):
                self.show_video_details()
                return
            if key == wx.WXK_LEFT and event.ControlDown() and event.ShiftDown():
                self.player_command("seek -600")
                return
            if key == wx.WXK_RIGHT and event.ControlDown() and event.ShiftDown():
                self.player_command("seek 600")
                return
            if key == wx.WXK_LEFT and event.ControlDown():
                self.player_command("seek -60")
                return
            if key == wx.WXK_RIGHT and event.ControlDown():
                self.player_command("seek 60")
                return
            if key == wx.WXK_LEFT:
                self.player_command("seek -5")
                return
            if key == wx.WXK_RIGHT:
                self.player_command("seek 5")
                return
            if key == wx.WXK_UP:
                self.change_volume_async(self.settings.volume_step)
                return
            if key == wx.WXK_DOWN:
                self.change_volume_async(-self.settings.volume_step)
                return
        event.Skip()

    def open_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        item = self.selected_result()
        actions = [
            (self.t("play"), self.play_selected),
            (f"{self.t('download_audio')}\tCtrl+Shift+A", self.download_audio),
            (f"{self.t('download_video')}\tCtrl+Shift+D", self.download_video),
            (self.t("add_favorite"), self.add_selected_favorite),
            (self.t("open_browser"), self.open_selected_in_browser),
            (self.t("copy_url"), self.copy_selected_url),
        ]
        if item and item.get("kind") in {"playlist", "channel"}:
            label = self.t("download_channel" if item.get("kind") == "channel" else "download_playlist")
            actions.insert(1, (label, lambda selected=dict(item): self.download_collection(selected)))
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def start_download(self, audio_only: bool, item: dict | None = None, remove_queued: bool = False) -> None:
        item = item or self.active_item()
        if not item:
            self.message(self.t("no_selection"))
            return
        action = "audio" if audio_only else "video"
        if self.settings.confirm_before_download:
            if wx.MessageBox(self.t("download_confirm", action=action, title=item["title"]), APP_NAME, wx.YES_NO | wx.ICON_QUESTION) != wx.YES:
                self.set_status(self.t("download_cancelled"))
                return
        if remove_queued:
            self.remove_queued_url(item.get("url", ""), announce=False)
        self.announce_player(self.t("download_started"))
        self.set_status(self.t("download_audio_start" if audio_only else "download_video_start"))
        wx.CallLater(900, self.start_download_worker_thread, item, audio_only)

    def start_download_worker_thread(self, item: dict, audio_only: bool) -> None:
        threading.Thread(target=self.download_worker, args=(item, audio_only), daemon=True).start()

    def download_audio(self) -> None:
        self.start_download(True)

    def download_video(self) -> None:
        self.start_download(False)

    def download_collection(self, item: dict | None = None) -> None:
        item = item or self.selected_result()
        if not item or item.get("kind") not in {"playlist", "channel"}:
            self.message(self.t("no_selection"))
            return
        kind = str(item.get("kind") or "playlist")
        if self.settings.confirm_before_download:
            action = self.t("download_channel" if kind == "channel" else "download_playlist")
            if wx.MessageBox(self.t("download_confirm", action=action, title=item["title"]), APP_NAME, wx.YES_NO | wx.ICON_QUESTION) != wx.YES:
                self.set_status(self.t("download_cancelled"))
                return
        start_key = "download_channel_start" if kind == "channel" else "download_playlist_start"
        self.announce_player(self.t("download_started"))
        self.set_status(self.t(start_key))
        threading.Thread(target=self.download_collection_worker, args=(dict(item),), daemon=True).start()

    def download_audio_shortcut(self) -> None:
        self.start_download_shortcut(True)

    def download_video_shortcut(self) -> None:
        self.start_download_shortcut(False)

    def start_download_shortcut(self, audio_only: bool) -> None:
        item = self.active_item()
        url = str(item.get("url", "")) if item else ""
        kind = "audio" if audio_only else "video"
        now = time.monotonic()
        last_kind, last_url, last_time = self.last_download_shortcut
        if kind == last_kind and url == last_url and now - last_time < 0.35:
            return
        self.last_download_shortcut = (kind, url, now)
        self.start_download(audio_only, item=item)

    def download_worker(self, item: dict, audio_only: bool) -> None:
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            folder = Path(self.settings.download_folder)
            folder.mkdir(parents=True, exist_ok=True)
            options = self.download_options(folder, audio_only, item["title"])
            with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
                ydl.download([item["url"]])
            done_text = self.t("download_audio_done" if audio_only else "download_video_done", title=item["title"])
            wx.CallAfter(self.finish_download, done_text, str(folder))
        except Exception as exc:
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def download_collection_worker(self, item: dict) -> None:
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            kind = str(item.get("kind") or "playlist")
            title = item.get("title") or self.t("channel" if kind == "channel" else "playlist")
            folder = Path(self.settings.download_folder) / self.safe_folder_name(title)
            folder.mkdir(parents=True, exist_ok=True)
            options = self.download_options(folder, False, title, allow_playlist=True)
            with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
                ydl.download([self.collection_download_url(item)])
            done_key = "download_channel_done" if kind == "channel" else "download_playlist_done"
            wx.CallAfter(self.finish_download, self.t(done_key, title=title), str(folder))
        except Exception as exc:
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def finish_download(self, done_text: str, folder: str) -> None:
        if self.settings.popup_when_download_complete:
            self.set_status(done_text)
            self.message(done_text, wx.ICON_INFORMATION)
        else:
            self.announce_player(done_text)
        if self.settings.open_folder_after_download:
            os.startfile(folder)  # type: ignore[attr-defined]

    def download_options(self, folder: Path, audio_only: bool, title: str, allow_playlist: bool = False) -> dict:
        progress_hook = self.make_download_progress_hook(title, audio_only)
        template = self.settings.filename_template or DEFAULT_FILENAME_TEMPLATE
        if allow_playlist and "%(playlist_index)" not in template:
            template = "%(playlist_index)s - " + template
        options = {
            "outtmpl": str(folder / template),
            "quiet": self.settings.quiet_downloads,
            "noplaylist": False if allow_playlist else not self.settings.keep_playlist_order,
            "writethumbnail": self.settings.write_thumbnail,
            "writedescription": self.settings.write_description,
            "writeinfojson": self.settings.write_info_json,
            "writesubtitles": self.settings.write_subtitles,
            "writeautomaticsub": self.settings.auto_subtitles,
            "subtitleslangs": self.parse_csv(self.settings.subtitle_languages),
            "embedmetadata": self.settings.embed_metadata,
            "embedthumbnail": self.settings.embed_thumbnail,
            "restrictfilenames": self.settings.restrict_filenames,
            "concurrent_fragment_downloads": self.settings.concurrent_fragments,
            "retries": self.settings.retries,
            "socket_timeout": self.settings.socket_timeout,
            "progress_hooks": [progress_hook],
        }
        for key, value in (("ratelimit", self.settings.rate_limit), ("proxy", self.settings.proxy), ("cookiefile", self.settings.cookies_file), ("ffmpeg_location", self.settings.ffmpeg_location)):
            if value.strip():
                options[key] = value.strip()
        if "ffmpeg_location" not in options:
            bundled_ffmpeg = self.bundled_path("ffmpeg", "ffmpeg.exe")
            if bundled_ffmpeg.exists():
                options["ffmpeg_location"] = str(bundled_ffmpeg)
            else:
                ffmpeg = shutil.which("ffmpeg")
                if ffmpeg:
                    options["ffmpeg_location"] = ffmpeg
        if self.settings.download_archive:
            options["download_archive"] = str(APP_DIR / "download-archive.txt")
        if audio_only:
            options.update({"format": "bestaudio/best", "postprocessors": [{"key": "FFmpegExtractAudio", "preferredcodec": self.settings.audio_format, "preferredquality": self.settings.audio_quality}]})
        else:
            video_mode = self.normalized_video_format()
            options["format"] = self.video_format_selector(video_mode)
            if video_mode in {VIDEO_FORMAT_MP4, VIDEO_FORMAT_MP4_SINGLE, VIDEO_FORMAT_SMALLEST}:
                options["merge_output_format"] = "mp4"
        return options

    def video_format_selector(self, video_mode: str) -> str:
        height = self.settings.max_video_height if self.settings.max_video_height > 0 else 0
        limit = f"[height<={height}]" if height else ""
        if video_mode == VIDEO_FORMAT_BEST_ANY:
            return f"bestvideo{limit}+bestaudio/best{limit}/best"
        if video_mode == VIDEO_FORMAT_MP4_SINGLE:
            return f"best[ext=mp4]{limit}/best{limit}/best"
        if video_mode == VIDEO_FORMAT_SMALLEST:
            return f"worst[ext=mp4]{limit}/worst{limit}/worst"
        return f"bestvideo[ext=mp4]{limit}+bestaudio[ext=m4a]/best[ext=mp4]{limit}/bestvideo{limit}+bestaudio/best{limit}/best"

    @staticmethod
    def safe_folder_name(value: str) -> str:
        cleaned = re.sub(r'[<>:"/\\|?*\x00-\x1f]', " ", str(value or "").strip())
        cleaned = re.sub(r"\s+", " ", cleaned).strip(" .")
        return cleaned[:150] or "Download"

    @staticmethod
    def collection_download_url(item: dict) -> str:
        url = str(item.get("url") or "")
        if item.get("kind") == "channel":
            url = url.rstrip("/")
            if not url.endswith("/videos"):
                url = f"{url}/videos"
        return url

    def make_download_progress_hook(self, title: str, audio_only: bool):
        mode = self.t("download_audio_mode" if audio_only else "download_video_mode")

        def hook(data: dict) -> None:
            status = data.get("status")
            if status == "downloading":
                percent_text = str(data.get("_percent_str") or "").strip().replace("%", "")
                if not percent_text:
                    downloaded = data.get("downloaded_bytes") or 0
                    total = data.get("total_bytes") or data.get("total_bytes_estimate") or 0
                    if total:
                        percent_text = f"{(float(downloaded) / float(total)) * 100:.1f}"
                if percent_text:
                    self.ui_queue.put(("status", self.t("download_progress", mode=mode, percent=percent_text, title=title)))
            elif status == "finished":
                self.ui_queue.put(("status", self.t("download_processing", mode=mode, title=title)))

        return hook

    def add_selected_favorite(self) -> None:
        item = self.selected_result()
        if not item:
            self.message(self.t("no_selection"))
            return
        favorite = {"title": item["title"], "channel": item["channel"], "url": item["url"]}
        if any(existing["url"] == favorite["url"] for existing in self.favorites):
            self.set_status(self.t("favorite_exists"))
            return
        self.favorites.append(favorite)
        self.save_favorites()
        self.set_status(self.t("favorite_added"))

    def refresh_favorites(self) -> None:
        if not hasattr(self, "favorites_list"):
            return
        self.favorites_list.Clear()
        for index, item in enumerate(self.favorites):
            self.favorites_list.Append(f"{index + 1}. {item['title']} | {self.t('channel')}: {item['channel']}")
        if self.favorites:
            self.favorites_list.SetSelection(0)

    def selected_favorite(self) -> dict | None:
        index = self.favorites_list.GetSelection()
        return None if index == wx.NOT_FOUND else self.favorites[index]

    def play_favorite(self) -> None:
        item = self.selected_favorite()
        if item:
            self.current_video_item = item
            self.current_video_info = dict(item)
            self.play_url(item["url"], item["title"])

    def remove_favorite(self) -> None:
        index = self.favorites_list.GetSelection()
        if index != wx.NOT_FOUND:
            del self.favorites[index]
            self.save_favorites()
            self.refresh_favorites()
            self.set_status(self.t("favorite_removed"))

    def open_selected_in_browser(self) -> None:
        item = self.active_item()
        if item:
            webbrowser.open(item["url"])

    def copy_selected_url(self) -> None:
        item = self.active_item()
        if item:
            self.copy_url_to_clipboard(item["url"])

    def toggle_download_queue(self, audio_only: bool) -> None:
        item = self.selected_result()
        if not item:
            self.message(self.t("no_selection"))
            return
        url = item.get("url", "")
        if not url:
            self.message(self.t("no_selection"))
            return
        existing = self.download_queue.get(url)
        if existing and existing.get("audio_only") == audio_only:
            self.download_queue.pop(url, None)
            self.announce_player(self.t("download_deselected", title=item.get("title", "")))
        else:
            queued = dict(item)
            queued["audio_only"] = audio_only
            self.download_queue[url] = queued
            key = "audio_selected_download" if audio_only else "video_selected_download"
            self.announce_player(self.t(key, title=item.get("title", "")))
        self.refresh_result_line(self.current_index)

    def remove_queued_url(self, url: str, announce: bool = True) -> None:
        if not url:
            return
        item = self.download_queue.pop(url, None)
        if item and announce:
            self.announce_player(self.t("download_deselected", title=item.get("title", "")))
        self.refresh_results_list_labels()
        if self.in_queue_screen:
            self.refresh_queue_view()

    def refresh_result_line(self, index: int) -> None:
        if not hasattr(self, "results_list") or index < 0 or index >= len(self.results):
            return
        try:
            self.results_list.SetString(index, self.result_line(index, self.results[index]))
            self.results_list.SetSelection(index)
        except RuntimeError:
            pass

    def refresh_queue_view(self) -> None:
        if not self.in_queue_screen or not hasattr(self, "queue_list"):
            return
        if not self.download_queue:
            self.show_download_queue()
            return
        try:
            selection = self.queue_list.GetSelection()
            self.queue_items = list(self.download_queue.values())
            self.queue_list.Clear()
            for item in self.queue_items:
                self.queue_list.Append(self.queue_line(item))
            self.queue_list.SetSelection(min(max(0, selection), len(self.queue_items) - 1))
        except RuntimeError:
            pass

    def download_selected_queue_item(self, audio_only: bool | None = None) -> None:
        item = self.selected_queue_item()
        if not item:
            self.announce_player(self.t("download_queue_empty"))
            return
        if audio_only is None:
            audio_only = bool(item.get("audio_only"))
        self.start_download(audio_only, item=dict(item), remove_queued=True)

    def remove_selected_queue_item(self) -> None:
        item = self.selected_queue_item()
        if not item:
            self.announce_player(self.t("download_queue_empty"))
            return
        self.remove_queued_url(item.get("url", ""), announce=True)

    def download_all_queued(self) -> None:
        if not self.download_queue:
            self.announce_player(self.t("download_queue_empty"))
            return
        items = list(self.download_queue.values())
        self.download_queue.clear()
        self.refresh_results_list_labels()
        if self.in_queue_screen:
            self.show_download_queue()
        self.announce_player(self.t("batch_download_start", count=len(items)))
        threading.Thread(target=self.download_batch_worker, args=(items,), daemon=True).start()

    def refresh_results_list_labels(self) -> None:
        if not hasattr(self, "results_list"):
            return
        try:
            selection = self.results_list.GetSelection()
            self.results_list.Clear()
            for index, item in enumerate(self.results):
                self.results_list.Append(self.result_line(index, item))
            if self.results:
                self.results_list.SetSelection(min(max(0, selection), len(self.results) - 1))
        except RuntimeError:
            pass

    def download_batch_worker(self, items: list[dict]) -> None:
        folder = Path(self.settings.download_folder)
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            folder.mkdir(parents=True, exist_ok=True)
            for item in items:
                audio_only = bool(item.get("audio_only"))
                mode_key = "download_audio_start" if audio_only else "download_video_start"
                wx.CallAfter(self.announce_player, self.t(mode_key))
                options = self.download_options(folder, audio_only, item.get("title", ""))
                with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
                    ydl.download([item["url"]])
            wx.CallAfter(self.finish_batch_download, str(folder))
        except Exception as exc:
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def finish_batch_download(self, folder: str) -> None:
        done_text = self.t("batch_download_done")
        if self.settings.popup_when_download_complete:
            self.set_status(done_text)
            self.message(done_text, wx.ICON_INFORMATION)
        else:
            self.announce_player(done_text)
        if self.settings.open_folder_after_download:
            os.startfile(folder)  # type: ignore[attr-defined]

    def restore_default_settings(self) -> None:
        self.settings = Settings()
        self.save_settings()
        self.set_status(self.t("defaults_restored"))
        self.speak_text(self.t("defaults_restored"))
        self.show_settings()

    def choose_download_folder(self) -> None:
        with wx.DirDialog(self, self.t("choose_download_folder"), self.settings.download_folder) as dialog:
            if dialog.ShowModal() == wx.ID_OK:
                self.settings.download_folder = dialog.GetPath()
                if hasattr(self, "controls") and "download_folder" in self.controls:
                    self.controls["download_folder"].SetValue(self.settings.download_folder)
                self.save_settings()
                self.set_status(self.t("settings_saved"))

    def export_browser_cookies_from_settings(self) -> None:
        if get_yt_dlp() is None:
            self.message(self.t("missing_ytdlp"), wx.ICON_ERROR)
            return
        self.apply_settings_from_visible_controls()
        browser = self.normalized_cookies_browser()
        if not browser:
            self.message(self.t("select_cookies_browser"))
            return
        self.announce_player(self.t("exporting_browser_cookies"))
        threading.Thread(target=self.export_browser_cookies_worker, args=(browser,), daemon=True).start()

    def export_browser_cookies_worker(self, browser: str) -> None:
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            CACHED_COOKIES_FILE.parent.mkdir(parents=True, exist_ok=True)
            options = {
                "logger": YTDLP_LOGGER,
                "no_warnings": True,
                "quiet": True,
                "skip_download": True,
                "cookiefile": str(CACHED_COOKIES_FILE),
                "cookiesfrombrowser": (browser,),
            }
            with ytdlp.YoutubeDL(options) as ydl:
                _ = ydl.cookiejar
                ydl.save_cookies()
            wx.CallAfter(self.finish_browser_cookies_export, str(CACHED_COOKIES_FILE))
        except Exception as exc:
            wx.CallAfter(self.message, self.t("browser_cookies_export_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def finish_browser_cookies_export(self, path: str) -> None:
        self.settings.cookies_file = path
        self.settings.cookies_from_browser = "none"
        self.save_settings()
        if hasattr(self, "controls"):
            if "cookies" in self.controls:
                self.controls["cookies"].SetValue(path)
            if "cookies_from_browser" in self.controls:
                self.controls["cookies_from_browser"].SetSelection(0)
        self.announce_player(self.t("browser_cookies_exported", path=path))

    def save_settings_from_ui(self) -> None:
        old_language = self.settings.language
        self.apply_settings_from_visible_controls()
        self.save_settings()
        self.set_status(self.t("settings_saved"))
        if self.settings.language != old_language:
            self.show_settings()

    def apply_settings_from_visible_controls(self) -> None:
        c = self.controls
        if "language" in c:
            selected = c["language"].GetSelection()
            self.settings.language = LANGUAGE_CODES[selected] if 0 <= selected < len(LANGUAGE_CODES) else "en"
        if "download_folder" in c:
            self.settings.download_folder = c["download_folder"].GetValue()
        if "results_limit" in c:
            self.settings.results_limit = self.to_int(c["results_limit"].GetStringSelection(), 20, 0, 250)
        if "seek_seconds" in c:
            self.settings.seek_seconds = self.to_int(c["seek_seconds"].GetStringSelection(), 5, 1)
        if "volume_step" in c:
            self.settings.volume_step = self.to_int(c["volume_step"].GetStringSelection(), 5, 1)
        if "speed_step" in c:
            self.settings.speed_step = self.to_float(c["speed_step"].GetStringSelection(), 0.01, 0.01, 0.25)
        if "pitch_step" in c:
            self.settings.pitch_step = self.to_float(c["pitch_step"].GetStringSelection(), 0.01, 0.01, 0.25)
        if "auto_update" in c:
            self.settings.auto_update_ytdlp = c["auto_update"].GetValue()
        if "auto_update_app" in c:
            self.settings.auto_update_app = c["auto_update_app"].GetValue()
        if "autoplay_next" in c:
            self.settings.autoplay_next = c["autoplay_next"].GetValue()
        if "confirm_download" in c:
            self.settings.confirm_before_download = c["confirm_download"].GetValue()
        if "open_after_download" in c:
            self.settings.open_folder_after_download = c["open_after_download"].GetValue()
        if "download_complete_popup" in c:
            self.settings.popup_when_download_complete = c["download_complete_popup"].GetValue()
        if "audio_format" in c:
            self.settings.audio_format = c["audio_format"].GetStringSelection() or "mp3"
        if "audio_quality" in c:
            self.settings.audio_quality = c["audio_quality"].GetStringSelection() or "0"
        if "video_format" in c:
            self.settings.video_format = self.normalize_video_format_value(self.selected_choice_value("video_format"))
        if "max_height" in c:
            self.settings.max_video_height = self.to_int(c["max_height"].GetStringSelection(), 1080, 0)
        if "filename_template" in c:
            self.settings.filename_template = c["filename_template"].GetValue() or DEFAULT_FILENAME_TEMPLATE
        if "subtitle_langs" in c:
            self.settings.subtitle_languages = c["subtitle_langs"].GetValue() or "sl,en"
        if "quiet_downloads" in c:
            self.settings.quiet_downloads = c["quiet_downloads"].GetValue()
        if "playlist_order" in c:
            self.settings.keep_playlist_order = c["playlist_order"].GetValue()
        if "write_thumbnail" in c:
            self.settings.write_thumbnail = c["write_thumbnail"].GetValue()
        if "write_description" in c:
            self.settings.write_description = c["write_description"].GetValue()
        if "write_info_json" in c:
            self.settings.write_info_json = c["write_info_json"].GetValue()
        if "write_subtitles" in c:
            self.settings.write_subtitles = c["write_subtitles"].GetValue()
        if "auto_subtitles" in c:
            self.settings.auto_subtitles = c["auto_subtitles"].GetValue()
        if "embed_metadata" in c:
            self.settings.embed_metadata = c["embed_metadata"].GetValue()
        if "embed_thumbnail" in c:
            self.settings.embed_thumbnail = c["embed_thumbnail"].GetValue()
        if "restrict_filenames" in c:
            self.settings.restrict_filenames = c["restrict_filenames"].GetValue()
        if "download_archive" in c:
            self.settings.download_archive = c["download_archive"].GetValue()
        if "player_command" in c:
            self.settings.player_command = c["player_command"].GetValue()
        if "player_speed" in c:
            self.settings.player_speed = c["player_speed"].GetStringSelection() or "1.0"
        if "pitch_mode" in c:
            self.settings.pitch_mode = self.normalize_pitch_mode_value(self.selected_choice_value("pitch_mode"))
        if "browser_playback" in c:
            self.settings.prefer_browser_playback = c["browser_playback"].GetValue()
        if "fullscreen" in c:
            self.settings.player_fullscreen = c["fullscreen"].GetValue()
        if "start_paused" in c:
            self.settings.player_start_paused = c["start_paused"].GetValue()
        if "rate_limit" in c:
            self.settings.rate_limit = c["rate_limit"].GetValue()
        if "proxy" in c:
            self.settings.proxy = c["proxy"].GetValue()
        if "cookies" in c:
            self.settings.cookies_file = c["cookies"].GetValue()
        if "cookies_from_browser" in c:
            self.settings.cookies_from_browser = c["cookies_from_browser"].GetStringSelection() or "none"
        if "ffmpeg" in c:
            self.settings.ffmpeg_location = c["ffmpeg"].GetValue()
        if "github_owner" in c:
            self.settings.github_owner = c["github_owner"].GetValue().strip() or DEFAULT_GITHUB_OWNER
        if "github_repo" in c:
            self.settings.github_repo = c["github_repo"].GetValue().strip() or DEFAULT_GITHUB_REPO
        if "github_token" in c:
            self.settings.github_token = c["github_token"].GetValue().strip()
        if "fragments" in c:
            self.settings.concurrent_fragments = self.to_int(c["fragments"].GetStringSelection(), 4, 1)
        if "retries" in c:
            self.settings.retries = self.to_int(c["retries"].GetStringSelection(), 10, 0)
        if "timeout" in c:
            self.settings.socket_timeout = self.to_int(c["timeout"].GetStringSelection(), 20, 1)

    def selected_choice_value(self, key: str) -> str:
        ctrl = self.controls.get(key) if hasattr(self, "controls") else None
        if not isinstance(ctrl, wx.Choice):
            return ""
        values = getattr(self, "choice_values", {}).get(key)
        selection = ctrl.GetSelection()
        if values and 0 <= selection < len(values):
            return values[selection]
        return ctrl.GetStringSelection()

    def start_ytdlp_update_check(self) -> None:
        self.set_status(self.t("checking_updates"))
        threading.Thread(target=self.update_ytdlp_worker, daemon=True).start()

    def update_ytdlp_worker(self) -> None:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            self.ui_queue.put(("status", self.t("missing_ytdlp")))
            return
        try:
            with ytdlp.YoutubeDL(self.ydl_options({"quiet": True, "skip_download": True})) as ydl:
                from yt_dlp.update import run_update

                run_update(ydl)
            self.ui_queue.put(("status", self.t("updates_ok")))
        except Exception as exc:
            self.ui_queue.put(("status", self.t("updates_failed", error=exc)))

    def manual_app_update_check(self) -> None:
        self.apply_settings_from_visible_controls()
        self.save_settings()
        self.start_app_update_check(manual=True)

    def start_app_update_check(self, manual: bool = False) -> None:
        if not manual and not self.settings.auto_update_app:
            self.set_status(self.t("app_update_disabled"))
            return
        self.set_status(self.t("checking_app_updates"))
        if manual:
            self.announce_player(self.t("checking_app_updates"))
        threading.Thread(target=self.app_update_worker, args=(manual,), daemon=True).start()

    def app_update_worker(self, manual: bool = False) -> None:
        try:
            release = self.fetch_latest_release()
            if not release:
                self.report_app_update_status(self.t("app_up_to_date"), manual)
                return
            remote_version = self.release_version(release)
            if not self.is_newer_version(remote_version, APP_VERSION):
                self.report_app_update_status(self.t("app_up_to_date"), manual)
                return
            if not manual and remote_version == self.settings.skipped_update_version:
                self.report_app_update_status(self.t("update_skip_status", version=remote_version), manual)
                return
            asset = self.find_release_asset(release)
            if not asset:
                self.report_app_update_status(self.t("app_update_failed", error="no Windows asset found in release"), manual)
                return
            wx.CallAfter(self.prompt_for_app_update, release, asset)
        except Exception as exc:
            message = self.t("app_update_failed", error=exc)
            self.report_app_update_status(message, manual)
            if manual:
                wx.CallAfter(self.message, message, wx.ICON_ERROR)

    def report_app_update_status(self, message: str, manual: bool = False) -> None:
        self.ui_queue.put(("status", message))
        if manual:
            wx.CallAfter(self.announce_player, message)

    def prompt_for_app_update(self, release: dict, asset: dict) -> None:
        version = self.release_version(release)
        self.log_update_event(f"Prompting for update {version} with asset {asset.get('name')}")
        if not getattr(sys, "frozen", False):
            self.message(self.t("update_source_only", version=version))
            return
        changelog = self.release_changelog_text(release)
        if self.show_update_prompt(version, changelog):
            self.log_update_event(f"User selected update now for {version}")
            self.begin_app_update_install(release, asset)
        else:
            self.log_update_event(f"User skipped update {version}")
            self.settings.skipped_update_version = version
            self.save_settings()
            self.announce_player(self.t("update_skipped", version=version))

    def show_update_prompt(self, version: str, changelog: str) -> bool:
        dialog = wx.Dialog(self, title=self.t("update_available_title"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("update_available_title"))
        dialog.SetMinSize((640, 420))
        root = wx.BoxSizer(wx.VERTICAL)
        version_label = wx.StaticText(dialog, label=self.t("update_version_heading", version=version))
        root.Add(version_label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 10)
        intro = wx.StaticText(dialog, label=self.t("whats_new"))
        root.Add(intro, 0, wx.LEFT | wx.RIGHT | wx.TOP, 10)
        details = wx.TextCtrl(dialog, value=changelog, style=wx.TE_MULTILINE | wx.TE_READONLY | wx.TE_RICH2)
        details.SetName(self.t("whats_new"))
        details.SetMinSize((580, 260))
        root.Add(details, 1, wx.EXPAND | wx.LEFT | wx.RIGHT, 10)
        question = wx.StaticText(dialog, label=self.t("update_now"))
        root.Add(question, 0, wx.ALL, 10)
        buttons = wx.StdDialogButtonSizer()
        update_button = wx.Button(dialog, wx.ID_YES, self.t("update_now_button"))
        skip_button = wx.Button(dialog, wx.ID_NO, self.t("skip_version_button"))
        update_button.SetName(self.t("update_now_button"))
        skip_button.SetName(self.t("skip_version_button"))
        update_button.SetDefault()
        try:
            dialog.SetAffirmativeId(wx.ID_YES)
            dialog.SetEscapeId(wx.ID_NO)
        except Exception:
            pass
        update_button.Bind(wx.EVT_BUTTON, lambda _event: dialog.EndModal(wx.ID_YES))
        skip_button.Bind(wx.EVT_BUTTON, lambda _event: dialog.EndModal(wx.ID_NO))
        buttons.AddButton(update_button)
        buttons.AddButton(skip_button)
        buttons.Realize()
        root.Add(buttons, 0, wx.EXPAND | wx.ALL, 10)
        dialog.SetSizerAndFit(root)
        wx.CallAfter(self.safe_set_focus, details)
        try:
            return dialog.ShowModal() == wx.ID_YES
        finally:
            dialog.Destroy()

    def begin_app_update_install(self, release: dict, asset: dict) -> None:
        version = self.release_version(release)
        self.log_update_event(f"Beginning update {version}; asset={asset.get('name')}")
        self.close_update_progress_dialog()
        self.update_progress_dialog = wx.ProgressDialog(
            self.t("update_progress_title"),
            self.t("update_download_unknown", version=version),
            maximum=100,
            parent=self,
            style=wx.PD_APP_MODAL | wx.PD_ELAPSED_TIME | wx.PD_ESTIMATED_TIME,
        )
        self.update_progress_dialog.Pulse(self.t("update_download_unknown", version=version))
        self.announce_player(self.t("downloading_update", version=version))
        threading.Thread(target=self.download_and_install_update, args=(release, asset), daemon=True).start()

    def close_update_progress_dialog(self) -> None:
        if self.update_progress_dialog:
            try:
                self.update_progress_dialog.Destroy()
            except Exception:
                pass
            self.update_progress_dialog = None

    def update_app_update_progress(self, version: str, percent: int | None) -> None:
        if not self.update_progress_dialog:
            return
        try:
            if percent is None:
                self.update_progress_dialog.Pulse(self.t("update_download_unknown", version=version))
            else:
                percent = min(100, max(0, percent))
                self.update_progress_dialog.Update(percent, self.t("update_download_percent", version=version, percent=percent))
        except Exception:
            pass

    def update_app_update_finished(self, version: str) -> None:
        if self.update_progress_dialog:
            try:
                self.update_progress_dialog.Update(100, self.t("update_download_complete"))
            except Exception:
                pass
        self.announce_player(self.t("update_download_complete"))

    def update_app_update_failed(self, error: Exception | str) -> None:
        self.log_update_event(f"Update failed before install: {error}")
        self.close_update_progress_dialog()
        self.message(self.t("app_update_failed", error=error), wx.ICON_ERROR)

    def download_and_install_update(self, release: dict, asset: dict) -> None:
        version = self.release_version(release)
        try:
            self.ui_queue.put(("status", self.t("downloading_update", version=version)))
            temp_dir = Path(tempfile.mkdtemp(prefix="apricotplayer-update-"))
            downloaded_path = temp_dir / asset["name"]
            self.log_update_event(f"Downloading update {version} to {downloaded_path}")
            self.download_update_asset(asset, downloaded_path, version)
            self.log_update_event(f"Downloaded update {version}; size={downloaded_path.stat().st_size}")
            self.validate_update_package(downloaded_path)
            wx.CallAfter(self.update_app_update_finished, version)
            wx.CallAfter(self.finish_app_update_install, str(downloaded_path), version)
        except Exception as exc:
            wx.CallAfter(self.update_app_update_failed, exc)

    def download_update_asset(self, asset: dict, downloaded_path: Path, version: str) -> None:
        token = self.resolve_github_token()
        attempts: list[tuple[str, dict[str, str]]] = []
        browser_url = str(asset.get("browser_download_url") or "")
        api_url = str(asset.get("url") or "")
        if browser_url:
            attempts.append((browser_url, self.github_headers("", accept="application/octet-stream")))
        if api_url:
            attempts.append((api_url, self.github_headers(token, accept="application/octet-stream")))
        if not attempts:
            raise RuntimeError("missing download url")
        last_error: Exception | None = None
        for download_url, headers in attempts:
            try:
                request = Request(download_url, headers=headers)
                with urlopen(request, timeout=120) as response, downloaded_path.open("wb") as handle:
                    total_header = response.headers.get("Content-Length")
                    total = int(total_header) if total_header and total_header.isdigit() else 0
                    downloaded = 0
                    last_percent = -1
                    while True:
                        chunk = response.read(1024 * 512)
                        if not chunk:
                            break
                        handle.write(chunk)
                        downloaded += len(chunk)
                        if total:
                            percent = int(downloaded * 100 / total)
                            if percent != last_percent:
                                last_percent = percent
                                wx.CallAfter(self.update_app_update_progress, version, percent)
                        elif downloaded % (1024 * 1024 * 8) < len(chunk):
                            wx.CallAfter(self.update_app_update_progress, version, None)
                return
            except Exception as exc:
                last_error = exc
                try:
                    downloaded_path.unlink(missing_ok=True)
                except Exception:
                    pass
        raise RuntimeError(last_error or "download failed")

    def finish_app_update_install(self, downloaded_path: str, version: str) -> None:
        if not getattr(sys, "frozen", False):
            self.message(self.t("update_source_only", version=version))
            return
        current_exe = Path(sys.executable)
        self.log_update_event(f"Preparing install for {version}; package={downloaded_path}; current_exe={current_exe}")
        if self.is_installer_asset(downloaded_path):
            script_path = self.write_installer_update_script(downloaded_path, os.getpid(), str(UPDATE_LOG_FILE), restart=True)
        elif self.is_portable_zip_asset(downloaded_path):
            script_path = self.write_portable_zip_update_script(downloaded_path, str(current_exe.parent), str(current_exe), os.getpid(), str(UPDATE_LOG_FILE), restart=True)
        else:
            script_path = self.write_update_script(downloaded_path, str(current_exe), os.getpid(), str(UPDATE_LOG_FILE), restart=True)
        self.log_update_event(f"Launching update script {script_path}")
        self.launch_update_script(script_path)
        self.set_status(self.t("installing_update", version=version))
        self.close_update_progress_dialog()
        self.announce_player(self.t("update_install_started"))
        self.set_status(self.t("update_install_log", path=UPDATE_LOG_FILE))
        wx.CallLater(800, self.exit_for_update)

    @staticmethod
    def is_installer_asset(path_or_name: str | Path) -> bool:
        name = Path(path_or_name).name.lower()
        return name == INSTALLER_ASSET_NAME.lower() or "setup" in name or "installer" in name

    @staticmethod
    def is_portable_zip_asset(path_or_name: str | Path) -> bool:
        name = Path(path_or_name).name.lower()
        return name == PORTABLE_ZIP_ASSET_NAME.lower() or (name.endswith(".zip") and "portable" in name)

    @staticmethod
    def validate_update_package(path: Path) -> None:
        if not path.exists() or path.stat().st_size < 1024 * 1024:
            raise RuntimeError("downloaded update is not a valid package")
        if MainFrame.is_portable_zip_asset(path):
            if not zipfile.is_zipfile(path):
                raise RuntimeError("downloaded portable update is not a valid zip file")
            return
        with path.open("rb") as handle:
            if handle.read(2) != b"MZ":
                raise RuntimeError("downloaded update is not a Windows executable")

    @classmethod
    def write_update_script(cls, downloaded_path: str, target_path: str, process_id: int, log_path: str, restart: bool = True) -> Path:
        script_path = Path(tempfile.gettempdir()) / f"apricotplayer-update-{int(time.time())}.ps1"
        restart_value = "$true" if restart else "$false"
        script = "\n".join(
            [
                "$ErrorActionPreference = 'Stop'",
                f"$source = {cls.powershell_literal(downloaded_path)}",
                f"$target = {cls.powershell_literal(target_path)}",
                f"$log = {cls.powershell_literal(log_path)}",
                f"$processIdToWait = {int(process_id)}",
                f"$restart = {restart_value}",
                "$targetDir = Split-Path -Parent $target",
                "$oldTarget = \"$target.old\"",
                "New-Item -ItemType Directory -Path (Split-Path -Parent $log) -Force | Out-Null",
                "function Log($message) { Add-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' ' + $message) -Encoding UTF8 }",
                "Set-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' Starting ApricotPlayer update') -Encoding UTF8",
                "Log \"Source: $source\"",
                "Log \"Target: $target\"",
                "Start-Sleep -Milliseconds 500",
                "try { Wait-Process -Id $processIdToWait -Timeout 180 -ErrorAction SilentlyContinue } catch { Log \"Wait-Process warning: $($_.Exception.Message)\" }",
                "$copied = $false",
                "for ($attempt = 0; $attempt -lt 180; $attempt++) {",
                "    try {",
                "        if (Test-Path -LiteralPath $oldTarget) { Remove-Item -LiteralPath $oldTarget -Force -ErrorAction SilentlyContinue }",
                "        if (Test-Path -LiteralPath $target) { Rename-Item -LiteralPath $target -NewName (Split-Path -Leaf $oldTarget) -Force -ErrorAction Stop }",
                "        Copy-Item -LiteralPath $source -Destination $target -Force -ErrorAction Stop",
                "        if ((Get-Item -LiteralPath $target).Length -lt 1048576) { throw 'Copied file is too small.' }",
                "        $copied = $true",
                "        Log \"Copy succeeded on attempt $attempt\"",
                "        break",
                "    } catch {",
                "        Log \"Copy attempt $attempt failed: $($_.Exception.Message)\"",
                "        if ((Test-Path -LiteralPath $oldTarget) -and -not (Test-Path -LiteralPath $target)) {",
                "            try { Rename-Item -LiteralPath $oldTarget -NewName (Split-Path -Leaf $target) -Force -ErrorAction SilentlyContinue } catch { }",
                "        }",
                "        Start-Sleep -Seconds 1",
                "    }",
                "}",
                "if (-not $copied) { Log 'Update failed: could not copy new executable'; exit 1 }",
                "Remove-Item -LiteralPath $source -Force -ErrorAction SilentlyContinue",
                "if (Test-Path -LiteralPath $oldTarget) { Remove-Item -LiteralPath $oldTarget -Force -ErrorAction SilentlyContinue }",
                "if ($restart) { Log 'Restarting ApricotPlayer'; Start-Process -FilePath $target -WorkingDirectory $targetDir }",
                "Log 'Update complete'",
                "Start-Sleep -Seconds 2",
                "Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue",
            ]
        )
        script_path.write_text(script, encoding="utf-8-sig")
        return script_path

    @classmethod
    def write_portable_zip_update_script(cls, downloaded_path: str, target_dir: str, target_exe: str, process_id: int, log_path: str, restart: bool = True) -> Path:
        script_path = Path(tempfile.gettempdir()) / f"apricotplayer-portable-update-{int(time.time())}.ps1"
        restart_value = "$true" if restart else "$false"
        script = "\n".join(
            [
                "$ErrorActionPreference = 'Stop'",
                f"$source = {cls.powershell_literal(downloaded_path)}",
                f"$targetDir = {cls.powershell_literal(target_dir)}",
                f"$targetExe = {cls.powershell_literal(target_exe)}",
                f"$log = {cls.powershell_literal(log_path)}",
                f"$processIdToWait = {int(process_id)}",
                f"$restart = {restart_value}",
                "$extractRoot = Join-Path ([IO.Path]::GetTempPath()) ('apricotplayer-portable-' + [Guid]::NewGuid().ToString())",
                "New-Item -ItemType Directory -Path (Split-Path -Parent $log) -Force | Out-Null",
                "function Log($message) { Add-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' ' + $message) -Encoding UTF8 }",
                "Set-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' Starting ApricotPlayer portable update') -Encoding UTF8",
                "Log \"Source: $source\"",
                "Log \"Target directory: $targetDir\"",
                "Start-Sleep -Milliseconds 500",
                "try { Wait-Process -Id $processIdToWait -Timeout 180 -ErrorAction SilentlyContinue } catch { Log \"Wait-Process warning: $($_.Exception.Message)\" }",
                "try {",
                "    New-Item -ItemType Directory -Path $extractRoot -Force | Out-Null",
                "    Expand-Archive -LiteralPath $source -DestinationPath $extractRoot -Force",
                "    $sourceAppDir = Join-Path $extractRoot 'ApricotPlayer'",
                "    if (-not (Test-Path -LiteralPath (Join-Path $sourceAppDir 'ApricotPlayer.exe'))) {",
                "        $candidate = Get-ChildItem -LiteralPath $extractRoot -Filter 'ApricotPlayer.exe' -Recurse -File | Select-Object -First 1",
                "        if (-not $candidate) { throw 'ApricotPlayer.exe was not found in portable zip.' }",
                "        $sourceAppDir = Split-Path -Parent $candidate.FullName",
                "    }",
                "    Log \"Extracted app directory: $sourceAppDir\"",
                "    Get-ChildItem -LiteralPath $sourceAppDir -Force | ForEach-Object {",
                "        Copy-Item -LiteralPath $_.FullName -Destination $targetDir -Recurse -Force -ErrorAction Stop",
                "    }",
                "    if (-not (Test-Path -LiteralPath $targetExe)) { throw 'Updated ApricotPlayer.exe is missing after copy.' }",
                "    Remove-Item -LiteralPath $source -Force -ErrorAction SilentlyContinue",
                "    Remove-Item -LiteralPath $extractRoot -Recurse -Force -ErrorAction SilentlyContinue",
                "    if ($restart) { Log 'Restarting ApricotPlayer'; Start-Process -FilePath $targetExe -WorkingDirectory $targetDir }",
                "    Log 'Update complete'",
                "} catch {",
                "    Log \"Portable update failed: $($_.Exception.Message)\"",
                "    try { if (Test-Path -LiteralPath $extractRoot) { Remove-Item -LiteralPath $extractRoot -Recurse -Force -ErrorAction SilentlyContinue } } catch { }",
                "    exit 1",
                "}",
                "Start-Sleep -Seconds 2",
                "Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue",
            ]
        )
        script_path.write_text(script, encoding="utf-8-sig")
        return script_path

    @classmethod
    def write_installer_update_script(cls, downloaded_path: str, process_id: int, log_path: str, restart: bool = True) -> Path:
        script_path = Path(tempfile.gettempdir()) / f"apricotplayer-installer-update-{int(time.time())}.ps1"
        restart_value = "$true" if restart else "$false"
        script = "\n".join(
            [
                "$ErrorActionPreference = 'Stop'",
                f"$source = {cls.powershell_literal(downloaded_path)}",
                f"$log = {cls.powershell_literal(log_path)}",
                f"$processIdToWait = {int(process_id)}",
                f"$restart = {restart_value}",
                "$silentArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CLOSEAPPLICATIONS', '/TASKS=desktopicon')",
                "$installCandidates = @(",
                "    (Join-Path $env:ProgramFiles 'ApricotPlayer\\ApricotPlayer.exe')",
                ")",
                "if (${env:ProgramFiles(x86)}) { $installCandidates += (Join-Path ${env:ProgramFiles(x86)} 'ApricotPlayer\\ApricotPlayer.exe') }",
                "New-Item -ItemType Directory -Path (Split-Path -Parent $log) -Force | Out-Null",
                "function Log($message) { Add-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' ' + $message) -Encoding UTF8 }",
                "Set-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' Starting ApricotPlayer installer update') -Encoding UTF8",
                "Log \"Installer: $source\"",
                "Start-Sleep -Milliseconds 500",
                "try { Wait-Process -Id $processIdToWait -Timeout 180 -ErrorAction SilentlyContinue } catch { Log \"Wait-Process warning: $($_.Exception.Message)\" }",
                "try {",
                "    Log 'Launching installer'",
                "    $process = Start-Process -FilePath $source -ArgumentList $silentArgs -Verb runAs -Wait -PassThru",
                "    if ($process -and $process.ExitCode -ne 0) { throw \"Installer exited with code $($process.ExitCode)\" }",
                "    Log 'Installer completed'",
                "    Remove-Item -LiteralPath $source -Force -ErrorAction SilentlyContinue",
                "    if ($restart) {",
                "        foreach ($candidate in $installCandidates) {",
                "            if (Test-Path -LiteralPath $candidate) {",
                "                Log \"Restarting ApricotPlayer from $candidate\"",
                "                Start-Process -FilePath $candidate -WorkingDirectory (Split-Path -Parent $candidate)",
                "                break",
                "            }",
                "        }",
                "    }",
                "    Log 'Update complete'",
                "} catch {",
                "    Log \"Installer update failed: $($_.Exception.Message)\"",
                "    exit 1",
                "}",
                "Start-Sleep -Seconds 2",
                "Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue",
            ]
        )
        script_path.write_text(script, encoding="utf-8-sig")
        return script_path

    @staticmethod
    def launch_update_script(script_path: Path) -> None:
        powershell = shutil.which("powershell.exe") or shutil.which("pwsh.exe")
        if not powershell:
            raise RuntimeError("PowerShell was not found")
        args = [powershell, "-NoProfile"]
        if Path(powershell).name.lower() == "powershell.exe":
            args.extend(["-ExecutionPolicy", "Bypass"])
        args.extend(["-File", str(script_path)])
        subprocess.Popen(args, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0), close_fds=True)

    @staticmethod
    def powershell_literal(value: str) -> str:
        return "'" + value.replace("'", "''") + "'"

    @staticmethod
    def log_update_event(message: str) -> None:
        try:
            APP_DIR.mkdir(parents=True, exist_ok=True)
            line = f"{datetime.now(timezone.utc).isoformat()} {message}\n"
            with UPDATE_LOG_FILE.open("a", encoding="utf-8") as handle:
                handle.write(line)
        except Exception:
            pass

    def exit_for_update(self) -> None:
        try:
            self.Destroy()
            app = wx.GetApp()
            if app:
                app.ExitMainLoop()
        finally:
            os._exit(0)

    def fetch_latest_release(self) -> dict | None:
        owner = self.settings.github_owner.strip() or DEFAULT_GITHUB_OWNER
        repo = self.settings.github_repo.strip() or DEFAULT_GITHUB_REPO
        token = self.resolve_github_token()
        latest_request = Request(
            f"https://api.github.com/repos/{owner}/{repo}/releases/latest",
            headers=self.github_headers(token),
        )
        try:
            with urlopen(latest_request, timeout=30) as response:
                release = json.loads(response.read().decode("utf-8"))
            if isinstance(release, dict) and not release.get("draft"):
                return release
        except HTTPError as exc:
            if exc.code != 404:
                raise
        list_request = Request(
            f"https://api.github.com/repos/{owner}/{repo}/releases",
            headers=self.github_headers(token),
        )
        with urlopen(list_request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
        if isinstance(payload, list):
            releases = [release for release in payload if not release.get("draft")]
            releases.sort(key=lambda release: str(release.get("published_at") or release.get("created_at") or ""), reverse=True)
            return releases[0] if releases else None
        return None

    def find_release_asset(self, release: dict) -> dict | None:
        assets = release.get("assets") or []
        preferred_names = [INSTALLER_ASSET_NAME, PORTABLE_ZIP_ASSET_NAME] if self.is_installed_build() else [PORTABLE_ZIP_ASSET_NAME, INSTALLER_ASSET_NAME]
        for preferred_name in preferred_names:
            for asset in assets:
                if asset.get("name") == preferred_name:
                    return asset
        for predicate in (self.is_portable_zip_asset, self.is_installer_asset):
            for asset in assets:
                if predicate(str(asset.get("name") or "")):
                    return asset
        for asset in assets:
            name = str(asset.get("name") or "").lower()
            if name.endswith(".exe"):
                return asset
        return None

    @staticmethod
    def is_installed_build() -> bool:
        if not getattr(sys, "frozen", False):
            return False
        try:
            exe_path = Path(sys.executable).resolve()
        except Exception:
            exe_path = Path(sys.executable)
        if (exe_path.parent / "unins000.exe").exists():
            return True
        roots = [os.environ.get("ProgramFiles", ""), os.environ.get("ProgramFiles(x86)", "")]
        for root in roots:
            if not root:
                continue
            try:
                exe_path.relative_to(Path(root).resolve())
                return True
            except Exception:
                pass
        return False

    @staticmethod
    def release_version(release: dict) -> str:
        return str(release.get("tag_name") or release.get("name") or "").strip().lstrip("v")

    def release_changelog_text(self, release: dict) -> str:
        body = str(release.get("body") or "").replace("\r\n", "\n").strip()
        if not body:
            return self.t("no_changelog")
        if len(body) > 6000:
            return body[:6000].rstrip() + "\n\n..."
        return body

    @staticmethod
    def parse_version(value: str) -> tuple[int, int, int, int, int]:
        match = re.match(r"^v?(\d+)\.(\d+)(?:\.(\d+))?(?:-([A-Za-z]+)(?:[.-]?(\d+))?)?$", value.strip())
        if not match:
            return (0, 0, 0, 0, 0)
        major, minor, patch = (int(match.group(1)), int(match.group(2)), int(match.group(3) or 0))
        stage_name = (match.group(4) or "").lower()
        stage_number = int(match.group(5) or 0)
        stage_rank = {"alpha": 1, "beta": 2, "rc": 3}.get(stage_name, 4)
        return (major, minor, patch, stage_rank, stage_number)

    @classmethod
    def is_newer_version(cls, remote_version: str, current_version: str) -> bool:
        return cls.parse_version(remote_version) > cls.parse_version(current_version)

    @staticmethod
    def github_headers(token: str = "", accept: str = "application/vnd.github+json") -> dict[str, str]:
        headers = {
            "Accept": accept,
            "User-Agent": f"{APP_NAME}/{APP_VERSION}",
            "X-GitHub-Api-Version": "2022-11-28",
        }
        if token:
            headers["Authorization"] = f"Bearer {token}"
        return headers

    def resolve_github_token(self) -> str:
        configured = self.settings.github_token.strip()
        if configured:
            return configured
        gh_path = self.find_gh_executable()
        if not gh_path:
            return ""
        try:
            result = subprocess.run(
                [gh_path, "auth", "token"],
                capture_output=True,
                text=True,
                timeout=15,
                check=True,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
            return result.stdout.strip()
        except Exception:
            return ""

    @staticmethod
    def find_gh_executable() -> str:
        candidates = [
            shutil.which("gh"),
            r"C:\Program Files\GitHub CLI\gh.exe",
            str(Path.home() / "AppData" / "Local" / "Programs" / "GitHub CLI" / "gh.exe"),
        ]
        for candidate in candidates:
            if candidate and Path(candidate).exists():
                return str(candidate)
        return ""

    def process_queue(self, _event) -> None:
        try:
            while True:
                kind, payload = self.ui_queue.get_nowait()
                if kind == "results":
                    self.show_results(payload)
                elif kind == "status":
                    self.set_status(str(payload))
                elif kind == "error":
                    self.message(str(payload), wx.ICON_ERROR)
        except queue.Empty:
            pass

    def set_status(self, text: str) -> None:
        self.status.SetStatusText(text)

    def message(self, text: str, style=wx.ICON_INFORMATION) -> None:
        wx.MessageBox(text, APP_NAME, wx.OK | style)

    def resolve_player(self) -> tuple[str, str] | None:
        configured = self.settings.player_command.strip().strip('"')
        if configured:
            lower = configured.lower()
            if "mpv" in lower:
                return configured, "mpv"
        bundled = self.bundled_path("mpv", "mpv.exe")
        if bundled.exists():
            return str(bundled), "mpv"
        local = Path(__file__).resolve().parent / "vendor" / "mpv" / "mpv.exe"
        if local.exists():
            return str(local), "mpv"
        mpv = shutil.which("mpv")
        if mpv:
            return mpv, "mpv"
        return None

    @staticmethod
    def bundled_path(*parts: str) -> Path:
        base = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent))
        return base.joinpath(*parts)

    def load_settings(self) -> Settings:
        source = SETTINGS_FILE if SETTINGS_FILE.exists() else LEGACY_SETTINGS_FILE
        if source.exists():
            try:
                data = json.loads(source.read_text(encoding="utf-8"))
                merged = {**asdict(Settings()), **data}
                if merged.get("language") not in LANGUAGE_CODES:
                    merged["language"] = "en"
                if merged.get("filename_template") == OLD_FILENAME_TEMPLATE:
                    merged["filename_template"] = DEFAULT_FILENAME_TEMPLATE
                merged["pitch_mode"] = self.normalize_pitch_mode_value(str(merged.get("pitch_mode") or ""))
                merged["video_format"] = self.normalize_video_format_value(str(merged.get("video_format") or ""))
                return Settings(**merged)
            except Exception:
                return Settings()
        return Settings()

    def save_settings(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        SETTINGS_FILE.write_text(json.dumps(asdict(self.settings), indent=2, ensure_ascii=False), encoding="utf-8")

    def load_favorites(self) -> list[dict]:
        source = FAVORITES_FILE if FAVORITES_FILE.exists() else LEGACY_FAVORITES_FILE
        if source.exists():
            try:
                data = json.loads(source.read_text(encoding="utf-8"))
                return data if isinstance(data, list) else []
            except Exception:
                return []
        return []

    def save_favorites(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        FAVORITES_FILE.write_text(json.dumps(self.favorites, indent=2, ensure_ascii=False), encoding="utf-8")

    @staticmethod
    def youtube_search_url(query: str, search_type: str) -> str:
        filters = {"Playlist": "EgIQAw==", "Kanal": "EgIQAg=="}
        return f"https://www.youtube.com/results?{urlencode({'search_query': query, 'sp': filters.get(search_type, '')})}"

    @staticmethod
    def parse_csv(value: str) -> list[str]:
        return [part.strip() for part in value.split(",") if part.strip()]

    @staticmethod
    def to_int(value: str, default: int, minimum: int, maximum: int | None = None) -> int:
        try:
            number = max(minimum, int(value))
            return min(maximum, number) if maximum is not None else number
        except ValueError:
            return default

    @staticmethod
    def to_float(value: str, default: float, minimum: float, maximum: float | None = None) -> float:
        try:
            number = max(minimum, float(value))
            if maximum is not None:
                number = min(maximum, number)
            return round(number, 2)
        except (TypeError, ValueError):
            return default

    @staticmethod
    def format_count(value) -> str:
        if value is None:
            return ""
        try:
            number = int(value)
        except (TypeError, ValueError):
            return str(value)
        if number >= 1_000_000_000:
            return f"{number / 1_000_000_000:.1f}B"
        if number >= 1_000_000:
            return f"{number / 1_000_000:.1f}M"
        if number >= 1_000:
            return f"{number / 1_000:.1f}K"
        return str(number)

    @staticmethod
    def format_duration(seconds) -> str:
        if not seconds:
            return ""
        minutes, sec = divmod(int(seconds), 60)
        hours, minutes = divmod(minutes, 60)
        return f"{hours}:{minutes:02d}:{sec:02d}" if hours else f"{minutes}:{sec:02d}"

    @staticmethod
    def format_seconds(seconds: float | int | None) -> str:
        if seconds is None:
            return "0:00"
        total = max(0, int(seconds))
        minutes, sec = divmod(total, 60)
        hours, minutes = divmod(minutes, 60)
        return f"{hours}:{minutes:02d}:{sec:02d}" if hours else f"{minutes}:{sec:02d}"

    @staticmethod
    def format_age(entry: dict) -> str:
        timestamp = entry.get("timestamp")
        if not timestamp:
            upload_date = str(entry.get("upload_date") or "")
            if len(upload_date) == 8:
                try:
                    uploaded = datetime.strptime(upload_date, "%Y%m%d").replace(tzinfo=timezone.utc)
                    timestamp = int(uploaded.timestamp())
                except ValueError:
                    timestamp = None
        if timestamp:
            return f"uploaded {MainFrame.format_ago(int(timestamp))}"
        return ""

    @staticmethod
    def format_ago(timestamp: int) -> str:
        diff = max(0, int(time.time()) - int(timestamp))
        for name, size in (("year", 31536000), ("month", 2592000), ("day", 86400), ("hour", 3600), ("minute", 60)):
            if diff >= size:
                amount = diff // size
                return f"{amount} {name}{'' if amount == 1 else 's'} ago"
        return "just now"

    @staticmethod
    def make_ipc_path() -> str:
        return rf"\\.\pipe\urhasaurus-youtube-{os.getpid()}" if os.name == "nt" else f"/tmp/urhasaurus-youtube-{os.getpid()}.sock"


class App(wx.App):
    def OnInit(self) -> bool:
        frame = MainFrame()
        frame.Show()
        return True


def main() -> int:
    app = App(False)
    app.MainLoop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
