from __future__ import annotations

import json
import os
import queue
import re
import shlex
import shutil
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import webbrowser
import zipfile
import ctypes
from dataclasses import asdict, dataclass, field, fields
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from importlib import import_module
from pathlib import Path
from urllib.parse import urlencode, urljoin
from urllib.request import Request, urlopen
from urllib.error import HTTPError
import xml.etree.ElementTree as ET

import wx
import wx.adv

try:
    import certifi
except ImportError:
    certifi = None

from locales import EXTRA_TEXT, SL_TRANSLATION_FIXES

yt_dlp = None
yt_dlp_import_error: Exception | None = None
_SSL_CONTEXT: ssl.SSLContext | None = None


def get_yt_dlp():
    global yt_dlp, yt_dlp_import_error
    if yt_dlp is not None:
        return yt_dlp
    if yt_dlp_import_error is not None:
        return None
    try:
        components_dir = globals().get("COMPONENTS_DIR")
        if components_dir:
            components_path = Path(components_dir)
            if (components_path / "yt_dlp").exists():
                components_text = str(components_path)
                if components_text not in sys.path:
                    sys.path.insert(0, components_text)
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


class DownloadCancelled(Exception):
    pass


YTDLP_LOGGER = QuietYtdlpLogger()
APP_NAME = "ApricotPlayer"
APP_VERSION = "0.6.9"
APP_VERSION_LABEL = "0.6.9"
WINDOW_TITLE = f"{APP_NAME} {APP_VERSION_LABEL}"
LEGACY_APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "UrhasaurusYouTubePlayer"
APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "ApricotPlayer"
UPDATE_RELAUNCH_ARG = "--updated-relaunch"
UPDATE_RELAUNCH_SENTINEL = APP_DIR / "updated-relaunch.json"
ACTIVATE_SIGNAL_FILE = APP_DIR / "activate.json"
SETTINGS_FILE = APP_DIR / "settings.json"
FAVORITES_FILE = APP_DIR / "favorites.json"
HISTORY_FILE = APP_DIR / "history.json"
SUBSCRIPTIONS_FILE = APP_DIR / "subscriptions.json"
RSS_FEEDS_FILE = APP_DIR / "rss_feeds.json"
USER_PLAYLISTS_FILE = APP_DIR / "playlists.json"
NOTIFICATIONS_FILE = APP_DIR / "notifications.json"
PLAYBACK_POSITIONS_FILE = APP_DIR / "playback_positions.json"
CACHED_COOKIES_FILE = APP_DIR / "cookies.txt"
COMPONENTS_DIR = APP_DIR / "components"
LEGACY_SETTINGS_FILE = LEGACY_APP_DIR / "settings.json"
LEGACY_FAVORITES_FILE = LEGACY_APP_DIR / "favorites.json"
DEFAULT_DOWNLOAD_ROOT = Path.home() / "Downloads" / "ApricotPlayer"
DEFAULT_CACHE_DIR = APP_DIR / "cache"
DEFAULT_FILENAME_TEMPLATE = "%(title)s.%(ext)s"
OLD_FILENAME_TEMPLATE = "%(title)s [%(id)s].%(ext)s"
RESULTS_PAGE_SIZE = 20
DEFAULT_GITHUB_OWNER = "Urh2006"
DEFAULT_GITHUB_REPO = "ApricotPlayer"
GITHUB_RELEASES_API_URL = f"https://api.github.com/repos/{DEFAULT_GITHUB_OWNER}/{DEFAULT_GITHUB_REPO}/releases"
GITHUB_LATEST_RELEASE_API_URL = f"https://api.github.com/repos/{DEFAULT_GITHUB_OWNER}/{DEFAULT_GITHUB_REPO}/releases/latest"
INSTALLER_ASSET_NAME = "ApricotPlayerSetup.exe"
PORTABLE_ZIP_ASSET_NAME = "ApricotPlayer.zip"
LEGACY_PORTABLE_ZIP_ASSET_NAME = "ApricotPlayerPortable.zip"
UPDATE_LOG_FILE = APP_DIR / "updater.log"
YTDLP_PYPI_JSON_URL = "https://pypi.org/pypi/yt-dlp/json"
PLAYBACK_SPEED_STEPS = [0.25, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 1.0, 1.1, 1.2, 1.25, 1.3, 1.4, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0]
PITCH_STEPS = [0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95, 1.0, 1.05, 1.1, 1.15, 1.2, 1.25, 1.3, 1.35, 1.4, 1.45, 1.5, 1.6, 1.7, 1.8, 1.9, 2.0]
DEFAULT_REACHED_SOUND = "default_reached.wav"
PITCH_MODE_RUBBERBAND = "Independent pitch - advanced (Rubberband)"
PITCH_MODE_MPV = "Independent pitch - highest quality (mpv built-in)"
PITCH_MODE_LINKED_SPEED = "Linked pitch and speed - pitch keys change both"
PITCH_MODE_OPTIONS = [PITCH_MODE_RUBBERBAND, PITCH_MODE_MPV, PITCH_MODE_LINKED_SPEED]
LEGACY_PITCH_MODE_RUBBERBAND = "rubberband"
LEGACY_PITCH_MODE_MPV = "mpv pitch"
LEGACY_PITCH_MODE_LINKED_SPEED = "linked speed"
LEGACY_PITCH_MODE_RUBBERBAND_LABEL = "independent pitch - best quality (rubberband)"
LEGACY_PITCH_MODE_MPV_LABEL = "independent pitch - basic (mpv built-in)"
RUBBERBAND_FILTER_LABEL = "apricot_pitch"
RUBBERBAND_FILTER_REF = f"@{RUBBERBAND_FILTER_LABEL}"
RATE_STEP_OPTIONS = ["0.01", "0.02", "0.05", "0.10", "0.25"]
SPEED_AUDIO_MODE_SCALETEMPO2 = "High quality scaletempo2"
SPEED_AUDIO_MODE_MPV = "mpv default scaletempo2"
SPEED_AUDIO_MODE_SCALETEMPO = "Classic scaletempo"
SPEED_AUDIO_MODE_RUBBERBAND = "Rubberband high quality"
SPEED_AUDIO_MODE_OPTIONS = [SPEED_AUDIO_MODE_SCALETEMPO2, SPEED_AUDIO_MODE_MPV, SPEED_AUDIO_MODE_SCALETEMPO, SPEED_AUDIO_MODE_RUBBERBAND]
DIRECT_LINK_ENTER_PLAY = "play"
DIRECT_LINK_ENTER_AUDIO = "download_audio"
DIRECT_LINK_ENTER_VIDEO = "download_video"
DIRECT_LINK_ENTER_STREAM = "copy_stream_url"
DIRECT_LINK_ENTER_OPTIONS = [DIRECT_LINK_ENTER_PLAY, DIRECT_LINK_ENTER_AUDIO, DIRECT_LINK_ENTER_VIDEO, DIRECT_LINK_ENTER_STREAM]
COOKIES_BROWSER_OPTIONS = ["none", "chrome", "edge", "firefox", "brave", "chromium", "opera", "vivaldi"]
VIDEO_FORMAT_MP4 = "mp4"
VIDEO_FORMAT_BEST_ANY = "best-any"
VIDEO_FORMAT_MP4_SINGLE = "mp4-single"
VIDEO_FORMAT_SMALLEST = "smallest"
VIDEO_FORMAT_OPTIONS = [VIDEO_FORMAT_MP4, VIDEO_FORMAT_BEST_ANY, VIDEO_FORMAT_MP4_SINGLE, VIDEO_FORMAT_SMALLEST]
PODCAST_DIRECTORY_PROVIDER_APPLE = "apple"
PODCAST_DIRECTORY_PROVIDER_OPTIONS = [PODCAST_DIRECTORY_PROVIDER_APPLE]
PODCAST_COUNTRY_OPTIONS = ["US", "SI", "GB", "DE", "FR", "ES", "IT", "AT", "HR", "RS", "CA", "AU", "NL", "SE", "PL"]
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
    ("cs", "Czech"),
    ("sk", "Slovak"),
    ("hu", "Hungarian"),
    ("ro", "Romanian"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("ru", "Russian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese Simplified"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("id", "Indonesian"),
    ("fi", "Finnish"),
    ("el", "Greek"),
]
LANGUAGE_CODES = [code for code, _name in LANGUAGES]
LANGUAGE_NAMES = {code: name for code, name in LANGUAGES}

DEFAULT_KEYBOARD_SHORTCUTS = {
    "download_audio": "Ctrl+Shift+A",
    "download_video": "Ctrl+Shift+D",
    "subscribe_channel": "Ctrl+Shift+S",
    "queue_audio": "Shift+A",
    "queue_video": "Shift+D",
    "create_playlist": "Ctrl+Shift+N",
    "add_to_playlist": "Ctrl+Shift+P",
    "remove_from_playlist": "Ctrl+Shift+R",
    "copy_stream_url": "Ctrl+D",
    "context_menu": "Applications",
    "open_selected": "Enter",
    "new_subscription_videos": "N",
    "remove_selected": "Delete",
    "player_copy_link": "L",
    "player_play_pause": "Space",
    "player_time": "T",
    "player_speed_down": "S",
    "player_speed_up": "D",
    "player_pitch_up": "Ctrl+Up",
    "player_pitch_down": "Ctrl+Down",
    "player_details": "V",
    "player_output_devices": "O",
    "player_previous": "Ctrl+PageUp",
    "player_next": "Ctrl+PageDown",
    "player_back": "Escape",
    "player_volume_boost": "F2",
    "player_seek_back": "Left",
    "player_seek_forward": "Right",
    "player_seek_back_large": "Ctrl+Left",
    "player_seek_forward_large": "Ctrl+Right",
    "player_seek_back_huge": "Ctrl+Shift+Left",
    "player_seek_forward_huge": "Ctrl+Shift+Right",
    "player_volume_up": "Up",
    "player_volume_down": "Down",
}

SHORTCUT_DEFINITIONS = [
    ("download_audio", "shortcut_download_audio"),
    ("download_video", "shortcut_download_video"),
    ("subscribe_channel", "shortcut_subscribe_channel"),
    ("queue_audio", "shortcut_queue_audio"),
    ("queue_video", "shortcut_queue_video"),
    ("create_playlist", "shortcut_create_playlist"),
    ("add_to_playlist", "shortcut_add_to_playlist"),
    ("remove_from_playlist", "shortcut_remove_from_playlist"),
    ("copy_stream_url", "shortcut_copy_stream_url"),
    ("context_menu", "shortcut_context_menu"),
    ("open_selected", "shortcut_open_selected"),
    ("new_subscription_videos", "shortcut_new_subscription_videos"),
    ("remove_selected", "shortcut_remove_selected"),
    ("player_copy_link", "shortcut_player_copy_link"),
    ("player_play_pause", "shortcut_player_play_pause"),
    ("player_time", "shortcut_player_time"),
    ("player_speed_down", "shortcut_player_speed_down"),
    ("player_speed_up", "shortcut_player_speed_up"),
    ("player_pitch_up", "shortcut_player_pitch_up"),
    ("player_pitch_down", "shortcut_player_pitch_down"),
    ("player_details", "shortcut_player_details"),
    ("player_output_devices", "shortcut_player_output_devices"),
    ("player_previous", "shortcut_player_previous"),
    ("player_next", "shortcut_player_next"),
    ("player_back", "shortcut_player_back"),
    ("player_volume_boost", "shortcut_player_volume_boost"),
    ("player_seek_back", "shortcut_player_seek_back"),
    ("player_seek_forward", "shortcut_player_seek_forward"),
    ("player_seek_back_large", "shortcut_player_seek_back_large"),
    ("player_seek_forward_large", "shortcut_player_seek_forward_large"),
    ("player_seek_back_huge", "shortcut_player_seek_back_huge"),
    ("player_seek_forward_huge", "shortcut_player_seek_forward_huge"),
    ("player_volume_up", "shortcut_player_volume_up"),
    ("player_volume_down", "shortcut_player_volume_down"),
]


TEXT = {
    "sl": {
        "ready": "Pripravljen.",
        "main_menu": "Glavni meni",
        "download_all": "Download all",
        "download_all_selected": "Prenesi vse izbrane elemente",
        "queued_videos_for_download": "Queued videos for download",
        "queued_downloads": "Queued videos for download",
        "current_downloads": "Trenutni prenosi",
        "no_queued_downloads": "No queued downloads.",
        "queued_download_instructions": "Use Enter to download with the queued format, Ctrl+Shift+A for audio, Ctrl+Shift+D for video, or the context menu.",
        "download_selected_queued": "Download selected queued item",
        "remove_from_queue": "Remove from queue",
        "cancel_download": "Preklici prenos",
        "cancel_all_downloads": "Preklici vse prenose",
        "no_active_download": "Ni aktivnega prenosa.",
        "search_youtube": "Iskanje po YouTube",
        "direct_link": "Neposredna povezava",
        "direct_link_url": "URL za predvajanje ali prenos",
        "play_direct_link": "Predvajaj povezavo",
        "download_direct_audio": "Prenesi zvok iz povezave",
        "download_direct_video": "Prenesi video iz povezave",
        "choose_download_folder": "Izbor mape za prenose",
        "favorites": "Priljubljeni",
        "playlists": "Playliste",
        "create_playlist": "Ustvari playlisto",
        "playlist_name": "Ime playliste",
        "playlist_created": "Playlista ustvarjena: {title}.",
        "playlist_exists": "Playlista s tem imenom ze obstaja.",
        "playlist_empty": "Playlista je prazna.",
        "no_playlists": "Ni playlist.",
        "open_playlist": "Odpri playlisto",
        "remove_playlist": "Odstrani playlisto",
        "playlist_removed": "Playlista odstranjena.",
        "playlist_items": "Elementi playliste",
        "add_to_playlist": "Dodaj v playlisto",
        "added_to_playlist": "Dodano v playlisto {playlist}: {title}.",
        "added_to_playlist_count": "Dodano v playlisto {playlist}: {count} elementov.",
        "remove_from_playlist": "Odstrani iz playliste",
        "removed_from_playlist": "Odstranjeno iz playliste.",
        "download_user_playlist": "Prenesi playlisto",
        "copy_stream_url": "Kopiraj direktni media URL",
        "resolving_stream_url": "Pridobivam direktni media URL.",
        "stream_url_copied": "Direktni media URL je kopiran.",
        "stream_url_failed": "Direktnega media URL-ja ni bilo mogoce pridobiti: {error}",
        "notification_center": "Notification center",
        "notification_center_empty": "Ni obvestil.",
        "clear_notifications": "Pocisti obvestila",
        "notifications_cleared": "Obvestila so pociscena.",
        "notification_new_video": "{channel}: nov video {title}",
        "notification_new_podcast": "{feed}: nova epizoda {title}",
        "settings": "Nastavitve",
        "settings_sections": "Razdelki nastavitev",
        "general_section": "Splošno",
        "playback_section": "Predvajanje",
        "downloads_section": "Prenosi",
        "cookies_network_section": "Piškotki in omrežje",
        "updates_advanced_section": "Posodobitve in napredno",
        "keyboard_shortcuts_section": "Bliznjicne tipke",
        "keyboard_shortcuts_help": "Vnesi bliznjice v obliki Ctrl+Shift+A, Space, Enter, Left ali F2. Spremembe se shranijo s tipko Shrani.",
        "search_results_empty": "Ni rezultatov iskanja.",
        "no_results": "Ni rezultatov.",
        "favorites_empty": "Ni priljubljenih.",
        "empty": "Prazno.",
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
        "collection_audio_selected_download": "Zvok zbirke dodan v cakalno vrsto: {title}",
        "collection_video_selected_download": "Video zbirke dodan v cakalno vrsto: {title}",
        "download_deselected": "Removed from download queue: {title}",
        "download_queue_empty": "Download queue is empty.",
        "audio_queued_marker": "audio queued",
        "video_queued_marker": "video queued",
        "collection_audio_queued_marker": "zbirka kot zvok v cakalni vrsti",
        "collection_video_queued_marker": "zbirka kot video v cakalni vrsti",
        "download_state_queued": "V cakalni vrsti",
        "download_state_downloading": "Prenasam",
        "download_state_processing": "Obdelujem",
        "download_state_done": "Koncano",
        "download_state_cancelled": "Preklicano",
        "download_state_failed": "Neuspesno",
        "downloads_remaining": "preostalo {remaining} od {total}",
        "download_percent_value": "{percent} odstotkov",
        "download_cancel_requested": "Zahtevan preklic: {title}",
        "all_downloads_cancel_requested": "Zahtevan preklic vseh prenosov.",
        "details_unavailable": "Video details are not available yet.",
        "version": "Verzija",
        "description": "Description",
        "url": "URL",
        "uploaded": "uploaded",
        "uploaded_unknown": "Uploaded unknown",
        "dynamic_results": "Dinamicno (nalaga po 20 rezultatov)",
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
        "pitch_mode_rubberband": "Neodvisna visina tona - Rubberband (napredno)",
        "pitch_mode_mpv": "Neodvisna visina tona - MPV (najboljsa kakovost, priporoceno)",
        "pitch_mode_linked_speed": "Povezana visina tona in hitrost - tipke za visino tona spremenijo oboje",
        "speed_audio_mode": "Kakovost zvoka pri spremembi hitrosti",
        "speed_audio_mode_scaletempo2": "Visoka kakovost scaletempo2",
        "speed_audio_mode_mpv": "mpv privzeto scaletempo2",
        "speed_audio_mode_scaletempo": "Klasicni scaletempo",
        "speed_audio_mode_rubberband": "Rubberband visoka kakovost (priporoceno)",
        "auto_update": "Ob vsakem zagonu preveri posodobitve yt-dlp",
        "autoplay_next": "Po koncu posnetka samodejno predvajaj naslednjega",
        "show_video_details_by_default": "Privzeto pokazi podrobnosti videa v predvajalniku",
        "enable_stream_cache": "Omogoci predpomnilnik za predvajanje",
        "cache_folder": "Mapa za predpomnilnik",
        "cache_size_mb": "Velikost predpomnilnika v MB",
        "resume_playback": "Nadaljuj predvajanje tam, kjer si ostal",
        "default_audio_device": "Privzeta izhodna zvocna naprava",
        "audio_device_missing": "Shranjena izhodna zvocna naprava ni vec na voljo. Izberi novo privzeto napravo.",
        "output_devices": "Izhodne zvocne naprave",
        "select_output_device": "Izberi izhodno zvocno napravo",
        "output_device_set": "Izhodna zvocna naprava nastavljena: {device}.",
        "no_output_devices": "Ni najdenih izhodnih zvocnih naprav.",
        "repeat": "Ponavljaj",
        "repeat_on": "Ponavljanje vklopljeno.",
        "repeat_off": "Ponavljanje izklopljeno.",
        "playback_restarted": "Predvajanje znova od zacetka.",
        "playback_finished": "Predvajanje koncano.",
        "previous": "Prejsnji",
        "next": "Naslednji",
        "show_video_details": "Pokazi podrobnosti videa",
        "copy_details": "Kopiraj podrobnosti",
        "details_copied": "Podrobnosti so kopirane.",
        "no_previous_item": "Ni prejsnjega elementa.",
        "no_next_item": "Ni naslednjega elementa.",
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
        "shortcut_download_audio": "Prenesi zvok",
        "shortcut_download_video": "Prenesi video",
        "shortcut_subscribe_channel": "Naroci se na kanal",
        "shortcut_queue_audio": "Oznaci za prenos zvoka",
        "shortcut_queue_video": "Oznaci za prenos videa",
        "shortcut_create_playlist": "Ustvari playlisto",
        "shortcut_add_to_playlist": "Dodaj v playlisto",
        "shortcut_remove_from_playlist": "Odstrani iz playliste",
        "shortcut_copy_stream_url": "Kopiraj direktni media URL",
        "shortcut_context_menu": "Kontekstni meni",
        "shortcut_open_selected": "Odpri izbrano",
        "shortcut_new_subscription_videos": "Novi videi iz narocnine",
        "shortcut_remove_selected": "Odstrani izbrano",
        "shortcut_player_copy_link": "Predvajalnik: kopiraj povezavo",
        "shortcut_player_play_pause": "Predvajalnik: predvajaj ali pavza",
        "shortcut_player_time": "Predvajalnik: povej cas",
        "shortcut_player_speed_down": "Predvajalnik: pocasneje",
        "shortcut_player_speed_up": "Predvajalnik: hitreje",
        "shortcut_player_pitch_up": "Predvajalnik: visji ton",
        "shortcut_player_pitch_down": "Predvajalnik: nizji ton",
        "shortcut_player_details": "Predvajalnik: podrobnosti videa",
        "shortcut_player_output_devices": "Predvajalnik: izhodne naprave",
        "shortcut_player_previous": "Predvajalnik: prejsnji element",
        "shortcut_player_next": "Predvajalnik: naslednji element",
        "shortcut_player_back": "Predvajalnik: nazaj ali zapri podrobnosti",
        "shortcut_player_volume_boost": "Predvajalnik: ojacanje glasnosti",
        "shortcut_player_seek_back": "Predvajalnik: 5 sekund nazaj",
        "shortcut_player_seek_forward": "Predvajalnik: 5 sekund naprej",
        "shortcut_player_seek_back_large": "Predvajalnik: 1 minuto nazaj",
        "shortcut_player_seek_forward_large": "Predvajalnik: 1 minuto naprej",
        "shortcut_player_seek_back_huge": "Predvajalnik: 10 minut nazaj",
        "shortcut_player_seek_forward_huge": "Predvajalnik: 10 minut naprej",
        "shortcut_player_volume_up": "Predvajalnik: glasneje",
        "shortcut_player_volume_down": "Predvajalnik: tisje",
        "components_updating": "Posodabljam komponente.",
        "components_done": "Komponente so posodobljene.",
        "components_updated": "Komponente posodobljene.",
        "checking_updates": "Preverjam posodobitve za YouTube podporo.",
        "updates_ok": "YouTube podpora je posodobljena.",
        "updates_failed": "Posodobitve YouTube podpore ni bilo mogoče preveriti: {error}",
        "missing_ytdlp": "Manjka yt-dlp.",
    },
    "en": {
        "ready": "Ready.",
        "main_menu": "Main menu",
        "download_all": "Download all",
        "download_all_selected": "Download all selected items",
        "queued_videos_for_download": "Queued videos for download",
        "queued_downloads": "Queued videos for download",
        "current_downloads": "Current downloads",
        "no_queued_downloads": "No queued downloads.",
        "queued_download_instructions": "Use Enter to download with the queued format, Ctrl+Shift+A for audio, Ctrl+Shift+D for video, or the context menu.",
        "download_selected_queued": "Download selected queued item",
        "remove_from_queue": "Remove from queue",
        "cancel_download": "Cancel download",
        "cancel_all_downloads": "Cancel all downloads",
        "no_active_download": "No active download.",
        "search_youtube": "Search YouTube",
        "choose_download_folder": "Choose download folder",
        "favorites": "Favorites",
        "history": "History",
        "subscriptions": "Subscriptions",
        "rss_feeds": "Podcasts and RSS feeds",
        "rss_feed_items": "Feed items",
        "podcasts_section": "Podcasts and RSS",
        "enable_history": "Enable History and show it in the main menu",
        "enable_podcasts_rss": "Enable Podcasts and RSS feeds and show it in the main menu",
        "podcast_source": "Podcast source",
        "podcast_source_info": "Podcast search uses the Apple Podcasts directory through the iTunes Search API. Direct RSS and Atom feed URLs are always supported.",
        "podcast_search_provider": "Podcast search provider",
        "podcast_search_provider_apple": "Apple Podcasts directory",
        "podcast_search_country": "Podcast search country",
        "podcast_search_limit": "Podcast search results",
        "rss_max_items": "Maximum episodes per feed",
        "rss_refresh_on_startup": "Refresh podcast and RSS feeds at startup",
        "rss_auto_refresh_enabled": "Refresh podcast and RSS feeds automatically",
        "rss_refresh_interval": "Podcast and RSS refresh interval in hours",
        "search_podcasts": "Search podcasts",
        "podcast_search_query": "Podcast search",
        "podcast_search_results": "Podcast search results",
        "podcast_searching": "Searching podcasts: {query}",
        "podcast_search_done": "Found {count} podcasts.",
        "podcast_search_failed": "Podcast search failed: {error}",
        "podcast_search_empty": "No podcast search results.",
        "add_podcast": "Add podcast",
        "podcast_added": "Podcast added: {title}.",
        "podcast_author": "Author",
        "podcast_genre": "Genre",
        "podcast_episode_count": "{count} episodes",
        "add_rss_feed": "Add feed",
        "rss_feed_url": "Feed URL",
        "refresh_feeds": "Refresh feeds",
        "refresh_feed": "Refresh feed",
        "open_feed": "Open feed",
        "remove_feed": "Remove feed",
        "rss_feeds_empty": "No podcast or RSS feeds.",
        "rss_items_empty": "No items in this feed.",
        "rss_feed_added": "Feed added: {title}.",
        "rss_feed_exists": "This feed is already added.",
        "rss_feed_removed": "Feed removed.",
        "rss_refresh_started": "Refreshing feeds.",
        "rss_refresh_done": "Feeds refreshed.",
        "rss_refresh_failed": "Feed refresh failed: {error}",
        "rss_feed_last_checked": "last checked {time}",
        "rss_feed_never_checked": "never checked",
        "rss_feed_item_count": "{count} items",
        "podcast_episode": "Podcast episode",
        "play_episode": "Play episode",
        "download_episode_audio": "Download episode audio",
        "queue_episode_audio": "Queue episode audio download",
        "download_feed": "Download entire feed",
        "download_feed_start": "Downloading feed...",
        "download_feed_done": "Feed download complete: {title}",
        "open_episode_page": "Open episode page",
        "published": "published",
        "rss_unknown_feed_title": "Untitled feed",
        "settings": "Settings",
        "settings_sections": "Settings sections",
        "general_section": "General",
        "playback_section": "Playback",
        "downloads_section": "Downloads",
        "library_section": "Library and subscriptions",
        "cookies_network_section": "Cookies and network",
        "updates_advanced_section": "Updates and advanced",
        "notifications_section": "Notifications",
        "keyboard_shortcuts_section": "Keyboard shortcuts",
        "keyboard_shortcuts_help": "Focus a field and press the new key combination. Tab and Shift+Tab still move between fields. Press Save to keep changes.",
        "shortcut_capture_hint": "Press the new key combination. Tab and Shift+Tab move focus.",
        "shortcut_captured": "Shortcut set to {shortcut}.",
        "shortcut_in_use": "{shortcut} is already assigned to {action}. Choose a different shortcut.",
        "shortcut_in_use_title": "Shortcut already in use",
        "search_results_empty": "No search results.",
        "no_results": "No results.",
        "favorites_empty": "No favorites.",
        "empty": "Empty.",
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
        "collection_audio_selected_download": "Collection audio download queued: {title}",
        "collection_video_selected_download": "Collection video download queued: {title}",
        "download_deselected": "Removed from download queue: {title}",
        "download_queue_empty": "Download queue is empty.",
        "audio_queued_marker": "audio queued",
        "video_queued_marker": "video queued",
        "podcast_audio_queued_marker": "podcast audio queued",
        "podcast_episode_audio_selected_download": "Podcast episode queued: {title}",
        "collection_audio_queued_marker": "collection audio queued",
        "collection_video_queued_marker": "collection video queued",
        "download_state_queued": "Queued",
        "download_state_downloading": "Downloading",
        "download_state_processing": "Processing",
        "download_state_done": "Done",
        "download_state_cancelled": "Cancelled",
        "download_state_failed": "Failed",
        "downloads_remaining": "{remaining} of {total} remaining",
        "download_percent_value": "{percent} percent",
        "download_cancel_requested": "Cancel requested: {title}",
        "all_downloads_cancel_requested": "Cancel requested for all downloads.",
        "details_unavailable": "Video details are not available yet.",
        "version": "Version",
        "description": "Description",
        "url": "URL",
        "uploaded": "uploaded",
        "dynamic_results": "Dynamic (loads 20 at a time)",
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
        "direct_link_enter_action": "Kaj se zgodi, ko v Direct link pritisnes Enter",
        "direct_link_enter_play": "Predvajaj povezavo",
        "direct_link_enter_audio": "Prenesi zvok",
        "direct_link_enter_video": "Prenesi video",
        "direct_link_enter_stream": "Kopiraj direktni media URL",
        "settings_file": "Settings file",
        "restore_defaults": "Restore to defaults",
        "defaults_restored": "Default settings restored.",
        "loading_more_results": "Loading more results.",
        "no_more_results": "No more results.",
        "auto_update_app": "Check for updates at startup",
        "check_app_updates_now": "Check for updates",
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
        "clear_history": "Clear history",
        "remove_history_item": "Remove from history",
        "history_empty": "History is empty.",
        "history_cleared": "History cleared.",
        "history_removed": "History item removed.",
        "subscribe_channel": "Subscribe to channel",
        "unsubscribe_channel": "Unsubscribe from channel",
        "subscription_added": "Subscribed to {title}.",
        "subscription_exists": "Already subscribed to {title}.",
        "subscription_removed": "Subscription removed: {title}.",
        "subscription_empty": "No subscriptions yet.",
        "subscription_check_now": "Check subscriptions now",
        "subscription_checking": "Checking subscriptions.",
        "subscription_check_complete": "Subscription check complete.",
        "subscription_check_failed": "Could not check subscriptions: {error}",
        "subscription_no_new": "No new subscription videos.",
        "subscription_new_videos": "{count} new videos from {title}.",
        "subscription_new_videos_button": "New videos",
        "subscription_new_videos_title": "New videos from {title}",
        "subscription_no_saved_new_videos": "No new videos saved for this channel.",
        "subscription_open_videos": "Open channel videos",
        "open_channel_videos": "Open channel videos",
        "open_playlist_videos": "Open playlist videos",
        "subscription_last_checked": "last checked {time}",
        "subscription_never_checked": "never checked",
        "subscription_notifications": "Windows notifications for new subscription videos",
        "windows_notifications": "Windows notifications",
        "download_notifications": "Windows notifications for completed downloads when ApricotPlayer is not focused",
        "notification_download_title": "Download complete",
        "subscription_check_enabled": "Check subscriptions automatically",
        "subscription_check_interval": "Subscription check interval in hours",
        "close_to_tray": "Close button or Alt+F4 sends ApricotPlayer to system tray",
        "tray_notification": "Windows notification when ApricotPlayer goes to the system tray",
        "tray_still_running": "ApricotPlayer is still running in the system tray.",
        "already_open": "ApricotPlayer is already open.",
        "tray_show": "Show ApricotPlayer",
        "tray_settings": "Settings",
        "tray_check_subscriptions": "Check subscriptions",
        "tray_exit": "Exit ApricotPlayer",
        "notification_subscription_title": "New YouTube videos",
        "history_limit": "History limit",
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
        "pitch_mode_rubberband": "Independent pitch - Rubberband (advanced)",
        "pitch_mode_mpv": "Independent pitch - MPV (highest quality, recommended)",
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
        "shortcut_download_audio": "Download audio",
        "shortcut_download_video": "Download video",
        "shortcut_subscribe_channel": "Subscribe to channel",
        "shortcut_queue_audio": "Queue audio download",
        "shortcut_queue_video": "Queue video download",
        "shortcut_context_menu": "Context menu",
        "shortcut_open_selected": "Open selected item",
        "shortcut_new_subscription_videos": "New subscription videos",
        "shortcut_remove_selected": "Remove selected item",
        "shortcut_player_copy_link": "Player: copy link",
        "shortcut_player_play_pause": "Player: play or pause",
        "shortcut_player_time": "Player: announce time",
        "shortcut_player_speed_down": "Player: slower",
        "shortcut_player_speed_up": "Player: faster",
        "shortcut_player_pitch_up": "Player: pitch up",
        "shortcut_player_pitch_down": "Player: pitch down",
        "shortcut_player_details": "Player: video details",
        "shortcut_player_back": "Player: back or close details",
        "shortcut_player_volume_boost": "Player: volume boost",
        "shortcut_player_seek_back": "Player: seek back 5 seconds",
        "shortcut_player_seek_forward": "Player: seek forward 5 seconds",
        "shortcut_player_seek_back_large": "Player: seek back 1 minute",
        "shortcut_player_seek_forward_large": "Player: seek forward 1 minute",
        "shortcut_player_seek_back_huge": "Player: seek back 10 minutes",
        "shortcut_player_seek_forward_huge": "Player: seek forward 10 minutes",
        "shortcut_player_volume_up": "Player: volume up",
        "shortcut_player_volume_down": "Player: volume down",
        "components_updating": "Updating components.",
        "components_done": "Components are up to date.",
        "components_updated": "Components updated.",
        "checking_updates": "Checking updates for YouTube support.",
        "updates_ok": "YouTube support is up to date.",
        "updates_failed": "Could not check YouTube support updates: {error}",
        "missing_ytdlp": "yt-dlp is missing.",
    },
}

TEXT["sl"].update(SL_TRANSLATION_FIXES)
TEXT.update(EXTRA_TEXT)
TEXT["en"].update(
    {
        "direct_link": "Direct link",
        "direct_link_url": "URL to play or download",
        "play_direct_link": "Play link",
        "download_direct_audio": "Download link audio",
        "download_direct_video": "Download link video",
        "playlists": "Playlists",
        "create_playlist": "Create playlist",
        "playlist_name": "Playlist name",
        "playlist_created": "Playlist created: {title}.",
        "playlist_exists": "A playlist with that name already exists.",
        "playlist_empty": "Playlist is empty.",
        "no_playlists": "No playlists.",
        "open_playlist": "Open playlist",
        "remove_playlist": "Remove playlist",
        "playlist_removed": "Playlist removed.",
        "playlist_items": "Playlist items",
        "add_to_playlist": "Add to playlist",
        "select_playlist": "Select playlist",
        "added_to_playlist": "Added to playlist {playlist}: {title}.",
        "added_to_playlist_count": "Added {count} items to playlist {playlist}.",
        "remove_from_playlist": "Remove from playlist",
        "removed_from_playlist": "Removed from playlist.",
        "download_user_playlist": "Download playlist",
        "copy_stream_url": "Copy direct media URL",
        "resolving_stream_url": "Resolving direct media URL.",
        "stream_url_copied": "Direct media URL copied.",
        "stream_url_failed": "Could not resolve direct media URL: {error}",
        "notification_center": "Notification center",
        "notification_center_empty": "No notifications.",
        "clear_notifications": "Clear notifications",
        "notifications_cleared": "Notifications cleared.",
        "notification_new_video": "{channel}: new video {title}",
        "notification_new_podcast": "{feed}: new episode {title}",
        "uploaded_unknown": "Uploaded unknown",
        "enable_stream_cache": "Enable playback cache",
        "cache_folder": "Playback cache folder",
        "cache_size_mb": "Playback cache size in MB",
        "resume_playback": "Resume videos where you left off",
        "default_audio_device": "Default audio output device",
        "audio_device_missing": "The saved audio output device is no longer available. Choose a new default device.",
        "output_devices": "Audio output devices",
        "select_output_device": "Select audio output device",
        "output_device_set": "Audio output device set to {device}.",
        "no_output_devices": "No audio output devices were found.",
        "repeat": "Repeat",
        "repeat_on": "Repeat on.",
        "repeat_off": "Repeat off.",
        "playback_restarted": "Playback restarted from the beginning.",
        "playback_finished": "Playback finished.",
        "previous": "Previous",
        "next": "Next",
        "show_video_details": "Show video details",
        "copy_details": "Copy details",
        "details_copied": "Details copied.",
        "no_previous_item": "No previous item.",
        "no_next_item": "No next item.",
        "speed_audio_mode": "Audio quality when changing speed",
        "speed_audio_mode_scaletempo2": "High quality scaletempo2",
        "speed_audio_mode_mpv": "mpv default scaletempo2",
        "speed_audio_mode_scaletempo": "Classic scaletempo",
        "speed_audio_mode_rubberband": "Rubberband high quality (recommended)",
        "show_video_details_by_default": "Show video details in the player by default",
        "direct_link_enter_action": "What Enter does in Direct link",
        "direct_link_enter_play": "Play link",
        "direct_link_enter_audio": "Download audio",
        "direct_link_enter_video": "Download video",
        "direct_link_enter_stream": "Copy direct media URL",
        "shortcut_create_playlist": "Create playlist",
        "shortcut_add_to_playlist": "Add to playlist",
        "shortcut_remove_from_playlist": "Remove from playlist",
        "shortcut_copy_stream_url": "Copy direct media URL",
        "shortcut_player_output_devices": "Player: audio output devices",
        "shortcut_player_previous": "Player: previous item",
        "shortcut_player_next": "Player: next item",
    }
)
TEXT["sl"].update(
    {
        "select_playlist": "Izberi playlisto",
        "shortcut_capture_hint": "Pritisni novo kombinacijo tipk. Tab in Shift+Tab premikata fokus.",
        "shortcut_captured": "Bliznjica nastavljena na {shortcut}.",
        "shortcut_in_use": "{shortcut} je ze nastavljen za {action}. Izberi drugo bliznjico.",
        "shortcut_in_use_title": "Bliznjica je ze v uporabi",
        "tray_notification": "Windows obvestilo, ko gre ApricotPlayer v system tray",
        "tray_settings": "Nastavitve",
        "repeat": "Ponavljaj",
        "repeat_on": "Ponavljanje vklopljeno.",
        "repeat_off": "Ponavljanje izklopljeno.",
        "playback_restarted": "Predvajanje znova od zacetka.",
        "playback_finished": "Predvajanje koncano.",
    }
)
SUPPLEMENTAL_TRANSLATIONS = {
    "de": {
        "download_all_selected": "Alle ausgewaehlten Elemente herunterladen",
        "current_downloads": "Aktuelle Downloads",
        "cancel_download": "Download abbrechen",
        "cancel_all_downloads": "Alle Downloads abbrechen",
        "no_active_download": "Kein aktiver Download.",
        "collection_audio_selected_download": "Sammlung als Audio in Warteschlange: {title}",
        "collection_video_selected_download": "Sammlung als Video in Warteschlange: {title}",
        "collection_audio_queued_marker": "Sammlung Audio in Warteschlange",
        "collection_video_queued_marker": "Sammlung Video in Warteschlange",
        "download_state_queued": "In Warteschlange",
        "download_state_downloading": "Wird heruntergeladen",
        "download_state_processing": "Wird verarbeitet",
        "download_state_done": "Fertig",
        "download_state_cancelled": "Abgebrochen",
        "download_state_failed": "Fehlgeschlagen",
        "downloads_remaining": "{remaining} von {total} verbleibend",
        "download_percent_value": "{percent} Prozent",
        "download_cancel_requested": "Abbruch angefordert: {title}",
        "all_downloads_cancel_requested": "Abbruch fuer alle Downloads angefordert.",
        "components_updating": "Komponenten werden aktualisiert.",
        "components_done": "Komponenten sind aktuell.",
        "components_updated": "Komponenten aktualisiert.",
        "dynamic_results": "Dynamisch (laedt jeweils 20 Ergebnisse)",
    },
    "fr": {
        "download_all_selected": "Telecharger tous les elements selectionnes",
        "current_downloads": "Telechargements en cours",
        "cancel_download": "Annuler le telechargement",
        "cancel_all_downloads": "Annuler tous les telechargements",
        "no_active_download": "Aucun telechargement actif.",
        "collection_audio_selected_download": "Collection audio ajoutee a la file: {title}",
        "collection_video_selected_download": "Collection video ajoutee a la file: {title}",
        "collection_audio_queued_marker": "collection audio en file",
        "collection_video_queued_marker": "collection video en file",
        "download_state_queued": "En file",
        "download_state_downloading": "Telechargement",
        "download_state_processing": "Traitement",
        "download_state_done": "Termine",
        "download_state_cancelled": "Annule",
        "download_state_failed": "Echec",
        "downloads_remaining": "{remaining} sur {total} restants",
        "download_percent_value": "{percent} pour cent",
        "download_cancel_requested": "Annulation demandee: {title}",
        "all_downloads_cancel_requested": "Annulation demandee pour tous les telechargements.",
        "components_updating": "Mise a jour des composants.",
        "components_done": "Les composants sont a jour.",
        "components_updated": "Composants mis a jour.",
        "dynamic_results": "Dynamique (charge 20 resultats a la fois)",
    },
    "es": {
        "download_all_selected": "Descargar todos los elementos seleccionados",
        "current_downloads": "Descargas actuales",
        "cancel_download": "Cancelar descarga",
        "cancel_all_downloads": "Cancelar todas las descargas",
        "no_active_download": "No hay descarga activa.",
        "collection_audio_selected_download": "Coleccion de audio en cola: {title}",
        "collection_video_selected_download": "Coleccion de video en cola: {title}",
        "collection_audio_queued_marker": "coleccion de audio en cola",
        "collection_video_queued_marker": "coleccion de video en cola",
        "download_state_queued": "En cola",
        "download_state_downloading": "Descargando",
        "download_state_processing": "Procesando",
        "download_state_done": "Listo",
        "download_state_cancelled": "Cancelado",
        "download_state_failed": "Fallido",
        "downloads_remaining": "quedan {remaining} de {total}",
        "download_percent_value": "{percent} por ciento",
        "download_cancel_requested": "Cancelacion solicitada: {title}",
        "all_downloads_cancel_requested": "Cancelacion solicitada para todas las descargas.",
        "components_updating": "Actualizando componentes.",
        "components_done": "Los componentes estan actualizados.",
        "components_updated": "Componentes actualizados.",
        "dynamic_results": "Dinamico (carga 20 resultados cada vez)",
    },
    "pt": {
        "download_all_selected": "Baixar todos os itens selecionados",
        "current_downloads": "Downloads atuais",
        "cancel_download": "Cancelar download",
        "cancel_all_downloads": "Cancelar todos os downloads",
        "no_active_download": "Nenhum download ativo.",
        "collection_audio_selected_download": "Colecao em audio na fila: {title}",
        "collection_video_selected_download": "Colecao em video na fila: {title}",
        "collection_audio_queued_marker": "colecao em audio na fila",
        "collection_video_queued_marker": "colecao em video na fila",
        "download_state_queued": "Na fila",
        "download_state_downloading": "Baixando",
        "download_state_processing": "Processando",
        "download_state_done": "Concluido",
        "download_state_cancelled": "Cancelado",
        "download_state_failed": "Falhou",
        "downloads_remaining": "{remaining} de {total} restantes",
        "download_percent_value": "{percent} por cento",
        "download_cancel_requested": "Cancelamento solicitado: {title}",
        "all_downloads_cancel_requested": "Cancelamento solicitado para todos os downloads.",
        "components_updating": "Atualizando componentes.",
        "components_done": "Componentes atualizados.",
        "components_updated": "Componentes atualizados.",
        "dynamic_results": "Dinamico (carrega 20 resultados por vez)",
    },
    "it": {
        "download_all_selected": "Scarica tutti gli elementi selezionati",
        "current_downloads": "Download correnti",
        "cancel_download": "Annulla download",
        "cancel_all_downloads": "Annulla tutti i download",
        "no_active_download": "Nessun download attivo.",
        "collection_audio_selected_download": "Raccolta audio in coda: {title}",
        "collection_video_selected_download": "Raccolta video in coda: {title}",
        "collection_audio_queued_marker": "raccolta audio in coda",
        "collection_video_queued_marker": "raccolta video in coda",
        "download_state_queued": "In coda",
        "download_state_downloading": "Download in corso",
        "download_state_processing": "Elaborazione",
        "download_state_done": "Completato",
        "download_state_cancelled": "Annullato",
        "download_state_failed": "Non riuscito",
        "downloads_remaining": "{remaining} di {total} rimanenti",
        "download_percent_value": "{percent} percento",
        "download_cancel_requested": "Annullamento richiesto: {title}",
        "all_downloads_cancel_requested": "Annullamento richiesto per tutti i download.",
        "components_updating": "Aggiornamento componenti.",
        "components_done": "Componenti aggiornati.",
        "components_updated": "Componenti aggiornati.",
        "dynamic_results": "Dinamico (carica 20 risultati alla volta)",
    },
    "pl": {
        "download_all_selected": "Pobierz wszystkie wybrane elementy",
        "current_downloads": "Biezace pobierania",
        "cancel_download": "Anuluj pobieranie",
        "cancel_all_downloads": "Anuluj wszystkie pobierania",
        "no_active_download": "Brak aktywnego pobierania.",
        "collection_audio_selected_download": "Kolekcja audio dodana do kolejki: {title}",
        "collection_video_selected_download": "Kolekcja wideo dodana do kolejki: {title}",
        "collection_audio_queued_marker": "kolekcja audio w kolejce",
        "collection_video_queued_marker": "kolekcja wideo w kolejce",
        "download_state_queued": "W kolejce",
        "download_state_downloading": "Pobieranie",
        "download_state_processing": "Przetwarzanie",
        "download_state_done": "Gotowe",
        "download_state_cancelled": "Anulowane",
        "download_state_failed": "Niepowodzenie",
        "downloads_remaining": "pozostalo {remaining} z {total}",
        "download_percent_value": "{percent} procent",
        "download_cancel_requested": "Zadano anulowanie: {title}",
        "all_downloads_cancel_requested": "Zadano anulowanie wszystkich pobieran.",
        "components_updating": "Aktualizowanie komponentow.",
        "components_done": "Komponenty sa aktualne.",
        "components_updated": "Komponenty zaktualizowane.",
        "dynamic_results": "Dynamicznie (laduje po 20 wynikow)",
    },
    "nl": {
        "download_all_selected": "Alle geselecteerde items downloaden",
        "current_downloads": "Huidige downloads",
        "cancel_download": "Download annuleren",
        "cancel_all_downloads": "Alle downloads annuleren",
        "no_active_download": "Geen actieve download.",
        "collection_audio_selected_download": "Collectie als audio in wachtrij: {title}",
        "collection_video_selected_download": "Collectie als video in wachtrij: {title}",
        "collection_audio_queued_marker": "collectie audio in wachtrij",
        "collection_video_queued_marker": "collectie video in wachtrij",
        "download_state_queued": "In wachtrij",
        "download_state_downloading": "Downloaden",
        "download_state_processing": "Verwerken",
        "download_state_done": "Klaar",
        "download_state_cancelled": "Geannuleerd",
        "download_state_failed": "Mislukt",
        "downloads_remaining": "{remaining} van {total} resterend",
        "download_percent_value": "{percent} procent",
        "download_cancel_requested": "Annuleren aangevraagd: {title}",
        "all_downloads_cancel_requested": "Annuleren aangevraagd voor alle downloads.",
        "components_updating": "Componenten bijwerken.",
        "components_done": "Componenten zijn bijgewerkt.",
        "components_updated": "Componenten bijgewerkt.",
        "dynamic_results": "Dynamisch (laadt 20 resultaten per keer)",
    },
    "sv": {
        "download_all_selected": "Ladda ner alla valda objekt",
        "current_downloads": "Aktuella nedladdningar",
        "cancel_download": "Avbryt nedladdning",
        "cancel_all_downloads": "Avbryt alla nedladdningar",
        "no_active_download": "Ingen aktiv nedladdning.",
        "collection_audio_selected_download": "Samling som ljud i ko: {title}",
        "collection_video_selected_download": "Samling som video i ko: {title}",
        "collection_audio_queued_marker": "samling ljud i ko",
        "collection_video_queued_marker": "samling video i ko",
        "download_state_queued": "I ko",
        "download_state_downloading": "Laddar ner",
        "download_state_processing": "Bearbetar",
        "download_state_done": "Klar",
        "download_state_cancelled": "Avbruten",
        "download_state_failed": "Misslyckades",
        "downloads_remaining": "{remaining} av {total} kvar",
        "download_percent_value": "{percent} procent",
        "download_cancel_requested": "Avbrott begart: {title}",
        "all_downloads_cancel_requested": "Avbrott begart for alla nedladdningar.",
        "components_updating": "Uppdaterar komponenter.",
        "components_done": "Komponenterna ar uppdaterade.",
        "components_updated": "Komponenter uppdaterade.",
        "dynamic_results": "Dynamiskt (laddar 20 resultat i taget)",
    },
    "hr": {
        "download_all_selected": "Preuzmi sve odabrane stavke",
        "current_downloads": "Trenutna preuzimanja",
        "cancel_download": "Otkazi preuzimanje",
        "cancel_all_downloads": "Otkazi sva preuzimanja",
        "no_active_download": "Nema aktivnog preuzimanja.",
        "collection_audio_selected_download": "Zbirka kao audio u redu: {title}",
        "collection_video_selected_download": "Zbirka kao video u redu: {title}",
        "collection_audio_queued_marker": "zbirka audio u redu",
        "collection_video_queued_marker": "zbirka video u redu",
        "download_state_queued": "U redu",
        "download_state_downloading": "Preuzimanje",
        "download_state_processing": "Obrada",
        "download_state_done": "Gotovo",
        "download_state_cancelled": "Otkazano",
        "download_state_failed": "Neuspjelo",
        "downloads_remaining": "preostalo {remaining} od {total}",
        "download_percent_value": "{percent} posto",
        "download_cancel_requested": "Zatrazeno otkazivanje: {title}",
        "all_downloads_cancel_requested": "Zatrazeno otkazivanje svih preuzimanja.",
        "components_updating": "Azuriranje komponenti.",
        "components_done": "Komponente su azurne.",
        "components_updated": "Komponente azurirane.",
        "dynamic_results": "Dinamicki (ucitava po 20 rezultata)",
    },
    "sr": {
        "download_all_selected": "Preuzmi sve izabrane stavke",
        "current_downloads": "Trenutna preuzimanja",
        "cancel_download": "Otkazi preuzimanje",
        "cancel_all_downloads": "Otkazi sva preuzimanja",
        "no_active_download": "Nema aktivnog preuzimanja.",
        "collection_audio_selected_download": "Zbirka kao audio u redu: {title}",
        "collection_video_selected_download": "Zbirka kao video u redu: {title}",
        "collection_audio_queued_marker": "zbirka audio u redu",
        "collection_video_queued_marker": "zbirka video u redu",
        "download_state_queued": "U redu",
        "download_state_downloading": "Preuzimanje",
        "download_state_processing": "Obrada",
        "download_state_done": "Gotovo",
        "download_state_cancelled": "Otkazano",
        "download_state_failed": "Neuspelo",
        "downloads_remaining": "preostalo {remaining} od {total}",
        "download_percent_value": "{percent} posto",
        "download_cancel_requested": "Zatrazeno otkazivanje: {title}",
        "all_downloads_cancel_requested": "Zatrazeno otkazivanje svih preuzimanja.",
        "components_updating": "Azuriranje komponenti.",
        "components_done": "Komponente su azurne.",
        "components_updated": "Komponente azurirane.",
        "dynamic_results": "Dinamicki (ucitava po 20 rezultata)",
    },
}
SUPPLEMENTAL_TRANSLATIONS.setdefault("de", {}).update(
    {
        "tray_notification": "Windows-Benachrichtigung, wenn ApricotPlayer in den Infobereich geht",
        "tray_settings": "Einstellungen",
        "repeat": "Wiederholen",
        "repeat_on": "Wiederholen ein.",
        "repeat_off": "Wiederholen aus.",
        "playback_restarted": "Wiedergabe von Anfang neu gestartet.",
        "playback_finished": "Wiedergabe beendet.",
    }
)
SUPPLEMENTAL_TRANSLATIONS.setdefault("fr", {}).update(
    {
        "tray_notification": "Notification Windows quand ApricotPlayer va dans la zone de notification",
        "tray_settings": "Parametres",
        "repeat": "Repeter",
        "repeat_on": "Repetition activee.",
        "repeat_off": "Repetition desactivee.",
        "playback_restarted": "Lecture relancee depuis le debut.",
        "playback_finished": "Lecture terminee.",
    }
)
SUPPLEMENTAL_TRANSLATIONS.setdefault("es", {}).update(
    {
        "tray_notification": "Notificacion de Windows cuando ApricotPlayer va a la bandeja",
        "tray_settings": "Configuracion",
        "repeat": "Repetir",
        "repeat_on": "Repeticion activada.",
        "repeat_off": "Repeticion desactivada.",
        "playback_restarted": "Reproduccion reiniciada desde el principio.",
        "playback_finished": "Reproduccion terminada.",
    }
)
SUPPLEMENTAL_TRANSLATIONS.setdefault("pt", {}).update(
    {
        "tray_notification": "Notificacao do Windows quando ApricotPlayer vai para a bandeja",
        "tray_settings": "Configuracoes",
        "repeat": "Repetir",
        "repeat_on": "Repeticao ativada.",
        "repeat_off": "Repeticao desativada.",
        "playback_restarted": "Reproducao reiniciada desde o inicio.",
        "playback_finished": "Reproducao concluida.",
    }
)
SUPPLEMENTAL_TRANSLATIONS.setdefault("it", {}).update(
    {
        "tray_notification": "Notifica di Windows quando ApricotPlayer va nell'area di notifica",
        "tray_settings": "Impostazioni",
        "repeat": "Ripeti",
        "repeat_on": "Ripetizione attiva.",
        "repeat_off": "Ripetizione disattivata.",
        "playback_restarted": "Riproduzione riavviata dall'inizio.",
        "playback_finished": "Riproduzione terminata.",
    }
)
SUPPLEMENTAL_TRANSLATIONS.setdefault("pl", {}).update(
    {
        "tray_notification": "Powiadomienie Windows, gdy ApricotPlayer przechodzi do zasobnika",
        "tray_settings": "Ustawienia",
        "repeat": "Powtarzaj",
        "repeat_on": "Powtarzanie wlaczone.",
        "repeat_off": "Powtarzanie wylaczone.",
        "playback_restarted": "Odtwarzanie uruchomione od poczatku.",
        "playback_finished": "Odtwarzanie zakonczone.",
    }
)
SUPPLEMENTAL_TRANSLATIONS.setdefault("nl", {}).update(
    {
        "tray_notification": "Windows-melding wanneer ApricotPlayer naar het systeemvak gaat",
        "tray_settings": "Instellingen",
        "repeat": "Herhalen",
        "repeat_on": "Herhalen aan.",
        "repeat_off": "Herhalen uit.",
        "playback_restarted": "Afspelen opnieuw gestart vanaf het begin.",
        "playback_finished": "Afspelen voltooid.",
    }
)
SUPPLEMENTAL_TRANSLATIONS.setdefault("sv", {}).update(
    {
        "tray_notification": "Windows-meddelande nar ApricotPlayer gar till systemfaltet",
        "tray_settings": "Installningar",
        "repeat": "Upprepa",
        "repeat_on": "Upprepning pa.",
        "repeat_off": "Upprepning av.",
        "playback_restarted": "Uppspelning startad om fran borjan.",
        "playback_finished": "Uppspelning klar.",
    }
)
SUPPLEMENTAL_TRANSLATIONS.setdefault("hr", {}).update(
    {
        "tray_notification": "Windows obavijest kada ApricotPlayer ode u sistemsku traku",
        "tray_settings": "Postavke",
        "repeat": "Ponavljaj",
        "repeat_on": "Ponavljanje ukljuceno.",
        "repeat_off": "Ponavljanje iskljuceno.",
        "playback_restarted": "Reprodukcija ponovno pokrenuta od pocetka.",
        "playback_finished": "Reprodukcija zavrsena.",
    }
)
SUPPLEMENTAL_TRANSLATIONS.setdefault("sr", {}).update(
    {
        "tray_notification": "Windows obavestenje kada ApricotPlayer ode u sistemsku traku",
        "tray_settings": "Podesavanja",
        "repeat": "Ponavljaj",
        "repeat_on": "Ponavljanje ukljuceno.",
        "repeat_off": "Ponavljanje iskljuceno.",
        "playback_restarted": "Reprodukcija ponovo pokrenuta od pocetka.",
        "playback_finished": "Reprodukcija zavrsena.",
    }
)
for language_code, translations in SUPPLEMENTAL_TRANSLATIONS.items():
    TEXT.setdefault(language_code, {}).update(translations)
for language_code in LANGUAGE_CODES:
    TEXT[language_code] = {**TEXT["en"], **TEXT.get(language_code, {})}


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
    prefer_browser_playback: bool = False
    player_fullscreen: bool = False
    player_start_paused: bool = False
    player_speed: str = "1.0"
    speed_audio_mode: str = SPEED_AUDIO_MODE_RUBBERBAND
    show_video_details_by_default: bool = False
    direct_link_enter_action: str = DIRECT_LINK_ENTER_PLAY
    enable_stream_cache: bool = True
    cache_folder: str = str(DEFAULT_CACHE_DIR)
    cache_size_mb: int = 512
    resume_playback: bool = True
    audio_output_device: str = "auto"
    speed_step: float = 0.01
    pitch_step: float = 0.01
    pitch_mode: str = PITCH_MODE_MPV
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
    concurrent_fragments: int = 4
    retries: int = 10
    socket_timeout: int = 20
    close_to_tray: bool = False
    tray_notification: bool = True
    subscription_check_enabled: bool = True
    subscription_check_interval_hours: int = 6
    windows_notifications: bool = True
    download_notifications: bool = True
    subscription_notifications: bool = True
    last_subscription_check: float = 0.0
    enable_history: bool = True
    enable_podcasts_rss: bool = True
    podcast_search_provider: str = PODCAST_DIRECTORY_PROVIDER_APPLE
    podcast_search_country: str = "US"
    podcast_search_limit: int = 20
    rss_max_items: int = 100
    rss_refresh_on_startup: bool = False
    rss_auto_refresh_enabled: bool = False
    rss_refresh_interval_hours: int = 12
    history_limit: int = 500
    keyboard_shortcuts: dict[str, str] = field(default_factory=lambda: dict(DEFAULT_KEYBOARD_SHORTCUTS))


class ApricotTaskBarIcon(wx.adv.TaskBarIcon):
    def __init__(self, frame: "MainFrame") -> None:
        super().__init__()
        self.frame = frame
        self.show_id = wx.NewIdRef()
        self.settings_id = wx.NewIdRef()
        self.check_id = wx.NewIdRef()
        self.exit_id = wx.NewIdRef()
        for event_name in ("EVT_TASKBAR_CLICK", "EVT_TASKBAR_LEFT_DOWN", "EVT_TASKBAR_LEFT_UP", "EVT_TASKBAR_LEFT_DCLICK"):
            event_binder = getattr(wx.adv, event_name, None)
            if event_binder is not None:
                self.Bind(event_binder, lambda _event: self.frame.restore_from_tray())
        self.Bind(wx.EVT_MENU, lambda _event: self.frame.restore_from_tray(), id=int(self.show_id))
        self.Bind(wx.EVT_MENU, lambda _event: self.frame.show_settings_from_tray(), id=int(self.settings_id))
        self.Bind(wx.EVT_MENU, lambda _event: self.frame.check_subscriptions(manual=True), id=int(self.check_id))
        self.Bind(wx.EVT_MENU, lambda _event: self.frame.quit_application(), id=int(self.exit_id))
        self.SetIcon(self.make_icon(), APP_NAME)

    def CreatePopupMenu(self) -> wx.Menu:
        menu = wx.Menu()
        menu.Append(int(self.show_id), self.frame.t("tray_show"))
        menu.Append(int(self.settings_id), self.frame.t("tray_settings"))
        menu.Append(int(self.check_id), self.frame.t("tray_check_subscriptions"))
        menu.AppendSeparator()
        menu.Append(int(self.exit_id), self.frame.t("tray_exit"))
        return menu

    @staticmethod
    def make_icon() -> wx.Icon:
        bitmap = wx.ArtProvider.GetBitmap(wx.ART_INFORMATION, wx.ART_OTHER, (16, 16))
        icon = wx.Icon()
        icon.CopyFromBitmap(bitmap)
        return icon


class MainFrame(wx.Frame):
    def __init__(self) -> None:
        super().__init__(None, title=WINDOW_TITLE, size=(950, 680))
        APP_DIR.mkdir(parents=True, exist_ok=True)
        settings_file_existed = SETTINGS_FILE.exists()
        self.settings = self.load_settings()
        if not settings_file_existed:
            self.save_settings()
        self.favorites = self.load_favorites()
        self.history = self.load_history()
        self.subscriptions = self.load_subscriptions()
        self.rss_feeds = self.load_rss_feeds()
        self.user_playlists = self.load_user_playlists()
        self.notifications = self.load_notifications()
        self.playback_positions = self.load_playback_positions()
        self.rss_items: list[dict] = []
        self.podcast_search_results: list[dict] = []
        self.results: list[dict] = []
        self.all_results: list[dict] = []
        self.return_results: list[dict] = []
        self.return_all_results: list[dict] = []
        self.return_index = 0
        self.return_visible_count = 0
        self.last_search_query = ""
        self.last_search_type_index = 0
        self.last_visible_count = 0
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.in_main_menu = False
        self.current_rss_feed_index = -1
        self.current_user_playlist_index = -1
        self.player_return_screen = ""
        self.player_return_data: dict = {}
        self.search_results_stack: list[dict] = []
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
        self.repeat_current = False
        self.player_generation = 0
        self.player_ended = False
        self.current_video_item: dict | None = None
        self.current_video_info: dict = {}
        self.details_label: wx.StaticText | None = None
        self.video_details: wx.TextCtrl | None = None
        self.download_queue: dict[str, dict] = {}
        self.active_downloads: dict[str, dict] = {}
        self.download_cancel_events: dict[str, threading.Event] = {}
        self.download_task_counter = 0
        self.queue_items: list[dict] = []
        self.last_download_shortcut: tuple[str, str, float] = ("", "", 0.0)
        self.ipc_path: str | None = None
        self.mpv_ipc_lock = threading.Lock()
        self.ui_queue: queue.Queue[tuple[str, object]] = queue.Queue()
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.current_search_type_code = "All"
        self.collection_url = ""
        self.collection_result_type = ""
        self.current_stream_url = ""
        self.current_audio_device = ""
        self.session_audio_output_device = ""
        self.audio_device_options_cache: tuple[float, list[str], list[str]] | None = None
        self.metadata_hydration_urls: set[str] = set()
        self.search_generation = 0
        self.last_activation_check = 0.0
        self.details_opened_temporarily = False
        self.nvda_client = self.load_nvda_client()
        self.update_progress_dialog: wx.ProgressDialog | None = None
        self.subscription_check_running = False
        self.rss_refresh_running = False
        self.exiting = False
        self.taskbar_icon: ApricotTaskBarIcon | None = None

        self.panel = wx.Panel(self)
        self.root_sizer = wx.BoxSizer(wx.VERTICAL)
        self.panel.SetSizer(self.root_sizer)
        self.status = self.CreateStatusBar()
        self.status.SetStatusText(self.t("ready"))

        self.Bind(wx.EVT_CHAR_HOOK, self.on_char_hook)
        self.Bind(wx.EVT_CLOSE, self.on_close)
        self.install_download_accelerators()
        self.setup_taskbar_icon()
        self.show_main_menu()
        self.timer = wx.Timer(self)
        self.Bind(wx.EVT_TIMER, self.process_queue, self.timer)
        self.timer.Start(100)
        self.subscription_timer = wx.Timer(self)
        self.Bind(wx.EVT_TIMER, self.on_subscription_timer, self.subscription_timer)
        self.configure_subscription_timer()
        self.rss_timer = wx.Timer(self)
        self.Bind(wx.EVT_TIMER, self.on_rss_timer, self.rss_timer)
        self.configure_rss_timer()
        if self.settings.auto_update_ytdlp:
            wx.CallLater(3500, self.start_ytdlp_update_check)
        if self.settings.auto_update_app:
            wx.CallLater(5500, self.start_app_update_check)
        if self.settings.subscription_check_enabled:
            wx.CallLater(8500, self.check_subscriptions_if_due)
        if self.settings.enable_podcasts_rss and self.settings.rss_refresh_on_startup and self.rss_feeds:
            wx.CallLater(9500, self.refresh_all_rss_feeds_background)
        wx.CallLater(6500, self.check_saved_audio_device_available)

    def install_download_accelerators(self) -> None:
        self.download_audio_accelerator_id = wx.NewIdRef()
        self.download_video_accelerator_id = wx.NewIdRef()
        self.subscribe_accelerator_id = wx.NewIdRef()
        self.Bind(wx.EVT_MENU, lambda _evt: self.download_audio_shortcut(), id=int(self.download_audio_accelerator_id))
        self.Bind(wx.EVT_MENU, lambda _evt: self.download_video_shortcut(), id=int(self.download_video_accelerator_id))
        self.Bind(wx.EVT_MENU, lambda _evt: self.subscribe_shortcut(), id=int(self.subscribe_accelerator_id))
        entries = []
        for action, menu_id in (
            ("download_audio", int(self.download_audio_accelerator_id)),
            ("download_video", int(self.download_video_accelerator_id)),
            ("subscribe_channel", int(self.subscribe_accelerator_id)),
        ):
            accelerator = self.shortcut_to_accelerator(self.shortcut_for(action))
            if accelerator:
                flags, key_code = accelerator
                entries.append((flags, key_code, menu_id))
        self.SetAcceleratorTable(wx.AcceleratorTable(entries))

    def shortcut_for(self, action: str) -> str:
        shortcuts = getattr(self.settings, "keyboard_shortcuts", {}) or {}
        value = str(shortcuts.get(action) or DEFAULT_KEYBOARD_SHORTCUTS.get(action) or "").strip()
        return value or DEFAULT_KEYBOARD_SHORTCUTS.get(action, "")

    def menu_label_with_shortcut(self, label_key: str, action: str) -> str:
        shortcut = self.shortcut_for(action)
        return f"{self.t(label_key)}\t{shortcut}" if shortcut else self.t(label_key)

    def shortcut_to_accelerator(self, shortcut: str) -> tuple[int, int] | None:
        parsed = self.parse_shortcut(shortcut)
        if not parsed:
            return None
        ctrl, shift, alt, key_name = parsed
        key_code = self.shortcut_key_code(key_name)
        if key_code is None or key_code < 0:
            return None
        flags = 0
        if ctrl:
            flags |= wx.ACCEL_CTRL
        if shift:
            flags |= wx.ACCEL_SHIFT
        if alt:
            flags |= wx.ACCEL_ALT
        return flags, key_code

    @staticmethod
    def parse_shortcut(shortcut: str) -> tuple[bool, bool, bool, str] | None:
        text = str(shortcut or "").strip()
        if not text:
            return None
        text = text.split("|", 1)[0].strip()
        parts = [part.strip() for part in text.replace("-", "+").split("+") if part.strip()]
        if not parts:
            return None
        ctrl = shift = alt = False
        key_parts: list[str] = []
        for part in parts:
            normalized = part.lower().replace(" ", "")
            if normalized in {"ctrl", "control", "strg"}:
                ctrl = True
            elif normalized in {"shift", "shft"}:
                shift = True
            elif normalized in {"alt", "option"}:
                alt = True
            else:
                key_parts.append(part)
        if not key_parts:
            return None
        return ctrl, shift, alt, " ".join(key_parts).strip()

    @staticmethod
    def shortcut_key_code(key_name: str) -> int | None:
        normalized = key_name.strip().lower().replace("_", " ").replace("-", " ")
        normalized = re.sub(r"\s+", " ", normalized)
        aliases = {
            "enter": wx.WXK_RETURN,
            "return": wx.WXK_RETURN,
            "space": wx.WXK_SPACE,
            "spacebar": wx.WXK_SPACE,
            "escape": wx.WXK_ESCAPE,
            "esc": wx.WXK_ESCAPE,
            "delete": wx.WXK_DELETE,
            "del": wx.WXK_DELETE,
            "backspace": wx.WXK_BACK,
            "back": wx.WXK_BACK,
            "insert": wx.WXK_INSERT,
            "ins": wx.WXK_INSERT,
            "home": wx.WXK_HOME,
            "end": wx.WXK_END,
            "pageup": wx.WXK_PAGEUP,
            "page up": wx.WXK_PAGEUP,
            "pagedown": wx.WXK_PAGEDOWN,
            "page down": wx.WXK_PAGEDOWN,
            "left": wx.WXK_LEFT,
            "left arrow": wx.WXK_LEFT,
            "right": wx.WXK_RIGHT,
            "right arrow": wx.WXK_RIGHT,
            "up": wx.WXK_UP,
            "up arrow": wx.WXK_UP,
            "down": wx.WXK_DOWN,
            "down arrow": wx.WXK_DOWN,
            "applications": getattr(wx, "WXK_WINDOWS_MENU", getattr(wx, "WXK_MENU", getattr(wx, "WXK_APPS", -1))),
            "application": getattr(wx, "WXK_WINDOWS_MENU", getattr(wx, "WXK_MENU", getattr(wx, "WXK_APPS", -1))),
            "apps": getattr(wx, "WXK_WINDOWS_MENU", getattr(wx, "WXK_MENU", getattr(wx, "WXK_APPS", -1))),
            "menu": getattr(wx, "WXK_WINDOWS_MENU", getattr(wx, "WXK_MENU", getattr(wx, "WXK_APPS", -1))),
            "context menu": getattr(wx, "WXK_WINDOWS_MENU", getattr(wx, "WXK_MENU", getattr(wx, "WXK_APPS", -1))),
        }
        if normalized in aliases:
            return aliases[normalized]
        match = re.fullmatch(r"f(\d{1,2})", normalized)
        if match:
            number = int(match.group(1))
            if 1 <= number <= 24:
                return wx.WXK_F1 + number - 1
        if len(normalized) == 1:
            return ord(normalized.upper())
        return None

    @staticmethod
    def shortcut_name_for_key_code(key_code: int, unicode_key: int = 0) -> str:
        names = {
            wx.WXK_RETURN: "Enter",
            wx.WXK_NUMPAD_ENTER: "Enter",
            wx.WXK_SPACE: "Space",
            wx.WXK_ESCAPE: "Escape",
            wx.WXK_DELETE: "Delete",
            wx.WXK_BACK: "Backspace",
            wx.WXK_INSERT: "Insert",
            wx.WXK_HOME: "Home",
            wx.WXK_END: "End",
            wx.WXK_PAGEUP: "PageUp",
            wx.WXK_PAGEDOWN: "PageDown",
            wx.WXK_LEFT: "Left",
            wx.WXK_RIGHT: "Right",
            wx.WXK_UP: "Up",
            wx.WXK_DOWN: "Down",
            getattr(wx, "WXK_APPS", -1): "Applications",
            getattr(wx, "WXK_MENU", -1): "Applications",
            getattr(wx, "WXK_WINDOWS_MENU", -1): "Applications",
        }
        if key_code in names:
            return names[key_code]
        if wx.WXK_F1 <= key_code <= wx.WXK_F24:
            return f"F{key_code - wx.WXK_F1 + 1}"
        if ord("A") <= key_code <= ord("Z"):
            return chr(key_code)
        if ord("0") <= key_code <= ord("9"):
            return chr(key_code)
        if unicode_key and 32 <= unicode_key < 127:
            return chr(unicode_key).upper()
        return ""

    def shortcut_from_key_event(self, event: wx.KeyEvent) -> str:
        key_code = event.GetKeyCode()
        modifier_keys = {wx.WXK_TAB, getattr(wx, "WXK_CONTROL", -1), getattr(wx, "WXK_SHIFT", -1), getattr(wx, "WXK_ALT", -1)}
        if key_code in modifier_keys:
            return ""
        key_name = self.shortcut_name_for_key_code(key_code, event.GetUnicodeKey())
        if not key_name:
            return ""
        parts: list[str] = []
        if event.ControlDown():
            parts.append("Ctrl")
        if event.ShiftDown():
            parts.append("Shift")
        if event.AltDown():
            parts.append("Alt")
        parts.append(key_name)
        return "+".join(parts)

    def on_shortcut_capture_key(self, event: wx.KeyEvent, control: wx.TextCtrl) -> None:
        if event.GetKeyCode() == wx.WXK_TAB:
            event.Skip()
            return
        shortcut = self.shortcut_from_key_event(event)
        if not shortcut:
            event.Skip()
            return
        action = str(getattr(control, "_apricot_shortcut_action", "") or "")
        conflict = self.shortcut_conflict(shortcut, action)
        if conflict:
            message = self.t("shortcut_in_use", shortcut=shortcut, action=self.t(conflict[1]))
            wx.MessageBox(message, self.t("shortcut_in_use_title"), wx.OK | wx.ICON_WARNING)
            self.speak_text(message)
            control.SetFocus()
            return
        control.ChangeValue(shortcut)
        control.SetInsertionPointEnd()
        control.SetFocus()
        self.speak_text(self.t("shortcut_captured", shortcut=shortcut))

    def canonical_shortcut(self, shortcut: str) -> str:
        parsed = self.parse_shortcut(shortcut)
        if not parsed:
            return ""
        ctrl, shift, alt, key_name = parsed
        key_code = self.shortcut_key_code(key_name)
        if key_code is None or key_code < 0:
            return ""
        key_label = self.shortcut_name_for_key_code(key_code)
        if not key_label:
            key_label = key_name.strip()
        parts: list[str] = []
        if ctrl:
            parts.append("Ctrl")
        if shift:
            parts.append("Shift")
        if alt:
            parts.append("Alt")
        parts.append(key_label)
        return "+".join(parts).lower()

    def shortcut_conflict(self, shortcut: str, current_action: str = "") -> tuple[str, str] | None:
        wanted = self.canonical_shortcut(shortcut)
        if not wanted:
            return None
        values = self.current_shortcut_values_from_controls()
        for action, label_key in SHORTCUT_DEFINITIONS:
            if action == current_action:
                continue
            if self.canonical_shortcut(values.get(action) or "") == wanted:
                return action, label_key
        return None

    def current_shortcut_values_from_controls(self) -> dict[str, str]:
        values = self.normalized_keyboard_shortcuts(getattr(self.settings, "keyboard_shortcuts", {}) or {})
        if hasattr(self, "controls"):
            for action, _label_key in SHORTCUT_DEFINITIONS:
                control = self.controls.get(f"shortcut_{action}")
                if isinstance(control, wx.TextCtrl):
                    values[action] = control.GetValue().strip() or DEFAULT_KEYBOARD_SHORTCUTS[action]
        return values

    def validate_shortcut_controls(self) -> bool:
        if not hasattr(self, "controls") or not any(f"shortcut_{action}" in self.controls for action, _label_key in SHORTCUT_DEFINITIONS):
            return True
        values = self.current_shortcut_values_from_controls()
        seen: dict[str, tuple[str, str]] = {}
        for action, label_key in SHORTCUT_DEFINITIONS:
            canonical = self.canonical_shortcut(values.get(action) or "")
            if not canonical:
                continue
            if canonical in seen:
                _other_action, other_label_key = seen[canonical]
                shortcut = values.get(action) or DEFAULT_KEYBOARD_SHORTCUTS[action]
                message = self.t("shortcut_in_use", shortcut=shortcut, action=self.t(other_label_key))
                wx.MessageBox(message, self.t("shortcut_in_use_title"), wx.OK | wx.ICON_WARNING)
                self.speak_text(message)
                control = self.controls.get(f"shortcut_{action}") if hasattr(self, "controls") else None
                if isinstance(control, wx.TextCtrl):
                    self.safe_set_focus(control)
                return False
            seen[canonical] = (action, label_key)
        return True

    @staticmethod
    def is_shortcut_capture_control(control: wx.Window | None) -> bool:
        return isinstance(control, wx.TextCtrl) and bool(getattr(control, "_apricot_shortcut_capture", False))

    def shortcut_matches(self, event: wx.KeyEvent, action: str) -> bool:
        return self.event_matches_shortcut(event, self.shortcut_for(action))

    def event_matches_shortcut(self, event: wx.KeyEvent, shortcut: str) -> bool:
        alternatives = re.split(r"\s*\|\s*", str(shortcut or ""))
        return any(self.event_matches_single_shortcut(event, alternative) for alternative in alternatives if alternative.strip())

    def event_matches_single_shortcut(self, event: wx.KeyEvent, shortcut: str) -> bool:
        parsed = self.parse_shortcut(shortcut)
        if not parsed:
            return False
        ctrl, shift, alt, key_name = parsed
        if bool(event.ControlDown()) != ctrl or bool(event.ShiftDown()) != shift or bool(event.AltDown()) != alt:
            return False
        key_code = self.shortcut_key_code(key_name)
        if key_code is None:
            return False
        event_codes = self.key_event_codes(event)
        if key_code == wx.WXK_RETURN and wx.WXK_NUMPAD_ENTER in event_codes:
            return True
        if len(key_name.strip()) == 1 and key_name.strip().isprintable():
            return self.key_event_matches_letter(event, key_name.strip()) if key_name.strip().isalpha() else key_code in event_codes
        return key_code in event_codes

    def context_menu_shortcut_matches(self, event: wx.KeyEvent) -> bool:
        context_codes = {
            getattr(wx, "WXK_APPS", -1),
            getattr(wx, "WXK_MENU", -1),
            getattr(wx, "WXK_WINDOWS_MENU", -1),
        }
        return self.shortcut_matches(event, "context_menu") or event.GetKeyCode() in context_codes or (event.GetKeyCode() == wx.WXK_F10 and event.ShiftDown())

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

    @staticmethod
    def listbox_strings(listbox: wx.ListBox) -> list[str]:
        return [listbox.GetString(index) for index in range(listbox.GetCount())]

    def set_listbox_items(self, listbox: wx.ListBox, labels: list[str], selection: int = 0) -> bool:
        labels = [str(label) for label in labels]
        if not labels:
            return False
        target_selection = min(max(0, selection), len(labels) - 1)
        current_selection = listbox.GetSelection()
        if self.listbox_strings(listbox) == labels:
            if current_selection != target_selection:
                listbox.SetSelection(target_selection)
                return True
            return False
        listbox.Freeze()
        try:
            listbox.Clear()
            for label in labels:
                listbox.Append(label)
            listbox.SetSelection(target_selection)
        finally:
            listbox.Thaw()
        return True

    def speak_text(self, text: str) -> None:
        if not text:
            return
        announced = False
        if self.nvda_client:
            try:
                if hasattr(self.nvda_client, "nvdaController_cancelSpeech"):
                    self.nvda_client.nvdaController_cancelSpeech()
                result = self.nvda_client.nvdaController_speakText(str(text))
                if result == 0:
                    announced = True
                if hasattr(self.nvda_client, "nvdaController_brailleMessage"):
                    braille_result = self.nvda_client.nvdaController_brailleMessage(str(text))
                    if braille_result == 0:
                        announced = True
            except Exception:
                self.nvda_client = None
        self.raise_accessibility_alert(text)
        if announced:
            return

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
                    if hasattr(client, "nvdaController_brailleMessage"):
                        client.nvdaController_brailleMessage.argtypes = [ctypes.c_wchar_p]
                        client.nvdaController_brailleMessage.restype = ctypes.c_int
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

    def setup_taskbar_icon(self) -> None:
        if self.taskbar_icon is not None:
            return
        try:
            self.taskbar_icon = ApricotTaskBarIcon(self)
        except Exception:
            self.taskbar_icon = None

    def destroy_taskbar_icon(self) -> None:
        if self.taskbar_icon is None:
            return
        try:
            self.taskbar_icon.RemoveIcon()
            self.taskbar_icon.Destroy()
        except Exception:
            pass
        self.taskbar_icon = None

    def on_close(self, event: wx.CloseEvent) -> None:
        if self.exiting or not self.settings.close_to_tray:
            self.destroy_taskbar_icon()
            event.Skip()
            return
        event.Veto()
        self.Hide()
        self.announce_player(self.t("tray_still_running"))
        self.show_desktop_notification(APP_NAME, self.t("tray_still_running"), enabled=self.settings.tray_notification)

    def restore_from_tray(self) -> None:
        try:
            if self.IsIconized():
                self.Iconize(False)
        except Exception:
            pass
        self.Show(True)
        try:
            self.RequestUserAttention(wx.USER_ATTENTION_INFO)
        except Exception:
            pass
        self.Raise()
        if hasattr(self, "menu_list") and self.in_main_menu:
            self.focus_later(self.menu_list)
        else:
            self.SetFocus()

    def show_settings_from_tray(self) -> None:
        self.restore_from_tray()
        wx.CallAfter(self.show_settings)

    def check_activation_signal(self) -> None:
        now = time.monotonic()
        if now - self.last_activation_check < 0.2:
            return
        self.last_activation_check = now
        try:
            if not ACTIVATE_SIGNAL_FILE.exists():
                return
            payload = json.loads(ACTIVATE_SIGNAL_FILE.read_text(encoding="utf-8"))
            ACTIVATE_SIGNAL_FILE.unlink(missing_ok=True)
        except Exception:
            return
        action = str(payload.get("action") or "show")
        if action == "settings":
            self.show_settings_from_tray()
        else:
            self.restore_from_tray()

    def quit_application(self) -> None:
        self.exiting = True
        self.destroy_taskbar_icon()
        self.Close(force=True)

    def app_has_focus(self) -> bool:
        try:
            if self.IsShown() and self.IsActive():
                return True
        except Exception:
            pass
        try:
            focus = wx.Window.FindFocus()
            if focus and focus.GetTopLevelParent() is self:
                return True
        except Exception:
            pass
        return False

    def show_desktop_notification(
        self,
        title: str,
        message: str,
        enabled: bool = True,
        only_when_unfocused: bool = False,
    ) -> bool:
        if not enabled or not self.settings.windows_notifications:
            return False
        if only_when_unfocused and self.app_has_focus():
            return False
        try:
            notification = wx.adv.NotificationMessage(title=title, message=message, parent=self)
            notification.Show(timeout=10)
            return True
        except Exception:
            return False

    def show_download_complete_notification(self, message: str) -> bool:
        return self.show_desktop_notification(
            self.t("notification_download_title"),
            message,
            enabled=self.settings.download_notifications,
            only_when_unfocused=True,
        )

    def show_main_menu(self) -> None:
        self.in_main_menu = True
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        title = wx.StaticText(self.panel, label=self.t("main_menu"))
        self.root_sizer.Add(title, 0, wx.ALL, 4)
        self.menu_actions = []
        download_count = len(self.download_queue) + len(self.active_downloads)
        if download_count:
            self.menu_actions.append((f"{self.t('current_downloads')} ({download_count})", self.show_download_queue))
        self.menu_actions.extend([
            (self.t("search_youtube"), self.show_search),
            (self.t("direct_link"), self.show_direct_link),
            (self.t("favorites"), self.show_favorites),
            (self.t("playlists"), self.show_user_playlists),
            (self.t("subscriptions"), self.show_subscriptions),
            (self.t("notification_center"), self.show_notification_center),
        ])
        if self.settings.enable_history:
            self.menu_actions.append((self.t("history"), self.show_history))
        if self.settings.enable_podcasts_rss:
            self.menu_actions.append((self.t("rss_feeds"), self.show_rss_feeds))
        self.menu_actions.extend([
            (self.t("settings"), self.show_settings),
            (self.t("exit"), self.quit_application),
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
        if self.shortcut_matches(event, "open_selected"):
            self.activate_menu()
            return
        event.Skip()

    def activate_menu(self) -> None:
        index = self.menu_list.GetSelection()
        if index != wx.NOT_FOUND:
            self.menu_actions[index][1]()

    def show_direct_link(self) -> None:
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = True
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("play_direct_link"), self.play_direct_link),
                (self.t("download_direct_audio"), lambda: self.download_direct_link(True)),
                (self.t("download_direct_video"), lambda: self.download_direct_link(False)),
                (self.t("copy_stream_url"), self.copy_direct_stream_url),
            ]
        )
        label = wx.StaticText(self.panel, label=self.t("direct_link_url"))
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.direct_link_ctrl = wx.TextCtrl(self.panel, style=wx.TE_PROCESS_ENTER)
        self.direct_link_ctrl.SetName(self.t("direct_link_url"))
        self.direct_link_ctrl.Bind(wx.EVT_TEXT_ENTER, lambda _evt: self.activate_direct_link_enter_action())
        self.root_sizer.Add(self.direct_link_ctrl, 0, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        self.focus_later(self.direct_link_ctrl)

    def direct_link_item(self) -> dict | None:
        if not hasattr(self, "direct_link_ctrl"):
            return None
        url = self.direct_link_ctrl.GetValue().strip()
        if not url:
            return None
        if not re.match(r"^[a-z][a-z0-9+.-]*://", url, flags=re.IGNORECASE):
            url = "https://" + url
        return {
            "title": url,
            "url": url,
            "webpage_url": url,
            "kind": "video",
            "type": self.t("direct_link"),
            "channel": "",
        }

    def play_direct_link(self) -> None:
        item = self.direct_link_item()
        if not item:
            self.message(self.t("no_selection"))
            return
        self.player_return_screen = "direct_link"
        self.player_return_data = {}
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(str(item.get("url") or ""), str(item.get("title") or ""))

    def activate_direct_link_enter_action(self) -> None:
        action = self.normalized_direct_link_enter_action()
        if action == DIRECT_LINK_ENTER_AUDIO:
            self.download_direct_link(True)
        elif action == DIRECT_LINK_ENTER_VIDEO:
            self.download_direct_link(False)
        elif action == DIRECT_LINK_ENTER_STREAM:
            self.copy_direct_stream_url(self.direct_link_item())
        else:
            self.play_direct_link()

    def download_direct_link(self, audio_only: bool) -> None:
        item = self.direct_link_item()
        if not item:
            self.message(self.t("no_selection"))
            return
        self.start_download(audio_only, item=item)

    def show_user_playlists(self) -> None:
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = True
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("create_playlist"), self.create_user_playlist_dialog),
                (self.t("open_playlist"), self.open_selected_user_playlist),
                (self.t("download_user_playlist"), self.download_selected_user_playlist),
                (self.t("remove_playlist"), self.remove_selected_user_playlist),
            ]
        )
        label = wx.StaticText(self.panel, label=self.t("playlists"))
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.user_playlist_list = wx.ListBox(self.panel, choices=[])
        self.user_playlist_list.SetName(self.t("playlists"))
        self.user_playlist_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.open_selected_user_playlist())
        self.user_playlist_list.Bind(wx.EVT_CONTEXT_MENU, self.open_user_playlists_context_menu)
        self.user_playlist_list.Bind(wx.EVT_KEY_DOWN, self.on_user_playlists_key)
        self.root_sizer.Add(self.user_playlist_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_user_playlists()
        self.panel.Layout()
        self.focus_later(self.user_playlist_list)

    def refresh_user_playlists(self) -> None:
        if not hasattr(self, "user_playlist_list"):
            return
        try:
            if self.user_playlists:
                labels = [self.user_playlist_line(playlist) for playlist in self.user_playlists]
                index = min(max(0, self.current_user_playlist_index), len(self.user_playlists) - 1)
                self.set_listbox_items(self.user_playlist_list, labels, index)
            else:
                self.set_listbox_items(self.user_playlist_list, [self.t("no_playlists")], 0)
                self.set_status(self.t("no_playlists"))
        except RuntimeError:
            pass

    def user_playlist_line(self, playlist: dict) -> str:
        count = len(playlist.get("items") or [])
        return f"{playlist.get('title', '')} | {count} {self.t('video')}"

    def selected_user_playlist(self) -> dict | None:
        if not hasattr(self, "user_playlist_list"):
            return None
        index = self.user_playlist_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.user_playlists):
            return None
        self.current_user_playlist_index = index
        return self.user_playlists[index]

    def on_user_playlists_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "create_playlist"):
            self.create_user_playlist_dialog()
        elif self.shortcut_matches(event, "open_selected"):
            self.open_selected_user_playlist()
        elif self.shortcut_matches(event, "remove_selected"):
            self.remove_selected_user_playlist()
        elif self.context_menu_shortcut_matches(event):
            self.open_user_playlists_context_menu()
        else:
            event.Skip()

    def open_user_playlists_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("open_playlist"), self.open_selected_user_playlist),
            (self.menu_label_with_shortcut("create_playlist", "create_playlist"), self.create_user_playlist_dialog),
            (self.t("download_user_playlist"), self.download_selected_user_playlist),
            (self.t("remove_playlist"), self.remove_selected_user_playlist),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def create_user_playlist_dialog(self, initial_item: dict | None = None) -> int | None:
        with wx.TextEntryDialog(self, self.t("playlist_name"), self.t("create_playlist")) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return None
            title = dialog.GetValue().strip()
        if not title:
            return None
        if any(str(playlist.get("title") or "").lower() == title.lower() for playlist in self.user_playlists):
            self.announce_player(self.t("playlist_exists"))
            return None
        playlist = {"title": title, "items": [], "created_at": time.time(), "updated_at": time.time()}
        self.user_playlists.append(playlist)
        self.current_user_playlist_index = len(self.user_playlists) - 1
        if initial_item:
            playlist["items"].append(self.playlist_item_from_media(initial_item))
        self.save_user_playlists()
        self.refresh_user_playlists()
        self.announce_player(self.t("playlist_created", title=title))
        return self.current_user_playlist_index

    def open_selected_user_playlist(self) -> None:
        if self.selected_user_playlist() is None:
            self.announce_player(self.t("no_playlists"))
            return
        self.show_user_playlist_items(self.current_user_playlist_index)

    def show_user_playlist_items(self, playlist_index: int, selection: int = 0) -> None:
        if playlist_index < 0 or playlist_index >= len(self.user_playlists):
            self.show_user_playlists()
            return
        self.current_user_playlist_index = playlist_index
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = True
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_user_playlists),
                (self.t("play"), self.play_selected_user_playlist_item),
                (self.t("download_user_playlist"), self.download_current_user_playlist),
                (self.t("remove_from_playlist"), self.remove_selected_user_playlist_item),
            ]
        )
        playlist = self.user_playlists[playlist_index]
        label = wx.StaticText(self.panel, label=f"{self.t('playlist_items')}: {playlist.get('title', '')}")
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.user_playlist_items = list(playlist.get("items") or [])
        self.user_playlist_items_list = wx.ListBox(self.panel, choices=[])
        self.user_playlist_items_list.SetName(self.t("playlist_items"))
        self.user_playlist_items_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected_user_playlist_item())
        self.user_playlist_items_list.Bind(wx.EVT_CONTEXT_MENU, self.open_user_playlist_items_context_menu)
        self.user_playlist_items_list.Bind(wx.EVT_KEY_DOWN, self.on_user_playlist_items_key)
        self.root_sizer.Add(self.user_playlist_items_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_user_playlist_items(selection)
        self.panel.Layout()
        self.focus_later(self.user_playlist_items_list)

    def refresh_user_playlist_items(self, selection: int = 0) -> None:
        if not hasattr(self, "user_playlist_items_list"):
            return
        try:
            self.user_playlist_items = list(self.user_playlists[self.current_user_playlist_index].get("items") or [])
            if self.user_playlist_items:
                labels = [self.result_line(index, item) for index, item in enumerate(self.user_playlist_items)]
                self.set_listbox_items(self.user_playlist_items_list, labels, selection)
            else:
                self.set_listbox_items(self.user_playlist_items_list, [self.t("playlist_empty")], 0)
                self.set_status(self.t("playlist_empty"))
        except RuntimeError:
            pass

    def selected_user_playlist_item(self) -> dict | None:
        if not hasattr(self, "user_playlist_items_list"):
            return None
        index = self.user_playlist_items_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.user_playlist_items):
            return None
        return dict(self.user_playlist_items[index], user_playlist_index=self.current_user_playlist_index, user_playlist_item_index=index)

    def on_user_playlist_items_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.play_selected_user_playlist_item()
        elif self.shortcut_matches(event, "download_audio"):
            self.start_download(True, item=self.selected_user_playlist_item())
        elif self.shortcut_matches(event, "download_video"):
            self.start_download(False, item=self.selected_user_playlist_item())
        elif self.shortcut_matches(event, "remove_from_playlist") or self.shortcut_matches(event, "remove_selected"):
            self.remove_selected_user_playlist_item()
        elif self.context_menu_shortcut_matches(event):
            self.open_user_playlist_items_context_menu()
        else:
            event.Skip()

    def open_user_playlist_items_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("play"), self.play_selected_user_playlist_item),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.start_download(True, item=self.selected_user_playlist_item())),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.start_download(False, item=self.selected_user_playlist_item())),
            (self.t("download_user_playlist"), self.download_current_user_playlist),
            (self.menu_label_with_shortcut("remove_from_playlist", "remove_from_playlist"), self.remove_selected_user_playlist_item),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_user_playlist_item())),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda: self.copy_direct_stream_url(self.selected_user_playlist_item())),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def play_selected_user_playlist_item(self) -> None:
        item = self.selected_user_playlist_item()
        if not item:
            self.announce_player(self.t("playlist_empty"))
            return
        self.player_return_screen = "user_playlist_items"
        self.player_return_data = {
            "playlist_index": self.current_user_playlist_index,
            "item_index": int(item.get("user_playlist_item_index") or 0),
        }
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(str(item.get("url") or ""), str(item.get("title") or ""))

    def remove_selected_user_playlist_item(self) -> None:
        item = self.selected_user_playlist_item()
        if not item:
            self.announce_player(self.t("playlist_empty"))
            return
        index = int(item.get("user_playlist_item_index") or 0)
        playlist = self.user_playlists[self.current_user_playlist_index]
        items = list(playlist.get("items") or [])
        if 0 <= index < len(items):
            del items[index]
            playlist["items"] = items
            playlist["updated_at"] = time.time()
            self.save_user_playlists()
            self.refresh_user_playlist_items(min(index, len(items) - 1))
            self.announce_player(self.t("removed_from_playlist"))

    def remove_selected_user_playlist(self) -> None:
        if not hasattr(self, "user_playlist_list"):
            return
        index = self.user_playlist_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.user_playlists):
            self.announce_player(self.t("no_playlists"))
            return
        del self.user_playlists[index]
        self.current_user_playlist_index = min(index, len(self.user_playlists) - 1)
        self.save_user_playlists()
        self.refresh_user_playlists()
        self.announce_player(self.t("playlist_removed"))

    def download_selected_user_playlist(self) -> None:
        playlist = self.selected_user_playlist()
        if not playlist:
            self.announce_player(self.t("no_playlists"))
            return
        self.download_user_playlist(playlist)

    def download_current_user_playlist(self) -> None:
        if self.current_user_playlist_index < 0 or self.current_user_playlist_index >= len(self.user_playlists):
            self.announce_player(self.t("no_playlists"))
            return
        self.download_user_playlist(self.user_playlists[self.current_user_playlist_index])

    def download_user_playlist(self, playlist: dict) -> None:
        title = str(playlist.get("title") or self.t("playlists"))
        folder = str(self.music_download_folder() / self.safe_folder_name(title))
        items = [dict(item, audio_only=False, download_folder_override=folder) for item in list(playlist.get("items") or []) if item.get("url")]
        if not items:
            self.announce_player(self.t("playlist_empty"))
            return
        self.announce_player(self.t("batch_download_start", count=len(items)))
        task_id, cancel_event = self.register_download_task({"title": title, "kind": "playlist"}, False, "batch", total=len(items))
        done_text = self.t("download_playlist_done", title=title)
        threading.Thread(target=self.download_batch_worker, args=(items, task_id, cancel_event, done_text, folder), daemon=True).start()

    def add_active_to_playlist(self) -> None:
        items = self.playlist_candidate_items()
        if not items:
            self.message(self.t("no_selection"))
            return
        playlist_index = self.choose_or_create_playlist_index()
        if playlist_index is None:
            return
        self.add_items_to_playlist(playlist_index, items)

    def playlist_candidate_items(self) -> list[dict]:
        queued_items = [dict(item) for item in self.download_queue.values() if self.playlist_item_is_supported(item)]
        if len(queued_items) > 1:
            return queued_items
        item = self.active_item()
        if item and self.playlist_item_is_supported(item):
            return [dict(item)]
        return queued_items

    @staticmethod
    def playlist_item_is_supported(item: dict | None) -> bool:
        return bool(item and item.get("url") and item.get("kind") not in {"channel", "playlist", "podcast"})

    def choose_or_create_playlist_index(self, initial_item: dict | None = None) -> int | None:
        if not self.user_playlists:
            return self.create_user_playlist_dialog(initial_item=initial_item)
        if len(self.user_playlists) == 1:
            return 0
        choices = [str(playlist.get("title") or "") for playlist in self.user_playlists]
        with wx.SingleChoiceDialog(self, self.t("select_playlist"), self.t("add_to_playlist"), choices) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return None
            index = dialog.GetSelection()
        return index if 0 <= index < len(self.user_playlists) else None

    def add_items_to_playlist(self, playlist_index: int, items: list[dict]) -> None:
        if playlist_index < 0 or playlist_index >= len(self.user_playlists):
            return
        playlist = self.user_playlists[playlist_index]
        existing_urls = {str(item.get("url") or "") for item in playlist.get("items") or []}
        added: list[dict] = []
        for item in items:
            url = str(item.get("url") or "")
            if not url or url in existing_urls:
                continue
            playlist_item = self.playlist_item_from_media(item)
            playlist.setdefault("items", []).append(playlist_item)
            existing_urls.add(url)
            added.append(playlist_item)
        if not added:
            self.announce_player(self.t("playlist_exists"))
            return
        playlist["updated_at"] = time.time()
        self.save_user_playlists()
        title = str(playlist.get("title") or "")
        if len(added) == 1:
            self.announce_player(self.t("added_to_playlist", playlist=title, title=added[0].get("title", "")))
        else:
            self.announce_player(self.t("added_to_playlist_count", playlist=title, count=len(added)))
        if self.user_playlist_items_screen_active:
            self.refresh_user_playlist_items()

    def playlist_item_from_media(self, item: dict) -> dict:
        keys = [
            "title",
            "channel",
            "channel_url",
            "channel_id",
            "views",
            "view_count",
            "age",
            "duration",
            "duration_seconds",
            "timestamp",
            "upload_date",
            "description",
            "type",
            "kind",
            "url",
            "webpage_url",
        ]
        playlist_item = {key: item.get(key, "") for key in keys}
        playlist_item["kind"] = playlist_item.get("kind") or "video"
        playlist_item["type"] = playlist_item.get("type") or self.t("video")
        playlist_item["added_at"] = time.time()
        return playlist_item

    def append_add_to_playlist_menu(self, menu: wx.Menu) -> None:
        if self.user_playlists:
            submenu = wx.Menu()
            for index, playlist in enumerate(self.user_playlists):
                menu_item = submenu.Append(wx.ID_ANY, str(playlist.get("title") or self.t("playlists")))
                self.Bind(wx.EVT_MENU, lambda _evt, idx=index: self.add_items_to_playlist(idx, self.playlist_candidate_items()), menu_item)
            create_item = submenu.Append(wx.ID_ANY, self.t("create_playlist"))
            self.Bind(wx.EVT_MENU, lambda _evt: self.add_active_to_playlist(), create_item)
            menu.AppendSubMenu(submenu, self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"))
        else:
            item = menu.Append(wx.ID_ANY, self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"))
            self.Bind(wx.EVT_MENU, lambda _evt: self.add_active_to_playlist(), item)

    def show_notification_center(self) -> None:
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = True
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("play"), self.open_selected_notification),
                (self.t("clear_notifications"), self.clear_notifications),
            ]
        )
        label = wx.StaticText(self.panel, label=self.t("notification_center"))
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.notification_list = wx.ListBox(self.panel, choices=[])
        self.notification_list.SetName(self.t("notification_center"))
        self.notification_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.open_selected_notification())
        self.notification_list.Bind(wx.EVT_CONTEXT_MENU, self.open_notification_context_menu)
        self.notification_list.Bind(wx.EVT_KEY_DOWN, self.on_notification_key)
        self.root_sizer.Add(self.notification_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_notification_center()
        self.panel.Layout()
        self.focus_later(self.notification_list)

    def refresh_notification_center(self) -> None:
        if not hasattr(self, "notification_list"):
            return
        try:
            if self.notifications:
                self.set_listbox_items(self.notification_list, [self.notification_line(notification) for notification in self.notifications], 0)
            else:
                self.set_listbox_items(self.notification_list, [self.t("notification_center_empty")], 0)
                self.set_status(self.t("notification_center_empty"))
        except RuntimeError:
            pass

    def notification_line(self, notification: dict) -> str:
        when = self.format_history_time(notification.get("timestamp"))
        item = notification.get("item") or {}
        parts = [
            notification.get("title", ""),
            notification.get("message", ""),
            item.get("title", ""),
            f"{self.t('channel')}: {item.get('channel', '')}" if item.get("channel") else "",
            when,
        ]
        return " | ".join(part for part in parts if part)

    def selected_notification(self) -> dict | None:
        if not hasattr(self, "notification_list"):
            return None
        index = self.notification_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.notifications):
            return None
        return self.notifications[index]

    def selected_notification_item(self) -> dict | None:
        notification = self.selected_notification()
        if not notification:
            return None
        item = notification.get("item")
        return dict(item) if isinstance(item, dict) else None

    def on_notification_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.open_selected_notification()
        elif self.shortcut_matches(event, "remove_selected"):
            self.clear_selected_notification()
        elif self.context_menu_shortcut_matches(event):
            self.open_notification_context_menu()
        else:
            event.Skip()

    def open_notification_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("play"), self.open_selected_notification),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_notification_item())),
            (self.t("clear_notifications"), self.clear_notifications),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def open_selected_notification(self) -> None:
        item = self.selected_notification_item()
        if not item or not item.get("url"):
            self.announce_player(self.t("notification_center_empty"))
            return
        self.player_return_screen = "notification_center"
        self.player_return_data = {}
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(str(item.get("url") or ""), str(item.get("title") or ""))

    def clear_selected_notification(self) -> None:
        if not hasattr(self, "notification_list"):
            return
        index = self.notification_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.notifications):
            del self.notifications[index]
            self.save_notifications()
            self.refresh_notification_center()

    def clear_notifications(self) -> None:
        self.notifications = []
        self.save_notifications()
        self.refresh_notification_center()
        self.announce_player(self.t("notifications_cleared"))

    def add_app_notification(self, notification: dict) -> None:
        item = notification.get("item") if isinstance(notification.get("item"), dict) else {}
        stored = {
            "kind": notification.get("kind", "info"),
            "title": notification.get("title", APP_NAME),
            "message": notification.get("message", ""),
            "item": item,
            "timestamp": time.time(),
        }
        self.notifications.insert(0, stored)
        self.notifications = self.notifications[:200]
        self.save_notifications()
        if self.notification_center_screen_active:
            self.refresh_notification_center()
        if not self.app_has_focus():
            enabled = self.settings.windows_notifications
            if stored.get("kind") == "subscription":
                enabled = enabled and self.settings.subscription_notifications
            self.show_desktop_notification(str(stored.get("title") or APP_NAME), str(stored.get("message") or ""), enabled=enabled)

    def show_download_queue(self) -> None:
        self.in_main_menu = False
        self.in_queue_screen = True
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        buttons = [(self.t("back"), self.show_main_menu)]
        if self.download_queue:
            buttons.append((self.t("download_all"), self.download_all_queued))
        if self.active_downloads:
            buttons.append((self.t("cancel_download"), self.cancel_selected_download))
            buttons.append((self.t("cancel_all_downloads"), self.cancel_all_downloads))
        self.add_button_row(buttons)
        title = wx.StaticText(self.panel, label=self.t("current_downloads"))
        self.root_sizer.Add(title, 0, wx.ALL, 4)
        instructions = wx.StaticText(self.panel, label=self.t("queued_download_instructions"))
        self.root_sizer.Add(instructions, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.queue_items = self.download_items_snapshot()
        queue_choices = [self.queue_line(item) for item in self.queue_items] or [self.t("no_queued_downloads")]
        self.queue_list = wx.ListBox(self.panel, choices=queue_choices)
        self.queue_list.SetName(self.t("current_downloads"))
        self.queue_list.Bind(wx.EVT_CONTEXT_MENU, self.open_queue_context_menu)
        self.queue_list.Bind(wx.EVT_KEY_DOWN, self.on_queue_key)
        self.root_sizer.Add(self.queue_list, 1, wx.EXPAND | wx.ALL, 4)
        self.queue_list.SetSelection(0)
        self.panel.Layout()
        self.focus_later(self.queue_list)

    def download_items_snapshot(self) -> list[dict]:
        active = sorted(self.active_downloads.values(), key=lambda item: item.get("created_at", 0))
        queued = list(self.download_queue.values())
        return [dict(item, queue_state="active") for item in active] + [dict(item, queue_state="queued") for item in queued]

    def queue_line(self, item: dict) -> str:
        if item.get("queue_state") == "active":
            state = self.t(str(item.get("status_key") or "download_state_downloading"))
            kind = str(item.get("task_kind") or "")
            if kind == "batch":
                completed = int(item.get("completed") or 0)
                total = int(item.get("total") or 0)
                remaining = max(0, total - completed)
                summary = self.t("downloads_remaining", remaining=remaining, total=total) if total else ""
            else:
                total = int(item.get("playlist_count") or 0)
                index = int(item.get("playlist_index") or 0)
                remaining = max(0, total - index) if total and index else 0
                summary = self.t("downloads_remaining", remaining=remaining, total=total) if total and index else ""
            current = item.get("current_title") or item.get("title", "")
            percent = item.get("percent")
            percent_text = self.t("download_percent_value", percent=percent) if percent else ""
            parts = [item.get("title", ""), state, summary, current, percent_text]
            return " | ".join(part for part in parts if part)
        mode = self.queue_mode_label(item)
        parts = [
            item.get("title", ""),
            item.get("type", ""),
            f"{self.t('channel')}: {item.get('channel', '')}" if item.get("channel") and item.get("kind") == "video" else "",
            mode,
            self.t("download_state_queued"),
        ]
        return " | ".join(part for part in parts if part)

    def queue_mode_label(self, item: dict) -> str:
        if item.get("kind") == "rss_item":
            return self.t("podcast_audio_queued_marker")
        if item.get("kind") in {"playlist", "channel"}:
            if item.get("audio_only"):
                return self.t("collection_audio_queued_marker")
            return self.t("collection_video_queued_marker")
        return self.t("audio_queued_marker" if item.get("audio_only") else "video_queued_marker")

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
        if self.shortcut_matches(event, "download_audio"):
            self.download_selected_queue_item(True)
        elif self.shortcut_matches(event, "download_video"):
            self.download_selected_queue_item(False)
        elif self.shortcut_matches(event, "open_selected"):
            self.download_selected_queue_item()
        elif self.context_menu_shortcut_matches(event):
            self.open_queue_context_menu()
        else:
            event.Skip()

    def open_queue_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        item = self.selected_queue_item()
        if item and item.get("queue_state") == "active":
            actions = [
                (self.t("cancel_download"), self.cancel_selected_download),
                (self.t("cancel_all_downloads"), self.cancel_all_downloads),
            ]
        else:
            actions = [
                (self.t("download_selected_queued"), lambda: self.download_selected_queue_item()),
                (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.download_selected_queue_item(True)),
                (self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.download_selected_queue_item(False)),
                (self.t("download_all"), self.download_all_queued),
                (self.t("remove_from_queue"), self.remove_selected_queue_item),
            ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def show_search(self, restore_search: bool = False) -> None:
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = True
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row([(self.t("back"), self.back_from_search)])
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
            choices=[self.t("all"), self.t("video"), self.t("playlist"), self.t("channel")],
        )
        self.search_type.SetName(self.t("type"))
        restored_type_index = self.last_search_type_index if restore_search else 0
        self.search_type.SetSelection(restored_type_index if 0 <= restored_type_index < self.search_type.GetCount() else 0)
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
        self.results_list = wx.ListBox(self.panel, choices=[self.t("search_results_empty")])
        self.results_list.SetName(self.t("search_youtube"))
        self.results_list.SetSelection(0)
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        if not restore_search:
            self.focus_later(self.query)

    def back_from_search(self) -> None:
        if self.search_results_stack:
            self.restore_previous_search_results()
        else:
            self.show_main_menu()

    def on_results_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "queue_audio"):
            self.toggle_download_queue(True)
        elif self.shortcut_matches(event, "queue_video"):
            self.toggle_download_queue(False)
        elif self.shortcut_matches(event, "download_audio"):
            self.download_audio_shortcut()
        elif self.shortcut_matches(event, "download_video"):
            self.download_video_shortcut()
        elif self.shortcut_matches(event, "subscribe_channel"):
            self.subscribe_shortcut()
        elif self.shortcut_matches(event, "open_selected"):
            self.play_selected()
        elif self.context_menu_shortcut_matches(event):
            self.open_context_menu()
        else:
            event.Skip()
            wx.CallAfter(self.maybe_extend_results)

    def on_results_selection(self, event) -> None:
        event.Skip()
        self.maybe_extend_results()

    def show_favorites(self) -> None:
        self.in_main_menu = False
        self.search_screen_active = False
        self.favorites_screen_active = True
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
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
        self.favorites_list.Bind(wx.EVT_CONTEXT_MENU, self.open_favorites_context_menu)
        self.favorites_list.Bind(wx.EVT_KEY_DOWN, self.on_favorites_key)
        self.root_sizer.Add(self.favorites_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_favorites()
        self.panel.Layout()
        self.focus_later(self.favorites_list)

    def on_favorites_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.play_favorite()
        elif self.shortcut_matches(event, "subscribe_channel"):
            self.subscribe_to_selected_channel(self.selected_favorite())
        elif self.context_menu_shortcut_matches(event):
            self.open_favorites_context_menu()
        else:
            event.Skip()

    def open_favorites_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("play"), self.play_favorite),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.start_download(True, item=self.selected_favorite())),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.start_download(False, item=self.selected_favorite())),
            (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), lambda: self.subscribe_to_selected_channel(self.selected_favorite())),
            (self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"), self.add_active_to_playlist),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda: self.copy_direct_stream_url(self.selected_favorite())),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_favorite())),
            (self.t("remove"), self.remove_favorite),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def show_history(self) -> None:
        if not self.settings.enable_history:
            self.show_main_menu()
            return
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = True
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("play"), self.play_history_item),
                (self.t("remove_history_item"), self.remove_history_item),
                (self.t("clear_history"), self.clear_history),
            ]
        )
        self.history_list = wx.ListBox(self.panel, choices=[])
        self.history_list.SetName(self.t("history"))
        self.history_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_history_item())
        self.history_list.Bind(wx.EVT_CONTEXT_MENU, self.open_history_context_menu)
        self.history_list.Bind(wx.EVT_KEY_DOWN, self.on_history_key)
        self.root_sizer.Add(self.history_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_history()
        self.panel.Layout()
        self.focus_later(self.history_list)

    def refresh_history(self) -> None:
        if not hasattr(self, "history_list"):
            return
        try:
            if self.history:
                self.set_listbox_items(self.history_list, [self.history_line(item) for item in self.history], 0)
            else:
                self.set_listbox_items(self.history_list, [self.t("history_empty")], 0)
                self.set_status(self.t("history_empty"))
        except RuntimeError:
            pass

    def history_line(self, item: dict) -> str:
        when = self.format_history_time(item.get("timestamp"))
        action = str(item.get("action") or "")
        parts = [when, action, item.get("title", ""), f"{self.t('channel')}: {item.get('channel', '')}" if item.get("channel") else ""]
        return " | ".join(part for part in parts if part)

    def selected_history_item(self) -> dict | None:
        if not hasattr(self, "history_list"):
            return None
        index = self.history_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.history):
            return None
        return self.history[index]

    def on_history_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.play_history_item()
        elif self.shortcut_matches(event, "download_audio"):
            self.start_download(True, item=self.selected_history_item())
        elif self.shortcut_matches(event, "download_video"):
            self.start_download(False, item=self.selected_history_item())
        elif self.shortcut_matches(event, "subscribe_channel"):
            self.subscribe_to_selected_channel(self.selected_history_item())
        elif self.shortcut_matches(event, "remove_selected"):
            self.remove_history_item()
        elif self.context_menu_shortcut_matches(event):
            self.open_history_context_menu()
        else:
            event.Skip()

    def open_history_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("play"), self.play_history_item),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.start_download(True, item=self.selected_history_item())),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.start_download(False, item=self.selected_history_item())),
            (self.t("add_favorite"), lambda: self.add_favorite_item(self.selected_history_item())),
            (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), lambda: self.subscribe_to_selected_channel(self.selected_history_item())),
            (self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"), self.add_active_to_playlist),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda: self.copy_direct_stream_url(self.selected_history_item())),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_history_item())),
            (self.t("remove_history_item"), self.remove_history_item),
            (self.t("clear_history"), self.clear_history),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def play_history_item(self) -> None:
        item = self.selected_history_item()
        if not item or not item.get("url"):
            self.announce_player(self.t("no_selection"))
            return
        self.open_library_item(dict(item), "history")

    def remove_history_item(self) -> None:
        if not hasattr(self, "history_list"):
            return
        index = self.history_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.history):
            del self.history[index]
            self.save_history()
            self.refresh_history()
            self.announce_player(self.t("history_removed"))

    def clear_history(self) -> None:
        self.history = []
        self.save_history()
        self.refresh_history()
        self.announce_player(self.t("history_cleared"))

    def show_subscriptions(self) -> None:
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = True
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("subscription_check_now"), lambda: self.check_subscriptions(manual=True)),
                (self.t("subscription_open_videos"), self.open_selected_subscription_videos),
                (self.t("subscription_new_videos_button"), self.open_selected_subscription_new_videos),
                (self.t("remove"), self.remove_subscription),
            ]
        )
        self.subscriptions_list = wx.ListBox(self.panel, choices=[])
        self.subscriptions_list.SetName(self.t("subscriptions"))
        self.subscriptions_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.open_selected_subscription_videos())
        self.subscriptions_list.Bind(wx.EVT_CONTEXT_MENU, self.open_subscriptions_context_menu)
        self.subscriptions_list.Bind(wx.EVT_KEY_DOWN, self.on_subscriptions_key)
        self.root_sizer.Add(self.subscriptions_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_subscriptions()
        self.panel.Layout()
        self.focus_later(self.subscriptions_list)

    def refresh_subscriptions(self) -> None:
        if not hasattr(self, "subscriptions_list"):
            return
        try:
            selection = self.subscriptions_list.GetSelection()
            if selection == wx.NOT_FOUND:
                selection = 0
            if self.subscriptions:
                self.set_listbox_items(self.subscriptions_list, [self.subscription_line(item) for item in self.subscriptions], selection)
            else:
                self.set_listbox_items(self.subscriptions_list, [self.t("subscription_empty")], 0)
                self.set_status(self.t("subscription_empty"))
        except RuntimeError:
            pass

    def subscription_line(self, item: dict) -> str:
        checked = self.format_history_time(item.get("last_checked")) if item.get("last_checked") else self.t("subscription_never_checked")
        new_count = int(item.get("last_new_count") or 0)
        parts = [
            item.get("title", ""),
            self.t("subscription_last_checked", time=checked) if item.get("last_checked") else checked,
            self.t("subscription_new_videos", count=new_count, title=item.get("title", "")) if new_count else "",
        ]
        return " | ".join(part for part in parts if part)

    def selected_subscription(self) -> dict | None:
        if not hasattr(self, "subscriptions_list"):
            return None
        index = self.subscriptions_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.subscriptions):
            return None
        return self.subscriptions[index]

    def on_subscriptions_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.open_selected_subscription_videos()
        elif self.shortcut_matches(event, "new_subscription_videos"):
            self.open_selected_subscription_new_videos()
        elif self.shortcut_matches(event, "remove_selected"):
            self.remove_subscription()
        elif self.context_menu_shortcut_matches(event):
            self.open_subscriptions_context_menu()
        else:
            event.Skip()

    def open_subscriptions_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("subscription_open_videos"), self.open_selected_subscription_videos),
            (self.t("subscription_new_videos_button"), self.open_selected_subscription_new_videos),
            (self.t("subscription_check_now"), lambda: self.check_subscriptions(manual=True)),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_subscription())),
            (self.t("remove"), self.remove_subscription),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def open_selected_subscription_videos(self) -> None:
        item = self.selected_subscription()
        if not item:
            self.message(self.t("no_selection"))
            return
        channel_item = {
            "title": item.get("title", ""),
            "url": item.get("url", ""),
            "kind": "channel",
            "type": self.t("channel"),
            "channel": item.get("title", ""),
        }
        self.open_library_item(channel_item, "subscriptions")

    def open_selected_subscription_new_videos(self) -> None:
        item = self.selected_subscription()
        if not item:
            self.message(self.t("no_selection"))
            return
        new_items = list(item.get("last_new_items") or [])
        if not new_items:
            self.announce_player(self.t("subscription_no_saved_new_videos"))
            return
        self.search_results_stack.append({"screen": "subscriptions"})
        self.last_search_query = self.t("subscription_new_videos_title", title=item.get("title", ""))
        self.last_search_type_index = 0
        self.current_search_type_code = "Video"
        self.collection_url = ""
        self.collection_result_type = ""
        self.loading_more_results = False
        self.dynamic_fetch_enabled = False
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        self.show_search(restore_search=True)
        self.show_results(new_items, selection=0, visible_count=len(new_items))
        wx.CallAfter(self.focus_results_list, 0)

    def remove_subscription(self) -> None:
        if not hasattr(self, "subscriptions_list"):
            return
        index = self.subscriptions_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.subscriptions):
            item = self.subscriptions.pop(index)
            self.save_subscriptions()
            self.refresh_subscriptions()
            self.announce_player(self.t("subscription_removed", title=item.get("title", "")))

    def subscribe_shortcut(self) -> None:
        self.subscribe_to_selected_channel(self.active_item())

    def subscribe_to_selected_channel(self, item: dict | None) -> None:
        if not item:
            self.message(self.t("no_selection"))
            return
        subscription = self.subscription_from_item(item)
        if not subscription:
            self.message(self.t("no_selection"))
            return
        url = subscription["url"]
        existing = next((sub for sub in self.subscriptions if sub.get("url") == url), None)
        if existing:
            self.announce_player(self.t("subscription_exists", title=existing.get("title", "")))
            return
        self.subscriptions.insert(0, subscription)
        self.save_subscriptions()
        self.refresh_subscriptions()
        self.announce_player(self.t("subscription_added", title=subscription.get("title", "")))

    def subscription_from_item(self, item: dict) -> dict | None:
        kind = item.get("kind")
        channel_url = str(item.get("channel_url") or "").strip()
        if kind == "channel":
            channel_url = str(item.get("url") or channel_url).strip()
        if not channel_url:
            return None
        title = item.get("channel") or item.get("title") or channel_url
        latest_urls = [item.get("url")] if item.get("kind") == "video" and item.get("url") else []
        return {
            "title": title,
            "url": channel_url,
            "latest_urls": latest_urls,
            "last_checked": 0.0,
            "last_new_count": 0,
            "created_at": time.time(),
        }

    def configure_subscription_timer(self) -> None:
        if not hasattr(self, "subscription_timer"):
            return
        try:
            self.subscription_timer.Stop()
        except Exception:
            pass
        if self.settings.subscription_check_enabled:
            interval_ms = max(1, int(self.settings.subscription_check_interval_hours or 6)) * 60 * 60 * 1000
            self.subscription_timer.Start(interval_ms)

    def on_subscription_timer(self, _event) -> None:
        self.check_subscriptions_if_due()

    def configure_rss_timer(self) -> None:
        if not hasattr(self, "rss_timer"):
            return
        try:
            self.rss_timer.Stop()
        except Exception:
            pass
        if self.settings.enable_podcasts_rss and self.settings.rss_auto_refresh_enabled:
            interval_ms = max(1, int(self.settings.rss_refresh_interval_hours or 12)) * 60 * 60 * 1000
            self.rss_timer.Start(interval_ms)

    def on_rss_timer(self, _event) -> None:
        self.refresh_all_rss_feeds_background()

    def check_subscriptions_if_due(self) -> None:
        if not self.settings.subscription_check_enabled or not self.subscriptions:
            return
        interval_seconds = max(1, int(self.settings.subscription_check_interval_hours or 6)) * 60 * 60
        last_check = float(getattr(self.settings, "last_subscription_check", 0.0) or 0.0)
        if time.time() - last_check >= interval_seconds:
            self.check_subscriptions(manual=False)

    def check_subscriptions(self, manual: bool = False) -> None:
        if self.subscription_check_running:
            return
        if not self.subscriptions:
            if manual:
                self.announce_player(self.t("subscription_empty"))
            return
        self.subscription_check_running = True
        if manual:
            self.announce_player(self.t("subscription_checking"))
        threading.Thread(target=self.check_subscriptions_worker, args=(manual,), daemon=True).start()

    def check_subscriptions_worker(self, manual: bool = False) -> None:
        try:
            total_new = 0
            failures = 0
            successes = 0
            updated_subscriptions = []
            for subscription in self.subscriptions:
                try:
                    updated, new_items = self.check_one_subscription(subscription)
                except Exception as exc:
                    failures += 1
                    updated = dict(subscription)
                    updated["last_checked"] = time.time()
                    updated["last_error"] = self.friendly_error(exc)
                    updated_subscriptions.append(updated)
                    if manual:
                        self.ui_queue.put(("announce", self.t("subscription_check_failed", error=self.friendly_error(exc))))
                    continue
                successes += 1
                updated_subscriptions.append(updated)
                if new_items:
                    total_new += len(new_items)
                    message = self.t("subscription_new_videos", count=len(new_items), title=updated.get("title", ""))
                    self.ui_queue.put(("announce", message))
                    for entry in new_items[:20]:
                        title = str(entry.get("title") or "")
                        notification_message = self.t("notification_new_video", channel=updated.get("title", ""), title=title)
                        self.ui_queue.put(
                            (
                                "app_notification",
                                {
                                    "kind": "subscription",
                                    "title": self.t("notification_subscription_title"),
                                    "message": notification_message,
                                    "item": entry,
                                },
                            )
                        )
            if updated_subscriptions:
                self.subscriptions = updated_subscriptions
                self.save_subscriptions()
            if successes:
                self.settings.last_subscription_check = time.time()
                self.save_settings()
            self.ui_queue.put(("subscriptions_changed", None))
            if manual and not (failures and not successes):
                key = "subscription_check_complete" if total_new or failures else "subscription_no_new"
                self.ui_queue.put(("announce", self.t(key)))
        except Exception as exc:
            if manual:
                self.ui_queue.put(("announce", self.t("subscription_check_failed", error=exc)))
        finally:
            self.subscription_check_running = False

    def check_one_subscription(self, subscription: dict) -> tuple[dict, list[dict]]:
        entries = self.fetch_subscription_entries(subscription)
        current_urls = [entry.get("url", "") for entry in entries if entry.get("url")]
        known_urls = set(subscription.get("latest_urls") or [])
        if not known_urls:
            new_items: list[dict] = []
        else:
            new_items = [entry for entry in entries if entry.get("url") and entry.get("url") not in known_urls]
        updated = dict(subscription)
        updated["latest_urls"] = current_urls[:20]
        updated["last_checked"] = time.time()
        updated["last_new_count"] = len(new_items)
        updated["last_new_items"] = new_items[:20]
        return updated, new_items

    def fetch_subscription_entries(self, subscription: dict) -> list[dict]:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            raise RuntimeError(self.t("missing_ytdlp"))
        url = self.collection_download_url({"kind": "channel", "url": subscription.get("url", "")})
        options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": 5}
        with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
            info = ydl.extract_info(url, download=False)
        entries = list(info.get("entries") or [])[:5]
        return [self.normalize_entry(entry, "Video") for entry in entries]

    def show_rss_feeds(self) -> None:
        if not self.settings.enable_podcasts_rss:
            self.show_main_menu()
            return
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = True
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("search_podcasts"), self.search_podcasts),
                (self.t("add_rss_feed"), self.add_rss_feed),
                (self.t("refresh_feeds"), self.refresh_all_rss_feeds),
                (self.t("open_feed"), self.open_selected_rss_feed),
                (self.t("remove_feed"), self.remove_rss_feed),
            ]
        )
        self.rss_feed_list = wx.ListBox(self.panel, choices=[])
        self.rss_feed_list.SetName(self.t("rss_feeds"))
        self.rss_feed_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.open_selected_rss_feed())
        self.rss_feed_list.Bind(wx.EVT_CONTEXT_MENU, self.open_rss_feed_context_menu)
        self.rss_feed_list.Bind(wx.EVT_KEY_DOWN, self.on_rss_feed_key)
        self.root_sizer.Add(self.rss_feed_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_rss_feed_list()
        self.panel.Layout()
        self.focus_later(self.rss_feed_list)

    def refresh_rss_feed_list(self) -> None:
        if not hasattr(self, "rss_feed_list"):
            return
        try:
            if self.rss_feeds:
                labels = [self.rss_feed_line(feed) for feed in self.rss_feeds]
                index = min(max(0, self.current_rss_feed_index), len(self.rss_feeds) - 1)
                self.set_listbox_items(self.rss_feed_list, labels, index)
            else:
                self.set_listbox_items(self.rss_feed_list, [self.t("rss_feeds_empty")], 0)
                self.set_status(self.t("rss_feeds_empty"))
        except RuntimeError:
            pass

    def rss_feed_line(self, feed: dict) -> str:
        checked = self.format_history_time(feed.get("last_checked")) if feed.get("last_checked") else self.t("rss_feed_never_checked")
        count = len(feed.get("items") or [])
        parts = [
            feed.get("title") or self.t("rss_unknown_feed_title"),
            self.t("rss_feed_item_count", count=count),
            self.t("rss_feed_last_checked", time=checked) if feed.get("last_checked") else checked,
        ]
        return " | ".join(part for part in parts if part)

    def selected_rss_feed(self) -> dict | None:
        if not hasattr(self, "rss_feed_list"):
            return None
        index = self.rss_feed_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.rss_feeds):
            return None
        self.current_rss_feed_index = index
        return self.rss_feeds[index]

    def on_rss_feed_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.open_selected_rss_feed()
        elif self.shortcut_matches(event, "remove_selected"):
            self.remove_rss_feed()
        elif self.context_menu_shortcut_matches(event):
            self.open_rss_feed_context_menu()
        else:
            event.Skip()

    def open_rss_feed_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("open_feed"), self.open_selected_rss_feed),
            (self.t("download_feed"), self.download_selected_rss_feed),
            (self.t("refresh_feed"), self.refresh_selected_rss_feed),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_rss_feed())),
            (self.t("remove_feed"), self.remove_rss_feed),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def search_podcasts(self) -> None:
        with wx.TextEntryDialog(self, self.t("podcast_search_query"), self.t("search_podcasts")) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            query = dialog.GetValue().strip()
        if not query:
            return
        self.announce_player(self.t("podcast_searching", query=query))
        threading.Thread(target=self.search_podcasts_worker, args=(query,), daemon=True).start()

    def search_podcasts_worker(self, query: str) -> None:
        try:
            provider = self.normalized_podcast_search_provider()
            if provider != PODCAST_DIRECTORY_PROVIDER_APPLE:
                provider = PODCAST_DIRECTORY_PROVIDER_APPLE
            limit = min(200, max(1, int(self.settings.podcast_search_limit or 20)))
            params = {
                "media": "podcast",
                "entity": "podcast",
                "term": query,
                "country": self.normalized_podcast_search_country(),
                "limit": str(limit),
            }
            url = f"https://itunes.apple.com/search?{urlencode(params)}"
            request = Request(url, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
            with self.open_url(request, timeout=30) as response:
                payload = json.loads(response.read().decode("utf-8", errors="replace"))
            results = [self.normalize_podcast_result(item) for item in payload.get("results") or []]
            results = [item for item in results if item.get("url")]
            self.ui_queue.put(("podcast_results", {"query": query, "results": results}))
        except Exception as exc:
            self.ui_queue.put(("announce", self.t("podcast_search_failed", error=self.friendly_error(exc))))

    def normalize_podcast_result(self, item: dict) -> dict:
        title = str(item.get("collectionName") or item.get("trackName") or "").strip()
        author = str(item.get("artistName") or "").strip()
        feed_url = str(item.get("feedUrl") or "").strip()
        page_url = str(item.get("collectionViewUrl") or item.get("trackViewUrl") or "").strip()
        genre = str(item.get("primaryGenreName") or "").strip()
        count = int(item.get("trackCount") or 0)
        return {
            "title": title or feed_url or page_url,
            "channel": author,
            "author": author,
            "genre": genre,
            "episode_count": count,
            "url": feed_url,
            "webpage_url": page_url or feed_url,
            "kind": "podcast",
            "type": self.t("rss_feeds"),
        }

    def show_podcast_search_results(self, results: list[dict], query: str = "") -> None:
        self.podcast_search_results = list(results)
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = True
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_rss_feeds),
                (self.t("add_podcast"), self.add_selected_podcast_result),
                (self.t("open_browser"), self.open_selected_in_browser),
            ]
        )
        label = wx.StaticText(self.panel, label=self.t("podcast_search_results"))
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.podcast_result_list = wx.ListBox(self.panel, choices=[])
        self.podcast_result_list.SetName(self.t("podcast_search_results"))
        self.podcast_result_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.add_selected_podcast_result())
        self.podcast_result_list.Bind(wx.EVT_CONTEXT_MENU, self.open_podcast_search_context_menu)
        self.podcast_result_list.Bind(wx.EVT_KEY_DOWN, self.on_podcast_search_key)
        self.root_sizer.Add(self.podcast_result_list, 1, wx.EXPAND | wx.ALL, 4)
        for item in self.podcast_search_results:
            self.podcast_result_list.Append(self.podcast_result_line(item))
        if self.podcast_search_results:
            self.podcast_result_list.SetSelection(0)
            self.set_status(self.t("podcast_search_done", count=len(self.podcast_search_results)))
        else:
            self.podcast_result_list.Append(self.t("podcast_search_empty"))
            self.podcast_result_list.SetSelection(0)
            self.set_status(self.t("podcast_search_empty"))
        self.panel.Layout()
        self.focus_later(self.podcast_result_list)

    def podcast_result_line(self, item: dict) -> str:
        count = int(item.get("episode_count") or 0)
        parts = [
            item.get("title", ""),
            f"{self.t('podcast_author')}: {item.get('author', '')}" if item.get("author") else "",
            f"{self.t('podcast_genre')}: {item.get('genre', '')}" if item.get("genre") else "",
            self.t("podcast_episode_count", count=count) if count else "",
        ]
        return " | ".join(part for part in parts if part)

    def selected_podcast_result(self) -> dict | None:
        if not hasattr(self, "podcast_result_list"):
            return None
        index = self.podcast_result_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.podcast_search_results):
            return None
        return self.podcast_search_results[index]

    def on_podcast_search_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.add_selected_podcast_result()
        elif self.context_menu_shortcut_matches(event):
            self.open_podcast_search_context_menu()
        else:
            event.Skip()

    def open_podcast_search_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("add_podcast"), self.add_selected_podcast_result),
            (self.t("open_browser"), self.open_selected_in_browser),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_podcast_result())),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def add_selected_podcast_result(self) -> None:
        item = self.selected_podcast_result()
        if not item:
            self.announce_player(self.t("podcast_search_empty"))
            return
        self.add_rss_feed_url(str(item.get("url") or ""))

    def add_rss_feed(self) -> None:
        with wx.TextEntryDialog(self, self.t("rss_feed_url"), self.t("add_rss_feed")) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            url = dialog.GetValue().strip()
        self.add_rss_feed_url(url)

    def add_rss_feed_url(self, url: str) -> None:
        if not url:
            return
        if not re.match(r"^https?://", url, flags=re.IGNORECASE):
            url = "https://" + url
        if any(str(feed.get("url") or "").rstrip("/") == url.rstrip("/") for feed in self.rss_feeds):
            self.announce_player(self.t("rss_feed_exists"))
            return
        self.announce_player(self.t("rss_refresh_started"))
        threading.Thread(target=self.add_rss_feed_worker, args=(url,), daemon=True).start()

    def add_rss_feed_worker(self, url: str) -> None:
        try:
            feed = self.fetch_rss_feed(url)
            self.rss_feeds.insert(0, feed)
            self.save_rss_feeds()
            self.ui_queue.put(("rss_feeds_changed", None))
            self.ui_queue.put(("announce", self.t("rss_feed_added", title=feed.get("title") or self.t("rss_unknown_feed_title"))))
        except Exception as exc:
            self.ui_queue.put(("announce", self.t("rss_refresh_failed", error=self.friendly_error(exc))))

    def refresh_all_rss_feeds(self) -> None:
        if not self.rss_feeds:
            self.announce_player(self.t("rss_feeds_empty"))
            return
        if self.rss_refresh_running:
            return
        self.rss_refresh_running = True
        self.announce_player(self.t("rss_refresh_started"))
        threading.Thread(target=self.refresh_rss_feeds_worker, args=(None, False), daemon=True).start()

    def refresh_all_rss_feeds_background(self) -> None:
        if not self.settings.enable_podcasts_rss or not self.rss_feeds or self.rss_refresh_running:
            return
        self.rss_refresh_running = True
        threading.Thread(target=self.refresh_rss_feeds_worker, args=(None, True), daemon=True).start()

    def refresh_selected_rss_feed(self) -> None:
        feed = self.selected_rss_feed()
        if not feed:
            self.message(self.t("no_selection"))
            return
        if self.rss_refresh_running:
            return
        self.rss_refresh_running = True
        self.announce_player(self.t("rss_refresh_started"))
        threading.Thread(target=self.refresh_rss_feeds_worker, args=(self.current_rss_feed_index, False), daemon=True).start()

    def refresh_rss_feeds_worker(self, feed_index: int | None, silent: bool = False) -> None:
        try:
            if feed_index is None:
                indexes = range(len(self.rss_feeds))
            else:
                indexes = [feed_index]
            updated_feeds = list(self.rss_feeds)
            failures: list[str] = []
            for index in indexes:
                if index < 0 or index >= len(updated_feeds):
                    continue
                existing = updated_feeds[index]
                try:
                    known_urls = {str(item.get("url") or "") for item in existing.get("items") or [] if item.get("url")}
                    refreshed = self.fetch_rss_feed(str(existing.get("url") or ""))
                    refreshed["created_at"] = existing.get("created_at", refreshed.get("created_at", time.time()))
                    updated_feeds[index] = refreshed
                    if known_urls:
                        for entry in list(refreshed.get("items") or []):
                            url = str(entry.get("url") or "")
                            if url and url not in known_urls:
                                notification_message = self.t("notification_new_podcast", feed=refreshed.get("title", ""), title=entry.get("title", ""))
                                self.ui_queue.put(
                                    (
                                        "app_notification",
                                        {
                                            "kind": "podcast",
                                            "title": self.t("rss_feeds"),
                                            "message": notification_message,
                                            "item": entry,
                                        },
                                    )
                                )
                except Exception as exc:
                    preserved = dict(existing)
                    preserved["last_error"] = self.friendly_error(exc)
                    preserved["last_checked"] = time.time()
                    updated_feeds[index] = preserved
                    failures.append(preserved.get("title") or preserved.get("url") or self.t("rss_unknown_feed_title"))
            self.rss_feeds = updated_feeds
            self.save_rss_feeds()
            self.ui_queue.put(("rss_feeds_changed", None))
            if not silent and failures:
                self.ui_queue.put(("announce", self.t("rss_refresh_failed", error=", ".join(failures[:3]))))
            elif not silent:
                self.ui_queue.put(("announce", self.t("rss_refresh_done")))
        except Exception as exc:
            if not silent:
                self.ui_queue.put(("announce", self.t("rss_refresh_failed", error=self.friendly_error(exc))))
        finally:
            self.rss_refresh_running = False

    def open_selected_rss_feed(self) -> None:
        feed = self.selected_rss_feed()
        if not feed:
            self.message(self.t("no_selection"))
            return
        self.show_rss_items(self.current_rss_feed_index)

    def remove_rss_feed(self) -> None:
        if not hasattr(self, "rss_feed_list"):
            return
        index = self.rss_feed_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.rss_feeds):
            del self.rss_feeds[index]
            self.current_rss_feed_index = min(index, len(self.rss_feeds) - 1)
            self.save_rss_feeds()
            self.refresh_rss_feed_list()
            self.announce_player(self.t("rss_feed_removed"))

    def show_rss_items(self, feed_index: int, selection: int = 0) -> None:
        if feed_index < 0 or feed_index >= len(self.rss_feeds):
            self.show_rss_feeds()
            return
        self.current_rss_feed_index = feed_index
        feed = self.rss_feeds[feed_index]
        self.rss_items = list(feed.get("items") or [])
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = True
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_rss_feeds),
                (self.t("refresh_feed"), self.refresh_selected_rss_feed_from_items),
                (self.t("play_episode"), self.play_selected_rss_item),
                (self.t("download_episode_audio"), self.download_selected_rss_item),
                (self.t("download_feed"), self.download_current_rss_feed),
            ]
        )
        label = wx.StaticText(self.panel, label=feed.get("title") or self.t("rss_feed_items"))
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.rss_items_list = wx.ListBox(self.panel, choices=[])
        self.rss_items_list.SetName(self.t("rss_feed_items"))
        self.rss_items_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected_rss_item())
        self.rss_items_list.Bind(wx.EVT_CONTEXT_MENU, self.open_rss_item_context_menu)
        self.rss_items_list.Bind(wx.EVT_KEY_DOWN, self.on_rss_item_key)
        self.root_sizer.Add(self.rss_items_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_rss_items_list(selection)
        self.panel.Layout()
        self.focus_later(self.rss_items_list)

    def refresh_rss_items_list(self, selection: int = 0) -> None:
        if not hasattr(self, "rss_items_list"):
            return
        try:
            if self.rss_items:
                self.set_listbox_items(self.rss_items_list, [self.rss_item_line(item) for item in self.rss_items], selection)
            else:
                self.set_listbox_items(self.rss_items_list, [self.t("rss_items_empty")], 0)
                self.set_status(self.t("rss_items_empty"))
        except RuntimeError:
            pass

    def rss_item_line(self, item: dict) -> str:
        published = self.format_history_time(item.get("timestamp")) if item.get("timestamp") else ""
        queued = self.t("podcast_audio_queued_marker") if str(item.get("url") or "") in self.download_queue else ""
        parts = [
            item.get("title", ""),
            f"{self.t('published')}: {published}" if published else "",
            item.get("duration", ""),
            item.get("type", self.t("podcast_episode")),
            queued,
        ]
        return " | ".join(part for part in parts if part)

    def selected_rss_item(self) -> dict | None:
        if not hasattr(self, "rss_items_list"):
            return None
        index = self.rss_items_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.rss_items):
            return None
        item = dict(self.rss_items[index])
        item["rss_feed_index"] = self.current_rss_feed_index
        item["rss_item_index"] = index
        return item

    def on_rss_item_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.play_selected_rss_item()
        elif self.shortcut_matches(event, "queue_audio"):
            self.toggle_rss_item_queue()
        elif self.shortcut_matches(event, "download_audio"):
            self.download_selected_rss_item()
        elif self.context_menu_shortcut_matches(event):
            self.open_rss_item_context_menu()
        else:
            event.Skip()

    def open_rss_item_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("play_episode"), self.play_selected_rss_item),
            (self.menu_label_with_shortcut("download_episode_audio", "download_audio"), self.download_selected_rss_item),
            (self.menu_label_with_shortcut("queue_episode_audio", "queue_audio"), self.toggle_rss_item_queue),
            (self.t("download_feed"), self.download_current_rss_feed),
            (self.t("open_episode_page"), self.open_selected_in_browser),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_rss_item())),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def play_selected_rss_item(self) -> None:
        item = self.selected_rss_item()
        if not item:
            self.message(self.t("no_selection"))
            return
        if not item.get("url"):
            self.message(self.t("no_selection"))
            return
        self.player_return_screen = "rss_items"
        self.player_return_data = {
            "feed_index": self.current_rss_feed_index,
            "item_index": int(item.get("rss_item_index") or 0),
        }
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(item.get("url", ""), item.get("title", ""))

    def download_selected_rss_item(self) -> None:
        item = self.selected_rss_item()
        if item and item.get("url"):
            self.start_download(True, item=item)
        else:
            self.message(self.t("no_selection"))

    def toggle_rss_item_queue(self) -> None:
        item = self.selected_rss_item()
        if not item or not item.get("url"):
            self.message(self.t("no_selection"))
            return
        url = str(item.get("url") or "")
        if url in self.download_queue:
            self.download_queue.pop(url, None)
            self.announce_player(self.t("download_deselected", title=item.get("title", "")))
        else:
            queued = dict(item)
            queued["audio_only"] = True
            self.download_queue[url] = queued
            self.announce_player(self.t("podcast_episode_audio_selected_download", title=item.get("title", "")))
        self.refresh_rss_items_list(int(item.get("rss_item_index") or 0))
        self.refresh_download_views()

    def download_selected_rss_feed(self) -> None:
        feed = self.selected_rss_feed()
        if not feed:
            self.message(self.t("no_selection"))
            return
        self.download_rss_feed(feed)

    def download_current_rss_feed(self) -> None:
        if self.current_rss_feed_index < 0 or self.current_rss_feed_index >= len(self.rss_feeds):
            self.message(self.t("no_selection"))
            return
        self.download_rss_feed(self.rss_feeds[self.current_rss_feed_index])

    def download_rss_feed(self, feed: dict) -> None:
        items = [dict(item, audio_only=True) for item in list(feed.get("items") or []) if item.get("url")]
        if not items:
            self.announce_player(self.t("rss_items_empty"))
            return
        title = feed.get("title") or self.t("rss_unknown_feed_title")
        for item in items:
            item.setdefault("channel", title)
            item.setdefault("kind", "rss_item")
            item.setdefault("type", self.t("podcast_episode"))
        self.announce_player(self.t("download_feed_start"))
        self.set_status(self.t("download_feed_start"))
        task_id, cancel_event = self.register_download_task({"title": title, "kind": "rss_feed"}, True, "rss_feed", total=len(items))
        self.refresh_download_views()
        finish_folder = str(self.podcasts_download_folder() / self.safe_folder_name(str(title)))
        done_text = self.t("download_feed_done", title=title)
        threading.Thread(target=self.download_batch_worker, args=(items, task_id, cancel_event, done_text, finish_folder), daemon=True).start()

    def refresh_selected_rss_feed_from_items(self) -> None:
        if self.current_rss_feed_index < 0 or self.current_rss_feed_index >= len(self.rss_feeds):
            self.show_rss_feeds()
            return
        if self.rss_refresh_running:
            return
        self.rss_refresh_running = True
        self.announce_player(self.t("rss_refresh_started"))
        threading.Thread(target=self.refresh_rss_feeds_worker, args=(self.current_rss_feed_index, False), daemon=True).start()

    def fetch_rss_feed(self, url: str) -> dict:
        request = Request(url, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=30) as response:
            final_url = response.geturl()
            raw = response.read(3_000_000)
        root = ET.fromstring(raw)
        title, site_url, items = self.parse_feed_root(root, final_url)
        return {
            "title": title or self.t("rss_unknown_feed_title"),
            "url": final_url or url,
            "site_url": site_url,
            "items": items[: min(500, max(1, int(self.settings.rss_max_items or 100)))],
            "last_checked": time.time(),
            "created_at": time.time(),
        }

    def parse_feed_root(self, root: ET.Element, base_url: str) -> tuple[str, str, list[dict]]:
        root_name = self.xml_local_name(root.tag)
        if root_name == "feed":
            return self.parse_atom_feed(root, base_url)
        channel = self.first_child(root, "channel") or root
        title = self.child_text(channel, "title")
        site_url = self.absolute_url(self.child_text(channel, "link"), base_url)
        items = [self.parse_rss_item(item, base_url, title) for item in self.children(channel, "item")]
        return title, site_url, [item for item in items if item.get("title") or item.get("url")]

    def parse_rss_item(self, item: ET.Element, base_url: str, feed_title: str) -> dict:
        title = self.child_text(item, "title")
        page_url = self.absolute_url(self.child_text(item, "link"), base_url)
        media_url = ""
        for child in list(item):
            name = self.xml_local_name(child.tag)
            if name in {"enclosure", "content"} and child.get("url"):
                media_url = self.absolute_url(str(child.get("url") or ""), base_url)
                break
        timestamp = self.parse_feed_timestamp(self.child_text(item, "pubDate") or self.child_text(item, "published"))
        description = self.strip_html(self.child_text(item, "description") or self.child_text(item, "summary") or self.child_text(item, "content"))
        guid = self.child_text(item, "guid")
        url = media_url or page_url or self.absolute_url(guid, base_url)
        duration = self.child_text(item, "duration")
        return {
            "title": title or page_url or media_url,
            "url": url,
            "webpage_url": page_url or url,
            "media_url": media_url,
            "description": description,
            "duration": duration,
            "timestamp": timestamp,
            "channel": feed_title,
            "kind": "rss_item",
            "type": self.t("podcast_episode"),
        }

    def parse_atom_feed(self, root: ET.Element, base_url: str) -> tuple[str, str, list[dict]]:
        title = self.child_text(root, "title")
        site_url = self.atom_link(root, base_url, {"alternate", ""})
        items = [self.parse_atom_item(entry, base_url, title) for entry in self.children(root, "entry")]
        return title, site_url, [item for item in items if item.get("title") or item.get("url")]

    def parse_atom_item(self, entry: ET.Element, base_url: str, feed_title: str) -> dict:
        title = self.child_text(entry, "title")
        page_url = self.atom_link(entry, base_url, {"alternate", ""})
        media_url = self.atom_link(entry, base_url, {"enclosure"})
        timestamp = self.parse_feed_timestamp(self.child_text(entry, "published") or self.child_text(entry, "updated"))
        description = self.strip_html(self.child_text(entry, "summary") or self.child_text(entry, "content"))
        item_id = self.child_text(entry, "id")
        url = media_url or page_url or self.absolute_url(item_id, base_url)
        duration = self.child_text(entry, "duration")
        return {
            "title": title or page_url or media_url,
            "url": url,
            "webpage_url": page_url or url,
            "media_url": media_url,
            "description": description,
            "duration": duration,
            "timestamp": timestamp,
            "channel": feed_title,
            "kind": "rss_item",
            "type": self.t("podcast_episode"),
        }

    def atom_link(self, element: ET.Element, base_url: str, rels: set[str]) -> str:
        for child in self.children(element, "link"):
            rel = str(child.get("rel") or "").lower()
            if rel in rels:
                href = str(child.get("href") or "").strip()
                if href:
                    return self.absolute_url(href, base_url)
        return ""

    @staticmethod
    def xml_local_name(tag: str) -> str:
        return str(tag).split("}", 1)[-1].lower()

    def children(self, element: ET.Element, local_name: str) -> list[ET.Element]:
        return [child for child in list(element) if self.xml_local_name(child.tag) == local_name.lower()]

    def first_child(self, element: ET.Element, local_name: str) -> ET.Element | None:
        for child in self.children(element, local_name):
            return child
        return None

    def child_text(self, element: ET.Element, local_name: str) -> str:
        child = self.first_child(element, local_name)
        if child is None:
            return ""
        return "".join(child.itertext()).strip()

    @staticmethod
    def absolute_url(value: str, base_url: str) -> str:
        value = str(value or "").strip()
        if not value:
            return ""
        return urljoin(base_url, value)

    @staticmethod
    def parse_feed_timestamp(value: str) -> float:
        value = str(value or "").strip()
        if not value:
            return 0.0
        try:
            return parsedate_to_datetime(value).timestamp()
        except Exception:
            pass
        try:
            return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()
        except Exception:
            return 0.0

    @staticmethod
    def strip_html(value: str) -> str:
        text = re.sub(r"<br\s*/?>", "\n", str(value or ""), flags=re.IGNORECASE)
        text = re.sub(r"<[^>]+>", " ", text)
        text = re.sub(r"[ \t]+", " ", text)
        text = re.sub(r"\n\s+", "\n", text)
        return text.strip()

    def show_settings(self) -> None:
        self.in_main_menu = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
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
            (self.t("library_section"), "library"),
            (self.t("podcasts_section"), "podcasts"),
            (self.t("notifications_section"), "notifications"),
            (self.t("cookies_network_section"), "cookies"),
            (self.t("keyboard_shortcuts_section"), "shortcuts"),
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
            result_limit_options = ["0", "10", "20", "50", "100", "150", "200", "250"]
            choice("results_limit", results_limit_value, result_limit_options, self.result_limit_labels(result_limit_options))
            choice("direct_link_enter_action", self.normalized_direct_link_enter_action(), DIRECT_LINK_ENTER_OPTIONS, self.direct_link_enter_action_labels())
            check("auto_update", self.settings.auto_update_ytdlp)
            check("auto_update_app", self.settings.auto_update_app)
            button("check_app_updates_now", self.manual_app_update_check)
            check("close_to_tray", self.settings.close_to_tray)
            check("tray_notification", self.settings.tray_notification)
        elif section_name == "playback":
            choice("player_speed", self.settings.player_speed, [self.format_playback_rate(step) for step in PLAYBACK_SPEED_STEPS if step <= 2.0])
            choice("speed_audio_mode", self.normalized_speed_audio_mode(), SPEED_AUDIO_MODE_OPTIONS, self.speed_audio_mode_labels())
            choice("pitch_mode", self.normalized_pitch_mode(), PITCH_MODE_OPTIONS, self.pitch_mode_labels())
            choice("speed_step", self.format_step_value(self.settings.speed_step), RATE_STEP_OPTIONS)
            choice("pitch_step", self.format_step_value(self.settings.pitch_step), RATE_STEP_OPTIONS)
            check("show_video_details_by_default", self.settings.show_video_details_by_default)
            check("enable_stream_cache", self.settings.enable_stream_cache)
            text("cache_folder", self.settings.cache_folder or str(DEFAULT_CACHE_DIR))
            choice("cache_size_mb", str(self.settings.cache_size_mb), ["128", "256", "512", "1024", "2048", "4096"])
            check("resume_playback", self.settings.resume_playback)
            device_values, device_labels = self.audio_output_device_options()
            choice("default_audio_device", self.normalized_audio_output_device(), device_values, device_labels)
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
        elif section_name == "library":
            check("enable_history", self.settings.enable_history)
            choice("history_limit", str(self.settings.history_limit), ["100", "250", "500", "1000", "2000"])
            check("subscription_check_enabled", self.settings.subscription_check_enabled)
            choice("subscription_check_interval", str(self.settings.subscription_check_interval_hours), ["1", "2", "3", "6", "12", "24"])
            button("subscription_check_now", lambda: self.check_subscriptions(manual=True))
        elif section_name == "podcasts":
            check("enable_podcasts_rss", self.settings.enable_podcasts_rss)
            text("podcast_source", self.t("podcast_source_info"), wx.TE_READONLY | wx.TE_MULTILINE)
            choice("podcast_search_provider", self.normalized_podcast_search_provider(), PODCAST_DIRECTORY_PROVIDER_OPTIONS, [self.t("podcast_search_provider_apple")])
            choice("podcast_search_country", self.normalized_podcast_search_country(), PODCAST_COUNTRY_OPTIONS)
            choice("podcast_search_limit", str(self.settings.podcast_search_limit), ["10", "20", "50", "100", "150", "200"])
            choice("rss_max_items", str(self.settings.rss_max_items), ["25", "50", "100", "200", "500"])
            check("rss_refresh_on_startup", self.settings.rss_refresh_on_startup)
            check("rss_auto_refresh_enabled", self.settings.rss_auto_refresh_enabled)
            choice("rss_refresh_interval", str(self.settings.rss_refresh_interval_hours), ["1", "2", "3", "6", "12", "24"])
        elif section_name == "notifications":
            check("windows_notifications", self.settings.windows_notifications)
            check("download_notifications", self.settings.download_notifications)
            check("subscription_notifications", self.settings.subscription_notifications)
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
        elif section_name == "shortcuts":
            form.Add(wx.StaticText(self.settings_scroller, label=self.t("keyboard_shortcuts_help")), 0, wx.ALIGN_CENTER_VERTICAL)
            form.AddSpacer(1)
            shortcuts = self.normalized_keyboard_shortcuts(getattr(self.settings, "keyboard_shortcuts", {}) or {})
            for action, label_key in SHORTCUT_DEFINITIONS:
                form.Add(wx.StaticText(self.settings_scroller, label=self.t(label_key)), 0, wx.ALIGN_CENTER_VERTICAL)
                ctrl = wx.TextCtrl(self.settings_scroller, value=shortcuts[action], style=wx.TE_PROCESS_ENTER)
                ctrl.SetName(f"{self.t(label_key)}. {self.t('shortcut_capture_hint')}")
                setattr(ctrl, "_apricot_shortcut_capture", True)
                setattr(ctrl, "_apricot_shortcut_action", action)
                ctrl.Bind(wx.EVT_KEY_DOWN, lambda evt, target=ctrl: self.on_shortcut_capture_key(evt, target))
                form.Add(ctrl, 1, wx.EXPAND)
                remember(f"shortcut_{action}", ctrl)

        self.settings_scroller.SetSizer(form, True)
        self.settings_scroller.Layout()
        self.settings_scroller.FitInside()
        self.panel.Layout()

    def search_type_code(self) -> str:
        index = self.search_type.GetSelection()
        options = ("All", "Video", "Playlist", "Channel")
        return options[index] if 0 <= index < len(options) else "All"

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
        self.search_results_stack = []
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.set_status(self.t("searching", query=query))
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.search_worker, args=(query, self.current_search_type_code, self.initial_results_limit(), generation), daemon=True).start()

    def effective_results_limit(self) -> int:
        return min(250, max(1, self.settings.results_limit))

    def initial_results_limit(self) -> int:
        return RESULTS_PAGE_SIZE if self.settings.results_limit == 0 else self.effective_results_limit()

    def max_results_limit(self) -> int:
        return 250 if self.settings.results_limit == 0 else self.effective_results_limit()

    def search_worker(self, query: str, search_type: str, limit: int, generation: int) -> None:
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
            wx.CallAfter(self.show_results_if_current, generation, [self.normalize_entry(entry, search_type) for entry in entries])
        except Exception as exc:
            wx.CallAfter(self.show_search_error_if_current, generation, self.friendly_error(exc))

    def show_results_if_current(self, generation: int, results: list[dict]) -> None:
        if generation == self.search_generation:
            self.show_results(results)

    def show_search_error_if_current(self, generation: int, error: str) -> None:
        if generation == self.search_generation:
            self.message(error, wx.ICON_ERROR)

    def normalize_entry(self, entry: dict, search_type: str) -> dict:
        url = entry.get("webpage_url") or entry.get("url") or ""
        ie_key = str(entry.get("ie_key") or "").lower()
        entry_type = str(entry.get("_type") or entry.get("result_type") or "").lower()
        url_text = str(url)
        is_playlist = search_type == "Playlist" or "playlist" in ie_key or "playlist" in entry_type or "list=" in url_text
        is_channel = (
            search_type in {"Channel", "Kanal"}
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
        timestamp = entry.get("timestamp") or entry.get("release_timestamp") or entry.get("modified_timestamp")
        upload_date = entry.get("upload_date")
        age = self.format_age({"timestamp": timestamp, "upload_date": upload_date}) if kind == "video" else ""
        return {
            "title": entry.get("title") or "",
            "channel": entry.get("uploader") or entry.get("channel") or "",
            "channel_url": self.normalize_channel_url(entry),
            "channel_id": entry.get("channel_id") or entry.get("uploader_id") or "",
            "views": self.format_count(entry.get("view_count")),
            "view_count": entry.get("view_count"),
            "age": age or (self.t("uploaded_unknown") if kind == "video" else ""),
            "duration": self.format_duration(entry.get("duration")),
            "duration_seconds": entry.get("duration"),
            "timestamp": timestamp,
            "upload_date": upload_date,
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
        if self.results:
            selected_index = min(max(0, selection), len(self.results) - 1)
            labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
            self.set_listbox_items(self.results_list, labels, selected_index)
            self.safe_set_focus(self.results_list)
            self.set_status(self.t("found", count=len(self.results)))
            self.start_result_metadata_hydration()
        else:
            self.set_listbox_items(self.results_list, [self.t("no_results")], 0)
            self.safe_set_focus(self.results_list)
            self.set_status(self.t("no_results"))

    def maybe_extend_results(self) -> None:
        if not self.dynamic_fetch_enabled or self.settings.results_limit != 0 or not hasattr(self, "results_list"):
            return
        if not self.results and not self.all_results:
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
        generation = self.search_generation
        if self.collection_url:
            threading.Thread(target=self.load_collection_worker, args=(self.collection_url, self.collection_result_type or "Video", next_limit, selection, generation), daemon=True).start()
        else:
            threading.Thread(target=self.search_more_worker, args=(self.last_search_query, self.current_search_type_code, next_limit, selection, generation), daemon=True).start()

    def search_more_worker(self, query: str, search_type: str, limit: int, selection: int, generation: int) -> None:
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
            wx.CallAfter(self.show_more_results_if_current, generation, [self.normalize_entry(entry, search_type) for entry in entries], selection)
        except Exception as exc:
            wx.CallAfter(self.dynamic_fetch_failed_if_current, generation, self.friendly_error(exc))

    def show_more_results(self, results: list[dict], selection: int) -> None:
        self.loading_more_results = False
        if len(results) <= len(self.all_results):
            self.set_status(self.t("no_more_results"))
            return
        self.show_results(results, selection=selection, visible_count=min(len(results), len(self.results) + RESULTS_PAGE_SIZE))
        self.set_status(self.t("search_more_loaded", count=len(self.results)))

    def show_more_results_if_current(self, generation: int, results: list[dict], selection: int) -> None:
        if generation == self.search_generation:
            self.show_more_results(results, selection)

    def dynamic_fetch_failed(self, error: str) -> None:
        self.loading_more_results = False
        self.message(error, wx.ICON_ERROR)

    def dynamic_fetch_failed_if_current(self, generation: int, error: str) -> None:
        if generation == self.search_generation:
            self.dynamic_fetch_failed(error)

    def start_result_metadata_hydration(self) -> None:
        candidates: list[dict] = []
        for item in list(self.results):
            url = str(item.get("url") or "")
            if item.get("kind") != "video" or not url or url in self.metadata_hydration_urls:
                continue
            if item.get("timestamp") or item.get("upload_date"):
                continue
            candidates.append(dict(item))
            self.metadata_hydration_urls.add(url)
            if len(candidates) >= RESULTS_PAGE_SIZE:
                break
        if candidates:
            threading.Thread(target=self.result_metadata_worker, args=(candidates,), daemon=True).start()

    def result_metadata_worker(self, items: list[dict]) -> None:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            return
        options = {"quiet": True, "skip_download": True, "noplaylist": True}
        try:
            with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
                for item in items:
                    url = str(item.get("url") or "")
                    if not url:
                        continue
                    try:
                        info = ydl.extract_info(url, download=False)
                        payload = self.metadata_from_info(info, item)
                        self.ui_queue.put(("result_metadata", payload))
                    except Exception:
                        continue
        except Exception:
            return

    def metadata_from_info(self, info: dict, item: dict) -> dict:
        timestamp = info.get("timestamp") or info.get("release_timestamp") or info.get("modified_timestamp") or item.get("timestamp")
        upload_date = info.get("upload_date") or item.get("upload_date")
        return {
            "url": item.get("url", ""),
            "title": info.get("title") or item.get("title", ""),
            "channel": info.get("uploader") or info.get("channel") or item.get("channel", ""),
            "channel_url": self.normalize_channel_url(info) or item.get("channel_url", ""),
            "channel_id": info.get("channel_id") or info.get("uploader_id") or item.get("channel_id", ""),
            "view_count": info.get("view_count", item.get("view_count")),
            "views": self.format_count(info.get("view_count", item.get("view_count"))),
            "timestamp": timestamp,
            "upload_date": upload_date,
            "age": self.format_age({"timestamp": timestamp, "upload_date": upload_date}) or item.get("age") or self.t("uploaded_unknown"),
            "duration_seconds": info.get("duration", item.get("duration_seconds")),
            "duration": self.format_duration(info.get("duration", item.get("duration_seconds"))),
            "description": info.get("description") or item.get("description", ""),
        }

    def apply_result_metadata(self, payload: dict) -> None:
        url = str(payload.get("url") or "")
        if not url:
            return
        for collection in (self.results, self.all_results, self.return_results, self.return_all_results):
            for item in collection:
                if item.get("url") == url:
                    item.update({key: value for key, value in payload.items() if value not in (None, "")})
        if hasattr(self, "results_list"):
            for index, item in enumerate(self.results):
                if item.get("url") == url:
                    self.refresh_result_line(index)
                    break

    def result_line(self, index: int, item: dict) -> str:
        if item.get("kind") in {"playlist", "channel"}:
            parts = [item["title"], item["type"]]
        else:
            parts = [
                item["title"],
                f"{self.t('channel')}: {item['channel']}",
                f"{self.t('views')}: {item['views']}",
                item.get("age") or self.t("uploaded_unknown"),
                item.get("duration", ""),
                item["type"],
            ]
        queued = self.download_queue.get(item.get("url", ""))
        if queued:
            parts.append(self.queue_mode_label(queued))
        return " | ".join(part for part in parts if part)

    def selected_result(self) -> dict | None:
        if not hasattr(self, "results_list"):
            return None
        try:
            index = self.results_list.GetSelection()
        except RuntimeError:
            return None
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.results):
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
        self.player_return_screen = "search"
        self.player_return_data = {}
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(item["url"], item["title"])

    def push_search_state(self) -> None:
        if not self.search_screen_active or not self.results:
            return
        self.search_results_stack.append(
            {
                "results": list(self.results),
                "all_results": list(self.all_results or self.results),
                "index": max(0, self.current_index),
                "visible_count": self.last_visible_count or len(self.results),
                "query": self.last_search_query,
                "type_index": self.last_search_type_index,
                "search_type": self.current_search_type_code,
                "collection_url": self.collection_url,
                "collection_result_type": self.collection_result_type,
            }
        )

    def restore_previous_search_results(self) -> None:
        if not self.search_results_stack:
            self.show_main_menu()
            return
        state = self.search_results_stack.pop()
        if state.get("screen") == "subscriptions":
            self.show_subscriptions()
            return
        if state.get("screen") == "history":
            self.show_history()
            return
        if state.get("screen") == "favorites":
            self.show_favorites()
            return
        self.last_search_query = str(state.get("query") or self.last_search_query)
        self.last_search_type_index = int(state.get("type_index") or 0)
        self.current_search_type_code = str(state.get("search_type") or "All")
        self.collection_url = str(state.get("collection_url") or "")
        self.collection_result_type = str(state.get("collection_result_type") or "")
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        results = list(state.get("all_results") or state.get("results") or [])
        selection = int(state.get("index") or 0)
        visible_count = int(state.get("visible_count") or len(results))
        self.show_search(restore_search=True)
        self.show_results(results, selection=selection, visible_count=visible_count)
        wx.CallAfter(self.focus_results_list, selection)

    def open_channel_videos(self, item: dict, push_state: bool = True) -> None:
        if push_state:
            self.push_search_state()
        self.set_status(self.t("loading_channel", title=item["title"]))
        url = item["url"].rstrip("/")
        if not url.endswith("/videos"):
            url = f"{url}/videos"
        self.collection_url = url
        self.collection_result_type = "Video"
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.load_collection_worker, args=(url, "Video", self.initial_results_limit(), 0, generation), daemon=True).start()

    def open_playlist_videos(self, item: dict, push_state: bool = True) -> None:
        if push_state:
            self.push_search_state()
        self.set_status(self.t("loading_playlist", title=item["title"]))
        self.collection_url = item["url"]
        self.collection_result_type = "Video"
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.load_collection_worker, args=(item["url"], "Video", self.initial_results_limit(), 0, generation), daemon=True).start()

    def load_collection_worker(self, url: str, result_type: str, limit: int | None = None, selection: int = 0, generation: int | None = None) -> None:
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            generation = self.search_generation if generation is None else generation
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
                wx.CallAfter(self.show_more_results_if_current, generation, normalized, selection)
            else:
                wx.CallAfter(self.show_results_if_current, generation, normalized)
                wx.CallAfter(self.clear_loading_more_if_current, generation)
        except Exception as exc:
            wx.CallAfter(self.dynamic_fetch_failed_if_current, generation or self.search_generation, self.friendly_error(exc))

    def clear_loading_more_if_current(self, generation: int) -> None:
        if generation == self.search_generation:
            self.loading_more_results = False

    def play_url(self, url: str, title: str = "") -> None:
        player = self.resolve_player()
        if not player:
            self.message(self.t("player_missing"), wx.ICON_ERROR)
            return
        if self.current_video_item:
            self.record_history(self.current_video_item, "played")
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
                "channel_url": self.normalize_channel_url(info) or self.current_video_info.get("channel_url", ""),
                "channel_id": info.get("channel_id") or info.get("uploader_id") or self.current_video_info.get("channel_id", ""),
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

    def playback_key(self, item: dict | None = None) -> str:
        item = item or self.current_video_item or self.current_video_info
        return str((item or {}).get("url") or (item or {}).get("webpage_url") or "").strip()

    def playback_resume_position(self) -> float:
        key = self.playback_key()
        if not key or not getattr(self.settings, "resume_playback", True):
            return 0.0
        try:
            position = float(self.playback_positions.get(key, 0.0) or 0.0)
        except (TypeError, ValueError):
            return 0.0
        return position if position >= 5.0 else 0.0

    def cache_folder_path(self) -> Path:
        return Path(str(getattr(self.settings, "cache_folder", "") or DEFAULT_CACHE_DIR)).expanduser()

    def audio_output_device_options(self, force_refresh: bool = False) -> tuple[list[str], list[str]]:
        now = time.monotonic()
        if not force_refresh and self.audio_device_options_cache and now - self.audio_device_options_cache[0] < 20:
            return list(self.audio_device_options_cache[1]), list(self.audio_device_options_cache[2])
        values = ["auto"]
        labels = ["auto"]
        player = self.resolve_player()
        if player:
            command, kind = player
            if kind == "mpv":
                try:
                    creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
                    result = subprocess.run(
                        [command, "--audio-device=help", "--idle=yes", "--frames=0"],
                        stdout=subprocess.PIPE,
                        stderr=subprocess.STDOUT,
                        text=True,
                        encoding="utf-8",
                        errors="replace",
                        timeout=5,
                        creationflags=creationflags,
                    )
                    for line in result.stdout.splitlines():
                        match = re.match(r"\s*'([^']+)'\s+\((.*)\)\s*$", line)
                        if not match:
                            continue
                        value, label = match.group(1).strip(), match.group(2).strip()
                        if value and value not in values:
                            values.append(value)
                            labels.append(f"{label} ({value})" if label and label != value else value)
                except Exception:
                    pass
        current = self.normalized_audio_output_device()
        if current and current not in values:
            values.append(current)
            labels.append(f"{current} ({self.t('no_output_devices')})")
        self.audio_device_options_cache = (now, list(values), list(labels))
        return values, labels

    def check_saved_audio_device_available(self) -> None:
        device = self.normalized_audio_output_device()
        if not device or device.lower() == "auto":
            return
        values, labels = self.audio_output_device_options(force_refresh=True)
        if device in values and labels[values.index(device)] != f"{device} ({self.t('no_output_devices')})":
            return
        self.prompt_for_new_default_audio_device(values, labels)

    def prompt_for_new_default_audio_device(self, values: list[str], labels: list[str]) -> None:
        if not values:
            values, labels = ["auto"], ["auto"]
        self.message(self.t("audio_device_missing"), wx.ICON_WARNING)
        with wx.SingleChoiceDialog(self, self.t("audio_device_missing"), self.t("default_audio_device"), labels) as dialog:
            dialog.SetSelection(0)
            if dialog.ShowModal() != wx.ID_OK:
                self.settings.audio_output_device = "auto"
                self.save_settings()
                return
            index = dialog.GetSelection()
        self.settings.audio_output_device = values[index] if 0 <= index < len(values) else "auto"
        self.save_settings()
        self.announce_player(self.t("settings_saved"))

    def player_audio_output_device(self) -> str:
        return (self.session_audio_output_device or self.normalized_audio_output_device() or "auto").strip() or "auto"

    def speed_audio_filter_args(self) -> list[str]:
        mode = self.normalized_speed_audio_mode()
        if mode == SPEED_AUDIO_MODE_MPV:
            return ["--audio-pitch-correction=yes"]
        if mode == SPEED_AUDIO_MODE_SCALETEMPO:
            return ["--audio-pitch-correction=no", "--af=@apricot_speed:scaletempo=stride=30:overlap=.50:search=10"]
        if mode == SPEED_AUDIO_MODE_RUBBERBAND:
            return [
                "--audio-pitch-correction=yes",
                "--af=@apricot_speed:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer",
            ]
        return ["--audio-pitch-correction=no", "--af=@apricot_speed:scaletempo2=search-interval=50:window-size=20:max-speed=8.0"]

    def speed_uses_mpv_auto_pitch_correction(self) -> bool:
        return self.normalized_speed_audio_mode() in {SPEED_AUDIO_MODE_MPV, SPEED_AUDIO_MODE_RUBBERBAND}

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
                "--keep-open=yes",
                "--volume-max=300",
                "--pitch=1.0",
                f"--speed={self.settings.player_speed}",
                f"--loop-file={'inf' if self.repeat_current else 'no'}",
                "--term-playing-msg=",
                "--msg-level=all=warn",
            ]
            args.extend(self.speed_audio_filter_args())
            if getattr(self.settings, "enable_stream_cache", True):
                cache_folder = self.cache_folder_path()
                cache_folder.mkdir(parents=True, exist_ok=True)
                cache_size = max(128, min(4096, int(getattr(self.settings, "cache_size_mb", 512) or 512)))
                back_cache = max(32, min(cache_size, cache_size // 4))
                args.extend(
                    [
                        "--cache=yes",
                        "--cache-on-disk=yes",
                        f"--demuxer-cache-dir={cache_folder}",
                        f"--demuxer-max-bytes={cache_size}MiB",
                        f"--demuxer-max-back-bytes={back_cache}MiB",
                        "--cache-pause=no",
                    ]
                )
            else:
                args.append("--cache=no")
            audio_device = self.player_audio_output_device()
            if audio_device and audio_device.lower() != "auto":
                args.append(f"--audio-device={audio_device}")
            resume_position = self.playback_resume_position()
            if resume_position:
                args.append(f"--start={resume_position:.1f}")
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
            self.player_ended = False
            self.player_generation += 1
            self.current_stream_url = stream_url
            self.current_audio_device = audio_device
            self.volume_boost_enabled = False
            self.rubberband_pitch_filter_active = False
            self.current_video_info["speed"] = self.format_playback_rate(float(self.settings.player_speed))
            self.current_video_info["pitch"] = self.format_playback_rate(1.0)
            self.update_details_text()
            self.set_status(self.t("playing", title=title))
            self.start_player_monitor(self.player_generation)
        except Exception as exc:
            self.message(self.t("player_failed", error=exc), wx.ICON_ERROR)

    def show_player_page(self, title: str) -> None:
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back_results"), self.back_to_results),
                (self.t("previous"), lambda: self.play_relative_item(-1)),
                (self.t("play"), self.player_play_pause),
                (self.t("next"), lambda: self.play_relative_item(1)),
                (self.t("output_devices"), self.show_output_devices),
                (self.t("copy_link"), self.copy_active_url),
                (self.t("copy_stream_url"), self.copy_direct_stream_url),
                (self.t("show_video_details"), self.show_video_details),
            ]
        )
        label = wx.StaticText(self.panel, label=f"{self.t('internal_player')}: {title}")
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.repeat_checkbox = wx.CheckBox(self.panel, label=self.t("repeat"))
        self.repeat_checkbox.SetName(self.t("repeat"))
        self.repeat_checkbox.SetValue(self.repeat_current)
        self.repeat_checkbox.Bind(wx.EVT_CHECKBOX, self.on_repeat_changed)
        self.root_sizer.Add(self.repeat_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.player_panel = wx.Panel(self.panel, style=wx.BORDER_SIMPLE)
        self.player_panel.SetName(self.t("internal_player"))
        self.player_panel.SetBackgroundColour(wx.BLACK)
        self.player_panel.Bind(wx.EVT_KEY_DOWN, self.on_player_key)
        self.player_panel.Bind(wx.EVT_CHAR_HOOK, self.on_char_hook)
        self.root_sizer.Add(self.player_panel, 1, wx.EXPAND | wx.ALL, 4)
        self.details_label = None
        self.video_details = None
        self.details_button_sizer = None
        self.details_opened_temporarily = False
        self.in_player_screen = True
        self.player_control_mode = True
        self.panel.Layout()
        if self.settings.show_video_details_by_default:
            wx.CallAfter(self.show_video_details, False)
        else:
            self.player_panel.SetFocus()

    def on_player_key(self, event: wx.KeyEvent) -> None:
        self.on_char_hook(event)

    def on_repeat_changed(self, _event=None) -> None:
        checked = bool(getattr(self, "repeat_checkbox", None) and self.repeat_checkbox.GetValue())
        self.repeat_current = checked
        if self.player_kind == "mpv" and self.mpv_process_alive():
            try:
                self.mpv_set_property("loop-file", "inf" if checked else "no", timeout=0.8)
            except Exception:
                pass
        self.announce_player(self.t("repeat_on" if checked else "repeat_off"))

    def back_to_results(self) -> None:
        self.stop_player(silent=True)
        self.in_player_screen = False
        if self.player_return_screen == "rss_items":
            feed_index = int(self.player_return_data.get("feed_index", self.current_rss_feed_index) or 0)
            item_index = int(self.player_return_data.get("item_index", 0) or 0)
            self.player_return_screen = ""
            self.player_return_data = {}
            self.show_rss_items(feed_index, selection=item_index)
            return
        if self.player_return_screen == "history":
            self.player_return_screen = ""
            self.player_return_data = {}
            self.show_history()
            return
        if self.player_return_screen == "user_playlist_items":
            playlist_index = int(self.player_return_data.get("playlist_index", self.current_user_playlist_index) or 0)
            item_index = int(self.player_return_data.get("item_index", 0) or 0)
            self.player_return_screen = ""
            self.player_return_data = {}
            self.show_user_playlist_items(playlist_index, selection=item_index)
            return
        if self.player_return_screen == "notification_center":
            self.player_return_screen = ""
            self.player_return_data = {}
            self.show_notification_center()
            return
        if self.player_return_screen == "direct_link":
            self.player_return_screen = ""
            self.player_return_data = {}
            self.show_direct_link()
            return
        if self.player_return_screen == "favorites":
            self.player_return_screen = ""
            self.player_return_data = {}
            self.show_favorites()
            return
        if self.player_return_screen == "subscriptions":
            self.player_return_screen = ""
            self.player_return_data = {}
            self.show_subscriptions()
            return
        results = self.return_all_results or self.all_results or self.return_results or self.results
        if results:
            self.metadata_hydration_urls.clear()
            self.search_generation += 1
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

    def show_video_details(self, temporary: bool | None = None) -> None:
        if not self.in_player_screen:
            return
        self.details_opened_temporarily = (not self.settings.show_video_details_by_default) if temporary is None else bool(temporary)
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
            self.details_button_sizer = wx.BoxSizer(wx.HORIZONTAL)
            copy_button = wx.Button(self.panel, label=self.t("copy_details"))
            copy_button.SetName(self.t("copy_details"))
            copy_button.Bind(wx.EVT_BUTTON, lambda _evt: self.copy_video_details())
            back_button = wx.Button(self.panel, label=self.t("back"))
            back_button.SetName(self.t("back"))
            back_button.Bind(wx.EVT_BUTTON, lambda _evt: self.hide_video_details())
            self.details_button_sizer.Add(copy_button, 0, wx.RIGHT, 6)
            self.details_button_sizer.Add(back_button, 0)
            self.root_sizer.Add(self.details_button_sizer, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.update_details_text()
        if self.details_label:
            self.details_label.Show()
        if self.video_details:
            self.video_details.Show()
        if self.details_button_sizer:
            self.show_sizer_items(self.details_button_sizer, True)
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
        if self.details_button_sizer:
            self.show_sizer_items(self.details_button_sizer, False)
        self.panel.Layout()
        if hasattr(self, "player_panel"):
            self.safe_set_focus(self.player_panel)
        self.announce_player(self.t("details_closed"))

    def copy_video_details(self) -> None:
        details = self.build_video_details_text()
        self.copy_plain_text_to_clipboard(details)
        self.announce_player(self.t("details_copied"))

    @staticmethod
    def show_sizer_items(sizer: wx.Sizer, show: bool) -> None:
        for child in sizer.GetChildren():
            window = child.GetWindow()
            if window:
                window.Show(show)

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
        if self.favorites_screen_active:
            return self.selected_favorite()
        if self.history_screen_active:
            return self.selected_history_item()
        if self.subscriptions_screen_active:
            return self.selected_subscription()
        if self.rss_feeds_screen_active:
            return self.selected_rss_feed()
        if self.rss_items_screen_active:
            return self.selected_rss_item()
        if self.podcast_search_screen_active:
            return self.selected_podcast_result()
        if self.user_playlists_screen_active:
            return self.selected_user_playlist()
        if self.user_playlist_items_screen_active:
            return self.selected_user_playlist_item()
        if self.notification_center_screen_active:
            return self.selected_notification_item()
        if self.direct_link_screen_active:
            return self.direct_link_item()
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

    def show_output_devices(self) -> None:
        if not self.in_player_screen or self.player_kind != "mpv":
            return
        try:
            devices = self.mpv_get_property("audio-device-list", timeout=1.5) or []
        except Exception:
            devices = []
        choices: list[str] = []
        values: list[str] = []
        for device in devices:
            if not isinstance(device, dict):
                continue
            name = str(device.get("name") or "").strip()
            if not name:
                continue
            description = str(device.get("description") or name).strip()
            choices.append(f"{description} ({name})" if description != name else name)
            values.append(name)
        if not choices:
            self.announce_player(self.t("no_output_devices"))
            return
        with wx.SingleChoiceDialog(self, self.t("select_output_device"), self.t("output_devices"), choices) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            index = dialog.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(values):
            return
        value = values[index]
        try:
            self.mpv_set_property("audio-device", value)
            self.session_audio_output_device = value
            self.current_audio_device = value
            self.announce_player(self.t("output_device_set", device=choices[index]))
        except Exception as exc:
            self.announce_player(self.t("stream_url_failed", error=self.friendly_error(exc)))

    def play_relative_item(self, delta: int) -> None:
        if delta < 0:
            item = self.relative_player_item(-1)
            if not item:
                self.announce_player(self.t("no_previous_item"))
                return
        else:
            item = self.relative_player_item(1)
            if not item:
                self.announce_player(self.t("no_next_item"))
                return
        self.open_relative_player_item(item)

    def relative_player_item(self, delta: int) -> dict | None:
        screen = self.player_return_screen
        data = dict(self.player_return_data or {})
        if screen == "rss_items":
            feed_index = int(data.get("feed_index", self.current_rss_feed_index) or 0)
            item_index = int(data.get("item_index", 0) or 0) + delta
            if 0 <= feed_index < len(self.rss_feeds):
                items = list(self.rss_feeds[feed_index].get("items") or [])
                if 0 <= item_index < len(items):
                    return dict(items[item_index], rss_feed_index=feed_index, rss_item_index=item_index)
        if screen == "user_playlist_items":
            playlist_index = int(data.get("playlist_index", self.current_user_playlist_index) or 0)
            item_index = int(data.get("item_index", 0) or 0) + delta
            if 0 <= playlist_index < len(self.user_playlists):
                items = list(self.user_playlists[playlist_index].get("items") or [])
                if 0 <= item_index < len(items):
                    return dict(items[item_index], user_playlist_index=playlist_index, user_playlist_item_index=item_index)
        results = self.return_all_results or self.all_results or self.return_results or self.results
        item_index = int(data.get("index", self.return_index) or self.return_index) + delta
        playable = [item for item in results if item.get("kind") not in {"channel", "playlist"}]
        if not playable:
            return None
        current_url = str((self.current_video_item or {}).get("url") or "")
        current_pos = next((i for i, item in enumerate(playable) if item.get("url") == current_url), -1)
        if current_pos >= 0:
            item_index = current_pos + delta
        if 0 <= item_index < len(playable):
            return dict(playable[item_index])
        return None

    def open_relative_player_item(self, item: dict) -> None:
        if not item.get("url"):
            return
        self.stop_player(silent=True)
        if item.get("kind") == "rss_item":
            self.player_return_screen = "rss_items"
            self.player_return_data = {
                "feed_index": int(item.get("rss_feed_index", self.current_rss_feed_index) or 0),
                "item_index": int(item.get("rss_item_index", 0) or 0),
            }
        elif "user_playlist_index" in item:
            self.player_return_screen = "user_playlist_items"
            self.player_return_data = {
                "playlist_index": int(item.get("user_playlist_index", self.current_user_playlist_index) or 0),
                "item_index": int(item.get("user_playlist_item_index", 0) or 0),
            }
        else:
            results = self.return_all_results or self.all_results or self.return_results or self.results
            self.return_index = next((i for i, result in enumerate(results) if result.get("url") == item.get("url")), self.return_index)
            self.player_return_screen = "search"
            self.player_return_data = {"index": self.return_index}
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(str(item.get("url") or ""), str(item.get("title") or ""))

    def open_library_item(self, item: dict, screen: str) -> None:
        kind = item.get("kind")
        if kind in {"channel", "playlist"}:
            self.search_results_stack.append({"screen": screen})
            self.last_search_query = item.get("title", "")
            self.last_search_type_index = 0
            self.current_search_type_code = "Video"
            self.show_search(restore_search=True)
            if kind == "channel":
                self.open_channel_videos(item, push_state=False)
            else:
                self.open_playlist_videos(item, push_state=False)
            return
        url = str(item.get("url") or "")
        if not url:
            self.announce_player(self.t("no_selection"))
            return
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.player_return_screen = screen
        self.player_return_data = {}
        self.play_url(url, item.get("title", ""))

    def next_download_task_id(self, prefix: str = "download") -> str:
        self.download_task_counter += 1
        return f"{prefix}-{self.download_task_counter}-{int(time.time() * 1000)}"

    def register_download_task(self, item: dict, audio_only: bool, task_kind: str = "single", total: int = 0) -> tuple[str, threading.Event]:
        task_id = self.next_download_task_id(task_kind)
        cancel_event = threading.Event()
        title = item.get("title") or self.t("download_video_mode")
        self.download_cancel_events[task_id] = cancel_event
        self.active_downloads[task_id] = {
            "task_id": task_id,
            "task_kind": task_kind,
            "title": title,
            "current_title": title,
            "url": item.get("url", ""),
            "kind": item.get("kind", "video"),
            "type": item.get("type", ""),
            "channel": item.get("channel", ""),
            "audio_only": audio_only,
            "status_key": "download_state_downloading",
            "total": total,
            "completed": 0,
            "remaining": total,
            "percent": "",
            "created_at": time.monotonic(),
        }
        self.refresh_download_views()
        return task_id, cancel_event

    def update_download_task(self, task_id: str, **fields) -> None:
        task = self.active_downloads.get(task_id)
        if not task:
            return
        task.update(fields)
        self.refresh_download_views(update_menu=False)

    def finish_download_task(self, task_id: str, status_key: str = "download_state_done") -> None:
        task = self.active_downloads.get(task_id)
        if task:
            task["status_key"] = status_key
        self.download_cancel_events.pop(task_id, None)
        self.active_downloads.pop(task_id, None)
        self.refresh_download_views()

    def refresh_download_views(self, update_menu: bool = True) -> None:
        if self.in_queue_screen:
            wx.CallAfter(self.refresh_queue_view)
        if update_menu and hasattr(self, "menu_list"):
            wx.CallAfter(self.refresh_main_menu_download_label)

    def refresh_main_menu_download_label(self) -> None:
        if not getattr(self, "in_main_menu", False) or not hasattr(self, "menu_list"):
            return
        try:
            self.show_main_menu()
        except RuntimeError:
            pass

    def start_player_monitor(self, generation: int) -> None:
        threading.Thread(target=self.player_monitor_worker, args=(generation,), daemon=True).start()

    def player_monitor_worker(self, generation: int) -> None:
        while generation == self.player_generation and self.mpv_process_alive():
            time.sleep(0.5)
            if generation != self.player_generation or not self.mpv_process_alive():
                return
            try:
                eof_reached = bool(self.mpv_get_property("eof-reached", timeout=0.25))
            except Exception:
                eof_reached = False
            if eof_reached:
                wx.CallAfter(self.handle_player_eof, generation)
                return

    def handle_player_eof(self, generation: int) -> None:
        if generation != self.player_generation or not self.in_player_screen:
            return
        if self.mpv_process_alive():
            try:
                if not bool(self.mpv_get_property("eof-reached", timeout=0.15)):
                    return
            except Exception:
                pass
        if self.repeat_current:
            self.player_ended = False
            return
        if self.settings.autoplay_next:
            next_item = self.relative_player_item(1)
            if next_item:
                self.open_relative_player_item(next_item)
                return
        self.player_ended = True
        self.announce_player(self.t("playback_finished"))

    def player_play_pause(self) -> None:
        if self.player_kind != "mpv":
            return
        if not self.mpv_process_alive():
            self.restart_current_playback()
            return
        try:
            eof_reached = bool(self.mpv_get_property("eof-reached", timeout=0.25))
        except Exception:
            eof_reached = False
        if self.player_ended or eof_reached:
            self.restart_current_playback()
            return
        self.player_command("cycle pause")

    def restart_current_playback(self) -> None:
        self.player_ended = False
        if self.mpv_process_alive():
            try:
                self.mpv_send(["seek", 0, "absolute"], timeout=0.8)
                self.mpv_set_property("pause", False, timeout=0.8)
                self.start_player_monitor(self.player_generation)
                self.announce_player(self.t("playback_restarted"))
                return
            except Exception:
                pass
        item = dict(self.current_video_item or self.current_video_info or {})
        url = str(item.get("url") or "")
        if not url:
            return
        key = self.playback_key(item)
        if key and key in self.playback_positions:
            self.playback_positions.pop(key, None)
            self.save_playback_positions()
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(url, str(item.get("title") or ""))

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
            self.mpv_set_property("audio-pitch-correction", self.speed_uses_mpv_auto_pitch_correction())
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
        mode = str(getattr(self.settings, "pitch_mode", PITCH_MODE_MPV) or PITCH_MODE_MPV)
        return self.normalize_pitch_mode_value(mode)

    def normalized_speed_audio_mode(self) -> str:
        mode = str(getattr(self.settings, "speed_audio_mode", SPEED_AUDIO_MODE_RUBBERBAND) or SPEED_AUDIO_MODE_RUBBERBAND)
        return self.normalize_speed_audio_mode_value(mode)

    def normalized_direct_link_enter_action(self) -> str:
        action = str(getattr(self.settings, "direct_link_enter_action", DIRECT_LINK_ENTER_PLAY) or DIRECT_LINK_ENTER_PLAY)
        return self.normalize_direct_link_enter_action(action)

    def normalized_audio_output_device(self) -> str:
        device = str(getattr(self.settings, "audio_output_device", "auto") or "auto").strip()
        return device or "auto"

    def normalized_video_format(self) -> str:
        return self.normalize_video_format_value(getattr(self.settings, "video_format", VIDEO_FORMAT_MP4))

    def normalized_podcast_search_provider(self) -> str:
        provider = str(getattr(self.settings, "podcast_search_provider", PODCAST_DIRECTORY_PROVIDER_APPLE) or PODCAST_DIRECTORY_PROVIDER_APPLE)
        return provider if provider in PODCAST_DIRECTORY_PROVIDER_OPTIONS else PODCAST_DIRECTORY_PROVIDER_APPLE

    def normalized_podcast_search_country(self) -> str:
        country = str(getattr(self.settings, "podcast_search_country", "US") or "US").upper()
        return country if country in PODCAST_COUNTRY_OPTIONS else "US"

    def pitch_mode_labels(self) -> list[str]:
        return [
            self.t("pitch_mode_rubberband"),
            self.t("pitch_mode_mpv"),
            self.t("pitch_mode_linked_speed"),
        ]

    def speed_audio_mode_labels(self) -> list[str]:
        return [
            self.t("speed_audio_mode_scaletempo2"),
            self.t("speed_audio_mode_mpv"),
            self.t("speed_audio_mode_scaletempo"),
            self.t("speed_audio_mode_rubberband"),
        ]

    def direct_link_enter_action_labels(self) -> list[str]:
        return [
            self.t("direct_link_enter_play"),
            self.t("direct_link_enter_audio"),
            self.t("direct_link_enter_video"),
            self.t("direct_link_enter_stream"),
        ]

    def video_format_labels(self) -> list[str]:
        return [
            self.t("video_format_mp4_recommended"),
            self.t("video_format_best_available"),
            self.t("video_format_mp4_single"),
            self.t("video_format_smallest"),
        ]

    def result_limit_labels(self, options: list[str]) -> list[str]:
        return [self.t("dynamic_results") if option == "0" else option for option in options]

    @staticmethod
    def normalize_pitch_mode_value(mode: str) -> str:
        normalized = str(mode or "").strip()
        lowered = normalized.lower()
        if normalized in PITCH_MODE_OPTIONS:
            return normalized
        if lowered in {LEGACY_PITCH_MODE_MPV, LEGACY_PITCH_MODE_MPV_LABEL}:
            return PITCH_MODE_MPV
        if lowered == LEGACY_PITCH_MODE_LINKED_SPEED:
            return PITCH_MODE_LINKED_SPEED
        if lowered in {LEGACY_PITCH_MODE_RUBBERBAND, LEGACY_PITCH_MODE_RUBBERBAND_LABEL}:
            return PITCH_MODE_RUBBERBAND
        return PITCH_MODE_MPV

    @staticmethod
    def normalize_speed_audio_mode_value(mode: str) -> str:
        normalized = str(mode or "").strip()
        if normalized in SPEED_AUDIO_MODE_OPTIONS:
            return normalized
        lowered = normalized.lower()
        if "rubber" in lowered:
            return SPEED_AUDIO_MODE_RUBBERBAND
        if "classic" in lowered or lowered == "scaletempo":
            return SPEED_AUDIO_MODE_SCALETEMPO
        if "mpv" in lowered or "default" in lowered:
            return SPEED_AUDIO_MODE_MPV
        return SPEED_AUDIO_MODE_RUBBERBAND

    @staticmethod
    def normalize_direct_link_enter_action(action: str) -> str:
        normalized = str(action or "").strip()
        return normalized if normalized in DIRECT_LINK_ENTER_OPTIONS else DIRECT_LINK_ENTER_PLAY

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
        self.save_current_playback_position()
        self.player_generation += 1
        self.player_ended = False
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
        self.current_stream_url = ""
        self.current_audio_device = ""
        if not self.in_player_screen:
            self.in_player_screen = False
        if not silent:
            self.set_status(self.t("stopped"))

    def save_current_playback_position(self) -> None:
        if not getattr(self.settings, "resume_playback", True) or not self.mpv_process_alive():
            return
        key = self.playback_key()
        if not key:
            return
        try:
            elapsed = self.mpv_get_property("time-pos", timeout=0.35)
            duration = self.mpv_get_property("duration", timeout=0.35)
            if elapsed is None:
                return
            position = float(elapsed)
            total = float(duration or 0.0)
            if position < 5.0:
                self.playback_positions.pop(key, None)
            elif total and position > max(5.0, total - 8.0):
                self.playback_positions.pop(key, None)
            else:
                self.playback_positions[key] = round(position, 1)
            self.save_playback_positions()
        except Exception:
            pass

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

    @staticmethod
    def details_text_navigation_key(event: wx.KeyEvent) -> bool:
        key = event.GetKeyCode()
        navigation = {
            wx.WXK_UP,
            wx.WXK_DOWN,
            wx.WXK_LEFT,
            wx.WXK_RIGHT,
            wx.WXK_HOME,
            wx.WXK_END,
            wx.WXK_PAGEUP,
            wx.WXK_PAGEDOWN,
            wx.WXK_TAB,
        }
        if key in navigation:
            return True
        if event.ControlDown() and MainFrame.key_event_matches_letter(event, "c"):
            return True
        if event.ControlDown() and MainFrame.key_event_matches_letter(event, "a"):
            return True
        return False

    def on_char_hook(self, event: wx.KeyEvent) -> None:
        focus = wx.Window.FindFocus()
        details_has_focus = focus is self.video_details
        if self.is_shortcut_capture_control(focus):
            self.on_shortcut_capture_key(event, focus)
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "menu_list", None):
            self.activate_menu()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "rss_feed_list", None):
            self.open_selected_rss_feed()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "podcast_result_list", None):
            self.add_selected_podcast_result()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "rss_items_list", None):
            self.play_selected_rss_item()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "user_playlist_list", None):
            self.open_selected_user_playlist()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "user_playlist_items_list", None):
            self.play_selected_user_playlist_item()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "notification_list", None):
            self.open_selected_notification()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "history_list", None):
            self.play_history_item()
            return
        if focus is getattr(self, "queue_list", None) and self.shortcut_matches(event, "download_audio"):
            self.download_selected_queue_item(True)
            return
        if focus is getattr(self, "queue_list", None) and self.shortcut_matches(event, "download_video"):
            self.download_selected_queue_item(False)
            return
        if focus is getattr(self, "queue_list", None) and self.shortcut_matches(event, "open_selected"):
            self.download_selected_queue_item()
            return
        if focus is getattr(self, "results_list", None) and self.shortcut_matches(event, "queue_audio"):
            self.toggle_download_queue(True)
            return
        if focus is getattr(self, "results_list", None) and self.shortcut_matches(event, "queue_video"):
            self.toggle_download_queue(False)
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "results_list", None):
            self.play_selected()
            return
        if self.shortcut_matches(event, "download_audio"):
            self.download_audio_shortcut()
            return
        if self.shortcut_matches(event, "download_video"):
            self.download_video_shortcut()
            return
        if self.shortcut_matches(event, "subscribe_channel"):
            self.subscribe_shortcut()
            return
        if self.shortcut_matches(event, "create_playlist"):
            self.create_user_playlist_dialog()
            return
        if self.shortcut_matches(event, "add_to_playlist"):
            self.add_active_to_playlist()
            return
        if self.shortcut_matches(event, "remove_from_playlist"):
            self.remove_selected_user_playlist_item()
            return
        if self.shortcut_matches(event, "player_back"):
            if self.in_player_screen and self.video_details_visible():
                if self.details_opened_temporarily:
                    self.hide_video_details()
                    return
                self.back_to_results()
                return
            if self.in_player_screen:
                self.back_to_results()
                return
            if self.search_screen_active and self.search_results_stack:
                self.restore_previous_search_results()
                return
            if self.rss_items_screen_active:
                self.show_rss_feeds()
                return
            if self.user_playlist_items_screen_active:
                self.show_user_playlists()
                return
            if self.podcast_search_screen_active:
                self.show_rss_feeds()
                return
            if self.rss_feeds_screen_active:
                self.show_main_menu()
                return
            if self.user_playlists_screen_active or self.notification_center_screen_active or self.direct_link_screen_active:
                self.show_main_menu()
                return
            self.show_main_menu()
            return
        if self.in_player_screen and self.shortcut_matches(event, "player_copy_link"):
            self.copy_active_url()
            return
        if self.shortcut_matches(event, "copy_stream_url"):
            self.copy_direct_stream_url()
            return
        if self.player_control_mode:
            if focus is getattr(self, "repeat_checkbox", None) and self.shortcut_matches(event, "player_play_pause"):
                event.Skip()
                return
            if details_has_focus and self.details_text_navigation_key(event):
                event.Skip()
                return
            if self.shortcut_matches(event, "player_output_devices"):
                self.show_output_devices()
                return
            if self.shortcut_matches(event, "player_previous"):
                self.play_relative_item(-1)
                return
            if self.shortcut_matches(event, "player_next"):
                self.play_relative_item(1)
                return
            if self.shortcut_matches(event, "player_volume_boost"):
                self.toggle_volume_boost()
                return
            if self.shortcut_matches(event, "player_play_pause"):
                self.player_play_pause()
                return
            if self.shortcut_matches(event, "player_time"):
                self.announce_time_async()
                return
            if self.shortcut_matches(event, "player_speed_down"):
                self.change_speed_async(-self.speed_step_value())
                return
            if self.shortcut_matches(event, "player_speed_up"):
                self.change_speed_async(self.speed_step_value())
                return
            if self.shortcut_matches(event, "player_pitch_up"):
                self.change_pitch_async(self.pitch_step_value())
                return
            if self.shortcut_matches(event, "player_pitch_down"):
                self.change_pitch_async(-self.pitch_step_value())
                return
            if self.shortcut_matches(event, "player_details"):
                self.show_video_details()
                return
            if self.shortcut_matches(event, "player_seek_back_huge"):
                self.player_command("seek -600")
                return
            if self.shortcut_matches(event, "player_seek_forward_huge"):
                self.player_command("seek 600")
                return
            if self.shortcut_matches(event, "player_seek_back_large"):
                self.player_command("seek -60")
                return
            if self.shortcut_matches(event, "player_seek_forward_large"):
                self.player_command("seek 60")
                return
            if self.shortcut_matches(event, "player_seek_back"):
                self.player_command("seek -5")
                return
            if self.shortcut_matches(event, "player_seek_forward"):
                self.player_command("seek 5")
                return
            if self.shortcut_matches(event, "player_volume_up"):
                self.change_volume_async(self.settings.volume_step)
                return
            if self.shortcut_matches(event, "player_volume_down"):
                self.change_volume_async(-self.settings.volume_step)
                return
        event.Skip()

    def open_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        item = self.selected_result()
        if item and item.get("kind") in {"playlist", "channel"}:
            is_channel = item.get("kind") == "channel"
            actions = [
                (self.t("open_channel_videos" if is_channel else "open_playlist_videos"), self.play_selected),
                (self.t("download_channel" if is_channel else "download_playlist"), lambda selected=dict(item): self.download_collection(selected)),
                (self.t("add_favorite"), self.add_selected_favorite),
                (self.t("open_browser"), self.open_selected_in_browser),
                (self.t("copy_url"), self.copy_selected_url),
            ]
            if is_channel:
                actions.insert(3, (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), self.subscribe_shortcut))
        else:
            actions = [
                (self.t("play"), self.play_selected),
                (self.menu_label_with_shortcut("download_audio", "download_audio"), self.download_audio),
                (self.menu_label_with_shortcut("download_video", "download_video"), self.download_video),
                (self.t("add_favorite"), self.add_selected_favorite),
                (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), self.subscribe_shortcut),
                (self.t("open_browser"), self.open_selected_in_browser),
                (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda selected=dict(item or {}): self.copy_direct_stream_url(selected)),
                (self.t("copy_url"), self.copy_selected_url),
            ]
        if len(self.download_queue) > 1:
            actions.insert(1, (self.t("download_all_selected"), self.download_all_queued))
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        selected = self.selected_result()
        if selected and selected.get("kind") not in {"playlist", "channel"}:
            self.append_add_to_playlist_menu(menu)
        self.PopupMenu(menu)
        menu.Destroy()

    def download_root_folder(self) -> Path:
        folder = Path(str(self.settings.download_folder or DEFAULT_DOWNLOAD_ROOT)).expanduser()
        return folder

    def music_download_folder(self) -> Path:
        root = self.download_root_folder()
        if root.name.lower() == "music":
            return root
        return root / "music"

    def podcasts_download_folder(self) -> Path:
        root = self.download_root_folder()
        root_name = root.name.lower()
        if root_name == "podcasts":
            return root
        if root_name == "music":
            return root.parent / "podcasts"
        return root / "podcasts"

    def download_folder_for_item(self, item: dict, audio_only: bool = True, collection: bool = False) -> Path:
        kind = str(item.get("kind") or "")
        if kind == "rss_item":
            feed_title = item.get("channel") or item.get("podcast_title") or self.t("rss_unknown_feed_title")
            return self.podcasts_download_folder() / self.safe_folder_name(str(feed_title))
        folder = self.music_download_folder()
        if collection or kind in {"playlist", "channel"}:
            title = item.get("title") or self.t("channel" if kind == "channel" else "playlist")
            return folder / self.safe_folder_name(str(title))
        return folder

    def start_download(self, audio_only: bool, item: dict | None = None, remove_queued: bool = False) -> None:
        item = item or self.active_item()
        if not item:
            self.message(self.t("no_selection"))
            return
        if item.get("kind") in {"playlist", "channel"}:
            self.download_collection(item, audio_only=audio_only, remove_queued=remove_queued)
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
        task_id, cancel_event = self.register_download_task(item, audio_only, "single", total=1)
        wx.CallLater(900, self.start_download_worker_thread, item, audio_only, task_id, cancel_event)

    def start_download_worker_thread(self, item: dict, audio_only: bool, task_id: str, cancel_event: threading.Event) -> None:
        threading.Thread(target=self.download_worker, args=(item, audio_only, task_id, cancel_event), daemon=True).start()

    def download_audio(self) -> None:
        self.start_download(True)

    def download_video(self) -> None:
        self.start_download(False)

    def download_collection(self, item: dict | None = None, audio_only: bool = False, remove_queued: bool = False) -> None:
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
        if remove_queued:
            self.remove_queued_url(item.get("url", ""), announce=False)
        start_key = "download_channel_start" if kind == "channel" else "download_playlist_start"
        self.announce_player(self.t("download_started"))
        self.set_status(self.t(start_key))
        task_id, cancel_event = self.register_download_task(item, audio_only, kind, total=0)
        threading.Thread(target=self.download_collection_worker, args=(dict(item), audio_only, task_id, cancel_event), daemon=True).start()

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

    def download_worker(self, item: dict, audio_only: bool, task_id: str, cancel_event: threading.Event) -> None:
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            folder = self.download_folder_for_item(item, audio_only)
            folder.mkdir(parents=True, exist_ok=True)
            options = self.download_options(folder, audio_only, item["title"], task_id=task_id, cancel_event=cancel_event)
            with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
                ydl.download([item["url"]])
            if cancel_event.is_set():
                raise DownloadCancelled()
            done_text = self.t("download_audio_done" if audio_only else "download_video_done", title=item["title"])
            wx.CallAfter(self.finish_download_task, task_id)
            wx.CallAfter(self.record_history, item, "downloaded audio" if audio_only else "downloaded video")
            wx.CallAfter(self.finish_download, done_text, str(folder))
        except DownloadCancelled:
            wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
            wx.CallAfter(self.announce_player, self.t("download_cancelled"))
        except Exception as exc:
            if cancel_event.is_set():
                wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
                wx.CallAfter(self.announce_player, self.t("download_cancelled"))
                return
            wx.CallAfter(self.finish_download_task, task_id, "download_state_failed")
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def download_collection_worker(self, item: dict, audio_only: bool, task_id: str, cancel_event: threading.Event) -> None:
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            kind = str(item.get("kind") or "playlist")
            title = item.get("title") or self.t("channel" if kind == "channel" else "playlist")
            folder = self.download_folder_for_item(item, audio_only, collection=True)
            folder.mkdir(parents=True, exist_ok=True)
            options = self.download_options(folder, audio_only, title, allow_playlist=True, task_id=task_id, cancel_event=cancel_event)
            with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
                ydl.download([self.collection_download_url(item)])
            if cancel_event.is_set():
                raise DownloadCancelled()
            done_key = "download_channel_done" if kind == "channel" else "download_playlist_done"
            wx.CallAfter(self.finish_download_task, task_id)
            wx.CallAfter(self.record_history, item, f"downloaded {kind}")
            wx.CallAfter(self.finish_download, self.t(done_key, title=title), str(folder))
        except DownloadCancelled:
            wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
            wx.CallAfter(self.announce_player, self.t("download_cancelled"))
        except Exception as exc:
            if cancel_event.is_set():
                wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
                wx.CallAfter(self.announce_player, self.t("download_cancelled"))
                return
            wx.CallAfter(self.finish_download_task, task_id, "download_state_failed")
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def finish_download(self, done_text: str, folder: str) -> None:
        self.set_status(done_text)
        focused = self.app_has_focus()
        if not focused:
            self.show_download_complete_notification(done_text)
        elif self.settings.popup_when_download_complete:
            self.message(done_text, wx.ICON_INFORMATION)
        else:
            self.announce_player(done_text)
        if self.settings.open_folder_after_download:
            os.startfile(folder)  # type: ignore[attr-defined]

    def download_options(
        self,
        folder: Path,
        audio_only: bool,
        title: str,
        allow_playlist: bool = False,
        task_id: str | None = None,
        cancel_event: threading.Event | None = None,
    ) -> dict:
        progress_hook = self.make_download_progress_hook(title, audio_only, task_id=task_id, cancel_event=cancel_event)
        template = self.settings.filename_template or DEFAULT_FILENAME_TEMPLATE
        if allow_playlist and self.settings.keep_playlist_order and "%(playlist_index)" not in template:
            template = "%(playlist_index)s - " + template
        options = {
            "outtmpl": str(folder / template),
            "quiet": self.settings.quiet_downloads,
            "noplaylist": not allow_playlist,
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

    def make_download_progress_hook(self, title: str, audio_only: bool, task_id: str | None = None, cancel_event: threading.Event | None = None):
        mode = self.t("download_audio_mode" if audio_only else "download_video_mode")
        last_reported_percent = ""
        last_reported_title = ""
        last_reported_at = 0.0

        def hook(data: dict) -> None:
            nonlocal last_reported_percent, last_reported_title, last_reported_at
            if cancel_event and cancel_event.is_set():
                raise DownloadCancelled()
            status = data.get("status")
            info_dict = data.get("info_dict") or {}
            current_title = info_dict.get("title") or title
            playlist_index = info_dict.get("playlist_index")
            playlist_count = info_dict.get("playlist_count") or info_dict.get("n_entries")
            if status == "downloading":
                percent_text = str(data.get("_percent_str") or "").strip().replace("%", "")
                if not percent_text:
                    downloaded = data.get("downloaded_bytes") or 0
                    total = data.get("total_bytes") or data.get("total_bytes_estimate") or 0
                    if total:
                        percent_text = f"{(float(downloaded) / float(total)) * 100:.1f}"
                try:
                    percent_value = float(percent_text)
                    percent_text = f"{percent_value:.1f}".rstrip("0").rstrip(".")
                    percent_bucket = str(min(100, max(0, int(percent_value))))
                except (TypeError, ValueError):
                    percent_bucket = percent_text
                now = time.monotonic()
                should_report = (
                    percent_bucket != last_reported_percent
                    or current_title != last_reported_title
                    or now - last_reported_at >= 0.75
                )
                if not should_report:
                    return
                last_reported_percent = percent_bucket
                last_reported_title = current_title
                last_reported_at = now
                if task_id:
                    self.ui_queue.put(
                        (
                            "download_task",
                            {
                                "task_id": task_id,
                                "status_key": "download_state_downloading",
                                "current_title": current_title,
                                "percent": percent_text,
                                "playlist_index": playlist_index or 0,
                                "playlist_count": playlist_count or 0,
                            },
                        )
                    )
                if percent_text:
                    self.ui_queue.put(("status", self.t("download_progress", mode=mode, percent=percent_text, title=title)))
            elif status == "finished":
                if task_id:
                    self.ui_queue.put(
                        (
                            "download_task",
                            {
                                "task_id": task_id,
                                "status_key": "download_state_processing",
                                "current_title": current_title,
                                "percent": "100",
                                "playlist_index": playlist_index or 0,
                                "playlist_count": playlist_count or 0,
                            },
                        )
                    )
                self.ui_queue.put(("status", self.t("download_processing", mode=mode, title=title)))

        return hook

    def add_selected_favorite(self) -> None:
        self.add_favorite_item(self.active_item())

    def add_favorite_item(self, item: dict | None) -> None:
        if not item:
            self.message(self.t("no_selection"))
            return
        favorite = {
            "title": item.get("title", ""),
            "channel": item.get("channel", ""),
            "channel_url": item.get("channel_url", ""),
            "kind": item.get("kind", "video"),
            "type": item.get("type", self.t("video")),
            "url": item.get("url", ""),
        }
        if any(existing["url"] == favorite["url"] for existing in self.favorites):
            self.set_status(self.t("favorite_exists"))
            return
        self.favorites.append(favorite)
        self.save_favorites()
        self.set_status(self.t("favorite_added"))

    def refresh_favorites(self) -> None:
        if not hasattr(self, "favorites_list"):
            return
        if self.favorites:
            labels = [f"{index + 1}. {item['title']} | {self.t('channel')}: {item['channel']}" for index, item in enumerate(self.favorites)]
            self.set_listbox_items(self.favorites_list, labels, 0)
        else:
            self.set_listbox_items(self.favorites_list, [self.t("favorites_empty")], 0)
            self.set_status(self.t("favorites_empty"))

    def selected_favorite(self) -> dict | None:
        index = self.favorites_list.GetSelection()
        return None if index == wx.NOT_FOUND or index < 0 or index >= len(self.favorites) else self.favorites[index]

    def play_favorite(self) -> None:
        item = self.selected_favorite()
        if item:
            self.open_library_item(item, "favorites")

    def remove_favorite(self) -> None:
        index = self.favorites_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.favorites):
            del self.favorites[index]
            self.save_favorites()
            self.refresh_favorites()
            self.set_status(self.t("favorite_removed"))

    def open_selected_in_browser(self) -> None:
        item = self.active_item()
        if item:
            webbrowser.open(str(item.get("webpage_url") or item.get("url") or ""))

    def copy_selected_url(self) -> None:
        item = self.active_item()
        if item:
            self.copy_url_to_clipboard(str(item.get("url") or ""))

    def copy_item_url(self, item: dict | None) -> None:
        if item:
            self.copy_url_to_clipboard(str(item.get("url") or ""))

    def copy_direct_stream_url(self, item: dict | None = None) -> None:
        item = item or self.active_item()
        if self.in_player_screen and not item and self.current_stream_url:
            self.copy_url_to_clipboard(self.current_stream_url)
            self.announce_player(self.t("stream_url_copied"))
            return
        if self.in_player_screen and item and self.current_video_item and item.get("url") == self.current_video_item.get("url") and self.current_stream_url:
            self.copy_plain_text_to_clipboard(self.current_stream_url)
            self.announce_player(self.t("stream_url_copied"))
            return
        if not item or not item.get("url"):
            self.message(self.t("no_selection"))
            return
        self.announce_player(self.t("resolving_stream_url"))
        threading.Thread(target=self.copy_direct_stream_url_worker, args=(dict(item),), daemon=True).start()

    def copy_direct_stream_url_worker(self, item: dict) -> None:
        try:
            stream_url, _headers, _info = self.resolve_stream_url(str(item.get("url") or ""))
            wx.CallAfter(self.copy_plain_text_to_clipboard, stream_url)
            wx.CallAfter(self.announce_player, self.t("stream_url_copied"))
        except Exception as exc:
            wx.CallAfter(self.announce_player, self.t("stream_url_failed", error=self.friendly_error(exc)))

    def copy_plain_text_to_clipboard(self, text: str) -> None:
        if not text:
            return
        if wx.TheClipboard.Open():
            try:
                wx.TheClipboard.SetData(wx.TextDataObject(text))
            finally:
                wx.TheClipboard.Close()

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
            if item.get("kind") in {"playlist", "channel"}:
                key = "collection_audio_selected_download" if audio_only else "collection_video_selected_download"
            else:
                key = "audio_selected_download" if audio_only else "video_selected_download"
            self.announce_player(self.t(key, title=item.get("title", "")))
        self.refresh_result_line(self.current_index)
        self.refresh_download_views()

    def remove_queued_url(self, url: str, announce: bool = True) -> None:
        if not url:
            return
        item = self.download_queue.pop(url, None)
        if item and announce:
            self.announce_player(self.t("download_deselected", title=item.get("title", "")))
        self.refresh_results_list_labels()
        if self.rss_items_screen_active:
            self.refresh_rss_items_list()
        if self.in_queue_screen:
            self.refresh_queue_view()
        self.refresh_download_views()

    def refresh_result_line(self, index: int) -> None:
        if not hasattr(self, "results_list") or index < 0 or index >= len(self.results):
            return
        try:
            line = self.result_line(index, self.results[index])
            if self.results_list.GetString(index) == line:
                return
            selection = self.results_list.GetSelection()
            self.results_list.Freeze()
            try:
                self.results_list.SetString(index, line)
                if selection != wx.NOT_FOUND and selection != self.results_list.GetSelection():
                    self.results_list.SetSelection(min(max(0, selection), len(self.results) - 1))
            finally:
                self.results_list.Thaw()
        except RuntimeError:
            pass

    def refresh_queue_view(self) -> None:
        if not self.in_queue_screen or not hasattr(self, "queue_list"):
            return
        if not self.download_queue and not self.active_downloads:
            self.show_download_queue()
            return
        try:
            selection = self.queue_list.GetSelection()
            self.queue_items = self.download_items_snapshot()
            labels = [self.queue_line(item) for item in self.queue_items]
            self.set_listbox_items(self.queue_list, labels, selection)
        except RuntimeError:
            pass

    def download_selected_queue_item(self, audio_only: bool | None = None) -> None:
        item = self.selected_queue_item()
        if not item:
            self.announce_player(self.t("download_queue_empty"))
            return
        if item.get("queue_state") == "active":
            self.cancel_download_task(str(item.get("task_id") or ""))
            return
        if audio_only is None:
            audio_only = bool(item.get("audio_only"))
        if item.get("kind") == "rss_item":
            audio_only = True
        if item.get("kind") in {"playlist", "channel"}:
            self.download_collection(dict(item), audio_only=audio_only, remove_queued=True)
        else:
            self.start_download(audio_only, item=dict(item), remove_queued=True)

    def remove_selected_queue_item(self) -> None:
        item = self.selected_queue_item()
        if not item:
            self.announce_player(self.t("download_queue_empty"))
            return
        if item.get("queue_state") == "active":
            self.cancel_download_task(str(item.get("task_id") or ""))
            return
        self.remove_queued_url(item.get("url", ""), announce=True)

    def cancel_selected_download(self) -> None:
        item = self.selected_queue_item()
        if not item or item.get("queue_state") != "active":
            self.announce_player(self.t("no_active_download"))
            return
        self.cancel_download_task(str(item.get("task_id") or ""))

    def cancel_download_task(self, task_id: str) -> None:
        cancel_event = self.download_cancel_events.get(task_id)
        task = self.active_downloads.get(task_id)
        if not cancel_event or not task:
            self.announce_player(self.t("no_active_download"))
            return
        cancel_event.set()
        task["status_key"] = "download_state_cancelled"
        self.announce_player(self.t("download_cancel_requested", title=task.get("title", "")))
        self.refresh_download_views()

    def cancel_all_downloads(self) -> None:
        if not self.active_downloads:
            self.announce_player(self.t("no_active_download"))
            return
        for task_id in list(self.active_downloads):
            cancel_event = self.download_cancel_events.get(task_id)
            if cancel_event:
                cancel_event.set()
            self.active_downloads[task_id]["status_key"] = "download_state_cancelled"
        self.announce_player(self.t("all_downloads_cancel_requested"))
        self.refresh_download_views()

    def download_all_queued(self) -> None:
        if not self.download_queue:
            self.announce_player(self.t("download_queue_empty"))
            return
        items = list(self.download_queue.values())
        self.download_queue.clear()
        self.refresh_results_list_labels()
        if self.rss_items_screen_active:
            self.refresh_rss_items_list()
        self.announce_player(self.t("batch_download_start", count=len(items)))
        task_id, cancel_event = self.register_download_task({"title": self.t("download_all_selected"), "kind": "batch"}, False, "batch", total=len(items))
        if self.in_queue_screen:
            self.show_download_queue()
        threading.Thread(target=self.download_batch_worker, args=(items, task_id, cancel_event), daemon=True).start()

    def refresh_results_list_labels(self) -> None:
        if not hasattr(self, "results_list"):
            return
        try:
            selection = self.results_list.GetSelection()
            labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
            if not labels:
                labels = [self.t("no_results")]
            self.set_listbox_items(self.results_list, labels, selection)
        except RuntimeError:
            pass

    def download_batch_worker(self, items: list[dict], task_id: str, cancel_event: threading.Event, done_text: str | None = None, finish_folder: str | None = None) -> None:
        folder = self.download_root_folder()
        last_item_folder = folder
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            folder.mkdir(parents=True, exist_ok=True)
            total = len(items)
            for index, item in enumerate(items, start=1):
                if cancel_event.is_set():
                    raise DownloadCancelled()
                audio_only = bool(item.get("audio_only"))
                mode_key = "download_audio_start" if audio_only else "download_video_start"
                self.ui_queue.put(
                    (
                        "download_task",
                        {
                            "task_id": task_id,
                            "status_key": "download_state_downloading",
                            "current_title": item.get("title", ""),
                            "completed": index - 1,
                            "total": total,
                            "percent": "",
                        },
                    )
                )
                wx.CallAfter(self.announce_player, self.t(mode_key))
                override_folder = str(item.get("download_folder_override") or "").strip()
                item_folder = Path(override_folder) if override_folder else self.download_folder_for_item(item, audio_only)
                allow_playlist = False
                url = item["url"]
                if item.get("kind") in {"playlist", "channel"}:
                    item_folder = self.download_folder_for_item(item, audio_only, collection=True)
                    allow_playlist = True
                    url = self.collection_download_url(item)
                item_folder.mkdir(parents=True, exist_ok=True)
                last_item_folder = item_folder
                options = self.download_options(item_folder, audio_only, item.get("title", ""), allow_playlist=allow_playlist, task_id=task_id, cancel_event=cancel_event)
                with ytdlp.YoutubeDL(self.ydl_options(options)) as ydl:
                    ydl.download([url])
                self.ui_queue.put(("download_task", {"task_id": task_id, "completed": index, "total": total}))
            wx.CallAfter(self.finish_download_task, task_id)
            wx.CallAfter(self.finish_batch_download, finish_folder or str(last_item_folder if len(items) == 1 else folder), done_text)
        except DownloadCancelled:
            wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
            wx.CallAfter(self.announce_player, self.t("download_cancelled"))
        except Exception as exc:
            if cancel_event.is_set():
                wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
                wx.CallAfter(self.announce_player, self.t("download_cancelled"))
                return
            wx.CallAfter(self.finish_download_task, task_id, "download_state_failed")
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def finish_batch_download(self, folder: str, done_text: str | None = None) -> None:
        done_text = done_text or self.t("batch_download_done")
        self.set_status(done_text)
        focused = self.app_has_focus()
        if not focused:
            self.show_download_complete_notification(done_text)
        elif self.settings.popup_when_download_complete:
            self.message(done_text, wx.ICON_INFORMATION)
        else:
            self.announce_player(done_text)
        if self.settings.open_folder_after_download:
            os.startfile(folder)  # type: ignore[attr-defined]

    def restore_default_settings(self) -> None:
        self.settings = Settings()
        self.save_settings()
        self.configure_subscription_timer()
        self.configure_rss_timer()
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
        if not self.validate_shortcut_controls():
            return
        self.apply_settings_from_visible_controls()
        self.save_settings()
        self.trim_history()
        self.configure_subscription_timer()
        self.configure_rss_timer()
        self.install_download_accelerators()
        saved_text = self.t("settings_saved")
        self.set_status(saved_text)
        self.speak_text(saved_text)
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
            self.settings.results_limit = self.to_int(self.selected_choice_value("results_limit"), 0, 0, 250)
        if "direct_link_enter_action" in c:
            self.settings.direct_link_enter_action = self.normalize_direct_link_enter_action(self.selected_choice_value("direct_link_enter_action"))
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
        if "close_to_tray" in c:
            self.settings.close_to_tray = c["close_to_tray"].GetValue()
        if "tray_notification" in c:
            self.settings.tray_notification = c["tray_notification"].GetValue()
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
        if "player_speed" in c:
            self.settings.player_speed = c["player_speed"].GetStringSelection() or "1.0"
        if "speed_audio_mode" in c:
            self.settings.speed_audio_mode = self.normalize_speed_audio_mode_value(self.selected_choice_value("speed_audio_mode"))
        if "pitch_mode" in c:
            self.settings.pitch_mode = self.normalize_pitch_mode_value(self.selected_choice_value("pitch_mode"))
        if "show_video_details_by_default" in c:
            self.settings.show_video_details_by_default = c["show_video_details_by_default"].GetValue()
        if "enable_stream_cache" in c:
            self.settings.enable_stream_cache = c["enable_stream_cache"].GetValue()
        if "cache_folder" in c:
            self.settings.cache_folder = c["cache_folder"].GetValue().strip() or str(DEFAULT_CACHE_DIR)
        if "cache_size_mb" in c:
            self.settings.cache_size_mb = self.to_int(c["cache_size_mb"].GetStringSelection(), 512, 128, 4096)
        if "resume_playback" in c:
            self.settings.resume_playback = c["resume_playback"].GetValue()
        if "default_audio_device" in c:
            self.settings.audio_output_device = self.selected_choice_value("default_audio_device") or "auto"
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
        if "fragments" in c:
            self.settings.concurrent_fragments = self.to_int(c["fragments"].GetStringSelection(), 4, 1)
        if "retries" in c:
            self.settings.retries = self.to_int(c["retries"].GetStringSelection(), 10, 0)
        if "timeout" in c:
            self.settings.socket_timeout = self.to_int(c["timeout"].GetStringSelection(), 20, 1)
        if "history_limit" in c:
            self.settings.history_limit = self.to_int(c["history_limit"].GetStringSelection(), 500, 100, 5000)
        if "enable_history" in c:
            self.settings.enable_history = c["enable_history"].GetValue()
        if "enable_podcasts_rss" in c:
            self.settings.enable_podcasts_rss = c["enable_podcasts_rss"].GetValue()
        if "podcast_search_provider" in c:
            self.settings.podcast_search_provider = self.selected_choice_value("podcast_search_provider") or PODCAST_DIRECTORY_PROVIDER_APPLE
        if "podcast_search_country" in c:
            self.settings.podcast_search_country = c["podcast_search_country"].GetStringSelection() or "US"
        if "podcast_search_limit" in c:
            self.settings.podcast_search_limit = self.to_int(c["podcast_search_limit"].GetStringSelection(), 20, 1, 200)
        if "rss_max_items" in c:
            self.settings.rss_max_items = self.to_int(c["rss_max_items"].GetStringSelection(), 100, 1, 500)
        if "rss_refresh_on_startup" in c:
            self.settings.rss_refresh_on_startup = c["rss_refresh_on_startup"].GetValue()
        if "rss_auto_refresh_enabled" in c:
            self.settings.rss_auto_refresh_enabled = c["rss_auto_refresh_enabled"].GetValue()
        if "rss_refresh_interval" in c:
            self.settings.rss_refresh_interval_hours = self.to_int(c["rss_refresh_interval"].GetStringSelection(), 12, 1, 168)
        if "subscription_check_enabled" in c:
            self.settings.subscription_check_enabled = c["subscription_check_enabled"].GetValue()
        if "subscription_check_interval" in c:
            self.settings.subscription_check_interval_hours = self.to_int(c["subscription_check_interval"].GetStringSelection(), 6, 1, 168)
        if "windows_notifications" in c:
            self.settings.windows_notifications = c["windows_notifications"].GetValue()
        if "download_notifications" in c:
            self.settings.download_notifications = c["download_notifications"].GetValue()
        if "subscription_notifications" in c:
            self.settings.subscription_notifications = c["subscription_notifications"].GetValue()
        shortcuts = dict(getattr(self.settings, "keyboard_shortcuts", {}) or {})
        for action, _label_key in SHORTCUT_DEFINITIONS:
            control_key = f"shortcut_{action}"
            if control_key in c:
                shortcuts[action] = c[control_key].GetValue().strip() or DEFAULT_KEYBOARD_SHORTCUTS[action]
        self.settings.keyboard_shortcuts = self.normalized_keyboard_shortcuts(shortcuts)

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
        threading.Thread(target=self.update_ytdlp_worker, daemon=True).start()

    def update_ytdlp_worker(self) -> None:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            self.ui_queue.put(("announce", self.t("missing_ytdlp")))
            return
        try:
            updated = self.update_ytdlp_component_package(ytdlp)
            if updated:
                self.ui_queue.put(("announce", self.t("components_updated")))
        except Exception as exc:
            self.ui_queue.put(("announce", self.t("updates_failed", error=exc)))

    def update_ytdlp_component_package(self, ytdlp_module) -> bool:
        try:
            current_version = str(import_module("yt_dlp.version").__version__)
        except Exception:
            current_version = str(getattr(ytdlp_module, "__version__", "0") or "0")
        latest_version, wheel_url = self.fetch_latest_ytdlp_wheel()
        if not self.is_component_version_newer(latest_version, current_version):
            return False
        if not wheel_url:
            raise RuntimeError("yt-dlp wheel URL is empty")
        self.ui_queue.put(("announce", self.t("components_updating")))
        COMPONENTS_DIR.mkdir(parents=True, exist_ok=True)
        temp_dir = Path(tempfile.mkdtemp(prefix="apricotplayer-ytdlp-"))
        wheel_path = temp_dir / "yt_dlp.whl"
        extract_dir = temp_dir / "extract"
        try:
            request = Request(wheel_url, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
            with self.open_url(request, timeout=120) as response, wheel_path.open("wb") as handle:
                shutil.copyfileobj(response, handle)
            extract_dir.mkdir(parents=True, exist_ok=True)
            with zipfile.ZipFile(wheel_path) as archive:
                archive.extractall(extract_dir)
            package_source = extract_dir / "yt_dlp"
            if not package_source.exists():
                raise RuntimeError("yt-dlp wheel did not contain yt_dlp package")
            package_target = COMPONENTS_DIR / "yt_dlp"
            old_target = COMPONENTS_DIR / "yt_dlp.old"
            renamed_old = False
            if old_target.exists():
                shutil.rmtree(old_target, ignore_errors=True)
            if package_target.exists():
                package_target.rename(old_target)
                renamed_old = True
            try:
                shutil.copytree(package_source, package_target)
            except Exception:
                if renamed_old and old_target.exists() and not package_target.exists():
                    old_target.rename(package_target)
                raise
            for dist_info in COMPONENTS_DIR.glob("yt_dlp-*.dist-info"):
                shutil.rmtree(dist_info, ignore_errors=True)
            for dist_info in extract_dir.glob("yt_dlp-*.dist-info"):
                shutil.copytree(dist_info, COMPONENTS_DIR / dist_info.name)
            if old_target.exists():
                shutil.rmtree(old_target, ignore_errors=True)
            self.reload_ytdlp_after_component_update()
            return True
        finally:
            shutil.rmtree(temp_dir, ignore_errors=True)

    def fetch_latest_ytdlp_wheel(self) -> tuple[str, str]:
        request = Request(YTDLP_PYPI_JSON_URL, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
        latest_version = str((payload.get("info") or {}).get("version") or "")
        urls = payload.get("urls") or []
        for item in urls:
            filename = str(item.get("filename") or "")
            if filename.endswith(".whl") and str(item.get("packagetype") or "") == "bdist_wheel":
                return latest_version, str(item.get("url") or "")
        raise RuntimeError("Could not find a yt-dlp wheel on PyPI")

    @staticmethod
    def is_component_version_newer(remote_version: str, current_version: str) -> bool:
        def parts(value: str) -> tuple[int, ...]:
            return tuple(int(part) for part in re.findall(r"\d+", value)[:4]) or (0,)

        remote_parts = parts(remote_version)
        current_parts = parts(current_version)
        length = max(len(remote_parts), len(current_parts))
        return remote_parts + (0,) * (length - len(remote_parts)) > current_parts + (0,) * (length - len(current_parts))

    @staticmethod
    def reload_ytdlp_after_component_update() -> None:
        global yt_dlp, yt_dlp_import_error
        for name in list(sys.modules):
            if name == "yt_dlp" or name.startswith("yt_dlp."):
                sys.modules.pop(name, None)
        yt_dlp = None
        yt_dlp_import_error = None

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
            try:
                cumulative = self.cumulative_changelog_text(APP_VERSION, remote_version)
                if cumulative:
                    release["_cumulative_changelog"] = cumulative
            except Exception:
                pass
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
            if self.settings.skipped_update_version:
                self.settings.skipped_update_version = ""
                self.save_settings()
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
        attempts: list[tuple[str, dict[str, str]]] = []
        browser_url = str(asset.get("browser_download_url") or "")
        api_url = str(asset.get("url") or "")
        if browser_url:
            attempts.append((browser_url, self.github_headers("", accept="application/octet-stream")))
        if api_url:
            attempts.append((api_url, self.github_headers("", accept="application/octet-stream")))
        if not attempts:
            raise RuntimeError("missing download url")
        last_error: Exception | None = None
        for download_url, headers in attempts:
            try:
                request = Request(download_url, headers=headers)
                with self.open_url(request, timeout=120) as response, downloaded_path.open("wb") as handle:
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
            script_path = self.write_installer_update_script(downloaded_path, str(current_exe.parent), os.getpid(), str(UPDATE_LOG_FILE), restart=True)
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
        self.log_update_event("Exiting ApricotPlayer for update")
        self.exit_for_update()

    @staticmethod
    def is_installer_asset(path_or_name: str | Path) -> bool:
        name = Path(path_or_name).name.lower()
        return name == INSTALLER_ASSET_NAME.lower() or "setup" in name or "installer" in name

    @staticmethod
    def is_portable_zip_asset(path_or_name: str | Path) -> bool:
        name = Path(path_or_name).name.lower()
        return name in {PORTABLE_ZIP_ASSET_NAME.lower(), LEGACY_PORTABLE_ZIP_ASSET_NAME.lower()} or (name.endswith(".zip") and "apricotplayer" in name)

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
                "if ($processIdToWait -gt 0) {",
                "    try { Wait-Process -Id $processIdToWait -Timeout 15 -ErrorAction SilentlyContinue } catch { Log \"Wait-Process warning: $($_.Exception.Message)\" }",
                "    try {",
                "        $stillRunning = Get-Process -Id $processIdToWait -ErrorAction SilentlyContinue",
                "        if ($stillRunning) { Log 'ApricotPlayer did not exit; forcing shutdown'; Stop-Process -Id $processIdToWait -Force -ErrorAction SilentlyContinue }",
                "    } catch { Log \"Force shutdown warning: $($_.Exception.Message)\" }",
                "}",
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
                f"if ($restart) {{ Log 'Restarting ApricotPlayer'; Start-Process -FilePath $target -WorkingDirectory $targetDir -ArgumentList {cls.powershell_literal(UPDATE_RELAUNCH_ARG)} }}",
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
                "if ($processIdToWait -gt 0) {",
                "    try { Wait-Process -Id $processIdToWait -Timeout 15 -ErrorAction SilentlyContinue } catch { Log \"Wait-Process warning: $($_.Exception.Message)\" }",
                "    try {",
                "        $stillRunning = Get-Process -Id $processIdToWait -ErrorAction SilentlyContinue",
                "        if ($stillRunning) { Log 'ApricotPlayer did not exit; forcing shutdown'; Stop-Process -Id $processIdToWait -Force -ErrorAction SilentlyContinue }",
                "    } catch { Log \"Force shutdown warning: $($_.Exception.Message)\" }",
                "}",
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
                f"    if ($restart) {{ Log 'Restarting ApricotPlayer'; Start-Process -FilePath $targetExe -WorkingDirectory $targetDir -ArgumentList {cls.powershell_literal(UPDATE_RELAUNCH_ARG)} }}",
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
    def write_installer_update_script(cls, downloaded_path: str, install_dir: str, process_id: int, log_path: str, restart: bool = True) -> Path:
        script_path = Path(tempfile.gettempdir()) / f"apricotplayer-installer-update-{int(time.time())}.ps1"
        restart_value = "$true" if restart else "$false"
        script = "\n".join(
            [
                "$ErrorActionPreference = 'Stop'",
                f"$source = {cls.powershell_literal(downloaded_path)}",
                f"$installDir = {cls.powershell_literal(install_dir)}",
                f"$log = {cls.powershell_literal(log_path)}",
                f"$processIdToWait = {int(process_id)}",
                f"$restart = {restart_value}",
                "$installerLog = [IO.Path]::ChangeExtension($log, '.inno.log')",
                "$silentArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CLOSEAPPLICATIONS', '/TASKS=desktopicon', ('/DIR=' + $installDir), ('/LOG=' + $installerLog))",
                "$installCandidates = @()",
                "function Normalize-ExecutablePath([string]$path) {",
                "    if (-not $path) { return '' }",
                "    $candidate = $path.Trim().Trim('\"')",
                "    if ($candidate -match '^(.*?\\.exe)') { $candidate = $matches[1] }",
                "    return $candidate",
                "}",
                "function Add-InstallCandidate([string]$path) {",
                "    $candidate = Normalize-ExecutablePath $path",
                "    if ($candidate -and -not ($script:installCandidates -contains $candidate)) { $script:installCandidates += $candidate }",
                "}",
                "function Find-InstalledApricotExe {",
                "    $roots = @(",
                "        'HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*',",
                "        'HKLM:\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*',",
                "        'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*'",
                "    )",
                "    foreach ($root in $roots) {",
                "        try {",
                "            $items = @(Get-ItemProperty -Path $root -ErrorAction SilentlyContinue)",
                "            foreach ($item in $items) {",
                "                if ($item.DisplayName -ne 'ApricotPlayer') { continue }",
                "                if ($item.InstallLocation) {",
                "                    $candidate = Join-Path $item.InstallLocation 'ApricotPlayer.exe'",
                "                    if (Test-Path -LiteralPath $candidate) { return $candidate }",
                "                }",
                "                $icon = Normalize-ExecutablePath ([string]$item.DisplayIcon)",
                "                if ($icon -and (Test-Path -LiteralPath $icon)) { return $icon }",
                "            }",
                "        } catch { }",
                "    }",
                "    return $null",
                "}",
                "function Stop-ApricotProcesses([string[]]$dirs) {",
                "    try {",
                "        $normalizedDirs = @($dirs | Where-Object { $_ } | ForEach-Object { try { [IO.Path]::GetFullPath($_).TrimEnd('\\') } catch { $_ } } | Select-Object -Unique)",
                "        Get-CimInstance Win32_Process -Filter \"Name = 'ApricotPlayer.exe'\" -ErrorAction SilentlyContinue | ForEach-Object {",
                "            $processPath = $_.ExecutablePath",
                "            if (-not $processPath) { return }",
                "            $processDir = Split-Path -Parent $processPath",
                "            try { $processDir = [IO.Path]::GetFullPath($processDir).TrimEnd('\\') } catch { }",
                "            if ($normalizedDirs -contains $processDir) {",
                "                Log \"Stopping ApricotPlayer process $($_.ProcessId) at $processPath\"",
                "                Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue",
                "            }",
                "        }",
                "    } catch { Log \"Process cleanup warning: $($_.Exception.Message)\" }",
                "}",
                "Add-InstallCandidate (Join-Path $installDir 'ApricotPlayer.exe')",
                "if ($env:ProgramFiles) { Add-InstallCandidate (Join-Path $env:ProgramFiles 'ApricotPlayer\\ApricotPlayer.exe') }",
                "if (${env:ProgramFiles(x86)}) { Add-InstallCandidate (Join-Path ${env:ProgramFiles(x86)} 'ApricotPlayer\\ApricotPlayer.exe') }",
                "New-Item -ItemType Directory -Path (Split-Path -Parent $log) -Force | Out-Null",
                "function Log($message) { Add-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' ' + $message) -Encoding UTF8 }",
                "Set-Content -LiteralPath $log -Value ((Get-Date -Format o) + ' Starting ApricotPlayer installer update') -Encoding UTF8",
                "Log \"Installer: $source\"",
                "Log \"Install directory: $installDir\"",
                "Start-Sleep -Milliseconds 500",
                "if ($processIdToWait -gt 0) {",
                "    try { Wait-Process -Id $processIdToWait -Timeout 15 -ErrorAction SilentlyContinue } catch { Log \"Wait-Process warning: $($_.Exception.Message)\" }",
                "    try {",
                "        $stillRunning = Get-Process -Id $processIdToWait -ErrorAction SilentlyContinue",
                "        if ($stillRunning) { Log 'ApricotPlayer did not exit; forcing shutdown'; Stop-Process -Id $processIdToWait -Force -ErrorAction SilentlyContinue }",
                "    } catch { Log \"Force shutdown warning: $($_.Exception.Message)\" }",
                "}",
                "$knownDirs = @($installCandidates | ForEach-Object { Split-Path -Parent $_ } | Where-Object { $_ } | Select-Object -Unique)",
                "Stop-ApricotProcesses $knownDirs",
                "try {",
                "    Log 'Launching installer'",
                "    $process = Start-Process -FilePath $source -ArgumentList $silentArgs -Verb runAs -Wait -PassThru",
                "    if ($process -and $process.ExitCode -ne 0) { throw \"Installer exited with code $($process.ExitCode)\" }",
                "    Log 'Installer completed'",
                "    $installedExe = Find-InstalledApricotExe",
                "    if (-not $installedExe) { $installedExe = Join-Path $installDir 'ApricotPlayer.exe' }",
                "    Add-InstallCandidate $installedExe",
                "    if (-not (Test-Path -LiteralPath $installedExe)) { throw \"Installed ApricotPlayer.exe was not found at $installedExe\" }",
                "    $installedItem = Get-Item -LiteralPath $installedExe",
                "    if ($installedItem.Length -lt 1048576) { throw 'Installed ApricotPlayer.exe is too small.' }",
                "    Log \"Installed executable: $installedExe size=$($installedItem.Length) modified=$($installedItem.LastWriteTimeUtc.ToString('o'))\"",
                "    Remove-Item -LiteralPath $source -Force -ErrorAction SilentlyContinue",
                "    $knownDirs = @($installCandidates | ForEach-Object { Split-Path -Parent $_ } | Where-Object { $_ } | Select-Object -Unique)",
                "    Stop-ApricotProcesses $knownDirs",
                "    if ($restart) {",
                "        $installedDir = Split-Path -Parent $installedExe",
                "        Log \"Restarting ApricotPlayer from $installedExe\"",
                f"        Start-Process -FilePath $installedExe -WorkingDirectory $installedDir -ArgumentList {cls.powershell_literal(UPDATE_RELAUNCH_ARG)}",
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
            self.exiting = True
            self.destroy_taskbar_icon()
            self.Destroy()
            app = wx.GetApp()
            if app:
                app.ExitMainLoop()
        finally:
            os._exit(0)

    def fetch_latest_release(self) -> dict | None:
        latest_request = Request(
            GITHUB_LATEST_RELEASE_API_URL,
            headers=self.github_headers(""),
        )
        try:
            with self.open_url(latest_request, timeout=30) as response:
                release = json.loads(response.read().decode("utf-8"))
            if isinstance(release, dict) and not release.get("draft"):
                return release
        except HTTPError as exc:
            if exc.code != 404:
                raise
        list_request = Request(
            GITHUB_RELEASES_API_URL,
            headers=self.github_headers(""),
        )
        with self.open_url(list_request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
        if isinstance(payload, list):
            releases = [release for release in payload if not release.get("draft")]
            releases.sort(key=lambda release: str(release.get("published_at") or release.get("created_at") or ""), reverse=True)
            return releases[0] if releases else None
        return None

    def fetch_public_releases(self) -> list[dict]:
        request = Request(GITHUB_RELEASES_API_URL, headers=self.github_headers(""))
        with self.open_url(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
        if not isinstance(payload, list):
            return []
        releases = [release for release in payload if isinstance(release, dict) and not release.get("draft")]
        releases.sort(key=lambda release: self.parse_version(self.release_version(release)), reverse=True)
        return releases

    def cumulative_changelog_text(self, current_version: str, latest_version: str) -> str:
        sections: list[str] = []
        for release in self.fetch_public_releases():
            version = self.release_version(release)
            if not version:
                continue
            if self.is_newer_version(version, current_version) and not self.is_newer_version(version, latest_version):
                body = str(release.get("body") or "").replace("\r\n", "\n").strip() or self.t("no_changelog")
                if re.match(r"^#*\s*what'?s new in version", body, flags=re.IGNORECASE):
                    sections.append(body)
                else:
                    sections.append(f"What's new in version {version}\n\n{body}")
        text = "\n\n".join(sections).strip()
        if len(text) > 12000:
            return text[:12000].rstrip() + "\n\n..."
        return text

    def find_release_asset(self, release: dict) -> dict | None:
        assets = release.get("assets") or []
        portable_names = [PORTABLE_ZIP_ASSET_NAME, LEGACY_PORTABLE_ZIP_ASSET_NAME]
        preferred_names = [INSTALLER_ASSET_NAME, *portable_names] if self.is_installed_build() else [*portable_names, INSTALLER_ASSET_NAME]
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
        cumulative = str(release.get("_cumulative_changelog") or "").strip()
        if cumulative:
            return cumulative
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

    @classmethod
    def open_url(cls, request: Request | str, timeout: int = 30):
        return urlopen(request, timeout=timeout, context=cls.ssl_context())

    @staticmethod
    def ssl_context() -> ssl.SSLContext:
        global _SSL_CONTEXT
        if _SSL_CONTEXT is not None:
            return _SSL_CONTEXT
        if certifi:
            _SSL_CONTEXT = ssl.create_default_context(cafile=certifi.where())
        else:
            _SSL_CONTEXT = ssl.create_default_context()
        return _SSL_CONTEXT

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

    def process_queue(self, _event) -> None:
        self.check_activation_signal()
        processed = 0
        max_items_per_tick = 200
        try:
            while processed < max_items_per_tick:
                kind, payload = self.ui_queue.get_nowait()
                processed += 1
                if kind == "results":
                    self.show_results(payload)
                elif kind == "status":
                    self.set_status(str(payload))
                elif kind == "announce":
                    self.announce_player(str(payload))
                elif kind == "download_task" and isinstance(payload, dict):
                    task_id = str(payload.pop("task_id", ""))
                    self.update_download_task(task_id, **payload)
                elif kind == "result_metadata" and isinstance(payload, dict):
                    self.apply_result_metadata(payload)
                elif kind == "notify" and isinstance(payload, tuple):
                    title, message = payload
                    self.show_desktop_notification(str(title), str(message), enabled=self.settings.subscription_notifications)
                elif kind == "app_notification" and isinstance(payload, dict):
                    self.add_app_notification(payload)
                elif kind == "subscriptions_changed":
                    self.refresh_subscriptions()
                elif kind == "rss_feeds_changed":
                    if self.rss_items_screen_active and 0 <= self.current_rss_feed_index < len(self.rss_feeds):
                        selection = 0
                        if hasattr(self, "rss_items_list"):
                            try:
                                selection = self.rss_items_list.GetSelection()
                            except RuntimeError:
                                selection = 0
                        self.rss_items = list(self.rss_feeds[self.current_rss_feed_index].get("items") or [])
                        self.refresh_rss_items_list(selection)
                    else:
                        self.refresh_rss_feed_list()
                elif kind == "podcast_results" and isinstance(payload, dict):
                    self.show_podcast_search_results(list(payload.get("results") or []), str(payload.get("query") or ""))
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

    @staticmethod
    def normalized_keyboard_shortcuts(shortcuts: dict | None) -> dict[str, str]:
        normalized = dict(DEFAULT_KEYBOARD_SHORTCUTS)
        if isinstance(shortcuts, dict):
            for action in DEFAULT_KEYBOARD_SHORTCUTS:
                value = str(shortcuts.get(action) or "").strip()
                if value:
                    normalized[action] = value
        return normalized

    def load_settings(self) -> Settings:
        source = SETTINGS_FILE if SETTINGS_FILE.exists() else LEGACY_SETTINGS_FILE
        if source.exists():
            try:
                data = json.loads(source.read_text(encoding="utf-8"))
                allowed_keys = {field.name for field in fields(Settings)}
                data = {key: value for key, value in data.items() if key in allowed_keys}
                merged = {**asdict(Settings()), **data}
                if merged.get("language") not in LANGUAGE_CODES:
                    merged["language"] = "en"
                if merged.get("filename_template") == OLD_FILENAME_TEMPLATE:
                    merged["filename_template"] = DEFAULT_FILENAME_TEMPLATE
                merged["pitch_mode"] = self.normalize_pitch_mode_value(str(merged.get("pitch_mode") or ""))
                merged["speed_audio_mode"] = self.normalize_speed_audio_mode_value(str(merged.get("speed_audio_mode") or ""))
                merged["direct_link_enter_action"] = self.normalize_direct_link_enter_action(str(merged.get("direct_link_enter_action") or ""))
                merged["video_format"] = self.normalize_video_format_value(str(merged.get("video_format") or ""))
                provider = str(merged.get("podcast_search_provider") or PODCAST_DIRECTORY_PROVIDER_APPLE)
                merged["podcast_search_provider"] = provider if provider in PODCAST_DIRECTORY_PROVIDER_OPTIONS else PODCAST_DIRECTORY_PROVIDER_APPLE
                country = str(merged.get("podcast_search_country") or "US").upper()
                merged["podcast_search_country"] = country if country in PODCAST_COUNTRY_OPTIONS else "US"
                merged["keyboard_shortcuts"] = self.normalized_keyboard_shortcuts(merged.get("keyboard_shortcuts"))
                skipped_version = str(merged.get("skipped_update_version") or "")
                if skipped_version and not self.is_newer_version(skipped_version, APP_VERSION):
                    merged["skipped_update_version"] = ""
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

    def load_history(self) -> list[dict]:
        return self.load_json_list(HISTORY_FILE)

    def save_history(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        HISTORY_FILE.write_text(json.dumps(self.history, indent=2, ensure_ascii=False), encoding="utf-8")

    def load_subscriptions(self) -> list[dict]:
        return self.load_json_list(SUBSCRIPTIONS_FILE)

    def save_subscriptions(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        SUBSCRIPTIONS_FILE.write_text(json.dumps(self.subscriptions, indent=2, ensure_ascii=False), encoding="utf-8")

    def load_rss_feeds(self) -> list[dict]:
        return self.load_json_list(RSS_FEEDS_FILE)

    def save_rss_feeds(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        RSS_FEEDS_FILE.write_text(json.dumps(self.rss_feeds, indent=2, ensure_ascii=False), encoding="utf-8")

    def load_user_playlists(self) -> list[dict]:
        return self.load_json_list(USER_PLAYLISTS_FILE)

    def save_user_playlists(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        USER_PLAYLISTS_FILE.write_text(json.dumps(self.user_playlists, indent=2, ensure_ascii=False), encoding="utf-8")

    def load_notifications(self) -> list[dict]:
        return self.load_json_list(NOTIFICATIONS_FILE)

    def save_notifications(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        NOTIFICATIONS_FILE.write_text(json.dumps(self.notifications, indent=2, ensure_ascii=False), encoding="utf-8")

    def load_playback_positions(self) -> dict:
        return self.load_json_dict(PLAYBACK_POSITIONS_FILE)

    def save_playback_positions(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        PLAYBACK_POSITIONS_FILE.write_text(json.dumps(self.playback_positions, indent=2, ensure_ascii=False), encoding="utf-8")

    @staticmethod
    def load_json_list(path: Path) -> list[dict]:
        if path.exists():
            try:
                data = json.loads(path.read_text(encoding="utf-8"))
                return data if isinstance(data, list) else []
            except Exception:
                return []
        return []

    @staticmethod
    def load_json_dict(path: Path) -> dict:
        if path.exists():
            try:
                data = json.loads(path.read_text(encoding="utf-8"))
                return data if isinstance(data, dict) else {}
            except Exception:
                return {}
        return {}

    def record_history(self, item: dict, action: str) -> None:
        if not self.settings.enable_history:
            return
        url = str(item.get("url") or "")
        if not url:
            return
        entry = {
            "title": item.get("title", ""),
            "channel": item.get("channel", ""),
            "channel_url": item.get("channel_url", ""),
            "channel_id": item.get("channel_id", ""),
            "url": url,
            "kind": item.get("kind", "video"),
            "type": item.get("type", self.t("video")),
            "action": action,
            "timestamp": time.time(),
        }
        self.history = [existing for existing in self.history if existing.get("url") != url]
        self.history.insert(0, entry)
        self.trim_history()
        self.save_history()

    def trim_history(self) -> None:
        limit = max(10, int(getattr(self.settings, "history_limit", 500) or 500))
        if len(self.history) > limit:
            self.history = self.history[:limit]
            self.save_history()

    @staticmethod
    def youtube_search_url(query: str, search_type: str) -> str:
        filters = {"Playlist": "EgIQAw==", "Channel": "EgIQAg==", "Kanal": "EgIQAg=="}
        return f"https://www.youtube.com/results?{urlencode({'search_query': query, 'sp': filters.get(search_type, '')})}"

    @staticmethod
    def normalize_channel_url(entry: dict) -> str:
        for key in ("channel_url", "uploader_url"):
            value = str(entry.get(key) or "").strip()
            if value:
                return value if value.startswith("http") else f"https://www.youtube.com/{value.lstrip('/')}"
        channel_id = str(entry.get("channel_id") or entry.get("uploader_id") or "").strip()
        if channel_id.startswith("UC"):
            return f"https://www.youtube.com/channel/{channel_id}"
        return ""

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
    def format_history_time(timestamp) -> str:
        try:
            return datetime.fromtimestamp(float(timestamp)).strftime("%Y-%m-%d %H:%M")
        except Exception:
            return ""

    @staticmethod
    def make_ipc_path() -> str:
        return rf"\\.\pipe\urhasaurus-youtube-{os.getpid()}" if os.name == "nt" else f"/tmp/urhasaurus-youtube-{os.getpid()}.sock"


def startup_language() -> str:
    try:
        data = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
        language = str(data.get("language") or "en")
        return language if language in TEXT else "en"
    except Exception:
        return "en"


def startup_text(key: str) -> str:
    language = startup_language()
    return TEXT.get(language, TEXT["en"]).get(key, TEXT["en"].get(key, key))


def update_relaunch_requested() -> bool:
    return any(arg == UPDATE_RELAUNCH_ARG for arg in sys.argv[1:])


def mark_update_relaunch_window(seconds: int = 45) -> None:
    try:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        payload = {"expires_at": time.time() + max(5, seconds)}
        UPDATE_RELAUNCH_SENTINEL.write_text(json.dumps(payload), encoding="utf-8")
    except Exception:
        pass


def suppress_already_open_for_update() -> bool:
    try:
        payload = json.loads(UPDATE_RELAUNCH_SENTINEL.read_text(encoding="utf-8"))
        expires_at = float(payload.get("expires_at", 0) or 0)
        if expires_at >= time.time():
            return True
        UPDATE_RELAUNCH_SENTINEL.unlink(missing_ok=True)
    except Exception:
        pass
    return False


def request_existing_instance_activation(action: str = "show") -> None:
    try:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        payload = {"action": action, "pid": os.getpid(), "timestamp": time.time()}
        ACTIVATE_SIGNAL_FILE.write_text(json.dumps(payload), encoding="utf-8")
    except Exception:
        pass


class App(wx.App):
    def OnInit(self) -> bool:
        if update_relaunch_requested():
            mark_update_relaunch_window()
        instance_name = f"{APP_NAME}-{wx.GetUserId() or 'user'}"
        self.instance_checker = wx.SingleInstanceChecker(instance_name)
        if self.instance_checker.IsAnotherRunning():
            if suppress_already_open_for_update():
                return False
            request_existing_instance_activation("show")
            return False
        frame = MainFrame()
        frame.Show()
        return True


def main() -> int:
    app = App(False)
    app.MainLoop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
