from __future__ import annotations

import json
import os
import queue
import random
import re
import http.cookiejar
import sys
import threading
import time
import xml.etree.ElementTree as ET
import zipfile
from dataclasses import asdict, dataclass, field, fields
from datetime import datetime, timezone
from importlib import import_module
from pathlib import Path
from urllib.parse import parse_qs, parse_qsl, unquote, urlencode, urljoin, urlparse

import wx
import wx.adv

try:
    import winreg
except ImportError:
    winreg = None

from apricot.locales import TEXT

yt_dlp = None
yt_dlp_import_error: Exception | None = None
_SSL_CONTEXT = None
_URLLIB_REQUEST_MODULE = None
_PARSEDATE_TO_DATETIME = None


class LazyModule:
    def __init__(self, module_name: str) -> None:
        self.module_name = module_name
        self.module = None

    def __getattr__(self, name: str):
        if self.module is None:
            self.module = import_module(self.module_name)
        return getattr(self.module, name)


hashlib = LazyModule("hashlib")
shutil = LazyModule("shutil")
ssl = LazyModule("ssl")
socket = LazyModule("socket")
subprocess = LazyModule("subprocess")
tempfile = LazyModule("tempfile")
ctypes = LazyModule("ctypes")


def urllib_request_module():
    global _URLLIB_REQUEST_MODULE
    if _URLLIB_REQUEST_MODULE is None:
        _URLLIB_REQUEST_MODULE = import_module("urllib.request")
    return _URLLIB_REQUEST_MODULE


def Request(*args, **kwargs):
    return urllib_request_module().Request(*args, **kwargs)


def urlopen(*args, **kwargs):
    return urllib_request_module().urlopen(*args, **kwargs)


def parsedate_to_datetime(value: str):
    global _PARSEDATE_TO_DATETIME
    if _PARSEDATE_TO_DATETIME is None:
        _PARSEDATE_TO_DATETIME = import_module("email.utils").parsedate_to_datetime
    return _PARSEDATE_TO_DATETIME(value)


def get_yt_dlp():
    global yt_dlp, yt_dlp_import_error
    if yt_dlp is not None:
        return yt_dlp
    if yt_dlp_import_error is not None:
        return None
    try:
        os.environ.setdefault("YTDLP_NO_PLUGINS", "1")
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
    os.environ.setdefault("YTDLP_NO_PLUGINS", "1")
    try:
        import_module("yt_dlp.globals").plugin_dirs.value = []
    except Exception:
        pass

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
# Single source of truth: apricot/__init__.py  ←  bump only that file per release.
# This module derives APP_VERSION and the human-readable label from it so that
# the window title, update comparisons, and installer metadata always agree.
from apricot import __version__ as APP_VERSION  # e.g. "0.9.44-beta.8"
def _make_version_label(v: str) -> str:
    # "0.9.44-beta.8"  →  "0.9.44 Beta 8"
    # "1.0.0"          →  "1.0.0"
    parts = v.split("-", 1)
    if len(parts) == 2:
        pre = parts[1].replace(".", " ").title()
        return f"{parts[0]} {pre}"
    return v
