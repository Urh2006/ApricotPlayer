from __future__ import annotations

import asyncio
import http.cookiejar
import json
import hashlib
import os
import queue
import random
import re
import shlex
import shutil
import ssl
import socket
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
from urllib.parse import unquote, urlencode, urljoin, urlparse
from urllib.request import Request, urlopen
from urllib.error import HTTPError
import xml.etree.ElementTree as ET

import wx
import wx.adv

try:
    import winreg
except ImportError:
    winreg = None

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
        disable_external_ytdlp_plugins()
    except ImportError as exc:
        yt_dlp_import_error = exc
        return None
    return yt_dlp


def disable_external_ytdlp_plugins() -> None:
    try:
        import_module("yt_dlp.globals").plugin_dirs.value = []
    except Exception:
        pass

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


class MemoryYtdlpLogger:
    def __init__(self) -> None:
        self.messages: list[str] = []

    def info(self, message: str, *args, **kwargs) -> None:
        text = str(message or "").strip()
        if text and not text.startswith("Extracting cookies from") and not text.startswith("Extracted "):
            self.messages.append(text)

    def debug(self, message: str, *args, **kwargs) -> None:
        text = str(message or "").strip()
        if text and not text.startswith("Searching for") and not text.startswith("Loading cookie"):
            self.messages.append(text)

    def warning(self, message: str, *args, **kwargs) -> None:
        text = str(message or "").strip()
        if text:
            self.messages.append(text)

    def error(self, message: str, *args, **kwargs) -> None:
        text = str(message or "").strip()
        if text:
            self.messages.append(text)

    def progress_bar(self):
        return None

    def summary(self) -> str:
        seen: set[str] = set()
        lines: list[str] = []
        for message in self.messages:
            if message not in seen:
                seen.add(message)
                lines.append(message)
            if len(lines) >= 5:
                break
        return "\n".join(lines)


class DownloadCancelled(Exception):
    pass


class SliderAccessible(wx.Accessible):
    def GetName(self, childId):
        window = self.GetWindow()
        if childId == 0 and window:
            name = getattr(window, "_apricot_accessible_name", "") or window.GetName()
            return wx.ACC_OK, str(name)
        return wx.ACC_NOT_IMPLEMENTED, ""

    def GetDescription(self, childId):
        window = self.GetWindow()
        if childId == 0 and window:
            description = getattr(window, "_apricot_accessible_description", "") or window.GetToolTipText()
            return wx.ACC_OK, str(description or "")
        return wx.ACC_NOT_IMPLEMENTED, ""

    def GetValue(self, childId):
        window = self.GetWindow()
        if childId == 0 and window:
            value = getattr(window, "_apricot_accessible_value", "")
            return wx.ACC_OK, str(value)
        return wx.ACC_NOT_IMPLEMENTED, ""

    def GetRole(self, childId):
        if childId == 0:
            return wx.ACC_OK, wx.ROLE_SYSTEM_SLIDER
        return wx.ACC_NOT_IMPLEMENTED, 0


class PlayerPanel(wx.Panel):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        try:
            self.SetCanFocus(True)
        except Exception:
            pass

    def AcceptsFocus(self) -> bool:
        return True

    def AcceptsFocusFromKeyboard(self) -> bool:
        return True


YTDLP_LOGGER = QuietYtdlpLogger()
APP_NAME = "ApricotPlayer"
APP_VERSION = "0.8.45"
APP_VERSION_LABEL = "0.8.45"
WINDOW_TITLE = f"{APP_NAME} {APP_VERSION_LABEL}"
LEGACY_APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "UrhasaurusYouTubePlayer"
APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "ApricotPlayer"
UPDATE_RELAUNCH_ARG = "--updated-relaunch"
START_IN_TRAY_ARG = "--start-in-tray"
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
PLAYBACK_QUEUE_FILE = APP_DIR / "playback_queue.json"
CACHED_COOKIES_FILE = APP_DIR / "cookies.txt"
COMPONENTS_DIR = APP_DIR / "components"
LEGACY_SETTINGS_FILE = LEGACY_APP_DIR / "settings.json"
LEGACY_FAVORITES_FILE = LEGACY_APP_DIR / "favorites.json"
DEFAULT_DOWNLOAD_ROOT = Path.home() / "Downloads" / "ApricotPlayer"
DEFAULT_CACHE_DIR = APP_DIR / "cache"
DEFAULT_FILENAME_TEMPLATE = "%(title)s.%(ext)s"
OLD_FILENAME_TEMPLATE = "%(title)s [%(id)s].%(ext)s"
RESULTS_PAGE_SIZE = 20
RESULT_METADATA_HYDRATION_BATCH = 5
VIDEO_DOWNLOAD_MIN_FRAGMENTS = 8
VIDEO_DOWNLOAD_HTTP_CHUNK_SIZE = 10 << 20
VIDEO_DOWNLOAD_BUFFER_SIZE = 1024 << 10
# Zero means dynamic mode has no app-side cap; Apricot keeps asking the source
# for the next 20 results until the source stops returning more.
DYNAMIC_RESULTS_MAX = 0
REFRESH_INTERVAL_OPTIONS = ["0.5", "1", "2", "3", "6", "12", "24"]
TRENDING_COUNTRIES: list[tuple[str, str]] = [
    ("global", "Global"),
    ("AR", "Argentina"), ("AU", "Australia"), ("AT", "Austria"), ("BE", "Belgium"),
    ("BR", "Brazil"), ("CA", "Canada"), ("CL", "Chile"), ("CO", "Colombia"),
    ("CZ", "Czechia"), ("DK", "Denmark"), ("EG", "Egypt"), ("FI", "Finland"),
    ("FR", "France"), ("DE", "Germany"), ("GR", "Greece"), ("HK", "Hong Kong"),
    ("HU", "Hungary"), ("IN", "India"), ("ID", "Indonesia"), ("IE", "Ireland"),
    ("IL", "Israel"), ("IT", "Italy"), ("JP", "Japan"), ("KE", "Kenya"),
    ("MY", "Malaysia"), ("MX", "Mexico"), ("NL", "Netherlands"), ("NZ", "New Zealand"),
    ("NG", "Nigeria"), ("NO", "Norway"), ("PK", "Pakistan"), ("PE", "Peru"),
    ("PH", "Philippines"), ("PL", "Poland"), ("PT", "Portugal"), ("RO", "Romania"),
    ("RU", "Russia"), ("SA", "Saudi Arabia"), ("RS", "Serbia"), ("SG", "Singapore"),
    ("SK", "Slovakia"), ("SI", "Slovenia"), ("ZA", "South Africa"), ("KR", "South Korea"),
    ("ES", "Spain"), ("SE", "Sweden"), ("CH", "Switzerland"), ("TW", "Taiwan"),
    ("TH", "Thailand"), ("TR", "Turkey"), ("UA", "Ukraine"), ("AE", "United Arab Emirates"),
    ("GB", "United Kingdom"), ("US", "United States"), ("VN", "Vietnam"),
]
TRENDING_CATEGORIES: list[tuple[str, str]] = [
    ("all", "All"),
    ("music", "Music"),
    ("movies", "Movies"),
    ("gaming", "Gaming"),
    ("sports", "Sports"),
    ("news", "News"),
    ("entertainment", "Entertainment"),
    ("comedy", "Comedy"),
    ("technology", "Science & Technology"),
]
YOUTUBE_API_VIDEOS_URL = "https://www.googleapis.com/youtube/v3/videos"
TRENDING_CATEGORY_IDS: dict[str, str] = {
    "all": "0",
    "music": "10",
    "movies": "1",
    "gaming": "20",
    "sports": "17",
    "news": "25",
    "entertainment": "24",
    "comedy": "23",
    "technology": "28",
}
TRENDING_PUBLIC_URLS: dict[str, list[str]] = {
    "all": ["https://www.youtube.com/feed/trending?gl={country}"],
    "music": ["https://charts.youtube.com/charts/TrendingVideos/{country_lower}/weekly"],
    "movies": ["https://www.youtube.com/feed/trending?bp=4gIKGgh0cmFpbGVycw%3D%3D&gl={country}"],
    "gaming": ["https://www.youtube.com/gaming?gl={country}"],
}
EQ_FILTER_LABEL = "apricot_eq"
EQ_FILTER_REF = f"@{EQ_FILTER_LABEL}"
EQ_BANDS: list[tuple[str, str]] = [
    ("31", "31 Hz sub bass rumble"),
    ("62", "62 Hz bass thump"),
    ("125", "125 Hz upper bass warmth"),
    ("250", "250 Hz low mids"),
    ("500", "500 Hz mids body"),
    ("1000", "1 kHz midrange presence"),
    ("2000", "2 kHz vocal clarity"),
    ("4000", "4 kHz attack and detail"),
    ("8000", "8 kHz brightness"),
    ("16000", "16 kHz air and sparkle"),
]
EQ_RANGE_OPTIONS = ["6", "12", "18", "24"]
EQ_PRESET_FLAT = "flat"
EQ_CUSTOM_PRESET_IDS = ["custom1", "custom2", "custom3"]
EQ_FACTORY_PRESET_VALUES: dict[str, list[float]] = {
    "flat": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    "bass_boost": [5, 4, 3, 2, 1, 0, 0, 0, 0, 0],
    "full_bass_treble": [4, 3, 0, -4, -2, 1, 4, 5, 6, 6],
    "dance": [5, 4, 2, 0, -1, -1, 0, 2, 3, 3],
    "hip_hop": [4, 5, 3, 1, -1, -1, 1, 2, 2, 1],
    "electronic": [4, 3, 0, -3, -2, 0, 4, 5, 5, 4],
    "rock": [4, 3, 1, -2, -3, -1, 2, 4, 5, 5],
    "pop": [-1, 2, 3, 4, 2, 0, -1, -1, 0, 1],
    "classical": [0, 0, 0, 0, 0, 0, -2, -2, -2, -3],
    "jazz": [2, 1, 0, 1, 2, 2, 1, 2, 3, 2],
    "acoustic": [2, 3, 2, 1, 0, 1, 2, 3, 2, 1],
    "vocal": [-2, -1, 0, 1, 2, 3, 4, 3, 1, -1],
    "podcast": [-3, -2, -1, 0, 2, 3, 4, 3, 0, -2],
    "bright": [-1, -1, 0, 0, 1, 2, 3, 4, 5, 5],
    "mellow": [1, 2, 1, 0, -1, -1, -1, -2, -3, -3],
    "treble_boost": [-5, -4, -3, -1, 1, 3, 5, 6, 6, 6],
    "laptop_headphones": [3, 5, 3, -2, 0, -3, -4, -4, 0, 0],
    "late_night": [2, 2, 1, 0, -1, -2, -1, 0, 1, 1],
}
EQ_PRESET_OPTIONS = list(EQ_FACTORY_PRESET_VALUES.keys()) + EQ_CUSTOM_PRESET_IDS
LOCAL_MEDIA_EXTENSIONS = {
    ".3g2",
    ".3ga",
    ".3gp",
    ".aac",
    ".ac3",
    ".amr",
    ".ape",
    ".au",
    ".aiff",
    ".aif",
    ".aifc",
    ".avi",
    ".caf",
    ".divx",
    ".dts",
    ".flac",
    ".flv",
    ".m4a",
    ".m4v",
    ".m2ts",
    ".m2v",
    ".mkv",
    ".mka",
    ".mov",
    ".mpe",
    ".mp3",
    ".mp2",
    ".mp2v",
    ".mp4",
    ".mpv",
    ".mpeg",
    ".mpg",
    ".mts",
    ".mxf",
    ".oga",
    ".ogm",
    ".ogv",
    ".ogg",
    ".ogx",
    ".opus",
    ".ra",
    ".rm",
    ".rmvb",
    ".snd",
    ".ts",
    ".vob",
    ".wav",
    ".weba",
    ".webm",
    ".wma",
    ".wmv",
}
AUDIO_CONVERT_FORMATS = ["mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "wma", "aiff", "alac", "ac3", "mp2"]
VIDEO_CONVERT_FORMATS = ["mp4", "mkv", "webm", "mov", "avi", "wmv", "m4v", "mpg", "mpeg", "flv", "3gp", "ogv", "ts", "m2ts", "asf"]
AUDIO_INPUT_EXTENSIONS = {f".{fmt}" for fmt in AUDIO_CONVERT_FORMATS} | {".aif", ".aifc", ".ape", ".mka"}
VIDEO_INPUT_EXTENSIONS = {f".{fmt}" for fmt in VIDEO_CONVERT_FORMATS} | {".asf", ".divx", ".m2ts", ".mts", ".ts", ".vob"}
CONVERTER_MEDIA_EXTENSIONS = AUDIO_INPUT_EXTENSIONS | VIDEO_INPUT_EXTENSIONS
CONVERTER_IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".bmp", ".webp", ".gif", ".tif", ".tiff"}
LOCAL_MEDIA_EXTENSIONS.update(CONVERTER_MEDIA_EXTENSIONS)
DEFAULT_GITHUB_OWNER = "Urh2006"
DEFAULT_GITHUB_REPO = "ApricotPlayer"
GITHUB_RELEASES_API_URL = f"https://api.github.com/repos/{DEFAULT_GITHUB_OWNER}/{DEFAULT_GITHUB_REPO}/releases"
GITHUB_LATEST_RELEASE_API_URL = f"https://api.github.com/repos/{DEFAULT_GITHUB_OWNER}/{DEFAULT_GITHUB_REPO}/releases/latest"
INSTALLER_ASSET_NAME = "ApricotPlayerSetup.exe"
PORTABLE_ZIP_ASSET_NAME = "ApricotPlayer.zip"
LEGACY_PORTABLE_ZIP_ASSET_NAME = "ApricotPlayerPortable.zip"
UPDATE_LOG_FILE = APP_DIR / "updater.log"
UPDATE_DOWNLOAD_CHUNK_SIZE = 1024 * 512
UPDATE_PROGRESS_MIN_INTERVAL = 0.35
YTDLP_PYPI_JSON_URL = "https://pypi.org/pypi/yt-dlp/json"
PLAYBACK_SPEED_STEPS = [0.25, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 1.0, 1.1, 1.2, 1.25, 1.3, 1.4, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0]
PITCH_STEPS = [0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95, 1.0, 1.05, 1.1, 1.15, 1.2, 1.25, 1.3, 1.35, 1.4, 1.45, 1.5, 1.6, 1.7, 1.8, 1.9, 2.0]
DEFAULT_REACHED_SOUND = "default_reached.wav"
PITCH_MODE_RUBBERBAND = "Independent pitch - advanced (Rubberband)"
PITCH_MODE_MPV = "Independent pitch - highest quality (mpv built-in)"
PITCH_MODE_LINKED_SPEED = "Linked pitch and speed - pitch keys change both"
PITCH_MODE_OPTIONS = [PITCH_MODE_MPV, PITCH_MODE_RUBBERBAND, PITCH_MODE_LINKED_SPEED]
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
SPEED_AUDIO_MODE_OPTIONS = [SPEED_AUDIO_MODE_RUBBERBAND, SPEED_AUDIO_MODE_SCALETEMPO2, SPEED_AUDIO_MODE_MPV, SPEED_AUDIO_MODE_SCALETEMPO]
DIRECT_LINK_ENTER_PLAY = "play"
DIRECT_LINK_ENTER_AUDIO = "download_audio"
DIRECT_LINK_ENTER_VIDEO = "download_video"
DIRECT_LINK_ENTER_STREAM = "copy_stream_url"
DIRECT_LINK_ENTER_OPTIONS = [DIRECT_LINK_ENTER_PLAY, DIRECT_LINK_ENTER_AUDIO, DIRECT_LINK_ENTER_VIDEO, DIRECT_LINK_ENTER_STREAM]
COOKIES_BROWSER_OPTIONS = ["none", "chrome", "edge", "firefox", "brave", "chromium", "opera", "vivaldi"]
COOKIE_PROFILE_AUTO = "auto"
COOKIE_PROFILE_OPTIONS = [COOKIE_PROFILE_AUTO]
CHROMIUM_COOKIE_BROWSERS = {"brave", "chrome", "edge", "chromium", "opera", "vivaldi"}
COOKIES_BROWSER_PROCESS_NAMES = {
    "chrome": ["chrome"],
    "edge": ["msedge"],
    "brave": ["brave"],
    "chromium": ["chromium"],
    "opera": ["opera"],
    "vivaldi": ["vivaldi"],
    "firefox": ["firefox"],
}
VIDEO_FORMAT_MP4 = "mp4"
VIDEO_FORMAT_BEST_ANY = "best-any"
VIDEO_FORMAT_MP4_SINGLE = "mp4-single"
VIDEO_FORMAT_SMALLEST = "smallest"
VIDEO_FORMAT_OPTIONS = [VIDEO_FORMAT_MP4, VIDEO_FORMAT_BEST_ANY, VIDEO_FORMAT_MP4_SINGLE, VIDEO_FORMAT_SMALLEST]
AUDIO_QUALITY_OPTIONS = ["0", "320", "256", "192", "160", "128", "96", "64", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]
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
VK_OEM_4_LEFT_BRACKET = 0xDB
VK_OEM_6_RIGHT_BRACKET = 0xDD
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
    "open_main_menu": "Ctrl+Alt+M",
    "open_search": "Ctrl+Alt+Y",
    "open_play_from_folder": "Ctrl+Alt+O",
    "open_direct_link": "Ctrl+Alt+L",
    "open_favorites": "Ctrl+Alt+F",
    "open_playlists": "Ctrl+Alt+P",
    "open_subscriptions": "Ctrl+Alt+B",
    "open_current_downloads": "Ctrl+Alt+D",
    "open_history": "Ctrl+Alt+H",
    "open_podcasts_rss": "Ctrl+Alt+R",
    "open_settings": "Ctrl+Alt+S",
    "background_play_pause": "Ctrl+Space",
    "download_audio": "Ctrl+Shift+A",
    "download_video": "Ctrl+Shift+D",
    "subscribe_channel": "Ctrl+Shift+S",
    "unsubscribe_channel": "Ctrl+Shift+U",
    "open_channel": "Ctrl+Shift+O",
    "queue_audio": "Shift+A",
    "add_to_playback_queue": "Ctrl+Shift+Q",
    "remove_from_playback_queue": "Ctrl+Shift+Delete",
    "open_playback_queue": "Ctrl+Alt+Q",
    "create_playlist": "Ctrl+Shift+N",
    "add_favorite": "Ctrl+F",
    "remove_favorite": "Ctrl+Shift+F",
    "add_to_playlist": "Ctrl+P",
    "remove_from_playlist": "Ctrl+Shift+P",
    "copy_link": "Ctrl+L",
    "copy_stream_url": "Ctrl+D",
    "context_menu": "Applications",
    "open_selected": "Enter",
    "new_subscription_videos": "Ctrl+Shift+V",
    "remove_selected": "Delete",
    "player_copy_link": "L",
    "player_play_pause": "Space",
    "player_time": "T",
    "player_speed_down": "S",
    "player_speed_up": "D",
    "player_pitch_up": "Ctrl+Up",
    "player_pitch_down": "Ctrl+Down",
    "player_volume_status": "V",
    "player_details": "F7",
    "player_output_devices": "O",
    "player_equalizer": "F4",
    "player_edit_mode": "E",
    "player_save_edit_copy": "Ctrl+S",
    "player_replace_edit_original": "Ctrl+R",
    "player_marker_start": "LeftBracket",
    "player_marker_end": "RightBracket",
    "player_previous": "Ctrl+PageUp",
    "player_next": "Ctrl+PageDown",
    "player_back": "Escape",
    "player_volume_boost": "F2",
    "player_bass_boost": "F3",
    "player_repeat": "R",
    "player_shuffle": "Shift+S",
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
    ("open_main_menu", "shortcut_open_main_menu"),
    ("open_search", "shortcut_open_search"),
    ("open_play_from_folder", "shortcut_open_play_from_folder"),
    ("open_direct_link", "shortcut_open_direct_link"),
    ("open_favorites", "shortcut_open_favorites"),
    ("open_playlists", "shortcut_open_playlists"),
    ("open_subscriptions", "shortcut_open_subscriptions"),
    ("open_current_downloads", "shortcut_open_current_downloads"),
    ("open_history", "shortcut_open_history"),
    ("open_podcasts_rss", "shortcut_open_podcasts_rss"),
    ("open_settings", "shortcut_open_settings"),
    ("background_play_pause", "shortcut_background_play_pause"),
    ("download_audio", "shortcut_download_audio"),
    ("download_video", "shortcut_download_video"),
    ("subscribe_channel", "shortcut_subscribe_channel"),
    ("unsubscribe_channel", "shortcut_unsubscribe_channel"),
    ("open_channel", "shortcut_open_channel"),
    ("queue_audio", "shortcut_queue_audio"),
    ("add_to_playback_queue", "shortcut_add_to_playback_queue"),
    ("remove_from_playback_queue", "shortcut_remove_from_playback_queue"),
    ("open_playback_queue", "shortcut_open_playback_queue"),
    ("create_playlist", "shortcut_create_playlist"),
    ("add_favorite", "shortcut_add_favorite"),
    ("remove_favorite", "shortcut_remove_favorite"),
    ("add_to_playlist", "shortcut_add_to_playlist"),
    ("remove_from_playlist", "shortcut_remove_from_playlist"),
    ("copy_link", "shortcut_copy_link"),
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
    ("player_volume_status", "shortcut_player_volume_status"),
    ("player_details", "shortcut_player_details"),
    ("player_output_devices", "shortcut_player_output_devices"),
    ("player_equalizer", "shortcut_player_equalizer"),
    ("player_edit_mode", "shortcut_player_edit_mode"),
    ("player_save_edit_copy", "shortcut_player_save_edit_copy"),
    ("player_replace_edit_original", "shortcut_player_replace_edit_original"),
    ("player_marker_start", "shortcut_player_marker_start"),
    ("player_marker_end", "shortcut_player_marker_end"),
    ("player_previous", "shortcut_player_previous"),
    ("player_next", "shortcut_player_next"),
    ("player_back", "shortcut_player_back"),
    ("player_volume_boost", "shortcut_player_volume_boost"),
    ("player_bass_boost", "shortcut_player_bass_boost"),
    ("player_repeat", "shortcut_player_repeat"),
    ("player_shuffle", "shortcut_player_shuffle"),
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
        "download_all_as_audio": "Prenesi vse kot zvok",
        "download_all_as_video": "Prenesi vse kot video",
        "download_all_selected": "Prenesi vse izbrane elemente",
        "queued_videos_for_download": "Queued videos for download",
        "queued_downloads": "Queued videos for download",
        "current_downloads": "Trenutni prenosi",
        "no_queued_downloads": "No queued downloads.",
        "queued_download_instructions": "Enter vprasa za zvok ali video. Uporabis lahko tudi Ctrl+Shift+A za zvok, Ctrl+Shift+D za video ali kontekstni meni.",
        "select_download_format": "Izberi obliko prenosa",
        "select_download_format_message": "Izberi, ali naj se ta element prenese kot zvok ali video.",
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
        "trending": "V trendu",
        "trending_country": "Drzava za trende",
        "trending_category": "Kategorija trendov",
        "load_trending": "Nalozi trende",
        "loading_trending": "Nalagam trende: {query}",
        "trending_all": "Vse",
        "trending_music": "Glasba",
        "trending_movies": "Filmi",
        "trending_gaming": "Gaming",
        "trending_sports": "Sport",
        "trending_news": "Novice",
        "trending_podcasts": "Podkasti",
        "trending_technology": "Tehnologija",
        "trending_comedy": "Komedija",
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
        "player": "Predvajalnik",
        "background_player": "Predvajalnik",
        "background_player_now_playing": "Predvajalnik: {title}",
        "background_player_hint": "Predvajanje se nadaljuje v ozadju.",
        "open_player": "Odpri predvajalnik",
        "close_player": "Zapri predvajalnik",
        "player_closed": "Predvajalnik zaprt.",
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
        "selected_for_download_or_playlist": "Izbrano za prenos ali playlisto: {title}",
        "audio_selected_download": "Audio download queued: {title}",
        "video_selected_download": "Video download queued: {title}",
        "collection_audio_selected_download": "Zvok zbirke dodan v cakalno vrsto: {title}",
        "collection_video_selected_download": "Video zbirke dodan v cakalno vrsto: {title}",
        "download_deselected": "Removed from download queue: {title}",
        "download_queue_empty": "Download queue is empty.",
        "selected_queued_marker": "izbrano",
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
        "channel_options": "Moznosti kanala",
        "channel_home": "Domov",
        "channel_videos": "Videi",
        "channel_popular": "Popularni videi",
        "channel_playlists": "Playliste kanala",
        "settings_file": "Settings file",
        "restore_defaults": "Restore to defaults",
        "reset_all_settings": "Reset all settings",
        "reset_settings_for_section": "Reset settings for {section}",
        "section_settings_reset": "{section} settings reset.",
        "defaults_restored": "Default settings restored.",
        "loading_more_results": "Loading more results.",
        "no_more_results": "No more results.",
        "auto_update_app": "Ob zagonu preveri posodobitve programa",
        "app_update_interval": "Interval preverjanja posodobitev programa",
        "app_update_notifications": "Windows obvestilo, ko je na voljo posodobitev programa",
        "app_update_menu_item": "Na voljo je posodobitev: {version}",
        "app_update_ready_status": "Na voljo je posodobitev programa {version}.",
        "app_update_notification_message": "ApricotPlayer {version} je na voljo.",
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
        "open_channel": "Odpri kanal",
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
        "enable_age_restricted_videos": "Podpora za starostno omejene YouTube videe (pocasnejsi fallback samo po potrebi)",
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
        "bass_boost": "Bass boost",
        "bass_boost_on": "Bass boost vklopljen.",
        "bass_boost_off": "Bass boost izklopljen.",
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
        "audio_quality": "Audio kvaliteta",
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
        "choose_cookies_file": "Izberi cookies.txt datoteko",
        "cookies_from_browser": "Cookies from browser",
        "cookies_browser_profile": "Browser profile",
        "browser_profile_auto": "Auto - poskusi vse profile",
        "export_browser_cookies": "Export browser cookies to cookies.txt",
        "exporting_browser_cookies": "Exporting browser cookies.",
        "browser_cookies_exported": "Browser cookies exported to {path} from {profile}.",
        "browser_cookies_export_failed": "Browser cookies export failed: {error}",
        "browser_cookies_no_youtube": "Cookies exported, but no YouTube login cookies were found. Odpri YouTube v izbranem browserju, se prijavi, potem poskusi znova.",
        "select_cookies_browser": "Najprej izberi browser pri Cookies from browser.",
        "close_browser_for_cookie_export_title": "Zapri browser za export cookies",
        "close_browser_for_cookie_export_message": "{browser} je odprt. ApricotPlayer ga lahko zapre in potem exporta cookies, kar obicajno popravi Chrome cookie database error. Shrani odprte stvari v browserju, potem izberi Yes. Nadaljujem?",
        "browser_closed_for_cookie_export": "Browser zaprt. Exportam cookies.",
        "cookies_file_selected": "Cookies file selected: {path}",
        "cookies_file_login_found": "YouTube login cookies found in selected cookies file.",
        "cookies_file_no_login_warning": "The selected cookies file does not appear to contain YouTube login cookies. ApricotPlayer will still use it, but YouTube may keep asking you to sign in.",
        "cookies_file_load_failed": "Could not read selected cookies file: {error}",
        "cookie_auto_refresh_start": "YouTube rabi prijavne cookies. Osvezujem cookies iz {browser}.",
        "cookie_auto_refresh_done": "Cookies osvezeni iz {profile}. Poskusam znova.",
        "cookie_auto_refresh_failed": "Cookies se niso mogli osveziti: {error}",
        "cookie_refresh_prompt_title": "Osvezi YouTube cookies",
        "cookie_refresh_prompt_message": "YouTube zahteva nove prijavne cookies ali bot potrditev. Ali zelis zdaj osveziti cookies iz izbranega browserja in poskusiti znova?",
        "cookie_profile_attempt_failed": "{profile}: {error}",
        "cookie_all_profiles_failed": "Noben browser profil ni deloval. Zapri browser v celoti ali se prijavi v YouTube in poskusi znova.",
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
        "youtube_auth_hint": "YouTube zahteva prijavo ali bot potrditev. V nastavitvah izberi Cookies from browser, na primer Brave, da ApricotPlayer lahko sam osvezi cookies.txt.",
        "cookie_copy_hint": "ApricotPlayer ne more prebrati Chrome cookie baze. Izberi pravi browser in profil v nastavitvah, potem uporabi Export browser cookies; ApricotPlayer bo po potrebi zaprl browser in poskusil vse profile.",
        "favorite_added": "Dodano med priljubljene.",
        "favorite_exists": "Ta element je že med priljubljenimi.",
        "favorite_removed": "Odstranjeno iz priljubljenih.",
        "settings_saved": "Nastavitve shranjene.",
        "shortcut_open_main_menu": "Odpri glavni meni",
        "shortcut_open_search": "Odpri iskanje YouTube",
        "shortcut_open_direct_link": "Odpri neposredno povezavo",
        "shortcut_open_favorites": "Odpri priljubljene",
        "shortcut_open_playlists": "Odpri playliste",
        "shortcut_open_subscriptions": "Odpri narocnine",
        "shortcut_open_current_downloads": "Odpri trenutne prenose",
        "shortcut_open_history": "Odpri zgodovino",
        "shortcut_open_podcasts_rss": "Odpri podkaste in RSS",
        "shortcut_open_settings": "Odpri nastavitve",
        "shortcut_background_play_pause": "Predvajanje v ozadju: predvajaj ali pavza",
        "shortcut_download_audio": "Prenesi zvok",
        "shortcut_download_video": "Prenesi video",
        "shortcut_subscribe_channel": "Naroci se na kanal",
        "shortcut_unsubscribe_channel": "Odjavi se od kanala",
        "shortcut_open_channel": "Odpri kanal videa",
        "no_channel": "Kanal ni na voljo.",
        "shortcut_queue_audio": "Oznaci video za prenos ali dodajanje v playliste",
        "shortcut_create_playlist": "Ustvari playlisto",
        "shortcut_add_to_playlist": "Dodaj v playlisto",
        "shortcut_remove_from_playlist": "Odstrani iz playliste",
        "shortcut_copy_link": "Kopiraj povezavo",
        "shortcut_copy_stream_url": "Kopiraj direktni media URL",
        "shortcut_context_menu": "Kontekstni meni",
        "shortcut_open_selected": "Odpri izbrano",
        "shortcut_new_subscription_videos": "Center obvestil",
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
        "download_all_as_audio": "Download all as audio",
        "download_all_as_video": "Download all as video",
        "download_all_selected": "Download all selected items",
        "queued_videos_for_download": "Queued videos for download",
        "queued_downloads": "Queued videos for download",
        "current_downloads": "Current downloads",
        "no_queued_downloads": "No queued downloads.",
        "queued_download_instructions": "Press Enter to choose audio or video. You can also use Ctrl+Shift+A for audio, Ctrl+Shift+D for video, or the context menu.",
        "select_download_format": "Choose download format",
        "select_download_format_message": "Choose whether to download this item as audio or video.",
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
        "rss_refresh_interval": "Podcast and RSS refresh interval",
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
        "shortcut_actions": "Shortcut actions",
        "shortcut_value": "Shortcut for selected action",
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
        "selected_for_download_or_playlist": "Selected for download or playlist: {title}",
        "audio_selected_download": "Audio download queued: {title}",
        "video_selected_download": "Video download queued: {title}",
        "collection_audio_selected_download": "Collection audio download queued: {title}",
        "collection_video_selected_download": "Collection video download queued: {title}",
        "download_deselected": "Removed from download queue: {title}",
        "download_queue_empty": "Download queue is empty.",
        "selected_queued_marker": "selected",
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
        "reset_all_settings": "Reset all settings",
        "reset_settings_for_section": "Reset settings for {section}",
        "section_settings_reset": "{section} settings reset.",
        "defaults_restored": "Default settings restored.",
        "loading_more_results": "Loading more results.",
        "no_more_results": "No more results.",
        "auto_update_app": "Check for updates at startup",
        "app_update_interval": "App update check interval",
        "app_update_notifications": "Windows notification when an app update is available",
        "app_update_menu_item": "Update available: {version}",
        "app_update_ready_status": "App update {version} is available.",
        "app_update_notification_message": "ApricotPlayer {version} is available.",
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
        "open_channel": "Open channel",
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
        "no_channel": "Channel is not available.",
        "open_channel_videos": "Open channel videos",
        "open_playlist_videos": "Open playlist videos",
        "subscription_last_checked": "last checked {time}",
        "subscription_never_checked": "never checked",
        "subscription_notifications": "Windows notifications for new subscription videos",
        "windows_notifications": "Windows notifications",
        "download_notifications": "Windows notifications for completed downloads when ApricotPlayer is not focused",
        "notification_download_title": "Download complete",
        "subscription_check_enabled": "Check subscriptions automatically",
        "subscription_check_interval": "Subscription check interval",
        "close_to_tray": "Close button or Alt+F4 sends ApricotPlayer to system tray",
        "start_with_windows": "Start ApricotPlayer at Windows startup",
        "startup_registration_failed": "Could not update Windows startup setting: {error}",
        "tray_notification": "Windows notification when ApricotPlayer goes to the system tray",
        "tray_still_running": "ApricotPlayer is still running in the system tray.",
        "interval_30_minutes": "30 minutes",
        "interval_1_hour": "1 hour",
        "interval_hours": "{hours} hours",
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
        "audio_quality": "Audio quality",
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
        "choose_cookies_file": "Choose cookies.txt file",
        "cookies_from_browser": "Cookies from browser",
        "cookies_browser_profile": "Browser profile",
        "browser_profile_auto": "Auto - try all profiles",
        "export_browser_cookies": "Export browser cookies to cookies.txt",
        "exporting_browser_cookies": "Exporting browser cookies.",
        "browser_cookies_exported": "Browser cookies exported to {path} from {profile}.",
        "browser_cookies_export_failed": "Browser cookies export failed: {error}",
        "browser_cookies_no_youtube": "Cookies were exported, but no YouTube login cookies were found. Open YouTube in the selected browser, sign in, then try again.",
        "cookie_auto_refresh_start": "YouTube needs sign-in cookies. Refreshing cookies from {browser}.",
        "cookie_auto_refresh_done": "Cookies refreshed from {profile}. Trying again.",
        "cookie_auto_refresh_failed": "Cookies could not be refreshed: {error}",
        "cookie_refresh_prompt_title": "Refresh YouTube cookies",
        "cookie_refresh_prompt_message": "YouTube needs fresh sign-in cookies or bot confirmation. Do you want to refresh cookies from the selected browser now and try again?",
        "cookie_profile_attempt_failed": "{profile}: {error}",
        "cookie_all_profiles_failed": "No browser profile worked. Close the browser completely or sign in to YouTube, then try again.",
        "select_cookies_browser": "Choose a browser in Cookies from browser first.",
        "close_browser_for_cookie_export_title": "Close browser to export cookies",
        "close_browser_for_cookie_export_message": "{browser} is open. ApricotPlayer can close it and then export cookies, which usually fixes the Chrome cookie database error. Save anything open in the browser, then choose Yes. Continue?",
        "browser_closed_for_cookie_export": "Browser closed. Exporting cookies.",
        "cookies_file_selected": "Cookies file selected: {path}",
        "cookies_file_login_found": "YouTube login cookies found in selected cookies file.",
        "cookies_file_no_login_warning": "The selected cookies file does not appear to contain YouTube login cookies. ApricotPlayer will still use it, but YouTube may keep asking you to sign in.",
        "cookies_file_load_failed": "Could not read selected cookies file: {error}",
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
        "youtube_auth_hint": "YouTube asks for sign-in or bot confirmation. Open Settings and choose Cookies from browser, for example Brave, so ApricotPlayer can refresh cookies.txt automatically.",
        "cookie_copy_hint": "ApricotPlayer could not read the Chrome cookie database. Choose the correct browser and profile in Settings, then use Export browser cookies; ApricotPlayer will close the browser if needed and try all profiles.",
        "favorite_added": "Added to favorites.",
        "favorite_exists": "This item is already in favorites.",
        "favorite_removed": "Removed from favorites.",
        "settings_saved": "Settings saved.",
        "shortcut_open_main_menu": "Open main menu",
        "shortcut_open_search": "Open YouTube search",
        "shortcut_open_direct_link": "Open direct link",
        "shortcut_open_favorites": "Open favorites",
        "shortcut_open_playlists": "Open playlists",
        "shortcut_open_subscriptions": "Open subscriptions",
        "shortcut_open_current_downloads": "Open current downloads",
        "shortcut_open_history": "Open history",
        "shortcut_open_podcasts_rss": "Open podcasts and RSS",
        "shortcut_open_settings": "Open settings",
        "shortcut_background_play_pause": "Background playback: play or pause",
        "shortcut_download_audio": "Download audio",
        "shortcut_download_video": "Download video",
        "shortcut_subscribe_channel": "Subscribe to channel",
        "shortcut_unsubscribe_channel": "Unsubscribe from channel",
        "shortcut_open_channel": "Open video's channel",
        "shortcut_queue_audio": "Select video for download or adding to playlists",
        "shortcut_copy_link": "Copy link",
        "shortcut_context_menu": "Context menu",
        "shortcut_open_selected": "Open selected item",
        "shortcut_new_subscription_videos": "Notification center",
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
        "trending": "Trending",
        "trending_country": "Trending country",
        "trending_category": "Trending category",
        "load_trending": "Load trending",
        "loading_trending": "Loading trending: {query}",
        "trending_all": "All",
        "trending_music": "Music",
        "trending_movies": "Movies",
        "trending_gaming": "Gaming",
        "trending_sports": "Sports",
        "trending_news": "News",
        "trending_podcasts": "Podcasts",
        "trending_technology": "Technology",
        "trending_comedy": "Comedy",
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
        "channel_options": "Channel options",
        "channel_home": "Home",
        "channel_videos": "Videos",
        "channel_popular": "Popular videos",
        "channel_playlists": "Channel playlists",
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
        "resume_playback": "Resume where you left off",
        "player": "Player",
        "background_player": "Player",
        "background_player_now_playing": "Player: {title}",
        "background_player_hint": "Playback continues in the background.",
        "open_player": "Open player",
        "close_player": "Close player",
        "player_closed": "Player closed.",
        "enable_age_restricted_videos": "Age-restricted YouTube video support (slower fallback only when needed)",
        "default_audio_device": "Default audio output device",
        "audio_device_missing": "The saved audio output device is no longer available. Choose a new default device.",
        "output_devices": "Audio output devices",
        "select_output_device": "Select audio output device",
        "output_device_set": "Audio output device set to {device}.",
        "no_output_devices": "No audio output devices were found.",
        "repeat": "Repeat",
        "repeat_on": "Repeat on.",
        "repeat_off": "Repeat off.",
        "bass_boost": "Bass boost",
        "bass_boost_on": "Bass boost on.",
        "bass_boost_off": "Bass boost off.",
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
        "reset_all_settings": "Resetiraj vse nastavitve",
        "reset_settings_for_section": "Resetiraj nastavitve za {section}",
        "section_settings_reset": "Nastavitve za {section} so resetirane.",
        "cookies_file_login_found": "YouTube prijavni piškotki so najdeni v izbrani cookies datoteki.",
        "cookies_file_no_login_warning": "Izbrana cookies datoteka očitno nima YouTube prijavnih piškotkov. ApricotPlayer jo bo vseeno uporabil, ampak YouTube lahko še vedno zahteva prijavo.",
        "cookies_file_load_failed": "Izbrane cookies datoteke ni bilo mogoče prebrati: {error}",
        "start_with_windows": "Zaženi ApricotPlayer ob zagonu Windows",
        "startup_registration_failed": "Nastavitve zagona z Windows ni bilo mogoče posodobiti: {error}",
        "tray_settings": "Nastavitve",
        "rss_refresh_interval": "Interval osvezevanja podcastov in RSS",
        "subscription_check_interval": "Interval preverjanja narocnin",
        "interval_30_minutes": "30 minut",
        "interval_1_hour": "1 ura",
        "interval_hours": "{hours} ur",
        "shortcut_actions": "Akcije bliznjic",
        "shortcut_value": "Bliznjica za izbrano akcijo",
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
COOKIE_TRANSLATION_UPDATES = {
    "sl": {
        "cookies_browser_profile": "Browser profil",
        "browser_profile_auto": "Auto - poskusi vse profile",
        "browser_cookies_exported": "Piškotki brskalnika so izvoženi v {path} iz profila {profile}.",
        "browser_cookies_no_youtube": "Piškotki so izvoženi, vendar ni bilo najdenih YouTube prijavnih piškotkov. Odpri YouTube v izbranem brskalniku, se prijavi, potem poskusi znova.",
        "cookie_auto_refresh_start": "YouTube zahteva prijavne piškotke. Osvežujem piškotke iz {browser}.",
        "cookie_auto_refresh_done": "Piškotki so osveženi iz profila {profile}. Poskušam znova.",
        "cookie_auto_refresh_failed": "Piškotkov ni bilo mogoče osvežiti: {error}",
        "cookie_refresh_prompt_title": "Osveži YouTube piškotke",
        "cookie_refresh_prompt_message": "YouTube zahteva sveže prijavne piškotke ali bot potrditev. Ali želiš zdaj osvežiti piškotke iz izbranega brskalnika in poskusiti znova?",
        "cookie_profile_attempt_failed": "{profile}: {error}",
        "cookie_all_profiles_failed": "Noben profil brskalnika ni deloval. Popolnoma zapri brskalnik ali se prijavi v YouTube in poskusi znova.",
        "youtube_auth_hint": "YouTube zahteva prijavo ali bot potrditev. V nastavitvah izberi Piškotki iz brskalnika, na primer Brave, da ApricotPlayer lahko sam osveži cookies.txt.",
        "cookie_copy_hint": "ApricotPlayer ne more prebrati Chromove baze piškotkov. Izberi pravi brskalnik in profil v nastavitvah, potem uporabi Izvozi piškotke brskalnika; ApricotPlayer bo po potrebi zaprl brskalnik in poskusil vse profile.",
    },
    "en": {
        "cookies_browser_profile": "Browser profile",
        "browser_profile_auto": "Auto - try all profiles",
        "browser_cookies_exported": "Browser cookies exported to {path} from {profile}.",
        "browser_cookies_no_youtube": "Cookies were exported, but no YouTube login cookies were found. Open YouTube in the selected browser, sign in, then try again.",
        "cookie_auto_refresh_start": "YouTube needs sign-in cookies. Refreshing cookies from {browser}.",
        "cookie_auto_refresh_done": "Cookies refreshed from {profile}. Trying again.",
        "cookie_auto_refresh_failed": "Cookies could not be refreshed: {error}",
        "cookie_refresh_prompt_title": "Refresh YouTube cookies",
        "cookie_refresh_prompt_message": "YouTube needs fresh sign-in cookies or bot confirmation. Do you want to refresh cookies from the selected browser now and try again?",
        "cookie_profile_attempt_failed": "{profile}: {error}",
        "cookie_all_profiles_failed": "No browser profile worked. Close the browser completely or sign in to YouTube, then try again.",
        "youtube_auth_hint": "YouTube asks for sign-in or bot confirmation. Open Settings and choose Cookies from browser, for example Brave, so ApricotPlayer can refresh cookies.txt automatically.",
        "cookie_copy_hint": "ApricotPlayer could not read the Chrome cookie database. Choose the correct browser and profile in Settings, then use Export browser cookies; ApricotPlayer will close the browser if needed and try all profiles.",
    },
}
for language_code in LANGUAGE_CODES:
    SUPPLEMENTAL_TRANSLATIONS.setdefault(language_code, {}).update(COOKIE_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"])
for language_code, translations in SUPPLEMENTAL_TRANSLATIONS.items():
    TEXT.setdefault(language_code, {}).update(translations)
for language_code in LANGUAGE_CODES:
    TEXT[language_code] = {**TEXT["en"], **TEXT.get(language_code, {})}
MEDIA_PLAYER_TRANSLATION_UPDATES = {
    "sl": {
        "equalizer_section": "Equalizer",
        "equalizer": "Equalizer",
        "play_from_folder": "Predvajaj iz mape",
        "announce_play_pause": "Najavi play/pause v predvajalniku",
        "playback_paused": "Pavzirano.",
        "playback_playing": "Predvajam.",
        "select_media_file": "Izberi audio ali video datoteko",
        "media_files": "Audio in video datoteke",
        "all_files": "Vse datoteke",
        "global_equalizer": "Globalni equalizer",
        "equalizer_preset": "Equalizer preset",
        "equalizer_preset_name": "Ime custom preseta",
        "equalizer_db_range": "Razpon equalizerja v dB",
        "equalizer_band_gain": "Equalizer {band}",
        "reset_equalizer": "Resetiraj ta preset",
        "equalizer_saved": "Equalizer shranjen.",
        "equalizer_closed": "Equalizer zaprt.",
        "equalizer_apply_failed": "Equalizer ni bil uporabljen: {error}",
        "eq_preset_flat": "Default / flat",
        "eq_preset_bass_boost": "Bass boost",
        "eq_preset_full_bass_treble": "Full bass and treble",
        "eq_preset_dance": "Dance",
        "eq_preset_hip_hop": "Hip-hop",
        "eq_preset_electronic": "Electronic",
        "eq_preset_rock": "Rock",
        "eq_preset_pop": "Pop",
        "eq_preset_classical": "Classical",
        "eq_preset_jazz": "Jazz",
        "eq_preset_acoustic": "Acoustic",
        "eq_preset_vocal": "Vocal clarity",
        "eq_preset_podcast": "Podcast / speech",
        "eq_preset_bright": "Bright",
        "eq_preset_mellow": "Mellow",
        "eq_preset_treble_boost": "Treble boost",
        "eq_preset_laptop_headphones": "Laptop/headphones",
        "eq_preset_late_night": "Late night",
        "set_default_player": "Nastavi ApricotPlayer kot privzeti predvajalnik",
        "default_player_settings_opened": "Odprte so Windows nastavitve za privzete aplikacije.",
        "default_player_settings_failed": "Windows nastavitev za privzete aplikacije ni bilo mogoce odpreti: {error}",
        "media_association_prompt_title": "Registracija media playerja",
        "media_association_prompt_message": "ApricotPlayer se ni registriran kot media player za Windows. Ali ga zelis dodati med programe za audio in video datoteke? Privzeti predvajalnik se vedno izberes rocno v Windows nastavitvah.",
        "media_association_registered": "ApricotPlayer je registriran kot media player.",
        "media_association_failed": "Registracija media playerja ni uspela: {error}",
        "local_media": "Lokalna media datoteka",
        "local_file_open_failed": "Datoteke ni bilo mogoce odpreti: {error}",
        "clip_start_marker_set": "Start marker nastavljen na {time}.",
        "clip_end_marker_set": "End marker nastavljen na {time}.",
        "clip_markers_missing": "Najprej nastavi start in end marker.",
        "clip_marker_invalid": "End marker mora biti za start markerjem.",
        "clip_export_started": "Export clip started.",
        "clip_export_done": "Clip exported: {title}",
        "clip_export_failed": "Clip export failed: {error}",
        "shortcut_player_equalizer": "Predvajalnik: equalizer",
        "shortcut_player_marker_start": "Predvajalnik: nastavi start marker",
        "shortcut_player_marker_end": "Predvajalnik: nastavi end marker",
        "shortcut_player_export_clip": "Predvajalnik: export oznacenega dela",
        "shortcut_open_play_from_folder": "Odpri: predvajaj iz mape",
    },
    "en": {
        "equalizer_section": "Equalizer",
        "equalizer": "Equalizer",
        "play_from_folder": "Play from folder",
        "announce_play_pause": "Announce play/pause in the player",
        "playback_paused": "Paused.",
        "playback_playing": "Playing.",
        "select_media_file": "Choose an audio or video file",
        "media_files": "Audio and video files",
        "all_files": "All files",
        "global_equalizer": "Global equalizer",
        "equalizer_preset": "Equalizer preset",
        "equalizer_preset_name": "Custom preset name",
        "equalizer_db_range": "Equalizer range in dB",
        "equalizer_band_gain": "Equalizer {band}",
        "reset_equalizer": "Reset this preset",
        "equalizer_saved": "Equalizer saved.",
        "equalizer_closed": "Equalizer closed.",
        "equalizer_apply_failed": "Equalizer could not be applied: {error}",
        "eq_preset_flat": "Default / flat",
        "eq_preset_bass_boost": "Bass boost",
        "eq_preset_full_bass_treble": "Full bass and treble",
        "eq_preset_dance": "Dance",
        "eq_preset_hip_hop": "Hip-hop",
        "eq_preset_electronic": "Electronic",
        "eq_preset_rock": "Rock",
        "eq_preset_pop": "Pop",
        "eq_preset_classical": "Classical",
        "eq_preset_jazz": "Jazz",
        "eq_preset_acoustic": "Acoustic",
        "eq_preset_vocal": "Vocal clarity",
        "eq_preset_podcast": "Podcast / speech",
        "eq_preset_bright": "Bright",
        "eq_preset_mellow": "Mellow",
        "eq_preset_treble_boost": "Treble boost",
        "eq_preset_laptop_headphones": "Laptop/headphones",
        "eq_preset_late_night": "Late night",
        "set_default_player": "Set ApricotPlayer as default media player",
        "default_player_settings_opened": "Windows Default apps settings opened.",
        "default_player_settings_failed": "Could not open Windows Default apps settings: {error}",
        "media_association_prompt_title": "Media player registration",
        "media_association_prompt_message": "ApricotPlayer is not registered as a Windows media player yet. Add it as an option for audio and video files? You still choose the default player manually in Windows settings.",
        "media_association_registered": "ApricotPlayer is registered as a media player.",
        "media_association_failed": "Media player registration failed: {error}",
        "local_media": "Local media file",
        "local_file_open_failed": "Could not open file: {error}",
        "clip_start_marker_set": "Start marker set at {time}.",
        "clip_end_marker_set": "End marker set at {time}.",
        "clip_markers_missing": "Set a start marker and end marker first.",
        "clip_marker_invalid": "The end marker must be after the start marker.",
        "clip_export_started": "Clip export started.",
        "clip_export_done": "Clip exported: {title}",
        "clip_export_failed": "Clip export failed: {error}",
        "shortcut_player_equalizer": "Player: equalizer",
        "shortcut_player_marker_start": "Player: set start marker",
        "shortcut_player_marker_end": "Player: set end marker",
        "shortcut_player_export_clip": "Player: export marked clip",
        "shortcut_open_play_from_folder": "Open: play from folder",
    },
}
MEDIA_PLAYER_TRANSLATION_UPDATES.update(
    {
        "de": {
            "play_from_folder": "Aus Ordner abspielen",
            "announce_play_pause": "Wiedergabe/Pause im Player ansagen",
            "playback_paused": "Pausiert.",
            "playback_playing": "Wiedergabe.",
            "select_media_file": "Audio- oder Videodatei auswählen",
            "media_files": "Audio- und Videodateien",
            "all_files": "Alle Dateien",
            "global_equalizer": "Globaler Equalizer",
            "equalizer_preset": "Equalizer-Voreinstellung",
            "equalizer_preset_name": "Name der eigenen Voreinstellung",
            "equalizer_db_range": "Equalizer-Bereich in dB",
            "equalizer_band_gain": "Equalizer {band}",
            "reset_equalizer": "Diese Voreinstellung zurücksetzen",
            "equalizer_saved": "Equalizer gespeichert.",
            "equalizer_closed": "Equalizer geschlossen.",
            "equalizer_apply_failed": "Equalizer konnte nicht angewendet werden: {error}",
            "set_default_player": "ApricotPlayer als Standard-Mediaplayer festlegen",
            "default_player_settings_opened": "Windows-Einstellungen für Standard-Apps geöffnet.",
            "default_player_settings_failed": "Windows-Einstellungen für Standard-Apps konnten nicht geöffnet werden: {error}",
            "media_association_prompt_title": "Mediaplayer-Registrierung",
            "media_association_prompt_message": "ApricotPlayer ist noch nicht als Windows-Mediaplayer registriert. Als Option für Audio- und Videodateien hinzufügen? Den Standardplayer wählst du weiterhin manuell in Windows.",
            "media_association_registered": "ApricotPlayer ist als Mediaplayer registriert.",
            "media_association_failed": "Mediaplayer-Registrierung fehlgeschlagen: {error}",
            "local_media": "Lokale Mediendatei",
            "local_file_open_failed": "Datei konnte nicht geöffnet werden: {error}",
            "clip_start_marker_set": "Startmarke bei {time} gesetzt.",
            "clip_end_marker_set": "Endmarke bei {time} gesetzt.",
            "clip_markers_missing": "Setze zuerst eine Start- und Endmarke.",
            "clip_marker_invalid": "Die Endmarke muss nach der Startmarke liegen.",
            "clip_export_started": "Clip-Export gestartet.",
            "clip_export_done": "Clip exportiert: {title}",
            "clip_export_failed": "Clip-Export fehlgeschlagen: {error}",
            "shortcut_player_equalizer": "Player: Equalizer",
            "shortcut_player_marker_start": "Player: Startmarke setzen",
            "shortcut_player_marker_end": "Player: Endmarke setzen",
            "shortcut_player_export_clip": "Player: markierten Clip exportieren",
            "shortcut_open_play_from_folder": "Öffnen: Aus Ordner abspielen",
            "bass_boost": "Bassverstärkung",
            "bass_boost_on": "Bassverstärkung ein.",
            "bass_boost_off": "Bassverstärkung aus.",
        },
        "fr": {
            "play_from_folder": "Lire depuis un dossier",
            "announce_play_pause": "Annoncer lecture/pause dans le lecteur",
            "playback_paused": "En pause.",
            "playback_playing": "Lecture.",
            "select_media_file": "Choisir un fichier audio ou vidéo",
            "media_files": "Fichiers audio et vidéo",
            "all_files": "Tous les fichiers",
            "global_equalizer": "Égaliseur global",
            "equalizer_preset": "Préréglage de l’égaliseur",
            "equalizer_preset_name": "Nom du préréglage personnalisé",
            "equalizer_db_range": "Plage de l’égaliseur en dB",
            "equalizer_band_gain": "Égaliseur {band}",
            "reset_equalizer": "Réinitialiser ce préréglage",
            "equalizer_saved": "Égaliseur enregistré.",
            "equalizer_closed": "Égaliseur fermé.",
            "equalizer_apply_failed": "Impossible d’appliquer l’égaliseur : {error}",
            "set_default_player": "Définir ApricotPlayer comme lecteur multimédia par défaut",
            "default_player_settings_opened": "Paramètres Windows des applications par défaut ouverts.",
            "default_player_settings_failed": "Impossible d’ouvrir les paramètres Windows des applications par défaut : {error}",
            "media_association_prompt_title": "Enregistrement du lecteur multimédia",
            "media_association_prompt_message": "ApricotPlayer n’est pas encore enregistré comme lecteur multimédia Windows. L’ajouter comme option pour les fichiers audio et vidéo ? Le lecteur par défaut se choisit toujours manuellement dans Windows.",
            "media_association_registered": "ApricotPlayer est enregistré comme lecteur multimédia.",
            "media_association_failed": "Échec de l’enregistrement du lecteur multimédia : {error}",
            "local_media": "Fichier multimédia local",
            "local_file_open_failed": "Impossible d’ouvrir le fichier : {error}",
            "clip_start_marker_set": "Marqueur de début placé à {time}.",
            "clip_end_marker_set": "Marqueur de fin placé à {time}.",
            "clip_markers_missing": "Placez d’abord un marqueur de début et de fin.",
            "clip_marker_invalid": "Le marqueur de fin doit être après le marqueur de début.",
            "clip_export_started": "Export du clip démarré.",
            "clip_export_done": "Clip exporté : {title}",
            "clip_export_failed": "Échec de l’export du clip : {error}",
            "shortcut_player_equalizer": "Lecteur : égaliseur",
            "shortcut_player_marker_start": "Lecteur : définir le marqueur de début",
            "shortcut_player_marker_end": "Lecteur : définir le marqueur de fin",
            "shortcut_player_export_clip": "Lecteur : exporter le clip marqué",
            "shortcut_open_play_from_folder": "Ouvrir : lire depuis un dossier",
            "bass_boost": "Amplification des basses",
            "bass_boost_on": "Amplification des basses activée.",
            "bass_boost_off": "Amplification des basses désactivée.",
        },
        "es": {
            "play_from_folder": "Reproducir desde carpeta",
            "announce_play_pause": "Anunciar reproducir/pausa en el reproductor",
            "playback_paused": "Pausado.",
            "playback_playing": "Reproduciendo.",
            "select_media_file": "Elegir un archivo de audio o video",
            "media_files": "Archivos de audio y video",
            "all_files": "Todos los archivos",
            "global_equalizer": "Ecualizador global",
            "equalizer_preset": "Preajuste del ecualizador",
            "equalizer_preset_name": "Nombre del preajuste personalizado",
            "equalizer_db_range": "Rango del ecualizador en dB",
            "equalizer_band_gain": "Ecualizador {band}",
            "reset_equalizer": "Restablecer este preajuste",
            "equalizer_saved": "Ecualizador guardado.",
            "equalizer_closed": "Ecualizador cerrado.",
            "equalizer_apply_failed": "No se pudo aplicar el ecualizador: {error}",
            "set_default_player": "Establecer ApricotPlayer como reproductor multimedia predeterminado",
            "default_player_settings_opened": "Configuración de aplicaciones predeterminadas de Windows abierta.",
            "default_player_settings_failed": "No se pudo abrir la configuración de aplicaciones predeterminadas de Windows: {error}",
            "media_association_prompt_title": "Registro de reproductor multimedia",
            "media_association_prompt_message": "ApricotPlayer aún no está registrado como reproductor multimedia de Windows. ¿Añadirlo como opción para archivos de audio y video? El reproductor predeterminado se elige manualmente en Windows.",
            "media_association_registered": "ApricotPlayer está registrado como reproductor multimedia.",
            "media_association_failed": "Falló el registro del reproductor multimedia: {error}",
            "local_media": "Archivo multimedia local",
            "local_file_open_failed": "No se pudo abrir el archivo: {error}",
            "clip_start_marker_set": "Marcador inicial establecido en {time}.",
            "clip_end_marker_set": "Marcador final establecido en {time}.",
            "clip_markers_missing": "Primero establece un marcador inicial y final.",
            "clip_marker_invalid": "El marcador final debe estar después del marcador inicial.",
            "clip_export_started": "Exportación de clip iniciada.",
            "clip_export_done": "Clip exportado: {title}",
            "clip_export_failed": "Falló la exportación del clip: {error}",
            "shortcut_player_equalizer": "Reproductor: ecualizador",
            "shortcut_player_marker_start": "Reproductor: establecer marcador inicial",
            "shortcut_player_marker_end": "Reproductor: establecer marcador final",
            "shortcut_player_export_clip": "Reproductor: exportar clip marcado",
            "shortcut_open_play_from_folder": "Abrir: reproducir desde carpeta",
            "bass_boost": "Refuerzo de graves",
            "bass_boost_on": "Refuerzo de graves activado.",
            "bass_boost_off": "Refuerzo de graves desactivado.",
        },
        "pt": {
            "play_from_folder": "Reproduzir da pasta",
            "announce_play_pause": "Anunciar reproduzir/pausa no player",
            "playback_paused": "Pausado.",
            "playback_playing": "Reproduzindo.",
            "select_media_file": "Escolher arquivo de áudio ou vídeo",
            "media_files": "Arquivos de áudio e vídeo",
            "all_files": "Todos os arquivos",
            "global_equalizer": "Equalizador global",
            "equalizer_preset": "Predefinição do equalizador",
            "equalizer_preset_name": "Nome da predefinição personalizada",
            "equalizer_db_range": "Faixa do equalizador em dB",
            "equalizer_band_gain": "Equalizador {band}",
            "reset_equalizer": "Redefinir esta predefinição",
            "equalizer_saved": "Equalizador salvo.",
            "equalizer_closed": "Equalizador fechado.",
            "equalizer_apply_failed": "Não foi possível aplicar o equalizador: {error}",
            "set_default_player": "Definir ApricotPlayer como player de mídia padrão",
            "default_player_settings_opened": "Configurações de apps padrão do Windows abertas.",
            "default_player_settings_failed": "Não foi possível abrir as configurações de apps padrão do Windows: {error}",
            "media_association_prompt_title": "Registro do player de mídia",
            "media_association_prompt_message": "ApricotPlayer ainda não está registrado como player de mídia do Windows. Adicionar como opção para arquivos de áudio e vídeo? O player padrão ainda é escolhido manualmente no Windows.",
            "media_association_registered": "ApricotPlayer está registrado como player de mídia.",
            "media_association_failed": "Falha ao registrar o player de mídia: {error}",
            "local_media": "Arquivo de mídia local",
            "local_file_open_failed": "Não foi possível abrir o arquivo: {error}",
            "clip_start_marker_set": "Marcador inicial definido em {time}.",
            "clip_end_marker_set": "Marcador final definido em {time}.",
            "clip_markers_missing": "Defina primeiro um marcador inicial e final.",
            "clip_marker_invalid": "O marcador final deve ficar depois do marcador inicial.",
            "clip_export_started": "Exportação do clipe iniciada.",
            "clip_export_done": "Clipe exportado: {title}",
            "clip_export_failed": "Falha ao exportar o clipe: {error}",
            "shortcut_player_equalizer": "Player: equalizador",
            "shortcut_player_marker_start": "Player: definir marcador inicial",
            "shortcut_player_marker_end": "Player: definir marcador final",
            "shortcut_player_export_clip": "Player: exportar clipe marcado",
            "shortcut_open_play_from_folder": "Abrir: reproduzir da pasta",
            "bass_boost": "Reforço de graves",
            "bass_boost_on": "Reforço de graves ativado.",
            "bass_boost_off": "Reforço de graves desativado.",
        },
        "it": {
            "play_from_folder": "Riproduci da cartella",
            "announce_play_pause": "Annuncia riproduzione/pausa nel player",
            "playback_paused": "In pausa.",
            "playback_playing": "Riproduzione.",
            "select_media_file": "Scegli un file audio o video",
            "media_files": "File audio e video",
            "all_files": "Tutti i file",
            "global_equalizer": "Equalizzatore globale",
            "equalizer_preset": "Preset equalizzatore",
            "equalizer_preset_name": "Nome preset personalizzato",
            "equalizer_db_range": "Intervallo equalizzatore in dB",
            "equalizer_band_gain": "Equalizzatore {band}",
            "reset_equalizer": "Reimposta questo preset",
            "equalizer_saved": "Equalizzatore salvato.",
            "equalizer_closed": "Equalizzatore chiuso.",
            "equalizer_apply_failed": "Impossibile applicare l’equalizzatore: {error}",
            "set_default_player": "Imposta ApricotPlayer come player multimediale predefinito",
            "default_player_settings_opened": "Impostazioni app predefinite di Windows aperte.",
            "default_player_settings_failed": "Impossibile aprire le impostazioni app predefinite di Windows: {error}",
            "media_association_prompt_title": "Registrazione player multimediale",
            "media_association_prompt_message": "ApricotPlayer non è ancora registrato come player multimediale di Windows. Aggiungerlo come opzione per file audio e video? Il player predefinito si sceglie comunque manualmente in Windows.",
            "media_association_registered": "ApricotPlayer è registrato come player multimediale.",
            "media_association_failed": "Registrazione player multimediale non riuscita: {error}",
            "local_media": "File multimediale locale",
            "local_file_open_failed": "Impossibile aprire il file: {error}",
            "clip_start_marker_set": "Marcatore iniziale impostato a {time}.",
            "clip_end_marker_set": "Marcatore finale impostato a {time}.",
            "clip_markers_missing": "Imposta prima un marcatore iniziale e finale.",
            "clip_marker_invalid": "Il marcatore finale deve essere dopo quello iniziale.",
            "clip_export_started": "Esportazione clip avviata.",
            "clip_export_done": "Clip esportata: {title}",
            "clip_export_failed": "Esportazione clip non riuscita: {error}",
            "shortcut_player_equalizer": "Player: equalizzatore",
            "shortcut_player_marker_start": "Player: imposta marcatore iniziale",
            "shortcut_player_marker_end": "Player: imposta marcatore finale",
            "shortcut_player_export_clip": "Player: esporta clip marcata",
            "shortcut_open_play_from_folder": "Apri: riproduci da cartella",
            "bass_boost": "Potenziamento bassi",
            "bass_boost_on": "Potenziamento bassi attivo.",
            "bass_boost_off": "Potenziamento bassi disattivo.",
        },
        "pl": {
            "play_from_folder": "Odtwórz z folderu",
            "announce_play_pause": "Ogłaszaj odtwarzanie/pauzę w odtwarzaczu",
            "playback_paused": "Wstrzymano.",
            "playback_playing": "Odtwarzanie.",
            "select_media_file": "Wybierz plik audio lub wideo",
            "media_files": "Pliki audio i wideo",
            "all_files": "Wszystkie pliki",
            "global_equalizer": "Globalny korektor",
            "equalizer_preset": "Preset korektora",
            "equalizer_preset_name": "Nazwa własnego presetu",
            "equalizer_db_range": "Zakres korektora w dB",
            "equalizer_band_gain": "Korektor {band}",
            "reset_equalizer": "Resetuj ten preset",
            "equalizer_saved": "Korektor zapisany.",
            "equalizer_closed": "Korektor zamknięty.",
            "equalizer_apply_failed": "Nie można zastosować korektora: {error}",
            "set_default_player": "Ustaw ApricotPlayer jako domyślny odtwarzacz multimediów",
            "default_player_settings_opened": "Otwarto ustawienia aplikacji domyślnych Windows.",
            "default_player_settings_failed": "Nie można otworzyć ustawień aplikacji domyślnych Windows: {error}",
            "media_association_prompt_title": "Rejestracja odtwarzacza multimediów",
            "media_association_prompt_message": "ApricotPlayer nie jest jeszcze zarejestrowany jako odtwarzacz multimediów Windows. Dodać go jako opcję dla plików audio i wideo? Domyślny odtwarzacz nadal wybierasz ręcznie w Windows.",
            "media_association_registered": "ApricotPlayer jest zarejestrowany jako odtwarzacz multimediów.",
            "media_association_failed": "Rejestracja odtwarzacza multimediów nie powiodła się: {error}",
            "local_media": "Lokalny plik multimedialny",
            "local_file_open_failed": "Nie można otworzyć pliku: {error}",
            "clip_start_marker_set": "Znacznik początku ustawiony na {time}.",
            "clip_end_marker_set": "Znacznik końca ustawiony na {time}.",
            "clip_markers_missing": "Najpierw ustaw znacznik początku i końca.",
            "clip_marker_invalid": "Znacznik końca musi być po znaczniku początku.",
            "clip_export_started": "Rozpoczęto eksport klipu.",
            "clip_export_done": "Klip wyeksportowany: {title}",
            "clip_export_failed": "Eksport klipu nie powiódł się: {error}",
            "shortcut_player_equalizer": "Odtwarzacz: korektor",
            "shortcut_player_marker_start": "Odtwarzacz: ustaw znacznik początku",
            "shortcut_player_marker_end": "Odtwarzacz: ustaw znacznik końca",
            "shortcut_player_export_clip": "Odtwarzacz: eksportuj zaznaczony klip",
            "shortcut_open_play_from_folder": "Otwórz: odtwórz z folderu",
            "bass_boost": "Wzmocnienie basu",
            "bass_boost_on": "Wzmocnienie basu włączone.",
            "bass_boost_off": "Wzmocnienie basu wyłączone.",
        },
        "nl": {
            "play_from_folder": "Afspelen uit map",
            "announce_play_pause": "Afspelen/pauze aankondigen in de speler",
            "playback_paused": "Gepauzeerd.",
            "playback_playing": "Afspelen.",
            "select_media_file": "Kies een audio- of videobestand",
            "media_files": "Audio- en videobestanden",
            "all_files": "Alle bestanden",
            "global_equalizer": "Globale equalizer",
            "equalizer_preset": "Equalizerpreset",
            "equalizer_preset_name": "Naam van aangepaste preset",
            "equalizer_db_range": "Equalizerbereik in dB",
            "equalizer_band_gain": "Equalizer {band}",
            "reset_equalizer": "Deze preset herstellen",
            "equalizer_saved": "Equalizer opgeslagen.",
            "equalizer_closed": "Equalizer gesloten.",
            "equalizer_apply_failed": "Equalizer kon niet worden toegepast: {error}",
            "set_default_player": "ApricotPlayer instellen als standaard mediaspeler",
            "default_player_settings_opened": "Windows-instellingen voor standaardapps geopend.",
            "default_player_settings_failed": "Windows-instellingen voor standaardapps konden niet worden geopend: {error}",
            "media_association_prompt_title": "Registratie als mediaspeler",
            "media_association_prompt_message": "ApricotPlayer is nog niet geregistreerd als Windows-mediaspeler. Toevoegen als optie voor audio- en videobestanden? De standaardspeler kies je nog steeds handmatig in Windows.",
            "media_association_registered": "ApricotPlayer is geregistreerd als mediaspeler.",
            "media_association_failed": "Registratie als mediaspeler mislukt: {error}",
            "local_media": "Lokaal mediabestand",
            "local_file_open_failed": "Bestand kon niet worden geopend: {error}",
            "clip_start_marker_set": "Startmarkering ingesteld op {time}.",
            "clip_end_marker_set": "Eindmarkering ingesteld op {time}.",
            "clip_markers_missing": "Stel eerst een start- en eindmarkering in.",
            "clip_marker_invalid": "De eindmarkering moet na de startmarkering staan.",
            "clip_export_started": "Clip exporteren gestart.",
            "clip_export_done": "Clip geëxporteerd: {title}",
            "clip_export_failed": "Clip exporteren mislukt: {error}",
            "shortcut_player_equalizer": "Speler: equalizer",
            "shortcut_player_marker_start": "Speler: startmarkering instellen",
            "shortcut_player_marker_end": "Speler: eindmarkering instellen",
            "shortcut_player_export_clip": "Speler: gemarkeerde clip exporteren",
            "shortcut_open_play_from_folder": "Openen: afspelen uit map",
            "bass_boost": "Basversterking",
            "bass_boost_on": "Basversterking aan.",
            "bass_boost_off": "Basversterking uit.",
        },
        "sv": {
            "play_from_folder": "Spela från mapp",
            "announce_play_pause": "Annonsera spela/paus i spelaren",
            "playback_paused": "Pausad.",
            "playback_playing": "Spelar.",
            "select_media_file": "Välj en ljud- eller videofil",
            "media_files": "Ljud- och videofiler",
            "all_files": "Alla filer",
            "global_equalizer": "Global equalizer",
            "equalizer_preset": "Equalizerförinställning",
            "equalizer_preset_name": "Namn på egen förinställning",
            "equalizer_db_range": "Equalizerintervall i dB",
            "equalizer_band_gain": "Equalizer {band}",
            "reset_equalizer": "Återställ denna förinställning",
            "equalizer_saved": "Equalizer sparad.",
            "equalizer_closed": "Equalizer stängd.",
            "equalizer_apply_failed": "Equalizer kunde inte användas: {error}",
            "set_default_player": "Ange ApricotPlayer som standardmediaspelare",
            "default_player_settings_opened": "Windows-inställningar för standardappar öppnade.",
            "default_player_settings_failed": "Kunde inte öppna Windows-inställningar för standardappar: {error}",
            "media_association_prompt_title": "Registrering som mediaspelare",
            "media_association_prompt_message": "ApricotPlayer är ännu inte registrerad som Windows-mediaspelare. Lägg till den som alternativ för ljud- och videofiler? Standardspelaren väljs fortfarande manuellt i Windows.",
            "media_association_registered": "ApricotPlayer är registrerad som mediaspelare.",
            "media_association_failed": "Registrering som mediaspelare misslyckades: {error}",
            "local_media": "Lokal mediafil",
            "local_file_open_failed": "Kunde inte öppna filen: {error}",
            "clip_start_marker_set": "Startmarkör satt vid {time}.",
            "clip_end_marker_set": "Slutmarkör satt vid {time}.",
            "clip_markers_missing": "Sätt först en start- och slutmarkör.",
            "clip_marker_invalid": "Slutmarkören måste vara efter startmarkören.",
            "clip_export_started": "Clipexport startad.",
            "clip_export_done": "Clip exporterad: {title}",
            "clip_export_failed": "Clipexport misslyckades: {error}",
            "shortcut_player_equalizer": "Spelare: equalizer",
            "shortcut_player_marker_start": "Spelare: sätt startmarkör",
            "shortcut_player_marker_end": "Spelare: sätt slutmarkör",
            "shortcut_player_export_clip": "Spelare: exportera markerat klipp",
            "shortcut_open_play_from_folder": "Öppna: spela från mapp",
            "bass_boost": "Basförstärkning",
            "bass_boost_on": "Basförstärkning på.",
            "bass_boost_off": "Basförstärkning av.",
        },
        "hr": {
            "play_from_folder": "Reproduciraj iz mape",
            "announce_play_pause": "Najavi reprodukciju/pauzu u playeru",
            "playback_paused": "Pauzirano.",
            "playback_playing": "Reproduciram.",
            "select_media_file": "Odaberi audio ili video datoteku",
            "media_files": "Audio i video datoteke",
            "all_files": "Sve datoteke",
            "global_equalizer": "Globalni equalizer",
            "equalizer_preset": "Preset equalizera",
            "equalizer_preset_name": "Naziv prilagođenog preseta",
            "equalizer_db_range": "Raspon equalizera u dB",
            "equalizer_band_gain": "Equalizer {band}",
            "reset_equalizer": "Resetiraj ovaj preset",
            "equalizer_saved": "Equalizer spremljen.",
            "equalizer_closed": "Equalizer zatvoren.",
            "equalizer_apply_failed": "Equalizer nije moguće primijeniti: {error}",
            "set_default_player": "Postavi ApricotPlayer kao zadani media player",
            "default_player_settings_opened": "Otvorene su Windows postavke zadanih aplikacija.",
            "default_player_settings_failed": "Nije moguće otvoriti Windows postavke zadanih aplikacija: {error}",
            "media_association_prompt_title": "Registracija media playera",
            "media_association_prompt_message": "ApricotPlayer još nije registriran kao Windows media player. Dodati ga kao opciju za audio i video datoteke? Zadani player i dalje se bira ručno u Windowsima.",
            "media_association_registered": "ApricotPlayer je registriran kao media player.",
            "media_association_failed": "Registracija media playera nije uspjela: {error}",
            "local_media": "Lokalna media datoteka",
            "local_file_open_failed": "Datoteku nije moguće otvoriti: {error}",
            "clip_start_marker_set": "Početni marker postavljen na {time}.",
            "clip_end_marker_set": "Završni marker postavljen na {time}.",
            "clip_markers_missing": "Najprije postavi početni i završni marker.",
            "clip_marker_invalid": "Završni marker mora biti nakon početnog.",
            "clip_export_started": "Izvoz isječka započeo.",
            "clip_export_done": "Isječak izvezen: {title}",
            "clip_export_failed": "Izvoz isječka nije uspio: {error}",
            "shortcut_player_equalizer": "Player: equalizer",
            "shortcut_player_marker_start": "Player: postavi početni marker",
            "shortcut_player_marker_end": "Player: postavi završni marker",
            "shortcut_player_export_clip": "Player: izvezi označeni isječak",
            "shortcut_open_play_from_folder": "Otvori: reproduciraj iz mape",
            "bass_boost": "Pojačanje basa",
            "bass_boost_on": "Pojačanje basa uključeno.",
            "bass_boost_off": "Pojačanje basa isključeno.",
        },
        "sr": {
            "play_from_folder": "Pusti iz fascikle",
            "announce_play_pause": "Najavi reprodukciju/pauzu u plejeru",
            "playback_paused": "Pauzirano.",
            "playback_playing": "Reprodukcija.",
            "select_media_file": "Izaberi audio ili video datoteku",
            "media_files": "Audio i video datoteke",
            "all_files": "Sve datoteke",
            "global_equalizer": "Globalni ekvilajzer",
            "equalizer_preset": "Podešavanje ekvilajzera",
            "equalizer_preset_name": "Ime prilagođenog podešavanja",
            "equalizer_db_range": "Opseg ekvilajzera u dB",
            "equalizer_band_gain": "Ekvilajzer {band}",
            "reset_equalizer": "Resetuj ovo podešavanje",
            "equalizer_saved": "Ekvilajzer sačuvan.",
            "equalizer_closed": "Ekvilajzer zatvoren.",
            "equalizer_apply_failed": "Ekvilajzer nije mogao biti primenjen: {error}",
            "set_default_player": "Postavi ApricotPlayer kao podrazumevani media plejer",
            "default_player_settings_opened": "Otvorena su Windows podešavanja podrazumevanih aplikacija.",
            "default_player_settings_failed": "Nije moguće otvoriti Windows podešavanja podrazumevanih aplikacija: {error}",
            "media_association_prompt_title": "Registracija media plejera",
            "media_association_prompt_message": "ApricotPlayer još nije registrovan kao Windows media plejer. Dodati ga kao opciju za audio i video datoteke? Podrazumevani plejer se i dalje bira ručno u Windowsu.",
            "media_association_registered": "ApricotPlayer je registrovan kao media plejer.",
            "media_association_failed": "Registracija media plejera nije uspela: {error}",
            "local_media": "Lokalna media datoteka",
            "local_file_open_failed": "Datoteka nije mogla da se otvori: {error}",
            "clip_start_marker_set": "Početni marker postavljen na {time}.",
            "clip_end_marker_set": "Završni marker postavljen na {time}.",
            "clip_markers_missing": "Prvo postavi početni i završni marker.",
            "clip_marker_invalid": "Završni marker mora biti posle početnog.",
            "clip_export_started": "Izvoz klipa je počeo.",
            "clip_export_done": "Klip izvezen: {title}",
            "clip_export_failed": "Izvoz klipa nije uspeo: {error}",
            "shortcut_player_equalizer": "Plejer: ekvilajzer",
            "shortcut_player_marker_start": "Plejer: postavi početni marker",
            "shortcut_player_marker_end": "Plejer: postavi završni marker",
            "shortcut_player_export_clip": "Plejer: izvezi označeni klip",
            "shortcut_open_play_from_folder": "Otvori: pusti iz fascikle",
            "bass_boost": "Pojačanje basa",
            "bass_boost_on": "Pojačanje basa uključeno.",
            "bass_boost_off": "Pojačanje basa isključeno.",
        },
        "cs": {
            "play_from_folder": "Přehrát ze složky",
            "announce_play_pause": "Oznamovat přehrát/pozastavit v přehrávači",
            "playback_paused": "Pozastaveno.",
            "playback_playing": "Přehrávání.",
            "select_media_file": "Vyberte audio nebo video soubor",
            "media_files": "Audio a video soubory",
            "all_files": "Všechny soubory",
            "global_equalizer": "Globální ekvalizér",
            "equalizer_preset": "Předvolba ekvalizéru",
            "equalizer_preset_name": "Název vlastní předvolby",
            "equalizer_db_range": "Rozsah ekvalizéru v dB",
            "equalizer_band_gain": "Ekvalizér {band}",
            "reset_equalizer": "Resetovat tuto předvolbu",
            "equalizer_saved": "Ekvalizér uložen.",
            "equalizer_closed": "Ekvalizér zavřen.",
            "equalizer_apply_failed": "Ekvalizér se nepodařilo použít: {error}",
            "set_default_player": "Nastavit ApricotPlayer jako výchozí přehrávač médií",
            "default_player_settings_opened": "Otevřeno nastavení výchozích aplikací Windows.",
            "default_player_settings_failed": "Nepodařilo se otevřít nastavení výchozích aplikací Windows: {error}",
            "media_association_prompt_title": "Registrace přehrávače médií",
            "media_association_prompt_message": "ApricotPlayer zatím není registrován jako přehrávač médií ve Windows. Přidat ho jako možnost pro audio a video soubory? Výchozí přehrávač stále vyberete ručně ve Windows.",
            "media_association_registered": "ApricotPlayer je registrován jako přehrávač médií.",
            "media_association_failed": "Registrace přehrávače médií selhala: {error}",
            "local_media": "Místní mediální soubor",
            "local_file_open_failed": "Soubor se nepodařilo otevřít: {error}",
            "clip_start_marker_set": "Počáteční značka nastavena na {time}.",
            "clip_end_marker_set": "Koncová značka nastavena na {time}.",
            "clip_markers_missing": "Nejprve nastavte počáteční a koncovou značku.",
            "clip_marker_invalid": "Koncová značka musí být za počáteční značkou.",
            "clip_export_started": "Export klipu spuštěn.",
            "clip_export_done": "Klip exportován: {title}",
            "clip_export_failed": "Export klipu selhal: {error}",
            "shortcut_player_equalizer": "Přehrávač: ekvalizér",
            "shortcut_player_marker_start": "Přehrávač: nastavit počáteční značku",
            "shortcut_player_marker_end": "Přehrávač: nastavit koncovou značku",
            "shortcut_player_export_clip": "Přehrávač: exportovat označený klip",
            "shortcut_open_play_from_folder": "Otevřít: přehrát ze složky",
            "bass_boost": "Zesílení basů",
            "bass_boost_on": "Zesílení basů zapnuto.",
            "bass_boost_off": "Zesílení basů vypnuto.",
        },
        "sk": {
            "play_from_folder": "Prehrať z priečinka",
            "announce_play_pause": "Oznamovať prehratie/pozastavenie v prehrávači",
            "playback_paused": "Pozastavené.",
            "playback_playing": "Prehráva sa.",
            "select_media_file": "Vyberte audio alebo video súbor",
            "media_files": "Audio a video súbory",
            "all_files": "Všetky súbory",
            "global_equalizer": "Globálny ekvalizér",
            "equalizer_preset": "Predvoľba ekvalizéra",
            "equalizer_preset_name": "Názov vlastnej predvoľby",
            "equalizer_db_range": "Rozsah ekvalizéra v dB",
            "equalizer_band_gain": "Ekvalizér {band}",
            "reset_equalizer": "Resetovať túto predvoľbu",
            "equalizer_saved": "Ekvalizér uložený.",
            "equalizer_closed": "Ekvalizér zatvorený.",
            "equalizer_apply_failed": "Ekvalizér sa nepodarilo použiť: {error}",
            "set_default_player": "Nastaviť ApricotPlayer ako predvolený prehrávač médií",
            "default_player_settings_opened": "Otvorené nastavenia predvolených aplikácií Windows.",
            "default_player_settings_failed": "Nepodarilo sa otvoriť nastavenia predvolených aplikácií Windows: {error}",
            "media_association_prompt_title": "Registrácia prehrávača médií",
            "media_association_prompt_message": "ApricotPlayer ešte nie je registrovaný ako prehrávač médií vo Windows. Pridať ho ako možnosť pre audio a video súbory? Predvolený prehrávač si stále vyberáte ručne vo Windows.",
            "media_association_registered": "ApricotPlayer je registrovaný ako prehrávač médií.",
            "media_association_failed": "Registrácia prehrávača médií zlyhala: {error}",
            "local_media": "Miestny mediálny súbor",
            "local_file_open_failed": "Súbor sa nepodarilo otvoriť: {error}",
            "clip_start_marker_set": "Počiatočná značka nastavená na {time}.",
            "clip_end_marker_set": "Koncová značka nastavená na {time}.",
            "clip_markers_missing": "Najprv nastavte počiatočnú a koncovú značku.",
            "clip_marker_invalid": "Koncová značka musí byť za počiatočnou.",
            "clip_export_started": "Export klipu spustený.",
            "clip_export_done": "Klip exportovaný: {title}",
            "clip_export_failed": "Export klipu zlyhal: {error}",
            "shortcut_player_equalizer": "Prehrávač: ekvalizér",
            "shortcut_player_marker_start": "Prehrávač: nastaviť počiatočnú značku",
            "shortcut_player_marker_end": "Prehrávač: nastaviť koncovú značku",
            "shortcut_player_export_clip": "Prehrávač: exportovať označený klip",
            "shortcut_open_play_from_folder": "Otvoriť: prehrať z priečinka",
            "bass_boost": "Zosilnenie basov",
            "bass_boost_on": "Zosilnenie basov zapnuté.",
            "bass_boost_off": "Zosilnenie basov vypnuté.",
        },
        "hu": {
            "play_from_folder": "Lejátszás mappából",
            "announce_play_pause": "Lejátszás/szünet bemondása a lejátszóban",
            "playback_paused": "Szüneteltetve.",
            "playback_playing": "Lejátszás.",
            "select_media_file": "Válassz hang- vagy videofájlt",
            "media_files": "Hang- és videofájlok",
            "all_files": "Minden fájl",
            "global_equalizer": "Globális hangszínszabályzó",
            "equalizer_preset": "Hangszínszabályzó előbeállítás",
            "equalizer_preset_name": "Egyéni előbeállítás neve",
            "equalizer_db_range": "Hangszínszabályzó tartománya dB-ben",
            "equalizer_band_gain": "Hangszínszabályzó {band}",
            "reset_equalizer": "Előbeállítás visszaállítása",
            "equalizer_saved": "Hangszínszabályzó mentve.",
            "equalizer_closed": "Hangszínszabályzó bezárva.",
            "equalizer_apply_failed": "A hangszínszabályzó nem alkalmazható: {error}",
            "set_default_player": "ApricotPlayer beállítása alapértelmezett médialejátszóként",
            "default_player_settings_opened": "Windows alapértelmezett alkalmazások beállításai megnyitva.",
            "default_player_settings_failed": "Nem sikerült megnyitni a Windows alapértelmezett alkalmazásbeállításait: {error}",
            "media_association_prompt_title": "Médialejátszó regisztrációja",
            "media_association_prompt_message": "Az ApricotPlayer még nincs Windows médialejátszóként regisztrálva. Hozzáadod opcióként hang- és videofájlokhoz? Az alapértelmezett lejátszót továbbra is kézzel választod ki a Windowsban.",
            "media_association_registered": "Az ApricotPlayer médialejátszóként regisztrálva.",
            "media_association_failed": "A médialejátszó regisztrációja sikertelen: {error}",
            "local_media": "Helyi médiafájl",
            "local_file_open_failed": "A fájl nem nyitható meg: {error}",
            "clip_start_marker_set": "Kezdőjelölő beállítva: {time}.",
            "clip_end_marker_set": "Végjelölő beállítva: {time}.",
            "clip_markers_missing": "Először állíts be kezdő- és végjelölőt.",
            "clip_marker_invalid": "A végjelölőnek a kezdőjelölő után kell lennie.",
            "clip_export_started": "Klipp exportálása elindult.",
            "clip_export_done": "Klipp exportálva: {title}",
            "clip_export_failed": "Klipp exportálása sikertelen: {error}",
            "shortcut_player_equalizer": "Lejátszó: hangszínszabályzó",
            "shortcut_player_marker_start": "Lejátszó: kezdőjelölő beállítása",
            "shortcut_player_marker_end": "Lejátszó: végjelölő beállítása",
            "shortcut_player_export_clip": "Lejátszó: kijelölt klipp exportálása",
            "shortcut_open_play_from_folder": "Megnyitás: lejátszás mappából",
            "bass_boost": "Basszuskiemelés",
            "bass_boost_on": "Basszuskiemelés bekapcsolva.",
            "bass_boost_off": "Basszuskiemelés kikapcsolva.",
        },
        "ro": {
            "play_from_folder": "Redă din folder",
            "announce_play_pause": "Anunță redare/pauză în player",
            "playback_paused": "Pauzat.",
            "playback_playing": "Se redă.",
            "select_media_file": "Alege un fișier audio sau video",
            "media_files": "Fișiere audio și video",
            "all_files": "Toate fișierele",
            "global_equalizer": "Egalizator global",
            "equalizer_preset": "Preset egalizator",
            "equalizer_preset_name": "Nume preset personalizat",
            "equalizer_db_range": "Interval egalizator în dB",
            "equalizer_band_gain": "Egalizator {band}",
            "reset_equalizer": "Resetează acest preset",
            "equalizer_saved": "Egalizator salvat.",
            "equalizer_closed": "Egalizator închis.",
            "equalizer_apply_failed": "Egalizatorul nu a putut fi aplicat: {error}",
            "set_default_player": "Setează ApricotPlayer ca player media implicit",
            "default_player_settings_opened": "Setările Windows pentru aplicații implicite au fost deschise.",
            "default_player_settings_failed": "Nu s-au putut deschide setările Windows pentru aplicații implicite: {error}",
            "media_association_prompt_title": "Înregistrare player media",
            "media_association_prompt_message": "ApricotPlayer nu este încă înregistrat ca player media Windows. Îl adaugi ca opțiune pentru fișiere audio și video? Playerul implicit se alege în continuare manual în Windows.",
            "media_association_registered": "ApricotPlayer este înregistrat ca player media.",
            "media_association_failed": "Înregistrarea playerului media a eșuat: {error}",
            "local_media": "Fișier media local",
            "local_file_open_failed": "Fișierul nu a putut fi deschis: {error}",
            "clip_start_marker_set": "Marcajul de început setat la {time}.",
            "clip_end_marker_set": "Marcajul de sfârșit setat la {time}.",
            "clip_markers_missing": "Setează mai întâi marcajul de început și de sfârșit.",
            "clip_marker_invalid": "Marcajul de sfârșit trebuie să fie după cel de început.",
            "clip_export_started": "Exportul clipului a început.",
            "clip_export_done": "Clip exportat: {title}",
            "clip_export_failed": "Exportul clipului a eșuat: {error}",
            "shortcut_player_equalizer": "Player: egalizator",
            "shortcut_player_marker_start": "Player: setează marcajul de început",
            "shortcut_player_marker_end": "Player: setează marcajul de sfârșit",
            "shortcut_player_export_clip": "Player: exportă clipul marcat",
            "shortcut_open_play_from_folder": "Deschide: redă din folder",
            "bass_boost": "Amplificare bass",
            "bass_boost_on": "Amplificare bass activată.",
            "bass_boost_off": "Amplificare bass dezactivată.",
        },
        "tr": {
            "play_from_folder": "Klasörden oynat",
            "announce_play_pause": "Oynatıcıda oynat/duraklat durumunu duyur",
            "playback_paused": "Duraklatıldı.",
            "playback_playing": "Oynatılıyor.",
            "select_media_file": "Ses veya video dosyası seç",
            "media_files": "Ses ve video dosyaları",
            "all_files": "Tüm dosyalar",
            "global_equalizer": "Genel ekolayzer",
            "equalizer_preset": "Ekolayzer ön ayarı",
            "equalizer_preset_name": "Özel ön ayar adı",
            "equalizer_db_range": "dB cinsinden ekolayzer aralığı",
            "equalizer_band_gain": "Ekolayzer {band}",
            "reset_equalizer": "Bu ön ayarı sıfırla",
            "equalizer_saved": "Ekolayzer kaydedildi.",
            "equalizer_closed": "Ekolayzer kapatıldı.",
            "equalizer_apply_failed": "Ekolayzer uygulanamadı: {error}",
            "set_default_player": "ApricotPlayer'ı varsayılan medya oynatıcı yap",
            "default_player_settings_opened": "Windows varsayılan uygulamalar ayarları açıldı.",
            "default_player_settings_failed": "Windows varsayılan uygulamalar ayarları açılamadı: {error}",
            "media_association_prompt_title": "Medya oynatıcı kaydı",
            "media_association_prompt_message": "ApricotPlayer henüz Windows medya oynatıcısı olarak kayıtlı değil. Ses ve video dosyaları için seçenek olarak eklensin mi? Varsayılan oynatıcıyı Windows içinde yine elle seçersiniz.",
            "media_association_registered": "ApricotPlayer medya oynatıcı olarak kaydedildi.",
            "media_association_failed": "Medya oynatıcı kaydı başarısız: {error}",
            "local_media": "Yerel medya dosyası",
            "local_file_open_failed": "Dosya açılamadı: {error}",
            "clip_start_marker_set": "Başlangıç işareti {time} konumuna ayarlandı.",
            "clip_end_marker_set": "Bitiş işareti {time} konumuna ayarlandı.",
            "clip_markers_missing": "Önce başlangıç ve bitiş işaretlerini ayarlayın.",
            "clip_marker_invalid": "Bitiş işareti başlangıç işaretinden sonra olmalıdır.",
            "clip_export_started": "Klip dışa aktarımı başladı.",
            "clip_export_done": "Klip dışa aktarıldı: {title}",
            "clip_export_failed": "Klip dışa aktarılamadı: {error}",
            "shortcut_player_equalizer": "Oynatıcı: ekolayzer",
            "shortcut_player_marker_start": "Oynatıcı: başlangıç işareti koy",
            "shortcut_player_marker_end": "Oynatıcı: bitiş işareti koy",
            "shortcut_player_export_clip": "Oynatıcı: işaretli klibi dışa aktar",
            "shortcut_open_play_from_folder": "Aç: klasörden oynat",
            "bass_boost": "Bas güçlendirme",
            "bass_boost_on": "Bas güçlendirme açık.",
            "bass_boost_off": "Bas güçlendirme kapalı.",
        },
        "uk": {
            "play_from_folder": "Відтворити з папки",
            "announce_play_pause": "Оголошувати відтворення/паузу у програвачі",
            "playback_paused": "Пауза.",
            "playback_playing": "Відтворення.",
            "select_media_file": "Виберіть аудіо або відеофайл",
            "media_files": "Аудіо та відеофайли",
            "all_files": "Усі файли",
            "global_equalizer": "Глобальний еквалайзер",
            "equalizer_preset": "Пресет еквалайзера",
            "equalizer_preset_name": "Назва власного пресета",
            "equalizer_db_range": "Діапазон еквалайзера в дБ",
            "equalizer_band_gain": "Еквалайзер {band}",
            "reset_equalizer": "Скинути цей пресет",
            "equalizer_saved": "Еквалайзер збережено.",
            "equalizer_closed": "Еквалайзер закрито.",
            "equalizer_apply_failed": "Не вдалося застосувати еквалайзер: {error}",
            "set_default_player": "Зробити ApricotPlayer типовим медіапрогравачем",
            "default_player_settings_opened": "Відкрито налаштування типових програм Windows.",
            "default_player_settings_failed": "Не вдалося відкрити налаштування типових програм Windows: {error}",
            "media_association_prompt_title": "Реєстрація медіапрогравача",
            "media_association_prompt_message": "ApricotPlayer ще не зареєстровано як медіапрогравач Windows. Додати його як варіант для аудіо та відеофайлів? Типовий програвач все одно вибирається вручну у Windows.",
            "media_association_registered": "ApricotPlayer зареєстровано як медіапрогравач.",
            "media_association_failed": "Не вдалося зареєструвати медіапрогравач: {error}",
            "local_media": "Локальний медіафайл",
            "local_file_open_failed": "Не вдалося відкрити файл: {error}",
            "clip_start_marker_set": "Початкову мітку встановлено на {time}.",
            "clip_end_marker_set": "Кінцеву мітку встановлено на {time}.",
            "clip_markers_missing": "Спочатку встановіть початкову та кінцеву мітки.",
            "clip_marker_invalid": "Кінцева мітка має бути після початкової.",
            "clip_export_started": "Експорт кліпу розпочато.",
            "clip_export_done": "Кліп експортовано: {title}",
            "clip_export_failed": "Не вдалося експортувати кліп: {error}",
            "shortcut_player_equalizer": "Програвач: еквалайзер",
            "shortcut_player_marker_start": "Програвач: встановити початкову мітку",
            "shortcut_player_marker_end": "Програвач: встановити кінцеву мітку",
            "shortcut_player_export_clip": "Програвач: експортувати позначений кліп",
            "shortcut_open_play_from_folder": "Відкрити: відтворити з папки",
            "bass_boost": "Підсилення басів",
            "bass_boost_on": "Підсилення басів увімкнено.",
            "bass_boost_off": "Підсилення басів вимкнено.",
        },
        "ru": {
            "play_from_folder": "Воспроизвести из папки",
            "announce_play_pause": "Озвучивать воспроизведение/паузу в плеере",
            "playback_paused": "Пауза.",
            "playback_playing": "Воспроизведение.",
            "select_media_file": "Выберите аудио или видеофайл",
            "media_files": "Аудио и видеофайлы",
            "all_files": "Все файлы",
            "global_equalizer": "Глобальный эквалайзер",
            "equalizer_preset": "Пресет эквалайзера",
            "equalizer_preset_name": "Имя пользовательского пресета",
            "equalizer_db_range": "Диапазон эквалайзера в дБ",
            "equalizer_band_gain": "Эквалайзер {band}",
            "reset_equalizer": "Сбросить этот пресет",
            "equalizer_saved": "Эквалайзер сохранён.",
            "equalizer_closed": "Эквалайзер закрыт.",
            "equalizer_apply_failed": "Не удалось применить эквалайзер: {error}",
            "set_default_player": "Сделать ApricotPlayer медиаплеером по умолчанию",
            "default_player_settings_opened": "Открыты параметры приложений по умолчанию Windows.",
            "default_player_settings_failed": "Не удалось открыть параметры приложений по умолчанию Windows: {error}",
            "media_association_prompt_title": "Регистрация медиаплеера",
            "media_association_prompt_message": "ApricotPlayer ещё не зарегистрирован как медиаплеер Windows. Добавить его как вариант для аудио и видеофайлов? Плеер по умолчанию всё равно выбирается вручную в Windows.",
            "media_association_registered": "ApricotPlayer зарегистрирован как медиаплеер.",
            "media_association_failed": "Не удалось зарегистрировать медиаплеер: {error}",
            "local_media": "Локальный медиафайл",
            "local_file_open_failed": "Не удалось открыть файл: {error}",
            "clip_start_marker_set": "Начальная метка установлена на {time}.",
            "clip_end_marker_set": "Конечная метка установлена на {time}.",
            "clip_markers_missing": "Сначала установите начальную и конечную метки.",
            "clip_marker_invalid": "Конечная метка должна быть после начальной.",
            "clip_export_started": "Экспорт клипа начат.",
            "clip_export_done": "Клип экспортирован: {title}",
            "clip_export_failed": "Экспорт клипа не удался: {error}",
            "shortcut_player_equalizer": "Плеер: эквалайзер",
            "shortcut_player_marker_start": "Плеер: установить начальную метку",
            "shortcut_player_marker_end": "Плеер: установить конечную метку",
            "shortcut_player_export_clip": "Плеер: экспортировать отмеченный клип",
            "shortcut_open_play_from_folder": "Открыть: воспроизвести из папки",
            "bass_boost": "Усиление басов",
            "bass_boost_on": "Усиление басов включено.",
            "bass_boost_off": "Усиление басов выключено.",
        },
        "ja": {
            "play_from_folder": "フォルダーから再生",
            "announce_play_pause": "プレイヤーで再生/一時停止を読み上げる",
            "playback_paused": "一時停止しました。",
            "playback_playing": "再生中です。",
            "select_media_file": "音声または動画ファイルを選択",
            "media_files": "音声と動画ファイル",
            "all_files": "すべてのファイル",
            "global_equalizer": "グローバルイコライザー",
            "equalizer_preset": "イコライザープリセット",
            "equalizer_preset_name": "カスタムプリセット名",
            "equalizer_db_range": "イコライザー範囲 dB",
            "equalizer_band_gain": "イコライザー {band}",
            "reset_equalizer": "このプリセットをリセット",
            "equalizer_saved": "イコライザーを保存しました。",
            "equalizer_closed": "イコライザーを閉じました。",
            "equalizer_apply_failed": "イコライザーを適用できませんでした: {error}",
            "set_default_player": "ApricotPlayer を既定のメディアプレイヤーに設定",
            "default_player_settings_opened": "Windows の既定のアプリ設定を開きました。",
            "default_player_settings_failed": "Windows の既定のアプリ設定を開けませんでした: {error}",
            "media_association_prompt_title": "メディアプレイヤー登録",
            "media_association_prompt_message": "ApricotPlayer はまだ Windows のメディアプレイヤーとして登録されていません。音声と動画ファイルの選択肢として追加しますか？既定のプレイヤーは Windows で手動で選びます。",
            "media_association_registered": "ApricotPlayer をメディアプレイヤーとして登録しました。",
            "media_association_failed": "メディアプレイヤー登録に失敗しました: {error}",
            "local_media": "ローカルメディアファイル",
            "local_file_open_failed": "ファイルを開けませんでした: {error}",
            "clip_start_marker_set": "開始マーカーを {time} に設定しました。",
            "clip_end_marker_set": "終了マーカーを {time} に設定しました。",
            "clip_markers_missing": "先に開始マーカーと終了マーカーを設定してください。",
            "clip_marker_invalid": "終了マーカーは開始マーカーより後である必要があります。",
            "clip_export_started": "クリップのエクスポートを開始しました。",
            "clip_export_done": "クリップをエクスポートしました: {title}",
            "clip_export_failed": "クリップのエクスポートに失敗しました: {error}",
            "shortcut_player_equalizer": "プレイヤー: イコライザー",
            "shortcut_player_marker_start": "プレイヤー: 開始マーカーを設定",
            "shortcut_player_marker_end": "プレイヤー: 終了マーカーを設定",
            "shortcut_player_export_clip": "プレイヤー: マークしたクリップをエクスポート",
            "shortcut_open_play_from_folder": "開く: フォルダーから再生",
            "bass_boost": "低音ブースト",
            "bass_boost_on": "低音ブースト オン。",
            "bass_boost_off": "低音ブースト オフ。",
        },
        "ko": {
            "play_from_folder": "폴더에서 재생",
            "announce_play_pause": "플레이어에서 재생/일시정지 알림",
            "playback_paused": "일시정지됨.",
            "playback_playing": "재생 중.",
            "select_media_file": "오디오 또는 비디오 파일 선택",
            "media_files": "오디오 및 비디오 파일",
            "all_files": "모든 파일",
            "global_equalizer": "전역 이퀄라이저",
            "equalizer_preset": "이퀄라이저 프리셋",
            "equalizer_preset_name": "사용자 프리셋 이름",
            "equalizer_db_range": "이퀄라이저 범위 dB",
            "equalizer_band_gain": "이퀄라이저 {band}",
            "reset_equalizer": "이 프리셋 초기화",
            "equalizer_saved": "이퀄라이저 저장됨.",
            "equalizer_closed": "이퀄라이저 닫힘.",
            "equalizer_apply_failed": "이퀄라이저를 적용할 수 없습니다: {error}",
            "set_default_player": "ApricotPlayer를 기본 미디어 플레이어로 설정",
            "default_player_settings_opened": "Windows 기본 앱 설정을 열었습니다.",
            "default_player_settings_failed": "Windows 기본 앱 설정을 열 수 없습니다: {error}",
            "media_association_prompt_title": "미디어 플레이어 등록",
            "media_association_prompt_message": "ApricotPlayer가 아직 Windows 미디어 플레이어로 등록되지 않았습니다. 오디오 및 비디오 파일 옵션으로 추가할까요? 기본 플레이어는 Windows에서 직접 선택합니다.",
            "media_association_registered": "ApricotPlayer가 미디어 플레이어로 등록되었습니다.",
            "media_association_failed": "미디어 플레이어 등록 실패: {error}",
            "local_media": "로컬 미디어 파일",
            "local_file_open_failed": "파일을 열 수 없습니다: {error}",
            "clip_start_marker_set": "시작 마커를 {time}에 설정했습니다.",
            "clip_end_marker_set": "끝 마커를 {time}에 설정했습니다.",
            "clip_markers_missing": "먼저 시작 마커와 끝 마커를 설정하세요.",
            "clip_marker_invalid": "끝 마커는 시작 마커 뒤에 있어야 합니다.",
            "clip_export_started": "클립 내보내기를 시작했습니다.",
            "clip_export_done": "클립 내보냄: {title}",
            "clip_export_failed": "클립 내보내기 실패: {error}",
            "shortcut_player_equalizer": "플레이어: 이퀄라이저",
            "shortcut_player_marker_start": "플레이어: 시작 마커 설정",
            "shortcut_player_marker_end": "플레이어: 끝 마커 설정",
            "shortcut_player_export_clip": "플레이어: 표시한 클립 내보내기",
            "shortcut_open_play_from_folder": "열기: 폴더에서 재생",
            "bass_boost": "베이스 부스트",
            "bass_boost_on": "베이스 부스트 켜짐.",
            "bass_boost_off": "베이스 부스트 꺼짐.",
        },
        "zh": {
            "play_from_folder": "从文件夹播放",
            "announce_play_pause": "在播放器中朗读播放/暂停",
            "playback_paused": "已暂停。",
            "playback_playing": "正在播放。",
            "select_media_file": "选择音频或视频文件",
            "media_files": "音频和视频文件",
            "all_files": "所有文件",
            "global_equalizer": "全局均衡器",
            "equalizer_preset": "均衡器预设",
            "equalizer_preset_name": "自定义预设名称",
            "equalizer_db_range": "均衡器范围 dB",
            "equalizer_band_gain": "均衡器 {band}",
            "reset_equalizer": "重置此预设",
            "equalizer_saved": "均衡器已保存。",
            "equalizer_closed": "均衡器已关闭。",
            "equalizer_apply_failed": "无法应用均衡器：{error}",
            "set_default_player": "将 ApricotPlayer 设为默认媒体播放器",
            "default_player_settings_opened": "已打开 Windows 默认应用设置。",
            "default_player_settings_failed": "无法打开 Windows 默认应用设置：{error}",
            "media_association_prompt_title": "媒体播放器注册",
            "media_association_prompt_message": "ApricotPlayer 尚未注册为 Windows 媒体播放器。是否将它添加为音频和视频文件的选项？默认播放器仍需在 Windows 中手动选择。",
            "media_association_registered": "ApricotPlayer 已注册为媒体播放器。",
            "media_association_failed": "媒体播放器注册失败：{error}",
            "local_media": "本地媒体文件",
            "local_file_open_failed": "无法打开文件：{error}",
            "clip_start_marker_set": "开始标记已设为 {time}。",
            "clip_end_marker_set": "结束标记已设为 {time}。",
            "clip_markers_missing": "请先设置开始和结束标记。",
            "clip_marker_invalid": "结束标记必须在开始标记之后。",
            "clip_export_started": "已开始导出片段。",
            "clip_export_done": "片段已导出：{title}",
            "clip_export_failed": "片段导出失败：{error}",
            "shortcut_player_equalizer": "播放器：均衡器",
            "shortcut_player_marker_start": "播放器：设置开始标记",
            "shortcut_player_marker_end": "播放器：设置结束标记",
            "shortcut_player_export_clip": "播放器：导出标记片段",
            "shortcut_open_play_from_folder": "打开：从文件夹播放",
            "bass_boost": "低音增强",
            "bass_boost_on": "低音增强开启。",
            "bass_boost_off": "低音增强关闭。",
        },
        "ar": {
            "play_from_folder": "تشغيل من مجلد",
            "announce_play_pause": "نطق التشغيل/الإيقاف المؤقت في المشغل",
            "playback_paused": "متوقف مؤقتا.",
            "playback_playing": "قيد التشغيل.",
            "select_media_file": "اختر ملف صوت أو فيديو",
            "media_files": "ملفات الصوت والفيديو",
            "all_files": "كل الملفات",
            "global_equalizer": "معادل الصوت العام",
            "equalizer_preset": "إعداد معادل الصوت",
            "equalizer_preset_name": "اسم الإعداد المخصص",
            "equalizer_db_range": "نطاق المعادل بالديسيبل",
            "equalizer_band_gain": "معادل الصوت {band}",
            "reset_equalizer": "إعادة ضبط هذا الإعداد",
            "equalizer_saved": "تم حفظ معادل الصوت.",
            "equalizer_closed": "تم إغلاق معادل الصوت.",
            "equalizer_apply_failed": "تعذر تطبيق معادل الصوت: {error}",
            "set_default_player": "تعيين ApricotPlayer كمشغل وسائط افتراضي",
            "default_player_settings_opened": "تم فتح إعدادات التطبيقات الافتراضية في Windows.",
            "default_player_settings_failed": "تعذر فتح إعدادات التطبيقات الافتراضية في Windows: {error}",
            "media_association_prompt_title": "تسجيل مشغل الوسائط",
            "media_association_prompt_message": "لم يتم تسجيل ApricotPlayer بعد كمشغل وسائط في Windows. هل تريد إضافته كخيار لملفات الصوت والفيديو؟ اختيار المشغل الافتراضي يتم يدويا من Windows.",
            "media_association_registered": "تم تسجيل ApricotPlayer كمشغل وسائط.",
            "media_association_failed": "فشل تسجيل مشغل الوسائط: {error}",
            "local_media": "ملف وسائط محلي",
            "local_file_open_failed": "تعذر فتح الملف: {error}",
            "clip_start_marker_set": "تم تعيين علامة البداية عند {time}.",
            "clip_end_marker_set": "تم تعيين علامة النهاية عند {time}.",
            "clip_markers_missing": "عيّن أولا علامة البداية وعلامة النهاية.",
            "clip_marker_invalid": "يجب أن تكون علامة النهاية بعد علامة البداية.",
            "clip_export_started": "بدأ تصدير المقطع.",
            "clip_export_done": "تم تصدير المقطع: {title}",
            "clip_export_failed": "فشل تصدير المقطع: {error}",
            "shortcut_player_equalizer": "المشغل: معادل الصوت",
            "shortcut_player_marker_start": "المشغل: تعيين علامة البداية",
            "shortcut_player_marker_end": "المشغل: تعيين علامة النهاية",
            "shortcut_player_export_clip": "المشغل: تصدير المقطع المحدد",
            "shortcut_open_play_from_folder": "فتح: تشغيل من مجلد",
            "bass_boost": "تعزيز الجهير",
            "bass_boost_on": "تعزيز الجهير مفعل.",
            "bass_boost_off": "تعزيز الجهير معطل.",
        },
        "hi": {
            "play_from_folder": "फ़ोल्डर से चलाएं",
            "announce_play_pause": "प्लेयर में प्ले/पॉज़ की घोषणा करें",
            "playback_paused": "पॉज़ किया गया.",
            "playback_playing": "चल रहा है.",
            "select_media_file": "ऑडियो या वीडियो फ़ाइल चुनें",
            "media_files": "ऑडियो और वीडियो फ़ाइलें",
            "all_files": "सभी फ़ाइलें",
            "global_equalizer": "ग्लोबल इक्वलाइज़र",
            "equalizer_preset": "इक्वलाइज़र प्रीसेट",
            "equalizer_preset_name": "कस्टम प्रीसेट नाम",
            "equalizer_db_range": "इक्वलाइज़र रेंज dB में",
            "equalizer_band_gain": "इक्वलाइज़र {band}",
            "reset_equalizer": "इस प्रीसेट को रीसेट करें",
            "equalizer_saved": "इक्वलाइज़र सहेजा गया.",
            "equalizer_closed": "इक्वलाइज़र बंद.",
            "equalizer_apply_failed": "इक्वलाइज़र लागू नहीं हो सका: {error}",
            "set_default_player": "ApricotPlayer को डिफ़ॉल्ट मीडिया प्लेयर बनाएं",
            "default_player_settings_opened": "Windows डिफ़ॉल्ट ऐप सेटिंग खुल गई.",
            "default_player_settings_failed": "Windows डिफ़ॉल्ट ऐप सेटिंग नहीं खुल सकी: {error}",
            "media_association_prompt_title": "मीडिया प्लेयर पंजीकरण",
            "media_association_prompt_message": "ApricotPlayer अभी Windows मीडिया प्लेयर के रूप में पंजीकृत नहीं है. क्या इसे ऑडियो और वीडियो फ़ाइलों के विकल्प के रूप में जोड़ना है? डिफ़ॉल्ट प्लेयर Windows में मैन्युअल रूप से चुना जाता है.",
            "media_association_registered": "ApricotPlayer मीडिया प्लेयर के रूप में पंजीकृत है.",
            "media_association_failed": "मीडिया प्लेयर पंजीकरण विफल: {error}",
            "local_media": "स्थानीय मीडिया फ़ाइल",
            "local_file_open_failed": "फ़ाइल नहीं खुल सकी: {error}",
            "clip_start_marker_set": "आरंभ मार्कर {time} पर सेट.",
            "clip_end_marker_set": "समाप्ति मार्कर {time} पर सेट.",
            "clip_markers_missing": "पहले आरंभ और समाप्ति मार्कर सेट करें.",
            "clip_marker_invalid": "समाप्ति मार्कर आरंभ मार्कर के बाद होना चाहिए.",
            "clip_export_started": "क्लिप निर्यात शुरू.",
            "clip_export_done": "क्लिप निर्यात हुई: {title}",
            "clip_export_failed": "क्लिप निर्यात विफल: {error}",
            "shortcut_player_equalizer": "प्लेयर: इक्वलाइज़र",
            "shortcut_player_marker_start": "प्लेयर: आरंभ मार्कर सेट करें",
            "shortcut_player_marker_end": "प्लेयर: समाप्ति मार्कर सेट करें",
            "shortcut_player_export_clip": "प्लेयर: चिह्नित क्लिप निर्यात करें",
            "shortcut_open_play_from_folder": "खोलें: फ़ोल्डर से चलाएं",
            "bass_boost": "बेस बूस्ट",
            "bass_boost_on": "बेस बूस्ट चालू.",
            "bass_boost_off": "बेस बूस्ट बंद.",
        },
        "id": {
            "play_from_folder": "Putar dari folder",
            "announce_play_pause": "Umumkan putar/jeda di pemutar",
            "playback_paused": "Dijeda.",
            "playback_playing": "Memutar.",
            "select_media_file": "Pilih file audio atau video",
            "media_files": "File audio dan video",
            "all_files": "Semua file",
            "global_equalizer": "Equalizer global",
            "equalizer_preset": "Preset equalizer",
            "equalizer_preset_name": "Nama preset khusus",
            "equalizer_db_range": "Rentang equalizer dalam dB",
            "equalizer_band_gain": "Equalizer {band}",
            "reset_equalizer": "Reset preset ini",
            "equalizer_saved": "Equalizer disimpan.",
            "equalizer_closed": "Equalizer ditutup.",
            "equalizer_apply_failed": "Equalizer tidak dapat diterapkan: {error}",
            "set_default_player": "Jadikan ApricotPlayer pemutar media default",
            "default_player_settings_opened": "Pengaturan aplikasi default Windows dibuka.",
            "default_player_settings_failed": "Tidak dapat membuka pengaturan aplikasi default Windows: {error}",
            "media_association_prompt_title": "Pendaftaran pemutar media",
            "media_association_prompt_message": "ApricotPlayer belum terdaftar sebagai pemutar media Windows. Tambahkan sebagai opsi untuk file audio dan video? Pemutar default tetap dipilih manual di Windows.",
            "media_association_registered": "ApricotPlayer terdaftar sebagai pemutar media.",
            "media_association_failed": "Pendaftaran pemutar media gagal: {error}",
            "local_media": "File media lokal",
            "local_file_open_failed": "File tidak dapat dibuka: {error}",
            "clip_start_marker_set": "Penanda awal diatur ke {time}.",
            "clip_end_marker_set": "Penanda akhir diatur ke {time}.",
            "clip_markers_missing": "Atur penanda awal dan akhir terlebih dahulu.",
            "clip_marker_invalid": "Penanda akhir harus setelah penanda awal.",
            "clip_export_started": "Ekspor klip dimulai.",
            "clip_export_done": "Klip diekspor: {title}",
            "clip_export_failed": "Ekspor klip gagal: {error}",
            "shortcut_player_equalizer": "Pemutar: equalizer",
            "shortcut_player_marker_start": "Pemutar: atur penanda awal",
            "shortcut_player_marker_end": "Pemutar: atur penanda akhir",
            "shortcut_player_export_clip": "Pemutar: ekspor klip bertanda",
            "shortcut_open_play_from_folder": "Buka: putar dari folder",
            "bass_boost": "Penguat bass",
            "bass_boost_on": "Penguat bass aktif.",
            "bass_boost_off": "Penguat bass nonaktif.",
        },
        "fi": {
            "play_from_folder": "Toista kansiosta",
            "announce_play_pause": "Ilmoita toisto/tauko soittimessa",
            "playback_paused": "Keskeytetty.",
            "playback_playing": "Toistetaan.",
            "select_media_file": "Valitse ääni- tai videotiedosto",
            "media_files": "Ääni- ja videotiedostot",
            "all_files": "Kaikki tiedostot",
            "global_equalizer": "Yleinen taajuuskorjain",
            "equalizer_preset": "Taajuuskorjaimen esiasetus",
            "equalizer_preset_name": "Mukautetun esiasetuksen nimi",
            "equalizer_db_range": "Taajuuskorjaimen alue dB",
            "equalizer_band_gain": "Taajuuskorjain {band}",
            "reset_equalizer": "Palauta tämä esiasetus",
            "equalizer_saved": "Taajuuskorjain tallennettu.",
            "equalizer_closed": "Taajuuskorjain suljettu.",
            "equalizer_apply_failed": "Taajuuskorjainta ei voitu käyttää: {error}",
            "set_default_player": "Aseta ApricotPlayer oletusmediasoittimeksi",
            "default_player_settings_opened": "Windowsin oletussovellusasetukset avattu.",
            "default_player_settings_failed": "Windowsin oletussovellusasetuksia ei voitu avata: {error}",
            "media_association_prompt_title": "Mediasoittimen rekisteröinti",
            "media_association_prompt_message": "ApricotPlayeria ei ole vielä rekisteröity Windows-mediasoittimeksi. Lisätäänkö se vaihtoehdoksi ääni- ja videotiedostoille? Oletussoitin valitaan edelleen käsin Windowsissa.",
            "media_association_registered": "ApricotPlayer on rekisteröity mediasoittimeksi.",
            "media_association_failed": "Mediasoittimen rekisteröinti epäonnistui: {error}",
            "local_media": "Paikallinen mediatiedosto",
            "local_file_open_failed": "Tiedostoa ei voitu avata: {error}",
            "clip_start_marker_set": "Alkumerkki asetettu kohtaan {time}.",
            "clip_end_marker_set": "Loppumerkki asetettu kohtaan {time}.",
            "clip_markers_missing": "Aseta ensin alku- ja loppumerkki.",
            "clip_marker_invalid": "Loppumerkin on oltava alkumerkin jälkeen.",
            "clip_export_started": "Leikkeen vienti aloitettu.",
            "clip_export_done": "Leike viety: {title}",
            "clip_export_failed": "Leikkeen vienti epäonnistui: {error}",
            "shortcut_player_equalizer": "Soitin: taajuuskorjain",
            "shortcut_player_marker_start": "Soitin: aseta alkumerkki",
            "shortcut_player_marker_end": "Soitin: aseta loppumerkki",
            "shortcut_player_export_clip": "Soitin: vie merkitty leike",
            "shortcut_open_play_from_folder": "Avaa: toista kansiosta",
            "bass_boost": "Bassokorostus",
            "bass_boost_on": "Bassokorostus päällä.",
            "bass_boost_off": "Bassokorostus pois.",
        },
        "el": {
            "play_from_folder": "Αναπαραγωγή από φάκελο",
            "announce_play_pause": "Ανακοίνωση αναπαραγωγής/παύσης στον player",
            "playback_paused": "Σε παύση.",
            "playback_playing": "Αναπαραγωγή.",
            "select_media_file": "Επιλέξτε αρχείο ήχου ή βίντεο",
            "media_files": "Αρχεία ήχου και βίντεο",
            "all_files": "Όλα τα αρχεία",
            "global_equalizer": "Γενικός ισοσταθμιστής",
            "equalizer_preset": "Προρύθμιση ισοσταθμιστή",
            "equalizer_preset_name": "Όνομα προσαρμοσμένης προρύθμισης",
            "equalizer_db_range": "Εύρος ισοσταθμιστή σε dB",
            "equalizer_band_gain": "Ισοσταθμιστής {band}",
            "reset_equalizer": "Επαναφορά αυτής της προρύθμισης",
            "equalizer_saved": "Ο ισοσταθμιστής αποθηκεύτηκε.",
            "equalizer_closed": "Ο ισοσταθμιστής έκλεισε.",
            "equalizer_apply_failed": "Δεν ήταν δυνατή η εφαρμογή του ισοσταθμιστή: {error}",
            "set_default_player": "Ορισμός του ApricotPlayer ως προεπιλεγμένου media player",
            "default_player_settings_opened": "Άνοιξαν οι ρυθμίσεις προεπιλεγμένων εφαρμογών των Windows.",
            "default_player_settings_failed": "Δεν ήταν δυνατό το άνοιγμα των ρυθμίσεων προεπιλεγμένων εφαρμογών των Windows: {error}",
            "media_association_prompt_title": "Καταχώριση media player",
            "media_association_prompt_message": "Το ApricotPlayer δεν έχει ακόμη καταχωριστεί ως media player των Windows. Να προστεθεί ως επιλογή για αρχεία ήχου και βίντεο; Ο προεπιλεγμένος player επιλέγεται ακόμη χειροκίνητα στα Windows.",
            "media_association_registered": "Το ApricotPlayer καταχωρίστηκε ως media player.",
            "media_association_failed": "Η καταχώριση media player απέτυχε: {error}",
            "local_media": "Τοπικό αρχείο πολυμέσων",
            "local_file_open_failed": "Δεν ήταν δυνατό το άνοιγμα του αρχείου: {error}",
            "clip_start_marker_set": "Ο δείκτης αρχής ορίστηκε στο {time}.",
            "clip_end_marker_set": "Ο δείκτης τέλους ορίστηκε στο {time}.",
            "clip_markers_missing": "Ορίστε πρώτα δείκτη αρχής και τέλους.",
            "clip_marker_invalid": "Ο δείκτης τέλους πρέπει να είναι μετά τον δείκτη αρχής.",
            "clip_export_started": "Η εξαγωγή κλιπ ξεκίνησε.",
            "clip_export_done": "Το κλιπ εξήχθη: {title}",
            "clip_export_failed": "Η εξαγωγή κλιπ απέτυχε: {error}",
            "shortcut_player_equalizer": "Player: ισοσταθμιστής",
            "shortcut_player_marker_start": "Player: ορισμός δείκτη αρχής",
            "shortcut_player_marker_end": "Player: ορισμός δείκτη τέλους",
            "shortcut_player_export_clip": "Player: εξαγωγή σημειωμένου κλιπ",
            "shortcut_open_play_from_folder": "Άνοιγμα: αναπαραγωγή από φάκελο",
            "bass_boost": "Ενίσχυση μπάσων",
            "bass_boost_on": "Ενίσχυση μπάσων ενεργή.",
            "bass_boost_off": "Ενίσχυση μπάσων ανενεργή.",
        },
    }
)
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(MEDIA_PLAYER_TRANSLATION_UPDATES.get(language_code, MEDIA_PLAYER_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in MEDIA_PLAYER_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_071_TRANSLATION_UPDATES = {
    "sl": {
        "first_run_language_prompt": "Izberi jezik za ApricotPlayer.",
        "ask_download_location_each_time": "Vedno vpraĹˇaj, kam shraniti vsak prenos",
        "choose_save_path": "Izberi ime in mesto shranjevanja",
        "choose_save_folder": "Izberi mapo za shranjevanje",
        "clip_start_marker_cleared": "Start marker izbrisan.",
        "clip_end_marker_cleared": "End marker izbrisan.",
        "playback_queue": "Vrstni red predvajanja",
        "playback_queue_empty": "Vrstni red predvajanja je prazen.",
        "playback_queue_instructions": "Pritisni Enter za takojĹˇnje predvajanje izbranega elementa.",
        "playback_queue_added": "Dodano v vrstni red predvajanja: {title}",
        "playback_queue_removed": "Odstranjeno iz vrstnega reda predvajanja: {title}",
        "playback_queue_already_added": "Ta element je Ĺľe v vrstnem redu predvajanja: {title}",
        "playback_queue_not_found": "Element ni v vrstnem redu predvajanja.",
        "add_to_playback_queue": "Dodaj v vrstni red predvajanja",
        "remove_from_playback_queue": "Odstrani iz vrstnega reda predvajanja",
        "edit_mode": "Edit mode",
        "edit_mode_on": "Edit mode on.",
        "edit_mode_off": "Edit mode off.",
        "edit_mode_local_only": "Edit mode je na voljo samo za lokalne datoteke.",
        "edit_save_started": "Shranjujem urejeno datoteko.",
        "edit_save_done": "Urejena datoteka shranjena: {title}",
        "edit_replace_done": "Originalna datoteka zamenjana: {title}",
        "edit_save_failed": "Shranjevanje urejene datoteke ni uspelo: {error}",
        "shortcut_add_to_playback_queue": "Dodaj v vrstni red predvajanja",
        "shortcut_remove_from_playback_queue": "Odstrani iz vrstnega reda predvajanja",
        "shortcut_open_playback_queue": "Odpri vrstni red predvajanja",
        "shortcut_player_edit_mode": "Predvajalnik: edit mode",
        "shortcut_player_save_edit_copy": "Predvajalnik: shrani urejeno kopijo lokalne datoteke",
        "shortcut_player_replace_edit_original": "Predvajalnik: zamenjaj originalno lokalno datoteko",
        "shortcut_player_equalizer": "Predvajalnik: equalizer",
    },
    "en": {
        "first_run_language_prompt": "Choose the language for ApricotPlayer.",
        "ask_download_location_each_time": "Ask where to save each download every time",
        "choose_save_path": "Choose file name and save location",
        "choose_save_folder": "Choose save folder",
        "clip_start_marker_cleared": "Start marker cleared.",
        "clip_end_marker_cleared": "End marker cleared.",
        "playback_queue": "Playback queue",
        "playback_queue_empty": "Playback queue is empty.",
        "playback_queue_instructions": "Press Enter to play the selected item immediately.",
        "playback_queue_added": "Added to playback queue: {title}",
        "playback_queue_removed": "Removed from playback queue: {title}",
        "playback_queue_already_added": "This item is already in the playback queue: {title}",
        "playback_queue_not_found": "Item is not in the playback queue.",
        "add_to_playback_queue": "Add to playback queue",
        "remove_from_playback_queue": "Remove from playback queue",
        "edit_mode": "Edit mode",
        "edit_mode_on": "Edit mode on.",
        "edit_mode_off": "Edit mode off.",
        "edit_mode_local_only": "Edit mode is available only for local files.",
        "edit_save_started": "Saving edited file.",
        "edit_save_done": "Edited file saved: {title}",
        "edit_replace_done": "Original file replaced: {title}",
        "edit_save_failed": "Could not save edited file: {error}",
        "shortcut_add_to_playback_queue": "Add to playback queue",
        "shortcut_remove_from_playback_queue": "Remove from playback queue",
        "shortcut_open_playback_queue": "Open playback queue",
        "shortcut_player_edit_mode": "Player: edit mode",
        "shortcut_player_save_edit_copy": "Player: save edited local-file copy",
        "shortcut_player_replace_edit_original": "Player: replace original local file",
        "shortcut_player_equalizer": "Player: equalizer",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_071_TRANSLATION_UPDATES.get(language_code, RELEASE_071_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_071_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_08_TRANSLATION_UPDATES = {
    "sl": {
        "default_volume": "Privzeta glasnost predvajanja",
        "volume_boost_by_default": "Volume boost vklopljen privzeto",
        "file_converter": "Pretvornik datotek",
        "folder_converter": "Pretvornik map",
        "converter_path": "Pot do datoteke ali mape",
        "file_to_convert": "Datoteka za pretvorbo",
        "folder_to_convert": "Mapa za pretvorbo",
        "browse_file": "Izberi datoteko",
        "browse_folder": "Izberi mapo",
        "detected_format": "Zaznan format",
        "output_format": "Izhodni format",
        "convert_to": "Pretvori v",
        "add_image": "Dodaj sliko",
        "dark_background": "Temno ozadje",
        "image_path": "Pot do slike",
        "choose_image": "Izberi sliko",
        "convert": "Pretvori",
        "conversion_started": "Pretvorba se je zacela.",
        "conversion_done": "Pretvorba koncana: {title}",
        "conversion_folder_done": "Pretvorba mape koncana: {count} datotek.",
        "conversion_failed": "Pretvorba ni uspela: {error}",
        "conversion_cancelled": "Pretvorba preklicana.",
        "conversion_no_media_files": "V tej mapi ni podprtih medijskih datotek.",
        "unsupported_input_format": "Ta vhodni format ni podprt.",
        "choose_output_file": "Izberi ime in mesto pretvorjene datoteke",
        "choose_output_folder": "Izberi mapo za pretvorjene datoteke",
        "select_image_file": "Izberi sliko za video ozadje",
        "converter_audio_to_video_options": "Moznosti za pretvorbo zvoka v video",
        "audio_files": "Zvokovne datoteke",
        "video_files": "Video datoteke",
        "image_files": "Slikovne datoteke",
    },
    "en": {
        "default_volume": "Default playback volume",
        "volume_boost_by_default": "Volume boost on by default",
        "file_converter": "File converter",
        "folder_converter": "Folder converter",
        "converter_path": "File or folder path",
        "file_to_convert": "File to convert",
        "folder_to_convert": "Folder to convert",
        "browse_file": "Browse file",
        "browse_folder": "Browse folder",
        "detected_format": "Detected format",
        "output_format": "Output format",
        "convert_to": "Convert to",
        "add_image": "Add image",
        "dark_background": "Dark background",
        "image_path": "Image path",
        "choose_image": "Choose image",
        "convert": "Convert",
        "conversion_started": "Conversion started.",
        "conversion_done": "Conversion complete: {title}",
        "conversion_folder_done": "Folder conversion complete: {count} files.",
        "conversion_failed": "Conversion failed: {error}",
        "conversion_cancelled": "Conversion cancelled.",
        "conversion_no_media_files": "No supported media files were found in this folder.",
        "unsupported_input_format": "This input format is not supported.",
        "choose_output_file": "Choose converted file name and save location",
        "choose_output_folder": "Choose folder for converted files",
        "select_image_file": "Choose image for video background",
        "converter_audio_to_video_options": "Audio to video options",
        "audio_files": "Audio files",
        "video_files": "Video files",
        "image_files": "Image files",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_08_TRANSLATION_UPDATES.get(language_code, RELEASE_08_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_08_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_086_TRANSLATION_UPDATES = {
    "sl": {
        "enable_background_playback": "Omogoci predvajanje v ozadju",
        "youtube_data_api_key": "YouTube Data API key za uradne trende",
        "trending_entertainment": "Entertainment",
        "trending_technology": "Science & Technology",
        "trending_loading_official": "Nalaganje uradnih YouTube trendov za {country}, {category}.",
        "trending_api_key_required": "Za zanesljive uradne YouTube trende vnesi YouTube Data API key v Settings, Cookies and network. ApricotPlayer ne bo prikazal #trending search rezultatov kot prave trende.",
        "trending_official_unavailable": "Uradni YouTube trend feed ni na voljo: {error}",
        "trending_source_api": "Uradni YouTube most-popular feed.",
        "trending_source_public": "Javni YouTube charts/explore feed.",
        "add_equalizer_profile": "Dodaj equalizer profil",
        "save_equalizer_as_global": "Shrani kot globalni equalizer preset",
        "equalizer_profile_name": "Ime equalizer profila",
        "equalizer_profile_saved": "Equalizer profil shranjen.",
        "equalizer_global_preview_blocked": "Player equalizer je aktiven. Globalni equalizer bo slisen, ko resetiras player equalizer ali predvajas naslednji posnetek.",
        "cookie_export_diagnostics": "Cookie diagnostics:\n{details}",
        "open_youtube_login_profile": "Odpri YouTube v izbranem profilu",
        "youtube_profile_opened": "YouTube odprt v izbranem profilu.",
        "youtube_profile_open_failed": "YouTube profila ni bilo mogoce odpreti: {error}",
    },
    "en": {
        "enable_background_playback": "Enable background playback",
        "youtube_data_api_key": "YouTube Data API key for official trending",
        "trending_entertainment": "Entertainment",
        "trending_technology": "Science & Technology",
        "trending_loading_official": "Loading official YouTube trending for {country}, {category}.",
        "trending_api_key_required": "Enter a YouTube Data API key in Settings, Cookies and network for reliable official YouTube trending. ApricotPlayer will not show #trending search results as real trending.",
        "trending_official_unavailable": "Official YouTube trending feed is unavailable: {error}",
        "trending_source_api": "Official YouTube most-popular feed.",
        "trending_source_public": "Public YouTube charts/explore feed.",
        "add_equalizer_profile": "Add equalizer profile",
        "save_equalizer_as_global": "Save as global equalizer preset",
        "equalizer_profile_name": "Equalizer profile name",
        "equalizer_profile_saved": "Equalizer profile saved.",
        "equalizer_global_preview_blocked": "Player equalizer is active. Global equalizer changes will be audible after you reset the player equalizer or play the next item.",
        "cookie_export_diagnostics": "Cookie diagnostics:\n{details}",
        "open_youtube_login_profile": "Open YouTube in selected profile",
        "youtube_profile_opened": "YouTube opened in the selected profile.",
        "youtube_profile_open_failed": "Could not open the YouTube profile: {error}",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_086_TRANSLATION_UPDATES.get(language_code, RELEASE_086_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_086_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_087_TRANSLATION_UPDATES = {
    "sl": {
        "enable_trending": "Prikazi Trending v glavnem meniju",
        "trending_disabled": "Trending je izklopljen v nastavitvah.",
        "trending_unavailable_returning": "Trending ni na voljo. Vracam se v glavni meni.",
    },
    "en": {
        "enable_trending": "Show Trending in the main menu",
        "trending_disabled": "Trending is disabled in Settings.",
        "trending_unavailable_returning": "Trending is unavailable. Returning to the main menu.",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_087_TRANSLATION_UPDATES.get(language_code, RELEASE_087_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_087_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_088_TRANSLATION_UPDATES = {
    "sl": {
        "play_folder": "Predvajaj mapo",
        "select_media_folder": "Izberi mapo z audio ali video datotekami",
        "folder_loaded": "Mapa nalozena: {count} datotek.",
        "folder_no_media": "V tej mapi ni podprtih audio ali video datotek.",
        "shuffle": "Shuffle",
        "shuffle_on": "Shuffle vklopljen.",
        "shuffle_off": "Shuffle izklopljen.",
        "conversion_folder_done_with_errors": "Pretvorba mape koncana: {count} datotek pretvorjenih, {failed} ni uspelo.",
        "player_context_menu": "Player menu",
        "shortcut_player_bass_boost": "Predvajalnik: bass boost",
        "shortcut_player_repeat": "Predvajalnik: repeat",
        "shortcut_player_shuffle": "Predvajalnik: shuffle",
    },
    "en": {
        "play_folder": "Play folder",
        "select_media_folder": "Choose a folder with audio or video files",
        "folder_loaded": "Folder loaded: {count} files.",
        "folder_no_media": "This folder does not contain supported audio or video files.",
        "shuffle": "Shuffle",
        "shuffle_on": "Shuffle on.",
        "shuffle_off": "Shuffle off.",
        "conversion_folder_done_with_errors": "Folder conversion complete: {count} files converted, {failed} failed.",
        "player_context_menu": "Player menu",
        "shortcut_player_bass_boost": "Player: bass boost",
        "shortcut_player_repeat": "Player: repeat",
        "shortcut_player_shuffle": "Player: shuffle",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_088_TRANSLATION_UPDATES.get(language_code, RELEASE_088_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_088_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_089_TRANSLATION_UPDATES = {
    "sl": {
        "announce_playback_finished": "Najavi, ko se predvajanje konca",
    },
    "en": {
        "announce_playback_finished": "Announce when playback finishes",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_089_TRANSLATION_UPDATES.get(language_code, RELEASE_089_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_089_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_0816_TRANSLATION_UPDATES = {
    "sl": {
        "pause": "Pavza",
        "open_player": "Odpri zaslon predvajalnika",
        "close_player": "Zapri",
        "shuffle_folder": "Shuffle mapa",
        "add_folder_to_queue": "Dodaj mapo v vrstni red predvajanja",
        "folder_queue_added": "Mapa dodana v vrstni red predvajanja: {count} elementov.",
        "move_up": "Premakni gor",
        "move_down": "Premakni dol",
        "playback_queue_reordered": "Vrstni red predvajanja posodobljen.",
        "volume_announcement": "Glasnost: {volume}",
        "shortcut_player_volume_status": "Predvajalnik: povej glasnost",
        "shortcut_player_details": "Predvajalnik: podrobnosti videa",
    },
    "en": {
        "pause": "Pause",
        "open_player": "Open player screen",
        "close_player": "Close",
        "shuffle_folder": "Shuffle folder",
        "add_folder_to_queue": "Add folder to queue",
        "folder_queue_added": "Folder added to playback queue: {count} items.",
        "move_up": "Move up",
        "move_down": "Move down",
        "playback_queue_reordered": "Playback queue updated.",
        "volume_announcement": "Volume: {volume}",
        "shortcut_player_volume_status": "Player: announce volume",
        "shortcut_player_details": "Player: video details",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_0816_TRANSLATION_UPDATES.get(language_code, RELEASE_0816_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_0816_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_0812_TRANSLATION_UPDATES = {
    "sl": {
        "cookie_user_agent": "Browser User-Agent za cookies datoteko",
        "cookies_file_imported": "Cookies datoteka uvozena in normalizirana: {path}",
        "cookies_file_json_imported": "JSON cookies pretvorjeni v cookies.txt: {path}",
        "cookies_file_header_imported": "Cookie header pretvorjen v cookies.txt: {path}",
        "cookies_file_netscape_imported": "Netscape cookies datoteka normalizirana: {path}",
        "cookies_file_unsupported": "Ta cookies datoteka ni v podprtem formatu. Izberi Netscape/Mozilla cookies.txt ali JSON export iz browser extensiona.",
        "cookies_file_import_hint": "ApricotPlayer bo izbrano datoteko pretvoril v interno Netscape cookies.txt kopijo, ker yt-dlp zahteva ta format.",
        "cookies_file_selected": "Cookies file imported: {path}",
        "cookies_file_login_found": "YouTube login cookies found in imported cookies file.",
        "cookies_file_no_login_warning": "Imported cookies do not appear to contain YouTube login cookies. ApricotPlayer will still use them, but YouTube may keep asking you to sign in.",
        "cookies_file_load_failed": "Could not import selected cookies file: {error}",
    },
    "en": {
        "cookie_user_agent": "Browser User-Agent for cookies file",
        "cookies_file_imported": "Cookies imported and normalized: {path}",
        "cookies_file_json_imported": "JSON cookies converted to cookies.txt: {path}",
        "cookies_file_header_imported": "Cookie header converted to cookies.txt: {path}",
        "cookies_file_netscape_imported": "Netscape cookies file normalized: {path}",
        "cookies_file_unsupported": "This cookies file is not in a supported format. Choose a Netscape/Mozilla cookies.txt file or a JSON export from a browser extension.",
        "cookies_file_import_hint": "ApricotPlayer converts the selected file into its own Netscape cookies.txt copy because yt-dlp requires that format.",
        "cookies_file_selected": "Cookies file imported: {path}",
        "cookies_file_login_found": "YouTube login cookies found in imported cookies file.",
        "cookies_file_no_login_warning": "Imported cookies do not appear to contain YouTube login cookies. ApricotPlayer will still use them, but YouTube may keep asking you to sign in.",
        "cookies_file_load_failed": "Could not import selected cookies file: {error}",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_0812_TRANSLATION_UPDATES.get(language_code, RELEASE_0812_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_0812_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_0817_TRANSLATION_UPDATES = {
    "sl": {
        "show_advanced_network_settings": "Prikazi napredne nastavitve za omrezje in prenose",
        "cookie_user_agent": "Cookie User-Agent override (napredno, pusti prazno razen ce cookies ne delajo)",
        "rate_limit": "Omejitev hitrosti prenosov (prazno pomeni brez omejitve; primeri: 500K, 2M)",
        "ffmpeg": "Custom FFmpeg path (napredno, pusti prazno za vgrajeni FFmpeg)",
        "fragments": "Socasni download fragmenti (4 priporoceno)",
    },
    "en": {
        "show_advanced_network_settings": "Show advanced network and download settings",
        "cookie_user_agent": "Cookie User-Agent override (advanced, leave empty unless cookies fail)",
        "rate_limit": "Download speed limit (empty means unlimited; examples: 500K, 2M)",
        "ffmpeg": "Custom FFmpeg path (advanced, leave empty to use bundled FFmpeg)",
        "fragments": "Concurrent download fragments (4 recommended)",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_0817_TRANSLATION_UPDATES.get(language_code, RELEASE_0817_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_0817_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_0818_TRANSLATION_UPDATES = {
    "sl": {
        "shortcut_add_favorite": "Dodaj med priljubljene",
        "shortcut_remove_favorite": "Odstrani iz priljubljenih",
        "remove_favorite": "Odstrani iz priljubljenih",
        "results_details": "{title}. Trajanje: {duration}. Kanal: {channel}. Ogledi: {views}. {age}. Tip: {type}.",
        "playlist_result_details": "{title}. Playlist. {count}.",
        "channel_result_details": "{title}. Kanal.",
        "playlist_video_count": "{count} videov",
        "not_in_favorites": "Ta element ni med priljubljenimi.",
        "not_in_playlist": "Ta element ni v nobeni playlisti.",
        "unknown": "neznano",
    },
    "en": {
        "shortcut_add_favorite": "Add to favorites",
        "shortcut_remove_favorite": "Remove from favorites",
        "remove_favorite": "Remove from favorites",
        "results_details": "{title}. Duration: {duration}. Channel: {channel}. Views: {views}. {age}. Type: {type}.",
        "playlist_result_details": "{title}. Playlist. {count}.",
        "channel_result_details": "{title}. Channel.",
        "playlist_video_count": "{count} videos",
        "not_in_favorites": "This item is not in favorites.",
        "not_in_playlist": "This item is not in any playlist.",
        "unknown": "unknown",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_0818_TRANSLATION_UPDATES.get(language_code, RELEASE_0818_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_0818_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_0820_TRANSLATION_UPDATES = {
    "sl": {
        "local_file_result_details": "{title}. Lokalna datoteka. Format: {format}. Mapa: {folder}. Pot: {path}.",
        "local_file_result_line": "{title} | Lokalna datoteka | Format: {format} | Mapa: {folder}",
        "file_format_unknown": "neznan format",
    },
    "en": {
        "local_file_result_details": "{title}. Local file. Format: {format}. Folder: {folder}. Path: {path}.",
        "local_file_result_line": "{title} | Local file | Format: {format} | Folder: {folder}",
        "file_format_unknown": "unknown format",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_0820_TRANSLATION_UPDATES.get(language_code, RELEASE_0820_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_0820_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)

RELEASE_0823_TRANSLATION_UPDATES = {
    "sl": {
        "unsubscribe_channel": "Odjavi se od kanala",
        "shortcut_unsubscribe_channel": "Odjavi se od kanala",
        "subscription_not_found": "Nisi naročen na {title}.",
    },
    "en": {
        "unsubscribe_channel": "Unsubscribe from channel",
        "shortcut_unsubscribe_channel": "Unsubscribe from channel",
        "subscription_not_found": "You are not subscribed to {title}.",
    },
}
for language_code in LANGUAGE_CODES:
    TEXT.setdefault(language_code, {}).update(RELEASE_0823_TRANSLATION_UPDATES.get(language_code, RELEASE_0823_TRANSLATION_UPDATES["sl" if language_code == "sl" else "en"]))
for language_code in LANGUAGE_CODES:
    for key, value in RELEASE_0823_TRANSLATION_UPDATES["en"].items():
        TEXT.setdefault(language_code, {}).setdefault(key, value)


def default_equalizer_gains() -> dict[str, float]:
    return {band_id: 0.0 for band_id, _label in EQ_BANDS}


def equalizer_gains_from_values(values: list[float] | tuple[float, ...]) -> dict[str, float]:
    gains: dict[str, float] = {}
    for index, (band_id, _label) in enumerate(EQ_BANDS):
        try:
            value = float(values[index])
        except Exception:
            value = 0.0
        gains[band_id] = round(max(-24.0, min(24.0, value)), 1)
    return gains


def default_equalizer_preset_gains() -> dict[str, dict[str, float]]:
    presets = {preset_id: equalizer_gains_from_values(values) for preset_id, values in EQ_FACTORY_PRESET_VALUES.items()}
    for custom_id in EQ_CUSTOM_PRESET_IDS:
        presets[custom_id] = default_equalizer_gains()
    return presets


def default_equalizer_custom_names() -> dict[str, str]:
    return {custom_id: f"Custom {index}" for index, custom_id in enumerate(EQ_CUSTOM_PRESET_IDS, start=1)}


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
    announce_play_pause: bool = True
    announce_playback_finished: bool = True
    enable_background_playback: bool = False
    player_speed: str = "1.0"
    speed_audio_mode: str = SPEED_AUDIO_MODE_RUBBERBAND
    show_video_details_by_default: bool = False
    direct_link_enter_action: str = DIRECT_LINK_ENTER_PLAY
    enable_age_restricted_videos: bool = False
    enable_stream_cache: bool = True
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
    ask_download_location_each_time: bool = False
    quiet_downloads: bool = False
    keep_playlist_order: bool = True
    filename_template: str = DEFAULT_FILENAME_TEMPLATE
    audio_quality: str = "0"
    seek_seconds: int = 5
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
    auto_update_ytdlp: bool = True
    auto_update_app: bool = True
    app_update_interval_hours: float = 6.0
    app_update_notifications: bool = True
    skipped_update_version: str = ""
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


class ApricotTaskBarIcon(wx.adv.TaskBarIcon):
    def __init__(self, frame: "MainFrame") -> None:
        super().__init__()
        self.frame = frame
        self.show_id = wx.NewIdRef()
        self.settings_id = wx.NewIdRef()
        self.check_id = wx.NewIdRef()
        self.exit_id = wx.NewIdRef()
        for event_name in ("EVT_TASKBAR_LEFT_UP", "EVT_TASKBAR_LEFT_DCLICK"):
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
    def __init__(self, start_hidden_in_tray: bool = False) -> None:
        super().__init__(None, title=WINDOW_TITLE, size=(950, 680))
        APP_DIR.mkdir(parents=True, exist_ok=True)
        self.started_hidden_in_tray = start_hidden_in_tray
        settings_file_existed = SETTINGS_FILE.exists()
        self.first_run_without_settings = not SETTINGS_FILE.exists() and not LEGACY_SETTINGS_FILE.exists()
        self.settings_migrated = False
        self.settings_loaded_from_path: Path | None = None
        self.settings_load_errors: list[str] = []
        self.settings_save_blocked = False
        self.settings = self.load_settings()
        if not settings_file_existed or self.settings_migrated:
            self.save_settings()
        self.favorites = self.load_favorites()
        self.history = self.load_history()
        self.subscriptions = self.load_subscriptions()
        self.rss_feeds = self.load_rss_feeds()
        self.user_playlists = self.load_user_playlists()
        self.notifications = self.load_notifications()
        self.playback_positions = self.load_playback_positions()
        self.playback_queue = self.load_playback_queue()
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
        self.last_trending_country_index = 0
        self.last_trending_category_index = 0
        self.search_screen_active = False
        self.trending_screen_active = False
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
        self.folder_screen_active = False
        self.in_main_menu = False
        self.current_rss_feed_index = -1
        self.current_user_playlist_index = -1
        self.player_return_screen = ""
        self.player_return_data: dict = {}
        self.search_results_stack: list[dict] = []
        self.settings_section_index = 0
        self.current_index = -1
        self.deferred_result_line_updates: set[int] = set()
        self.player_process: subprocess.Popen | None = None
        self.player_log_handle = None
        self.player_kind = ""
        self.player_control_mode = False
        self.volume_boost_enabled = False
        self.rubberband_pitch_filter_active = False
        self.in_player_screen = False
        self.in_queue_screen = False
        self.repeat_current = False
        self.shuffle_current = False
        self.session_volume: float | None = None
        self.player_generation = 0
        self.player_ended = False
        self.player_paused = False
        self.current_video_item: dict | None = None
        self.current_video_info: dict = {}
        self.player_panel: wx.Panel | None = None
        self.player_fullscreen_session = False
        self.player_fullscreen_results_override = False
        self.manual_background_playback_active = False
        self.player_navigation_controls = []
        self.player_action_controls = []
        self.player_escape_stop_controls = []
        self.fullscreen_checkbox: wx.CheckBox | None = None
        self.details_label: wx.StaticText | None = None
        self.video_details: wx.TextCtrl | None = None
        self.details_button_sizer: wx.Sizer | None = None
        self.background_player_controls: list[wx.Window] = []
        self.player_play_pause_buttons: list[wx.Button] = []
        self.background_player_section_added = False
        self.download_queue: dict[str, dict] = {}
        self.active_downloads: dict[str, dict] = {}
        self.download_cancel_events: dict[str, threading.Event] = {}
        self.download_task_counter = 0
        self.queue_items: list[dict] = []
        self.last_download_shortcut: tuple[str, str, float] = ("", "", 0.0)
        self.ipc_path: str | None = None
        self.mpv_ipc_lock = threading.Lock()
        self.cookie_repair_lock = threading.Lock()
        self.cookie_repair_suppressed_until = 0.0
        self.ui_queue: queue.Queue[tuple[str, object]] = queue.Queue()
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.current_search_type_code = "All"
        self.collection_url = ""
        self.collection_result_type = ""
        self.current_stream_url = ""
        self.current_stream_headers: dict = {}
        self.current_audio_device = ""
        self.session_audio_output_device = ""
        self.edit_mode_enabled = False
        self.session_equalizer_enabled: bool | None = None
        self.session_equalizer_gains: dict[str, float] = {}
        self.session_equalizer_before_bass_boost: tuple[bool | None, dict[str, float]] | None = None
        self.bass_boost_enabled = False
        self.equalizer_filter_active = False
        self.clip_start_marker: float | None = None
        self.clip_end_marker: float | None = None
        self.audio_device_options_cache: tuple[float, list[str], list[str]] | None = None
        self.audio_device_refresh_running = False
        self.metadata_hydration_urls: set[str] = set()
        self.search_generation = 0
        self.local_folder_cache: dict[str, list[dict]] = {}
        self.last_activation_check = 0.0
        self.settings_render_generation = 0
        self.settings_pending_section_index = -1
        self.settings_controls_applied_for_pending = False
        self.shortcut_editor_values: dict[str, str] = {}
        self.shortcut_editor_actions: list[str] = []
        self.shortcut_editor_current_action = ""
        self.details_opened_temporarily = False
        self.nvda_client = self.load_nvda_client()
        self.update_progress_dialog: wx.ProgressDialog | None = None
        self.app_update_check_running = False
        self.pending_app_update_release: dict | None = None
        self.pending_app_update_asset: dict | None = None
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
        self.app_update_timer = wx.Timer(self)
        self.Bind(wx.EVT_TIMER, self.on_app_update_timer, self.app_update_timer)
        self.configure_app_update_timer()
        if self.settings.auto_update_ytdlp:
            wx.CallLater(3500, self.start_ytdlp_update_check)
        if self.settings.auto_update_app:
            wx.CallLater(5500, self.start_app_update_check)
        if self.settings.subscription_check_enabled:
            wx.CallLater(8500, self.check_subscriptions_if_due)
        if self.settings.enable_podcasts_rss and self.settings.rss_refresh_on_startup and self.rss_feeds:
            wx.CallLater(9500, self.refresh_all_rss_feeds_background)
        if not self.started_hidden_in_tray:
            wx.CallLater(6500, self.check_saved_audio_device_available)
            wx.CallLater(7200, self.maybe_prompt_media_association_registration)
        if self.settings.start_with_windows:
            wx.CallLater(1800, self.sync_windows_startup_registration)
        if self.first_run_without_settings and not self.settings.language_prompted and not self.started_hidden_in_tray:
            wx.CallAfter(self.prompt_initial_language)

    def install_download_accelerators(self) -> None:
        self.global_accelerator_ids: dict[str, wx.WindowIDRef] = {}
        global_actions = [
            ("open_main_menu", self.open_main_menu_shortcut),
            ("open_search", self.open_search_shortcut),
            ("open_play_from_folder", self.open_play_from_folder_shortcut),
            ("open_direct_link", self.open_direct_link_shortcut),
            ("open_favorites", self.open_favorites_shortcut),
            ("open_playlists", self.open_playlists_shortcut),
            ("open_subscriptions", self.open_subscriptions_shortcut),
            ("open_current_downloads", self.open_current_downloads_shortcut),
            ("open_history", self.open_history_shortcut),
            ("open_podcasts_rss", self.open_podcasts_rss_shortcut),
            ("open_settings", self.open_settings_shortcut),
            ("open_playback_queue", self.open_playback_queue_shortcut),
            ("background_play_pause", self.background_play_pause_shortcut),
            ("download_audio", self.download_audio_shortcut),
            ("download_video", self.download_video_shortcut),
            ("subscribe_channel", self.subscribe_shortcut),
            ("unsubscribe_channel", self.unsubscribe_shortcut),
            ("new_subscription_videos", self.open_notification_center_shortcut),
        ]
        for action, handler in global_actions:
            menu_id = wx.NewIdRef()
            self.global_accelerator_ids[action] = menu_id
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), id=int(menu_id))
        entries = []
        for action, menu_id in self.global_accelerator_ids.items():
            if self.shortcut_is_plain_printable(action):
                continue
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
            "[": VK_OEM_4_LEFT_BRACKET,
            "leftbracket": VK_OEM_4_LEFT_BRACKET,
            "left bracket": VK_OEM_4_LEFT_BRACKET,
            "openbracket": VK_OEM_4_LEFT_BRACKET,
            "open bracket": VK_OEM_4_LEFT_BRACKET,
            "physical left bracket": VK_OEM_4_LEFT_BRACKET,
            "]": VK_OEM_6_RIGHT_BRACKET,
            "rightbracket": VK_OEM_6_RIGHT_BRACKET,
            "right bracket": VK_OEM_6_RIGHT_BRACKET,
            "closebracket": VK_OEM_6_RIGHT_BRACKET,
            "close bracket": VK_OEM_6_RIGHT_BRACKET,
            "physical right bracket": VK_OEM_6_RIGHT_BRACKET,
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
            VK_OEM_4_LEFT_BRACKET: "LeftBracket",
            VK_OEM_6_RIGHT_BRACKET: "RightBracket",
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
        raw_key_code = 0
        try:
            raw_key_code = int(event.GetRawKeyCode())
        except Exception:
            raw_key_code = 0
        if raw_key_code in (VK_OEM_4_LEFT_BRACKET, VK_OEM_6_RIGHT_BRACKET):
            key_name = self.shortcut_name_for_key_code(raw_key_code, event.GetUnicodeKey())
        else:
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
        action = str(getattr(control, "_apricot_shortcut_action", "") or "")
        if (
            event.GetKeyCode() == getattr(wx, "WXK_SPACE", ord(" "))
            and not event.ControlDown()
            and not event.ShiftDown()
            and not event.AltDown()
            and action != "player_play_pause"
        ):
            return
        shortcut = self.shortcut_from_key_event(event)
        if not shortcut:
            event.Skip()
            return
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
        if action:
            self.shortcut_editor_values[action] = shortcut
            self.update_shortcut_action_label(action)
        self.speak_text(self.t("shortcut_captured", shortcut=shortcut))

    def shortcut_label_key(self, wanted_action: str) -> str:
        for action, label_key in SHORTCUT_DEFINITIONS:
            if action == wanted_action:
                return label_key
        return wanted_action

    def shortcut_display_label(self, action: str, shortcut: str) -> str:
        label = self.t(self.shortcut_label_key(action))
        return f"{label}: {shortcut or DEFAULT_KEYBOARD_SHORTCUTS.get(action, '')}"

    def sync_shortcut_editor_value(self) -> None:
        if not hasattr(self, "controls"):
            return
        control = self.controls.get("shortcut_active_value")
        if not isinstance(control, wx.TextCtrl):
            return
        action = str(getattr(control, "_apricot_shortcut_action", "") or self.shortcut_editor_current_action)
        if action:
            self.shortcut_editor_current_action = action
            self.shortcut_editor_values[action] = control.GetValue().strip() or DEFAULT_KEYBOARD_SHORTCUTS.get(action, "")

    def update_shortcut_action_label(self, action: str) -> None:
        control = self.controls.get("shortcut_action_list") if hasattr(self, "controls") else None
        if not isinstance(control, wx.ListBox) or action not in self.shortcut_editor_actions:
            return
        index = self.shortcut_editor_actions.index(action)
        try:
            control.SetString(index, self.shortcut_display_label(action, self.shortcut_editor_values.get(action, "")))
        except Exception:
            pass

    def on_shortcut_action_selected(self, _event) -> None:
        self.sync_shortcut_editor_value()
        list_control = self.controls.get("shortcut_action_list") if hasattr(self, "controls") else None
        value_control = self.controls.get("shortcut_active_value") if hasattr(self, "controls") else None
        if not isinstance(list_control, wx.ListBox) or not isinstance(value_control, wx.TextCtrl):
            return
        index = list_control.GetSelection()
        if not (0 <= index < len(self.shortcut_editor_actions)):
            return
        action = self.shortcut_editor_actions[index]
        self.shortcut_editor_current_action = action
        shortcut = self.shortcut_editor_values.get(action) or DEFAULT_KEYBOARD_SHORTCUTS.get(action, "")
        value_control.ChangeValue(shortcut)
        value_control.SetName(f"{self.t('shortcut_value')}. {self.t(self.shortcut_label_key(action))}. {self.t('shortcut_capture_hint')}")
        setattr(value_control, "_apricot_shortcut_action", action)

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
            if "shortcut_action_list" in self.controls and "shortcut_active_value" in self.controls:
                self.sync_shortcut_editor_value()
                values.update(self.shortcut_editor_values)
            else:
                for action, _label_key in SHORTCUT_DEFINITIONS:
                    control = self.controls.get(f"shortcut_{action}")
                    if isinstance(control, wx.TextCtrl):
                        values[action] = control.GetValue().strip() or DEFAULT_KEYBOARD_SHORTCUTS[action]
        return values

    def validate_shortcut_controls(self) -> bool:
        has_shortcut_editor = hasattr(self, "controls") and "shortcut_action_list" in self.controls and "shortcut_active_value" in self.controls
        has_legacy_controls = hasattr(self, "controls") and any(f"shortcut_{action}" in self.controls for action, _label_key in SHORTCUT_DEFINITIONS)
        if not has_shortcut_editor and not has_legacy_controls:
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
                if has_shortcut_editor:
                    list_control = self.controls.get("shortcut_action_list")
                    value_control = self.controls.get("shortcut_active_value")
                    if isinstance(list_control, wx.ListBox) and action in self.shortcut_editor_actions:
                        list_control.SetSelection(self.shortcut_editor_actions.index(action))
                        self.on_shortcut_action_selected(None)
                    if isinstance(value_control, wx.TextCtrl):
                        self.safe_set_focus(value_control)
                else:
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

    def shortcut_is_plain_printable(self, action: str) -> bool:
        parsed = self.parse_shortcut(self.shortcut_for(action))
        if not parsed:
            return False
        ctrl, shift, alt, key_name = parsed
        return not ctrl and not alt and len(key_name.strip()) == 1 and key_name.strip().isprintable()

    @staticmethod
    def focus_accepts_text(focus: wx.Window | None) -> bool:
        if focus is None:
            return False
        try:
            return isinstance(focus, wx.TextCtrl) and not bool(focus.GetWindowStyleFlag() & wx.TE_READONLY)
        except Exception:
            return False

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
        if wx.WXK_F1 <= key_code <= wx.WXK_F24:
            raw_vk = 0x70 + (key_code - wx.WXK_F1)
            return self.event_key_code(event) == key_code or self.event_raw_key_code(event) == raw_vk
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

    def ydl_options(self, options: dict | None = None, use_cookies: bool = False, use_js_solver: bool = False) -> dict:
        disable_external_ytdlp_plugins()
        merged = {
            "logger": YTDLP_LOGGER,
            "no_warnings": True,
        }
        if use_js_solver:
            merged["js_runtimes"] = self.ytdlp_js_runtimes()
            if not self.ytdlp_ejs_available():
                merged["remote_components"] = ["ejs:github"]
        if options:
            merged.update(options)
        cookiefile = str(merged.get("cookiefile") or "").strip()
        if use_cookies and not cookiefile:
            cookiefile = self.effective_cookies_file()
        if cookiefile:
            merged["cookiefile"] = str(Path(os.path.expandvars(cookiefile.strip('"'))).expanduser())
            cookie_user_agent = str(getattr(self.settings, "cookie_user_agent", "") or "").strip()
            if cookie_user_agent:
                headers = dict(merged.get("http_headers") or {})
                headers["User-Agent"] = cookie_user_agent
                merged["http_headers"] = headers
        return merged

    @staticmethod
    def ytdlp_ejs_available() -> bool:
        try:
            import_module("yt_dlp_ejs")
            return True
        except Exception:
            return False

    def bundled_node_executable(self) -> str:
        candidates = [
            self.bundled_path("node", "node.exe"),
            Path(__file__).resolve().parent / "vendor" / "node" / "node.exe",
        ]
        for candidate in candidates:
            if candidate.exists():
                return str(candidate)
        node = shutil.which("node")
        return node or ""

    def ytdlp_js_runtimes(self) -> dict:
        node = self.bundled_node_executable()
        if node:
            return {"node": {"path": node}}
        return {"deno": {}, "node": {}, "quickjs": {}, "bun": {}}

    def effective_cookies_file(self) -> str:
        configured = str(getattr(self.settings, "cookies_file", "") or "").strip()
        if configured:
            configured_path = Path(os.path.expandvars(configured.strip('"'))).expanduser()
            try:
                same_as_cache = configured_path.resolve() == CACHED_COOKIES_FILE.resolve()
            except OSError:
                same_as_cache = False
            attempts = getattr(self, "_cookies_file_auto_import_attempts", None)
            if attempts is None:
                attempts = set()
                self._cookies_file_auto_import_attempts = attempts
            attempt_key = str(configured_path)
            if not same_as_cache and attempt_key not in attempts and configured_path.exists():
                attempts.add(attempt_key)
                try:
                    result = self.import_cookie_file_to_cache(configured_path)
                    self.settings.cookies_file = str(result["path"])
                    self.settings.cookies_from_browser = "none"
                    self.settings.cookies_browser_profile = COOKIE_PROFILE_AUTO
                    self.save_settings()
                    return str(result["path"])
                except Exception:
                    pass
            return str(configured_path)
        try:
            if CACHED_COOKIES_FILE.exists() and CACHED_COOKIES_FILE.stat().st_size > 0:
                return str(CACHED_COOKIES_FILE)
        except OSError:
            pass
        return ""

    @staticmethod
    def is_youtube_url(url: str) -> bool:
        try:
            host = (urlparse(str(url or "")).netloc or "").lower()
        except Exception:
            return False
        return "youtube.com" in host or "youtu.be" in host

    def cookies_file_has_youtube_login(self, path: str) -> bool:
        if not path:
            return False
        cookie_path = Path(path)
        try:
            stat = cookie_path.stat()
        except OSError:
            return False
        cache = getattr(self, "_youtube_cookie_login_cache", None)
        if cache is None:
            cache = {}
            self._youtube_cookie_login_cache = cache
        key = (str(cookie_path), stat.st_mtime_ns, stat.st_size)
        if key in cache:
            return bool(cache[key])
        try:
            _score, _youtube_count, _total_count, has_login = self.cookie_file_score(cookie_path)
        except Exception:
            has_login = False
        cache.clear()
        cache[key] = bool(has_login)
        return bool(has_login)

    def playback_cookies_file_for_url(self, url: str) -> str:
        cookie_file = self.effective_cookies_file()
        if not cookie_file:
            return ""
        if not self.is_youtube_url(url):
            return cookie_file
        return cookie_file if self.cookies_file_has_youtube_login(cookie_file) else ""

    def cookie_file_score(self, path: str | Path) -> tuple[int, int, int, bool]:
        jar = http.cookiejar.MozillaCookieJar()
        jar.load(str(path), ignore_discard=True, ignore_expires=True)
        score, youtube_count, total_count = self.cookie_jar_score(jar)
        return score, youtube_count, total_count, self.cookie_jar_has_login_cookies(jar)

    @staticmethod
    def decode_cookie_file_bytes(data: bytes) -> str:
        for encoding in ("utf-8-sig", "utf-8", "cp1252"):
            try:
                return data.decode(encoding)
            except UnicodeDecodeError:
                continue
        return data.decode("utf-8", errors="replace")

    @staticmethod
    def cookie_bool(value) -> bool:
        if isinstance(value, bool):
            return value
        if isinstance(value, (int, float)):
            return bool(value)
        return str(value or "").strip().lower() in {"1", "true", "yes", "y", "on"}

    @staticmethod
    def cookie_expiry(value) -> int | None:
        if value in (None, "", -1, "-1", 0, "0"):
            return None
        try:
            expires = float(value)
        except (TypeError, ValueError):
            return None
        if expires > 10_000_000_000:
            expires /= 1000.0
        if expires <= 0:
            return None
        return int(expires)

    @staticmethod
    def cookie_default_domain_from_text(text: str) -> str:
        text = str(text or "").strip()
        if not text:
            return ""
        if "://" not in text and "." in text:
            return text.split("/", 1)[0]
        try:
            parsed = urlparse(text)
            return parsed.netloc or parsed.path.split("/", 1)[0]
        except Exception:
            return ""

    @staticmethod
    def looks_like_cookie_domain_key(key: str) -> bool:
        key = str(key or "").strip()
        if not key or len(key) > 120 or " " in key:
            return False
        if key.startswith("."):
            key = key[1:]
        return "." in key and "/" not in key and "\\" not in key

    def cookie_from_mapping(self, item: dict, default_domain: str = "") -> http.cookiejar.Cookie | None:
        name = str(item.get("name") or item.get("Name") or item.get("key") or "").strip()
        if not name:
            return None
        value = item.get("value")
        if value is None:
            value = item.get("Value")
        if value is None:
            value = ""
        domain = str(
            item.get("domain")
            or item.get("Domain")
            or item.get("host")
            or item.get("host_key")
            or item.get("hostKey")
            or default_domain
            or ""
        ).strip()
        if domain.startswith("#HttpOnly_"):
            domain = domain[len("#HttpOnly_") :]
        if "://" in domain:
            domain = self.cookie_default_domain_from_text(domain)
        if not domain:
            return None
        path = str(item.get("path") or item.get("Path") or "/")
        expires = None
        for key in ("expirationDate", "expiration_date", "expires", "expiry", "expiration", "Expiry"):
            if key in item:
                expires = self.cookie_expiry(item.get(key))
                break
        http_only = self.cookie_bool(item.get("httpOnly") if "httpOnly" in item else item.get("http_only"))
        secure = self.cookie_bool(item.get("secure"))
        return http.cookiejar.Cookie(
            version=0,
            name=name,
            value=str(value),
            port=None,
            port_specified=False,
            domain=domain,
            domain_specified=True,
            domain_initial_dot=domain.startswith("."),
            path=path or "/",
            path_specified=True,
            secure=secure,
            expires=expires,
            discard=expires is None,
            comment=None,
            comment_url=None,
            rest={"HttpOnly": None} if http_only else {},
            rfc2109=False,
        )

    def iter_cookie_json_items(self, data, default_domain: str = ""):
        if isinstance(data, list):
            for item in data:
                yield from self.iter_cookie_json_items(item, default_domain)
            return
        if not isinstance(data, dict):
            return
        own_default = (
            self.cookie_default_domain_from_text(str(data.get("url") or data.get("host") or data.get("domain") or ""))
            or default_domain
        )
        if any(key in data for key in ("name", "Name", "key")) and any(key in data for key in ("value", "Value")):
            yield data, own_default
        for key, value in data.items():
            child_default = own_default
            if self.looks_like_cookie_domain_key(key):
                child_default = key
            if isinstance(value, (list, dict)):
                yield from self.iter_cookie_json_items(value, child_default)

    def cookie_jar_from_json_data(self, data) -> http.cookiejar.MozillaCookieJar:
        jar = http.cookiejar.MozillaCookieJar()
        seen: set[tuple[str, str, str]] = set()
        for item, default_domain in self.iter_cookie_json_items(data):
            cookie = self.cookie_from_mapping(item, default_domain)
            if not cookie:
                continue
            key = (cookie.domain, cookie.path, cookie.name)
            if key in seen:
                continue
            seen.add(key)
            jar.set_cookie(cookie)
        return jar

    @staticmethod
    def looks_like_netscape_cookie_text(text: str) -> bool:
        lowered = text[:500].lower()
        if "# netscape http cookie file" in lowered or "# http cookie file" in lowered:
            return True
        for raw_line in text.splitlines():
            line = raw_line.strip()
            if not line:
                continue
            if line.startswith("#") and not line.startswith("#HttpOnly_"):
                continue
            if len(line.split("\t")) >= 7:
                return True
            if len(re.split(r"\s+", line, maxsplit=6)) >= 7:
                return True
        return False

    @staticmethod
    def normalized_netscape_cookie_text(text: str) -> str:
        lines: list[str] = []
        has_header = False
        for raw_line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n"):
            line = raw_line.lstrip("\ufeff")
            lowered = line.lower()
            if lowered.startswith("# netscape http cookie file") or lowered.startswith("# http cookie file"):
                has_header = True
            stripped = line.strip()
            if stripped and not stripped.startswith("#") and "\t" not in stripped:
                parts = re.split(r"\s+", stripped, maxsplit=6)
                if len(parts) >= 7:
                    line = "\t".join(parts[:7])
            lines.append(line.rstrip("\n"))
        if not has_header:
            lines.insert(0, "# Netscape HTTP Cookie File")
            lines.insert(1, "# This file was normalized by ApricotPlayer.")
        return "\n".join(lines).rstrip() + "\n"

    def cookie_jar_from_netscape_text(self, text: str) -> http.cookiejar.MozillaCookieJar:
        normalized = self.normalized_netscape_cookie_text(text)
        temp_path = CACHED_COOKIES_FILE.with_suffix(".import.tmp")
        temp_path.parent.mkdir(parents=True, exist_ok=True)
        temp_path.write_text(normalized, encoding="utf-8", newline="\n")
        try:
            jar = http.cookiejar.MozillaCookieJar()
            jar.load(str(temp_path), ignore_discard=True, ignore_expires=True)
            return jar
        finally:
            try:
                temp_path.unlink()
            except OSError:
                pass

    def cookie_jar_from_header_text(self, text: str) -> http.cookiejar.MozillaCookieJar:
        combined = " ".join(line.strip() for line in text.splitlines() if line.strip() and not line.lstrip().startswith("#"))
        if not combined:
            raise RuntimeError(self.t("cookies_file_unsupported"))
        if combined.lower().startswith("cookie:"):
            combined = combined.split(":", 1)[1].strip()
        if "=" not in combined or ";" not in combined:
            raise RuntimeError(self.t("cookies_file_unsupported"))
        jar = http.cookiejar.MozillaCookieJar()
        ignored = {"path", "expires", "max-age", "secure", "httponly", "samesite", "domain", "priority"}
        for part in combined.split(";"):
            if "=" not in part:
                continue
            name, value = part.split("=", 1)
            name = name.strip()
            if not name or name.lower() in ignored:
                continue
            cookie = self.cookie_from_mapping({"name": name, "value": value.strip(), "domain": ".youtube.com", "path": "/"})
            if cookie:
                jar.set_cookie(cookie)
        return jar

    @staticmethod
    def cookie_jar_total(cookie_jar) -> int:
        return sum(1 for _cookie in cookie_jar)

    def save_cookie_jar_to_cache(self, cookie_jar) -> None:
        CACHED_COOKIES_FILE.parent.mkdir(parents=True, exist_ok=True)
        temp_path = CACHED_COOKIES_FILE.with_suffix(".txt.tmp")
        cookie_jar.save(str(temp_path), ignore_discard=True, ignore_expires=True)
        os.replace(temp_path, CACHED_COOKIES_FILE)

    def import_cookie_file_to_cache(self, source_path: str | Path) -> dict:
        source = Path(source_path)
        text = self.decode_cookie_file_bytes(source.read_bytes())
        import_kind = "netscape"
        jar: http.cookiejar.MozillaCookieJar | None = None
        stripped = text.lstrip()
        if stripped.startswith("{") or stripped.startswith("["):
            try:
                jar = self.cookie_jar_from_json_data(json.loads(text))
                import_kind = "json"
            except json.JSONDecodeError:
                jar = None
        if jar is None and self.looks_like_netscape_cookie_text(text):
            jar = self.cookie_jar_from_netscape_text(text)
            import_kind = "netscape"
        if jar is None:
            jar = self.cookie_jar_from_header_text(text)
            import_kind = "header"
        total_count = self.cookie_jar_total(jar)
        if total_count <= 0:
            raise RuntimeError(self.t("cookies_file_unsupported"))
        self.save_cookie_jar_to_cache(jar)
        score, youtube_count, total_count = self.cookie_jar_score(jar)
        return {
            "path": str(CACHED_COOKIES_FILE),
            "kind": import_kind,
            "score": score,
            "youtube_count": youtube_count,
            "total_count": total_count,
            "has_login": self.cookie_jar_has_login_cookies(jar),
        }

    def normalized_cookies_browser(self) -> str:
        browser = str(getattr(self.settings, "cookies_from_browser", "none") or "none").strip().lower()
        return "" if browser == "none" else browser

    def age_restricted_video_support_enabled(self) -> bool:
        return bool(getattr(self.settings, "enable_age_restricted_videos", False))

    def friendly_error(self, exc: Exception | str) -> str:
        text = str(exc)
        lowered = text.lower()
        if "failed to decrypt with dpapi" in lowered or "nonetype" in lowered and "decode" in lowered:
            return f"{text}\n\n{self.t('cookie_copy_hint')}"
        if "could not copy" in lowered and "cookie" in lowered and "database" in lowered:
            return f"{text}\n\n{self.t('cookie_copy_hint')}"
        if "sign in to confirm" in lowered or "not a bot" in lowered or "cookies-from-browser" in lowered:
            return f"{text}\n\n{self.t('youtube_auth_hint')}"
        return text

    def is_cookie_auth_error(self, exc: Exception | str) -> bool:
        lowered = str(exc).lower()
        checks = (
            "sign in to confirm",
            "not a bot",
            "confirm you're not a bot",
            "confirm you are not a bot",
            "cookies-from-browser",
            "failed to load cookies",
            "could not copy chrome cookie database",
            "no youtube login cookies",
            "cookies were exported, but no youtube login cookies",
            "failed to decrypt with dpapi",
            "object has no attribute 'decode'",
            "login required",
            "this video may be inappropriate",
        )
        return any(check in lowered for check in checks)

    def is_age_or_js_playback_error(self, exc: Exception | str) -> bool:
        lowered = str(exc).lower()
        checks = (
            "requested format is not available",
            "no video formats found",
            "nsig extraction failed",
            "signature extraction failed",
            "n challenge",
            "age restricted",
            "age-restricted",
            "this video may be inappropriate",
            "only available to registered users",
        )
        return any(check in lowered for check in checks)

    def is_requested_format_error(self, exc: Exception | str) -> bool:
        return "requested format is not available" in str(exc).lower()

    def ydl_extract_info(
        self,
        url: str,
        options: dict | None = None,
        download: bool = False,
        use_cookies: bool = False,
        use_js_solver: bool = False,
        allow_cookie_retry: bool = True,
    ) -> dict:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            raise RuntimeError(self.t("missing_ytdlp"))

        def run_once(run_with_cookies: bool = False):
            with ytdlp.YoutubeDL(self.ydl_options(options, use_cookies=run_with_cookies, use_js_solver=use_js_solver)) as ydl:
                return ydl.extract_info(url, download=download)

        try:
            return run_once(use_cookies)
        except Exception as exc:
            if not allow_cookie_retry or not self.is_cookie_auth_error(exc):
                raise
            retry_error: Exception | str = exc
            if not use_cookies and self.effective_cookies_file():
                try:
                    return run_once(True)
                except Exception as cookie_exc:
                    retry_error = cookie_exc
                    if not self.is_cookie_auth_error(cookie_exc):
                        raise
            if self.repair_cookies_for_error(retry_error):
                return run_once(True)
            raise retry_error if isinstance(retry_error, Exception) else exc

    def ydl_download_urls(self, urls: list[str], options: dict | None = None) -> None:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            raise RuntimeError(self.t("missing_ytdlp"))

        def run_once(use_cookies: bool = False) -> None:
            with ytdlp.YoutubeDL(self.ydl_options(options, use_cookies=use_cookies)) as ydl:
                ydl.download(urls)

        try:
            run_once()
        except Exception as exc:
            if not self.is_cookie_auth_error(exc):
                raise
            retry_error: Exception | str = exc
            if self.effective_cookies_file():
                try:
                    run_once(use_cookies=True)
                    return
                except Exception as cookie_exc:
                    retry_error = cookie_exc
                    if not self.is_cookie_auth_error(cookie_exc):
                        raise
            if self.repair_cookies_for_error(retry_error):
                run_once(use_cookies=True)
                return
            raise retry_error if isinstance(retry_error, Exception) else exc

    def repair_cookies_for_error(self, exc: Exception | str) -> bool:
        if not self.is_cookie_auth_error(exc):
            return False
        browser = self.normalized_cookies_browser()
        if not browser:
            return False
        if time.monotonic() < self.cookie_repair_suppressed_until:
            return False
        if not self.cookie_repair_lock.acquire(blocking=False):
            with self.cookie_repair_lock:
                return bool(self.effective_cookies_file())
        try:
            self.ui_queue.put(("announce", self.t("cookie_auto_refresh_start", browser=browser.title())))
            try:
                result = self.export_browser_cookies_blocking(browser, allow_close=True)
            except Exception as export_exc:
                self.cookie_repair_suppressed_until = time.monotonic() + 300.0
                self.ui_queue.put(("announce", self.t("cookie_auto_refresh_failed", error=self.friendly_error(export_exc))))
                return False
            self.ui_queue.put(("announce", self.t("cookie_auto_refresh_done", profile=result.get("profile_label", self.t("browser_profile_auto")))))
            return True
        finally:
            self.cookie_repair_lock.release()

    def cookie_browser_root(self, browser: str) -> Path | None:
        browser = str(browser or "").lower()
        local = Path(os.getenv("LOCALAPPDATA", ""))
        roaming = Path(os.getenv("APPDATA", ""))
        roots = {
            "brave": local / "BraveSoftware" / "Brave-Browser" / "User Data",
            "chrome": local / "Google" / "Chrome" / "User Data",
            "chromium": local / "Chromium" / "User Data",
            "edge": local / "Microsoft" / "Edge" / "User Data",
            "vivaldi": local / "Vivaldi" / "User Data",
            "opera": roaming / "Opera Software" / "Opera Stable",
        }
        return roots.get(browser)

    @staticmethod
    def chromium_cookie_file(profile: Path) -> Path:
        network_cookie = profile / "Network" / "Cookies"
        return network_cookie if network_cookie.exists() else profile / "Cookies"

    def discover_cookie_profiles(self, browser: str) -> list[tuple[str, str]]:
        browser = str(browser or "").lower()
        profiles: list[tuple[str, str]] = []
        if browser == "firefox":
            roots = [
                Path(os.getenv("APPDATA", "")) / "Mozilla" / "Firefox" / "Profiles",
                Path(os.getenv("LOCALAPPDATA", "")) / "Packages" / "Mozilla.Firefox_n80bbvh6b1yt2" / "LocalCache" / "Roaming" / "Mozilla" / "Firefox" / "Profiles",
            ]
            for root in roots:
                if not root.exists():
                    continue
                for profile in root.iterdir():
                    if profile.is_dir() and (profile / "cookies.sqlite").exists():
                        profiles.append((profile.name, str(profile)))
            return sorted(profiles, key=lambda item: item[0].lower())
        root = self.cookie_browser_root(browser)
        if not root or not root.exists():
            return []
        if browser == "opera":
            if self.chromium_cookie_file(root).exists():
                return [(root.name, str(root))]
            return []
        candidates = []
        if self.chromium_cookie_file(root).exists():
            candidates.append(root)
        try:
            candidates.extend(path for path in root.iterdir() if path.is_dir() and self.chromium_cookie_file(path).exists())
        except OSError:
            pass

        def sort_key(path: Path) -> tuple[int, str]:
            name = path.name
            if name == "Default":
                return (0, name)
            match = re.fullmatch(r"Profile (\d+)", name)
            if match:
                return (1, f"{int(match.group(1)):04d}")
            return (2, name.lower())

        seen: set[str] = set()
        for profile in sorted(candidates, key=sort_key):
            value = profile.name if profile.parent == root and browser != "opera" else str(profile)
            if value in seen:
                continue
            seen.add(value)
            profiles.append((profile.name, value))
        return profiles

    def cookie_profile_choice_values(self, browser: str | None = None) -> list[str]:
        browser = browser or self.normalized_cookies_browser()
        values = [COOKIE_PROFILE_AUTO]
        values.extend(value for _label, value in self.discover_cookie_profiles(browser))
        selected = str(getattr(self.settings, "cookies_browser_profile", COOKIE_PROFILE_AUTO) or COOKIE_PROFILE_AUTO).strip()
        if selected and selected not in values:
            values.append(selected)
        return values

    def cookie_profile_choice_labels(self, values: list[str]) -> list[str]:
        labels = []
        for value in values:
            if value == COOKIE_PROFILE_AUTO:
                labels.append(self.t("browser_profile_auto"))
            elif os.path.isabs(value):
                labels.append(Path(value).name)
            else:
                labels.append(value)
        return labels

    @staticmethod
    def free_local_port() -> int:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.bind(("127.0.0.1", 0))
            return int(sock.getsockname()[1])

    def cookie_browser_executable(self, browser: str) -> str:
        browser = str(browser or "").lower()
        program_files = Path(os.getenv("ProgramFiles", r"C:\Program Files"))
        program_files_x86 = Path(os.getenv("ProgramFiles(x86)", r"C:\Program Files (x86)"))
        local = Path(os.getenv("LOCALAPPDATA", ""))
        candidates = {
            "brave": [
                program_files / "BraveSoftware" / "Brave-Browser" / "Application" / "brave.exe",
                program_files_x86 / "BraveSoftware" / "Brave-Browser" / "Application" / "brave.exe",
                local / "BraveSoftware" / "Brave-Browser" / "Application" / "brave.exe",
            ],
            "chrome": [
                program_files / "Google" / "Chrome" / "Application" / "chrome.exe",
                program_files_x86 / "Google" / "Chrome" / "Application" / "chrome.exe",
                local / "Google" / "Chrome" / "Application" / "chrome.exe",
            ],
            "edge": [
                program_files_x86 / "Microsoft" / "Edge" / "Application" / "msedge.exe",
                program_files / "Microsoft" / "Edge" / "Application" / "msedge.exe",
                local / "Microsoft" / "Edge" / "Application" / "msedge.exe",
            ],
            "chromium": [
                program_files / "Chromium" / "Application" / "chrome.exe",
                program_files_x86 / "Chromium" / "Application" / "chrome.exe",
                local / "Chromium" / "Application" / "chrome.exe",
            ],
            "opera": [
                local / "Programs" / "Opera" / "opera.exe",
                program_files / "Opera" / "opera.exe",
                program_files_x86 / "Opera" / "opera.exe",
            ],
            "vivaldi": [
                local / "Vivaldi" / "Application" / "vivaldi.exe",
                program_files / "Vivaldi" / "Application" / "vivaldi.exe",
                program_files_x86 / "Vivaldi" / "Application" / "vivaldi.exe",
            ],
        }.get(browser, [])
        for candidate in candidates:
            if candidate.exists():
                return str(candidate)
        return ""

    def chromium_profile_launch_args(self, browser: str, profile: str | None, headless: bool = True) -> tuple[str, list[str]]:
        root = self.cookie_browser_root(browser)
        if not root:
            raise RuntimeError(f"browser profile root not found for {browser}")
        profile_value = str(profile or "").strip()
        profile_dir = ""
        user_data_dir = root
        if profile_value and os.path.isabs(profile_value):
            profile_path = Path(profile_value)
            if profile_path.exists() and profile_path.parent.exists():
                user_data_dir = profile_path.parent
                profile_dir = profile_path.name
        elif profile_value:
            profile_dir = profile_value
        elif browser != "opera":
            profile_dir = "Default"
        args = [
            f"--user-data-dir={user_data_dir}",
            "--remote-allow-origins=*",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-features=LockProfileCookieDatabase",
        ]
        if headless:
            args.append("--headless=new")
        else:
            args.extend(["--window-position=-32000,-32000", "--window-size=800,600"])
        if profile_dir and browser != "opera":
            args.append(f"--profile-directory={profile_dir}")
        return profile_dir or root.name, args

    async def devtools_get_all_cookies(self, websocket_url: str) -> list[dict]:
        websockets = import_module("websockets")
        async with websockets.connect(websocket_url, max_size=32_000_000) as ws:
            await ws.send(json.dumps({"id": 1, "method": "Storage.getCookies", "params": {}}))
            while True:
                payload = json.loads(await ws.recv())
                if payload.get("id") != 1:
                    continue
                if payload.get("error"):
                    raise RuntimeError(str(payload["error"]))
                return list((payload.get("result") or {}).get("cookies") or [])

    def fetch_devtools_json(self, port: int, endpoint: str, timeout: float = 1.0) -> dict:
        request = Request(f"http://127.0.0.1:{port}{endpoint}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with urlopen(request, timeout=timeout) as response:
            return json.loads(response.read().decode("utf-8", errors="replace"))

    def cdp_cookies_to_cookie_jar(self, cookies: list[dict]) -> http.cookiejar.MozillaCookieJar:
        jar = http.cookiejar.MozillaCookieJar()
        for item in cookies:
            name = str(item.get("name") or "")
            value = str(item.get("value") or "")
            domain = str(item.get("domain") or "")
            if not name or not domain:
                continue
            path = str(item.get("path") or "/")
            expires_value = item.get("expires")
            try:
                expires = int(float(expires_value)) if expires_value not in (None, "", -1) else None
            except (TypeError, ValueError):
                expires = None
            if expires is not None and expires <= 0:
                expires = None
            cookie = http.cookiejar.Cookie(
                version=0,
                name=name,
                value=value,
                port=None,
                port_specified=False,
                domain=domain,
                domain_specified=domain.startswith("."),
                domain_initial_dot=domain.startswith("."),
                path=path,
                path_specified=True,
                secure=bool(item.get("secure")),
                expires=expires,
                discard=expires is None,
                comment=None,
                comment_url=None,
                rest={"HttpOnly": None} if item.get("httpOnly") else {},
                rfc2109=False,
            )
            jar.set_cookie(cookie)
        return jar

    def export_chromium_cookies_via_devtools(self, browser: str, profile: str | None, headless: bool = True) -> tuple[str, object]:
        executable = self.cookie_browser_executable(browser)
        if not executable:
            raise RuntimeError(f"{browser} executable not found")
        profile_label, base_args = self.chromium_profile_launch_args(browser, profile, headless=headless)
        port = self.free_local_port()
        args = [
            executable,
            f"--remote-debugging-port={port}",
            *base_args,
            "https://www.youtube.com/",
        ]
        creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
        process = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=creationflags)
        try:
            version_payload: dict | None = None
            deadline = time.monotonic() + 12.0
            while time.monotonic() < deadline:
                try:
                    version_payload = self.fetch_devtools_json(port, "/json/version", timeout=1.0)
                    break
                except Exception:
                    time.sleep(0.25)
            if not version_payload:
                raise RuntimeError("browser devtools endpoint did not start")
            websocket_url = str(version_payload.get("webSocketDebuggerUrl") or "")
            if not websocket_url:
                raise RuntimeError("browser devtools websocket is missing")
            cookies = asyncio.run(self.devtools_get_all_cookies(websocket_url))
            cookie_jar = self.cdp_cookies_to_cookie_jar(cookies)
            score, youtube_count, total_count = self.cookie_jar_score(cookie_jar)
            if total_count <= 0 or score <= 0 or not self.cookie_jar_has_login_cookies(cookie_jar):
                raise RuntimeError(self.t("browser_cookies_no_youtube"))
            return profile_label, cookie_jar
        finally:
            try:
                process.terminate()
                process.wait(timeout=5)
            except Exception:
                try:
                    process.kill()
                except Exception:
                    pass

    def cookie_profile_candidates(self, browser: str) -> list[tuple[str, str | None]]:
        selected = str(getattr(self.settings, "cookies_browser_profile", COOKIE_PROFILE_AUTO) or COOKIE_PROFILE_AUTO).strip()
        discovered = self.discover_cookie_profiles(browser)
        candidates: list[tuple[str, str | None]] = []
        if selected and selected != COOKIE_PROFILE_AUTO:
            label = Path(selected).name if os.path.isabs(selected) else selected
            candidates.append((label, selected))
        candidates.extend(discovered)
        candidates.append((self.t("browser_profile_auto"), None))
        deduped: list[tuple[str, str | None]] = []
        seen: set[str] = set()
        for label, profile in candidates:
            key = profile or ""
            if key in seen:
                continue
            seen.add(key)
            deduped.append((label, profile))
        return deduped

    @staticmethod
    def youtube_auth_cookie_names() -> set[str]:
        return {
            "sid",
            "sidcc",
            "lsid",
            "osid",
            "hsid",
            "ssid",
            "apisid",
            "sapisid",
            "login_info",
            "account_chooser",
            "__secure-osid",
            "__secure-1psid",
            "__secure-3psid",
            "__secure-1papisid",
            "__secure-3papisid",
            "__secure-1psidts",
            "__secure-3psidts",
            "__secure-1psidcc",
            "__secure-3psidcc",
        }

    @staticmethod
    def cookie_jar_has_login_cookies(cookie_jar) -> bool:
        auth_names = MainFrame.youtube_auth_cookie_names()
        for cookie in cookie_jar:
            domain = str(getattr(cookie, "domain", "") or "").lower()
            name = str(getattr(cookie, "name", "") or "").lower()
            if ("google.com" in domain or "youtube.com" in domain) and name in auth_names:
                return True
        return False

    @staticmethod
    def cookie_jar_score(cookie_jar) -> tuple[int, int, int]:
        auth_names = MainFrame.youtube_auth_cookie_names()
        score = 0
        youtube_count = 0
        total_count = 0
        for cookie in cookie_jar:
            total_count += 1
            domain = str(getattr(cookie, "domain", "") or "").lower()
            name = str(getattr(cookie, "name", "") or "").lower()
            if "youtube.com" in domain:
                youtube_count += 1
                score += 3
            if "google.com" in domain or "youtube.com" in domain:
                score += 1
                if name in auth_names:
                    score += 100
        return score, youtube_count, total_count

    def cookie_score_summary(self, label: str, cookie_jar) -> str:
        score, youtube_count, total_count = self.cookie_jar_score(cookie_jar)
        has_login = self.cookie_jar_has_login_cookies(cookie_jar)
        return f"{label}: {total_count} cookies, {youtube_count} YouTube cookies, login cookies {'yes' if has_login else 'no'}, score {score}"

    def export_browser_cookies_blocking(self, browser: str, allow_close: bool = False) -> dict:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            raise RuntimeError(self.t("missing_ytdlp"))
        if allow_close and self.cookie_browser_is_running(browser):
            self.close_cookie_browser_processes(browser)
            self.wait_for_cookie_browser_exit(browser)
        cookies_module = import_module("yt_dlp.cookies")
        candidates = self.cookie_profile_candidates(browser)
        errors: list[str] = []
        best: tuple[int, str, object, str] | None = None
        copy_lock_error_seen = False
        for attempt in range(2):
            lock_error_seen = False
            for label, profile in candidates:
                logger = MemoryYtdlpLogger()
                try:
                    cookie_jar = cookies_module.extract_cookies_from_browser(browser, profile, logger)
                    score, youtube_count, total_count = self.cookie_jar_score(cookie_jar)
                    if total_count <= 0:
                        errors.append(self.t("cookie_profile_attempt_failed", profile=label, error="no cookies found"))
                        continue
                    errors.append(self.cookie_score_summary(label, cookie_jar))
                    if best is None or score > best[0]:
                        best = (score, label, cookie_jar, logger.summary())
                    if score >= 100 and youtube_count > 0:
                        break
                except Exception as exc:
                    error_text = self.cookie_export_error_text(exc, logger)
                    if "could not copy" in error_text.lower() and "cookie" in error_text.lower():
                        lock_error_seen = True
                        copy_lock_error_seen = True
                    errors.append(self.t("cookie_profile_attempt_failed", profile=label, error=error_text))
            if best and best[0] > 0:
                break
            if allow_close and lock_error_seen and attempt == 0:
                self.close_cookie_browser_processes(browser)
                self.wait_for_cookie_browser_exit(browser, timeout=8.0)
                time.sleep(1.0)
                continue
            break
        needs_devtools_fallback = copy_lock_error_seen or not best or (best is not None and not self.cookie_jar_has_login_cookies(best[2]))
        if allow_close and browser in CHROMIUM_COOKIE_BROWSERS and needs_devtools_fallback:
            self.close_cookie_browser_processes(browser)
            self.wait_for_cookie_browser_exit(browser, timeout=8.0)
            tried_profiles: set[str] = set()
            for label, profile in candidates:
                profile_key = profile or "Default"
                if profile_key in tried_profiles:
                    continue
                tried_profiles.add(profile_key)
                for headless in (True, False):
                    mode_label = "DevTools headless" if headless else "DevTools window"
                    try:
                        cdp_label, cookie_jar = self.export_chromium_cookies_via_devtools(browser, profile, headless=headless)
                        score, youtube_count, total_count = self.cookie_jar_score(cookie_jar)
                        if total_count <= 0:
                            errors.append(self.t("cookie_profile_attempt_failed", profile=f"{label} {mode_label}", error="no cookies found"))
                            continue
                        errors.append(self.cookie_score_summary(f"{cdp_label or label} {mode_label}", cookie_jar))
                        if best is None or score > best[0]:
                            best = (score, cdp_label or label, cookie_jar, mode_label)
                        if score >= 100 and youtube_count > 0:
                            break
                    except Exception as exc:
                        errors.append(self.t("cookie_profile_attempt_failed", profile=f"{label} {mode_label}", error=self.cookie_export_error_text(exc)))
                if best and best[0] >= 100 and self.cookie_jar_has_login_cookies(best[2]):
                    break
        if not best or best[0] <= 0 or not self.cookie_jar_has_login_cookies(best[2]):
            details = list(errors[-10:]) if errors else [self.t("cookie_all_profiles_failed")]
            if best:
                details.append(f"Best profile was {best[1]}, but it did not contain usable Google/YouTube login cookies.")
            detail = "\n".join(details)
            raise RuntimeError(f"{self.t('browser_cookies_no_youtube')}\n\n{self.t('cookie_export_diagnostics', details=detail)}")
        _score, label, cookie_jar, _summary = best
        CACHED_COOKIES_FILE.parent.mkdir(parents=True, exist_ok=True)
        cookie_jar.save(str(CACHED_COOKIES_FILE), ignore_discard=True, ignore_expires=True)
        self.settings.cookies_file = str(CACHED_COOKIES_FILE)
        self.settings.cookies_from_browser = browser
        self.cookie_repair_suppressed_until = 0.0
        self.save_settings()
        return {"path": str(CACHED_COOKIES_FILE), "profile_label": label}

    def cookie_export_error_text(self, exc: Exception | str, logger: MemoryYtdlpLogger | None = None) -> str:
        text = self.friendly_error(exc)
        summary = logger.summary() if logger else ""
        if summary and summary not in text:
            text = f"{text}\n{summary}"
        return text

    def wait_for_cookie_browser_exit(self, browser: str, timeout: float = 6.0) -> bool:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if not self.cookie_browser_is_running(browser):
                return True
            time.sleep(0.25)
        return not self.cookie_browser_is_running(browser)

    def set_window_title(self, media_title: str | None = None) -> None:
        title = re.sub(r"\s+", " ", str(media_title or "").strip())
        if title:
            window_title = f"{title} - {WINDOW_TITLE}"
        else:
            window_title = WINDOW_TITLE
        try:
            if self.GetTitle() == window_title:
                return
        except Exception:
            pass
        self.SetTitle(window_title)

    def player_is_active(self) -> bool:
        return self.player_kind == "mpv" and self.mpv_process_alive()

    def background_playback_enabled(self) -> bool:
        return bool(getattr(self.settings, "enable_background_playback", False))

    def background_player_section_enabled(self) -> bool:
        return self.background_playback_enabled()

    def current_player_title(self) -> str:
        info = self.current_video_info or {}
        item = self.current_video_item or {}
        return str(info.get("title") or item.get("title") or self.t("player")).strip()

    def current_play_pause_label(self) -> str:
        if not self.player_is_active() or self.player_ended or self.player_paused:
            return self.t("play")
        return self.t("pause")

    def update_play_pause_buttons(self) -> None:
        label = self.current_play_pause_label()
        changed = False
        for button in list(getattr(self, "player_play_pause_buttons", [])):
            try:
                if button and not button.IsBeingDeleted():
                    if button.GetLabel() != label:
                        button.SetLabel(label)
                        changed = True
                    if button.GetName() != label:
                        button.SetName(label)
                        changed = True
                    if button.GetToolTipText() != label:
                        button.SetToolTip(label)
                        changed = True
            except RuntimeError:
                continue
        if changed:
            try:
                self.panel.Layout()
            except Exception:
                pass

    def clear(self) -> None:
        preserved_player_panel = None
        if self.player_is_active() and self.player_panel is not None:
            try:
                if not self.player_panel.IsBeingDeleted():
                    preserved_player_panel = self.player_panel
                    self.root_sizer.Detach(preserved_player_panel)
                    preserved_player_panel.Hide()
            except RuntimeError:
                preserved_player_panel = None
        if self.player_is_active():
            self.set_window_title(self.current_player_title())
        elif not self.in_player_screen:
            self.set_window_title()
        self.root_sizer.Clear(delete_windows=True)
        self.background_player_controls = []
        self.player_action_controls = []
        self.player_play_pause_buttons = []
        self.background_player_section_added = False
        if not self.in_player_screen:
            if preserved_player_panel is not None:
                self.player_panel = preserved_player_panel
            elif not self.player_is_active():
                self.player_panel = None
            self.details_label = None
            self.video_details = None
            self.details_button_sizer = None
            self.details_opened_temporarily = False

    def focus_later(self, control: wx.Window) -> None:
        wx.CallAfter(self.safe_set_focus, control)

    @staticmethod
    def safe_set_focus(control: wx.Window) -> None:
        try:
            if control and not getattr(control, "IsBeingDeleted", lambda: False)():
                if wx.Window.FindFocus() is control:
                    return
                control.SetFocus()
        except RuntimeError:
            pass

    def set_equalizer_slider_accessibility(self, ctrl: wx.Slider, label: str) -> None:
        value_text = f"{float(ctrl.GetValue()) / 10.0:.1f} dB"
        name = str(label).strip() or self.t("equalizer")
        full_text = f"{name}: {value_text}"
        ctrl.SetName(full_text)
        ctrl.SetLabel(full_text)
        ctrl.SetToolTip(full_text)
        ctrl._apricot_accessible_name = name
        ctrl._apricot_accessible_description = full_text
        ctrl._apricot_accessible_value = value_text
        if not getattr(ctrl, "_apricot_accessible", None):
            try:
                ctrl._apricot_accessible = SliderAccessible(ctrl)
                ctrl.SetAccessible(ctrl._apricot_accessible)
            except Exception:
                pass
        try:
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_NAMECHANGE, ctrl, wx.OBJID_CLIENT, 0)
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_VALUECHANGE, ctrl, wx.OBJID_CLIENT, 0)
        except Exception:
            pass

    def set_integer_slider_accessibility(self, ctrl: wx.Slider, label: str, unit: str = "") -> None:
        value = int(ctrl.GetValue())
        name = str(label).strip()
        value_text = self.t("download_percent_value", percent=value) if unit == "percent" else f"{value} {unit}".strip()
        full_text = f"{name}: {value_text}" if value_text else name
        ctrl.SetName(full_text)
        ctrl.SetLabel(full_text)
        ctrl.SetToolTip(full_text)
        ctrl._apricot_accessible_name = name
        ctrl._apricot_accessible_description = full_text
        ctrl._apricot_accessible_value = value_text
        if not getattr(ctrl, "_apricot_accessible", None):
            try:
                ctrl._apricot_accessible = SliderAccessible(ctrl)
                ctrl.SetAccessible(ctrl._apricot_accessible)
            except Exception:
                pass
        try:
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_NAMECHANGE, ctrl, wx.OBJID_CLIENT, 0)
            wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_VALUECHANGE, ctrl, wx.OBJID_CLIENT, 0)
        except Exception:
            pass

    def foreground_window(self) -> None:
        try:
            self.Show(True)
        except Exception:
            pass
        try:
            if self.IsIconized():
                self.Iconize(False)
        except Exception:
            pass
        try:
            self.Raise()
        except Exception:
            pass
        if os.name == "nt":
            try:
                hwnd = int(self.GetHandle())
                if hwnd:
                    user32 = ctypes.windll.user32
                    kernel32 = ctypes.windll.kernel32
                    foreground = user32.GetForegroundWindow()
                    foreground_thread = user32.GetWindowThreadProcessId(foreground, None) if foreground else 0
                    target_thread = user32.GetWindowThreadProcessId(hwnd, None)
                    current_thread = kernel32.GetCurrentThreadId()
                    attached: list[tuple[int, int]] = []
                    for source_thread, target_input_thread in (
                        (current_thread, foreground_thread),
                        (target_thread, foreground_thread),
                    ):
                        if source_thread and target_input_thread and source_thread != target_input_thread:
                            try:
                                if user32.AttachThreadInput(source_thread, target_input_thread, True):
                                    attached.append((source_thread, target_input_thread))
                            except Exception:
                                pass
                    try:
                        user32.ShowWindow(hwnd, 9)
                        user32.BringWindowToTop(hwnd)
                        user32.SetForegroundWindow(hwnd)
                        user32.SetActiveWindow(hwnd)
                        user32.SetFocus(hwnd)
                    finally:
                        for source_thread, target_input_thread in reversed(attached):
                            try:
                                user32.AttachThreadInput(source_thread, target_input_thread, False)
                            except Exception:
                                pass
                    try:
                        if user32.GetForegroundWindow() != hwnd:
                            self.RequestUserAttention(wx.USER_ATTENTION_INFO)
                    except Exception:
                        pass
            except Exception:
                pass

    def primary_focus_candidate(self) -> wx.Window | None:
        if getattr(self, "in_main_menu", False) and hasattr(self, "menu_list"):
            return self.menu_list
        if getattr(self, "search_screen_active", False) and hasattr(self, "query"):
            return self.query
        focus = wx.Window.FindFocus()
        return focus or self

    def focus_primary_control(self) -> None:
        focus = self.primary_focus_candidate()
        if focus:
            self.safe_set_focus(focus)

    def activate_window(self) -> None:
        focus = wx.Window.FindFocus()
        primary = self.primary_focus_candidate()
        if primary is not None and focus is primary and self.app_has_focus():
            return
        self.foreground_window()
        self.focus_primary_control()

    def activate_window_later(self, delays: tuple[int, ...] = (0, 75, 250, 750)) -> None:
        for delay in delays:
            if delay <= 0:
                wx.CallAfter(self.activate_window)
            else:
                wx.CallLater(delay, self.activate_window)

    def activate_after_update_relaunch(self) -> None:
        self.activate_window_later((0, 100, 350, 900, 1800, 3000))

    @staticmethod
    def listbox_matches(listbox: wx.ListBox, labels: list[str]) -> bool:
        try:
            if listbox.GetCount() != len(labels):
                return False
            return all(listbox.GetString(index) == labels[index] for index in range(len(labels)))
        except RuntimeError:
            return False

    def set_listbox_items(self, listbox: wx.ListBox, labels: list[str], selection: int = 0) -> bool:
        labels = [str(label) for label in labels]
        if not labels:
            return False
        target_selection = min(max(0, selection), len(labels) - 1)
        current_selection = listbox.GetSelection()
        if self.listbox_matches(listbox, labels):
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

    def append_listbox_items(self, listbox: wx.ListBox, labels: list[str], previous_count: int, selection: int) -> bool:
        labels = [str(label) for label in labels]
        if previous_count < 0 or previous_count > len(labels):
            return False
        try:
            if listbox.GetCount() != previous_count:
                return False
        except RuntimeError:
            return False
        target_selection = min(max(0, selection), len(labels) - 1)
        listbox.Freeze()
        try:
            for label in labels[previous_count:]:
                listbox.Append(label)
            if listbox.GetSelection() != target_selection:
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

    def add_button_row(self, buttons: list[tuple[str, callable]]) -> list[wx.Button]:
        row = wx.BoxSizer(wx.HORIZONTAL)
        created_buttons = []
        for label, handler in buttons:
            is_play_pause = getattr(handler, "__name__", "") == "player_play_pause"
            button_label = self.current_play_pause_label() if is_play_pause else label
            button = wx.Button(self.panel, label=button_label)
            if is_play_pause:
                button.SetName(button_label)
                button.SetToolTip(button_label)
                self.player_play_pause_buttons.append(button)
            button.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            row.Add(button, 0, wx.RIGHT, 6)
            created_buttons.append(button)
        self.root_sizer.Add(row, 0, wx.ALL, 4)
        if self.background_player_section_enabled() and not self.in_player_screen:
            self.add_background_player_section()
        return created_buttons

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

    @staticmethod
    def windows_startup_run_key_path() -> str:
        return r"Software\Microsoft\Windows\CurrentVersion\Run"

    @staticmethod
    def windows_startup_value_name() -> str:
        return APP_NAME

    @staticmethod
    def current_launch_command(start_in_tray: bool = False) -> str:
        if getattr(sys, "frozen", False):
            parts = [sys.executable]
        else:
            parts = [sys.executable, str(Path(__file__).resolve())]
        if start_in_tray:
            parts.append(START_IN_TRAY_ARG)
        return subprocess.list2cmdline(parts)

    def sync_windows_startup_registration(self, show_error: bool = False) -> bool:
        if os.name != "nt" or winreg is None:
            return False
        try:
            access = winreg.KEY_SET_VALUE | winreg.KEY_QUERY_VALUE
            with winreg.CreateKeyEx(winreg.HKEY_CURRENT_USER, self.windows_startup_run_key_path(), 0, access) as key:
                value_name = self.windows_startup_value_name()
                if self.settings.start_with_windows:
                    command = self.current_launch_command(start_in_tray=False)
                    current = ""
                    try:
                        current, _value_type = winreg.QueryValueEx(key, value_name)
                    except FileNotFoundError:
                        current = ""
                    if str(current) != command:
                        winreg.SetValueEx(key, value_name, 0, winreg.REG_SZ, command)
                else:
                    try:
                        winreg.DeleteValue(key, value_name)
                    except FileNotFoundError:
                        pass
            return True
        except Exception as exc:
            if show_error:
                self.message(self.t("startup_registration_failed", error=self.friendly_error(exc)), wx.ICON_WARNING)
            return False

    def on_close(self, event: wx.CloseEvent) -> None:
        if self.exiting or not self.settings.close_to_tray:
            self.shutdown_runtime()
            event.Skip()
            return
        event.Veto()
        self.Hide()
        self.announce_player(self.t("tray_still_running"))
        self.show_desktop_notification(APP_NAME, self.t("tray_still_running"), enabled=self.settings.tray_notification)

    def restore_from_tray(self) -> None:
        try:
            self.RequestUserAttention(wx.USER_ATTENTION_INFO)
        except Exception:
            pass
        self.activate_window_later((0, 75, 250, 700, 1400))

    def show_settings_from_tray(self) -> None:
        self.restore_from_tray()
        wx.CallAfter(self.open_settings_screen)

    def check_activation_signal(self) -> None:
        now = time.monotonic()
        if now - self.last_activation_check < 0.05:
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
        if action == "open_file":
            self.restore_from_tray()
            path = str(payload.get("path") or "")
            if path:
                wx.CallAfter(self.open_local_media_file, path)
        elif action == "settings":
            self.show_settings_from_tray()
        else:
            self.restore_from_tray()

    def quit_application(self) -> None:
        self.shutdown_runtime()
        self.Close(force=True)

    def shutdown_runtime(self) -> None:
        self.exiting = True
        for timer in (
            getattr(self, "timer", None),
            getattr(self, "subscription_timer", None),
            getattr(self, "rss_timer", None),
            getattr(self, "app_update_timer", None),
        ):
            try:
                if timer and timer.IsRunning():
                    timer.Stop()
            except Exception:
                pass
        try:
            self.stop_player(silent=True)
        except Exception:
            pass
        self.destroy_taskbar_icon()

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
        self.trending_screen_active = False
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
        self.folder_screen_active = False
        self.settings_section_index = 0
        self.clear()
        title = wx.StaticText(self.panel, label=self.t("main_menu"))
        self.root_sizer.Add(title, 0, wx.ALL, 4)
        self.add_background_player_section()
        self.menu_actions = self.build_main_menu_actions()
        self.menu_list = wx.ListBox(self.panel, choices=[item[0] for item in self.menu_actions])
        self.menu_list.SetName(self.t("main_menu"))
        self.menu_list.SetSelection(0)
        self.menu_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.activate_menu())
        self.menu_list.Bind(wx.EVT_KEY_DOWN, self.on_menu_key)
        self.root_sizer.Add(self.menu_list, 1, wx.EXPAND | wx.ALL, 4)
        self.add_button_row([(self.t("open"), self.activate_menu)])
        self.panel.Layout()
        self.focus_later(self.menu_list)

    def build_main_menu_actions(self) -> list[tuple[str, callable]]:
        actions: list[tuple[str, callable]] = []
        pending_version = self.pending_app_update_version()
        if pending_version:
            actions.append((self.t("app_update_menu_item", version=pending_version), self.open_pending_app_update))
        download_count = len(self.download_queue) + len(self.active_downloads)
        if download_count:
            actions.append((f"{self.t('current_downloads')} ({download_count})", self.show_download_queue))
        if self.playback_queue:
            actions.append((f"{self.t('playback_queue')} ({len(self.playback_queue)})", self.show_playback_queue))
        primary_actions = [
            (self.t("search_youtube"), self.show_search),
            (self.t("play_folder"), self.show_play_from_folder),
            (self.t("direct_link"), self.show_direct_link),
            (self.t("favorites"), self.show_favorites),
            (self.t("playlists"), self.show_user_playlists),
            (self.t("subscriptions"), self.show_subscriptions),
            (self.t("notification_center"), self.show_notification_center),
        ]
        if getattr(self.settings, "enable_trending", False):
            primary_actions.insert(1, (self.t("trending"), self.show_trending))
        actions.extend(primary_actions)
        if self.settings.enable_history:
            actions.append((self.t("history"), self.show_history))
        if self.settings.enable_podcasts_rss:
            actions.append((self.t("rss_feeds"), self.show_rss_feeds))
        actions.extend([
            (self.t("file_converter"), self.show_file_converter),
            (self.t("folder_converter"), self.show_folder_converter),
            (self.t("settings"), self.show_settings),
            (self.t("exit"), self.quit_application),
        ])
        return actions

    def add_background_player_section(self) -> None:
        if self.background_player_section_added:
            return
        self.background_player_controls = []
        if not self.background_player_section_enabled() or not self.player_is_active():
            return
        self.background_player_section_added = True
        title = self.current_player_title()
        label = wx.StaticText(self.panel, label=self.t("background_player_now_playing", title=title))
        label.SetName(self.t("background_player"))
        self.root_sizer.Add(label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 4)

        if self.player_panel is not None:
            try:
                if not self.player_panel.IsBeingDeleted():
                    self.player_panel.Show()
                    self.player_panel.SetCanFocus(True)
                    self.player_panel.SetName(self.t("player"))
                    self.player_panel.SetLabel(self.t("player"))
                    self.player_panel.SetMinSize((-1, 96))
                    self.root_sizer.Add(self.player_panel, 0, wx.EXPAND | wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
                    self.background_player_controls.append(self.player_panel)
            except RuntimeError:
                pass

        row = wx.BoxSizer(wx.HORIZONTAL)
        controls = [
            (self.t("previous"), lambda: self.play_relative_item(-1)),
            (self.current_play_pause_label(), self.player_play_pause),
            (self.t("next"), lambda: self.play_relative_item(1)),
            (self.t("playback_queue"), self.show_playback_queue),
            (self.t("add_to_playlist"), lambda: self.add_active_to_playlist(prefer_active=True)),
            (self.t("output_devices"), self.show_output_devices),
            (self.t("equalizer"), self.show_player_equalizer),
            (self.t("fullscreen"), self.enter_player_fullscreen),
            (self.t("bass_boost"), self.toggle_bass_boost),
            (self.t("repeat"), self.toggle_repeat),
            (self.t("shuffle"), self.toggle_shuffle),
            (self.t("copy_link"), self.copy_current_player_url),
            (self.t("close_player"), self.close_current_player),
        ]
        for label_text, handler in controls:
            button = wx.Button(self.panel, label=label_text)
            button.SetName(f"{self.t('background_player')}: {label_text}")
            button.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            button.Bind(wx.EVT_KEY_DOWN, self.on_background_player_key)
            if getattr(handler, "__name__", "") == "player_play_pause":
                self.player_play_pause_buttons.append(button)
            row.Add(button, 0, wx.RIGHT, 6)
            self.background_player_controls.append(button)
        self.root_sizer.Add(row, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        previous_control = None
        for control in self.background_player_controls:
            if previous_control is not None:
                try:
                    control.MoveAfterInTabOrder(previous_control)
                except RuntimeError:
                    pass
            previous_control = control

    def on_menu_key(self, event: wx.KeyEvent) -> None:
        if self.is_modifier_only_event(event):
            return
        if self.shortcut_matches(event, "open_selected"):
            self.activate_menu()
            return
        if self.handle_global_navigation_shortcut(event, self.menu_list):
            return
        event.Skip()

    def on_background_player_key(self, event: wx.KeyEvent) -> None:
        self.on_char_hook(event)

    @staticmethod
    def live_window(control: wx.Window | None) -> wx.Window | None:
        if control is None:
            return None
        try:
            if control.IsBeingDeleted():
                return None
            if not control.IsShownOnScreen() and not control.IsShown():
                return None
        except RuntimeError:
            return None
        except Exception:
            pass
        return control

    def background_player_previous_target(self) -> wx.Window | None:
        if getattr(self, "in_main_menu", False):
            return self.live_window(getattr(self, "menu_list", None))
        candidate_names = [
            "results_list",
            "queue_list",
            "rss_items_list",
            "rss_feed_list",
            "podcast_result_list",
            "user_playlist_items_list",
            "user_playlist_list",
            "notification_list",
            "history_list",
            "direct_link_ctrl",
        ]
        for name in candidate_names:
            target = self.live_window(getattr(self, name, None))
            if target is not None and not self.focus_in_background_player_controls(target):
                return target
        return None

    def handle_background_player_tab_navigation(self, event: wx.KeyEvent, focus: wx.Window | None) -> bool:
        if event.GetKeyCode() != wx.WXK_TAB:
            return False
        controls = [
            control
            for control in getattr(self, "background_player_controls", [])
            if control is not None and not getattr(control, "IsBeingDeleted", lambda: False)()
        ]
        if not controls or focus not in controls:
            return False
        try:
            index = controls.index(focus)
        except ValueError:
            return False
        next_index = index - 1 if event.ShiftDown() else index + 1
        if 0 <= next_index < len(controls):
            self.safe_set_focus(controls[next_index])
            return True
        if event.ShiftDown() and index == 0:
            target = self.background_player_previous_target()
            if target is not None:
                self.safe_set_focus(target)
                return True
        return False

    def show_current_player_screen(self) -> None:
        if not self.player_is_active():
            self.announce_player(self.t("no_player"))
            self.show_main_menu()
            return
        self.show_player_page(self.current_player_title())

    def player_fullscreen_mode_active(self) -> bool:
        if self.player_fullscreen_session:
            return True
        return bool(getattr(self.settings, "player_fullscreen", False) and not self.player_fullscreen_results_override)

    def exit_fullscreen_window(self) -> None:
        self.player_fullscreen_session = False
        self.player_fullscreen_results_override = True
        try:
            if self.player_kind == "mpv" and self.mpv_process_alive():
                self.mpv_request(["set_property", "fullscreen", False], timeout=0.5)
        except Exception:
            pass
        try:
            if self.IsFullScreen():
                self.ShowFullScreen(False)
        except Exception:
            pass

    def exit_fullscreen_to_results(self) -> None:
        if not self.player_is_active():
            self.back_to_results(stop_playback=False)
            return
        self.exit_fullscreen_window()
        self.show_player_page(self.current_player_title(), focus_target="results")
        wx.CallAfter(self.focus_results_list, self.return_index)
        wx.CallLater(100, self.focus_results_list, self.return_index)
        wx.CallLater(300, self.focus_results_list, self.return_index)

    def exit_fullscreen_to_player(self) -> None:
        if not self.player_is_active():
            self.back_to_results(stop_playback=False)
            return
        self.exit_fullscreen_window()
        self.show_player_page(self.current_player_title(), focus_target="player")
        if self.player_panel is not None:
            wx.CallAfter(self.safe_set_focus, self.player_panel)
            wx.CallLater(100, self.safe_set_focus, self.player_panel)
            wx.CallLater(300, self.safe_set_focus, self.player_panel)

    def enter_player_fullscreen(self) -> None:
        if not self.player_is_active():
            self.announce_player(self.t("no_player"))
            return
        self.player_fullscreen_session = True
        self.player_fullscreen_results_override = False
        try:
            self.show_player_page(self.current_player_title())
            if self.player_kind == "mpv":
                self.mpv_request(["set_property", "fullscreen", True], timeout=0.5)
            self.ShowFullScreen(True)
            if self.player_panel is not None:
                self.safe_set_focus(self.player_panel)
        except Exception:
            try:
                self.ShowFullScreen(True)
            except Exception:
                pass

    def on_player_fullscreen_changed(self, _event=None) -> None:
        checked = bool(getattr(self, "fullscreen_checkbox", None) and self.fullscreen_checkbox.GetValue())
        if checked:
            self.enter_player_fullscreen()
        else:
            self.exit_fullscreen_to_results()

    def activate_menu(self) -> None:
        index = self.menu_list.GetSelection()
        if index != wx.NOT_FOUND:
            self.menu_actions[index][1]()

    def pending_app_update_version(self) -> str:
        if not self.pending_app_update_release:
            return ""
        return self.release_version(self.pending_app_update_release)

    def open_pending_app_update(self) -> None:
        release = self.pending_app_update_release
        asset = self.pending_app_update_asset
        if not release or not asset:
            self.start_app_update_check(manual=True)
            return
        version = self.release_version(release)
        if not getattr(sys, "frozen", False):
            self.message(self.t("update_source_only", version=version))
            return
        changelog = self.release_changelog_text(release)
        if self.show_update_prompt(version, changelog):
            self.log_update_event(f"User selected pending update now for {version}")
            if self.settings.skipped_update_version:
                self.settings.skipped_update_version = ""
                self.save_settings()
            self.begin_app_update_install(release, asset)
        else:
            self.log_update_event(f"User skipped pending update {version}")
            self.settings.skipped_update_version = version
            self.pending_app_update_release = None
            self.pending_app_update_asset = None
            self.save_settings()
            self.announce_player(self.t("update_skipped", version=version))
            if self.in_main_menu:
                self.show_main_menu()

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
        self.add_background_player_section()
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

    def show_file_converter(self) -> None:
        self.show_converter_dialog(folder_mode=False)

    def show_folder_converter(self) -> None:
        self.show_converter_dialog(folder_mode=True)

    def converter_input_kind(self, path: str | Path) -> str:
        suffix = Path(path).suffix.lower()
        if suffix in AUDIO_INPUT_EXTENSIONS:
            return "audio"
        if suffix in VIDEO_INPUT_EXTENSIONS:
            return "video"
        return ""

    def converter_format_values(self, input_kind: str = "") -> list[str]:
        if input_kind == "audio":
            return [*AUDIO_CONVERT_FORMATS, *VIDEO_CONVERT_FORMATS]
        if input_kind == "video":
            return [*VIDEO_CONVERT_FORMATS, *AUDIO_CONVERT_FORMATS]
        return [*AUDIO_CONVERT_FORMATS, *VIDEO_CONVERT_FORMATS]

    @staticmethod
    def converter_format_labels(values: list[str]) -> list[str]:
        labels = []
        for value in values:
            labels.append("ALAC (M4A)" if value == "alac" else value.upper())
        return labels

    @staticmethod
    def converter_output_extension(target_format: str) -> str:
        return "m4a" if target_format == "alac" else target_format

    def converter_wildcard_for_target(self, target_format: str) -> str:
        extension = self.converter_output_extension(target_format)
        return f"{extension.upper()} (*.{extension})|*.{extension}|{self.t('all_files')} (*.*)|*.*"

    def converter_default_output_path(self, source: Path, target_format: str) -> Path:
        extension = self.converter_output_extension(target_format)
        return source.with_name(f"{source.stem}.{extension}")

    def converter_is_audio_to_video(self, source_path: str | Path, target_format: str) -> bool:
        return self.converter_input_kind(source_path) == "audio" and target_format in VIDEO_CONVERT_FORMATS

    def folder_has_audio_inputs(self, folder: Path) -> bool:
        try:
            return any(path.is_file() and path.suffix.lower() in AUDIO_INPUT_EXTENSIONS for path in folder.iterdir())
        except OSError:
            return False

    def converter_media_files_in_folder(self, folder: Path) -> list[Path]:
        try:
            return sorted(path for path in folder.iterdir() if path.is_file() and path.suffix.lower() in CONVERTER_MEDIA_EXTENSIONS)
        except OSError:
            return []

    def show_converter_dialog(self, folder_mode: bool = False) -> None:
        title = self.t("folder_converter" if folder_mode else "file_converter")
        dialog = wx.Dialog(self, title=title, style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        try:
            main = wx.BoxSizer(wx.VERTICAL)
            form = wx.FlexGridSizer(0, 2, 6, 6)
            form.AddGrowableCol(1, 1)

            path_key = "folder_to_convert" if folder_mode else "file_to_convert"
            path_label = wx.StaticText(dialog, label=self.t(path_key))
            path_ctrl = wx.TextCtrl(dialog)
            path_ctrl.SetName(self.t(path_key))
            browse_button = wx.Button(dialog, label=self.t("browse_folder" if folder_mode else "browse_file"))
            browse_button.SetName(self.t("browse_folder" if folder_mode else "browse_file"))
            path_row = wx.BoxSizer(wx.HORIZONTAL)
            path_row.Add(path_ctrl, 1, wx.EXPAND | wx.RIGHT, 4)
            path_row.Add(browse_button, 0)
            form.Add(path_label, 0, wx.ALIGN_CENTER_VERTICAL)
            form.Add(path_row, 1, wx.EXPAND)

            detected_ctrl = wx.TextCtrl(dialog, value=self.t("empty"), style=wx.TE_READONLY)
            detected_ctrl.SetName(self.t("detected_format"))
            form.Add(wx.StaticText(dialog, label=self.t("detected_format")), 0, wx.ALIGN_CENTER_VERTICAL)
            form.Add(detected_ctrl, 1, wx.EXPAND)

            target_choice = wx.Choice(dialog, choices=[])
            target_choice.SetName(self.t("convert_to"))
            target_values: list[str] = []
            form.Add(wx.StaticText(dialog, label=self.t("convert_to")), 0, wx.ALIGN_CENTER_VERTICAL)
            form.Add(target_choice, 1, wx.EXPAND)

            options_label = wx.StaticText(dialog, label=self.t("converter_audio_to_video_options"))
            add_image_box = wx.CheckBox(dialog, label=self.t("add_image"))
            add_image_box.SetName(self.t("add_image"))
            dark_box = wx.CheckBox(dialog, label=self.t("dark_background"))
            dark_box.SetName(self.t("dark_background"))
            image_label = wx.StaticText(dialog, label=self.t("image_path"))
            image_ctrl = wx.TextCtrl(dialog)
            image_ctrl.SetName(self.t("image_path"))
            image_button = wx.Button(dialog, label=self.t("choose_image"))
            image_button.SetName(self.t("choose_image"))
            image_row = wx.BoxSizer(wx.HORIZONTAL)
            image_row.Add(image_ctrl, 1, wx.EXPAND | wx.RIGHT, 4)
            image_row.Add(image_button, 0)
            form.Add(options_label, 0, wx.ALIGN_CENTER_VERTICAL)
            form.Add(add_image_box, 1, wx.EXPAND)
            form.AddSpacer(1)
            form.Add(dark_box, 1, wx.EXPAND)
            form.Add(image_label, 0, wx.ALIGN_CENTER_VERTICAL)
            form.Add(image_row, 1, wx.EXPAND)

            button_row = wx.BoxSizer(wx.HORIZONTAL)
            convert_button = wx.Button(dialog, label=self.t("convert"))
            convert_button.SetName(self.t("convert"))
            cancel_button = wx.Button(dialog, wx.ID_CANCEL, self.t("back"))
            button_row.Add(convert_button, 0, wx.RIGHT, 6)
            button_row.Add(cancel_button, 0)

            main.Add(form, 1, wx.EXPAND | wx.ALL, 10)
            main.Add(button_row, 0, wx.ALIGN_RIGHT | wx.LEFT | wx.RIGHT | wx.BOTTOM, 10)
            dialog.SetSizer(main)

            def selected_target() -> str:
                selection = target_choice.GetSelection()
                if 0 <= selection < len(target_values):
                    return target_values[selection]
                return target_values[0] if target_values else "mp3"

            def should_show_audio_video_options() -> bool:
                target = selected_target()
                if target not in VIDEO_CONVERT_FORMATS:
                    return False
                path = Path(path_ctrl.GetValue().strip().strip('"'))
                if folder_mode:
                    return not path.exists() or self.folder_has_audio_inputs(path)
                return self.converter_is_audio_to_video(path, target)

            def update_audio_video_controls() -> None:
                show_options = should_show_audio_video_options()
                if show_options and not add_image_box.GetValue() and not dark_box.GetValue():
                    dark_box.SetValue(True)
                show_image = show_options and add_image_box.GetValue()
                for ctrl in (options_label, add_image_box, dark_box):
                    ctrl.Show(show_options)
                image_label.Show(show_image)
                image_ctrl.Show(show_image)
                image_button.Show(show_image)
                dialog.Layout()
                dialog.Fit()

            def update_detected_and_formats(_event=None) -> None:
                nonlocal target_values
                raw_path = path_ctrl.GetValue().strip().strip('"')
                path = Path(raw_path) if raw_path else Path()
                if folder_mode:
                    detected = self.t("folder_converter") if raw_path else self.t("empty")
                    input_kind = ""
                else:
                    input_kind = self.converter_input_kind(path) if raw_path else ""
                    detected = (path.suffix.lower().lstrip(".") or self.t("empty")) if input_kind else self.t("unsupported_input_format")
                    if not raw_path:
                        detected = self.t("empty")
                detected_ctrl.SetValue(detected)
                current = selected_target() if target_values else ""
                target_values = self.converter_format_values(input_kind)
                target_choice.Set(self.converter_format_labels(target_values))
                target_choice.SetSelection(target_values.index(current) if current in target_values else 0)
                update_audio_video_controls()

            def browse_path(_event=None) -> None:
                if folder_mode:
                    with wx.DirDialog(dialog, self.t("browse_folder"), style=wx.DD_DEFAULT_STYLE | wx.DD_DIR_MUST_EXIST) as chooser:
                        if chooser.ShowModal() == wx.ID_OK:
                            path_ctrl.SetValue(chooser.GetPath())
                else:
                    wildcard = self.converter_input_wildcard()
                    with wx.FileDialog(dialog, self.t("browse_file"), wildcard=wildcard, style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST) as chooser:
                        if chooser.ShowModal() == wx.ID_OK:
                            path_ctrl.SetValue(chooser.GetPath())
                update_detected_and_formats()

            def browse_image(_event=None) -> None:
                with wx.FileDialog(dialog, self.t("select_image_file"), wildcard=self.converter_image_wildcard(), style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST) as chooser:
                    if chooser.ShowModal() == wx.ID_OK:
                        image_ctrl.SetValue(chooser.GetPath())

            def on_add_image(_event=None) -> None:
                if add_image_box.GetValue():
                    dark_box.SetValue(False)
                elif not dark_box.GetValue():
                    dark_box.SetValue(True)
                update_audio_video_controls()

            def on_dark(_event=None) -> None:
                if dark_box.GetValue():
                    add_image_box.SetValue(False)
                elif not add_image_box.GetValue():
                    add_image_box.SetValue(True)
                update_audio_video_controls()

            def convert(_event=None) -> None:
                raw_path = path_ctrl.GetValue().strip().strip('"')
                source = Path(raw_path).expanduser()
                if not raw_path or not source.exists():
                    self.message(self.t("no_selection"), wx.ICON_WARNING)
                    return
                target = selected_target()
                use_image = bool(add_image_box.IsShown() and add_image_box.GetValue())
                image_path = Path(image_ctrl.GetValue().strip().strip('"')).expanduser() if use_image else None
                if use_image and (not image_path or not image_path.exists()):
                    self.message(self.t("select_image_file"), wx.ICON_WARNING)
                    return
                if folder_mode:
                    with wx.DirDialog(dialog, self.t("choose_output_folder"), defaultPath=str(source), style=wx.DD_DEFAULT_STYLE | wx.DD_DIR_MUST_EXIST) as chooser:
                        if chooser.ShowModal() != wx.ID_OK:
                            self.announce_player(self.t("conversion_cancelled"))
                            return
                        output_folder = Path(chooser.GetPath()).expanduser()
                    self.start_folder_conversion(source, output_folder, target, image_path)
                else:
                    if not self.converter_input_kind(source):
                        self.message(self.t("unsupported_input_format"), wx.ICON_WARNING)
                        return
                    default_output = self.converter_default_output_path(source, target)
                    with wx.FileDialog(
                        dialog,
                        self.t("choose_output_file"),
                        defaultDir=str(default_output.parent),
                        defaultFile=default_output.name,
                        wildcard=self.converter_wildcard_for_target(target),
                        style=wx.FD_SAVE | wx.FD_OVERWRITE_PROMPT,
                    ) as chooser:
                        if chooser.ShowModal() != wx.ID_OK:
                            self.announce_player(self.t("conversion_cancelled"))
                            return
                        output = Path(chooser.GetPath()).expanduser()
                    if not output.suffix:
                        output = output.with_suffix(f".{self.converter_output_extension(target)}")
                    self.start_file_conversion(source, output, target, image_path)
                dialog.EndModal(wx.ID_OK)

            browse_button.Bind(wx.EVT_BUTTON, browse_path)
            image_button.Bind(wx.EVT_BUTTON, browse_image)
            path_ctrl.Bind(wx.EVT_TEXT, update_detected_and_formats)
            target_choice.Bind(wx.EVT_CHOICE, lambda evt: update_audio_video_controls())
            add_image_box.Bind(wx.EVT_CHECKBOX, on_add_image)
            dark_box.Bind(wx.EVT_CHECKBOX, on_dark)
            convert_button.Bind(wx.EVT_BUTTON, convert)
            update_detected_and_formats()
            dialog.Fit()
            dialog.SetMinSize((600, -1))
            dialog.ShowModal()
        finally:
            dialog.Destroy()

    def converter_input_wildcard(self) -> str:
        audio_patterns = ";".join(f"*{extension}" for extension in sorted(AUDIO_INPUT_EXTENSIONS))
        video_patterns = ";".join(f"*{extension}" for extension in sorted(VIDEO_INPUT_EXTENSIONS))
        all_patterns = ";".join(f"*{extension}" for extension in sorted(CONVERTER_MEDIA_EXTENSIONS))
        return (
            f"{self.t('media_files')}|{all_patterns}|"
            f"{self.t('audio_files')}|{audio_patterns}|"
            f"{self.t('video_files')}|{video_patterns}|"
            f"{self.t('all_files')} (*.*)|*.*"
        )

    def converter_image_wildcard(self) -> str:
        patterns = ";".join(f"*{extension}" for extension in sorted(CONVERTER_IMAGE_EXTENSIONS))
        return f"{self.t('image_files')}|{patterns}|{self.t('all_files')} (*.*)|*.*"

    def start_file_conversion(self, source: Path, output: Path, target_format: str, image_path: Path | None = None) -> None:
        output = self.unique_converter_output_path(output, source)
        self.announce_player(self.t("conversion_started"))
        self.set_status(self.t("conversion_started"))
        threading.Thread(target=self.file_conversion_worker, args=(source, output, target_format, image_path), daemon=True).start()

    def start_folder_conversion(self, source_folder: Path, output_folder: Path, target_format: str, image_path: Path | None = None) -> None:
        self.announce_player(self.t("conversion_started"))
        self.set_status(self.t("conversion_started"))
        threading.Thread(target=self.folder_conversion_worker, args=(source_folder, output_folder, target_format, image_path), daemon=True).start()

    def file_conversion_worker(self, source: Path, output: Path, target_format: str, image_path: Path | None = None) -> None:
        try:
            ffmpeg = self.ffmpeg_executable()
            if not ffmpeg:
                raise RuntimeError("FFmpeg was not found")
            output.parent.mkdir(parents=True, exist_ok=True)
            args = self.converter_ffmpeg_args(ffmpeg, source, output, target_format, image_path)
            self.run_ffmpeg_conversion(args)
            wx.CallAfter(self.set_status, self.t("conversion_done", title=output.name))
            wx.CallAfter(self.announce_player, self.t("conversion_done", title=output.name))
        except Exception as exc:
            wx.CallAfter(self.message, self.t("conversion_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def folder_conversion_worker(self, source_folder: Path, output_folder: Path, target_format: str, image_path: Path | None = None) -> None:
        try:
            ffmpeg = self.ffmpeg_executable()
            if not ffmpeg:
                raise RuntimeError("FFmpeg was not found")
            files = self.converter_media_files_in_folder(source_folder)
            if not files:
                wx.CallAfter(self.message, self.t("conversion_no_media_files"), wx.ICON_INFORMATION)
                return
            output_folder.mkdir(parents=True, exist_ok=True)
            converted = 0
            failed = 0
            for index, source in enumerate(files, start=1):
                target = self.unique_converter_output_path(output_folder / f"{source.stem}.{self.converter_output_extension(target_format)}", source)
                self.ui_queue.put(("status", f"{self.t('conversion_started')} {index}/{len(files)}: {source.name}"))
                try:
                    args = self.converter_ffmpeg_args(ffmpeg, source, target, target_format, image_path)
                    self.run_ffmpeg_conversion(args)
                    converted += 1
                except Exception:
                    failed += 1
                    continue
            text = self.t("conversion_folder_done_with_errors", count=converted, failed=failed) if failed else self.t("conversion_folder_done", count=converted)
            wx.CallAfter(self.set_status, text)
            wx.CallAfter(self.announce_player, text)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("conversion_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    @staticmethod
    def unique_converter_output_path(path: Path, source: Path | None = None) -> Path:
        candidate = path
        counter = 2
        while candidate.exists() or (source is not None and candidate.resolve() == source.resolve()):
            candidate = path.with_name(f"{path.stem} ({counter}){path.suffix}")
            counter += 1
        return candidate

    def run_ffmpeg_conversion(self, args: list[str]) -> None:
        creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
        result = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace", creationflags=creationflags)
        if result.returncode != 0:
            error = (result.stderr or result.stdout or "").strip() or f"FFmpeg exited with code {result.returncode}"
            raise RuntimeError(error[-900:])

    def converter_audio_codec_args(self, target_format: str) -> list[str]:
        fmt = target_format.lower()
        if fmt == "mp3":
            return ["-vn", "-c:a", "libmp3lame", "-b:a", "320k"]
        if fmt in {"m4a", "aac"}:
            return ["-vn", "-c:a", "aac", "-b:a", "256k"]
        if fmt == "alac":
            return ["-vn", "-c:a", "alac"]
        if fmt == "opus":
            return ["-vn", "-c:a", "libopus", "-b:a", "160k"]
        if fmt == "ogg":
            return ["-vn", "-c:a", "libvorbis", "-q:a", "5"]
        if fmt == "wma":
            return ["-vn", "-c:a", "wmav2", "-b:a", "192k"]
        if fmt == "ac3":
            return ["-vn", "-c:a", "ac3", "-b:a", "192k"]
        if fmt == "mp2":
            return ["-vn", "-c:a", "mp2", "-b:a", "192k"]
        if fmt == "aiff":
            return ["-vn", "-c:a", "pcm_s16be"]
        if fmt == "wav":
            return ["-vn", "-c:a", "pcm_s16le"]
        if fmt == "flac":
            return ["-vn", "-c:a", "flac"]
        return ["-vn", "-c:a", "aac", "-b:a", "256k"]

    def converter_video_codec_args(self, target_format: str) -> list[str]:
        fmt = target_format.lower()
        if fmt == "webm":
            return ["-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "32", "-c:a", "libopus", "-b:a", "160k"]
        if fmt == "avi":
            return ["-c:v", "mpeg4", "-q:v", "4", "-c:a", "libmp3lame", "-b:a", "192k"]
        if fmt in {"wmv", "asf"}:
            return ["-c:v", "wmv2", "-b:v", "2500k", "-c:a", "wmav2", "-b:a", "192k"]
        if fmt in {"mpg", "mpeg"}:
            return ["-c:v", "mpeg2video", "-q:v", "4", "-c:a", "mp2", "-b:a", "192k"]
        if fmt in {"ts", "m2ts"}:
            return ["-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", "-f", "mpegts"]
        if fmt == "flv":
            return ["-c:v", "flv", "-q:v", "4", "-c:a", "libmp3lame", "-b:a", "192k"]
        if fmt == "ogv":
            return ["-c:v", "libtheora", "-q:v", "7", "-c:a", "libvorbis", "-q:a", "5"]
        extra = ["-movflags", "+faststart"] if fmt in {"mp4", "m4v", "mov"} else []
        return ["-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", *extra]

    def converter_ffmpeg_args(self, ffmpeg: str, source: Path, output: Path, target_format: str, image_path: Path | None = None) -> list[str]:
        source_kind = self.converter_input_kind(source)
        if not source_kind:
            raise RuntimeError(self.t("unsupported_input_format"))
        target_format = target_format.lower()
        args = [ffmpeg, "-y", "-hide_banner", "-loglevel", "error"]
        if target_format in AUDIO_CONVERT_FORMATS:
            return [*args, "-i", str(source), *self.converter_audio_codec_args(target_format), str(output)]
        if target_format not in VIDEO_CONVERT_FORMATS:
            raise RuntimeError(self.t("unsupported_input_format"))
        if source_kind == "audio":
            if image_path:
                args.extend(["-loop", "1", "-framerate", "1", "-i", str(image_path), "-i", str(source)])
                args.extend(["-shortest", "-map", "0:v:0", "-map", "1:a:0", "-vf", "scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:(ow-iw)/2:(oh-ih)/2,fps=30"])
            else:
                args.extend(["-f", "lavfi", "-i", "color=c=black:s=1280x720:r=30", "-i", str(source)])
                args.extend(["-shortest", "-map", "0:v:0", "-map", "1:a:0"])
            args.extend(self.converter_video_codec_args(target_format))
            args.append(str(output))
            return args
        return [*args, "-i", str(source), *self.converter_video_codec_args(target_format), str(output)]

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
        self.folder_screen_active = False
        self.clear()
        self.add_background_player_section()
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
        self.add_background_player_section()
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
        elif self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
        elif self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
        elif self.shortcut_matches(event, "open_channel"):
            self.open_item_channel(self.selected_user_playlist_item())
        elif self.shortcut_matches(event, "remove_from_playlist") or self.shortcut_matches(event, "remove_selected"):
            self.remove_selected_user_playlist_item()
        elif self.context_menu_shortcut_matches(event):
            self.open_user_playlist_items_context_menu()
        else:
            event.Skip()

    def open_user_playlist_items_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        selected = self.selected_user_playlist_item()
        actions = [
            (self.t("play"), self.play_selected_user_playlist_item),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.start_download(True, item=self.selected_user_playlist_item())),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.start_download(False, item=self.selected_user_playlist_item())),
            (self.t("download_user_playlist"), self.download_current_user_playlist),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playlist", "remove_from_playlist"), self.remove_selected_user_playlist_item),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_user_playlist_item())),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda: self.copy_direct_stream_url(self.selected_user_playlist_item())),
        ]
        if self.item_has_openable_youtube_channel(selected):
            actions.insert(6, (self.menu_label_with_shortcut("open_channel", "open_channel"), lambda selected=dict(selected or {}): self.open_item_channel(selected)))
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
        items = [
            dict(item, audio_only=False, download_folder_override=folder)
            for item in list(playlist.get("items") or [])
            if item.get("url") and item.get("kind") != "local_file"
        ]
        if not items:
            self.announce_player(self.t("playlist_empty"))
            return
        self.announce_player(self.t("batch_download_start", count=len(items)))
        task_id, cancel_event = self.register_download_task({"title": title, "kind": "playlist"}, False, "batch", total=len(items))
        done_text = self.t("download_playlist_done", title=title)
        threading.Thread(target=self.download_batch_worker, args=(items, task_id, cancel_event, done_text, folder), daemon=True).start()

    def add_active_to_playlist(self, prefer_active: bool = False) -> None:
        items = self.playlist_candidate_items(prefer_active=prefer_active)
        if not items:
            self.message(self.t("no_selection"))
            return
        playlist_index = self.choose_or_create_playlist_index()
        if playlist_index is None:
            return
        self.add_items_to_playlist(playlist_index, items)

    def playlist_candidate_items(self, prefer_active: bool = False) -> list[dict]:
        item = self.active_item()
        if prefer_active and item and self.playlist_item_is_supported(item):
            return [dict(item)]
        queued_items = [dict(item) for item in self.download_queue.values() if self.playlist_item_is_supported(item)]
        if len(queued_items) > 1:
            return queued_items
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
        self.clear_playlist_candidate_download_queue_items(items)
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

    def remove_active_from_playlist(self) -> None:
        if self.user_playlist_items_screen_active:
            self.remove_selected_user_playlist_item()
            return
        item = self.active_item()
        if not self.playlist_item_is_supported(item):
            self.announce_player(self.t("no_selection"))
            return
        url = str(item.get("url") or "")
        matches: list[tuple[int, int, str]] = []
        for playlist_index, playlist in enumerate(self.user_playlists):
            for item_index, playlist_item in enumerate(list(playlist.get("items") or [])):
                if str(playlist_item.get("url") or "") == url:
                    matches.append((playlist_index, item_index, str(playlist.get("title") or "")))
                    break
        if not matches:
            self.announce_player(self.t("not_in_playlist"))
            return
        selected_match = matches[0]
        if len(matches) > 1:
            choices = [title or self.t("playlists") for _playlist_index, _item_index, title in matches]
            with wx.SingleChoiceDialog(self, self.t("select_playlist"), self.t("remove_from_playlist"), choices) as dialog:
                if dialog.ShowModal() != wx.ID_OK:
                    return
                selected_index = dialog.GetSelection()
            if 0 <= selected_index < len(matches):
                selected_match = matches[selected_index]
        playlist_index, item_index, _title = selected_match
        playlist = self.user_playlists[playlist_index]
        items = list(playlist.get("items") or [])
        if 0 <= item_index < len(items):
            del items[item_index]
            playlist["items"] = items
            playlist["updated_at"] = time.time()
            self.save_user_playlists()
            if self.user_playlist_items_screen_active and playlist_index == self.current_user_playlist_index:
                self.refresh_user_playlist_items(min(item_index, len(items) - 1))
            self.announce_player(self.t("removed_from_playlist"))

    def clear_playlist_candidate_download_queue_items(self, items: list[dict]) -> None:
        changed = False
        for item in items:
            url = str(item.get("url") or "")
            if url and url in self.download_queue and self.playlist_item_is_supported(self.download_queue.get(url)):
                self.download_queue.pop(url, None)
                changed = True
        if not changed:
            return
        self.refresh_results_list_labels()
        if self.in_queue_screen:
            self.refresh_queue_view()
        self.refresh_download_views()

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

    def append_add_to_playlist_menu(self, menu: wx.Menu, prefer_active: bool = False) -> None:
        if self.user_playlists:
            submenu = wx.Menu()
            for index, playlist in enumerate(self.user_playlists):
                menu_item = submenu.Append(wx.ID_ANY, str(playlist.get("title") or self.t("playlists")))
                self.Bind(wx.EVT_MENU, lambda _evt, idx=index, prefer=prefer_active: self.add_items_to_playlist(idx, self.playlist_candidate_items(prefer_active=prefer)), menu_item)
            create_item = submenu.Append(wx.ID_ANY, self.t("create_playlist"))
            self.Bind(wx.EVT_MENU, lambda _evt, prefer=prefer_active: self.add_active_to_playlist(prefer_active=prefer), create_item)
            menu.AppendSubMenu(submenu, self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"))
        else:
            item = menu.Append(wx.ID_ANY, self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"))
            self.Bind(wx.EVT_MENU, lambda _evt, prefer=prefer_active: self.add_active_to_playlist(prefer_active=prefer), item)

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
        self.add_background_player_section()
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

    def open_notification_center_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_notification_center)

    def leave_player_for_global_navigation(self) -> None:
        if not self.in_player_screen:
            return
        keep_playing = self.background_playback_enabled()
        if not keep_playing:
            self.stop_player(silent=True)
        self.in_player_screen = False
        self.player_control_mode = keep_playing and self.player_control_mode

    def run_global_navigation_shortcut(self, handler) -> None:
        self.leave_player_for_global_navigation()
        handler()

    def open_main_menu_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_main_menu)

    def open_search_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_search)

    def open_play_from_folder_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_play_from_folder)

    def open_direct_link_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_direct_link)

    def open_favorites_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_favorites)

    def open_playlists_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_user_playlists)

    def open_subscriptions_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_subscriptions)

    def open_current_downloads_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_download_queue)

    def open_history_shortcut(self) -> None:
        if self.settings.enable_history:
            self.run_global_navigation_shortcut(self.show_history)

    def open_podcasts_rss_shortcut(self) -> None:
        if self.settings.enable_podcasts_rss:
            self.run_global_navigation_shortcut(self.show_rss_feeds)

    def open_settings_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.open_settings_screen)

    def refresh_notification_center(self) -> None:
        if not hasattr(self, "notification_list"):
            return
        try:
            selection = self.notification_list.GetSelection()
            if self.notifications:
                self.set_listbox_items(self.notification_list, [self.notification_line(notification) for notification in self.notifications], max(0, selection))
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
        self.folder_screen_active = False
        self.clear()
        self.add_background_player_section()
        buttons = [(self.t("back"), self.show_main_menu)]
        if self.download_queue:
            buttons.append((self.t("download_all_as_audio"), lambda: self.download_all_queued(True)))
            buttons.append((self.t("download_all_as_video"), lambda: self.download_all_queued(False)))
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
        if not isinstance(item.get("audio_only"), bool):
            return self.t("selected_queued_marker")
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
                (self.t("download_all_as_audio"), lambda: self.download_all_queued(True)),
                (self.t("download_all_as_video"), lambda: self.download_all_queued(False)),
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
        self.trending_screen_active = False
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
        self.folder_screen_active = False
        self.clear()
        self.add_background_player_section()
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

    def prompt_initial_language(self) -> None:
        if self.settings.language_prompted:
            return
        choices = [name for _code, name in LANGUAGES]
        current = LANGUAGE_CODES.index(self.settings.language) if self.settings.language in LANGUAGE_CODES else 0
        with wx.SingleChoiceDialog(self, self.t("first_run_language_prompt"), self.t("language"), choices) as dialog:
            dialog.SetSelection(current)
            if dialog.ShowModal() == wx.ID_OK:
                index = dialog.GetSelection()
                if 0 <= index < len(LANGUAGE_CODES):
                    self.settings.language = LANGUAGE_CODES[index]
        self.settings.language_prompted = True
        self.save_settings()
        self.show_main_menu()
        self.announce_player(self.t("settings_saved"))

    def back_from_search(self) -> None:
        if self.search_results_stack:
            self.restore_previous_search_results()
        else:
            self.show_main_menu()

    def on_results_key(self, event: wx.KeyEvent) -> None:
        if self.result_details_key(event):
            self.announce_selected_result_details()
        elif self.shortcut_matches(event, "queue_audio"):
            self.toggle_download_queue()
        elif self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
        elif self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
        elif self.shortcut_matches(event, "add_favorite"):
            self.add_selected_favorite()
        elif self.shortcut_matches(event, "remove_favorite"):
            self.remove_selected_favorite_shortcut()
        elif self.shortcut_matches(event, "add_to_playlist"):
            self.add_active_to_playlist()
        elif self.shortcut_matches(event, "remove_from_playlist"):
            self.remove_active_from_playlist()
        elif self.shortcut_matches(event, "download_audio"):
            self.download_audio_shortcut()
        elif self.shortcut_matches(event, "download_video"):
            self.download_video_shortcut()
        elif self.shortcut_matches(event, "subscribe_channel"):
            self.subscribe_shortcut()
        elif self.shortcut_matches(event, "unsubscribe_channel"):
            self.unsubscribe_shortcut()
        elif self.shortcut_matches(event, "open_channel"):
            self.open_item_channel(self.selected_result())
        elif self.shortcut_matches(event, "copy_link"):
            self.copy_selected_url()
        elif self.shortcut_matches(event, "open_selected"):
            self.play_selected()
        elif self.context_menu_shortcut_matches(event):
            self.open_context_menu()
        else:
            event.Skip()
            wx.CallAfter(self.maybe_extend_results)

    @staticmethod
    def result_details_key(event: wx.KeyEvent) -> bool:
        return (
            event.GetKeyCode() == wx.WXK_SPACE
            and not event.ControlDown()
            and not event.ShiftDown()
            and not event.AltDown()
        )

    def announce_selected_result_details(self) -> None:
        item = self.selected_result()
        if not item:
            self.announce_player(self.t("no_selection"))
            return
        self.announce_player(self.result_details_text(item))

    def result_details_text(self, item: dict) -> str:
        kind = str(item.get("kind") or "")
        title = str(item.get("title") or "")
        if kind == "local_file":
            return self.t(
                "local_file_result_details",
                title=title,
                format=str(item.get("ext") or self.t("file_format_unknown")),
                folder=str(item.get("folder") or item.get("channel") or self.t("unknown")),
                path=str(item.get("path") or item.get("url") or item.get("webpage_url") or ""),
            )
        if kind == "playlist":
            count = self.playlist_count_text(item) or self.t("playlist")
            return self.t("playlist_result_details", title=title, count=count)
        if kind == "channel":
            return self.t("channel_result_details", title=title)
        return self.t(
            "results_details",
            title=title,
            duration=str(item.get("duration") or self.t("unknown")),
            channel=str(item.get("channel") or self.t("unknown")),
            views=str(item.get("views") or self.t("unknown")),
            age=str(item.get("age") or self.t("uploaded_unknown")),
            type=str(item.get("type") or self.t("video")),
        )

    def on_results_selection(self, event) -> None:
        event.Skip()
        selection = self.current_results_selection(-1)
        self.apply_deferred_result_line_updates(exclude_index=selection)
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
        self.folder_screen_active = False
        self.clear()
        self.add_background_player_section()
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
        elif self.shortcut_matches(event, "unsubscribe_channel"):
            self.unsubscribe_from_selected_channel(self.selected_favorite())
        elif self.shortcut_matches(event, "open_channel"):
            self.open_item_channel(self.selected_favorite())
        elif self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
        elif self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
        elif self.context_menu_shortcut_matches(event):
            self.open_favorites_context_menu()
        else:
            event.Skip()

    def open_favorites_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        selected = self.selected_favorite()
        actions = [
            (self.t("play"), self.play_favorite),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.start_download(True, item=self.selected_favorite())),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.start_download(False, item=self.selected_favorite())),
            (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), lambda: self.subscribe_to_selected_channel(self.selected_favorite())),
            (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), lambda: self.unsubscribe_from_selected_channel(self.selected_favorite())),
            (self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"), self.add_active_to_playlist),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda: self.copy_direct_stream_url(self.selected_favorite())),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_favorite())),
            (self.t("remove"), self.remove_favorite),
        ]
        if self.item_has_openable_youtube_channel(selected):
            actions.insert(5, (self.menu_label_with_shortcut("open_channel", "open_channel"), lambda selected=dict(selected or {}): self.open_item_channel(selected)))
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
        self.folder_screen_active = False
        self.clear()
        self.add_background_player_section()
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
        elif self.shortcut_matches(event, "unsubscribe_channel"):
            self.unsubscribe_from_selected_channel(self.selected_history_item())
        elif self.shortcut_matches(event, "open_channel"):
            self.open_item_channel(self.selected_history_item())
        elif self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
        elif self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
        elif self.shortcut_matches(event, "remove_selected"):
            self.remove_history_item()
        elif self.context_menu_shortcut_matches(event):
            self.open_history_context_menu()
        else:
            event.Skip()

    def open_history_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        selected = self.selected_history_item()
        actions = [
            (self.t("play"), self.play_history_item),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.start_download(True, item=self.selected_history_item())),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.start_download(False, item=self.selected_history_item())),
            (self.t("add_favorite"), lambda: self.add_favorite_item(self.selected_history_item())),
            (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), lambda: self.subscribe_to_selected_channel(self.selected_history_item())),
            (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), lambda: self.unsubscribe_from_selected_channel(self.selected_history_item())),
            (self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"), self.add_active_to_playlist),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda: self.copy_direct_stream_url(self.selected_history_item())),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_history_item())),
            (self.t("remove_history_item"), self.remove_history_item),
            (self.t("clear_history"), self.clear_history),
        ]
        if self.item_has_openable_youtube_channel(selected):
            actions.insert(6, (self.menu_label_with_shortcut("open_channel", "open_channel"), lambda selected=dict(selected or {}): self.open_item_channel(selected)))
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
        self.folder_screen_active = False
        self.clear()
        self.add_background_player_section()
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
            self.open_notification_center_shortcut()
        elif self.shortcut_matches(event, "unsubscribe_channel"):
            self.remove_subscription()
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
            (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), self.remove_subscription),
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
        self.dynamic_fetch_enabled = True
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
        if self.in_main_menu:
            return
        self.subscribe_to_selected_channel(self.active_item())

    def unsubscribe_shortcut(self) -> None:
        if self.in_main_menu:
            return
        if self.subscriptions_screen_active:
            self.remove_subscription()
            return
        self.unsubscribe_from_selected_channel(self.active_item())

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

    def unsubscribe_from_selected_channel(self, item: dict | None) -> None:
        if not item:
            self.message(self.t("no_selection"))
            return
        subscription = self.subscription_from_item(item)
        if not subscription:
            self.message(self.t("no_selection"))
            return
        target_url = self.canonical_channel_url(subscription.get("url", ""))
        target_title = subscription.get("title") or self.t("channel")
        for index, existing in enumerate(list(self.subscriptions)):
            existing_url = self.canonical_channel_url(str(existing.get("url") or ""))
            if existing_url and existing_url == target_url:
                removed = self.subscriptions.pop(index)
                self.save_subscriptions()
                self.refresh_subscriptions()
                self.announce_player(self.t("subscription_removed", title=removed.get("title") or target_title))
                return
        self.announce_player(self.t("subscription_not_found", title=target_title))

    def subscription_from_item(self, item: dict) -> dict | None:
        kind = item.get("kind")
        channel_url = self.normalize_channel_url(item)
        if kind == "channel":
            channel_url = str(item.get("url") or channel_url).strip()
        if not channel_url and item.get("latest_urls") is not None:
            channel_url = str(item.get("url") or "").strip()
        channel_url = self.canonical_channel_url(channel_url)
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

    @staticmethod
    def canonical_channel_url(url: str) -> str:
        text = str(url or "").strip()
        if not text:
            return ""
        if text.startswith("@") or text.startswith("/@"):
            text = f"https://www.youtube.com/{text.lstrip('/')}"
        elif text and not text.startswith("http"):
            text = f"https://www.youtube.com/{text.lstrip('/')}"
        base = text.split("?", 1)[0].split("#", 1)[0].rstrip("/")
        base = re.sub(r"/(videos|playlists|featured|streams|shorts|community|about)$", "", base, flags=re.IGNORECASE)
        return base.rstrip("/")

    def refresh_interval_seconds(self, value, default: float, maximum_hours: float = 168.0) -> float:
        hours = self.to_float(str(value), default, 0.5, maximum_hours)
        return max(30 * 60, hours * 60 * 60)

    def configure_subscription_timer(self) -> None:
        if not hasattr(self, "subscription_timer"):
            return
        try:
            self.subscription_timer.Stop()
        except Exception:
            pass
        if self.settings.subscription_check_enabled:
            interval_ms = int(self.refresh_interval_seconds(self.settings.subscription_check_interval_hours, 6.0) * 1000)
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
            interval_ms = int(self.refresh_interval_seconds(self.settings.rss_refresh_interval_hours, 12.0) * 1000)
            self.rss_timer.Start(interval_ms)

    def on_rss_timer(self, _event) -> None:
        self.refresh_all_rss_feeds_background()

    def configure_app_update_timer(self) -> None:
        if not hasattr(self, "app_update_timer"):
            return
        try:
            self.app_update_timer.Stop()
        except Exception:
            pass
        if self.settings.auto_update_app:
            interval_ms = int(self.refresh_interval_seconds(self.settings.app_update_interval_hours, 6.0, maximum_hours=24.0) * 1000)
            self.app_update_timer.Start(interval_ms)

    def on_app_update_timer(self, _event) -> None:
        self.start_app_update_check(manual=False, prompt=False, notify=True)

    def check_subscriptions_if_due(self) -> None:
        if not self.settings.subscription_check_enabled or not self.subscriptions:
            return
        interval_seconds = self.refresh_interval_seconds(self.settings.subscription_check_interval_hours, 6.0)
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
        url = self.collection_download_url({"kind": "channel", "url": subscription.get("url", "")})
        options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": 5}
        info = self.ydl_extract_info(url, options, download=False)
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
        self.add_background_player_section()
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
        self.add_background_player_section()
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
        elif self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
        elif self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
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
            (self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"), self.add_active_to_playlist),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
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

    def open_settings_screen(self) -> None:
        self.settings_section_index = 0
        self.show_settings()

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
        self.add_background_player_section()
        self.add_button_row([(self.t("back"), self.show_main_menu), (self.t("save"), self.save_settings_from_ui), (self.t("restore_defaults"), self.restore_default_settings)])
        self.controls = {}
        self.choice_values = {}
        self.settings_control_order = []
        self.settings_render_generation = 0
        self.settings_pending_section_index = -1
        self.settings_controls_applied_for_pending = False
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
            (self.t("equalizer_section"), "equalizer"),
            (self.t("downloads_section"), "downloads"),
            (self.t("library_section"), "library"),
            (self.t("podcasts_section"), "podcasts"),
            (self.t("notifications_section"), "notifications"),
            (self.t("cookies_network_section"), "cookies"),
            (self.t("keyboard_shortcuts_section"), "shortcuts"),
        ]

    def settings_section_label(self, section_name: str) -> str:
        for label, name in self.settings_sections():
            if name == section_name:
                return label
        return section_name

    @staticmethod
    def settings_section_fields() -> dict[str, list[str]]:
        return {
            "general": [
                "language",
                "download_folder",
                "results_limit",
                "direct_link_enter_action",
                "auto_update_ytdlp",
                "auto_update_app",
                "app_update_interval_hours",
                "app_update_notifications",
                "close_to_tray",
                "start_with_windows",
                "tray_notification",
                "skipped_update_version",
            ],
            "playback": [
                "autoplay_next",
                "prefer_browser_playback",
                "player_fullscreen",
                "player_start_paused",
                "announce_play_pause",
                "announce_playback_finished",
                "enable_background_playback",
                "player_speed",
                "speed_audio_mode",
                "show_video_details_by_default",
                "enable_age_restricted_videos",
                "enable_stream_cache",
                "cache_folder",
                "cache_size_mb",
                "resume_playback",
                "audio_output_device",
                "speed_step",
                "pitch_step",
                "pitch_mode",
                "seek_seconds",
                "volume_step",
                "default_volume",
                "volume_boost_by_default",
            ],
            "equalizer": [
                "global_equalizer_enabled",
                "global_equalizer_preset",
                "global_equalizer_gains",
                "equalizer_preset_gains",
                "equalizer_custom_names",
                "equalizer_db_range",
            ],
            "downloads": [
                "audio_format",
                "video_format",
                "max_video_height",
                "ask_download_location_each_time",
                "quiet_downloads",
                "keep_playlist_order",
                "filename_template",
                "audio_quality",
                "write_thumbnail",
                "write_description",
                "write_info_json",
                "write_subtitles",
                "auto_subtitles",
                "subtitle_languages",
                "embed_metadata",
                "embed_thumbnail",
                "restrict_filenames",
                "open_folder_after_download",
                "popup_when_download_complete",
                "confirm_before_download",
                "download_archive",
            ],
            "library": [
                "subscription_check_enabled",
                "subscription_check_interval_hours",
                "last_subscription_check",
                "enable_trending",
                "enable_history",
                "history_limit",
            ],
            "podcasts": [
                "enable_podcasts_rss",
                "podcast_search_provider",
                "podcast_search_country",
                "podcast_search_limit",
                "rss_max_items",
                "rss_refresh_on_startup",
                "rss_auto_refresh_enabled",
                "rss_refresh_interval_hours",
            ],
            "notifications": [
                "windows_notifications",
                "download_notifications",
                "subscription_notifications",
                "app_update_notifications",
            ],
            "cookies": [
                "rate_limit",
                "proxy",
                "youtube_data_api_key",
                "cookies_file",
                "cookies_from_browser",
                "cookies_browser_profile",
                "show_advanced_network_settings",
                "cookie_user_agent",
                "ffmpeg_location",
                "concurrent_fragments",
                "retries",
                "socket_timeout",
            ],
            "shortcuts": ["keyboard_shortcuts"],
        }

    def on_settings_section_changed(self, event) -> None:
        event.Skip()
        if not hasattr(self, "settings_section_list"):
            return
        new_index = self.settings_section_list.GetSelection()
        if new_index < 0 or new_index == self.settings_section_index and self.settings_pending_section_index < 0:
            return
        if not self.settings_controls_applied_for_pending:
            self.apply_settings_from_visible_controls()
            self.settings_controls_applied_for_pending = True
        self.settings_pending_section_index = new_index
        self.settings_render_generation += 1
        wx.CallLater(140, self.render_pending_settings_section, self.settings_render_generation)

    def render_pending_settings_section(self, generation: int) -> None:
        if generation != self.settings_render_generation or self.settings_pending_section_index < 0:
            return
        self.settings_section_index = self.settings_pending_section_index
        self.settings_pending_section_index = -1
        self.settings_controls_applied_for_pending = False
        self.render_settings_section()

    def flush_settings_section_render(self) -> None:
        if self.settings_pending_section_index < 0:
            return
        self.settings_render_generation += 1
        self.settings_section_index = self.settings_pending_section_index
        self.settings_pending_section_index = -1
        self.settings_controls_applied_for_pending = False
        self.render_settings_section()

    def on_settings_section_key(self, event: wx.KeyEvent) -> None:
        if event.GetKeyCode() == wx.WXK_RETURN:
            self.flush_settings_section_render()
            self.focus_first_settings_control()
        else:
            event.Skip()

    def focus_first_settings_control(self) -> None:
        if self.settings_control_order:
            self.safe_set_focus(self.settings_control_order[0])

    def on_advanced_network_toggle(self, _event: wx.CommandEvent) -> None:
        self.apply_settings_from_visible_controls()
        self.render_settings_section_and_focus("show_advanced_network_settings")

    def render_settings_section(self) -> None:
        if not hasattr(self, "settings_scroller"):
            return
        try:
            self.settings_scroller.Freeze()
        except RuntimeError:
            pass
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
            return ctrl

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
            return ctrl

        def check(key: str, value: bool):
            form.AddSpacer(1)
            ctrl = wx.CheckBox(self.settings_scroller, label=self.t(key))
            ctrl.SetName(self.t(key))
            ctrl.SetValue(value)
            form.Add(ctrl, 1, wx.EXPAND)
            remember(key, ctrl)
            return ctrl

        def button(key: str, handler):
            form.AddSpacer(1)
            ctrl = wx.Button(self.settings_scroller, label=self.t(key))
            ctrl.SetName(self.t(key))
            ctrl.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            form.Add(ctrl, 0)
            self.settings_control_order.append(ctrl)
            return ctrl

        def button_label(label: str, handler):
            form.AddSpacer(1)
            ctrl = wx.Button(self.settings_scroller, label=label)
            ctrl.SetName(label)
            ctrl.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            form.Add(ctrl, 0)
            self.settings_control_order.append(ctrl)
            return ctrl

        def slider(key: str, label: str, value: float, minimum: int, maximum: int):
            form.Add(wx.StaticText(self.settings_scroller, label=label), 0, wx.ALIGN_CENTER_VERTICAL)
            scaled_value = int(round(float(value) * 10))
            ctrl = wx.Slider(
                self.settings_scroller,
                value=min(max(scaled_value, minimum), maximum),
                minValue=minimum,
                maxValue=maximum,
                style=wx.SL_HORIZONTAL,
            )
            self.set_equalizer_slider_accessibility(ctrl, label)
            ctrl.Bind(wx.EVT_SLIDER, lambda evt, label_text=label: self.on_equalizer_settings_slider(evt, label_text))
            form.Add(ctrl, 1, wx.EXPAND)
            remember(key, ctrl)
            return ctrl

        def int_slider(key: str, label: str, value: int, minimum: int, maximum: int):
            form.Add(wx.StaticText(self.settings_scroller, label=label), 0, wx.ALIGN_CENTER_VERTICAL)
            ctrl = wx.Slider(
                self.settings_scroller,
                value=min(max(int(value), minimum), maximum),
                minValue=minimum,
                maxValue=maximum,
                style=wx.SL_HORIZONTAL,
            )
            unit = "percent" if key == "default_volume" else ""
            self.set_integer_slider_accessibility(ctrl, label, unit)
            ctrl.Bind(wx.EVT_SLIDER, lambda evt, label_text=label, unit_text=unit: self.set_integer_slider_accessibility(evt.GetEventObject(), label_text, unit_text))
            form.Add(ctrl, 1, wx.EXPAND)
            remember(key, ctrl)
            return ctrl

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
            button("set_default_player", self.open_windows_default_apps_settings)
            results_limit_value = "0" if self.settings.results_limit == 0 else str(min(250, self.settings.results_limit))
            result_limit_options = ["0", "10", "20", "50", "100", "150", "200", "250"]
            choice("results_limit", results_limit_value, result_limit_options, self.result_limit_labels(result_limit_options))
            choice("direct_link_enter_action", self.normalized_direct_link_enter_action(), DIRECT_LINK_ENTER_OPTIONS, self.direct_link_enter_action_labels())
            check("auto_update", self.settings.auto_update_ytdlp)
            check("auto_update_app", self.settings.auto_update_app)
            choice(
                "app_update_interval",
                self.format_refresh_interval_value(self.settings.app_update_interval_hours, 6.0),
                REFRESH_INTERVAL_OPTIONS,
                self.refresh_interval_labels(),
            )
            button("check_app_updates_now", self.manual_app_update_check)
            check("close_to_tray", self.settings.close_to_tray)
            check("start_with_windows", self.settings.start_with_windows)
            check("tray_notification", self.settings.tray_notification)
            button("reset_all_settings", self.restore_default_settings)
        elif section_name == "playback":
            choice("player_speed", self.settings.player_speed, [self.format_playback_rate(step) for step in PLAYBACK_SPEED_STEPS if step <= 2.0])
            choice("speed_audio_mode", self.normalized_speed_audio_mode(), SPEED_AUDIO_MODE_OPTIONS, self.speed_audio_mode_labels())
            choice("pitch_mode", self.normalized_pitch_mode(), PITCH_MODE_OPTIONS, self.pitch_mode_labels())
            choice("speed_step", self.format_step_value(self.settings.speed_step), RATE_STEP_OPTIONS)
            choice("pitch_step", self.format_step_value(self.settings.pitch_step), RATE_STEP_OPTIONS)
            check("show_video_details_by_default", self.settings.show_video_details_by_default)
            check("enable_age_restricted_videos", self.settings.enable_age_restricted_videos)
            check("enable_stream_cache", self.settings.enable_stream_cache)
            text("cache_folder", self.settings.cache_folder or str(DEFAULT_CACHE_DIR))
            choice("cache_size_mb", str(self.settings.cache_size_mb), ["128", "256", "512", "1024", "2048", "4096"])
            check("resume_playback", self.settings.resume_playback)
            device_values, device_labels = self.audio_output_device_options(allow_probe=False)
            choice("default_audio_device", self.normalized_audio_output_device(), device_values, device_labels)
            self.refresh_audio_output_devices_async()
            choice("seek_seconds", str(self.settings.seek_seconds), ["5", "10", "15", "30"])
            choice("volume_step", str(self.settings.volume_step), ["1", "2", "5", "10"])
            int_slider("default_volume", self.t("default_volume"), self.default_volume_value(), 0, 300)
            check("volume_boost_by_default", bool(getattr(self.settings, "volume_boost_by_default", False)))
            check("autoplay_next", self.settings.autoplay_next)
            check("browser_playback", self.settings.prefer_browser_playback)
            check("fullscreen", self.settings.player_fullscreen)
            check("start_paused", self.settings.player_start_paused)
            check("announce_play_pause", self.settings.announce_play_pause)
            check("announce_playback_finished", bool(getattr(self.settings, "announce_playback_finished", True)))
            check("enable_background_playback", bool(getattr(self.settings, "enable_background_playback", False)))
        elif section_name == "equalizer":
            equalizer_enabled = bool(getattr(self.settings, "global_equalizer_enabled", False))
            enabled_box = check("global_equalizer", equalizer_enabled)
            enabled_box.Bind(wx.EVT_CHECKBOX, self.on_global_equalizer_toggle)
            if equalizer_enabled:
                preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
                self.visible_equalizer_preset = preset
                preset_choice = choice("equalizer_preset", preset, self.equalizer_preset_options(), self.equalizer_preset_labels())
                preset_choice.Bind(wx.EVT_CHOICE, self.on_equalizer_settings_preset_changed)
                if self.is_custom_equalizer_preset(preset):
                    name_ctrl = text("equalizer_preset_name", self.equalizer_custom_name(preset))
                    name_ctrl.Bind(wx.EVT_KILL_FOCUS, self.on_equalizer_settings_name_changed)
                db_range = str(self.equalizer_db_range_value())
                choice("equalizer_db_range", db_range, EQ_RANGE_OPTIONS)
                gains = self.equalizer_gains_for_preset(preset)
                slider_min = -int(db_range) * 10
                slider_max = int(db_range) * 10
                for band_id, band_label in EQ_BANDS:
                    label = self.t("equalizer_band_gain", band=band_label)
                    slider(f"eq_{band_id}", label, gains.get(band_id, 0.0), slider_min, slider_max)
                button("reset_equalizer", self.reset_visible_equalizer_controls)
                button("add_equalizer_profile", self.add_equalizer_profile_from_settings)
        elif section_name == "downloads":
            check("confirm_download", self.settings.confirm_before_download)
            check("open_after_download", self.settings.open_folder_after_download)
            check("download_complete_popup", self.settings.popup_when_download_complete)
            check("ask_download_location_each_time", self.settings.ask_download_location_each_time)
            choice("audio_format", self.settings.audio_format, ["mp3", "m4a", "opus", "wav", "flac"])
            choice("audio_quality", self.normalize_audio_quality_value(self.settings.audio_quality), AUDIO_QUALITY_OPTIONS, self.audio_quality_labels())
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
            check("enable_trending", bool(getattr(self.settings, "enable_trending", False)))
            check("enable_history", self.settings.enable_history)
            choice("history_limit", str(self.settings.history_limit), ["100", "250", "500", "1000", "2000"])
            check("subscription_check_enabled", self.settings.subscription_check_enabled)
            choice(
                "subscription_check_interval",
                self.format_refresh_interval_value(self.settings.subscription_check_interval_hours, 6.0),
                REFRESH_INTERVAL_OPTIONS,
                self.refresh_interval_labels(),
            )
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
            choice(
                "rss_refresh_interval",
                self.format_refresh_interval_value(self.settings.rss_refresh_interval_hours, 12.0),
                REFRESH_INTERVAL_OPTIONS,
                self.refresh_interval_labels(),
            )
        elif section_name == "notifications":
            check("windows_notifications", self.settings.windows_notifications)
            check("download_notifications", self.settings.download_notifications)
            check("subscription_notifications", self.settings.subscription_notifications)
            check("app_update_notifications", self.settings.app_update_notifications)
        elif section_name == "cookies":
            text("cookies", self.settings.cookies_file)
            button("choose_cookies_file", self.choose_cookies_file)
            choice("cookies_from_browser", self.settings.cookies_from_browser or "none", COOKIES_BROWSER_OPTIONS)
            profile_values = self.cookie_profile_choice_values(self.settings.cookies_from_browser or "none")
            profile_value = self.settings.cookies_browser_profile if self.settings.cookies_browser_profile in profile_values else COOKIE_PROFILE_AUTO
            choice("cookies_browser_profile", profile_value, profile_values, self.cookie_profile_choice_labels(profile_values))
            button("open_youtube_login_profile", self.open_youtube_login_profile_from_settings)
            button("export_browser_cookies", self.export_browser_cookies_from_settings)
            text("proxy", self.settings.proxy)
            text("youtube_data_api_key", getattr(self.settings, "youtube_data_api_key", ""))
            advanced_box = check("show_advanced_network_settings", bool(getattr(self.settings, "show_advanced_network_settings", False)))
            advanced_box.Bind(wx.EVT_CHECKBOX, self.on_advanced_network_toggle)
            if bool(getattr(self.settings, "show_advanced_network_settings", False)):
                text("cookie_user_agent", getattr(self.settings, "cookie_user_agent", ""))
                text("rate_limit", self.settings.rate_limit)
                text("ffmpeg", self.settings.ffmpeg_location)
                choice("fragments", str(self.settings.concurrent_fragments), ["1", "2", "4", "8", "16"])
            choice("retries", str(self.settings.retries), ["0", "3", "5", "10", "20"])
            choice("timeout", str(self.settings.socket_timeout), ["5", "10", "20", "30", "60"])
        elif section_name == "shortcuts":
            form.Add(wx.StaticText(self.settings_scroller, label=self.t("keyboard_shortcuts_help")), 0, wx.ALIGN_CENTER_VERTICAL)
            form.AddSpacer(1)
            shortcuts = self.normalized_keyboard_shortcuts(getattr(self.settings, "keyboard_shortcuts", {}) or {})
            self.shortcut_editor_values = dict(shortcuts)
            self.shortcut_editor_actions = [action for action, _label_key in SHORTCUT_DEFINITIONS]
            if self.shortcut_editor_current_action not in self.shortcut_editor_actions:
                self.shortcut_editor_current_action = self.shortcut_editor_actions[0] if self.shortcut_editor_actions else ""
            selected_index = self.shortcut_editor_actions.index(self.shortcut_editor_current_action) if self.shortcut_editor_current_action in self.shortcut_editor_actions else 0
            form.Add(wx.StaticText(self.settings_scroller, label=self.t("shortcut_actions")), 0, wx.ALIGN_CENTER_VERTICAL)
            shortcut_list = wx.ListBox(
                self.settings_scroller,
                choices=[self.shortcut_display_label(action, self.shortcut_editor_values.get(action, "")) for action in self.shortcut_editor_actions],
                style=wx.LB_SINGLE,
            )
            shortcut_list.SetName(self.t("shortcut_actions"))
            shortcut_list.SetMinSize((-1, 260))
            if self.shortcut_editor_actions:
                shortcut_list.SetSelection(selected_index)
            shortcut_list.Bind(wx.EVT_LISTBOX, self.on_shortcut_action_selected)
            form.Add(shortcut_list, 1, wx.EXPAND)
            remember("shortcut_action_list", shortcut_list)
            form.Add(wx.StaticText(self.settings_scroller, label=self.t("shortcut_value")), 0, wx.ALIGN_CENTER_VERTICAL)
            active_value = self.shortcut_editor_values.get(self.shortcut_editor_current_action, "")
            shortcut_ctrl = wx.TextCtrl(self.settings_scroller, value=active_value, style=wx.TE_PROCESS_ENTER)
            shortcut_ctrl.SetName(f"{self.t('shortcut_value')}. {self.t('shortcut_capture_hint')}")
            setattr(shortcut_ctrl, "_apricot_shortcut_capture", True)
            setattr(shortcut_ctrl, "_apricot_shortcut_action", self.shortcut_editor_current_action)
            shortcut_ctrl.Bind(wx.EVT_KEY_DOWN, lambda evt, target=shortcut_ctrl: self.on_shortcut_capture_key(evt, target))
            form.Add(shortcut_ctrl, 1, wx.EXPAND)
            remember("shortcut_active_value", shortcut_ctrl)

        button_label(
            self.t("reset_settings_for_section", section=self.settings_section_label(section_name)),
            lambda name=section_name: self.reset_settings_section(name),
        )
        self.settings_scroller.SetSizer(form, True)
        self.settings_scroller.Layout()
        self.settings_scroller.FitInside()
        self.panel.Layout()
        try:
            self.settings_scroller.Thaw()
        except RuntimeError:
            pass

    def render_settings_section_and_focus(self, focus_key: str | None = None) -> None:
        self.render_settings_section()
        focus = self.controls.get(focus_key or "") if hasattr(self, "controls") else None
        if focus is None and self.settings_control_order:
            focus = self.settings_control_order[0]
        if focus is not None:
            self.focus_later(focus)

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
        return DYNAMIC_RESULTS_MAX if self.settings.results_limit == 0 else self.effective_results_limit()

    def search_worker(self, query: str, search_type: str, limit: int, generation: int) -> None:
        try:
            options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": limit}
            if search_type == "Video":
                info = self.ydl_extract_info(f"ytsearch{limit}:{query}", options, download=False)
            else:
                info = self.ydl_extract_info(self.youtube_search_url(query, search_type), options, download=False)
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
        playlist_count = entry.get("playlist_count") or entry.get("n_entries") or entry.get("video_count") or entry.get("playlist_count_text")
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
            "playlist_count": playlist_count if kind == "playlist" else "",
            "url": url,
        }

    def show_results(self, results: list[dict], selection: int = 0, visible_count: int | None = None, focus_results: bool = True) -> None:
        self.deferred_result_line_updates.clear()
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
            if focus_results:
                self.safe_set_focus(self.results_list)
            self.set_status(self.t("found", count=len(self.results)))
            self.start_result_metadata_hydration()
        else:
            self.set_listbox_items(self.results_list, [self.t("no_results")], 0)
            if focus_results:
                self.safe_set_focus(self.results_list)
            self.set_status(self.t("no_results"))

    def current_results_selection(self, fallback: int = 0) -> int:
        if not hasattr(self, "results_list"):
            return max(0, fallback)
        try:
            selection = self.results_list.GetSelection()
        except RuntimeError:
            return max(0, fallback)
        if selection == wx.NOT_FOUND:
            return max(0, fallback)
        return max(0, int(selection))

    def result_identity_at(self, index: int) -> str:
        if index < 0 or index >= len(self.results):
            return ""
        item = self.results[index]
        return str(item.get("url") or item.get("webpage_url") or item.get("title") or "")

    def result_index_for_identity(self, identity: str, fallback: int, limit: int | None = None) -> int:
        if identity:
            items = self.results[:limit] if limit is not None else self.results
            for index, item in enumerate(items):
                if str(item.get("url") or item.get("webpage_url") or item.get("title") or "") == identity:
                    return index
        if not self.results:
            return 0
        return min(max(0, fallback), len(self.results) - 1)

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
        current_selection = self.current_results_selection(selection)
        current_identity = self.result_identity_at(current_selection)
        previous_count = len(self.results)
        self.last_visible_count = next_count
        self.results = self.all_results[: self.last_visible_count]
        selected_index = self.result_index_for_identity(current_identity, current_selection, next_count)
        labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
        if not self.append_listbox_items(self.results_list, labels, previous_count, selected_index):
            self.set_listbox_items(self.results_list, labels, selected_index)
        self.set_status(self.t("search_more_loaded", count=len(self.results)))
        self.start_result_metadata_hydration()

    def fetch_more_dynamic_results(self, selection: int) -> None:
        if self.loading_more_results:
            return
        current_count = len(self.all_results)
        max_limit = self.max_results_limit()
        if max_limit and current_count >= max_limit:
            self.set_status(self.t("no_more_results"))
            return
        next_limit = current_count + RESULTS_PAGE_SIZE
        if max_limit:
            next_limit = min(max_limit, next_limit)
        self.loading_more_results = True
        self.set_status(self.t("loading_more_results"))
        generation = self.search_generation
        if self.collection_url:
            threading.Thread(target=self.load_collection_worker, args=(self.collection_url, self.collection_result_type or "Video", next_limit, selection, generation), daemon=True).start()
        else:
            threading.Thread(target=self.search_more_worker, args=(self.last_search_query, self.current_search_type_code, next_limit, selection, generation), daemon=True).start()

    def search_more_worker(self, query: str, search_type: str, limit: int, selection: int, generation: int) -> None:
        try:
            options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": limit}
            if search_type == "Video":
                info = self.ydl_extract_info(f"ytsearch{limit}:{query}", options, download=False)
            else:
                info = self.ydl_extract_info(self.youtube_search_url(query, search_type), options, download=False)
            entries = list(info.get("entries") or [])[:limit]
            wx.CallAfter(self.show_more_results_if_current, generation, [self.normalize_entry(entry, search_type) for entry in entries], selection)
        except Exception as exc:
            wx.CallAfter(self.dynamic_fetch_failed_if_current, generation, self.friendly_error(exc))

    def show_more_results(self, results: list[dict], selection: int) -> None:
        self.loading_more_results = False
        if len(results) <= len(self.all_results):
            self.set_status(self.t("no_more_results"))
            return
        current_selection = self.current_results_selection(selection)
        current_identity = self.result_identity_at(current_selection)
        previous_count = len(self.results)
        self.all_results = list(results)
        visible_count = min(len(self.all_results), max(previous_count, previous_count + RESULTS_PAGE_SIZE))
        self.last_visible_count = visible_count
        self.results = self.all_results[:visible_count]
        selected_index = self.result_index_for_identity(current_identity, current_selection, visible_count)
        labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
        if not self.append_listbox_items(self.results_list, labels, previous_count, selected_index):
            self.set_listbox_items(self.results_list, labels, selected_index)
        self.set_status(self.t("search_more_loaded", count=len(self.results)))
        self.start_result_metadata_hydration()

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
            if len(candidates) >= RESULT_METADATA_HYDRATION_BATCH:
                break
        if candidates:
            threading.Thread(target=self.result_metadata_worker, args=(candidates,), daemon=True).start()

    def result_metadata_worker(self, items: list[dict]) -> None:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            return
        options = {"quiet": True, "skip_download": True, "noplaylist": True}
        try:
            for item in items:
                url = str(item.get("url") or "")
                if not url:
                    continue
                try:
                    info = self.ydl_extract_info(url, options, download=False)
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
        if item.get("kind") == "local_file":
            title = str(item.get("relative_path") or item.get("title") or "")
            return self.t(
                "local_file_result_line",
                title=title,
                format=str(item.get("ext") or self.t("file_format_unknown")),
                folder=str(item.get("folder") or item.get("channel") or ""),
            )
        if item.get("kind") in {"playlist", "channel"}:
            parts = [item.get("title", ""), item.get("type", self.t("playlist" if item.get("kind") == "playlist" else "channel"))]
            if item.get("kind") == "playlist":
                count_text = self.playlist_count_text(item)
                if count_text:
                    parts.append(count_text)
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

    def playlist_count_text(self, item: dict) -> str:
        raw_count = item.get("playlist_count") or item.get("n_entries") or item.get("video_count")
        if raw_count in (None, ""):
            return ""
        try:
            count = int(str(raw_count).replace(",", "").strip())
        except (TypeError, ValueError):
            return str(raw_count)
        return self.t("playlist_video_count", count=count)

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
            self.show_channel_options(item)
            return
        if item.get("kind") == "playlist":
            self.open_playlist_videos(item)
            return
        self.return_results = list(self.results)
        self.return_all_results = list(self.all_results or self.results)
        self.return_index = self.current_index
        self.return_visible_count = self.last_visible_count or len(self.results)
        folder_context = self.folder_screen_active or (self.in_player_screen and self.player_return_screen == "folder")
        trending_context = getattr(self, "trending_screen_active", False) or (self.in_player_screen and self.player_return_screen == "trending")
        if folder_context:
            self.player_return_screen = "folder"
            self.player_return_data = {"index": self.current_index, "folder": self.last_search_query}
        elif trending_context:
            self.player_return_screen = "trending"
            self.player_return_data = {
                "index": self.current_index,
                "country_index": getattr(self, "last_trending_country_index", 0),
                "category_index": getattr(self, "last_trending_category_index", 0),
            }
        else:
            self.player_return_screen = "search"
            self.player_return_data = {}
        if folder_context:
            items = [dict(result) for result in (self.all_results or self.results) if result.get("kind") == "local_file" and result.get("url")]
            queue_items = [self.playback_queue_item_with_folder_return(result, items, auto_folder_queue=True) for result in items[self.current_index + 1 :]]
            self.playback_queue = queue_items
            self.save_playback_queue()
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(item["url"], item["title"])

    def push_search_state(self) -> None:
        if not self.search_screen_active or not self.results:
            return
        self.search_results_stack.append(
            {
                "screen": "trending" if getattr(self, "trending_screen_active", False) else "search",
                "results": list(self.results),
                "all_results": list(self.all_results or self.results),
                "index": max(0, self.current_index),
                "visible_count": self.last_visible_count or len(self.results),
                "query": self.last_search_query,
                "type_index": self.last_search_type_index,
                "search_type": self.current_search_type_code,
                "collection_url": self.collection_url,
                "collection_result_type": self.collection_result_type,
                "country_index": getattr(self, "last_trending_country_index", 0),
                "category_index": getattr(self, "last_trending_category_index", 0),
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
        if state.get("screen") == "trending":
            results = list(state.get("all_results") or state.get("results") or [])
            selection = int(state.get("index") or 0)
            visible_count = int(state.get("visible_count") or len(results))
            country_index = int(state.get("country_index") or 0)
            category_index = int(state.get("category_index") or 0)
            self.loading_more_results = False
            self.dynamic_fetch_enabled = False
            self.metadata_hydration_urls.clear()
            self.search_generation += 1
            self.show_trending(auto_load=False, country_index=country_index, category_index=category_index)
            self.show_results(results, selection=selection, visible_count=visible_count)
            wx.CallAfter(self.focus_results_list, selection)
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
        self.open_channel_tab(item, "videos", push_state=push_state)

    def channel_tab_url(self, item: dict, tab: str) -> str:
        url = str(item.get("url") or item.get("channel_url") or "").strip()
        if not url:
            return ""
        base = url.split("?", 1)[0].rstrip("/")
        base = re.sub(r"/(videos|playlists|featured|streams|shorts)$", "", base, flags=re.IGNORECASE)
        if tab == "home":
            return base
        if tab == "popular":
            return f"{base}/videos?sort=p"
        if tab == "playlists":
            return f"{base}/playlists"
        return f"{base}/videos"

    def show_channel_options(self, item: dict | None = None) -> None:
        item = item or self.selected_result()
        if not item or item.get("kind") != "channel":
            self.message(self.t("no_selection"))
            return
        tabs = [
            ("videos", self.t("channel_videos")),
            ("playlists", self.t("channel_playlists")),
            ("home", self.t("channel_home")),
            ("popular", self.t("channel_popular")),
        ]
        with wx.SingleChoiceDialog(self, item.get("title", self.t("channel")), self.t("channel_options"), [label for _tab, label in tabs]) as dialog:
            dialog.SetSelection(0)
            if dialog.ShowModal() != wx.ID_OK:
                return
            index = dialog.GetSelection()
        if 0 <= index < len(tabs):
            self.open_channel_tab(item, tabs[index][0], push_state=True)

    def open_channel_tab(self, item: dict, tab: str = "videos", push_state: bool = True) -> None:
        if push_state:
            self.push_search_state()
        self.trending_screen_active = False
        url = self.channel_tab_url(item, tab)
        if not url:
            self.message(self.t("no_selection"))
            return
        title = str(item.get("title") or self.t("channel"))
        if tab == "playlists":
            result_type = "Playlist"
            label = self.t("channel_playlists")
        elif tab == "home":
            result_type = "All"
            label = self.t("channel_home")
        elif tab == "popular":
            result_type = "Video"
            label = self.t("channel_popular")
        else:
            result_type = "Video"
            label = self.t("channel_videos")
        self.set_status(self.t("loading_channel", title=f"{title} - {label}"))
        self.collection_url = url
        self.collection_result_type = result_type
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.load_collection_worker, args=(url, result_type, self.initial_results_limit(), 0, generation), daemon=True).start()

    def show_trending(self, auto_load: bool = True, country_index: int | None = None, category_index: int | None = None) -> None:
        if not getattr(self.settings, "enable_trending", False):
            self.announce_player(self.t("trending_disabled"))
            self.show_main_menu()
            return
        self.in_main_menu = False
        self.search_screen_active = True
        self.trending_screen_active = True
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
        self.folder_screen_active = False
        self.clear()
        self.add_background_player_section()
        self.add_button_row([(self.t("back"), self.show_main_menu), (self.t("load_trending"), self.load_trending_results)])
        grid = wx.FlexGridSizer(2, 2, 6, 6)
        grid.AddGrowableCol(1, 1)
        grid.Add(wx.StaticText(self.panel, label=self.t("trending_country")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.trending_country_choice = wx.Choice(self.panel, choices=[label for _code, label in TRENDING_COUNTRIES])
        self.trending_country_choice.SetName(self.t("trending_country"))
        selected_country = self.last_trending_country_index if country_index is None else country_index
        self.trending_country_choice.SetSelection(min(max(0, selected_country), self.trending_country_choice.GetCount() - 1))
        self.trending_country_choice.Bind(wx.EVT_CHOICE, lambda _evt: self.load_trending_results())
        self.trending_country_choice.Bind(wx.EVT_KEY_DOWN, self.on_trending_filter_key)
        grid.Add(self.trending_country_choice, 1, wx.EXPAND)
        grid.Add(wx.StaticText(self.panel, label=self.t("trending_category")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.trending_category_choice = wx.Choice(self.panel, choices=[self.t(f"trending_{code}") for code, _label in TRENDING_CATEGORIES])
        self.trending_category_choice.SetName(self.t("trending_category"))
        selected_category = self.last_trending_category_index if category_index is None else category_index
        self.trending_category_choice.SetSelection(min(max(0, selected_category), self.trending_category_choice.GetCount() - 1))
        self.trending_category_choice.Bind(wx.EVT_CHOICE, lambda _evt: self.load_trending_results())
        self.trending_category_choice.Bind(wx.EVT_KEY_DOWN, self.on_trending_filter_key)
        grid.Add(self.trending_category_choice, 1, wx.EXPAND)
        self.root_sizer.Add(grid, 0, wx.EXPAND | wx.ALL, 4)
        self.results_list = wx.ListBox(self.panel, choices=[self.t("search_results_empty")])
        self.results_list.SetName(self.t("trending"))
        self.results_list.SetSelection(0)
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        self.focus_later(self.trending_country_choice)
        if auto_load:
            wx.CallAfter(self.load_trending_results)

    def on_trending_filter_key(self, event: wx.KeyEvent) -> None:
        if event.GetKeyCode() == wx.WXK_RETURN:
            self.load_trending_results()
            return
        event.Skip()

    def load_trending_results(self) -> None:
        country_index = self.trending_country_choice.GetSelection() if hasattr(self, "trending_country_choice") else 0
        category_index = self.trending_category_choice.GetSelection() if hasattr(self, "trending_category_choice") else 0
        country_code = TRENDING_COUNTRIES[country_index][0] if 0 <= country_index < len(TRENDING_COUNTRIES) else "global"
        category_code = TRENDING_CATEGORIES[category_index][0] if 0 <= category_index < len(TRENDING_CATEGORIES) else "all"
        self.last_trending_country_index = min(max(0, country_index), len(TRENDING_COUNTRIES) - 1)
        self.last_trending_category_index = min(max(0, category_index), len(TRENDING_CATEGORIES) - 1)
        country_label = dict(TRENDING_COUNTRIES).get(country_code, country_code)
        category_label = self.t(f"trending_{category_code}")
        self.last_search_query = f"official trending {country_code} {category_code}"
        self.last_search_type_index = 1
        self.current_search_type_code = "Video"
        self.collection_url = ""
        self.collection_result_type = ""
        self.search_results_stack = []
        self.loading_more_results = False
        self.dynamic_fetch_enabled = False
        self.metadata_hydration_urls.clear()
        self.set_status(self.t("trending_loading_official", country=country_label, category=category_label))
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.trending_worker, args=(country_code, category_code, generation), daemon=True).start()

    def trending_worker(self, country_code: str, category_code: str, generation: int) -> None:
        try:
            source_key = "trending_source_public"
            api_error = ""
            results: list[dict] = []
            if self.youtube_data_api_key():
                try:
                    results = self.fetch_youtube_api_trending(country_code, category_code)
                    source_key = "trending_source_api"
                except Exception as exc:
                    api_error = self.friendly_error(exc)
            if not results:
                source_key = "trending_source_public"
                results = self.fetch_public_official_trending(country_code, category_code)
            wx.CallAfter(self.show_results_if_current, generation, results)
            wx.CallAfter(self.set_status, self.t(source_key))
        except Exception as exc:
            message = self.friendly_error(exc)
            if "api_error" in locals() and api_error:
                message = f"{api_error}\n\n{message}"
            wx.CallAfter(self.show_trending_error_if_current, generation, self.t("trending_official_unavailable", error=message))

    def show_trending_error_if_current(self, generation: int, error: str) -> None:
        if generation != self.search_generation:
            return
        self.search_generation += 1
        self.message(error, wx.ICON_ERROR)
        self.announce_player(self.t("trending_unavailable_returning"))
        self.show_main_menu()

    def youtube_data_api_key(self) -> str:
        return str(getattr(self.settings, "youtube_data_api_key", "") or "").strip()

    def fetch_youtube_api_trending(self, country_code: str, category_code: str) -> list[dict]:
        api_key = self.youtube_data_api_key()
        if not api_key:
            raise RuntimeError(self.t("trending_api_key_required"))
        limit = self.max_results_limit() or 50
        max_results = min(50, max(1, limit))
        params = {
            "part": "snippet,contentDetails,statistics",
            "chart": "mostPopular",
            "maxResults": str(max_results),
            "key": api_key,
        }
        if country_code and country_code != "global":
            params["regionCode"] = country_code
        category_id = TRENDING_CATEGORY_IDS.get(category_code, "0")
        if category_id and category_id != "0":
            params["videoCategoryId"] = category_id
        request = Request(f"{YOUTUBE_API_VIDEOS_URL}?{urlencode(params)}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=25) as response:
            payload = json.loads(response.read().decode("utf-8", errors="replace"))
        if isinstance(payload, dict) and payload.get("error"):
            error = payload.get("error") or {}
            message = error.get("message") if isinstance(error, dict) else str(error)
            raise RuntimeError(message or self.t("trending_api_key_required"))
        return [self.normalize_youtube_api_video(item) for item in list(payload.get("items") or []) if isinstance(item, dict)]

    def fetch_public_official_trending(self, country_code: str, category_code: str) -> list[dict]:
        if country_code == "global":
            country_code = "US"
        urls = TRENDING_PUBLIC_URLS.get(category_code) or TRENDING_PUBLIC_URLS.get("all", [])
        last_error = self.t("trending_api_key_required")
        for template in urls:
            url = template.format(country=country_code, country_lower=country_code.lower())
            try:
                limit = self.max_results_limit() or 50
                options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": min(50, limit)}
                info = self.ydl_extract_info(url, options, download=False, allow_cookie_retry=False)
                entries = list((info or {}).get("entries") or [])
                normalized = [self.normalize_entry(entry, "Video") for entry in entries if isinstance(entry, dict)]
                if normalized:
                    return normalized
            except Exception as exc:
                last_error = self.friendly_error(exc)
        raise RuntimeError(f"{self.t('trending_api_key_required')}\n\n{last_error}")

    def normalize_youtube_api_video(self, item: dict) -> dict:
        snippet = item.get("snippet") or {}
        statistics = item.get("statistics") or {}
        content_details = item.get("contentDetails") or {}
        video_id = str(item.get("id") or "")
        title = str(snippet.get("title") or "")
        channel = str(snippet.get("channelTitle") or "")
        channel_id = str(snippet.get("channelId") or "")
        published_at = str(snippet.get("publishedAt") or "")
        timestamp = self.timestamp_from_iso_datetime(published_at)
        duration_seconds = self.seconds_from_iso8601_duration(str(content_details.get("duration") or ""))
        view_count = statistics.get("viewCount")
        return {
            "title": title,
            "channel": channel,
            "channel_url": f"https://www.youtube.com/channel/{channel_id}" if channel_id else "",
            "channel_id": channel_id,
            "views": self.format_count(view_count),
            "view_count": view_count,
            "age": self.format_age({"timestamp": timestamp}) if timestamp else self.t("uploaded_unknown"),
            "duration": self.format_duration(duration_seconds),
            "duration_seconds": duration_seconds,
            "timestamp": timestamp,
            "upload_date": "",
            "description": snippet.get("description") or "",
            "type": self.t("video"),
            "kind": "video",
            "url": f"https://www.youtube.com/watch?v={video_id}" if video_id else "",
        }

    def open_playlist_videos(self, item: dict, push_state: bool = True) -> None:
        if push_state:
            self.push_search_state()
        self.trending_screen_active = False
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
            generation = self.search_generation if generation is None else generation
            limit = limit or self.initial_results_limit()
            options = {
                "quiet": True,
                "extract_flat": True,
                "skip_download": True,
                "playlistend": limit,
            }
            info = self.ydl_extract_info(url, options, download=False)
            entries = list(info.get("entries") or [])[:limit]
            normalized = [self.normalize_entry(entry, result_type) for entry in entries]
            if self.settings.results_limit == 0 and selection:
                wx.CallAfter(self.show_more_results_if_current, generation, normalized, selection)
            else:
                wx.CallAfter(self.show_results_if_current, generation, normalized)
                wx.CallAfter(self.clear_loading_more_if_current, generation)
        except Exception as exc:
            wx.CallAfter(self.dynamic_fetch_failed_if_current, generation or self.search_generation, self.friendly_error(exc))

    @staticmethod
    def local_media_path_from_input(value: str) -> Path | None:
        text = str(value or "").strip().strip('"')
        if not text:
            return None
        if text.lower().startswith("file:"):
            parsed = urlparse(text)
            path_text = unquote(parsed.path or "")
            if parsed.netloc:
                text = f"//{parsed.netloc}{path_text}"
            elif os.name == "nt" and re.match(r"^/[A-Za-z]:/", path_text):
                text = path_text[1:]
            else:
                text = path_text
        candidate = Path(text).expanduser()
        try:
            if candidate.exists() and candidate.is_file():
                return candidate.resolve()
        except OSError:
            return None
        return None

    @staticmethod
    def looks_like_local_media_path(value: str) -> bool:
        path = MainFrame.local_media_path_from_input(value)
        return bool(path and (path.suffix.lower() in LOCAL_MEDIA_EXTENSIONS or path.is_file()))

    def local_media_item(self, path: Path, base_folder: Path | None = None) -> dict:
        folder = path.parent.name
        relative_path = path.name
        if base_folder is not None:
            try:
                relative_path = str(path.relative_to(base_folder))
            except ValueError:
                relative_path = path.name
        return {
            "title": path.stem or path.name,
            "url": str(path),
            "webpage_url": str(path),
            "kind": "local_file",
            "type": self.t("local_media"),
            "channel": folder,
            "folder": folder,
            "ext": path.suffix.lstrip(".").lower(),
            "path": str(path),
            "relative_path": relative_path,
            "description": str(path),
        }

    def local_media_wildcard(self) -> str:
        patterns = ";".join(f"*{extension}" for extension in sorted(LOCAL_MEDIA_EXTENSIONS))
        return f"{self.t('media_files')} ({patterns})|{patterns}|{self.t('all_files')} (*.*)|*.*"

    def local_media_files_in_folder(self, folder: Path) -> list[Path]:
        files: list[Path] = []

        def ignore_walk_error(_error: OSError) -> None:
            return

        try:
            for root, directories, names in os.walk(folder, onerror=ignore_walk_error):
                directories.sort(key=str.lower)
                for name in sorted(names, key=str.lower):
                    path = Path(root) / name
                    try:
                        if path.is_file() and path.suffix.lower() in LOCAL_MEDIA_EXTENSIONS:
                            files.append(path)
                    except OSError:
                        continue
        except OSError:
            return []
        return sorted(files, key=lambda path: str(path.relative_to(folder)).lower())

    @staticmethod
    def local_folder_cache_key(folder: Path) -> str:
        try:
            return str(folder.expanduser().resolve()).lower()
        except OSError:
            return str(folder.expanduser()).lower()

    def cached_local_folder_items(self, folder: Path) -> list[dict]:
        return [dict(item) for item in self.local_folder_cache.get(self.local_folder_cache_key(folder), [])]

    def cache_local_folder_items(self, folder: Path, items: list[dict]) -> None:
        key = self.local_folder_cache_key(folder)
        self.local_folder_cache[key] = [dict(item) for item in items]

    def show_play_from_folder(self) -> None:
        start_dir = self.settings.download_folder or str(Path.home())
        with wx.DirDialog(
            self,
            self.t("select_media_folder"),
            defaultPath=start_dir if Path(start_dir).exists() else str(Path.home()),
            style=wx.DD_DEFAULT_STYLE | wx.DD_DIR_MUST_EXIST,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                self.show_main_menu()
                return
            path = dialog.GetPath()
        self.open_local_media_folder(path)

    def open_local_media_folder(self, value: str) -> None:
        folder = Path(str(value or "")).expanduser()
        if not folder.exists() or not folder.is_dir():
            self.message(self.t("folder_no_media"), wx.ICON_WARNING)
            self.show_main_menu()
            return
        cached_items = self.cached_local_folder_items(folder)
        if cached_items:
            self.show_local_media_folder(folder, cached_items, selection=0)
            return
        files = self.local_media_files_in_folder(folder)
        if not files:
            self.message(self.t("folder_no_media"), wx.ICON_INFORMATION)
            self.show_main_menu()
            return
        items = [self.local_media_item(path, folder) for path in files]
        self.cache_local_folder_items(folder, items)
        self.show_local_media_folder(folder, items, selection=0)

    def show_local_media_folder(self, folder: Path, items: list[dict], selection: int = 0) -> None:
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
        self.folder_screen_active = True
        self.clear()
        self.add_background_player_section()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("play"), self.play_selected),
                (self.t("play_folder"), lambda: self.play_local_folder(start_index=0, shuffle=False)),
                (self.t("shuffle_folder"), lambda: self.play_local_folder(start_index=0, shuffle=True)),
                (self.t("add_folder_to_queue"), self.add_local_folder_to_playback_queue),
                (self.t("playback_queue"), self.show_playback_queue),
            ]
        )
        label = wx.StaticText(self.panel, label=f"{self.t('play_from_folder')}: {folder}")
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.results_list = wx.ListBox(self.panel, choices=[self.t("search_results_empty")])
        self.results_list.SetName(self.t("play_from_folder"))
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)
        self.last_search_query = str(folder)
        self.last_search_type_index = 0
        self.current_search_type_code = "All"
        self.collection_url = ""
        self.collection_result_type = ""
        self.search_results_stack = []
        self.loading_more_results = False
        self.dynamic_fetch_enabled = False
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        self.cache_local_folder_items(folder, items)
        self.show_results(items, selection=selection, visible_count=len(items))
        self.set_status(self.t("folder_loaded", count=len(items)))
        self.return_results = list(self.results)
        self.return_all_results = list(self.all_results or self.results)
        self.return_index = min(max(0, selection), len(items) - 1)
        self.return_visible_count = self.last_visible_count or len(self.results)
        self.panel.Layout()
        self.focus_later(self.results_list)

    def selected_local_folder_items(self) -> list[dict]:
        return [dict(item) for item in (self.all_results or self.results) if item.get("kind") == "local_file" and item.get("url")]

    def play_local_folder(self, start_index: int = 0, shuffle: bool = False) -> None:
        items = self.selected_local_folder_items()
        if not items:
            self.announce_player(self.t("folder_no_media"))
            return
        current = self.selected_result()
        if current and any(item.get("url") == current.get("url") for item in items):
            start_index = next((index for index, item in enumerate(items) if item.get("url") == current.get("url")), start_index)
        start_index = min(max(0, start_index), len(items) - 1)
        ordered = list(items)
        if shuffle:
            random.shuffle(ordered)
            start_index = 0
            self.shuffle_current = True
        else:
            self.shuffle_current = False
        current_item = dict(ordered[start_index])
        queue_items = ordered[start_index + 1 :]
        self.playback_queue = [self.playback_queue_item_with_folder_return(item, items, auto_folder_queue=True) for item in queue_items]
        self.save_playback_queue()
        self.player_return_screen = "folder"
        self.player_return_data = {
            "index": next((index for index, item in enumerate(items) if item.get("url") == current_item.get("url")), start_index),
            "folder": self.last_search_query,
        }
        self.return_results = list(self.results)
        self.return_all_results = list(self.all_results or self.results)
        self.return_index = int(self.player_return_data.get("index") or 0)
        self.return_visible_count = self.last_visible_count or len(self.results)
        self.current_video_item = current_item
        self.current_video_info = dict(current_item)
        self.play_url(str(current_item.get("url") or ""), str(current_item.get("title") or ""))

    def add_local_folder_to_playback_queue(self) -> None:
        source_items = self.selected_local_folder_items()
        items = [self.playback_queue_item_with_folder_return(item, source_items) for item in source_items]
        if not items:
            self.announce_player(self.t("folder_no_media"))
            return
        existing_urls = {str(item.get("url") or "") for item in self.playback_queue}
        added = 0
        for item in items:
            url = str(item.get("url") or "")
            if url and url not in existing_urls:
                self.playback_queue.append(item)
                existing_urls.add(url)
                added += 1
        self.save_playback_queue()
        self.announce_player(self.t("folder_queue_added", count=added))

    def playback_queue_item_with_folder_return(self, item: dict, source_items: list[dict], auto_folder_queue: bool = False) -> dict:
        queue_item = self.playlist_item_from_media(item)
        queue_item["_return_screen"] = "folder"
        queue_item["_return_folder"] = self.last_search_query
        queue_item["_return_index"] = next(
            (index for index, source in enumerate(source_items) if source.get("url") == item.get("url")),
            0,
        )
        if auto_folder_queue:
            queue_item["_auto_folder_queue"] = True
        return queue_item

    def open_local_media_file(self, value: str) -> None:
        try:
            path = self.local_media_path_from_input(value)
            if not path:
                raise FileNotFoundError(value)
            item = self.local_media_item(path)
            self.player_return_screen = "local_file"
            self.player_return_data = {}
            self.current_video_item = item
            self.current_video_info = dict(item)
            self.play_url(str(path), item["title"])
        except Exception as exc:
            self.message(self.t("local_file_open_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def clear_loading_more_if_current(self, generation: int) -> None:
        if generation == self.search_generation:
            self.loading_more_results = False

    def play_url(self, url: str, title: str = "", show_player: bool = True, announce_start: bool = False) -> None:
        player = self.resolve_player()
        if not player:
            self.message(self.t("player_missing"), wx.ICON_ERROR)
            return
        if self.current_video_item:
            self.record_history(self.current_video_item, "played")
        self.current_index = max(0, self.current_index)
        continuing_session = self.player_is_active()
        self.remember_current_player_volume()
        self.stop_player(silent=True, reset_session=not continuing_session)
        if not continuing_session:
            self.session_equalizer_enabled = None
            self.session_equalizer_gains = {}
            self.session_equalizer_before_bass_boost = None
            self.bass_boost_enabled = False
            self.shuffle_current = False
        self.edit_mode_enabled = False
        self.equalizer_filter_active = False
        self.clip_start_marker = None
        self.clip_end_marker = None
        self.current_stream_headers = {}
        self.player_fullscreen_results_override = False
        command, kind = player
        if kind != "mpv":
            self.message(self.t("player_missing"), wx.ICON_ERROR)
            return
        if show_player:
            self.show_player_page(title)
        else:
            self.in_player_screen = False
            self.player_control_mode = True
            self.set_window_title(title or self.current_player_title())
        self.set_status(self.t("preparing_stream", title=title or url))
        threading.Thread(target=self.resolve_and_start_player, args=(command, url, title, announce_start), daemon=True).start()

    def resolve_and_start_player(self, command: str, url: str, title: str, announce_start: bool = False) -> None:
        try:
            stream_url, headers, info = self.resolve_stream_url(url)
            wx.CallAfter(self.merge_current_video_info, info)
            wx.CallAfter(self.start_mpv, command, stream_url, title or url, headers, announce_start)
        except Exception as exc:
            if self.age_restricted_video_support_enabled() and self.is_cookie_auth_error(exc) and self.normalized_cookies_browser():
                wx.CallAfter(self.prompt_cookie_refresh_for_playback, command, url, title, self.friendly_error(exc), announce_start)
            else:
                wx.CallAfter(self.message, self.t("player_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def prompt_cookie_refresh_for_playback(self, command: str, url: str, title: str, error: str, announce_start: bool = False) -> None:
        message = f"{self.t('player_failed', error=error)}\n\n{self.t('cookie_refresh_prompt_message')}"
        answer = wx.MessageBox(message, self.t("cookie_refresh_prompt_title"), wx.YES_NO | wx.ICON_QUESTION)
        if answer != wx.YES:
            return
        browser = self.normalized_cookies_browser()
        if not browser:
            self.message(self.t("select_cookies_browser"), wx.ICON_WARNING)
            return
        self.announce_player(self.t("cookie_auto_refresh_start", browser=browser.title()))
        threading.Thread(target=self.refresh_cookies_and_retry_playback_worker, args=(browser, command, url, title, announce_start), daemon=True).start()

    def refresh_cookies_and_retry_playback_worker(self, browser: str, command: str, url: str, title: str, announce_start: bool = False) -> None:
        try:
            result = self.export_browser_cookies_blocking(browser, allow_close=True)
            self.ui_queue.put(("announce", self.t("cookie_auto_refresh_done", profile=result.get("profile_label", self.t("browser_profile_auto")))))
            self.resolve_and_start_player(command, url, title, announce_start)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("cookie_auto_refresh_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def resolve_stream_url(self, url: str) -> tuple[str, dict, dict]:
        local_path = self.local_media_path_from_input(url)
        if local_path:
            info = self.local_media_item(local_path)
            return str(local_path), {}, info
        options = {
            "quiet": True,
            "skip_download": True,
            "format": "best[ext=mp4]/best",
            "noplaylist": True,
        }
        format_fallback_options = dict(options)
        format_fallback_options["format"] = "best[acodec!=none][vcodec!=none]/18/22/17/best"
        try:
            info = self.ydl_extract_info(url, options, download=False, allow_cookie_retry=False)
        except Exception as exc:
            cookie_file = self.playback_cookies_file_for_url(url)
            cookie_error = self.is_cookie_auth_error(exc)
            age_or_js_error = self.is_age_or_js_playback_error(exc)
            requested_format_error = self.is_requested_format_error(exc)
            retry_error: Exception | str = exc
            if requested_format_error:
                try:
                    info = self.ydl_extract_info(url, format_fallback_options, download=False, allow_cookie_retry=False)
                    stream_url = info.get("url")
                    if stream_url:
                        return stream_url, info.get("http_headers") or {}, info
                except Exception as format_exc:
                    retry_error = format_exc
                    cookie_error = cookie_error or self.is_cookie_auth_error(format_exc)
                    age_or_js_error = age_or_js_error or self.is_age_or_js_playback_error(format_exc)
            can_retry_with_cookies = bool(cookie_file) and (cookie_error or age_or_js_error)
            can_retry_with_restricted_fallback = self.age_restricted_video_support_enabled() and (cookie_error or age_or_js_error)
            can_retry_with_js_format_fallback = requested_format_error and age_or_js_error
            if not (can_retry_with_cookies or can_retry_with_restricted_fallback):
                if can_retry_with_js_format_fallback:
                    try:
                        info = self.ydl_extract_info(
                            url,
                            format_fallback_options,
                            download=False,
                            use_cookies=False,
                            use_js_solver=True,
                            allow_cookie_retry=False,
                        )
                    except Exception:
                        raise retry_error if isinstance(retry_error, Exception) else exc
                else:
                    raise retry_error if isinstance(retry_error, Exception) else exc
            else:
                info = None
            if cookie_file:
                try:
                    info = self.ydl_extract_info(
                        url,
                        format_fallback_options if requested_format_error else options,
                        download=False,
                        use_cookies=True,
                        use_js_solver=False,
                        allow_cookie_retry=False,
                    )
                except Exception as cookie_exc:
                    retry_error = cookie_exc
                    if not self.is_age_or_js_playback_error(cookie_exc) and not self.is_cookie_auth_error(cookie_exc):
                        raise
                    info = self.ydl_extract_info(
                        url,
                        format_fallback_options if requested_format_error else options,
                        download=False,
                        use_cookies=True,
                        use_js_solver=True,
                        allow_cookie_retry=False,
                    )
            elif can_retry_with_restricted_fallback and info is None:
                try:
                    info = self.ydl_extract_info(
                        url,
                        format_fallback_options if requested_format_error else options,
                        download=False,
                        use_cookies=False,
                        use_js_solver=True,
                        allow_cookie_retry=False,
                    )
                except Exception:
                    raise retry_error if isinstance(retry_error, Exception) else exc
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
                "ext": info.get("ext") or self.current_video_info.get("ext", ""),
            }
        )
        if self.current_video_item is not None:
            self.current_video_item.update(self.current_video_info)
        if self.in_player_screen:
            self.set_window_title(str(self.current_video_info.get("title") or ""))
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

    def audio_output_device_options(self, force_refresh: bool = False, allow_probe: bool = True) -> tuple[list[str], list[str]]:
        now = time.monotonic()
        if not force_refresh and self.audio_device_options_cache and now - self.audio_device_options_cache[0] < 20:
            return list(self.audio_device_options_cache[1]), list(self.audio_device_options_cache[2])
        values = ["auto"]
        labels = ["auto"]
        if not allow_probe:
            current = self.normalized_audio_output_device()
            if current and current.lower() != "auto":
                values.append(current)
                labels.append(current)
            return values, labels
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

    def refresh_audio_output_devices_async(self) -> None:
        if self.audio_device_refresh_running:
            return
        if self.audio_device_options_cache and time.monotonic() - self.audio_device_options_cache[0] < 60:
            return
        self.audio_device_refresh_running = True
        threading.Thread(target=self.refresh_audio_output_devices_worker, daemon=True).start()

    def refresh_audio_output_devices_worker(self) -> None:
        values: list[str] = ["auto"]
        labels: list[str] = ["auto"]
        try:
            values, labels = self.audio_output_device_options(force_refresh=True, allow_probe=True)
        finally:
            wx.CallAfter(self.finish_audio_output_device_refresh, values, labels)

    def finish_audio_output_device_refresh(self, values: list[str], labels: list[str]) -> None:
        self.audio_device_refresh_running = False
        if not hasattr(self, "settings_sections") or not hasattr(self, "controls"):
            return
        try:
            section_name = self.settings_sections()[self.settings_section_index][1]
        except Exception:
            return
        if section_name != "playback":
            return
        ctrl = self.controls.get("default_audio_device")
        if not isinstance(ctrl, wx.Choice) or (hasattr(ctrl, "IsBeingDeleted") and ctrl.IsBeingDeleted()):
            return
        current = self.selected_choice_value("default_audio_device") or self.normalized_audio_output_device()
        if current and current not in values:
            values = [*values, current]
            labels = [*labels, current]
        selected = values.index(current) if current in values else 0
        try:
            ctrl.Freeze()
            ctrl.Set(labels)
            ctrl.SetSelection(selected)
            self.choice_values["default_audio_device"] = list(values)
        finally:
            ctrl.Thaw()

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

    def remember_current_player_volume(self) -> None:
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            return
        try:
            current = self.mpv_get_property("volume", timeout=0.3)
            if current is not None:
                self.session_volume = max(0.0, min(300.0, float(current)))
        except Exception:
            pass

    def player_start_volume_value(self) -> float:
        if self.session_volume is not None:
            return max(0.0, min(300.0, float(self.session_volume)))
        return float(self.default_volume_value())

    def start_mpv(self, command: str, stream_url: str, title: str, headers: dict, announce_start: bool = False) -> None:
        try:
            self.ipc_path = self.make_ipc_path()
            target_volume = self.player_start_volume_value()
            embed_player = False
            hwnd = 0
            try:
                embed_player = bool(self.in_player_screen and self.player_panel and not self.player_panel.IsBeingDeleted())
                if embed_player:
                    self.player_panel.Update()
                    hwnd = self.player_panel.GetHandle()
            except Exception:
                embed_player = False
            args = [
                command,
                "--no-config",
                "--force-window=yes" if embed_player else "--force-window=no",
                f"--input-ipc-server={self.ipc_path}",
                "--idle=no",
                "--keep-open=yes",
                "--volume-max=300",
                f"--volume={target_volume:g}",
                "--pitch=1.0",
                f"--speed={self.settings.player_speed}",
                f"--loop-file={'inf' if self.repeat_current else 'no'}",
                "--term-playing-msg=",
                "--msg-level=all=warn",
            ]
            if embed_player and hwnd:
                args.insert(2, f"--wid={hwnd}")
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
            if self.player_fullscreen_mode_active():
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
            self.player_paused = bool(self.settings.player_start_paused)
            self.player_generation += 1
            self.current_stream_url = stream_url
            self.current_stream_headers = dict(headers or {})
            self.current_audio_device = audio_device
            self.volume_boost_enabled = bool(self.volume_boost_enabled or getattr(self.settings, "volume_boost_by_default", False) or target_volume > 100)
            self.rubberband_pitch_filter_active = False
            self.equalizer_filter_active = False
            self.current_video_info["speed"] = self.format_playback_rate(float(self.settings.player_speed))
            self.current_video_info["pitch"] = self.format_playback_rate(1.0)
            self.update_details_text()
            self.set_status(self.t("playing", title=title))
            if announce_start:
                self.announce_player(self.t("playing", title=title))
            wx.CallAfter(self.update_play_pause_buttons)
            threading.Thread(target=self.apply_initial_volume_worker, args=(self.player_generation, target_volume), daemon=True).start()
            wx.CallLater(700, self.apply_equalizer_to_player)
            self.start_player_monitor(self.player_generation)
        except Exception as exc:
            self.message(self.t("player_failed", error=exc), wx.ICON_ERROR)

    def apply_initial_volume_worker(self, generation: int, target_volume: float) -> None:
        deadline = time.monotonic() + 2.0
        while time.monotonic() < deadline:
            if generation != self.player_generation or not self.mpv_process_alive():
                return
            try:
                self.mpv_set_property("volume-max", 300, timeout=0.4)
                self.mpv_set_property("volume", target_volume, timeout=0.4)
                return
            except Exception:
                time.sleep(0.12)

    def show_player_page(self, title: str, focus_target: str = "player") -> None:
        fullscreen_mode = self.player_fullscreen_mode_active()
        background_enabled = self.background_playback_enabled()
        embedded_results = background_enabled and not fullscreen_mode
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
        self.folder_screen_active = False
        self.clear()
        self.in_player_screen = True
        self.player_control_mode = True
        self.player_navigation_controls = []
        self.player_action_controls = []
        self.player_escape_stop_controls = []
        navigation_controls = []
        if fullscreen_mode and background_enabled:
            navigation_controls.append((self.t("back_results"), self.exit_fullscreen_to_results))
        elif not embedded_results:
            navigation_controls.append((self.t("back_results"), self.leave_player_to_previous_screen))
            if not background_enabled:
                navigation_controls.append((self.t("back"), lambda: self.leave_player_to_main_menu(force_keep_playing=False)))
        else:
            navigation_controls.append((self.t("back"), lambda: self.leave_player_to_main_menu(force_keep_playing=True)))
        navigation_buttons = self.add_button_row(navigation_controls)
        self.player_navigation_controls = list(navigation_buttons)
        if not embedded_results and not (fullscreen_mode and self.background_playback_enabled()):
            self.player_escape_stop_controls.extend(navigation_buttons)
        if embedded_results:
            self.add_player_results_section()
        label = wx.StaticText(self.panel, label=f"{self.t('internal_player')}: {title}")
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        existing_panel = None
        if self.player_is_active() and self.player_panel is not None:
            try:
                if not self.player_panel.IsBeingDeleted():
                    existing_panel = self.player_panel
            except RuntimeError:
                existing_panel = None
        if existing_panel is not None:
            self.player_panel = existing_panel
            self.player_panel.Show()
        else:
            self.player_panel = PlayerPanel(self.panel, style=wx.BORDER_SIMPLE | wx.WANTS_CHARS)
            self.player_panel.SetBackgroundColour(wx.BLACK)
            self.player_panel.Bind(wx.EVT_KEY_DOWN, self.on_player_key)
            self.player_panel.Bind(wx.EVT_CONTEXT_MENU, self.open_player_context_menu)
        try:
            self.player_panel.SetCanFocus(True)
        except Exception:
            pass
        self.player_panel.SetName(self.t("player"))
        self.player_panel.SetLabel(self.t("player"))
        self.root_sizer.Add(self.player_panel, 1, wx.EXPAND | wx.ALL, 4)
        if focus_target == "player" and not self.settings.show_video_details_by_default:
            self.player_panel.SetFocus()
        player_controls = [
            (self.t("previous"), lambda: self.play_relative_item(-1)),
            (self.current_play_pause_label(), self.player_play_pause),
            (self.t("next"), lambda: self.play_relative_item(1)),
            (self.t("playback_queue"), self.show_playback_queue),
            (self.t("add_to_playlist"), lambda: self.add_active_to_playlist(prefer_active=True)),
            (self.t("output_devices"), self.show_output_devices),
            (self.t("equalizer"), self.show_player_equalizer),
            (self.t("edit_mode"), self.toggle_edit_mode),
            (self.t("copy_link"), self.copy_active_url),
            (self.t("copy_stream_url"), self.copy_direct_stream_url),
            (self.t("show_video_details"), self.show_video_details),
        ]
        if background_enabled:
            player_controls.append((self.t("close_player"), self.close_current_player))
        player_action_buttons = self.add_button_row(player_controls)
        self.player_action_controls = list(player_action_buttons)
        self.player_escape_stop_controls.extend(player_action_buttons)
        self.fullscreen_checkbox = wx.CheckBox(self.panel, label=self.t("fullscreen"))
        self.fullscreen_checkbox.SetName(self.t("fullscreen"))
        self.fullscreen_checkbox.SetValue(fullscreen_mode)
        self.fullscreen_checkbox.Bind(wx.EVT_CHECKBOX, self.on_player_fullscreen_changed)
        self.root_sizer.Add(self.fullscreen_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.player_action_controls.append(self.fullscreen_checkbox)
        self.player_escape_stop_controls.append(self.fullscreen_checkbox)
        self.repeat_checkbox = wx.CheckBox(self.panel, label=self.t("repeat"))
        self.repeat_checkbox.SetName(self.t("repeat"))
        self.repeat_checkbox.SetValue(self.repeat_current)
        self.repeat_checkbox.Bind(wx.EVT_CHECKBOX, self.on_repeat_changed)
        self.root_sizer.Add(self.repeat_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.player_action_controls.append(self.repeat_checkbox)
        self.player_escape_stop_controls.append(self.repeat_checkbox)
        self.bass_boost_checkbox = wx.CheckBox(self.panel, label=self.t("bass_boost"))
        self.bass_boost_checkbox.SetName(self.t("bass_boost"))
        self.bass_boost_checkbox.SetValue(self.bass_boost_enabled)
        self.bass_boost_checkbox.Bind(wx.EVT_CHECKBOX, self.on_bass_boost_changed)
        self.root_sizer.Add(self.bass_boost_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.player_action_controls.append(self.bass_boost_checkbox)
        self.player_escape_stop_controls.append(self.bass_boost_checkbox)
        self.details_label = None
        self.video_details = None
        self.details_button_sizer = None
        self.details_opened_temporarily = False
        self.set_window_title(title)
        self.panel.Layout()
        if focus_target == "results":
            wx.CallAfter(self.focus_results_list, self.return_index)
        elif self.settings.show_video_details_by_default:
            wx.CallAfter(self.show_video_details, False)
        else:
            self.player_panel.SetFocus()
        if fullscreen_mode:
            wx.CallAfter(self.ShowFullScreen, True)

    def player_results_snapshot(self) -> tuple[list[dict], int, int]:
        results = list(self.return_all_results or self.all_results or self.return_results or self.results)
        if not results:
            return [], 0, 0
        visible_count = self.return_visible_count or len(self.return_results or self.results or results)
        visible_count = min(max(1, int(visible_count or len(results))), len(results))
        current_url = str((self.current_video_item or self.current_video_info or {}).get("url") or "")
        selection = min(max(0, self.return_index), visible_count - 1)
        if current_url:
            for index, item in enumerate(results[:visible_count]):
                if str(item.get("url") or "") == current_url:
                    selection = index
                    break
        return results, visible_count, selection

    def add_player_results_section(self) -> None:
        results, visible_count, selection = self.player_results_snapshot()
        if not results:
            return
        self.deferred_result_line_updates.clear()
        self.all_results = list(results)
        self.last_visible_count = visible_count
        self.results = self.all_results[:visible_count]
        label = wx.StaticText(self.panel, label=self.t("search_youtube"))
        label.SetName(self.t("search_youtube"))
        self.root_sizer.Add(label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 4)
        labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
        self.results_list = wx.ListBox(self.panel, choices=labels or [self.t("search_results_empty")])
        self.results_list.SetName(self.t("search_youtube"))
        if labels:
            self.results_list.SetSelection(min(max(0, selection), len(labels) - 1))
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)

    def on_player_key(self, event: wx.KeyEvent) -> None:
        self.on_char_hook(event)

    def on_repeat_changed(self, _event=None) -> None:
        checked = bool(getattr(self, "repeat_checkbox", None) and self.repeat_checkbox.GetValue())
        self.set_repeat_enabled(checked)

    def set_repeat_enabled(self, checked: bool, announce: bool = True) -> None:
        self.repeat_current = checked
        if self.player_kind == "mpv" and self.mpv_process_alive():
            try:
                self.mpv_set_property("loop-file", "inf" if checked else "no", timeout=0.8)
            except Exception:
                pass
        if getattr(self, "repeat_checkbox", None):
            try:
                self.repeat_checkbox.SetValue(checked)
            except RuntimeError:
                pass
        if announce:
            self.announce_player(self.t("repeat_on" if checked else "repeat_off"))

    def toggle_repeat(self) -> None:
        self.set_repeat_enabled(not self.repeat_current)

    def on_bass_boost_changed(self, _event=None) -> None:
        checked = bool(getattr(self, "bass_boost_checkbox", None) and self.bass_boost_checkbox.GetValue())
        self.set_bass_boost_enabled(checked)

    def set_bass_boost_enabled(self, checked: bool, announce: bool = True) -> None:
        if checked == self.bass_boost_enabled:
            return
        if checked:
            self.session_equalizer_before_bass_boost = (self.session_equalizer_enabled, dict(self.session_equalizer_gains))
            self.session_equalizer_enabled = True
            self.session_equalizer_gains = self.factory_equalizer_gains_for_preset("bass_boost")
        else:
            previous = self.session_equalizer_before_bass_boost
            if previous is None:
                self.session_equalizer_enabled = None
                self.session_equalizer_gains = {}
            else:
                self.session_equalizer_enabled, gains = previous
                self.session_equalizer_gains = dict(gains)
            self.session_equalizer_before_bass_boost = None
        self.bass_boost_enabled = checked
        if getattr(self, "bass_boost_checkbox", None):
            try:
                self.bass_boost_checkbox.SetValue(checked)
            except RuntimeError:
                pass
        self.apply_equalizer_to_player()
        if announce:
            self.announce_player(self.t("bass_boost_on" if checked else "bass_boost_off"))

    def toggle_bass_boost(self) -> None:
        self.set_bass_boost_enabled(not self.bass_boost_enabled)

    def toggle_shuffle(self) -> None:
        self.shuffle_current = not self.shuffle_current
        self.announce_player(self.t("shuffle_on" if self.shuffle_current else "shuffle_off"))

    def player_escape_closes_playback(self, focus: wx.Window | None) -> bool:
        if focus is getattr(self, "results_list", None):
            return False
        if focus is getattr(self, "player_panel", None):
            return True
        if focus is getattr(self, "fullscreen_checkbox", None):
            return True
        if focus is getattr(self, "repeat_checkbox", None):
            return True
        if focus is getattr(self, "bass_boost_checkbox", None):
            return True
        return focus in getattr(self, "player_escape_stop_controls", [])

    def visible_player_controls(self, controls: list[wx.Window] | tuple[wx.Window, ...]) -> list[wx.Window]:
        visible_controls: list[wx.Window] = []
        for control in controls:
            live = self.live_window(control)
            if live is not None:
                visible_controls.append(live)
        return visible_controls

    def handle_player_tab_navigation(self, event: wx.KeyEvent, focus: wx.Window | None) -> bool:
        if not self.in_player_screen or event.GetKeyCode() != wx.WXK_TAB:
            return False
        panel = getattr(self, "player_panel", None)
        results = getattr(self, "results_list", None)
        if focus is results:
            if event.ShiftDown():
                return False
            if panel is not None:
                self.safe_set_focus(panel)
                return True
        action_controls = self.visible_player_controls(getattr(self, "player_action_controls", []))
        navigation_controls = self.visible_player_controls(getattr(self, "player_navigation_controls", []))
        if navigation_controls and focus in navigation_controls:
            try:
                nav_index = navigation_controls.index(focus)
            except ValueError:
                nav_index = -1
            if event.ShiftDown():
                if nav_index > 0:
                    self.safe_set_focus(navigation_controls[nav_index - 1])
                    return True
                return False
            if 0 <= nav_index < len(navigation_controls) - 1:
                self.safe_set_focus(navigation_controls[nav_index + 1])
                return True
            if panel is not None:
                self.safe_set_focus(panel)
                return True
        if action_controls and focus in action_controls:
            try:
                action_index = action_controls.index(focus)
            except ValueError:
                action_index = -1
            if event.ShiftDown():
                if action_index > 0:
                    self.safe_set_focus(action_controls[action_index - 1])
                    return True
                if panel is not None:
                    self.safe_set_focus(panel)
                    return True
                return False
            if 0 <= action_index < len(action_controls) - 1:
                self.safe_set_focus(action_controls[action_index + 1])
                return True
            return False
        if focus is panel:
            if event.ShiftDown():
                if results is not None:
                    self.safe_set_focus(results)
                    return True
                if navigation_controls:
                    self.safe_set_focus(navigation_controls[-1])
                    return True
                return False
            if action_controls:
                self.safe_set_focus(action_controls[0])
                return True
        return False

    def leave_player_to_main_menu(self, force_keep_playing: bool = False) -> None:
        keep_playing = bool(force_keep_playing and self.background_playback_enabled())
        if keep_playing and force_keep_playing:
            self.manual_background_playback_active = True
        self.exit_fullscreen_window()
        self.in_player_screen = False
        self.player_control_mode = keep_playing and self.player_control_mode
        if not keep_playing:
            self.stop_player(silent=True)
        self.show_main_menu()

    def leave_player_to_previous_screen(self) -> None:
        self.back_to_results(stop_playback=True)

    def back_to_results(self, stop_playback: bool = True) -> None:
        keep_playing = self.background_playback_enabled() and not stop_playback
        self.exit_fullscreen_window()
        self.in_player_screen = False
        self.player_control_mode = keep_playing and self.player_control_mode
        if not keep_playing:
            self.stop_player(silent=True)
        if self.player_return_screen == "rss_items":
            feed_index = int(self.player_return_data.get("feed_index", self.current_rss_feed_index) or 0)
            item_index = int(self.player_return_data.get("item_index", 0) or 0)
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_rss_items(feed_index, selection=item_index)
            return
        if self.player_return_screen == "history":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_history()
            return
        if self.player_return_screen == "user_playlist_items":
            playlist_index = int(self.player_return_data.get("playlist_index", self.current_user_playlist_index) or 0)
            item_index = int(self.player_return_data.get("item_index", 0) or 0)
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_user_playlist_items(playlist_index, selection=item_index)
            return
        if self.player_return_screen == "notification_center":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_notification_center()
            return
        if self.player_return_screen == "direct_link":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_direct_link()
            return
        if self.player_return_screen == "folder":
            folder = Path(str(self.player_return_data.get("folder") or self.last_search_query or Path.home())).expanduser()
            index = int(self.player_return_data.get("index", self.return_index) or 0)
            results = list(self.return_all_results or self.all_results or self.return_results or self.results)
            if not results:
                results = self.cached_local_folder_items(folder)
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            if results:
                self.show_local_media_folder(folder, results, selection=index)
            else:
                self.open_local_media_folder(str(folder))
            return
        if self.player_return_screen == "local_file":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_main_menu()
            return
        if self.player_return_screen == "playback_queue":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_main_menu()
            return
        if self.player_return_screen == "favorites":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_favorites()
            return
        if self.player_return_screen == "subscriptions":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_subscriptions()
            return
        if self.player_return_screen == "trending":
            country_index = int(self.player_return_data.get("country_index", getattr(self, "last_trending_country_index", 0)) or 0)
            category_index = int(self.player_return_data.get("category_index", getattr(self, "last_trending_category_index", 0)) or 0)
            results = self.return_all_results or self.all_results or self.return_results or self.results
            visible_count = self.return_visible_count or len(self.return_results or self.results)
            index = min(max(0, self.return_index), max(0, len(results) - 1))
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_trending(auto_load=False, country_index=country_index, category_index=category_index)
            self.show_results(results, selection=index, visible_count=visible_count)
            wx.CallAfter(self.focus_results_list, index)
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
                target = min(max(0, index), self.results_list.GetCount() - 1)
                if self.results_list.GetSelection() != target:
                    self.results_list.SetSelection(target)
            self.safe_set_focus(self.results_list)
        except RuntimeError:
            pass

    def announce_player(self, text: str) -> None:
        self.set_status(text)
        self.speak_text(text)

    def show_video_details(self, temporary: bool | None = None) -> None:
        if not self.in_player_screen:
            if self.player_is_active():
                self.show_current_player_screen()
                wx.CallAfter(self.show_video_details, temporary)
            else:
                self.announce_player(self.t("no_player"))
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
        focus = wx.Window.FindFocus()
        if focus is getattr(self, "results_list", None):
            return self.selected_result()
        if self.current_video_item and (self.in_player_screen or self.focus_in_background_player_controls(focus)):
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

    def copy_current_player_url(self) -> None:
        item = self.current_video_item or self.current_video_info or {}
        self.copy_url_to_clipboard(str(item.get("url") or item.get("webpage_url") or ""))

    def youtube_channel_item_for_video(self, item: dict | None) -> dict | None:
        if not item or not isinstance(item, dict):
            return None
        kind = str(item.get("kind") or "").strip().lower()
        if kind in {"channel", "playlist", "local_file", "rss_item", "podcast", "feed"}:
            return None
        channel_url = self.normalize_channel_url(item)
        if not channel_url or "youtube.com" not in channel_url.lower():
            return None
        title = str(item.get("channel") or item.get("uploader") or item.get("channel_id") or channel_url).strip()
        return {
            "title": title,
            "channel": title,
            "url": channel_url,
            "channel_url": channel_url,
            "kind": "channel",
            "type": self.t("channel"),
        }

    def item_has_openable_youtube_channel(self, item: dict | None) -> bool:
        return self.youtube_channel_item_for_video(item) is not None

    def open_item_channel(self, item: dict | None = None) -> None:
        explicit_item = item is not None
        channel_item = self.youtube_channel_item_for_video(item if explicit_item else self.active_item())
        if channel_item is None and not explicit_item and self.player_is_active():
            channel_item = self.youtube_channel_item_for_video(self.current_video_item or self.current_video_info)
        if not channel_item:
            self.announce_player(self.t("no_channel"))
            return
        self.open_channel_tab(channel_item, "videos", push_state=True)

    def show_output_devices(self) -> None:
        if not self.player_is_active():
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

    def effective_equalizer_state(self) -> tuple[bool, dict[str, float]]:
        if self.session_equalizer_enabled is not None:
            return bool(self.session_equalizer_enabled), self.normalized_equalizer_gains(self.session_equalizer_gains)
        preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        return bool(getattr(self.settings, "global_equalizer_enabled", False)), self.equalizer_gains_for_preset(preset)

    def use_global_equalizer_for_live_preview(self) -> None:
        self.session_equalizer_enabled = None
        self.session_equalizer_gains = {}
        if self.bass_boost_enabled:
            self.bass_boost_enabled = False
            self.session_equalizer_before_bass_boost = None
            if getattr(self, "bass_boost_checkbox", None):
                try:
                    self.bass_boost_checkbox.SetValue(False)
                except RuntimeError:
                    pass

    @staticmethod
    def equalizer_filter(gains: dict[str, float]) -> str:
        filters = []
        for band_id, _band_label in EQ_BANDS:
            gain = max(-24.0, min(24.0, float(gains.get(band_id, 0.0) or 0.0)))
            filters.append(f"equalizer=f={band_id}:t=q:w=1:g={gain:.1f}")
        return f"{EQ_FILTER_REF}:lavfi=[{','.join(filters)}]"

    def apply_equalizer_to_player(self) -> None:
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            return
        enabled, gains = self.effective_equalizer_state()
        try:
            self.mpv_request(["af", "remove", EQ_FILTER_REF], timeout=0.8)
        except Exception:
            pass
        self.equalizer_filter_active = False
        if not enabled or not any(abs(float(value)) >= 0.05 for value in gains.values()):
            return
        try:
            response = self.mpv_request(["af", "add", self.equalizer_filter(gains)], timeout=1.0)
            if response.get("error") == "success":
                self.equalizer_filter_active = True
        except Exception:
            self.equalizer_filter_active = False

    def show_player_equalizer(self) -> None:
        if not self.player_is_active():
            return
        original_enabled = self.session_equalizer_enabled
        original_gains = dict(self.session_equalizer_gains)
        _enabled, gains = self.effective_equalizer_state()
        active_preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        db_range = self.equalizer_db_range_value()
        slider_min = -db_range * 10
        slider_max = db_range * 10
        dialog = wx.Dialog(self, title=self.t("equalizer"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("equalizer"))
        dialog.SetMinSize((520, 520))
        preset_options = self.equalizer_preset_options()
        dialog_visible_preset = active_preset
        outer = wx.BoxSizer(wx.VERTICAL)
        outer.Add(wx.StaticText(dialog, label=self.t("equalizer_preset")), 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
        preset_choice = wx.Choice(dialog, choices=self.equalizer_preset_labels())
        preset_choice.SetName(self.t("equalizer_preset"))
        preset_choice.SetSelection(preset_options.index(active_preset) if active_preset in preset_options else 0)
        outer.Add(preset_choice, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)
        name_label = wx.StaticText(dialog, label=self.t("equalizer_preset_name"))
        name_ctrl = wx.TextCtrl(dialog, value=self.equalizer_custom_name(active_preset))
        name_ctrl.SetName(self.t("equalizer_preset_name"))
        outer.Add(name_label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
        outer.Add(name_ctrl, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)
        sliders: dict[str, wx.Slider] = {}
        for band_id, band_label in EQ_BANDS:
            label_text = self.t("equalizer_band_gain", band=band_label)
            outer.Add(wx.StaticText(dialog, label=label_text), 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
            band_value = min(max(int(round(gains.get(band_id, 0.0) * 10)), slider_min), slider_max)
            slider = wx.Slider(
                dialog,
                value=band_value,
                minValue=slider_min,
                maxValue=slider_max,
                style=wx.SL_HORIZONTAL,
            )
            self.set_equalizer_slider_accessibility(slider, label_text)
            sliders[band_id] = slider
            outer.Add(slider, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)

        buttons = wx.StdDialogButtonSizer()
        ok_button = wx.Button(dialog, wx.ID_OK)
        cancel_button = wx.Button(dialog, wx.ID_CANCEL)
        reset_button = wx.Button(dialog, label=self.t("reset_equalizer"))
        save_global_button = wx.Button(dialog, label=self.t("save_equalizer_as_global"))
        add_profile_button = wx.Button(dialog, label=self.t("add_equalizer_profile"))
        buttons.AddButton(ok_button)
        buttons.AddButton(cancel_button)
        buttons.Realize()
        row = wx.BoxSizer(wx.HORIZONTAL)
        row.Add(add_profile_button, 0, wx.RIGHT, 8)
        row.Add(save_global_button, 0, wx.RIGHT, 8)
        row.Add(reset_button, 0, wx.RIGHT, 8)
        row.Add(buttons, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.ALL, 8)
        dialog.SetSizer(outer)

        def current_dialog_gains() -> dict[str, float]:
            return {band_id: round(float(ctrl.GetValue()) / 10.0, 1) for band_id, ctrl in sliders.items()}

        def current_preset() -> str:
            index = preset_choice.GetSelection()
            return preset_options[index] if 0 <= index < len(preset_options) else EQ_PRESET_FLAT

        def update_custom_name_visibility() -> None:
            visible = self.is_custom_equalizer_preset(current_preset())
            name_label.Show(visible)
            name_ctrl.Show(visible)
            dialog.Layout()

        def save_current_dialog_name(preset_id: str | None = None) -> None:
            preset_id = self.normalized_equalizer_preset(preset_id or dialog_visible_preset)
            if not self.is_custom_equalizer_preset(preset_id):
                return
            names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
            names[preset_id] = name_ctrl.GetValue().strip()[:80] or self.equalizer_custom_name(preset_id)
            self.settings.equalizer_custom_names = names

        def live_apply() -> None:
            self.session_equalizer_enabled = True
            self.session_equalizer_gains = current_dialog_gains()
            self.apply_equalizer_to_player()

        def load_preset_into_sliders(preset_id: str) -> None:
            nonlocal dialog_visible_preset
            dialog_visible_preset = self.normalized_equalizer_preset(preset_id)
            preset_gains = self.equalizer_gains_for_preset(preset_id)
            for band_id, band_label in EQ_BANDS:
                value = min(max(preset_gains.get(band_id, 0.0), -db_range), db_range)
                sliders[band_id].SetValue(int(round(value * 10)))
                self.set_equalizer_slider_accessibility(sliders[band_id], self.t("equalizer_band_gain", band=band_label))
            name_ctrl.SetValue(self.equalizer_custom_name(preset_id))
            update_custom_name_visibility()
            live_apply()

        def refresh_preset_choices(selected_preset: str) -> None:
            nonlocal preset_options
            nonlocal dialog_visible_preset
            dialog_visible_preset = self.normalized_equalizer_preset(selected_preset)
            preset_options = self.equalizer_preset_options()
            preset_choice.SetItems(self.equalizer_preset_labels())
            preset_choice.SetSelection(preset_options.index(selected_preset) if selected_preset in preset_options else 0)
            update_custom_name_visibility()

        def on_preset_changed(_event: wx.CommandEvent) -> None:
            save_current_dialog_name(dialog_visible_preset)
            load_preset_into_sliders(current_preset())

        def on_slider(event: wx.CommandEvent, label: str) -> None:
            ctrl = event.GetEventObject()
            if isinstance(ctrl, wx.Slider):
                self.set_equalizer_slider_accessibility(ctrl, label)
            live_apply()

        for band_id, band_label in EQ_BANDS:
            sliders[band_id].Bind(wx.EVT_SLIDER, lambda evt, label=self.t("equalizer_band_gain", band=band_label): on_slider(evt, label))
        preset_choice.Bind(wx.EVT_CHOICE, on_preset_changed)

        def reset_dialog_equalizer(_event=None) -> None:
            preset_gains = self.factory_equalizer_gains_for_preset(current_preset())
            for band_id, band_label in EQ_BANDS:
                value = min(max(preset_gains.get(band_id, 0.0), -db_range), db_range)
                sliders[band_id].SetValue(int(round(value * 10)))
                self.set_equalizer_slider_accessibility(sliders[band_id], self.t("equalizer_band_gain", band=band_label))
            live_apply()

        reset_button.Bind(wx.EVT_BUTTON, reset_dialog_equalizer)

        def add_profile_from_dialog(_event=None) -> None:
            preset_id = self.create_equalizer_profile_dialog(current_dialog_gains())
            if not preset_id:
                return
            refresh_preset_choices(preset_id)
            name_ctrl.SetValue(self.equalizer_custom_name(preset_id))
            live_apply()

        def save_dialog_as_global(_event=None) -> None:
            save_current_dialog_name()
            preset_id = self.choose_equalizer_profile_for_save(current_dialog_gains())
            if not preset_id:
                return
            self.settings.global_equalizer_enabled = True
            self.settings.global_equalizer_preset = preset_id
            self.save_settings()
            refresh_preset_choices(preset_id)
            self.announce_player(self.t("equalizer_profile_saved"))

        add_profile_button.Bind(wx.EVT_BUTTON, add_profile_from_dialog)
        save_global_button.Bind(wx.EVT_BUTTON, save_dialog_as_global)
        update_custom_name_visibility()
        result = dialog.ShowModal()
        if result == wx.ID_OK:
            save_current_dialog_name()
            self.save_settings()
        dialog.Destroy()
        if result == wx.ID_OK:
            if self.bass_boost_enabled:
                self.session_equalizer_before_bass_boost = (self.session_equalizer_enabled, dict(self.session_equalizer_gains))
            self.announce_player(self.t("equalizer_saved"))
            return
        self.session_equalizer_enabled = original_enabled
        self.session_equalizer_gains = original_gains
        self.apply_equalizer_to_player()
        self.announce_player(self.t("equalizer_closed"))

    def play_relative_item(self, delta: int) -> None:
        if delta > 0:
            queued_item = self.pop_next_playback_queue_item()
            if queued_item:
                self.open_playback_queue_item(queued_item, announce_start=True)
                return
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
        self.open_relative_player_item(item, announce_start=True)

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
        if self.shuffle_current and delta > 0 and playable:
            choices = [item for item in playable if str(item.get("url") or "") != current_url] or playable
            return dict(random.choice(choices))
        if current_pos >= 0:
            item_index = current_pos + delta
        if 0 <= item_index < len(playable):
            return dict(playable[item_index])
        return None

    def open_relative_player_item(self, item: dict, announce_start: bool = False) -> None:
        if not item.get("url"):
            return
        show_player = self.in_player_screen or not self.background_playback_enabled()
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
        elif self.player_return_screen == "folder" or item.get("kind") == "local_file":
            results = self.return_all_results or self.all_results or self.return_results or self.results
            self.return_index = next((i for i, result in enumerate(results) if result.get("url") == item.get("url")), self.return_index)
            self.player_return_screen = "folder"
            self.player_return_data = {"index": self.return_index, "folder": self.last_search_query}
        else:
            results = self.return_all_results or self.all_results or self.return_results or self.results
            self.return_index = next((i for i, result in enumerate(results) if result.get("url") == item.get("url")), self.return_index)
            self.player_return_screen = "search"
            self.player_return_data = {"index": self.return_index}
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(str(item.get("url") or ""), str(item.get("title") or ""), show_player=show_player, announce_start=announce_start)

    def playable_queue_item(self, item: dict | None) -> dict | None:
        if not item or item.get("kind") in {"channel", "playlist"}:
            return None
        url = str(item.get("url") or item.get("webpage_url") or "").strip()
        if not url:
            return None
        return self.playlist_item_from_media(dict(item))

    def add_active_to_playback_queue(self) -> None:
        item = self.playable_queue_item(self.active_item())
        if not item:
            self.announce_player(self.t("no_selection"))
            return
        url = str(item.get("url") or "")
        if any(str(existing.get("url") or "") == url for existing in self.playback_queue):
            self.announce_player(self.t("playback_queue_already_added", title=item.get("title", "")))
            return
        self.playback_queue.append(item)
        self.save_playback_queue()
        self.refresh_main_menu_after_playback_queue_change()
        self.announce_player(self.t("playback_queue_added", title=item.get("title", "")))

    def remove_active_from_playback_queue(self) -> None:
        item = self.playable_queue_item(self.active_item())
        if not item:
            self.announce_player(self.t("no_selection"))
            return
        if self.remove_playback_queue_url(str(item.get("url") or "")):
            self.announce_player(self.t("playback_queue_removed", title=item.get("title", "")))
        else:
            self.announce_player(self.t("playback_queue_not_found"))

    def remove_playback_queue_url(self, url: str) -> bool:
        before = len(self.playback_queue)
        self.playback_queue = [item for item in self.playback_queue if str(item.get("url") or "") != url]
        changed = len(self.playback_queue) != before
        if changed:
            self.save_playback_queue()
            self.refresh_main_menu_after_playback_queue_change()
        return changed

    def clear_auto_folder_playback_queue(self) -> None:
        before = len(self.playback_queue)
        self.playback_queue = [item for item in self.playback_queue if not item.get("_auto_folder_queue")]
        if len(self.playback_queue) == before:
            return
        self.save_playback_queue()
        self.refresh_main_menu_after_playback_queue_change()

    def refresh_main_menu_after_playback_queue_change(self) -> None:
        if not self.in_main_menu or not hasattr(self, "menu_list"):
            return
        wx.CallAfter(self.refresh_main_menu_download_label)

    def playback_queue_line(self, item: dict, index: int) -> str:
        parts = [
            str(index + 1),
            item.get("title", ""),
            f"{self.t('channel')}: {item.get('channel', '')}" if item.get("channel") else "",
            item.get("type", ""),
        ]
        return ". ".join([parts[0], " | ".join(part for part in parts[1:] if part)])

    def open_playback_queue_shortcut(self) -> None:
        self.show_playback_queue()

    def show_playback_queue(self) -> None:
        if not self.playback_queue:
            self.announce_player(self.t("playback_queue_empty"))
            self.refresh_main_menu_after_playback_queue_change()
            return
        dialog = wx.Dialog(self, title=self.t("playback_queue"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("playback_queue"))
        dialog.SetMinSize((560, 420))
        outer = wx.BoxSizer(wx.VERTICAL)
        instructions = wx.StaticText(dialog, label=self.t("playback_queue_instructions"))
        outer.Add(instructions, 0, wx.ALL, 8)
        queue_list = wx.ListBox(dialog, choices=[self.playback_queue_line(item, index) for index, item in enumerate(self.playback_queue)])
        queue_list.SetName(self.t("playback_queue"))
        queue_list.SetSelection(0)
        outer.Add(queue_list, 1, wx.EXPAND | wx.LEFT | wx.RIGHT | wx.BOTTOM, 8)
        row = wx.BoxSizer(wx.HORIZONTAL)
        play_button = wx.Button(dialog, label=self.t("play"))
        move_up_button = wx.Button(dialog, label=self.t("move_up"))
        move_down_button = wx.Button(dialog, label=self.t("move_down"))
        remove_button = wx.Button(dialog, label=self.t("remove_from_playback_queue"))
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        row.Add(play_button, 0, wx.RIGHT, 8)
        row.Add(move_up_button, 0, wx.RIGHT, 8)
        row.Add(move_down_button, 0, wx.RIGHT, 8)
        row.Add(remove_button, 0, wx.RIGHT, 8)
        row.Add(close_button, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.ALL, 8)
        dialog.SetSizer(outer)
        action: dict[str, int | str] = {}

        def selected_index() -> int:
            index = queue_list.GetSelection()
            return index if 0 <= index < len(self.playback_queue) else -1

        def play_selected(_event=None) -> None:
            index = selected_index()
            if index >= 0:
                action.update({"action": "play", "index": index})
                dialog.EndModal(wx.ID_OK)

        def remove_selected(_event=None) -> None:
            index = selected_index()
            if index < 0:
                return
            title = str(self.playback_queue[index].get("title") or "")
            del self.playback_queue[index]
            self.save_playback_queue()
            queue_list.Set([self.playback_queue_line(item, item_index) for item_index, item in enumerate(self.playback_queue)] or [self.t("playback_queue_empty")])
            if self.playback_queue:
                queue_list.SetSelection(min(index, len(self.playback_queue) - 1))
            self.announce_player(self.t("playback_queue_removed", title=title))
            if not self.playback_queue:
                dialog.EndModal(wx.ID_CANCEL)

        def refresh_queue_list(selection: int) -> None:
            labels = [self.playback_queue_line(item, item_index) for item_index, item in enumerate(self.playback_queue)] or [self.t("playback_queue_empty")]
            queue_list.Set(labels)
            if self.playback_queue:
                queue_list.SetSelection(min(max(0, selection), len(self.playback_queue) - 1))

        def move_selected(delta: int) -> None:
            index = selected_index()
            target = index + delta
            if index < 0 or target < 0 or target >= len(self.playback_queue):
                return
            self.playback_queue[index], self.playback_queue[target] = self.playback_queue[target], self.playback_queue[index]
            self.save_playback_queue()
            refresh_queue_list(target)
            self.announce_player(self.t("playback_queue_reordered"))

        def open_queue_context_menu(event=None) -> None:
            menu = wx.Menu()
            actions = [
                (self.t("play"), play_selected),
                (self.t("move_up"), lambda _evt=None: move_selected(-1)),
                (self.t("move_down"), lambda _evt=None: move_selected(1)),
                (self.t("remove_from_playback_queue"), remove_selected),
            ]
            for label, handler in actions:
                menu_item = menu.Append(wx.ID_ANY, label)
                dialog.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), menu_item)
            queue_list.PopupMenu(menu)
            menu.Destroy()

        queue_list.Bind(wx.EVT_LISTBOX_DCLICK, play_selected)
        def on_queue_key(evt: wx.KeyEvent) -> None:
            if self.shortcut_matches(evt, "open_selected"):
                play_selected()
                return
            if self.context_menu_shortcut_matches(evt):
                open_queue_context_menu(evt)
                return
            evt.Skip()

        queue_list.Bind(wx.EVT_KEY_DOWN, on_queue_key)
        queue_list.Bind(wx.EVT_CONTEXT_MENU, open_queue_context_menu)
        play_button.Bind(wx.EVT_BUTTON, play_selected)
        move_up_button.Bind(wx.EVT_BUTTON, lambda _evt: move_selected(-1))
        move_down_button.Bind(wx.EVT_BUTTON, lambda _evt: move_selected(1))
        remove_button.Bind(wx.EVT_BUTTON, remove_selected)
        result = dialog.ShowModal()
        dialog.Destroy()
        self.refresh_main_menu_after_playback_queue_change()
        if result == wx.ID_OK and action.get("action") == "play":
            self.play_playback_queue_index(int(action.get("index", -1)))

    def play_playback_queue_index(self, index: int) -> None:
        if index < 0 or index >= len(self.playback_queue):
            self.announce_player(self.t("playback_queue_empty"))
            return
        item = dict(self.playback_queue.pop(index))
        self.save_playback_queue()
        self.refresh_main_menu_after_playback_queue_change()
        self.open_playback_queue_item(item)

    def pop_next_playback_queue_item(self) -> dict | None:
        if not self.playback_queue:
            return None
        item = dict(self.playback_queue.pop(0))
        self.save_playback_queue()
        self.refresh_main_menu_after_playback_queue_change()
        return item

    def open_playback_queue_item(self, item: dict, announce_start: bool = False) -> None:
        show_player = self.in_player_screen or not self.background_playback_enabled()
        self.open_playback_queue_item_with_mode(item, show_player=show_player, announce_start=announce_start)

    def open_playback_queue_item_with_mode(self, item: dict, show_player: bool = True, announce_start: bool = False) -> None:
        url = str(item.get("url") or "")
        if not url:
            self.announce_player(self.t("no_selection"))
            return
        source_screen = str(item.get("_return_screen") or "")
        if source_screen == "folder":
            self.player_return_screen = "folder"
            self.player_return_data = {
                "index": int(item.get("_return_index") or 0),
                "folder": str(item.get("_return_folder") or self.last_search_query),
            }
        else:
            self.player_return_screen = "playback_queue"
            self.player_return_data = {}
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(url, str(item.get("title") or ""), show_player=show_player, announce_start=announce_start)

    def current_local_media_path(self) -> Path | None:
        item = self.current_video_item or self.current_video_info or {}
        if str(item.get("kind") or "") != "local_file":
            return None
        return self.local_media_path_from_input(str(item.get("url") or item.get("webpage_url") or ""))

    def toggle_edit_mode(self) -> None:
        if not self.player_is_active():
            return
        if not self.current_local_media_path():
            self.announce_player(self.t("edit_mode_local_only"))
            return
        self.edit_mode_enabled = not self.edit_mode_enabled
        self.announce_player(self.t("edit_mode_on" if self.edit_mode_enabled else "edit_mode_off"))

    def save_edited_local_file(self, replace_original: bool = False) -> None:
        if not self.edit_mode_enabled:
            return
        source = self.current_local_media_path()
        if not source:
            self.announce_player(self.t("edit_mode_local_only"))
            return
        self.announce_player(self.t("edit_save_started"))
        if replace_original:
            self.stop_player(silent=True)
        threading.Thread(target=self.save_edited_local_file_worker, args=(source, replace_original), daemon=True).start()

    def save_edited_local_file_worker(self, source: Path, replace_original: bool = False) -> None:
        try:
            ffmpeg = self.ffmpeg_executable()
            if not ffmpeg:
                raise RuntimeError("FFmpeg was not found")
            output = self.edited_output_path(source, replace_original)
            temp_output = output.with_name(f"{output.stem}.apricot-temp{output.suffix}") if replace_original else output
            args = self.local_edit_ffmpeg_args(ffmpeg, source, temp_output)
            creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
            result = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace", creationflags=creationflags)
            if result.returncode != 0:
                error = (result.stderr or result.stdout or "").strip() or f"FFmpeg exited with code {result.returncode}"
                raise RuntimeError(error[-600:])
            if replace_original:
                os.replace(temp_output, source)
                wx.CallAfter(self.announce_player, self.t("edit_replace_done", title=source.name))
            else:
                wx.CallAfter(self.announce_player, self.t("edit_save_done", title=output.name))
        except Exception as exc:
            wx.CallAfter(self.message, self.t("edit_save_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def edited_output_path(self, source: Path, replace_original: bool = False) -> Path:
        if replace_original:
            return source
        output = source.with_name(f"{source.stem} - edited{source.suffix}")
        counter = 2
        while output.exists():
            output = source.with_name(f"{source.stem} - edited ({counter}){source.suffix}")
            counter += 1
        return output

    @staticmethod
    def is_video_file_extension(path: Path) -> bool:
        return path.suffix.lower() in {".3g2", ".3gp", ".avi", ".m4v", ".mkv", ".mov", ".mp4", ".mpeg", ".mpg", ".webm", ".wmv"}

    def current_speed_value(self) -> float:
        try:
            return self.parse_rate_value(self.current_video_info.get("speed") or self.settings.player_speed or 1.0)
        except (TypeError, ValueError):
            return 1.0

    @staticmethod
    def ffmpeg_atempo_chain(factor: float) -> list[str]:
        values: list[str] = []
        factor = max(0.0625, min(16.0, float(factor or 1.0)))
        while factor < 0.5:
            values.append("atempo=0.5")
            factor /= 0.5
        while factor > 2.0:
            values.append("atempo=2.0")
            factor /= 2.0
        if abs(factor - 1.0) >= 0.001:
            values.append(f"atempo={factor:.6f}")
        return values

    def ffmpeg_equalizer_filters(self, gains: dict[str, float]) -> list[str]:
        filters: list[str] = []
        for band_id, _band_label in EQ_BANDS:
            gain = max(-24.0, min(24.0, float(gains.get(band_id, 0.0) or 0.0)))
            if abs(gain) >= 0.05:
                filters.append(f"equalizer=f={band_id}:t=q:w=1:g={gain:.1f}")
        return filters

    def local_edit_audio_filters(self) -> list[str]:
        speed = max(0.25, min(4.0, self.current_speed_value()))
        pitch = max(0.5, min(2.0, self.current_pitch_value()))
        enabled, gains = self.effective_equalizer_state()
        filters = self.ffmpeg_equalizer_filters(gains) if enabled else []
        if abs(pitch - 1.0) >= 0.001:
            filters.extend([f"asetrate=48000*{pitch:.6f}", "aresample=48000"])
        filters.extend(self.ffmpeg_atempo_chain(speed / pitch))
        return filters

    def local_edit_audio_codec_args(self, suffix: str) -> list[str]:
        suffix = suffix.lower()
        if suffix == ".mp3":
            return ["-c:a", "libmp3lame", "-b:a", "320k"]
        if suffix in {".m4a", ".mp4", ".m4v", ".mov"}:
            return ["-c:a", "aac", "-b:a", "256k"]
        if suffix == ".opus":
            return ["-c:a", "libopus", "-b:a", "160k"]
        if suffix == ".wav":
            return ["-c:a", "pcm_s16le"]
        if suffix == ".flac":
            return ["-c:a", "flac"]
        return ["-c:a", "aac", "-b:a", "256k"]

    def local_edit_ffmpeg_args(self, ffmpeg: str, source: Path, output: Path) -> list[str]:
        speed = max(0.25, min(4.0, self.current_speed_value()))
        audio_filters = self.local_edit_audio_filters()
        args = [ffmpeg, "-y", "-hide_banner", "-loglevel", "error", "-i", str(source)]
        if audio_filters:
            args.extend(["-af", ",".join(audio_filters)])
            args.extend(self.local_edit_audio_codec_args(output.suffix))
        else:
            args.extend(["-c:a", "copy"])
        if self.is_video_file_extension(source):
            if abs(speed - 1.0) >= 0.001:
                args.extend(["-vf", f"setpts={1.0 / speed:.8f}*PTS", "-c:v", "libx264", "-preset", "veryfast", "-crf", "18"])
            else:
                args.extend(["-c:v", "copy"])
        args.append(str(output))
        return args

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
            old_selection = self.menu_list.GetSelection()
            old_label = self.menu_list.GetString(old_selection) if old_selection != wx.NOT_FOUND else ""
            self.menu_actions = self.build_main_menu_actions()
            labels = [item[0] for item in self.menu_actions]
            selection = old_selection
            if old_label in labels:
                selection = labels.index(old_label)
            elif labels:
                selection = min(max(0, old_selection), len(labels) - 1)
            self.set_listbox_items(self.menu_list, labels, selection)
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
        if generation != self.player_generation:
            return
        if self.mpv_process_alive():
            try:
                if not bool(self.mpv_get_property("eof-reached", timeout=0.15)):
                    return
            except Exception:
                pass
        if self.repeat_current:
            self.player_ended = False
            self.player_paused = False
            self.update_play_pause_buttons()
            self.restart_current_playback(announce=False)
            return
        queued_item = self.pop_next_playback_queue_item()
        if queued_item:
            self.open_playback_queue_item_with_mode(queued_item, show_player=self.in_player_screen or not self.background_playback_enabled())
            return
        if self.shuffle_current or self.settings.autoplay_next:
            next_item = self.relative_player_item(1)
            if next_item:
                self.open_relative_player_item(next_item)
                return
        self.player_ended = True
        self.player_paused = True
        self.update_play_pause_buttons()
        if bool(getattr(self.settings, "announce_playback_finished", True)):
            self.announce_player(self.t("playback_finished"))
        else:
            self.set_status(self.t("playback_finished"))

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
            if self.player_should_restart_from_end(eof_reached):
                self.restart_current_playback()
                return
            self.player_ended = False
            try:
                self.mpv_set_property("pause", False, timeout=0.5)
                self.start_player_monitor(self.player_generation)
                self.player_paused = False
                self.update_play_pause_buttons()
                self.announce_play_pause_state(False)
            except Exception:
                self.toggle_player_pause_fallback()
            return
        self.toggle_player_pause()

    def toggle_player_pause(self) -> None:
        try:
            paused = bool(self.mpv_get_property("pause", timeout=0.35))
            new_paused = not paused
            self.mpv_set_property("pause", new_paused, timeout=0.5)
            self.player_paused = new_paused
            self.update_play_pause_buttons()
            self.announce_play_pause_state(new_paused)
        except Exception:
            self.toggle_player_pause_fallback()

    def toggle_player_pause_fallback(self) -> None:
        self.player_command("cycle pause")
        wx.CallLater(140, self.refresh_play_pause_button_state)
        if self.settings.announce_play_pause:
            wx.CallLater(120, self.announce_current_play_pause_state)

    def announce_current_play_pause_state(self) -> None:
        if not self.settings.announce_play_pause or self.player_kind != "mpv" or not self.mpv_process_alive():
            return
        try:
            paused = bool(self.mpv_get_property("pause", timeout=0.35))
            self.player_paused = paused
            self.update_play_pause_buttons()
            self.announce_play_pause_state(paused)
        except Exception:
            pass

    def refresh_play_pause_button_state(self) -> None:
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            self.update_play_pause_buttons()
            return
        try:
            self.player_paused = bool(self.mpv_get_property("pause", timeout=0.35))
        except Exception:
            pass
        self.update_play_pause_buttons()

    def announce_play_pause_state(self, paused: bool) -> None:
        if self.settings.announce_play_pause:
            self.announce_player(self.t("playback_paused" if paused else "playback_playing"))

    def player_should_restart_from_end(self, eof_reached: bool) -> bool:
        if self.player_ended and not eof_reached:
            return True
        if not eof_reached:
            return False
        try:
            elapsed = self.mpv_get_property("time-pos", timeout=0.2)
            duration = self.mpv_get_property("duration", timeout=0.2)
            if elapsed is not None and duration is not None:
                return float(elapsed) >= max(0.0, float(duration) - 0.35)
        except Exception:
            pass
        return True

    def restart_current_playback(self, announce: bool = True) -> None:
        self.player_ended = False
        self.player_paused = False
        if self.mpv_process_alive():
            try:
                self.mpv_send(["seek", 0, "absolute+exact"], timeout=0.8)
                self.mpv_set_property("pause", False, timeout=0.8)
                self.start_player_monitor(self.player_generation)
                self.update_play_pause_buttons()
                if announce:
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

    def player_seek(self, seconds: float) -> None:
        if self.player_kind != "mpv" or not self.ipc_path:
            return
        was_ended = self.player_ended
        try:
            response = self.mpv_request(["seek", float(seconds), "relative+exact"], timeout=0.8)
            if response.get("error") == "success":
                self.after_player_seek(seconds, was_ended)
                return
        except Exception:
            pass
        try:
            self.mpv_send(["seek", float(seconds), "relative+exact"], timeout=0.8)
            self.after_player_seek(seconds, was_ended)
        except Exception:
            pass

    def after_player_seek(self, seconds: float, was_ended: bool) -> None:
        if seconds < 0 and was_ended:
            self.player_ended = False
            self.start_player_monitor(self.player_generation)

    def set_clip_marker_async(self, marker: str) -> None:
        threading.Thread(target=self.set_clip_marker_worker, args=(marker,), daemon=True).start()

    def set_clip_marker_worker(self, marker: str) -> None:
        try:
            if marker == "start" and self.clip_start_marker is not None:
                self.clip_start_marker = None
                wx.CallAfter(self.announce_player, self.t("clip_start_marker_cleared"))
                return
            if marker == "end" and self.clip_end_marker is not None:
                self.clip_end_marker = None
                wx.CallAfter(self.announce_player, self.t("clip_end_marker_cleared"))
                return
            elapsed = self.mpv_get_property("time-pos")
            if elapsed is None:
                wx.CallAfter(self.announce_player, self.t("timing_unavailable"))
                return
            position = max(0.0, float(elapsed))
            if marker == "start":
                self.clip_start_marker = position
                wx.CallAfter(self.announce_player, self.t("clip_start_marker_set", time=self.format_seconds(position)))
            else:
                self.clip_end_marker = position
                wx.CallAfter(self.announce_player, self.t("clip_end_marker_set", time=self.format_seconds(position)))
        except Exception:
            wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

    def clip_markers_are_set(self) -> bool:
        return self.clip_start_marker is not None and self.clip_end_marker is not None

    def clip_markers_partially_set(self) -> bool:
        return (self.clip_start_marker is None) != (self.clip_end_marker is None)

    def export_marked_clip(self, audio_only: bool = False) -> None:
        if self.clip_start_marker is None or self.clip_end_marker is None:
            self.announce_player(self.t("clip_markers_missing"))
            return
        start = float(self.clip_start_marker)
        end = float(self.clip_end_marker)
        if end - start < 0.25:
            self.announce_player(self.t("clip_marker_invalid"))
            return
        item = dict(self.current_video_item or self.current_video_info or {})
        stream_url = self.current_stream_url
        headers = dict(self.current_stream_headers or {})
        self.announce_player(self.t("clip_export_started"))
        threading.Thread(target=self.export_marked_clip_worker, args=(item, stream_url, headers, start, end, audio_only), daemon=True).start()

    def ffmpeg_executable(self) -> str:
        configured = str(getattr(self.settings, "ffmpeg_location", "") or "").strip()
        if configured:
            configured_path = Path(configured)
            if configured_path.is_dir():
                candidate = configured_path / ("ffmpeg.exe" if os.name == "nt" else "ffmpeg")
                if candidate.exists():
                    return str(candidate)
            elif configured_path.exists():
                return configured
        bundled = self.bundled_path("ffmpeg", "ffmpeg.exe")
        if bundled.exists():
            return str(bundled)
        return shutil.which("ffmpeg") or ""

    def clip_output_folder_for_item(self, item: dict) -> Path:
        if item.get("kind") == "rss_item":
            folder = self.podcasts_download_folder()
        else:
            folder = self.music_download_folder()
        return folder / "clips"

    def clip_output_extension(self, source: str, item: dict, audio_only: bool = False) -> str:
        if audio_only:
            return f".{self.settings.audio_format}"
        local_path = self.local_media_path_from_input(source)
        if local_path and local_path.suffix:
            return local_path.suffix.lower()
        ext = str(item.get("ext") or "").strip().lower().lstrip(".")
        if ext:
            return f".{ext}"
        kind = str(item.get("kind") or "")
        if kind == "rss_item":
            return ".m4a"
        return ".mp4"

    def audio_export_codec_args(self) -> list[str]:
        fmt = str(self.settings.audio_format or "mp3").lower()
        quality = self.normalize_audio_quality_value(self.settings.audio_quality)
        if fmt == "mp3":
            bitrate = "320" if quality == "0" else quality
            return ["-vn", "-c:a", "libmp3lame", "-b:a", f"{bitrate}k"]
        if fmt == "m4a":
            bitrate = "256" if quality == "0" else quality
            return ["-vn", "-c:a", "aac", "-b:a", f"{bitrate}k"]
        if fmt == "opus":
            bitrate = "160" if quality == "0" else quality
            return ["-vn", "-c:a", "libopus", "-b:a", f"{bitrate}k"]
        if fmt == "wav":
            return ["-vn", "-c:a", "pcm_s16le"]
        if fmt == "flac":
            return ["-vn", "-c:a", "flac"]
        return ["-vn"]

    def export_marked_clip_worker(self, item: dict, stream_url: str, headers: dict, start: float, end: float, audio_only: bool = False) -> None:
        try:
            ffmpeg = self.ffmpeg_executable()
            if not ffmpeg:
                raise RuntimeError("FFmpeg was not found")
            input_url = stream_url
            if not input_url:
                input_url, headers, _info = self.resolve_stream_url(str(item.get("url") or ""))
            folder = self.clip_output_folder_for_item(item)
            folder.mkdir(parents=True, exist_ok=True)
            title = str(item.get("title") or Path(input_url).stem or "clip")
            suffix = self.clip_output_extension(input_url, item, audio_only=audio_only)
            output = folder / f"{self.safe_folder_name(title)} - {self.format_seconds(start).replace(':', '-')}-{self.format_seconds(end).replace(':', '-')}{suffix}"
            counter = 2
            while output.exists():
                output = folder / f"{self.safe_folder_name(title)} - {self.format_seconds(start).replace(':', '-')}-{self.format_seconds(end).replace(':', '-')} ({counter}){suffix}"
                counter += 1
            args = [ffmpeg, "-y", "-hide_banner", "-loglevel", "error", "-ss", f"{start:.3f}"]
            header_text = "".join(f"{name}: {value}\r\n" for name, value in headers.items() if value)
            if header_text:
                args.extend(["-headers", header_text])
            args.extend(["-i", input_url, "-t", f"{end - start:.3f}"])
            if audio_only:
                args.extend(self.audio_export_codec_args())
            else:
                args.extend(["-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero"])
            args.append(str(output))
            creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
            result = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace", creationflags=creationflags)
            if result.returncode != 0:
                error = (result.stderr or result.stdout or "").strip() or f"FFmpeg exited with code {result.returncode}"
                raise RuntimeError(error[-600:])
            wx.CallAfter(self.announce_player, self.t("clip_export_done", title=output.name))
        except Exception as exc:
            wx.CallAfter(self.message, self.t("clip_export_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

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
            return self.parse_rate_value(stored)
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
            self.session_volume = volume
        except Exception:
            pass

    def announce_volume_async(self) -> None:
        threading.Thread(target=self.announce_volume_worker, daemon=True).start()

    def announce_volume_worker(self) -> None:
        try:
            current = self.mpv_get_property("volume", timeout=0.5)
            if current is None:
                raise RuntimeError("volume unavailable")
            volume = int(round(float(current)))
            wx.CallAfter(self.announce_player, self.t("volume_announcement", volume=volume))
        except Exception:
            wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

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
                self.session_volume = 100.0
            elif current is not None:
                self.session_volume = max(0.0, min(100.0, float(current)))
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
    def parse_rate_value(value) -> float:
        text = str(value).strip().lower().removesuffix("x").strip()
        return float(text)

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

    def default_volume_value(self) -> int:
        return self.to_int(str(getattr(self.settings, "default_volume", 100)), 100, 0, 300)

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
            self.t("pitch_mode_mpv"),
            self.t("pitch_mode_rubberband"),
            self.t("pitch_mode_linked_speed"),
        ]

    def speed_audio_mode_labels(self) -> list[str]:
        return [
            self.t("speed_audio_mode_rubberband"),
            self.t("speed_audio_mode_scaletempo2"),
            self.t("speed_audio_mode_mpv"),
            self.t("speed_audio_mode_scaletempo"),
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

    @staticmethod
    def audio_quality_label(value: str) -> str:
        value = str(value or "").strip()
        if value == "0":
            return "Best variable quality (VBR 0)"
        if value in {"1", "2", "3", "4", "5", "6", "7", "8", "9", "10"}:
            return f"Variable quality (VBR {value})"
        return f"{value} kbps"

    def audio_quality_labels(self) -> list[str]:
        return [self.audio_quality_label(value) for value in AUDIO_QUALITY_OPTIONS]

    def equalizer_db_range_value(self) -> int:
        try:
            value = int(getattr(self.settings, "equalizer_db_range", 12) or 12)
        except (TypeError, ValueError):
            value = 12
        return min(24, max(6, value))

    def normalized_equalizer_preset(self, preset: str | None) -> str:
        value = str(preset or EQ_PRESET_FLAT).strip()
        if value in EQ_FACTORY_PRESET_VALUES or value in EQ_CUSTOM_PRESET_IDS or value.startswith(("custom_", "user_")):
            return value
        return value if value in self.equalizer_preset_options() else EQ_PRESET_FLAT

    @staticmethod
    def normalized_equalizer_gains(gains: dict | None) -> dict[str, float]:
        normalized = default_equalizer_gains()
        if isinstance(gains, dict):
            for band_id, _band_label in EQ_BANDS:
                try:
                    normalized[band_id] = round(max(-24.0, min(24.0, float(gains.get(band_id, 0.0) or 0.0))), 1)
                except (TypeError, ValueError):
                    normalized[band_id] = 0.0
        return normalized

    def normalized_equalizer_preset_gains(self, presets: dict | None) -> dict[str, dict[str, float]]:
        normalized = default_equalizer_preset_gains()
        if isinstance(presets, dict):
            for preset_id in list(EQ_FACTORY_PRESET_VALUES.keys()) + EQ_CUSTOM_PRESET_IDS:
                gains = presets.get(preset_id)
                if isinstance(gains, dict):
                    normalized[preset_id] = self.normalized_equalizer_gains(gains)
            for preset_id, gains in presets.items():
                preset_text = str(preset_id or "").strip()
                if not preset_text or preset_text in normalized or preset_text in EQ_FACTORY_PRESET_VALUES:
                    continue
                if isinstance(gains, dict):
                    normalized[preset_text] = self.normalized_equalizer_gains(gains)
        return normalized

    def normalized_equalizer_custom_names(self, names: dict | None) -> dict[str, str]:
        normalized = default_equalizer_custom_names()
        if isinstance(names, dict):
            for custom_id, value in names.items():
                custom_text = str(custom_id or "").strip()
                if not custom_text or custom_text in EQ_FACTORY_PRESET_VALUES:
                    continue
                name = str(value or "").strip()
                if name:
                    normalized[custom_text] = name[:80]
        return normalized

    def equalizer_custom_ids(self) -> list[str]:
        settings = getattr(self, "settings", None)
        names = self.normalized_equalizer_custom_names(getattr(settings, "equalizer_custom_names", {}) or {})
        presets = self.normalized_equalizer_preset_gains(getattr(settings, "equalizer_preset_gains", {}) or {})
        custom_ids = set(EQ_CUSTOM_PRESET_IDS)
        custom_ids.update(key for key in names if key not in EQ_FACTORY_PRESET_VALUES)
        custom_ids.update(key for key in presets if key not in EQ_FACTORY_PRESET_VALUES)
        return sorted(custom_ids, key=lambda key: (0, EQ_CUSTOM_PRESET_IDS.index(key)) if key in EQ_CUSTOM_PRESET_IDS else (1, key.lower()))

    def equalizer_preset_options(self) -> list[str]:
        return list(EQ_FACTORY_PRESET_VALUES.keys()) + self.equalizer_custom_ids()

    def is_custom_equalizer_preset(self, preset_id: str) -> bool:
        return preset_id not in EQ_FACTORY_PRESET_VALUES

    def equalizer_custom_name(self, preset_id: str) -> str:
        names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
        return names.get(preset_id, default_equalizer_custom_names().get(preset_id, preset_id))

    def equalizer_preset_label(self, preset_id: str) -> str:
        if self.is_custom_equalizer_preset(preset_id):
            return self.equalizer_custom_name(preset_id)
        return self.t(f"eq_preset_{preset_id}")

    def equalizer_preset_labels(self) -> list[str]:
        return [self.equalizer_preset_label(preset_id) for preset_id in self.equalizer_preset_options()]

    def equalizer_gains_for_preset(self, preset_id: str | None) -> dict[str, float]:
        preset_id = self.normalized_equalizer_preset(preset_id)
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        return self.normalized_equalizer_gains(presets.get(preset_id) or {})

    def factory_equalizer_gains_for_preset(self, preset_id: str | None) -> dict[str, float]:
        preset_id = self.normalized_equalizer_preset(preset_id)
        if preset_id in EQ_FACTORY_PRESET_VALUES:
            return equalizer_gains_from_values(EQ_FACTORY_PRESET_VALUES[preset_id])
        return default_equalizer_gains()

    def visible_equalizer_gains(self) -> dict[str, float]:
        gains: dict[str, float] = {}
        if not hasattr(self, "controls"):
            return gains
        for band_id, _band_label in EQ_BANDS:
            ctrl = self.controls.get(f"eq_{band_id}")
            if isinstance(ctrl, wx.Slider):
                gains[band_id] = round(float(ctrl.GetValue()) / 10.0, 1)
        return gains

    def save_visible_equalizer_gains_to_preset(self, preset_id: str | None = None) -> None:
        preset_id = self.normalized_equalizer_preset(preset_id or getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        gains = self.visible_equalizer_gains()
        if not gains:
            return
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        presets[preset_id] = self.normalized_equalizer_gains(gains)
        self.settings.equalizer_preset_gains = presets
        self.settings.global_equalizer_gains = self.normalized_equalizer_gains(gains)

    def next_equalizer_profile_id(self) -> str:
        existing = set(self.equalizer_preset_options())
        counter = 1
        while True:
            candidate = f"custom_{counter}"
            if candidate not in existing:
                return candidate
            counter += 1

    def create_equalizer_profile(self, name: str, gains: dict[str, float] | None = None) -> str:
        preset_id = self.next_equalizer_profile_id()
        names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
        names[preset_id] = (name.strip()[:80] if name.strip() else self.t("equalizer_profile_name"))
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        presets[preset_id] = self.normalized_equalizer_gains(gains or default_equalizer_gains())
        self.settings.equalizer_custom_names = names
        self.settings.equalizer_preset_gains = presets
        return preset_id

    def create_equalizer_profile_dialog(self, gains: dict[str, float] | None = None) -> str:
        with wx.TextEntryDialog(self, self.t("equalizer_profile_name"), self.t("add_equalizer_profile"), self.t("equalizer_profile_name")) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return ""
            name = dialog.GetValue().strip()
        preset_id = self.create_equalizer_profile(name, gains)
        self.settings.global_equalizer_preset = preset_id
        self.save_settings()
        self.announce_player(self.t("equalizer_profile_saved"))
        return preset_id

    def choose_equalizer_profile_for_save(self, gains: dict[str, float]) -> str:
        profile_ids = self.equalizer_custom_ids()
        labels = [self.equalizer_custom_name(profile_id) for profile_id in profile_ids]
        labels.append(self.t("add_equalizer_profile"))
        with wx.SingleChoiceDialog(self, self.t("save_equalizer_as_global"), self.t("equalizer"), labels) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return ""
            selection = dialog.GetSelection()
        if selection == len(profile_ids):
            return self.create_equalizer_profile_dialog(gains)
        if selection < 0 or selection >= len(profile_ids):
            return ""
        preset_id = profile_ids[selection]
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        presets[preset_id] = self.normalized_equalizer_gains(gains)
        self.settings.equalizer_preset_gains = presets
        self.save_settings()
        return preset_id

    def add_equalizer_profile_from_settings(self) -> None:
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        preset_id = self.create_equalizer_profile_dialog(self.visible_equalizer_gains() or default_equalizer_gains())
        if not preset_id:
            return
        self.settings.global_equalizer_preset = preset_id
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_preset")

    def on_global_equalizer_toggle(self, _event: wx.CommandEvent) -> None:
        ctrl = self.controls.get("global_equalizer") if hasattr(self, "controls") else None
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        if isinstance(ctrl, wx.CheckBox):
            self.settings.global_equalizer_enabled = ctrl.GetValue()
        if self.player_is_active():
            self.use_global_equalizer_for_live_preview()
            self.apply_equalizer_to_player()
        wx.CallAfter(self.render_settings_section_and_focus, "global_equalizer")

    def on_equalizer_settings_preset_changed(self, _event: wx.CommandEvent) -> None:
        previous = getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        self.save_visible_equalizer_gains_to_preset(previous)
        preset = self.selected_choice_value("equalizer_preset")
        self.settings.global_equalizer_preset = self.normalized_equalizer_preset(preset)
        if self.player_is_active():
            self.use_global_equalizer_for_live_preview()
            self.apply_equalizer_to_player()
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_preset")

    def on_equalizer_settings_name_changed(self, event: wx.FocusEvent) -> None:
        preset = self.normalized_equalizer_preset(getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT)))
        ctrl = self.controls.get("equalizer_preset_name") if hasattr(self, "controls") else None
        if self.is_custom_equalizer_preset(preset) and isinstance(ctrl, wx.TextCtrl):
            names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
            names[preset] = ctrl.GetValue().strip()[:80] or self.equalizer_custom_name(preset)
            self.settings.equalizer_custom_names = names
            self.save_visible_equalizer_gains_to_preset(preset)
            wx.CallAfter(self.render_settings_section_and_focus, "equalizer_preset")
        event.Skip()

    def on_equalizer_settings_slider(self, event: wx.CommandEvent, label: str) -> None:
        ctrl = event.GetEventObject()
        if isinstance(ctrl, wx.Slider):
            self.set_equalizer_slider_accessibility(ctrl, label)
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        if self.player_is_active():
            self.settings.global_equalizer_enabled = True
            self.use_global_equalizer_for_live_preview()
            self.apply_equalizer_to_player()
        event.Skip()

    def reset_visible_equalizer_controls(self) -> None:
        if not hasattr(self, "controls"):
            return
        preset = self.normalized_equalizer_preset(self.selected_choice_value("equalizer_preset") or getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        gains = self.factory_equalizer_gains_for_preset(preset)
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        presets[preset] = gains
        self.settings.equalizer_preset_gains = presets
        for band_id, band_label in EQ_BANDS:
            ctrl = self.controls.get(f"eq_{band_id}")
            if isinstance(ctrl, wx.Slider):
                value = gains.get(band_id, 0.0)
                ctrl.SetValue(int(round(value * 10)))
                self.set_equalizer_slider_accessibility(ctrl, self.t("equalizer_band_gain", band=band_label))
        if self.player_is_active():
            self.use_global_equalizer_for_live_preview()
            self.apply_equalizer_to_player()
        self.announce_player(self.t("equalizer_saved"))

    def result_limit_labels(self, options: list[str]) -> list[str]:
        return [self.t("dynamic_results") if option == "0" else option for option in options]

    def refresh_interval_labels(self) -> list[str]:
        return [self.refresh_interval_label(option) for option in REFRESH_INTERVAL_OPTIONS]

    def refresh_interval_label(self, value: str) -> str:
        try:
            hours = float(value)
        except (TypeError, ValueError):
            hours = 1.0
        if hours == 0.5:
            return self.t("interval_30_minutes")
        if hours == 1.0:
            return self.t("interval_1_hour")
        label_hours = int(hours) if hours.is_integer() else hours
        return self.t("interval_hours", hours=label_hours)

    @staticmethod
    def format_refresh_interval_value(value, default: float) -> str:
        try:
            hours = max(0.5, min(168.0, float(value)))
        except (TypeError, ValueError):
            hours = default
        if hours.is_integer():
            return str(int(hours))
        return f"{hours:.1f}".rstrip("0").rstrip(".")

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
    def normalize_audio_quality_value(value: str) -> str:
        normalized = str(value or "").strip().lower().replace("kbps", "").replace("k", "").strip()
        try:
            numeric = float(normalized)
        except (TypeError, ValueError):
            return "0"
        if numeric.is_integer():
            normalized = str(int(numeric))
        else:
            normalized = str(numeric)
        return normalized if normalized in AUDIO_QUALITY_OPTIONS else "0"

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

    def stop_player(self, silent: bool = False, reset_session: bool = True) -> None:
        self.save_current_playback_position()
        self.player_generation += 1
        self.player_ended = False
        if not reset_session:
            self.remember_current_player_volume()
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
        self.player_paused = False
        self.rubberband_pitch_filter_active = False
        self.equalizer_filter_active = False
        self.current_stream_url = ""
        self.current_stream_headers = {}
        self.current_audio_device = ""
        if reset_session:
            if self.player_return_screen == "folder":
                self.clear_auto_folder_playback_queue()
            self.player_fullscreen_session = False
            self.player_fullscreen_results_override = False
            self.manual_background_playback_active = False
            self.session_volume = None
            self.session_audio_output_device = ""
            self.session_equalizer_enabled = None
            self.session_equalizer_gains = {}
            self.session_equalizer_before_bass_boost = None
            self.bass_boost_enabled = False
            self.volume_boost_enabled = False
            self.shuffle_current = False
        if self.player_panel is not None:
            try:
                self.root_sizer.Detach(self.player_panel)
            except Exception:
                pass
            try:
                if not self.player_panel.IsBeingDeleted():
                    self.player_panel.Destroy()
            except RuntimeError:
                pass
            self.player_panel = None
        if not self.in_player_screen:
            self.in_player_screen = False
        if not silent:
            self.set_status(self.t("stopped"))

    def close_current_player(self) -> None:
        was_player_screen = self.in_player_screen
        was_main_menu = self.in_main_menu
        had_background_section = bool(getattr(self, "background_player_section_added", False))
        self.stop_player(silent=False)
        self.in_player_screen = False
        self.current_stream_url = ""
        self.current_stream_headers = {}
        self.current_audio_device = ""
        self.announce_player(self.t("player_closed"))
        if was_main_menu or was_player_screen or had_background_section:
            self.show_main_menu()

    def background_play_pause_shortcut(self) -> None:
        if self.player_is_active():
            self.player_play_pause()
            return
        self.announce_player(self.t("no_player"))

    def focus_in_background_player_controls(self, focus: wx.Window | None) -> bool:
        if not focus:
            return False
        return any(focus is control for control in getattr(self, "background_player_controls", []))

    def player_shortcuts_allowed(self, focus: wx.Window | None = None) -> bool:
        return self.in_player_screen or self.focus_in_background_player_controls(focus)

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

    @staticmethod
    def event_key_code(event: wx.KeyEvent) -> int:
        try:
            return int(event.GetKeyCode())
        except Exception:
            return -1

    @staticmethod
    def event_raw_key_code(event: wx.KeyEvent) -> int:
        getter = getattr(event, "GetRawKeyCode", None)
        if not getter:
            return -1
        try:
            return int(getter())
        except Exception:
            return -1

    @staticmethod
    def is_modifier_only_event(event: wx.KeyEvent) -> bool:
        modifier_codes = {
            getattr(wx, "WXK_CONTROL", -1),
            getattr(wx, "WXK_SHIFT", -1),
            getattr(wx, "WXK_ALT", -1),
            16, 17, 18,
            160, 161, 162, 163, 164, 165,
        }
        codes = MainFrame.key_event_codes(event)
        return bool(codes) and all(code in modifier_codes for code in codes)

    def shortcut_allowed_for_focus(self, action: str, focus: wx.Window | None) -> bool:
        return not (self.focus_accepts_text(focus) and self.shortcut_is_plain_printable(action))

    def handle_global_navigation_shortcut(self, event: wx.KeyEvent, focus: wx.Window | None = None) -> bool:
        focus = focus or wx.Window.FindFocus()
        actions = [
            ("open_main_menu", self.open_main_menu_shortcut),
            ("open_search", self.open_search_shortcut),
            ("open_play_from_folder", self.open_play_from_folder_shortcut),
            ("open_direct_link", self.open_direct_link_shortcut),
            ("open_favorites", self.open_favorites_shortcut),
            ("open_playlists", self.open_playlists_shortcut),
            ("open_subscriptions", self.open_subscriptions_shortcut),
            ("open_current_downloads", self.open_current_downloads_shortcut),
            ("open_history", self.open_history_shortcut),
            ("open_podcasts_rss", self.open_podcasts_rss_shortcut),
            ("open_settings", self.open_settings_shortcut),
            ("open_playback_queue", self.open_playback_queue_shortcut),
            ("new_subscription_videos", self.open_notification_center_shortcut),
            ("background_play_pause", self.background_play_pause_shortcut),
        ]
        for action, handler in actions:
            if self.shortcut_matches(event, action) and self.shortcut_allowed_for_focus(action, focus):
                handler()
                return True
        return False

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

    @classmethod
    def is_function_key_event(cls, event: wx.KeyEvent, number: int) -> bool:
        if not 1 <= number <= 24:
            return False
        target = wx.WXK_F1 + number - 1
        raw_target = 0x70 + number - 1
        return cls.event_key_code(event) == target or cls.event_raw_key_code(event) == raw_target

    def player_details_shortcut_matches(self, event: wx.KeyEvent) -> bool:
        if self.shortcut_matches(event, "player_details"):
            return True
        return (
            not event.ControlDown()
            and not event.ShiftDown()
            and not event.AltDown()
            and self.is_function_key_event(event, 7)
        )

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

    def handle_player_shortcut_event(self, event: wx.KeyEvent, focus: wx.Window | None, details_has_focus: bool = False) -> bool:
        if not (self.player_control_mode and self.player_shortcuts_allowed(focus)):
            return False
        if self.context_menu_shortcut_matches(event):
            self.open_player_context_menu()
            return True
        if focus is getattr(self, "repeat_checkbox", None) and self.shortcut_matches(event, "player_play_pause"):
            event.Skip()
            return True
        if focus is getattr(self, "bass_boost_checkbox", None) and self.shortcut_matches(event, "player_play_pause"):
            event.Skip()
            return True
        if details_has_focus and self.details_text_navigation_key(event):
            event.Skip()
            return True
        if self.shortcut_matches(event, "player_output_devices"):
            self.show_output_devices()
            return True
        if self.shortcut_matches(event, "player_copy_link"):
            self.copy_current_player_url()
            return True
        if self.shortcut_matches(event, "open_channel"):
            self.open_item_channel(self.current_video_item or self.current_video_info)
            return True
        if self.shortcut_matches(event, "player_equalizer"):
            self.show_player_equalizer()
            return True
        if self.shortcut_matches(event, "player_edit_mode"):
            self.toggle_edit_mode()
            return True
        if self.shortcut_matches(event, "player_save_edit_copy"):
            self.save_edited_local_file(replace_original=False)
            return True
        if self.shortcut_matches(event, "player_replace_edit_original"):
            self.save_edited_local_file(replace_original=True)
            return True
        if self.shortcut_matches(event, "player_marker_start"):
            self.set_clip_marker_async("start")
            return True
        if self.shortcut_matches(event, "player_marker_end"):
            self.set_clip_marker_async("end")
            return True
        if self.shortcut_matches(event, "player_previous"):
            self.play_relative_item(-1)
            return True
        if self.shortcut_matches(event, "player_next"):
            self.play_relative_item(1)
            return True
        if self.shortcut_matches(event, "player_volume_boost"):
            self.toggle_volume_boost()
            return True
        if self.shortcut_matches(event, "player_bass_boost"):
            self.toggle_bass_boost()
            return True
        if self.shortcut_matches(event, "player_repeat"):
            self.toggle_repeat()
            return True
        if self.shortcut_matches(event, "player_shuffle"):
            self.toggle_shuffle()
            return True
        if self.shortcut_matches(event, "player_play_pause"):
            self.player_play_pause()
            return True
        if self.shortcut_matches(event, "player_time"):
            self.announce_time_async()
            return True
        if self.shortcut_matches(event, "player_speed_down"):
            self.change_speed_async(-self.speed_step_value())
            return True
        if self.shortcut_matches(event, "player_speed_up"):
            self.change_speed_async(self.speed_step_value())
            return True
        if self.shortcut_matches(event, "player_pitch_up"):
            self.change_pitch_async(self.pitch_step_value())
            return True
        if self.shortcut_matches(event, "player_pitch_down"):
            self.change_pitch_async(-self.pitch_step_value())
            return True
        if self.player_details_shortcut_matches(event):
            self.show_video_details()
            return True
        if self.shortcut_matches(event, "player_volume_status"):
            self.announce_volume_async()
            return True
        if self.shortcut_matches(event, "player_seek_back_huge"):
            self.player_seek(-600)
            return True
        if self.shortcut_matches(event, "player_seek_forward_huge"):
            self.player_seek(600)
            return True
        if self.shortcut_matches(event, "player_seek_back_large"):
            self.player_seek(-60)
            return True
        if self.shortcut_matches(event, "player_seek_forward_large"):
            self.player_seek(60)
            return True
        if self.shortcut_matches(event, "player_seek_back"):
            self.player_seek(-5)
            return True
        if self.shortcut_matches(event, "player_seek_forward"):
            self.player_seek(5)
            return True
        if self.shortcut_matches(event, "player_volume_up"):
            self.change_volume_async(self.settings.volume_step)
            return True
        if self.shortcut_matches(event, "player_volume_down"):
            self.change_volume_async(-self.settings.volume_step)
            return True
        return False

    def on_char_hook(self, event: wx.KeyEvent) -> None:
        focus = wx.Window.FindFocus()
        details_has_focus = focus is self.video_details
        if self.is_modifier_only_event(event):
            return
        if self.is_shortcut_capture_control(focus):
            self.on_shortcut_capture_key(event, focus)
            return
        if self.handle_background_player_tab_navigation(event, focus):
            return
        if self.handle_player_tab_navigation(event, focus):
            return
        if self.in_main_menu:
            if self.handle_player_shortcut_event(event, focus, details_has_focus):
                return
            if self.shortcut_matches(event, "open_channel"):
                self.open_item_channel()
                return
            if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "menu_list", None):
                self.activate_menu()
                return
            if self.handle_global_navigation_shortcut(event, focus):
                return
            event.Skip()
            return
        if self.handle_global_navigation_shortcut(event, focus):
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
            self.toggle_download_queue()
            return
        if focus is getattr(self, "results_list", None) and self.result_details_key(event):
            self.announce_selected_result_details()
            return
        if self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
            return
        if self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "results_list", None):
            self.play_selected()
            return
        if focus is getattr(self, "results_list", None) and self.shortcut_matches(event, "copy_link"):
            self.copy_selected_url()
            return
        if focus is getattr(self, "results_list", None) and self.shortcut_matches(event, "add_favorite"):
            self.add_selected_favorite()
            return
        if focus is getattr(self, "results_list", None) and self.shortcut_matches(event, "remove_favorite"):
            self.remove_selected_favorite_shortcut()
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
        if self.shortcut_matches(event, "unsubscribe_channel"):
            self.unsubscribe_shortcut()
            return
        if self.shortcut_matches(event, "open_channel"):
            self.open_item_channel()
            return
        if self.shortcut_matches(event, "create_playlist"):
            self.create_user_playlist_dialog()
            return
        if self.shortcut_matches(event, "add_to_playlist"):
            prefer_player_item = bool(self.current_video_item and (self.in_player_screen or self.focus_in_background_player_controls(focus)))
            self.add_active_to_playlist(prefer_active=prefer_player_item)
            return
        if self.shortcut_matches(event, "remove_from_playlist"):
            self.remove_active_from_playlist()
            return
        if self.shortcut_matches(event, "player_back"):
            if self.in_player_screen and self.video_details_visible():
                self.hide_video_details()
                return
            if self.in_player_screen:
                if self.player_fullscreen_mode_active() or self.IsFullScreen():
                    self.exit_fullscreen_to_player()
                    return
                if self.player_escape_closes_playback(focus):
                    if self.IsFullScreen():
                        self.ShowFullScreen(False)
                    self.leave_player_to_previous_screen()
                else:
                    self.leave_player_to_main_menu(force_keep_playing=True)
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
        if self.shortcut_matches(event, "copy_stream_url"):
            self.copy_direct_stream_url()
            return
        if self.player_details_shortcut_matches(event) and (self.in_player_screen or self.focus_in_background_player_controls(focus)):
            self.show_video_details()
            return
        if self.in_player_screen and focus is getattr(self, "results_list", None):
            event.Skip()
            wx.CallAfter(self.maybe_extend_results)
            return
        if self.handle_player_shortcut_event(event, focus, details_has_focus):
            return
        event.Skip()

    def open_player_context_menu(self, _event=None) -> None:
        item = self.current_video_item or self.current_video_info or {}
        menu = wx.Menu()
        actions = []
        if item.get("kind") != "local_file":
            actions.extend(
                [
                    (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.start_download(True, item=dict(item))),
                    (self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.start_download(False, item=dict(item))),
                ]
            )
        actions.extend([
            (self.menu_label_with_shortcut("add_favorite", "add_favorite"), lambda: self.add_favorite_item(dict(item))),
            (self.menu_label_with_shortcut("remove_favorite", "remove_favorite"), lambda: self.remove_favorite_item(dict(item))),
            (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), lambda: self.subscribe_to_selected_channel(dict(item))),
            (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), lambda: self.unsubscribe_from_selected_channel(dict(item))),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playlist", "remove_from_playlist"), self.remove_active_from_playlist),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda: self.copy_direct_stream_url(dict(item))),
            (self.menu_label_with_shortcut("copy_url", "copy_link"), self.copy_current_player_url),
            (self.t("output_devices"), self.show_output_devices),
            (self.t("equalizer"), self.show_player_equalizer),
            (self.t("close_player"), self.close_current_player),
        ])
        if self.item_has_openable_youtube_channel(item):
            actions.insert(6, (self.menu_label_with_shortcut("open_channel", "open_channel"), lambda: self.open_item_channel(dict(item))))
        if item.get("kind") != "local_file":
            actions.insert(-5, (self.t("open_browser"), lambda: webbrowser.open(str(item.get("webpage_url") or item.get("url") or ""))))
        for label, handler in actions:
            menu_item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), menu_item)
        if self.playlist_item_is_supported(item):
            self.append_add_to_playlist_menu(menu, prefer_active=True)
        self.PopupMenu(menu)
        menu.Destroy()

    def open_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        item = self.selected_result()
        if item and item.get("kind") in {"playlist", "channel"}:
            is_channel = item.get("kind") == "channel"
            if is_channel:
                actions = [
                    (self.t("channel_options"), lambda selected=dict(item): self.show_channel_options(selected)),
                    (self.t("channel_videos"), lambda selected=dict(item): self.open_channel_tab(selected, "videos")),
                    (self.t("channel_popular"), lambda selected=dict(item): self.open_channel_tab(selected, "popular")),
                    (self.t("channel_playlists"), lambda selected=dict(item): self.open_channel_tab(selected, "playlists")),
                    (self.t("download_channel"), lambda selected=dict(item): self.download_collection(selected)),
                    (self.menu_label_with_shortcut("add_favorite", "add_favorite"), self.add_selected_favorite),
                    (self.menu_label_with_shortcut("remove_favorite", "remove_favorite"), self.remove_selected_favorite_shortcut),
                    (self.t("open_browser"), self.open_selected_in_browser),
                    (self.menu_label_with_shortcut("copy_url", "copy_link"), self.copy_selected_url),
                ]
            else:
                actions = [
                    (self.t("open_playlist_videos"), self.play_selected),
                    (self.t("download_playlist"), lambda selected=dict(item): self.download_collection(selected)),
                    (self.menu_label_with_shortcut("add_favorite", "add_favorite"), self.add_selected_favorite),
                    (self.menu_label_with_shortcut("remove_favorite", "remove_favorite"), self.remove_selected_favorite_shortcut),
                    (self.t("open_browser"), self.open_selected_in_browser),
                    (self.menu_label_with_shortcut("copy_url", "copy_link"), self.copy_selected_url),
                ]
            if is_channel:
                actions.insert(5, (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), self.subscribe_shortcut))
                actions.insert(6, (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), self.unsubscribe_shortcut))
        else:
            actions = [
                (self.t("play"), self.play_selected),
                (self.menu_label_with_shortcut("download_audio", "download_audio"), self.download_audio),
                (self.menu_label_with_shortcut("download_video", "download_video"), self.download_video),
                (self.menu_label_with_shortcut("add_favorite", "add_favorite"), self.add_selected_favorite),
                (self.menu_label_with_shortcut("remove_favorite", "remove_favorite"), self.remove_selected_favorite_shortcut),
                (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), self.subscribe_shortcut),
                (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), self.unsubscribe_shortcut),
                (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
                (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
                (self.menu_label_with_shortcut("remove_from_playlist", "remove_from_playlist"), self.remove_active_from_playlist),
                (self.t("open_browser"), self.open_selected_in_browser),
                (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda selected=dict(item or {}): self.copy_direct_stream_url(selected)),
                (self.menu_label_with_shortcut("copy_url", "copy_link"), self.copy_selected_url),
            ]
            if self.item_has_openable_youtube_channel(item):
                actions.insert(7, (self.menu_label_with_shortcut("open_channel", "open_channel"), lambda selected=dict(item or {}): self.open_item_channel(selected)))
        if len(self.download_queue) > 1:
            actions[1:1] = [
                (self.t("download_all_as_audio"), lambda: self.download_all_queued(True)),
                (self.t("download_all_as_video"), lambda: self.download_all_queued(False)),
            ]
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

    def default_download_filename(self, item: dict, audio_only: bool) -> str:
        title = self.safe_folder_name(str(item.get("title") or "download"))
        extension = self.settings.audio_format if audio_only else "mp4"
        return f"{title}.{extension}"

    def choose_download_target_path(self, item: dict, audio_only: bool) -> Path | None:
        folder = self.download_folder_for_item(item, audio_only)
        folder.mkdir(parents=True, exist_ok=True)
        if audio_only:
            wildcard = f"{self.settings.audio_format.upper()} (*.{self.settings.audio_format})|*.{self.settings.audio_format}|{self.t('all_files')} (*.*)|*.*"
        else:
            wildcard = f"MP4 (*.mp4)|*.mp4|{self.t('all_files')} (*.*)|*.*"
        with wx.FileDialog(
            self,
            self.t("choose_save_path"),
            defaultDir=str(folder),
            defaultFile=self.default_download_filename(item, audio_only),
            wildcard=wildcard,
            style=wx.FD_SAVE | wx.FD_OVERWRITE_PROMPT,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return None
            path = Path(dialog.GetPath()).expanduser()
        expected_suffix = f".{self.settings.audio_format}" if audio_only else ".mp4"
        if not path.suffix:
            path = path.with_suffix(expected_suffix)
        return path

    def choose_download_target_folder(self, item: dict, collection: bool = False) -> Path | None:
        folder = self.download_folder_for_item(item, True, collection=collection)
        folder.mkdir(parents=True, exist_ok=True)
        with wx.DirDialog(self, self.t("choose_save_folder"), defaultPath=str(folder), style=wx.DD_DEFAULT_STYLE | wx.DD_DIR_MUST_EXIST) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return None
            return Path(dialog.GetPath()).expanduser()

    def start_download(self, audio_only: bool, item: dict | None = None, remove_queued: bool = False) -> None:
        item = item or self.active_item()
        if not item:
            self.message(self.t("no_selection"))
            return
        if self.in_player_screen and self.clip_markers_are_set():
            self.export_marked_clip(audio_only=audio_only)
            return
        if self.in_player_screen and self.clip_markers_partially_set():
            self.announce_player(self.t("clip_markers_missing"))
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
        target_path = self.choose_download_target_path(item, audio_only) if self.settings.ask_download_location_each_time else None
        if self.settings.ask_download_location_each_time and target_path is None:
            self.set_status(self.t("download_cancelled"))
            return
        self.announce_player(self.t("download_started"))
        self.set_status(self.t("download_audio_start" if audio_only else "download_video_start"))
        task_id, cancel_event = self.register_download_task(item, audio_only, "single", total=1)
        wx.CallLater(900, self.start_download_worker_thread, item, audio_only, task_id, cancel_event, target_path)

    def start_download_worker_thread(self, item: dict, audio_only: bool, task_id: str, cancel_event: threading.Event, target_path: Path | None = None) -> None:
        threading.Thread(target=self.download_worker, args=(item, audio_only, task_id, cancel_event, target_path), daemon=True).start()

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
        target_folder = self.choose_download_target_folder(item, collection=True) if self.settings.ask_download_location_each_time else None
        if self.settings.ask_download_location_each_time and target_folder is None:
            self.set_status(self.t("download_cancelled"))
            return
        start_key = "download_channel_start" if kind == "channel" else "download_playlist_start"
        self.announce_player(self.t("download_started"))
        self.set_status(self.t(start_key))
        task_id, cancel_event = self.register_download_task(item, audio_only, kind, total=0)
        threading.Thread(target=self.download_collection_worker, args=(dict(item), audio_only, task_id, cancel_event, target_folder), daemon=True).start()

    def download_audio_shortcut(self) -> None:
        self.start_download_shortcut(True)

    def download_video_shortcut(self) -> None:
        self.start_download_shortcut(False)

    def start_download_shortcut(self, audio_only: bool) -> None:
        if self.in_main_menu:
            return
        item = self.active_item()
        url = str(item.get("url", "")) if item else ""
        kind = "audio" if audio_only else "video"
        now = time.monotonic()
        last_kind, last_url, last_time = self.last_download_shortcut
        if kind == last_kind and url == last_url and now - last_time < 0.35:
            return
        self.last_download_shortcut = (kind, url, now)
        self.start_download(audio_only, item=item)

    def download_worker(self, item: dict, audio_only: bool, task_id: str, cancel_event: threading.Event, target_path: Path | None = None) -> None:
        try:
            folder = target_path.parent if target_path else self.download_folder_for_item(item, audio_only)
            folder.mkdir(parents=True, exist_ok=True)
            options = self.download_options(folder, audio_only, item["title"], task_id=task_id, cancel_event=cancel_event, target_path=target_path)
            self.ydl_download_urls([item["url"]], options)
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

    def download_collection_worker(self, item: dict, audio_only: bool, task_id: str, cancel_event: threading.Event, target_folder: Path | None = None) -> None:
        try:
            kind = str(item.get("kind") or "playlist")
            title = item.get("title") or self.t("channel" if kind == "channel" else "playlist")
            folder = target_folder or self.download_folder_for_item(item, audio_only, collection=True)
            folder.mkdir(parents=True, exist_ok=True)
            options = self.download_options(folder, audio_only, title, allow_playlist=True, task_id=task_id, cancel_event=cancel_event)
            self.ydl_download_urls([self.collection_download_url(item)], options)
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
        target_path: Path | None = None,
    ) -> dict:
        progress_hook = self.make_download_progress_hook(title, audio_only, task_id=task_id, cancel_event=cancel_event)
        template = self.settings.filename_template or DEFAULT_FILENAME_TEMPLATE
        if allow_playlist and self.settings.keep_playlist_order and "%(playlist_index)" not in template:
            template = "%(playlist_index)s - " + template
        if target_path and audio_only:
            outtmpl = str(target_path.with_suffix("")) + ".%(ext)s"
        else:
            outtmpl = str(target_path) if target_path else str(folder / template)
        options = {
            "outtmpl": outtmpl,
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
        for key, value in (("ratelimit", self.settings.rate_limit), ("proxy", self.settings.proxy), ("ffmpeg_location", self.settings.ffmpeg_location)):
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
            options["concurrent_fragment_downloads"] = max(self.settings.concurrent_fragments, VIDEO_DOWNLOAD_MIN_FRAGMENTS)
            options["http_chunk_size"] = VIDEO_DOWNLOAD_HTTP_CHUNK_SIZE
            options["buffersize"] = VIDEO_DOWNLOAD_BUFFER_SIZE
            options["progress_delta"] = 0.5
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
            return f"best[ext=mp4][vcodec!=none][acodec!=none]{limit}/best[ext=mp4][vcodec!=none][acodec!=none]/best{limit}/best"
        if video_mode == VIDEO_FORMAT_SMALLEST:
            return f"worst[ext=mp4][vcodec!=none][acodec!=none]{limit}/worst[ext=mp4][vcodec!=none][acodec!=none]/worst{limit}/worst"
        return f"best[ext=mp4][vcodec!=none][acodec!=none]{limit}/best[ext=mp4][vcodec!=none][acodec!=none]/bestvideo[ext=mp4]{limit}+bestaudio[ext=m4a]/bestvideo{limit}+bestaudio/best{limit}/best"

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
            self.announce_player(self.t("favorite_exists"))
            return
        self.favorites.append(favorite)
        self.save_favorites()
        self.announce_player(self.t("favorite_added"))

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
            self.announce_player(self.t("favorite_removed"))

    def remove_selected_favorite_shortcut(self) -> None:
        self.remove_favorite_item(self.active_item())

    def remove_favorite_item(self, item: dict | None) -> None:
        if not item:
            self.announce_player(self.t("no_selection"))
            return
        url = str(item.get("url") or "")
        if not url:
            self.announce_player(self.t("not_in_favorites"))
            return
        for index, favorite in enumerate(list(self.favorites)):
            if str(favorite.get("url") or "") == url:
                del self.favorites[index]
                self.save_favorites()
                if self.favorites_screen_active:
                    self.refresh_favorites()
                self.announce_player(self.t("favorite_removed"))
                return
        self.announce_player(self.t("not_in_favorites"))

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

    def toggle_download_queue(self, audio_only: bool | None = None) -> None:
        item = self.selected_result()
        if not item:
            self.message(self.t("no_selection"))
            return
        url = item.get("url", "")
        if not url:
            self.message(self.t("no_selection"))
            return
        existing = self.download_queue.get(url)
        existing_audio_only = existing.get("audio_only") if existing and "audio_only" in existing else None
        if existing and existing_audio_only == audio_only:
            self.download_queue.pop(url, None)
            self.announce_player(self.t("download_deselected", title=item.get("title", "")))
        else:
            queued = dict(item)
            queued["audio_only"] = audio_only
            self.download_queue[url] = queued
            if audio_only is None:
                key = "selected_for_download_or_playlist"
            elif item.get("kind") in {"playlist", "channel"}:
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
        if self.result_line_update_should_defer(index):
            self.deferred_result_line_updates.add(index)
            return
        self.refresh_result_line_now(index)

    def result_line_update_should_defer(self, index: int) -> bool:
        if not hasattr(self, "results_list") or index < 0:
            return False
        try:
            return wx.Window.FindFocus() is self.results_list and self.results_list.GetSelection() == index
        except RuntimeError:
            return False

    def apply_deferred_result_line_updates(self, exclude_index: int | None = None) -> None:
        if not self.deferred_result_line_updates:
            return
        for index in sorted(list(self.deferred_result_line_updates)):
            if exclude_index is not None and index == exclude_index:
                continue
            self.deferred_result_line_updates.discard(index)
            self.refresh_result_line_now(index)

    def refresh_result_line_now(self, index: int) -> None:
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
            if item.get("kind") == "rss_item":
                audio_only = True
            elif isinstance(item.get("audio_only"), bool):
                audio_only = bool(item.get("audio_only"))
            else:
                selected_format = self.choose_queue_download_format()
                if selected_format is None:
                    return
                audio_only = selected_format
        elif item.get("kind") == "rss_item":
            audio_only = True
        if item.get("kind") in {"playlist", "channel"}:
            self.download_collection(dict(item), audio_only=audio_only, remove_queued=True)
        else:
            self.start_download(audio_only, item=dict(item), remove_queued=True)

    def choose_queue_download_format(self) -> bool | None:
        choices = [self.t("download_audio"), self.t("download_video")]
        with wx.SingleChoiceDialog(self, self.t("select_download_format_message"), self.t("select_download_format"), choices) as dialog:
            dialog.SetSelection(0)
            if dialog.ShowModal() != wx.ID_OK:
                return None
            return dialog.GetSelection() == 0

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

    def download_all_queued(self, audio_only: bool | None = None) -> None:
        if not self.download_queue:
            self.announce_player(self.t("download_queue_empty"))
            return
        if audio_only is None and any(not isinstance(item.get("audio_only"), bool) and item.get("kind") != "rss_item" for item in self.download_queue.values()):
            audio_only = self.choose_queue_download_format()
            if audio_only is None:
                return
        items = []
        for queued_item in self.download_queue.values():
            item = dict(queued_item)
            if item.get("kind") == "rss_item":
                item["audio_only"] = True
            elif audio_only is not None:
                item["audio_only"] = audio_only
            elif not isinstance(item.get("audio_only"), bool):
                item["audio_only"] = True
            items.append(item)
        batch_folder = None
        if self.settings.ask_download_location_each_time:
            batch_folder = self.choose_download_target_folder({"title": self.t("download_all_selected"), "kind": "batch"}, collection=False)
            if batch_folder is None:
                self.set_status(self.t("download_cancelled"))
                return
            for item in items:
                item["download_folder_override"] = str(batch_folder)
        self.download_queue.clear()
        self.refresh_results_list_labels()
        if self.rss_items_screen_active:
            self.refresh_rss_items_list()
        self.announce_player(self.t("batch_download_start", count=len(items)))
        if audio_only is True:
            batch_title = self.t("download_all_as_audio")
        elif audio_only is False:
            batch_title = self.t("download_all_as_video")
        else:
            batch_title = self.t("download_all_selected")
        task_id, cancel_event = self.register_download_task({"title": batch_title, "kind": "batch"}, False, "batch", total=len(items))
        if self.in_queue_screen:
            self.show_download_queue()
        threading.Thread(target=self.download_batch_worker, args=(items, task_id, cancel_event, None, str(batch_folder) if batch_folder else None), daemon=True).start()

    def refresh_results_list_labels(self) -> None:
        if not hasattr(self, "results_list"):
            return
        try:
            selection = self.results_list.GetSelection()
            labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
            if not labels:
                labels = [self.t("no_results")]
            if wx.Window.FindFocus() is self.results_list and self.results_list.GetCount() == len(labels):
                self.results_list.Freeze()
                try:
                    for index, label in enumerate(labels):
                        if index == selection:
                            if self.results_list.GetString(index) != label:
                                self.deferred_result_line_updates.add(index)
                            continue
                        if self.results_list.GetString(index) != label:
                            self.results_list.SetString(index, label)
                finally:
                    self.results_list.Thaw()
                return
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
                    if override_folder:
                        title = item.get("title") or self.t("channel" if item.get("kind") == "channel" else "playlist")
                        item_folder = Path(override_folder) / self.safe_folder_name(str(title))
                    else:
                        item_folder = self.download_folder_for_item(item, audio_only, collection=True)
                    allow_playlist = True
                    url = self.collection_download_url(item)
                item_folder.mkdir(parents=True, exist_ok=True)
                last_item_folder = item_folder
                options = self.download_options(item_folder, audio_only, item.get("title", ""), allow_playlist=allow_playlist, task_id=task_id, cancel_event=cancel_event)
                self.ydl_download_urls([url], options)
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
        self.cookie_repair_suppressed_until = 0.0
        try:
            if CACHED_COOKIES_FILE.exists():
                CACHED_COOKIES_FILE.unlink()
        except OSError:
            pass
        self.save_settings()
        self.sync_windows_startup_registration(show_error=True)
        self.configure_subscription_timer()
        self.configure_rss_timer()
        self.configure_app_update_timer()
        self.set_status(self.t("defaults_restored"))
        self.speak_text(self.t("defaults_restored"))
        self.show_settings()

    def reset_settings_section(self, section_name: str) -> None:
        section_fields = self.settings_section_fields().get(section_name, [])
        if not section_fields:
            return
        defaults = asdict(Settings())
        for key in section_fields:
            if key in defaults:
                setattr(self.settings, key, defaults[key])
        if section_name == "cookies":
            self.cookie_repair_suppressed_until = 0.0
            try:
                if CACHED_COOKIES_FILE.exists():
                    CACHED_COOKIES_FILE.unlink()
            except OSError:
                pass
        self.save_settings()
        self.sync_windows_startup_registration(show_error=True)
        self.configure_subscription_timer()
        self.configure_rss_timer()
        self.configure_app_update_timer()
        if section_name == "shortcuts":
            self.shortcut_editor_values = dict(DEFAULT_KEYBOARD_SHORTCUTS)
        if section_name == "equalizer":
            self.visible_equalizer_preset = EQ_PRESET_FLAT
            if self.player_is_active() and self.session_equalizer_enabled is None:
                self.apply_equalizer_to_player()
        text = self.t("section_settings_reset", section=self.settings_section_label(section_name))
        self.set_status(text)
        self.speak_text(text)
        self.render_settings_section_and_focus()

    def choose_download_folder(self) -> None:
        with wx.DirDialog(self, self.t("choose_download_folder"), self.settings.download_folder) as dialog:
            if dialog.ShowModal() == wx.ID_OK:
                self.settings.download_folder = dialog.GetPath()
                if hasattr(self, "controls") and "download_folder" in self.controls:
                    self.controls["download_folder"].SetValue(self.settings.download_folder)
                self.save_settings()
                self.set_status(self.t("settings_saved"))

    def open_windows_default_apps_settings(self) -> None:
        try:
            if os.name == "nt" and not self.media_association_registry_complete():
                self.register_media_associations_current_user()
                self.set_status(self.t("media_association_registered"))
            os.startfile("ms-settings:defaultapps")  # type: ignore[attr-defined]
            self.announce_player(self.t("default_player_settings_opened"))
        except Exception as exc:
            try:
                subprocess.Popen(["control.exe", "/name", "Microsoft.DefaultPrograms"])
                self.announce_player(self.t("default_player_settings_opened"))
            except Exception:
                self.message(self.t("default_player_settings_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def choose_cookies_file(self) -> None:
        with wx.FileDialog(
            self,
            self.t("choose_cookies_file"),
            wildcard="cookies.txt (*.txt)|*.txt|All files (*.*)|*.*",
            style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            path = dialog.GetPath()
        try:
            result = self.import_cookie_file_to_cache(path)
        except Exception as exc:
            self.message(self.t("cookies_file_load_failed", error=self.friendly_error(exc)), wx.ICON_WARNING)
            return
        imported_path = str(result["path"])
        self.settings.cookies_file = imported_path
        self.settings.cookies_from_browser = "none"
        self.settings.cookies_browser_profile = COOKIE_PROFILE_AUTO
        self.cookie_repair_suppressed_until = 0.0
        self.save_settings()
        if hasattr(self, "controls"):
            if "cookies" in self.controls:
                self.controls["cookies"].SetValue(imported_path)
            if "cookies_from_browser" in self.controls:
                self.controls["cookies_from_browser"].SetSelection(0)
            if "cookies_browser_profile" in self.controls:
                self.controls["cookies_browser_profile"].SetSelection(0)
        message_key = {
            "json": "cookies_file_json_imported",
            "header": "cookies_file_header_imported",
            "netscape": "cookies_file_netscape_imported",
        }.get(str(result.get("kind") or ""), "cookies_file_imported")
        self.announce_player(self.t(message_key, path=imported_path))
        if result.get("has_login"):
            self.announce_player(self.t("cookies_file_login_found"))
        else:
            self.message(self.t("cookies_file_no_login_warning"), wx.ICON_WARNING)

    def export_browser_cookies_from_settings(self) -> None:
        if get_yt_dlp() is None:
            self.message(self.t("missing_ytdlp"), wx.ICON_ERROR)
            return
        self.apply_settings_from_visible_controls()
        browser = self.normalized_cookies_browser()
        if not browser:
            self.message(self.t("select_cookies_browser"))
            return
        if self.cookie_browser_is_running(browser):
            label = browser.title()
            with wx.MessageDialog(
                self,
                self.t("close_browser_for_cookie_export_message", browser=label),
                self.t("close_browser_for_cookie_export_title"),
                wx.YES_NO | wx.ICON_WARNING,
            ) as dialog:
                if dialog.ShowModal() != wx.ID_YES:
                    return
            if self.close_cookie_browser_processes(browser):
                self.announce_player(self.t("browser_closed_for_cookie_export"))
            self.wait_for_cookie_browser_exit(browser)
        self.announce_player(self.t("exporting_browser_cookies"))
        threading.Thread(target=self.export_browser_cookies_worker, args=(browser,), daemon=True).start()

    def open_youtube_login_profile_from_settings(self) -> None:
        self.apply_settings_from_visible_controls()
        browser = self.normalized_cookies_browser()
        if not browser:
            self.message(self.t("select_cookies_browser"))
            return
        try:
            if browser in CHROMIUM_COOKIE_BROWSERS:
                executable = self.cookie_browser_executable(browser)
                if not executable:
                    raise RuntimeError(f"{browser} executable not found")
                profile = str(getattr(self.settings, "cookies_browser_profile", COOKIE_PROFILE_AUTO) or COOKIE_PROFILE_AUTO)
                profile_dir = ""
                if profile and profile != COOKIE_PROFILE_AUTO:
                    if os.path.isabs(profile):
                        profile_path = Path(profile)
                        profile_dir = profile_path.name
                    else:
                        profile_dir = profile
                args = [executable]
                if profile_dir and browser != "opera":
                    args.append(f"--profile-directory={profile_dir}")
                args.append("https://www.youtube.com/")
                subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            else:
                webbrowser.open("https://www.youtube.com/")
            self.announce_player(self.t("youtube_profile_opened"))
        except Exception as exc:
            self.message(self.t("youtube_profile_open_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def cookie_browser_process_names(self, browser: str) -> list[str]:
        return COOKIES_BROWSER_PROCESS_NAMES.get(str(browser or "").lower(), [])

    def cookie_browser_is_running(self, browser: str) -> bool:
        if os.name != "nt":
            return False
        creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
        for name in self.cookie_browser_process_names(browser):
            try:
                result = subprocess.run(
                    ["tasklist", "/FI", f"IMAGENAME eq {name}.exe", "/NH"],
                    capture_output=True,
                    text=True,
                    timeout=2,
                    creationflags=creationflags,
                )
                if f"{name}.exe".lower() in (result.stdout or "").lower():
                    return True
            except Exception:
                continue
        return False

    def close_cookie_browser_processes(self, browser: str) -> bool:
        if os.name != "nt":
            return False
        creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
        attempted = False
        for name in self.cookie_browser_process_names(browser):
            attempted = True
            try:
                subprocess.run(
                    ["taskkill", "/IM", f"{name}.exe", "/T", "/F"],
                    capture_output=True,
                    text=True,
                    timeout=5,
                    creationflags=creationflags,
                )
            except Exception:
                continue
        if attempted:
            time.sleep(1.0)
        return attempted and not self.cookie_browser_is_running(browser)

    def export_browser_cookies_worker(self, browser: str) -> None:
        try:
            result = self.export_browser_cookies_blocking(browser, allow_close=True)
            wx.CallAfter(self.finish_browser_cookies_export, str(result["path"]), str(result["profile_label"]), browser)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("browser_cookies_export_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def finish_browser_cookies_export(self, path: str, profile_label: str, browser: str) -> None:
        self.settings.cookies_file = path
        self.settings.cookies_from_browser = browser
        self.cookie_repair_suppressed_until = 0.0
        self.save_settings()
        if hasattr(self, "controls"):
            if "cookies" in self.controls:
                self.controls["cookies"].SetValue(path)
            if "cookies_from_browser" in self.controls:
                selection = COOKIES_BROWSER_OPTIONS.index(browser) if browser in COOKIES_BROWSER_OPTIONS else 0
                self.controls["cookies_from_browser"].SetSelection(selection)
        self.announce_player(self.t("browser_cookies_exported", path=path, profile=profile_label))

    def save_settings_from_ui(self) -> None:
        old_language = self.settings.language
        if not self.validate_shortcut_controls():
            return
        self.apply_settings_from_visible_controls()
        self.save_settings()
        self.sync_windows_startup_registration(show_error=True)
        self.trim_history()
        self.configure_subscription_timer()
        self.configure_rss_timer()
        self.configure_app_update_timer()
        self.install_download_accelerators()
        if self.player_is_active() and self.session_equalizer_enabled is None:
            self.apply_equalizer_to_player()
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
        if "default_volume" in c:
            self.settings.default_volume = self.to_int(str(c["default_volume"].GetValue()), 100, 0, 300)
        if "volume_boost_by_default" in c:
            self.settings.volume_boost_by_default = c["volume_boost_by_default"].GetValue()
        if "speed_step" in c:
            self.settings.speed_step = self.to_float(c["speed_step"].GetStringSelection(), 0.01, 0.01, 0.25)
        if "pitch_step" in c:
            self.settings.pitch_step = self.to_float(c["pitch_step"].GetStringSelection(), 0.01, 0.01, 0.25)
        if "auto_update" in c:
            self.settings.auto_update_ytdlp = c["auto_update"].GetValue()
        if "auto_update_app" in c:
            self.settings.auto_update_app = c["auto_update_app"].GetValue()
        if "app_update_interval" in c:
            self.settings.app_update_interval_hours = self.to_float(self.selected_choice_value("app_update_interval"), 6.0, 0.5, 24.0)
        if "close_to_tray" in c:
            self.settings.close_to_tray = c["close_to_tray"].GetValue()
        if "start_with_windows" in c:
            self.settings.start_with_windows = c["start_with_windows"].GetValue()
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
        if "ask_download_location_each_time" in c:
            self.settings.ask_download_location_each_time = c["ask_download_location_each_time"].GetValue()
        if "audio_format" in c:
            self.settings.audio_format = c["audio_format"].GetStringSelection() or "mp3"
        if "audio_quality" in c:
            self.settings.audio_quality = self.normalize_audio_quality_value(self.selected_choice_value("audio_quality"))
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
        if "global_equalizer" in c:
            self.settings.global_equalizer_enabled = c["global_equalizer"].GetValue()
        selected_equalizer_preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        if "equalizer_preset" in c:
            selected_equalizer_preset = self.normalized_equalizer_preset(self.selected_choice_value("equalizer_preset"))
            self.settings.global_equalizer_preset = selected_equalizer_preset
        if "equalizer_preset_name" in c and self.is_custom_equalizer_preset(selected_equalizer_preset):
            names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
            names[selected_equalizer_preset] = c["equalizer_preset_name"].GetValue().strip()[:80] or self.equalizer_custom_name(selected_equalizer_preset)
            self.settings.equalizer_custom_names = names
        if "equalizer_db_range" in c:
            self.settings.equalizer_db_range = self.to_int(self.selected_choice_value("equalizer_db_range"), 12, 6, 24)
        eq_gains: dict[str, float] = {}
        for band_id, _band_label in EQ_BANDS:
            ctrl = c.get(f"eq_{band_id}")
            if isinstance(ctrl, wx.Slider):
                eq_gains[band_id] = round(float(ctrl.GetValue()) / 10.0, 1)
        if eq_gains:
            eq_gains = self.normalized_equalizer_gains(eq_gains)
            presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
            presets[selected_equalizer_preset] = eq_gains
            self.settings.equalizer_preset_gains = presets
            self.settings.global_equalizer_gains = eq_gains
        if "show_video_details_by_default" in c:
            self.settings.show_video_details_by_default = c["show_video_details_by_default"].GetValue()
        if "enable_age_restricted_videos" in c:
            self.settings.enable_age_restricted_videos = c["enable_age_restricted_videos"].GetValue()
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
        if "announce_play_pause" in c:
            self.settings.announce_play_pause = c["announce_play_pause"].GetValue()
        if "announce_playback_finished" in c:
            self.settings.announce_playback_finished = c["announce_playback_finished"].GetValue()
        if "enable_background_playback" in c:
            self.settings.enable_background_playback = c["enable_background_playback"].GetValue()
        if "rate_limit" in c:
            self.settings.rate_limit = c["rate_limit"].GetValue()
        if "proxy" in c:
            self.settings.proxy = c["proxy"].GetValue()
        if "youtube_data_api_key" in c:
            self.settings.youtube_data_api_key = c["youtube_data_api_key"].GetValue().strip()
        if "cookies" in c:
            self.settings.cookies_file = c["cookies"].GetValue()
        if "cookies_from_browser" in c:
            self.settings.cookies_from_browser = c["cookies_from_browser"].GetStringSelection() or "none"
        if "cookies_browser_profile" in c:
            self.settings.cookies_browser_profile = self.selected_choice_value("cookies_browser_profile") or COOKIE_PROFILE_AUTO
        if "show_advanced_network_settings" in c:
            self.settings.show_advanced_network_settings = c["show_advanced_network_settings"].GetValue()
        if "cookie_user_agent" in c:
            self.settings.cookie_user_agent = c["cookie_user_agent"].GetValue().strip()
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
        if "enable_trending" in c:
            self.settings.enable_trending = c["enable_trending"].GetValue()
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
            self.settings.rss_refresh_interval_hours = self.to_float(self.selected_choice_value("rss_refresh_interval"), 12.0, 0.5, 168.0)
        if "subscription_check_enabled" in c:
            self.settings.subscription_check_enabled = c["subscription_check_enabled"].GetValue()
        if "subscription_check_interval" in c:
            self.settings.subscription_check_interval_hours = self.to_float(self.selected_choice_value("subscription_check_interval"), 6.0, 0.5, 168.0)
        if "windows_notifications" in c:
            self.settings.windows_notifications = c["windows_notifications"].GetValue()
        if "download_notifications" in c:
            self.settings.download_notifications = c["download_notifications"].GetValue()
        if "subscription_notifications" in c:
            self.settings.subscription_notifications = c["subscription_notifications"].GetValue()
        if "app_update_notifications" in c:
            self.settings.app_update_notifications = c["app_update_notifications"].GetValue()
        shortcuts = dict(getattr(self.settings, "keyboard_shortcuts", {}) or {})
        if "shortcut_action_list" in c and "shortcut_active_value" in c:
            self.sync_shortcut_editor_value()
            shortcuts.update(self.shortcut_editor_values)
        else:
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
        latest_version, wheel_url, wheel_sha256 = self.fetch_latest_ytdlp_wheel()
        if not self.is_component_version_newer(latest_version, current_version):
            return False
        if not wheel_url:
            raise RuntimeError("yt-dlp wheel URL is empty")
        self.validate_trusted_download_url(wheel_url, {"files.pythonhosted.org", "pypi.org", "pypi.python.org"})
        self.ui_queue.put(("announce", self.t("components_updating")))
        COMPONENTS_DIR.mkdir(parents=True, exist_ok=True)
        temp_dir = Path(tempfile.mkdtemp(prefix="apricotplayer-ytdlp-"))
        wheel_path = temp_dir / "yt_dlp.whl"
        extract_dir = temp_dir / "extract"
        try:
            request = Request(wheel_url, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
            with self.open_url(request, timeout=120) as response, wheel_path.open("wb") as handle:
                self.validate_https_response_url(response.geturl())
                shutil.copyfileobj(response, handle)
            if wheel_sha256:
                self.verify_file_sha256(wheel_path, wheel_sha256)
            extract_dir.mkdir(parents=True, exist_ok=True)
            with zipfile.ZipFile(wheel_path) as archive:
                self.safe_extract_zip(archive, extract_dir)
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

    def fetch_latest_ytdlp_wheel(self) -> tuple[str, str, str]:
        request = Request(YTDLP_PYPI_JSON_URL, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
        latest_version = str((payload.get("info") or {}).get("version") or "")
        urls = payload.get("urls") or []
        for item in urls:
            filename = str(item.get("filename") or "")
            if filename.endswith(".whl") and str(item.get("packagetype") or "") == "bdist_wheel":
                digest = str(((item.get("digests") or {}).get("sha256")) or "")
                return latest_version, str(item.get("url") or ""), digest
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

    def start_app_update_check(self, manual: bool = False, prompt: bool = True, notify: bool = False) -> None:
        if not manual and not self.settings.auto_update_app:
            self.set_status(self.t("app_update_disabled"))
            return
        if self.app_update_check_running:
            if manual:
                self.announce_player(self.t("checking_app_updates"))
            return
        self.app_update_check_running = True
        self.set_status(self.t("checking_app_updates"))
        if manual:
            self.announce_player(self.t("checking_app_updates"))
        threading.Thread(target=self.app_update_worker, args=(manual, prompt, notify), daemon=True).start()

    def app_update_worker(self, manual: bool = False, prompt: bool = True, notify: bool = False) -> None:
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
            if prompt:
                wx.CallAfter(self.prompt_for_app_update, release, asset)
            else:
                wx.CallAfter(self.store_pending_app_update, release, asset, notify)
        except Exception as exc:
            message = self.t("app_update_failed", error=exc)
            self.report_app_update_status(message, manual)
            if manual:
                wx.CallAfter(self.message, message, wx.ICON_ERROR)
        finally:
            self.app_update_check_running = False

    def report_app_update_status(self, message: str, manual: bool = False) -> None:
        self.ui_queue.put(("status", message))
        if manual:
            wx.CallAfter(self.announce_player, message)

    def store_pending_app_update(self, release: dict, asset: dict, notify: bool = False) -> None:
        version = self.release_version(release)
        if not self.is_newer_version(version, APP_VERSION):
            return
        self.pending_app_update_release = release
        self.pending_app_update_asset = asset
        message = self.t("app_update_ready_status", version=version)
        self.set_status(message)
        if notify:
            self.show_desktop_notification(
                self.t("update_available_title"),
                self.t("app_update_notification_message", version=version),
                enabled=self.settings.app_update_notifications,
                only_when_unfocused=True,
            )
        if self.in_main_menu:
            self.show_main_menu()

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
            self.pending_app_update_release = None
            self.pending_app_update_asset = None
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
        temp_dir: Path | None = None
        try:
            self.ui_queue.put(("status", self.t("downloading_update", version=version)))
            temp_dir = Path(tempfile.mkdtemp(prefix="apricotplayer-update-"))
            downloaded_path = temp_dir / self.safe_asset_filename(asset)
            self.log_update_event(f"Downloading update {version} to {downloaded_path}")
            self.download_update_asset(asset, downloaded_path, version)
            self.log_update_event(f"Downloaded update {version}; size={downloaded_path.stat().st_size}")
            self.verify_release_asset_file(asset, downloaded_path)
            self.validate_update_package(downloaded_path)
            wx.CallAfter(self.update_app_update_finished, version)
            wx.CallAfter(self.finish_app_update_install, str(downloaded_path), version)
        except Exception as exc:
            if temp_dir:
                try:
                    shutil.rmtree(temp_dir, ignore_errors=True)
                except Exception:
                    pass
            wx.CallAfter(self.update_app_update_failed, exc)

    def download_update_asset(self, asset: dict, downloaded_path: Path, version: str) -> None:
        attempts: list[tuple[str, dict[str, str]]] = []
        browser_url = str(asset.get("browser_download_url") or "")
        api_url = str(asset.get("url") or "")
        if browser_url:
            self.validate_trusted_download_url(browser_url, {"github.com"})
            attempts.append((browser_url, self.github_headers("", accept="application/octet-stream")))
        if api_url:
            self.validate_trusted_download_url(api_url, {"api.github.com"})
            attempts.append((api_url, self.github_headers("", accept="application/octet-stream")))
        if not attempts:
            raise RuntimeError("missing download url")
        last_error: Exception | None = None
        for download_url, headers in attempts:
            try:
                started = time.monotonic()
                self.log_update_event(f"Download attempt: host={urlparse(download_url).hostname or ''}; asset={asset.get('name')}; expected_size={asset.get('size') or 'unknown'}")
                request = Request(download_url, headers=headers)
                with self.open_url(request, timeout=300) as response, downloaded_path.open("wb") as handle:
                    self.validate_https_response_url(response.geturl())
                    total_header = response.headers.get("Content-Length")
                    total = int(total_header) if total_header and total_header.isdigit() else 0
                    downloaded = 0
                    last_percent = -1
                    last_progress_time = 0.0
                    while True:
                        chunk = response.read(UPDATE_DOWNLOAD_CHUNK_SIZE)
                        if not chunk:
                            break
                        handle.write(chunk)
                        downloaded += len(chunk)
                        now = time.monotonic()
                        if total:
                            percent = int(downloaded * 100 / total)
                            if percent != last_percent and (percent >= 100 or now - last_progress_time >= UPDATE_PROGRESS_MIN_INTERVAL):
                                last_percent = percent
                                last_progress_time = now
                                wx.CallAfter(self.update_app_update_progress, version, percent)
                        elif now - last_progress_time >= UPDATE_PROGRESS_MIN_INTERVAL:
                            last_progress_time = now
                            wx.CallAfter(self.update_app_update_progress, version, None)
                elapsed = max(0.001, time.monotonic() - started)
                self.log_update_event(f"Download completed: bytes={downloaded_path.stat().st_size}; seconds={elapsed:.1f}; mbps={(downloaded_path.stat().st_size * 8 / 1_000_000 / elapsed):.2f}")
                return
            except Exception as exc:
                last_error = exc
                self.log_update_event(f"Download attempt failed from {urlparse(download_url).hostname or download_url}: {exc}")
                try:
                    downloaded_path.unlink(missing_ok=True)
                except Exception:
                    pass
        raise RuntimeError(last_error or "download failed")

    @staticmethod
    def safe_asset_filename(asset: dict) -> str:
        name = Path(str(asset.get("name") or "")).name
        if name not in {INSTALLER_ASSET_NAME, PORTABLE_ZIP_ASSET_NAME, LEGACY_PORTABLE_ZIP_ASSET_NAME}:
            raise RuntimeError(f"unexpected update asset name: {name or 'missing'}")
        return name

    @staticmethod
    def validate_trusted_download_url(download_url: str, allowed_hosts: set[str]) -> None:
        parsed = urlparse(str(download_url or ""))
        host = (parsed.hostname or "").lower()
        if parsed.scheme.lower() != "https" or host not in {allowed.lower() for allowed in allowed_hosts}:
            raise RuntimeError(f"untrusted download URL: {download_url}")

    @staticmethod
    def validate_https_response_url(download_url: str) -> None:
        parsed = urlparse(str(download_url or ""))
        if parsed.scheme.lower() != "https":
            raise RuntimeError(f"download redirected to a non-HTTPS URL: {download_url}")

    @staticmethod
    def file_sha256(path: Path) -> str:
        digest = hashlib.sha256()
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
        return digest.hexdigest()

    @classmethod
    def verify_file_sha256(cls, path: Path, expected_sha256: str) -> None:
        expected = str(expected_sha256 or "").strip().lower()
        if expected.startswith("sha256:"):
            expected = expected.split(":", 1)[1]
        if not expected:
            return
        actual = cls.file_sha256(path)
        if actual.lower() != expected:
            raise RuntimeError("downloaded file checksum did not match the published SHA-256 digest")

    @classmethod
    def verify_release_asset_file(cls, asset: dict, path: Path) -> None:
        expected_size = asset.get("size")
        if isinstance(expected_size, int) and expected_size > 0 and path.stat().st_size != expected_size:
            raise RuntimeError("downloaded update size did not match the GitHub release asset size")
        cls.verify_file_sha256(path, str(asset.get("digest") or ""))

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
    def validate_zip_member_path(member_name: str) -> None:
        normalized = member_name.replace("\\", "/")
        if not normalized or normalized.startswith("/") or re.match(r"^[A-Za-z]:", normalized):
            raise RuntimeError("zip package contains an unsafe absolute path")
        parts = [part for part in normalized.split("/") if part]
        if any(part == ".." for part in parts):
            raise RuntimeError("zip package contains an unsafe parent-directory path")

    @classmethod
    def safe_extract_zip(cls, archive: zipfile.ZipFile, target_dir: Path) -> None:
        target_root = target_dir.resolve()
        for member in archive.infolist():
            cls.validate_zip_member_path(member.filename)
            destination = (target_root / member.filename).resolve()
            try:
                destination.relative_to(target_root)
            except ValueError:
                raise RuntimeError("zip package member would extract outside the target directory") from None
        archive.extractall(target_root)

    @classmethod
    def validate_update_package(cls, path: Path) -> None:
        if not path.exists() or path.stat().st_size < 1024 * 1024:
            raise RuntimeError("downloaded update is not a valid package")
        if MainFrame.is_portable_zip_asset(path):
            if not zipfile.is_zipfile(path):
                raise RuntimeError("downloaded portable update is not a valid zip file")
            with zipfile.ZipFile(path) as archive:
                for member in archive.infolist():
                    cls.validate_zip_member_path(member.filename)
                if not any(Path(member.filename.replace("\\", "/")).name.lower() == "apricotplayer.exe" for member in archive.infolist()):
                    raise RuntimeError("downloaded portable update does not contain ApricotPlayer.exe")
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
                "$silentArgs = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CLOSEAPPLICATIONS', '/TASKS=desktopicon,mediaassoc', ('/DIR=\"' + $installDir + '\"'), ('/LOG=\"' + $installerLog + '\"'))",
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
        try:
            release = self.fetch_github_latest_release()
            if release:
                return release
        except Exception:
            pass
        try:
            releases = self.fetch_public_releases()
            if releases:
                return releases[0]
        except Exception:
            pass
        return self.fetch_github_latest_release()

    def fetch_github_latest_release(self) -> dict | None:
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
        return None

    def fetch_public_releases(self) -> list[dict]:
        request = Request(GITHUB_RELEASES_API_URL, headers=self.github_headers(""))
        with self.open_url(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
        if not isinstance(payload, list):
            releases = []
        else:
            releases = [release for release in payload if isinstance(release, dict) and not release.get("draft")]
        try:
            latest = self.fetch_github_latest_release()
            if latest:
                latest_tag = str(latest.get("tag_name") or "")
                latest_id = latest.get("id")
                if not any(release.get("id") == latest_id or str(release.get("tag_name") or "") == latest_tag for release in releases):
                    releases.append(latest)
        except Exception:
            pass
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
        return None

    @staticmethod
    def current_executable_path() -> Path:
        try:
            return Path(sys.executable if getattr(sys, "frozen", False) else __file__).resolve()
        except Exception:
            return Path(sys.executable if getattr(sys, "frozen", False) else __file__)

    @staticmethod
    def registry_read_value(root, subkey: str, value_name: str = "") -> str:
        if winreg is None:
            return ""
        try:
            with winreg.OpenKey(root, subkey, 0, winreg.KEY_READ) as key:
                value, _value_type = winreg.QueryValueEx(key, value_name)
                return str(value or "")
        except Exception:
            return ""

    def media_association_registry_complete(self) -> bool:
        if os.name != "nt" or winreg is None:
            return True
        expected_exe = str(self.current_executable_path()).lower()
        required_extensions = [".mp3", ".mp4", ".mkv", ".m4a", ".flac", ".wav", ".webm"]
        for root in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
            registered = self.registry_read_value(root, r"Software\RegisteredApplications", APP_NAME)
            command = self.registry_read_value(root, rf"Software\Classes\{APP_NAME}.Media\shell\open\command")
            if not registered or not command:
                continue
            command_lower = command.lower()
            if expected_exe not in command_lower or "%1" not in command_lower:
                continue
            extension_commands_ok = True
            for extension in required_extensions:
                extension_command = self.registry_read_value(root, rf"Software\Classes\SystemFileAssociations\{extension}\shell\{APP_NAME}\command")
                extension_command_lower = extension_command.lower()
                if expected_exe not in extension_command_lower or "%1" not in extension_command_lower:
                    extension_commands_ok = False
                    break
            if extension_commands_ok:
                return True
        return False

    @staticmethod
    def registry_set_value(root, subkey: str, value_name: str, value_type: int, value) -> None:
        if winreg is None:
            raise RuntimeError("Windows registry is not available")
        with winreg.CreateKeyEx(root, subkey, 0, winreg.KEY_WRITE) as key:
            winreg.SetValueEx(key, value_name, 0, value_type, value)

    def register_media_associations_current_user(self) -> None:
        if os.name != "nt" or winreg is None:
            return
        exe_path = str(self.current_executable_path())
        command = f'"{exe_path}" "%1"'
        icon = f"{exe_path},0"
        root = winreg.HKEY_CURRENT_USER
        self.registry_set_value(root, r"Software\RegisteredApplications", APP_NAME, winreg.REG_SZ, rf"Software\{APP_NAME}\Capabilities")
        self.registry_set_value(root, rf"Software\{APP_NAME}\Capabilities", "ApplicationName", winreg.REG_SZ, APP_NAME)
        self.registry_set_value(
            root,
            rf"Software\{APP_NAME}\Capabilities",
            "ApplicationDescription",
            winreg.REG_SZ,
            "Accessible media player, YouTube player, downloader, podcast and RSS player",
        )
        for extension in sorted(LOCAL_MEDIA_EXTENSIONS):
            self.registry_set_value(root, rf"Software\{APP_NAME}\Capabilities\FileAssociations", extension, winreg.REG_SZ, f"{APP_NAME}.Media")
            self.registry_set_value(root, rf"Software\Classes\{extension}\OpenWithProgids", f"{APP_NAME}.Media", winreg.REG_NONE, b"")
            extension_base = rf"Software\Classes\SystemFileAssociations\{extension}\shell\{APP_NAME}"
            self.registry_set_value(root, extension_base, "MUIVerb", winreg.REG_SZ, f"Play with {APP_NAME}")
            self.registry_set_value(root, extension_base, "Icon", winreg.REG_SZ, icon)
            self.registry_set_value(root, rf"{extension_base}\command", "", winreg.REG_SZ, command)
        self.registry_set_value(root, rf"Software\Classes\{APP_NAME}.Media", "", winreg.REG_SZ, f"{APP_NAME} media file")
        self.registry_set_value(root, rf"Software\Classes\{APP_NAME}.Media\DefaultIcon", "", winreg.REG_SZ, icon)
        self.registry_set_value(root, rf"Software\Classes\{APP_NAME}.Media\shell\open\command", "", winreg.REG_SZ, command)
        for media_kind in ("audio", "video"):
            base = rf"Software\Classes\SystemFileAssociations\{media_kind}\shell\{APP_NAME}"
            self.registry_set_value(root, base, "MUIVerb", winreg.REG_SZ, f"Play with {APP_NAME}")
            self.registry_set_value(root, base, "Icon", winreg.REG_SZ, icon)
            self.registry_set_value(root, rf"{base}\command", "", winreg.REG_SZ, command)
        try:
            ctypes.windll.shell32.SHChangeNotify(0x08000000, 0x0000, None, None)
        except Exception:
            pass

    def maybe_prompt_media_association_registration(self) -> None:
        if not self.is_installed_build() or self.media_association_registry_complete():
            return
        if str(getattr(self.settings, "media_association_prompted_version", "")) == APP_VERSION:
            return
        result = wx.MessageBox(
            self.t("media_association_prompt_message"),
            self.t("media_association_prompt_title"),
            wx.YES_NO | wx.ICON_QUESTION,
        )
        self.settings.media_association_prompted_version = APP_VERSION
        self.save_settings()
        if result != wx.YES:
            return
        try:
            self.register_media_associations_current_user()
            self.set_status(self.t("media_association_registered"))
            self.speak_text(self.t("media_association_registered"))
        except Exception as exc:
            self.message(self.t("media_association_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

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
    def parse_version(value: str) -> tuple[int, int, int, int, int, int]:
        match = re.match(r"^v?(\d+)\.(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:-([A-Za-z]+)(?:[.-]?(\d+))?)?$", value.strip())
        if not match:
            return (0, 0, 0, 0, 0, 0)
        major, minor, patch, hotfix = (
            int(match.group(1)),
            int(match.group(2)),
            int(match.group(3) or 0),
            int(match.group(4) or 0),
        )
        stage_name = (match.group(5) or "").lower()
        stage_number = int(match.group(6) or 0)
        stage_rank = {"alpha": 1, "beta": 2, "rc": 3}.get(stage_name, 4)
        return (major, minor, patch, hotfix, stage_rank, stage_number)

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
        if configured and (Path(configured).exists() or shutil.which(configured)):
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

    def first_available_shortcut(self, values: dict[str, str], action: str, candidates: list[str]) -> str:
        used = {
            self.canonical_shortcut(shortcut)
            for other_action, shortcut in values.items()
            if other_action != action
        }
        for candidate in candidates:
            canonical = self.canonical_shortcut(candidate)
            if canonical and canonical not in used:
                return candidate
        return ""

    def repair_keyboard_shortcut_conflicts(self, shortcuts: dict[str, str]) -> dict[str, str]:
        repaired = dict(shortcuts)
        if str(repaired.get("player_marker_start", "")).strip() in {"[", "š", "Š"}:
            repaired["player_marker_start"] = DEFAULT_KEYBOARD_SHORTCUTS["player_marker_start"]
        if str(repaired.get("player_marker_end", "")).strip() in {"]", "đ", "Đ"}:
            repaired["player_marker_end"] = DEFAULT_KEYBOARD_SHORTCUTS["player_marker_end"]
        equalizer_shortcut = self.canonical_shortcut(repaired.get("player_equalizer", ""))
        if equalizer_shortcut in {"e", "g"}:
            repaired["player_equalizer"] = DEFAULT_KEYBOARD_SHORTCUTS["player_equalizer"]
        if not repaired.get("player_edit_mode"):
            repaired["player_edit_mode"] = DEFAULT_KEYBOARD_SHORTCUTS["player_edit_mode"]
        details_shortcut = self.canonical_shortcut(repaired.get("player_details", ""))
        volume_status_shortcut = self.canonical_shortcut(repaired.get("player_volume_status", ""))
        if details_shortcut in {"", "v"} or details_shortcut == volume_status_shortcut:
            repaired["player_details"] = DEFAULT_KEYBOARD_SHORTCUTS["player_details"]
        if not repaired.get("player_volume_status") or volume_status_shortcut in {"f7", self.canonical_shortcut(repaired.get("player_details", ""))}:
            repaired["player_volume_status"] = DEFAULT_KEYBOARD_SHORTCUTS["player_volume_status"]
        if self.canonical_shortcut(repaired.get("new_subscription_videos", "")) == self.canonical_shortcut(repaired.get("player_play_pause", "")):
            replacement = self.first_available_shortcut(repaired, "new_subscription_videos", ["Ctrl+Shift+V", "Ctrl+Alt+V", "Alt+N"])
            if replacement:
                repaired["new_subscription_videos"] = replacement
        if self.canonical_shortcut(repaired.get("add_to_playlist", "")) == self.canonical_shortcut("Ctrl+Shift+P"):
            repaired["add_to_playlist"] = DEFAULT_KEYBOARD_SHORTCUTS["add_to_playlist"]
        if self.canonical_shortcut(repaired.get("remove_from_playlist", "")) == self.canonical_shortcut("Ctrl+Shift+R"):
            repaired["remove_from_playlist"] = DEFAULT_KEYBOARD_SHORTCUTS["remove_from_playlist"]
        if not str(repaired.get("add_favorite", "")).strip():
            repaired["add_favorite"] = DEFAULT_KEYBOARD_SHORTCUTS["add_favorite"]
        if not str(repaired.get("remove_favorite", "")).strip():
            repaired["remove_favorite"] = DEFAULT_KEYBOARD_SHORTCUTS["remove_favorite"]
        seen: dict[str, str] = {}
        for action, _label_key in SHORTCUT_DEFINITIONS:
            canonical = self.canonical_shortcut(repaired.get(action, ""))
            if not canonical:
                repaired[action] = DEFAULT_KEYBOARD_SHORTCUTS.get(action, "")
                canonical = self.canonical_shortcut(repaired.get(action, ""))
            if canonical and canonical in seen:
                replacement = self.first_available_shortcut(repaired, action, [DEFAULT_KEYBOARD_SHORTCUTS.get(action, ""), f"Ctrl+Alt+{action[:1].upper()}"])
                if replacement:
                    repaired[action] = replacement
            if canonical:
                seen[self.canonical_shortcut(repaired.get(action, ""))] = action
        return repaired

    def load_settings(self) -> Settings:
        backup_settings = SETTINGS_FILE.with_suffix(".json.bak")
        sources = [SETTINGS_FILE, backup_settings, LEGACY_SETTINGS_FILE]
        load_errors: list[str] = []
        for source in sources:
            if not source.exists():
                continue
            try:
                raw_text = source.read_text(encoding="utf-8")
                if not raw_text.strip():
                    raise ValueError("settings file is empty")
                raw_data = json.loads(raw_text)
                if not isinstance(raw_data, dict):
                    raise ValueError(f"settings file must contain an object, got {type(raw_data).__name__}")
                data = dict(raw_data)
                allowed_keys = {field.name for field in fields(Settings)}
                data = {key: value for key, value in data.items() if key in allowed_keys}
                if not data:
                    raise ValueError("settings file contains no recognized settings")
                merged = {**asdict(Settings()), **data}
                if merged.get("language") not in LANGUAGE_CODES:
                    merged["language"] = "en"
                if merged.get("filename_template") == OLD_FILENAME_TEMPLATE:
                    merged["filename_template"] = DEFAULT_FILENAME_TEMPLATE
                merged["pitch_mode"] = self.normalize_pitch_mode_value(str(merged.get("pitch_mode") or ""))
                merged["speed_audio_mode"] = self.normalize_speed_audio_mode_value(str(merged.get("speed_audio_mode") or ""))
                merged["direct_link_enter_action"] = self.normalize_direct_link_enter_action(str(merged.get("direct_link_enter_action") or ""))
                merged["video_format"] = self.normalize_video_format_value(str(merged.get("video_format") or ""))
                merged["global_equalizer_gains"] = self.normalized_equalizer_gains(merged.get("global_equalizer_gains"))
                merged["global_equalizer_preset"] = self.normalized_equalizer_preset(str(merged.get("global_equalizer_preset") or EQ_PRESET_FLAT))
                merged["equalizer_preset_gains"] = self.normalized_equalizer_preset_gains(merged.get("equalizer_preset_gains"))
                merged["equalizer_custom_names"] = self.normalized_equalizer_custom_names(merged.get("equalizer_custom_names"))
                if "global_equalizer_preset" not in data and any(abs(value) >= 0.05 for value in merged["global_equalizer_gains"].values()):
                    merged["global_equalizer_preset"] = "custom1"
                    merged["equalizer_custom_names"]["custom1"] = "Imported"
                    merged["equalizer_preset_gains"]["custom1"] = merged["global_equalizer_gains"]
                    self.settings_migrated = True
                merged["equalizer_db_range"] = self.to_int(str(merged.get("equalizer_db_range") or "12"), 12, 6, 24)
                merged["default_volume"] = self.to_int(str(merged.get("default_volume") or "100"), 100, 0, 300)
                old_audio_quality = str(merged.get("audio_quality") or "")
                merged["audio_quality"] = self.normalize_audio_quality_value(old_audio_quality)
                if old_audio_quality and merged["audio_quality"] != old_audio_quality:
                    self.settings_migrated = True
                provider = str(merged.get("podcast_search_provider") or PODCAST_DIRECTORY_PROVIDER_APPLE)
                merged["podcast_search_provider"] = provider if provider in PODCAST_DIRECTORY_PROVIDER_OPTIONS else PODCAST_DIRECTORY_PROVIDER_APPLE
                country = str(merged.get("podcast_search_country") or "US").upper()
                merged["podcast_search_country"] = country if country in PODCAST_COUNTRY_OPTIONS else "US"
                if not str(merged.get("cookies_browser_profile") or "").strip():
                    merged["cookies_browser_profile"] = COOKIE_PROFILE_AUTO
                shortcuts = self.normalized_keyboard_shortcuts(merged.get("keyboard_shortcuts"))
                repaired_shortcuts = self.repair_keyboard_shortcut_conflicts(shortcuts)
                if repaired_shortcuts != shortcuts:
                    self.settings_migrated = True
                merged["keyboard_shortcuts"] = repaired_shortcuts
                skipped_version = str(merged.get("skipped_update_version") or "")
                if skipped_version and not self.is_newer_version(skipped_version, APP_VERSION):
                    merged["skipped_update_version"] = ""
                self.settings_loaded_from_path = source
                if source != SETTINGS_FILE:
                    self.settings_migrated = True
                return Settings(**merged)
            except Exception as exc:
                load_errors.append(f"{source}: {exc}")
                continue
        self.settings_load_errors = load_errors
        if SETTINGS_FILE.exists() or backup_settings.exists():
            self.settings_save_blocked = True
            self.log_update_event("Settings load failed; automatic settings saves are blocked to avoid overwriting user preferences. " + " | ".join(load_errors[-3:]))
        return Settings()

    def save_settings(self) -> None:
        if getattr(self, "settings_save_blocked", False):
            self.log_update_event("Settings save skipped because settings could not be loaded safely.")
            return
        APP_DIR.mkdir(parents=True, exist_ok=True)
        payload = json.dumps(asdict(self.settings), indent=2, ensure_ascii=False)
        temp_file = SETTINGS_FILE.with_suffix(".json.tmp")
        backup_file = SETTINGS_FILE.with_suffix(".json.bak")
        temp_file.write_text(payload, encoding="utf-8")
        if SETTINGS_FILE.exists():
            try:
                shutil.copy2(SETTINGS_FILE, backup_file)
            except OSError:
                pass
        os.replace(temp_file, SETTINGS_FILE)

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

    def load_playback_queue(self) -> list[dict]:
        return self.load_json_list(PLAYBACK_QUEUE_FILE)

    def save_playback_queue(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        PLAYBACK_QUEUE_FILE.write_text(json.dumps(self.playback_queue, indent=2, ensure_ascii=False), encoding="utf-8")

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
    def seconds_from_iso8601_duration(value: str) -> int:
        match = re.fullmatch(r"P(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?)?", str(value or ""))
        if not match:
            return 0
        days, hours, minutes, seconds = (int(part or 0) for part in match.groups())
        return days * 86400 + hours * 3600 + minutes * 60 + seconds

    @staticmethod
    def timestamp_from_iso_datetime(value: str) -> int | None:
        text = str(value or "").strip()
        if not text:
            return None
        try:
            parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
            if parsed.tzinfo is None:
                parsed = parsed.replace(tzinfo=timezone.utc)
            return int(parsed.timestamp())
        except ValueError:
            try:
                return int(parsedate_to_datetime(text).timestamp())
            except Exception:
                return None

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


def start_in_tray_requested() -> bool:
    return any(arg == START_IN_TRAY_ARG for arg in sys.argv[1:])


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


def startup_media_path_argument(argv: list[str] | None = None) -> str:
    args = list(sys.argv[1:] if argv is None else argv)
    for arg in args:
        if arg in {UPDATE_RELAUNCH_ARG, START_IN_TRAY_ARG} or arg.startswith("--"):
            continue
        path = MainFrame.local_media_path_from_input(arg)
        if path:
            return str(path)
    return ""


def request_existing_instance_activation(action: str = "show", **extra_payload) -> None:
    try:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        payload = {"action": action, "pid": os.getpid(), "timestamp": time.time()}
        payload.update(extra_payload)
        ACTIVATE_SIGNAL_FILE.write_text(json.dumps(payload), encoding="utf-8")
    except Exception:
        pass


class App(wx.App):
    def OnInit(self) -> bool:
        update_relaunch = update_relaunch_requested()
        if update_relaunch:
            mark_update_relaunch_window()
        startup_media_path = startup_media_path_argument()
        tray_start = start_in_tray_requested() and not startup_media_path
        instance_name = f"{APP_NAME}-{wx.GetUserId() or 'user'}"
        self.instance_checker = wx.SingleInstanceChecker(instance_name)
        if self.instance_checker.IsAnotherRunning():
            if suppress_already_open_for_update():
                return False
            if startup_media_path:
                request_existing_instance_activation("open_file", path=startup_media_path)
            elif not tray_start:
                request_existing_instance_activation("show")
            else:
                return False
            return False
        frame = MainFrame(start_hidden_in_tray=tray_start)
        self.SetTopWindow(frame)
        if tray_start:
            frame.Hide()
        else:
            frame.Show()
            if update_relaunch:
                frame.activate_after_update_relaunch()
            else:
                frame.activate_window_later()
        if startup_media_path:
            wx.CallAfter(frame.open_local_media_file, startup_media_path)
        return True


def main() -> int:
    app = App(False)
    app.MainLoop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