APP_VERSION_LABEL = _make_version_label(APP_VERSION)
del _make_version_label
WINDOW_TITLE = f"{APP_NAME} {APP_VERSION_LABEL}"
LEGACY_APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "UrhasaurusYouTubePlayer"
APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "ApricotPlayer"
UPDATE_RELAUNCH_ARG = "--updated-relaunch"
START_IN_TRAY_ARG = "--start-in-tray"
UPDATE_RELAUNCH_SENTINEL = APP_DIR / "updated-relaunch.json"
ACTIVATE_SIGNAL_FILE = APP_DIR / "activate.json"
SETTINGS_FILE = APP_DIR / "settings.json"
WINDOWS_ERROR_ALREADY_EXISTS = 183
FAVORITES_FILE = APP_DIR / "favorites.json"
BOOKMARKS_FILE = APP_DIR / "bookmarks.json"
HISTORY_FILE = APP_DIR / "history.json"
SUBSCRIPTIONS_FILE = APP_DIR / "subscriptions.json"
RSS_FEEDS_FILE = APP_DIR / "rss_feeds.json"
USER_PLAYLISTS_FILE = APP_DIR / "playlists.json"
NOTIFICATIONS_FILE = APP_DIR / "notifications.json"
PLAYBACK_POSITIONS_FILE = APP_DIR / "playback_positions.json"
PLAYBACK_QUEUE_FILE = APP_DIR / "playback_queue.json"
STREAM_URL_CACHE_FILE = APP_DIR / "stream_url_cache.json"
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
YOUTUBE_API_SEARCH_URL = "https://www.googleapis.com/youtube/v3/search"
YOUTUBE_API_COMMENT_THREADS_URL = "https://www.googleapis.com/youtube/v3/commentThreads"
YOUTUBE_API_COMMENTS_URL = "https://www.googleapis.com/youtube/v3/comments"
YOUTUBE_API_CREDENTIALS_URL = "https://console.cloud.google.com/apis/credentials"
LRCLIB_API_GET_URL = "https://lrclib.net/api/get"
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
EQ_FILTER_ALT_LABEL = "apricot_eq_next"
EQ_FILTER_ALT_REF = f"@{EQ_FILTER_ALT_LABEL}"
EQ_LIMITER_FILTER = "alimiter=limit=0.95:attack=5:release=80"
EQ_APPLY_DELAY_MS = 160
EQ_CLIPPING_HEADROOM_LIMIT_DB = 12.0
NORMAL_VOLUME_MAX = 100
BOOSTED_VOLUME_MAX = 300
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
EQ_BAND_IDS = {band_id for band_id, _label in EQ_BANDS}
EQ_FILTER_Q_WIDTH = 1.8
EQ_FILTER_Q_WIDTHS = {
    "31": 1.7,
    "62": 2.0,
    "125": 2.6,
    "250": 2.3,
    "500": 2.1,
    "1000": 2.0,
    "2000": 1.9,
    "4000": 1.9,
    "8000": 1.7,
    "16000": 1.5,
}
EQ_RANGE_OPTIONS = ["6", "12", "18", "24"]
POPULAR_CHANNEL_METADATA_WORKERS = 4
POPULAR_CHANNEL_PROGRESS_INTERVAL = 25
SEEK_SECONDS_OPTIONS = ["0.1", "0.25", "0.5", "0.75", "1", "1.5", "2", "2.5", "3", "4", "5", "7.5", "10", "15", "20", "30", "45", "60"]
STREAM_URL_CACHE_OPTIONS = ["5", "10", "20", "30", "60", "240", "1440", "10080", "0"]
REPLAYGAIN_MODE_OFF = "no"
REPLAYGAIN_MODE_TRACK = "track"
REPLAYGAIN_MODE_ALBUM = "album"
REPLAYGAIN_MODE_OPTIONS = [REPLAYGAIN_MODE_OFF, REPLAYGAIN_MODE_TRACK, REPLAYGAIN_MODE_ALBUM]
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
PODCAST_COUNTRY_OPTIONS = ["US", "SI", "GB", "DE", "FR", "ES", "IT", "AT", "HR", "RS", "CA", "AU", "NL", "SE", "PL", "AR", "BE", "BR", "CH", "CL", "CO", "CZ", "DK", "EG", "FI", "GR", "HK", "HU", "ID", "IE", "IL", "IN", "JP", "KR", "MX", "NO", "NZ", "PH", "PT", "RO", "RU", "SG", "SK", "TH", "TR", "TW", "UA", "VN", "ZA"]
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
    "open_bookmarks": "Ctrl+Alt+K",
    "open_playlists": "Ctrl+Alt+P",
    "open_subscriptions": "Ctrl+Alt+B",
    "open_current_downloads": "Ctrl+Alt+D",
    "open_history": "Ctrl+Alt+H",
    "open_podcasts_rss": "Ctrl+Alt+R",
    "open_settings": "Ctrl+Alt+S",
    "background_play_pause": "Ctrl+Space",
    "copy_diagnostic_report": "Ctrl+Alt+Shift+D",
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
    "player_copy_timestamp_link": "Ctrl+Shift+L",
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
    "player_replaygain": "Ctrl+Shift+G",
    "player_add_bookmark": "Ctrl+Shift+B",
    "player_bookmarks": "Ctrl+Shift+K",
    "player_chapters": "Ctrl+Shift+C",
    "player_transcript": "Ctrl+Shift+T",
    "player_lyrics": "Ctrl+Shift+Y",
    "player_comments": "Ctrl+Shift+M",
    "player_previous_chapter": "Alt+Left",
    "player_next_chapter": "Alt+Right",
    "player_edit_mode": "E",
    "player_save_edit_copy": "Ctrl+S",
    "player_replace_edit_original": "Ctrl+R",
    "player_marker_start": "LeftBracket",
    "player_marker_end": "RightBracket",
    "player_preview_marked_clip": "P",
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
    ("open_bookmarks", "shortcut_open_bookmarks"),
    ("open_playlists", "shortcut_open_playlists"),
    ("open_subscriptions", "shortcut_open_subscriptions"),
    ("open_current_downloads", "shortcut_open_current_downloads"),
    ("open_history", "shortcut_open_history"),
    ("open_podcasts_rss", "shortcut_open_podcasts_rss"),
    ("open_settings", "shortcut_open_settings"),
    ("background_play_pause", "shortcut_background_play_pause"),
    ("copy_diagnostic_report", "shortcut_copy_diagnostic_report"),
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
    ("player_copy_timestamp_link", "shortcut_player_copy_timestamp_link"),
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
    ("player_replaygain", "shortcut_player_replaygain"),
    ("player_add_bookmark", "shortcut_player_add_bookmark"),
    ("player_bookmarks", "shortcut_player_bookmarks"),
    ("player_chapters", "shortcut_player_chapters"),
    ("player_transcript", "shortcut_player_transcript"),
    ("player_lyrics", "shortcut_player_lyrics"),
    ("player_comments", "shortcut_player_comments"),
    ("player_previous_chapter", "shortcut_player_previous_chapter"),
    ("player_next_chapter", "shortcut_player_next_chapter"),
    ("player_edit_mode", "shortcut_player_edit_mode"),
    ("player_save_edit_copy", "shortcut_player_save_edit_copy"),
    ("player_replace_edit_original", "shortcut_player_replace_edit_original"),
    ("player_marker_start", "shortcut_player_marker_start"),
    ("player_marker_end", "shortcut_player_marker_end"),
    ("player_preview_marked_clip", "shortcut_player_preview_marked_clip"),
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


# TEXT is imported from locales.py at the top.





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



