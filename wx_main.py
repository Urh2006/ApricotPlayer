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

from locales import TEXT

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
APP_VERSION = "0.9.17"
APP_VERSION_LABEL = "0.9.17"
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
    "player_chapters": "Ctrl+Shift+C",
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
    ("player_chapters", "shortcut_player_chapters"),
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
        self.rss_feeds: list[dict] = []
        self.rss_feeds_loaded = False
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
        self.session_autoplay_next = False
        self.session_volume: float | None = None
        self.player_generation = 0
        self.play_request_generation = 0
        self.playback_start_pending = False
        self.player_ended = False
        self.player_paused = False
        self.current_video_item: dict | None = None
        self.current_video_info: dict = {}
        self.player_panel: wx.Panel | None = None
        self.player_fullscreen_session = False
        self.player_fullscreen_results_override = False
        self.fullscreen_checkbox_toggle_block_until = 0.0
        self.manual_background_playback_active = False
        self.player_navigation_controls = []
        self.player_action_controls = []
        self.player_escape_stop_controls = []
        self.fullscreen_checkbox: wx.CheckBox | None = None
        self.session_autoplay_checkbox: wx.CheckBox | None = None
        self.details_label: wx.StaticText | None = None
        self.video_details: wx.TextCtrl | None = None
        self.details_button_sizer: wx.Sizer | None = None
        self.background_player_controls: list[wx.Window] = []
        self.background_player_previous_control: wx.Window | None = None
        self.last_button_row_controls: list[wx.Button] = []
        self.player_play_pause_buttons: list[wx.Button] = []
        self.background_player_section_added = False
        self.background_player_section_pending = False
        self.background_player_section_generation = 0
        self.download_queue: dict[str, dict] = {}
        self.active_downloads: dict[str, dict] = {}
        self.download_cancel_events: dict[str, threading.Event] = {}
        self.download_progress_dialog: wx.ProgressDialog | None = None
        self.download_progress_task_id = ""
        self.download_task_counter = 0
        self.conversion_progress_dialog: wx.ProgressDialog | None = None
        self.queue_items: list[dict] = []
        self.last_download_shortcut: tuple[str, str, float] = ("", "", 0.0)
        self.ipc_path: str | None = None
        self.mpv_ipc_lock = threading.Lock()
        self.cookie_repair_lock = threading.Lock()
        self.cookie_repair_suppressed_until = 0.0
        self.ui_queue: queue.Queue[tuple[str, object]] = queue.Queue()
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.results_selection_update_suppressed = False
        self.last_user_result_index = 0
        self.last_user_result_identity = ""
        self.current_search_type_code = "All"
        self.collection_url = ""
        self.collection_result_type = ""
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
        self.pending_player_next_after_dynamic_load = False
        self.pending_player_next_preserve_focus = False
        self.pending_player_next_current_url = ""
        self.current_stream_url = ""
        self.current_stream_headers: dict = {}
        self.player_sequence_results: list[dict] = []
        self.stream_url_cache: dict[str, dict] = {}
        self.stream_url_cache_lock = threading.Lock()
        self.prefetch_stream_urls: set[str] = set()
        self.player_session_open = False
        self.current_audio_device = ""
        self.session_audio_output_device = ""
        self.edit_mode_enabled = False
        self.session_equalizer_enabled: bool | None = None
        self.session_equalizer_gains: dict[str, float] = {}
        self.session_equalizer_before_bass_boost: tuple[bool | None, dict[str, float]] | None = None
        self.visible_equalizer_draft_gains: dict[str, float] = {}
        self.equalizer_controls_loading = False
        self.equalizer_apply_generation = 0
        self.equalizer_apply_timer: wx.CallLater | None = None
        self.bass_boost_enabled = False
        self.equalizer_filter_active = False
        self.equalizer_filter_ref = EQ_FILTER_REF
        self.volume_change_lock = threading.Lock()
        self.volume_change_pending_target: float | None = None
        self.volume_change_timer: wx.CallLater | None = None
        self.clip_start_marker: float | None = None
        self.clip_end_marker: float | None = None
        self.clip_preview_generation = 0
        self.audio_device_options_cache: tuple[float, list[str], list[str]] | None = None
        self.audio_device_refresh_running = False
        self.metadata_hydration_urls: set[str] = set()
        self.search_generation = 0
        self.playlist_play_generation = 0
        self.local_folder_cache: dict[str, list[dict]] = {}
        self.current_local_folder_path = ""
        self.current_local_folder_items: list[dict] = []
        self.last_activation_check = 0.0
        self.settings_render_generation = 0
        self.settings_pending_section_index = -1
        self.settings_controls_applied_for_pending = False
        self.settings_initial_focus_pending = False
        self.shortcut_editor_values: dict[str, str] = {}
        self.shortcut_editor_actions: list[str] = []
        self.shortcut_editor_current_action = ""
        self.details_opened_temporarily = False
        self.nvda_client = None
        self.nvda_client_load_attempted = False
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
        self.Bind(wx.EVT_NAVIGATION_KEY, self.on_player_navigation_key)
        self.panel.Bind(wx.EVT_NAVIGATION_KEY, self.on_player_navigation_key)
        self.Bind(wx.EVT_CLOSE, self.on_close)
        self.install_download_accelerators()
        if self.started_hidden_in_tray:
            self.setup_taskbar_icon()
        else:
            wx.CallLater(750, self.setup_taskbar_icon)
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
        if self.settings.enable_podcasts_rss and self.settings.rss_refresh_on_startup:
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
        cookiejar = import_module("http.cookiejar")
        jar = cookiejar.MozillaCookieJar()
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
        cookiejar = import_module("http.cookiejar")
        return cookiejar.Cookie(
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
        cookiejar = import_module("http.cookiejar")
        jar = cookiejar.MozillaCookieJar()
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
            cookiejar = import_module("http.cookiejar")
            jar = cookiejar.MozillaCookieJar()
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
        cookiejar = import_module("http.cookiejar")
        jar = cookiejar.MozillaCookieJar()
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
        cookiejar = import_module("http.cookiejar")
        jar = cookiejar.MozillaCookieJar()
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
            cookie = cookiejar.Cookie(
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
            asyncio_module = import_module("asyncio")
            cookies = asyncio_module.run(self.devtools_get_all_cookies(websocket_url))
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

    def current_player_item(self) -> dict:
        item = self.current_video_item or self.current_video_info or {}
        return item if isinstance(item, dict) else {}

    def item_is_local_media(self, item: dict | None) -> bool:
        if not isinstance(item, dict):
            return False
        if str(item.get("kind") or "").strip().lower() == "local_file":
            return True
        value = str(item.get("path") or item.get("url") or "").strip()
        return bool(value and self.local_media_path_from_input(value))

    def current_player_is_local_media(self) -> bool:
        return self.item_is_local_media(self.current_player_item())

    def player_copy_reference_label_key(self) -> str:
        return "copy_path" if self.current_player_is_local_media() else "copy_link"

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
        self.background_player_previous_control = None
        self.last_button_row_controls = []
        self.background_player_section_added = False
        self.background_player_section_pending = False
        self.background_player_section_generation += 1
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
        previous_value = getattr(ctrl, "_apricot_accessible_value", None)
        if ctrl.GetName() != name:
            ctrl.SetName(name)
        if ctrl.GetLabel() != name:
            ctrl.SetLabel(name)
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
        if (
            previous_value != value_text
            and getattr(ctrl, "_apricot_accessible_initialized", False)
            and not getattr(ctrl, "_apricot_suppress_accessible_notify", False)
        ):
            try:
                wx.Accessible.NotifyEvent(wx.ACC_EVENT_OBJECT_VALUECHANGE, ctrl, wx.OBJID_CLIENT, 0)
            except Exception:
                pass
        ctrl._apricot_accessible_initialized = True

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

    def ensure_window_visible(self) -> None:
        try:
            if not self.IsShown():
                self.Show()
            if self.IsIconized():
                self.Iconize(False)
            self.Raise()
        except RuntimeError:
            pass

    def activate_window(self) -> None:
        self.ensure_window_visible()
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
        suppress_results_selection = listbox is getattr(self, "results_list", None)
        if self.listbox_matches(listbox, labels):
            if current_selection != target_selection:
                if suppress_results_selection:
                    self.results_selection_update_suppressed = True
                try:
                    listbox.SetSelection(target_selection)
                finally:
                    if suppress_results_selection:
                        wx.CallAfter(self.clear_results_selection_update_suppression)
                return True
            return False
        listbox.Freeze()
        try:
            if suppress_results_selection:
                self.results_selection_update_suppressed = True
            listbox.Clear()
            for label in labels:
                listbox.Append(label)
            listbox.SetSelection(target_selection)
        finally:
            listbox.Thaw()
            if suppress_results_selection:
                wx.CallAfter(self.clear_results_selection_update_suppression)
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
        suppress_results_selection = listbox is getattr(self, "results_list", None)
        listbox.Freeze()
        try:
            if suppress_results_selection:
                self.results_selection_update_suppressed = True
            for label in labels[previous_count:]:
                listbox.Append(label)
            if listbox.GetSelection() != target_selection:
                listbox.SetSelection(target_selection)
        finally:
            listbox.Thaw()
            if suppress_results_selection:
                wx.CallAfter(self.clear_results_selection_update_suppression)
        return True

    def clear_results_selection_update_suppression(self) -> None:
        self.results_selection_update_suppressed = False

    def speak_text(self, text: str) -> None:
        if not text:
            return
        announced = False
        client = self.ensure_nvda_client()
        if client:
            try:
                if hasattr(client, "nvdaController_cancelSpeech"):
                    client.nvdaController_cancelSpeech()
                result = client.nvdaController_speakText(str(text))
                if result == 0:
                    announced = True
                if hasattr(client, "nvdaController_brailleMessage"):
                    braille_result = client.nvdaController_brailleMessage(str(text))
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

    def ensure_nvda_client(self):
        if not getattr(self, "nvda_client_load_attempted", False):
            self.nvda_client_load_attempted = True
            self.nvda_client = self.load_nvda_client()
        return self.nvda_client

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
        self.last_button_row_controls = list(created_buttons)
        if self.background_player_section_enabled() and not self.in_player_screen:
            self.add_background_player_section()
        return created_buttons

    def setup_taskbar_icon(self) -> None:
        if getattr(self, "exiting", False):
            return
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
        self.setup_taskbar_icon()
        self.Hide()
        self.announce_player(self.t("tray_still_running"))
        self.show_desktop_notification(APP_NAME, self.t("tray_still_running"), enabled=self.settings.tray_notification)

    def restore_from_tray(self) -> None:
        self.ensure_window_visible()
        try:
            self.RequestUserAttention(wx.USER_ATTENTION_INFO)
        except Exception:
            pass
        try:
            self.activate_window()
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
            path = str(payload.get("path") or "")
            if path:
                wx.CallAfter(self.open_local_media_file, path, True)
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
            (self.t("play_file"), self.show_play_file),
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

    def add_background_player_section(self, defer: bool = True) -> None:
        if self.background_player_section_added:
            return
        if defer and not self.in_player_screen:
            if self.background_player_section_pending:
                return
            if not self.background_player_section_enabled() or not self.player_is_active():
                return
            self.background_player_section_pending = True
            generation = self.background_player_section_generation
            wx.CallAfter(self.flush_background_player_section, generation)
            return
        self.background_player_controls = []
        self.background_player_section_pending = False
        if not self.background_player_section_enabled() or not self.player_is_active():
            return
        self.background_player_section_added = True
        self.background_player_previous_control = self.background_player_previous_target()
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
                    self.bind_player_navigation_control(self.player_panel)
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
            (self.t("fullscreen"), lambda: self.toggle_player_fullscreen(announce=True)),
            (self.t("bass_boost"), self.toggle_bass_boost),
            (self.t("repeat"), self.toggle_repeat),
            (self.t("shuffle"), self.toggle_shuffle),
            (self.t("copy_link"), self.copy_current_player_url),
            (self.t("close_player"), self.close_current_player),
        ]
        for label_text, handler in controls:
            button = wx.Button(self.panel, label=label_text)
            button.SetName(f"{self.t('background_player')}: {label_text}")
            button._apricot_background_player_handler = handler
            button.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            button.Bind(wx.EVT_KEY_DOWN, self.on_background_player_key)
            self.bind_player_navigation_control(button)
            if getattr(handler, "__name__", "") == "player_play_pause":
                self.player_play_pause_buttons.append(button)
            row.Add(button, 0, wx.RIGHT, 6)
            self.background_player_controls.append(button)
        self.root_sizer.Add(row, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        previous_control = self.live_window(self.background_player_previous_control)
        for control in self.background_player_controls:
            if previous_control is not None:
                try:
                    control.MoveAfterInTabOrder(previous_control)
                except RuntimeError:
                    pass
            previous_control = control

    def flush_background_player_section(self, generation: int) -> None:
        if generation != getattr(self, "background_player_section_generation", -1):
            return
        self.background_player_section_pending = False
        if self.in_player_screen:
            return
        self.add_background_player_section(defer=False)
        try:
            self.panel.Layout()
        except Exception:
            pass

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
        key = event.GetKeyCode()
        if key in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER, wx.WXK_SPACE} and not event.ControlDown() and not event.AltDown():
            control = event.GetEventObject()
            handler = getattr(control, "_apricot_background_player_handler", None)
            if callable(handler):
                handler()
                return
        self.on_char_hook(event)

    def bind_player_navigation_control(self, control: wx.Window | None) -> None:
        if control is None or getattr(control, "_apricot_navigation_bound", False):
            return
        try:
            control.Bind(wx.EVT_NAVIGATION_KEY, self.on_player_navigation_key)
            control._apricot_navigation_bound = True
        except Exception:
            pass

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

    @staticmethod
    def window_is_or_descendant(window: wx.Window | None, ancestor: wx.Window | None) -> bool:
        if window is None or ancestor is None:
            return False
        current = window
        while current is not None:
            if current is ancestor:
                return True
            try:
                current = current.GetParent()
            except RuntimeError:
                return False
            except Exception:
                return False
        return False

    def background_player_previous_target(self) -> wx.Window | None:
        for control in reversed(getattr(self, "last_button_row_controls", [])):
            target = self.live_window(control)
            if target is not None and not self.focus_in_background_player_controls(target):
                return target
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
        return self.move_background_player_tab_focus(not event.ShiftDown(), focus)

    def move_background_player_tab_focus(self, forward: bool, focus: wx.Window | None) -> bool:
        controls = [
            control
            for control in getattr(self, "background_player_controls", [])
            if control is not None and not getattr(control, "IsBeingDeleted", lambda: False)()
        ]
        if not controls or not self.window_is_or_descendant(focus, controls[0]):
            return False
        if forward and len(controls) > 1:
            self.safe_set_focus(controls[1])
            return True
        if not forward:
            target = self.live_window(self.background_player_previous_control) or self.background_player_previous_target()
            if target is None:
                return False
            self.safe_set_focus(target)
            return True
        return False

    def on_player_navigation_key(self, event: wx.NavigationKeyEvent) -> None:
        try:
            if event.IsWindowChange():
                event.Skip()
                return
            focus = event.GetCurrentFocus() or wx.Window.FindFocus()
            forward = bool(event.GetDirection())
        except Exception:
            event.Skip()
            return
        if self.move_background_player_tab_focus(forward, focus):
            return
        if self.move_player_tab_focus(forward, focus):
            return
        event.Skip()

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

    def focus_player_target_later(self, focus_target: str) -> bool:
        targets = {
            "player": "player_panel",
            "fullscreen_checkbox": "fullscreen_checkbox",
            "repeat_checkbox": "repeat_checkbox",
            "bass_boost_checkbox": "bass_boost_checkbox",
        }
        attr_name = targets.get(focus_target)
        if not attr_name:
            return False
        control = self.live_window(getattr(self, attr_name, None))
        if control is None:
            return False
        wx.CallAfter(self.safe_set_focus, control)
        wx.CallLater(100, self.safe_set_focus, control)
        wx.CallLater(300, self.safe_set_focus, control)
        return True

    def exit_fullscreen_to_player(self, focus_target: str = "player", announce: bool = False) -> None:
        if not self.player_is_active():
            self.back_to_results(stop_playback=False)
            return
        self.exit_fullscreen_window()
        self.show_player_page(self.current_player_title(), focus_target=focus_target)
        self.focus_player_target_later(focus_target)
        if announce:
            self.announce_player(self.t("fullscreen_off"))

    def enter_player_fullscreen(self, focus_target: str = "player", announce: bool = False) -> None:
        if not self.player_is_active():
            self.announce_player(self.t("no_player"))
            return
        self.player_fullscreen_session = True
        self.player_fullscreen_results_override = False
        try:
            self.show_player_page(self.current_player_title(), focus_target=focus_target)
            if self.player_kind == "mpv":
                self.mpv_request(["set_property", "fullscreen", True], timeout=0.5)
            self.ShowFullScreen(True)
            self.focus_player_target_later(focus_target)
            if announce:
                self.announce_player(self.t("fullscreen_on"))
        except Exception:
            try:
                self.ShowFullScreen(True)
            except Exception:
                pass
            if announce:
                self.announce_player(self.t("fullscreen_on"))

    def toggle_player_fullscreen(self, focus_target: str = "player", announce: bool = False) -> None:
        try:
            fullscreen_active = bool(self.player_fullscreen_mode_active() or self.IsFullScreen())
        except Exception:
            fullscreen_active = bool(self.player_fullscreen_mode_active())
        if fullscreen_active:
            self.exit_fullscreen_to_player(focus_target=focus_target, announce=announce)
        else:
            self.enter_player_fullscreen(focus_target=focus_target, announce=announce)

    def on_player_fullscreen_changed(self, _event=None) -> None:
        checked = bool(getattr(self, "fullscreen_checkbox", None) and self.fullscreen_checkbox.GetValue())
        if checked:
            self.enter_player_fullscreen(focus_target="fullscreen_checkbox", announce=True)
        else:
            self.exit_fullscreen_to_player(focus_target="fullscreen_checkbox", announce=True)

    def activate_menu(self) -> None:
        index = self.menu_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.menu_actions):
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
        self.clear_player_sequence()
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
            return any(path.is_file() and path.suffix.lower() in AUDIO_INPUT_EXTENSIONS for path in folder.rglob("*"))
        except OSError:
            return False

    def converter_media_files_in_folder(self, folder: Path) -> list[Path]:
        try:
            return sorted(
                path
                for path in folder.rglob("*")
                if path.is_file() and path.suffix.lower() in CONVERTER_MEDIA_EXTENSIONS and ".apricot-converting" not in path.name
            )
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

            output_mode_label = wx.StaticText(dialog, label=self.t("output_format"))
            create_new_box = wx.CheckBox(dialog, label=self.t("converter_create_new_folder" if folder_mode else "converter_create_new_file"))
            create_new_box.SetName(self.t("converter_create_new_folder" if folder_mode else "converter_create_new_file"))
            replace_box = wx.CheckBox(dialog, label=self.t("converter_replace_originals" if folder_mode else "converter_replace_original_file"))
            replace_box.SetName(self.t("converter_replace_originals" if folder_mode else "converter_replace_original_file"))
            create_new_box.SetValue(True)
            output_mode_row = wx.BoxSizer(wx.VERTICAL)
            output_mode_row.Add(create_new_box, 0, wx.BOTTOM, 3)
            output_mode_row.Add(replace_box, 0)
            form.Add(output_mode_label, 0, wx.ALIGN_TOP)
            form.Add(output_mode_row, 1, wx.EXPAND)

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

            def update_formats(_event=None) -> None:
                nonlocal target_values
                raw_path = path_ctrl.GetValue().strip().strip('"')
                path = Path(raw_path) if raw_path else Path()
                if folder_mode:
                    input_kind = ""
                else:
                    input_kind = self.converter_input_kind(path) if raw_path else ""
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
                update_formats()

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

            def on_create_new(_event=None) -> None:
                if create_new_box.GetValue():
                    replace_box.SetValue(False)
                elif not replace_box.GetValue():
                    create_new_box.SetValue(True)

            def on_replace(_event=None) -> None:
                if replace_box.GetValue():
                    create_new_box.SetValue(False)
                elif not create_new_box.GetValue():
                    replace_box.SetValue(True)

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
                    if replace_box.GetValue():
                        output_folder = source
                        replace_originals = True
                    else:
                        with wx.DirDialog(dialog, self.t("choose_output_folder"), defaultPath=str(source), style=wx.DD_DEFAULT_STYLE | wx.DD_DIR_MUST_EXIST) as chooser:
                            if chooser.ShowModal() != wx.ID_OK:
                                self.announce_player(self.t("conversion_cancelled"))
                                return
                            chosen = Path(chooser.GetPath()).expanduser()
                        output_folder = self.unique_folder_path(chosen / f"{source.name} converted")
                        replace_originals = False
                    self.start_folder_conversion(source, output_folder, target, image_path, replace_originals=replace_originals)
                else:
                    if not self.converter_input_kind(source):
                        self.message(self.t("unsupported_input_format"), wx.ICON_WARNING)
                        return
                    if replace_box.GetValue():
                        output = source.with_suffix(f".{self.converter_output_extension(target)}")
                        self.start_file_conversion(source, output, target, image_path, replace_original=True)
                    else:
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
                        self.start_file_conversion(source, output, target, image_path, replace_original=False)
                dialog.EndModal(wx.ID_OK)

            browse_button.Bind(wx.EVT_BUTTON, browse_path)
            image_button.Bind(wx.EVT_BUTTON, browse_image)
            path_ctrl.Bind(wx.EVT_TEXT, update_formats)
            target_choice.Bind(wx.EVT_CHOICE, lambda evt: update_audio_video_controls())
            add_image_box.Bind(wx.EVT_CHECKBOX, on_add_image)
            dark_box.Bind(wx.EVT_CHECKBOX, on_dark)
            create_new_box.Bind(wx.EVT_CHECKBOX, on_create_new)
            replace_box.Bind(wx.EVT_CHECKBOX, on_replace)
            convert_button.Bind(wx.EVT_BUTTON, convert)
            update_formats()
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

    def start_file_conversion(self, source: Path, output: Path, target_format: str, image_path: Path | None = None, replace_original: bool = False) -> None:
        output = output if replace_original else self.unique_converter_output_path(output, source)
        self.announce_player(self.t("conversion_started"))
        self.set_status(self.t("conversion_started"))
        threading.Thread(target=self.file_conversion_worker, args=(source, output, target_format, image_path, replace_original), daemon=True).start()

    def start_folder_conversion(self, source_folder: Path, output_folder: Path, target_format: str, image_path: Path | None = None, replace_originals: bool = False) -> None:
        self.announce_player(self.t("conversion_started"))
        self.set_status(self.t("conversion_started"))
        threading.Thread(target=self.folder_conversion_worker, args=(source_folder, output_folder, target_format, image_path, replace_originals), daemon=True).start()

    def file_conversion_worker(self, source: Path, output: Path, target_format: str, image_path: Path | None = None, replace_original: bool = False) -> None:
        try:
            ffmpeg = self.ffmpeg_executable()
            if not ffmpeg:
                raise RuntimeError("FFmpeg was not found")
            output.parent.mkdir(parents=True, exist_ok=True)
            final_output = output
            work_output = self.temporary_conversion_path(output) if replace_original else output
            args = self.converter_ffmpeg_args(ffmpeg, source, work_output, target_format, image_path)
            self.run_ffmpeg_conversion(args)
            if replace_original:
                self.replace_converted_original(source, work_output, final_output)
            done_text = self.t("conversion_done", title=final_output.name)
            wx.CallAfter(self.set_status, done_text)
            wx.CallAfter(self.finish_conversion_message, done_text)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("conversion_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def folder_conversion_worker(self, source_folder: Path, output_folder: Path, target_format: str, image_path: Path | None = None, replace_originals: bool = False) -> None:
        try:
            ffmpeg = self.ffmpeg_executable()
            if not ffmpeg:
                raise RuntimeError("FFmpeg was not found")
            files = self.converter_media_files_in_folder(source_folder)
            if not files:
                wx.CallAfter(self.message, self.t("conversion_no_media_files"), wx.ICON_INFORMATION)
                return
            if not replace_originals:
                output_folder.mkdir(parents=True, exist_ok=True)
            wx.CallAfter(self.show_conversion_progress_dialog, len(files))
            converted = 0
            failed = 0
            for index, source in enumerate(files, start=1):
                if replace_originals:
                    target = source.with_suffix(f".{self.converter_output_extension(target_format)}")
                    work_target = self.temporary_conversion_path(target)
                else:
                    try:
                        relative = source.relative_to(source_folder)
                    except ValueError:
                        relative = Path(source.name)
                    target = self.unique_converter_output_path(output_folder / relative.with_suffix(f".{self.converter_output_extension(target_format)}"), source)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    work_target = target
                self.ui_queue.put(("status", f"{self.t('conversion_started')} {index}/{len(files)}: {source.name}"))
                self.ui_queue.put(("conversion_progress", {"file": source.name, "converted": converted, "total": len(files)}))
                try:
                    args = self.converter_ffmpeg_args(ffmpeg, source, work_target, target_format, image_path)
                    self.run_ffmpeg_conversion(args)
                    if replace_originals:
                        self.replace_converted_original(source, work_target, target)
                    converted += 1
                except Exception:
                    failed += 1
                    continue
                self.ui_queue.put(("conversion_progress", {"file": source.name, "converted": converted, "total": len(files)}))
            text = self.t("conversion_folder_done_with_errors", count=converted, failed=failed) if failed else self.t("conversion_folder_done", count=converted)
            wx.CallAfter(self.set_status, text)
            wx.CallAfter(self.close_conversion_progress_dialog)
            wx.CallAfter(self.finish_conversion_message, text)
        except Exception as exc:
            wx.CallAfter(self.close_conversion_progress_dialog)
            wx.CallAfter(self.message, self.t("conversion_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def finish_conversion_message(self, text: str) -> None:
        if getattr(self.settings, "popup_when_conversion_complete", True):
            self.message(text, wx.ICON_INFORMATION)
        else:
            self.announce_player(text)

    def show_conversion_progress_dialog(self, total: int) -> None:
        self.close_conversion_progress_dialog()
        maximum = max(1, int(total or 1))
        self.conversion_progress_dialog = wx.ProgressDialog(
            self.t("conversion_progress_title"),
            self.t("conversion_progress_message", file="", converted=0, total=maximum, remaining=maximum),
            maximum=maximum,
            parent=self,
            style=wx.PD_ELAPSED_TIME | wx.PD_ESTIMATED_TIME | wx.PD_REMAINING_TIME,
        )

    def update_conversion_progress_dialog(self, payload: dict) -> None:
        dialog = self.conversion_progress_dialog
        if not dialog:
            return
        total = max(1, int(payload.get("total") or 1))
        converted = max(0, min(total, int(payload.get("converted") or 0)))
        remaining = max(0, total - converted)
        message = self.t("conversion_progress_message", file=str(payload.get("file") or ""), converted=converted, total=total, remaining=remaining)
        try:
            dialog.Update(converted, message)
        except RuntimeError:
            self.conversion_progress_dialog = None

    def close_conversion_progress_dialog(self) -> None:
        dialog = self.conversion_progress_dialog
        self.conversion_progress_dialog = None
        if dialog:
            try:
                dialog.Destroy()
            except RuntimeError:
                pass

    @staticmethod
    def unique_converter_output_path(path: Path, source: Path | None = None) -> Path:
        candidate = path
        counter = 2
        while candidate.exists() or (source is not None and candidate.resolve() == source.resolve()):
            candidate = path.with_name(f"{path.stem} ({counter}){path.suffix}")
            counter += 1
        return candidate

    @staticmethod
    def unique_folder_path(path: Path) -> Path:
        candidate = path
        counter = 2
        while candidate.exists():
            candidate = path.with_name(f"{path.name} ({counter})")
            counter += 1
        return candidate

    @staticmethod
    def temporary_conversion_path(path: Path) -> Path:
        return path.with_name(f"{path.stem}.apricot-converting{path.suffix}")

    @staticmethod
    def replace_converted_original(source: Path, work_output: Path, final_output: Path) -> None:
        if not work_output.exists():
            raise RuntimeError("Converted file was not created")
        if source.exists() and source.resolve() != final_output.resolve():
            source.unlink()
        if final_output.exists() and final_output.resolve() != work_output.resolve():
            final_output.unlink()
        if work_output.resolve() != final_output.resolve():
            shutil.move(str(work_output), str(final_output))

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
        playlist_actions = [
            (self.t("back"), self.show_main_menu),
            (self.t("create_playlist"), self.create_user_playlist_dialog),
        ]
        if self.user_playlists:
            playlist_actions.extend(
                [
                    (self.t("open_playlist"), self.open_selected_user_playlist),
                    (self.t("download_user_playlist"), self.download_selected_user_playlist),
                    (self.t("remove_playlist"), self.remove_selected_user_playlist),
                ]
            )
        self.add_button_row(playlist_actions)
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
        actions = [(self.menu_label_with_shortcut("create_playlist", "create_playlist"), self.create_user_playlist_dialog)]
        if self.user_playlists:
            actions = [
                (self.t("open_playlist"), self.open_selected_user_playlist),
                *actions,
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
        if self.user_playlists_screen_active:
            wx.CallAfter(self.show_user_playlists)
        else:
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
        playlist = self.user_playlists[playlist_index]
        item_actions = [(self.t("back"), self.show_user_playlists)]
        if playlist.get("items"):
            item_actions.extend(
                [
                    (self.t("play"), self.play_selected_user_playlist_item),
                    (self.t("download_user_playlist"), self.download_current_user_playlist),
                    (self.t("remove_from_playlist"), self.remove_selected_user_playlist_item),
                ]
            )
        self.add_button_row(item_actions)
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
        if not selected:
            item = menu.Append(wx.ID_ANY, self.t("playlist_empty"))
            item.Enable(False)
            self.PopupMenu(menu)
            menu.Destroy()
            return
        actions = [
            (self.t("play"), self.play_selected_user_playlist_item),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda selected=dict(selected): self.start_download(True, item=selected)),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda selected=dict(selected): self.start_download(False, item=selected)),
            (self.t("download_user_playlist"), self.download_current_user_playlist),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playlist", "remove_from_playlist"), self.remove_selected_user_playlist_item),
            (self.t("copy_url"), lambda selected=dict(selected): self.copy_item_url(selected)),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda selected=dict(selected): self.copy_direct_stream_url(selected)),
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
        self.clear_player_sequence()
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
            if self.user_playlist_items_screen_active and not items:
                wx.CallAfter(self.show_user_playlist_items, self.current_user_playlist_index)
            else:
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
        if self.user_playlists_screen_active and not self.user_playlists:
            wx.CallAfter(self.show_user_playlists)
        else:
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
            "live_status",
            "is_live",
            "kind",
            "url",
            "webpage_url",
        ]
        playlist_item = {key: item.get(key, "") for key in keys}
        playlist_item["kind"] = playlist_item.get("kind") or "video"
        playlist_item["type"] = self.item_type_label(playlist_item)
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
        self.in_player_screen = False
        if not self.player_is_active():
            self.player_control_mode = False
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
        self.results_list.SetName(self.t("result_list"))
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
            type=self.item_type_label(item),
        )

    def on_results_selection(self, event) -> None:
        event.Skip()
        selection = self.current_results_selection(-1)
        if not getattr(self, "results_selection_update_suppressed", False):
            self.remember_user_result_selection(selection)
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
        favorite_actions = [(self.t("back"), self.show_main_menu)]
        if self.favorites:
            favorite_actions.extend(
                [
                    (self.t("play"), self.play_favorite),
                    (self.t("remove"), self.remove_favorite),
                    (self.t("refresh"), self.refresh_favorites),
                ]
            )
        self.add_button_row(favorite_actions)
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
        if not selected:
            item = menu.Append(wx.ID_ANY, self.t("favorites_empty"))
            item.Enable(False)
            self.PopupMenu(menu)
            menu.Destroy()
            return
        actions = [
            (self.t("play"), self.play_favorite),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda selected=dict(selected): self.start_download(True, item=selected)),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda selected=dict(selected): self.start_download(False, item=selected)),
            (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), lambda selected=dict(selected): self.subscribe_to_selected_channel(selected)),
            (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), lambda selected=dict(selected): self.unsubscribe_from_selected_channel(selected)),
            (self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"), self.add_active_to_playlist),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda selected=dict(selected): self.copy_direct_stream_url(selected)),
            (self.t("copy_url"), lambda selected=dict(selected): self.copy_item_url(selected)),
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
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
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
        self.ensure_rss_feeds_loaded()
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
            ]
        )
        self.add_button_row(
            [
                (self.t("open_feed"), self.open_selected_rss_feed),
                (self.t("remove_feed"), self.remove_rss_feed),
                (self.t("import_opml"), self.import_rss_from_opml),
                (self.t("export_opml"), self.export_rss_to_opml),
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
        self.ensure_rss_feeds_loaded()
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

    def export_rss_to_opml(self) -> None:
        self.ensure_rss_feeds_loaded()
        if not self.rss_feeds:
            self.message(self.t("opml_no_feeds"))
            return
        
        start_dir = self.settings.download_folder or str(Path.home())
        with wx.FileDialog(
            self,
            self.t("export_opml"),
            defaultDir=start_dir if Path(start_dir).exists() else str(Path.home()),
            defaultFile="apricot_feeds.opml",
            wildcard=f"{self.t('opml_files')} (*.opml)|*.opml",
            style=wx.FD_SAVE | wx.FD_OVERWRITE_PROMPT,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            path = dialog.GetPath()
            
        try:
            opml = ET.Element("opml", version="2.0")
            head = ET.SubElement(opml, "head")
            title = ET.SubElement(head, "title")
            title.text = "ApricotPlayer RSS Feeds Export"
            
            body = ET.SubElement(opml, "body")
            outline_parent = ET.SubElement(body, "outline", text="Apricot RSS Feeds", title="Apricot RSS Feeds")
            
            for feed in self.rss_feeds:
                feed_title = feed.get("title") or "Unknown Feed"
                feed_url = feed.get("url") or ""
                if not feed_url:
                    continue
                ET.SubElement(
                    outline_parent,
                    "outline",
                    type="rss",
                    text=feed_title,
                    title=feed_title,
                    xmlUrl=feed_url,
                    htmlUrl=feed_url,
                )
                
            tree = ET.ElementTree(opml)
            ET.indent(tree, space="  ", level=0)
            with open(path, "wb") as f:
                tree.write(f, encoding="utf-8", xml_declaration=True)
                
            self.message(self.t("opml_export_success"))
        except Exception as exc:
            self.message(self.t("opml_export_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def import_rss_from_opml(self) -> None:
        start_dir = self.settings.download_folder or str(Path.home())
        with wx.FileDialog(
            self,
            self.t("import_opml"),
            defaultDir=start_dir if Path(start_dir).exists() else str(Path.home()),
            wildcard=f"{self.t('opml_files')} (*.opml)|*.opml",
            style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            path = dialog.GetPath()
            
        try:
            tree = ET.parse(path)
            root = tree.getroot()
            
            feed_urls = []
            for outline in root.findall(".//outline"):
                xml_url = outline.get("xmlUrl")
                if xml_url:
                    xml_url = xml_url.strip()
                    title = outline.get("title") or outline.get("text") or "RSS Feed"
                    if xml_url:
                        feed_urls.append((xml_url, title))
                        
            if not feed_urls:
                self.message(self.t("opml_no_feeds"), wx.ICON_WARNING)
                return
                
            self.ensure_rss_feeds_loaded()
            existing_urls = {str(feed.get("url") or "").rstrip("/").lower() for feed in self.rss_feeds}
            
            to_import = []
            for url, title in feed_urls:
                normalized = url.rstrip("/").lower()
                if normalized not in existing_urls:
                    to_import.append((url, title))
                    existing_urls.add(normalized)
                    
            if not to_import:
                self.message(self.t("opml_all_feeds_exist"))
                return
                
            self.announce_player(self.t("opml_import_started", count=len(to_import)))
            threading.Thread(target=self.import_opml_worker, args=(to_import,), daemon=True).start()
        except Exception as exc:
            self.message(self.t("opml_import_failed_msg", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def import_opml_worker(self, to_import: list[tuple[str, str]]) -> None:
        imported_count = 0
        failed_count = 0
        
        for i, (url, title) in enumerate(to_import):
            self.ui_queue.put(("announce", self.t("opml_import_progress", current=i + 1, total=len(to_import), title=title)))
            try:
                feed = self.fetch_rss_feed(url)
                self.rss_feeds.append(feed)
                imported_count += 1
            except Exception:
                failed_count += 1
                
        if imported_count > 0:
            self.save_rss_feeds()
            self.ui_queue.put(("rss_feeds_changed", None))
            
        if failed_count == 0:
            self.ui_queue.put(("announce", self.t("opml_import_done", count=imported_count)))
        else:
            self.ui_queue.put(("announce", self.t("opml_import_done_with_errors", imported=imported_count, failed=failed_count)))

    def refresh_all_rss_feeds(self) -> None:
        self.ensure_rss_feeds_loaded()
        if not self.rss_feeds:
            self.announce_player(self.t("rss_feeds_empty"))
            return
        if self.rss_refresh_running:
            return
        self.rss_refresh_running = True
        self.announce_player(self.t("rss_refresh_started"))
        threading.Thread(target=self.refresh_rss_feeds_worker, args=(None, False), daemon=True).start()

    def refresh_all_rss_feeds_background(self) -> None:
        if not self.settings.enable_podcasts_rss:
            return
        self.ensure_rss_feeds_loaded()
        if not self.rss_feeds or self.rss_refresh_running:
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
        self.ensure_rss_feeds_loaded()
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
        self.clear_player_sequence()
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
        ET = import_module("xml.etree.ElementTree")
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
        chapters_url, chapters_type = self.podcast_chapters_reference(item, base_url)
        return {
            "title": title or page_url or media_url,
            "url": url,
            "webpage_url": page_url or url,
            "media_url": media_url,
            "description": description,
            "duration": duration,
            "chapters": self.parse_inline_podcast_chapters(item),
            "chapters_url": chapters_url,
            "chapters_type": chapters_type,
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
        chapters_url, chapters_type = self.podcast_chapters_reference(entry, base_url)
        return {
            "title": title or page_url or media_url,
            "url": url,
            "webpage_url": page_url or url,
            "media_url": media_url,
            "description": description,
            "duration": duration,
            "chapters": self.parse_inline_podcast_chapters(entry),
            "chapters_url": chapters_url,
            "chapters_type": chapters_type,
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

    def parse_inline_podcast_chapters(self, element: ET.Element) -> list[dict]:
        raw_chapters: list[dict] = []
        for chapters_element in self.children(element, "chapters"):
            chapter_children = self.children(chapters_element, "chapter")
            if not chapter_children:
                continue
            for chapter in chapter_children:
                raw_chapters.append(
                    {
                        "start": chapter.get("start") or chapter.get("time") or self.child_text(chapter, "start"),
                        "end": chapter.get("end") or self.child_text(chapter, "end"),
                        "title": chapter.get("title") or self.child_text(chapter, "title") or (chapter.text or ""),
                    }
                )
        return self.normalized_chapters(raw_chapters)

    def podcast_chapters_reference(self, element: ET.Element, base_url: str) -> tuple[str, str]:
        for child in self.children(element, "chapters"):
            url = str(child.get("url") or child.get("href") or "").strip()
            if url:
                return self.absolute_url(url, base_url), str(child.get("type") or "").strip()
        return "", ""

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
        self.focus_settings_section_list_later()

    def focus_settings_section_list_later(self) -> None:
        generation = self.settings_render_generation
        self.settings_initial_focus_pending = True
        wx.CallAfter(self.focus_settings_section_list_if_safe, generation)
        wx.CallLater(100, self.focus_settings_section_list_if_safe, generation)

    def focus_settings_section_list_if_safe(self, generation: int) -> None:
        if not getattr(self, "settings_initial_focus_pending", False):
            return
        if generation != getattr(self, "settings_render_generation", -1):
            return
        target = self.live_window(getattr(self, "settings_section_list", None))
        if target is None:
            return
        focus = wx.Window.FindFocus()
        if focus is target:
            self.settings_initial_focus_pending = False
            return
        if focus is not None:
            for control in getattr(self, "settings_control_order", []):
                if self.window_is_or_descendant(focus, control):
                    self.settings_initial_focus_pending = False
                    return
        self.settings_initial_focus_pending = False
        self.safe_set_focus(target)

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
                "enable_stream_url_cache",
                "stream_url_cache_minutes",
                "prefetch_next_stream_url",
                "gapless_playback",
                "replaygain_mode",
                "enable_online_lyrics",
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
                "equalizer_clipping_protection",
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
                "popup_when_conversion_complete",
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
        key = event.GetKeyCode()
        if key == wx.WXK_TAB and not event.ShiftDown():
            self.settings_initial_focus_pending = False
            self.flush_settings_section_render()
            self.focus_first_settings_control()
            return
        if key == wx.WXK_RETURN:
            self.flush_settings_section_render()
            self.focus_first_settings_control()
        else:
            event.Skip()

    def focus_first_settings_control(self) -> None:
        if self.settings_control_order:
            self.safe_set_focus(self.settings_control_order[0])

    def apply_settings_tab_order(self) -> None:
        section_list = self.live_window(getattr(self, "settings_section_list", None))
        settings_scroller = self.live_window(getattr(self, "settings_scroller", None))
        if section_list is not None and settings_scroller is not None:
            try:
                settings_scroller.MoveAfterInTabOrder(section_list)
            except Exception:
                pass
        self.apply_tab_order(list(getattr(self, "settings_control_order", [])))

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
        if section_name != "equalizer":
            self.visible_equalizer_draft_gains = {}

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

        def slider(key: str, label: str, value: float, minimum: int, maximum: int, band_id: str | None = None):
            form.Add(wx.StaticText(self.settings_scroller, label=label), 0, wx.ALIGN_CENTER_VERTICAL)
            scaled_value = int(round(float(value) * 10))
            ctrl = wx.Slider(
                self.settings_scroller,
                value=min(max(scaled_value, minimum), maximum),
                minValue=minimum,
                maxValue=maximum,
                style=wx.SL_HORIZONTAL,
            )
            if band_id:
                ctrl._apricot_eq_band_id = str(band_id)
            self.configure_equalizer_slider_steps(ctrl)
            self.set_equalizer_slider_accessibility(ctrl, label)
            self.bind_equalizer_slider_events(ctrl, lambda evt, label_text=label: self.on_equalizer_settings_slider(evt, label_text))
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
            button("check_ytdlp_updates_now", self.manual_ytdlp_update_check)
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
            check("enable_stream_url_cache", bool(getattr(self.settings, "enable_stream_url_cache", True)))
            choice(
                "stream_url_cache_minutes",
                str(self.normalized_stream_url_cache_minutes()),
                STREAM_URL_CACHE_OPTIONS,
                self.stream_url_cache_labels(STREAM_URL_CACHE_OPTIONS),
            )
            check("prefetch_next_stream_url", bool(getattr(self.settings, "prefetch_next_stream_url", True)))
            check("gapless_playback", bool(getattr(self.settings, "gapless_playback", True)))
            choice("replaygain_mode", self.normalized_replaygain_mode(), REPLAYGAIN_MODE_OPTIONS, self.replaygain_mode_labels())
            check("enable_online_lyrics", bool(getattr(self.settings, "enable_online_lyrics", True)))
            text("cache_folder", self.settings.cache_folder or str(DEFAULT_CACHE_DIR))
            choice("cache_size_mb", str(self.settings.cache_size_mb), ["128", "256", "512", "1024", "2048", "4096"])
            check("resume_playback", self.settings.resume_playback)
            device_values, device_labels = self.audio_output_device_options(allow_probe=False)
            choice("default_audio_device", self.normalized_audio_output_device(), device_values, device_labels)
            self.refresh_audio_output_devices_async()
            choice("seek_seconds", self.format_seek_seconds_value(self.seek_seconds_value()), SEEK_SECONDS_OPTIONS)
            choice("volume_step", str(self.settings.volume_step), ["1", "2", "5", "10"])
            int_slider("default_volume", self.t("default_volume"), self.default_volume_value(), 0, self.default_volume_max_value())
            volume_boost_default_box = check("volume_boost_by_default", bool(getattr(self.settings, "volume_boost_by_default", False)))
            volume_boost_default_box.Bind(wx.EVT_CHECKBOX, self.on_volume_boost_by_default_settings_changed)
            check("autoplay_next", self.settings.autoplay_next)
            check("browser_playback", self.settings.prefer_browser_playback)
            check("fullscreen", self.settings.player_fullscreen)
            check("start_paused", self.settings.player_start_paused)
            check("announce_play_pause", self.settings.announce_play_pause)
            check("announce_playback_finished", bool(getattr(self.settings, "announce_playback_finished", True)))
            check("enable_background_playback", bool(getattr(self.settings, "enable_background_playback", False)))
        elif section_name == "equalizer":
            self.equalizer_controls_loading = True
            try:
                equalizer_enabled = bool(getattr(self.settings, "global_equalizer_enabled", False))
                enabled_box = check("global_equalizer", equalizer_enabled)
                enabled_box.Bind(wx.EVT_CHECKBOX, self.on_global_equalizer_toggle)
                clipping_box = check("equalizer_clipping_protection", bool(getattr(self.settings, "equalizer_clipping_protection", False)))
                clipping_box.Bind(wx.EVT_CHECKBOX, self.on_equalizer_clipping_protection_changed)
                if not equalizer_enabled:
                    self.visible_equalizer_draft_gains = {}
                if equalizer_enabled:
                    preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
                    self.visible_equalizer_preset = preset
                    preset_choice = choice("equalizer_preset", preset, self.equalizer_preset_options(), self.equalizer_preset_labels())
                    preset_choice.Bind(wx.EVT_CHOICE, self.on_equalizer_settings_preset_changed)
                    if self.is_custom_equalizer_preset(preset):
                        name_ctrl = text("equalizer_preset_name", self.equalizer_custom_name(preset))
                        name_ctrl.Bind(wx.EVT_KILL_FOCUS, self.on_equalizer_settings_name_changed)
                    db_range = str(self.equalizer_db_range_value())
                    range_choice = choice("equalizer_db_range", db_range, EQ_RANGE_OPTIONS)
                    range_choice.Bind(wx.EVT_CHOICE, self.on_equalizer_range_changed)
                    gains = self.equalizer_gains_for_preset(preset)
                    self.visible_equalizer_draft_gains = self.normalized_equalizer_gains(gains)
                    slider_min = -int(db_range) * 10
                    slider_max = int(db_range) * 10
                    for band_id, band_label in EQ_BANDS:
                        label = self.t("equalizer_band_gain", band=band_label)
                        slider(f"eq_{band_id}", label, gains.get(band_id, 0.0), slider_min, slider_max, band_id=band_id)
                    button("reset_equalizer", self.reset_visible_equalizer_controls)
                    button("add_equalizer_profile", self.add_equalizer_profile_from_settings)
                    if self.is_custom_equalizer_preset(preset):
                        button("delete_equalizer_profile", self.delete_visible_equalizer_profile_from_settings)
            finally:
                self.equalizer_controls_loading = False
        elif section_name == "downloads":
            check("confirm_download", self.settings.confirm_before_download)
            check("open_after_download", self.settings.open_folder_after_download)
            check("download_complete_popup", self.settings.popup_when_download_complete)
            check("conversion_complete_popup", bool(getattr(self.settings, "popup_when_conversion_complete", True)))
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
            button("obtain_youtube_api_key", self.open_youtube_api_key_page_from_settings)
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
        self.apply_settings_tab_order()
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
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
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

    def set_status_if_current(self, generation: int, text: str) -> None:
        if generation == self.search_generation:
            self.set_status(text)

    @staticmethod
    def metadata_live_status(info: dict | None) -> str:
        if not isinstance(info, dict):
            return ""
        snippet = info.get("snippet") if isinstance(info.get("snippet"), dict) else {}
        value = info.get("live_status") or info.get("liveBroadcastContent") or snippet.get("liveBroadcastContent") or ""
        return str(value or "").strip().lower().replace("-", "_")

    @staticmethod
    def metadata_bool(value) -> bool:
        if isinstance(value, str):
            return value.strip().lower() in {"1", "true", "yes", "y", "live", "is_live"}
        return bool(value)

    @classmethod
    def metadata_is_live_stream(cls, info: dict | None) -> bool:
        if not isinstance(info, dict):
            return False
        return cls.metadata_live_status(info) in {"is_live", "live"} or cls.metadata_bool(info.get("is_live"))

    def item_type_label(self, item: dict | None, default: str | None = None) -> str:
        if isinstance(item, dict) and str(item.get("kind") or "video") == "video" and self.metadata_is_live_stream(item):
            return self.t("live_stream")
        return str((item or {}).get("type") or default or self.t("video"))

    @staticmethod
    def parse_chapter_seconds(value) -> float | None:
        if value is None or value == "":
            return None
        if isinstance(value, bool):
            return None
        if isinstance(value, (int, float)):
            return max(0.0, float(value))
        text = str(value).strip().replace(",", ".")
        if not text:
            return None
        if re.fullmatch(r"\d+(?:\.\d+)?", text):
            return max(0.0, float(text))
        parts = text.split(":")
        if 1 < len(parts) <= 3:
            try:
                total = 0.0
                for part in parts:
                    total = total * 60.0 + float(part)
                return max(0.0, total)
            except ValueError:
                return None
        return None

    def normalized_chapters(self, raw_chapters) -> list[dict]:
        chapters: list[dict] = []
        if not isinstance(raw_chapters, list):
            return chapters
        for index, chapter in enumerate(raw_chapters):
            if not isinstance(chapter, dict):
                continue
            start = chapter.get("start_time", chapter.get("time", chapter.get("start", chapter.get("startTime"))))
            end = chapter.get("end_time", chapter.get("end", chapter.get("endTime")))
            start_value = self.parse_chapter_seconds(start)
            if start_value is None:
                continue
            end_value = self.parse_chapter_seconds(end)
            title = str(chapter.get("title") or chapter.get("name") or self.t("chapters")).strip()
            if not title:
                title = f"{self.t('chapters')} {index + 1}"
            normalized = {
                "title": title,
                "start_time": round(start_value, 3),
            }
            if end_value is not None and end_value > start_value:
                normalized["end_time"] = round(end_value, 3)
            chapters.append(normalized)
        return sorted(chapters, key=lambda item: float(item.get("start_time") or 0.0))

    def extract_youtube_video_id(self, item: dict | None = None) -> str:
        item = item or self.current_video_info or self.current_video_item or {}
        video_id = str((item or {}).get("id") or "").strip()
        if video_id and re.fullmatch(r"[\w-]{8,}", video_id):
            return video_id
        url = str((item or {}).get("url") or (item or {}).get("webpage_url") or "").strip()
        if not url:
            return ""
        try:
            parsed = urlparse(url)
        except Exception:
            return ""
        host = (parsed.netloc or "").lower()
        if "youtu.be" in host:
            return parsed.path.strip("/").split("/", 1)[0]
        if "youtube.com" not in host:
            return ""
        query_id = (parse_qs(parsed.query).get("v") or [""])[0]
        if query_id:
            return query_id
        match = re.search(r"/(?:shorts|embed|live)/([\w-]+)", parsed.path or "")
        return match.group(1) if match else ""

    def youtube_comments_source_url(self, item: dict | None, video_id: str) -> str:
        item = item if isinstance(item, dict) else {}
        for key in ("webpage_url", "original_url", "watch_url", "url"):
            url = str(item.get(key) or "").strip()
            if not url:
                continue
            try:
                host = (urlparse(url).netloc or "").lower()
            except Exception:
                continue
            if "googlevideo.com" in host or "youtubei.googleapis.com" in host:
                continue
            if "youtube.com" not in host and "youtu.be" not in host:
                continue
            if self.extract_youtube_video_id({"url": url}) == video_id:
                return url
        return f"https://www.youtube.com/watch?v={video_id}"

    def with_live_stream_display_fields(self, item: dict, source: dict | None = None) -> dict:
        source = source if isinstance(source, dict) else item
        live_status = self.metadata_live_status(source) or self.metadata_live_status(item)
        is_live = self.metadata_is_live_stream(source) or self.metadata_is_live_stream(item)
        if live_status:
            item["live_status"] = live_status
        item["is_live"] = bool(is_live)
        if str(item.get("kind") or "video") == "video" and is_live:
            item["type"] = self.t("live_stream")
            item["age"] = self.t("live_now")
        elif str(item.get("kind") or "video") == "video":
            item["type"] = item.get("type") or self.t("video")
        return item

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
            display_type = self.t("live_stream") if self.metadata_is_live_stream(entry) else self.t("video")
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
        is_live = kind == "video" and self.metadata_is_live_stream(entry)
        age = self.t("live_now") if is_live else (self.format_age({"timestamp": timestamp, "upload_date": upload_date}) if kind == "video" else "")
        playlist_count = entry.get("playlist_count") or entry.get("n_entries") or entry.get("video_count") or entry.get("playlist_count_text")
        item = {
            "title": entry.get("title") or "",
            "id": entry.get("id") or "",
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
            "artist": entry.get("artist") or entry.get("creator") or "",
            "track": entry.get("track") or "",
            "album": entry.get("album") or "",
            "chapters": self.normalized_chapters(entry.get("chapters")),
            "type": display_type,
            "kind": kind,
            "playlist_count": playlist_count if kind == "playlist" else "",
            "url": url,
            "live_status": self.metadata_live_status(entry),
            "is_live": is_live,
        }
        return self.with_live_stream_display_fields(item, entry)

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
            self.remember_user_result_selection(selected_index)
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

    def remember_user_result_selection(self, index: int | None = None) -> None:
        if not self.results:
            self.last_user_result_index = 0
            self.last_user_result_identity = ""
            return
        if index is None:
            index = self.current_results_selection(self.last_user_result_index)
        index = min(max(0, int(index)), len(self.results) - 1)
        self.last_user_result_index = index
        self.last_user_result_identity = self.result_identity_at(index)

    def result_identity_at(self, index: int) -> str:
        if index < 0 or index >= len(self.results):
            return ""
        return self.result_identity_for_item(self.results[index])

    @staticmethod
    def result_identity_for_item(item: dict | None) -> str:
        item = item or {}
        return str(item.get("url") or item.get("webpage_url") or item.get("id") or item.get("title") or "")

    def result_index_for_identity(self, identity: str, fallback: int, limit: int | None = None) -> int:
        if identity:
            items = self.results[:limit] if limit is not None else self.results
            for index, item in enumerate(items):
                if self.result_identity_for_item(item) == identity:
                    return index
        if not self.results:
            return 0
        return min(max(0, fallback), len(self.results) - 1)

    def stable_selected_result(self) -> dict | None:
        if not hasattr(self, "results_list") or not self.results:
            return None
        selection = self.current_results_selection(self.last_user_result_index)
        if self.focus_in_results_control(wx.Window.FindFocus()) and self.last_user_result_identity:
            selection = self.result_index_for_identity(self.last_user_result_identity, self.last_user_result_index)
        if selection < 0 or selection >= len(self.results):
            return None
        self.current_index = selection
        try:
            if self.results_list.GetSelection() != selection:
                self.results_selection_update_suppressed = True
                self.results_list.SetSelection(selection)
                wx.CallAfter(self.clear_results_selection_update_suppression)
        except RuntimeError:
            pass
        return self.results[selection]

    def search_return_data(self, index: int | None = None) -> dict:
        return {
            "index": self.current_index if index is None else int(index),
            "collection_url": str(self.collection_url or ""),
            "collection_result_type": str(self.collection_result_type or ""),
            "collection_sort_mode": str(self.collection_sort_mode or ""),
            "collection_channel_id": str(self.collection_channel_id or ""),
            "collection_fully_loaded": bool(self.collection_fully_loaded),
            "dynamic_fetch_enabled": bool(self.dynamic_fetch_enabled),
        }

    def restore_search_return_context(self, data: dict | None = None) -> None:
        data = data if isinstance(data, dict) else {}
        self.collection_url = str(data.get("collection_url") or "")
        self.collection_result_type = str(data.get("collection_result_type") or "")
        self.collection_sort_mode = str(data.get("collection_sort_mode") or "")
        self.collection_channel_id = str(data.get("collection_channel_id") or "")
        self.collection_fully_loaded = bool(data.get("collection_fully_loaded", False))
        self.dynamic_fetch_enabled = bool(data.get("dynamic_fetch_enabled", True))
        self.loading_more_results = False

    def maybe_extend_results(self) -> None:
        if not self.dynamic_fetch_enabled or self.settings.results_limit != 0 or not hasattr(self, "results_list"):
            return
        if not self.results and not self.all_results:
            return
        try:
            selection = self.results_list.GetSelection()
        except RuntimeError:
            return
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
        self.remember_user_result_selection(selected_index)
        self.set_status(self.t("search_more_loaded", count=len(self.results)))
        self.start_result_metadata_hydration()

    def fetch_more_dynamic_results(self, selection: int) -> None:
        if self.loading_more_results:
            return
        if getattr(self, "collection_fully_loaded", False) and len(self.results) >= len(self.all_results):
            self.set_status(self.t("no_more_results"))
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
            threading.Thread(
                target=self.load_collection_worker,
                args=(self.collection_url, self.collection_result_type or "Video", next_limit, selection, generation, self.collection_sort_mode),
                daemon=True,
            ).start()
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

    def merge_dynamic_results(self, existing: list[dict], fetched: list[dict], anchor_identity: str = "") -> list[dict]:
        existing_items = [dict(item) for item in existing]
        fetched_items = [dict(item) for item in fetched]
        if not existing_items:
            return fetched_items
        merged = list(existing_items)
        seen = {identity for identity in (self.result_identity_for_item(item) for item in merged) if identity}
        ordered_fetched = list(fetched_items)
        if anchor_identity:
            anchor_index = next(
                (index for index, item in enumerate(fetched_items) if self.result_identity_for_item(item) == anchor_identity),
                -1,
            )
            if anchor_index >= 0:
                ordered_fetched = fetched_items[anchor_index + 1 :] + fetched_items[: anchor_index + 1]
        for item in ordered_fetched:
            identity = self.result_identity_for_item(item)
            if identity and identity in seen:
                continue
            merged.append(item)
            if identity:
                seen.add(identity)
        return merged

    def show_more_results(self, results: list[dict], selection: int) -> None:
        self.loading_more_results = False
        current_selection = self.current_results_selection(selection)
        current_identity = str(self.pending_player_next_current_url or "") if self.pending_player_next_after_dynamic_load else self.result_identity_at(current_selection)
        merged_results = self.merge_dynamic_results(self.all_results, results, current_identity)
        if len(merged_results) <= len(self.all_results):
            self.set_status(self.t("no_more_results"))
            if self.pending_player_next_after_dynamic_load:
                self.pending_player_next_after_dynamic_load = False
                self.pending_player_next_preserve_focus = False
                self.pending_player_next_current_url = ""
                self.announce_player(self.t("no_next_item"))
            return
        previous_count = len(self.results)
        self.all_results = list(merged_results)
        visible_count = min(len(self.all_results), max(previous_count, previous_count + RESULTS_PAGE_SIZE))
        self.last_visible_count = visible_count
        self.results = self.all_results[:visible_count]
        selected_index = self.result_index_for_identity(current_identity, current_selection, visible_count)
        labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
        if not self.append_listbox_items(self.results_list, labels, previous_count, selected_index):
            self.set_listbox_items(self.results_list, labels, selected_index)
        self.remember_user_result_selection(selected_index)
        self.set_status(self.t("search_more_loaded", count=len(self.results)))
        self.start_result_metadata_hydration()
        if self.player_return_screen in {"search", "trending"}:
            self.return_all_results = list(self.all_results)
            self.return_results = list(self.results)
            self.return_visible_count = self.last_visible_count
        self.finish_pending_player_next_after_dynamic_load()

    def show_more_results_if_current(self, generation: int, results: list[dict], selection: int) -> None:
        if generation == self.search_generation:
            self.show_more_results(results, selection)

    def dynamic_fetch_failed(self, error: str) -> None:
        self.loading_more_results = False
        if self.pending_player_next_after_dynamic_load:
            self.pending_player_next_after_dynamic_load = False
            self.pending_player_next_preserve_focus = False
            self.pending_player_next_current_url = ""
            self.set_status(error)
            self.announce_player(error)
            return
        self.set_status(error)
        self.announce_player(error)

    def dynamic_fetch_failed_if_current(self, generation: int, error: str) -> None:
        if generation == self.search_generation:
            self.dynamic_fetch_failed(error)

    def request_player_next_dynamic_load(self, preserve_focus: bool = False) -> bool:
        if self.player_return_screen not in {"search", "trending"}:
            return False
        if not self.dynamic_fetch_enabled or self.settings.results_limit != 0 or not hasattr(self, "results_list"):
            return False
        if self.loading_more_results:
            self.pending_player_next_after_dynamic_load = True
            self.pending_player_next_preserve_focus = bool(preserve_focus)
            self.pending_player_next_current_url = str((self.current_video_item or {}).get("url") or "")
            return True
        current_count = len(self.all_results)
        if current_count <= 0:
            return False
        max_limit = self.max_results_limit()
        if max_limit and current_count >= max_limit:
            return False
        if getattr(self, "collection_fully_loaded", False) and len(self.results) >= len(self.all_results):
            return False
        self.pending_player_next_after_dynamic_load = True
        self.pending_player_next_preserve_focus = bool(preserve_focus)
        self.pending_player_next_current_url = str((self.current_video_item or {}).get("url") or "")
        selection = max(0, len(self.results) - 1)
        self.fetch_more_dynamic_results(selection)
        return True

    def finish_pending_player_next_after_dynamic_load(self) -> None:
        if not self.pending_player_next_after_dynamic_load:
            return
        preserve_focus = bool(self.pending_player_next_preserve_focus)
        current_url = str(self.pending_player_next_current_url or "")
        self.pending_player_next_after_dynamic_load = False
        self.pending_player_next_preserve_focus = False
        self.pending_player_next_current_url = ""
        if not self.player_is_active():
            return
        next_item = self.next_player_item_after_url(current_url)
        if next_item:
            wx.CallAfter(self.open_relative_player_item, next_item, True, preserve_focus)
            return
        self.announce_player(self.t("no_next_item"))

    def next_player_item_after_url(self, current_url: str) -> dict | None:
        if not current_url:
            return None
        playable = [item for item in self.player_navigation_results() if item.get("kind") not in {"channel", "playlist"}]
        current_pos = next((index for index, item in enumerate(playable) if str(item.get("url") or "") == current_url), -1)
        target = current_pos + 1
        if 0 <= current_pos and target < len(playable):
            return dict(playable[target])
        return None

    def start_result_metadata_hydration(self) -> None:
        candidates: list[dict] = []
        for item in list(self.results):
            url = str(item.get("url") or "")
            if item.get("kind") != "video" or not url or url in self.metadata_hydration_urls:
                continue
            if (item.get("timestamp") or item.get("upload_date")) and item.get("view_count") not in (None, ""):
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
        is_live = self.metadata_is_live_stream(info) or self.metadata_is_live_stream(item)
        payload = {
            "url": item.get("url", ""),
            "title": info.get("title") or item.get("title", ""),
            "id": info.get("id") or item.get("id", ""),
            "channel": info.get("uploader") or info.get("channel") or item.get("channel", ""),
            "channel_url": self.normalize_channel_url(info) or item.get("channel_url", ""),
            "channel_id": info.get("channel_id") or info.get("uploader_id") or item.get("channel_id", ""),
            "view_count": info.get("view_count", item.get("view_count")),
            "views": self.format_count(info.get("view_count", item.get("view_count"))),
            "timestamp": timestamp,
            "upload_date": upload_date,
            "age": self.t("live_now") if is_live else (self.format_age({"timestamp": timestamp, "upload_date": upload_date}) or item.get("age") or self.t("uploaded_unknown")),
            "duration_seconds": info.get("duration", item.get("duration_seconds")),
            "duration": self.format_duration(info.get("duration", item.get("duration_seconds"))),
            "description": info.get("description") or item.get("description", ""),
            "artist": info.get("artist") or info.get("creator") or item.get("artist", ""),
            "track": info.get("track") or item.get("track", ""),
            "album": info.get("album") or item.get("album", ""),
            "chapters": self.normalized_chapters(info.get("chapters")) or item.get("chapters", []),
            "kind": item.get("kind", "video"),
            "type": self.t("live_stream") if is_live else item.get("type", self.t("video")),
            "live_status": self.metadata_live_status(info) or self.metadata_live_status(item),
            "is_live": is_live,
        }
        return self.with_live_stream_display_fields(payload, info)

    @staticmethod
    def numeric_view_count(value) -> int:
        if value in (None, ""):
            return -1
        try:
            return int(float(str(value).replace(",", "").strip()))
        except (TypeError, ValueError):
            return -1

    @staticmethod
    def is_youtube_channel_id(value: str) -> bool:
        return bool(re.fullmatch(r"UC[\w-]{20,}", str(value or "").strip()))

    @staticmethod
    def popular_result_sort_key(item: dict) -> tuple[int, int, str]:
        timestamp = item.get("timestamp")
        upload_date = item.get("upload_date")
        try:
            age_value = int(timestamp or 0)
        except (TypeError, ValueError):
            age_value = 0
        if not age_value:
            try:
                age_value = int(str(upload_date or "0"))
            except (TypeError, ValueError):
                age_value = 0
        return (
            MainFrame.numeric_view_count(item.get("view_count")),
            age_value,
            str(item.get("title") or "").lower(),
        )

    def sort_popular_results(self, results: list[dict]) -> list[dict]:
        return sorted(results, key=self.popular_result_sort_key, reverse=True)

    def dedupe_results_by_url(self, results: list[dict]) -> list[dict]:
        deduped: list[dict] = []
        seen: set[str] = set()
        for item in results:
            url = str(item.get("url") or item.get("webpage_url") or "").strip()
            key = url or str(item.get("title") or "")
            if not key or key in seen:
                continue
            seen.add(key)
            deduped.append(item)
        return deduped

    def hydrate_video_metadata_for_popular(self, item: dict) -> dict:
        updated = dict(item)
        url = str(updated.get("url") or "")
        if updated.get("kind") != "video" or not url or self.numeric_view_count(updated.get("view_count")) >= 0:
            return updated
        options = {"quiet": True, "skip_download": True, "noplaylist": True}
        try:
            info = self.ydl_extract_info(url, options, download=False)
            updated.update({key: value for key, value in self.metadata_from_info(info, updated).items() if value not in (None, "")})
        except Exception:
            pass
        return updated

    def sorted_popular_channel_results(self, results: list[dict]) -> list[dict]:
        return self.sort_popular_results([self.hydrate_video_metadata_for_popular(item) for item in results])

    def resolve_channel_id_for_popular(self, url: str) -> str:
        existing = str(getattr(self, "collection_channel_id", "") or "").strip()
        if self.is_youtube_channel_id(existing):
            return existing
        try:
            options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": 1}
            info = self.ydl_extract_info(url, options, download=False)
        except Exception:
            return ""
        channel_id = str((info or {}).get("channel_id") or (info or {}).get("id") or "").strip()
        return channel_id if self.is_youtube_channel_id(channel_id) else ""

    def fetch_youtube_api_videos_by_ids(self, video_ids: list[str]) -> list[dict]:
        api_key = self.youtube_data_api_key()
        ordered_ids = [video_id for video_id in video_ids if video_id]
        if not api_key or not ordered_ids:
            return []
        videos_by_id: dict[str, dict] = {}
        for start in range(0, len(ordered_ids), 50):
            chunk = ordered_ids[start : start + 50]
            params = {
                "part": "snippet,contentDetails,statistics",
                "id": ",".join(chunk),
                "key": api_key,
                "maxResults": str(len(chunk)),
            }
            request = Request(f"{YOUTUBE_API_VIDEOS_URL}?{urlencode(params)}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
            with self.open_url(request, timeout=25) as response:
                payload = json.loads(response.read().decode("utf-8", errors="replace"))
            if isinstance(payload, dict) and payload.get("error"):
                error = payload.get("error") or {}
                message = error.get("message") if isinstance(error, dict) else str(error)
                raise RuntimeError(message or self.t("trending_api_key_required"))
            for item in list((payload or {}).get("items") or []):
                if isinstance(item, dict):
                    video_id = str(item.get("id") or "")
                    if video_id:
                        videos_by_id[video_id] = item
        return [self.normalize_youtube_api_video(videos_by_id[video_id]) for video_id in ordered_ids if video_id in videos_by_id]

    def fetch_youtube_api_channel_popular(self, url: str, limit: int) -> tuple[list[dict], bool]:
        api_key = self.youtube_data_api_key()
        if not api_key:
            return [], False
        channel_id = self.resolve_channel_id_for_popular(url)
        if not channel_id:
            return [], False
        video_ids: list[str] = []
        next_page = ""
        while len(video_ids) < limit:
            params = {
                "part": "snippet",
                "channelId": channel_id,
                "order": "viewCount",
                "type": "video",
                "maxResults": str(min(50, max(1, limit - len(video_ids)))),
                "key": api_key,
            }
            if next_page:
                params["pageToken"] = next_page
            request = Request(f"{YOUTUBE_API_SEARCH_URL}?{urlencode(params)}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
            with self.open_url(request, timeout=25) as response:
                payload = json.loads(response.read().decode("utf-8", errors="replace"))
            if isinstance(payload, dict) and payload.get("error"):
                error = payload.get("error") or {}
                message = error.get("message") if isinstance(error, dict) else str(error)
                raise RuntimeError(message or self.t("trending_api_key_required"))
            for item in list((payload or {}).get("items") or []):
                video_id = str(((item.get("id") or {}) if isinstance(item, dict) else {}).get("videoId") or "")
                if video_id and video_id not in video_ids:
                    video_ids.append(video_id)
            next_page = str((payload or {}).get("nextPageToken") or "")
            if not next_page:
                break
        return self.fetch_youtube_api_videos_by_ids(video_ids[:limit]), not next_page

    def fetch_ytdlp_channel_popular(self, url: str, generation: int) -> tuple[list[dict], bool]:
        options = {"quiet": True, "extract_flat": True, "skip_download": True}
        info = self.ydl_extract_info(url, options, download=False)
        entries = [entry for entry in list((info or {}).get("entries") or []) if isinstance(entry, dict)]
        normalized = self.dedupe_results_by_url([self.normalize_entry(entry, "Video") for entry in entries])
        total = len(normalized)
        if not total:
            return [], True
        wx.CallAfter(self.set_status_if_current, generation, self.t("popular_scan_status", done=0, total=total))
        hydrated: list[dict] = []
        workers = min(POPULAR_CHANNEL_METADATA_WORKERS, total)
        done = 0
        futures_module = import_module("concurrent.futures")
        with futures_module.ThreadPoolExecutor(max_workers=workers) as executor:
            futures = [executor.submit(self.hydrate_video_metadata_for_popular, item) for item in normalized]
            for future in futures_module.as_completed(futures):
                try:
                    hydrated.append(future.result())
                except Exception:
                    pass
                done += 1
                if done == total or done % POPULAR_CHANNEL_PROGRESS_INTERVAL == 0:
                    wx.CallAfter(self.set_status_if_current, generation, self.t("popular_scan_status", done=done, total=total))
        return self.sort_popular_results(hydrated), True

    def fetch_popular_channel_results(self, url: str, limit: int, generation: int) -> tuple[list[dict], bool]:
        if self.youtube_data_api_key():
            try:
                results, fully_loaded = self.fetch_youtube_api_channel_popular(url, limit)
                if results:
                    return self.sort_popular_results(results), fully_loaded
            except Exception:
                pass
        return self.fetch_ytdlp_channel_popular(url, generation)

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
                self.item_type_label(item),
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
        if self.focus_in_results_control(wx.Window.FindFocus()) and getattr(self, "last_user_result_identity", ""):
            index = self.result_index_for_identity(self.last_user_result_identity, self.last_user_result_index)
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.results):
            return None
        self.current_index = index
        return self.results[index]

    def play_selected(self) -> None:
        item = self.stable_selected_result()
        if not item:
            self.message(self.t("no_selection"))
            return
        if item.get("kind") == "channel":
            self.show_channel_options(item)
            return
        if item.get("kind") == "playlist":
            self.open_playlist_videos(item)
            return
        self.clear_player_sequence()
        self.shuffle_current = False
        self.return_results = list(self.results)
        self.return_all_results = list(self.all_results or self.results)
        self.return_index = self.current_index
        self.return_visible_count = self.last_visible_count or len(self.results)
        folder_context = self.folder_screen_active or (self.in_player_screen and self.player_return_screen == "folder")
        trending_context = getattr(self, "trending_screen_active", False) or (self.in_player_screen and self.player_return_screen == "trending")
        if folder_context:
            items = self.selected_local_folder_items()
            if items:
                item_url = str(item.get("url") or "")
                folder_index = next((index for index, result in enumerate(items) if str(result.get("url") or "") == item_url), self.current_index)
                folder_index = min(max(0, folder_index), len(items) - 1)
                self.return_results = list(items)
                self.return_all_results = list(items)
                self.return_index = folder_index
                self.return_visible_count = len(items)
                self.current_index = folder_index
            self.player_return_screen = "folder"
            self.player_return_data = {"index": self.current_index, "folder": self.current_local_folder_path or self.last_search_query}
        elif trending_context:
            self.player_return_screen = "trending"
            self.player_return_data = {
                "index": self.current_index,
                "country_index": getattr(self, "last_trending_country_index", 0),
                "category_index": getattr(self, "last_trending_category_index", 0),
            }
        else:
            self.player_return_screen = "search"
            self.player_return_data = self.search_return_data(self.current_index)
        if folder_context:
            self.clear_auto_folder_playback_queue()
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
                "collection_sort_mode": self.collection_sort_mode,
                "collection_channel_id": getattr(self, "collection_channel_id", ""),
                "collection_fully_loaded": bool(getattr(self, "collection_fully_loaded", False)),
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
        self.collection_sort_mode = str(state.get("collection_sort_mode") or "")
        self.collection_channel_id = str(state.get("collection_channel_id") or "")
        self.collection_fully_loaded = bool(state.get("collection_fully_loaded", False))
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
        if tab == "popular":
            return f"{base}/videos"
        if tab == "playlists":
            return f"{base}/playlists"
        if tab == "streams":
            return f"{base}/streams"
        return f"{base}/videos"

    def show_channel_options(self, item: dict | None = None) -> None:
        item = item or self.selected_result()
        if not item or item.get("kind") != "channel":
            self.message(self.t("no_selection"))
            return
        tabs = [
            ("videos", self.t("channel_videos")),
            ("playlists", self.t("channel_playlists")),
            ("streams", self.t("channel_live_streams")),
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
        elif tab == "popular":
            result_type = "Video"
            label = self.t("channel_popular")
        elif tab == "streams":
            result_type = "Video"
            label = self.t("channel_live_streams")
        else:
            result_type = "Video"
            label = self.t("channel_videos")
        self.set_status(self.t("loading_channel", title=f"{title} - {label}"))
        self.collection_url = url
        self.collection_result_type = result_type
        self.collection_sort_mode = "popular" if tab == "popular" else ""
        self.collection_channel_id = str(item.get("channel_id") or "")
        self.collection_fully_loaded = False
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.load_collection_worker, args=(url, result_type, self.initial_results_limit(), 0, generation, self.collection_sort_mode), daemon=True).start()

    def show_trending(self, auto_load: bool = True, country_index: int | None = None, category_index: int | None = None) -> None:
        if not getattr(self.settings, "enable_trending", False):
            self.announce_player(self.t("trending_disabled"))
            self.show_main_menu()
            return
        self.in_player_screen = False
        if not self.player_is_active():
            self.player_control_mode = False
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
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
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
        live_status = self.metadata_live_status(snippet)
        is_live = self.metadata_is_live_stream(snippet)
        normalized = {
            "title": title,
            "id": video_id,
            "channel": channel,
            "channel_url": f"https://www.youtube.com/channel/{channel_id}" if channel_id else "",
            "channel_id": channel_id,
            "views": self.format_count(view_count),
            "view_count": view_count,
            "age": self.t("live_now") if is_live else (self.format_age({"timestamp": timestamp}) if timestamp else self.t("uploaded_unknown")),
            "duration": self.format_duration(duration_seconds),
            "duration_seconds": duration_seconds,
            "timestamp": timestamp,
            "upload_date": "",
            "description": snippet.get("description") or "",
            "type": self.t("live_stream") if is_live else self.t("video"),
            "kind": "video",
            "url": f"https://www.youtube.com/watch?v={video_id}" if video_id else "",
            "live_status": live_status,
            "is_live": is_live,
        }
        return self.with_live_stream_display_fields(normalized, snippet)

    def open_playlist_videos(self, item: dict, push_state: bool = True) -> None:
        if push_state:
            self.push_search_state()
        self.trending_screen_active = False
        self.set_status(self.t("loading_playlist", title=item["title"]))
        self.collection_url = item["url"]
        self.collection_result_type = "Video"
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.load_collection_worker, args=(item["url"], "Video", self.initial_results_limit(), 0, generation, ""), daemon=True).start()

    def play_playlist_from_result(self, item: dict | None = None, shuffle: bool = False) -> None:
        item = dict(item or self.stable_selected_result() or {})
        if item.get("kind") != "playlist" or not item.get("url"):
            self.announce_player(self.t("no_selection"))
            return
        self.return_results = list(self.results)
        self.return_all_results = list(self.all_results or self.results)
        self.return_index = max(0, self.current_index)
        self.return_visible_count = self.last_visible_count or len(self.results)
        self.playlist_play_generation += 1
        generation = self.playlist_play_generation
        self.set_status(self.t("loading_playlist", title=item.get("title") or self.t("playlist")))
        threading.Thread(target=self.play_playlist_worker, args=(item, shuffle, generation), daemon=True).start()

    def play_playlist_worker(self, item: dict, shuffle: bool, generation: int) -> None:
        try:
            options = {"quiet": True, "extract_flat": True, "skip_download": True}
            info = self.ydl_extract_info(str(item.get("url") or ""), options, download=False)
            entries = [entry for entry in list((info or {}).get("entries") or []) if isinstance(entry, dict)]
            playable = [
                result
                for result in (self.normalize_entry(entry, "Video") for entry in entries)
                if result.get("kind") == "video" and result.get("url")
            ]
            wx.CallAfter(self.start_playlist_playback_if_current, generation, item, playable, shuffle)
        except Exception as exc:
            wx.CallAfter(self.dynamic_fetch_failed_if_current, self.search_generation, self.friendly_error(exc))

    def start_playlist_playback_if_current(self, generation: int, playlist_item: dict, items: list[dict], shuffle: bool) -> None:
        if generation != self.playlist_play_generation:
            return
        self.start_playlist_playback(playlist_item, items, shuffle)

    def start_playlist_playback(self, playlist_item: dict, items: list[dict], shuffle: bool) -> None:
        if not items:
            self.announce_player(self.t("playlist_no_videos"))
            return
        ordered = [dict(item) for item in items]
        if shuffle:
            random.shuffle(ordered)
        self.shuffle_current = False
        sequence = [self.playlist_item_from_media(item) for item in ordered]
        self.set_player_sequence(sequence)
        current_item = dict(sequence[0])
        self.player_return_screen = "search"
        self.player_return_data = {"index": self.return_index, "playlist_title": playlist_item.get("title", "")}
        self.current_video_item = current_item
        self.current_video_info = dict(current_item)
        self.play_url(str(current_item.get("url") or ""), str(current_item.get("title") or ""))

    def load_collection_worker(
        self,
        url: str,
        result_type: str,
        limit: int | None = None,
        selection: int = 0,
        generation: int | None = None,
        sort_mode: str = "",
    ) -> None:
        try:
            generation = self.search_generation if generation is None else generation
            limit = limit or self.initial_results_limit()
            if sort_mode == "popular" and result_type == "Video":
                normalized, fully_loaded = self.fetch_popular_channel_results(url, limit, generation)
                if self.settings.results_limit != 0:
                    normalized = normalized[:limit]
                wx.CallAfter(self.mark_collection_fully_loaded_if_current, generation, fully_loaded)
                if self.settings.results_limit == 0 and selection:
                    wx.CallAfter(self.show_more_results_if_current, generation, normalized, selection)
                else:
                    wx.CallAfter(self.show_results_if_current, generation, normalized)
                    wx.CallAfter(self.clear_loading_more_if_current, generation)
                return
            options = {
                "quiet": True,
                "extract_flat": True,
                "skip_download": True,
                "playlistend": limit,
            }
            info = self.ydl_extract_info(url, options, download=False)
            entries = list(info.get("entries") or [])[:limit]
            normalized = [self.normalize_entry(entry, result_type) for entry in entries]
            if sort_mode == "popular":
                normalized = self.sorted_popular_channel_results(normalized)
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

    @staticmethod
    def natural_sort_key(value) -> list[tuple[int, object]]:
        text = str(value or "").casefold()
        text = re.sub(r"\.([^./\\]+)$", lambda match: "\x00" + match.group(1), text)
        parts = re.split(r"(\d+)", text)
        return [(1, int(part)) if part.isdigit() else (0, part) for part in parts]

    def local_media_files_in_folder(self, folder: Path) -> list[Path]:
        files: list[Path] = []

        def ignore_walk_error(_error: OSError) -> None:
            return

        try:
            for root, directories, names in os.walk(folder, onerror=ignore_walk_error):
                directories.sort(key=self.natural_sort_key)
                for name in sorted(names, key=self.natural_sort_key):
                    path = Path(root) / name
                    try:
                        if path.is_file() and path.suffix.lower() in LOCAL_MEDIA_EXTENSIONS:
                            files.append(path)
                    except OSError:
                        continue
        except OSError:
            return []
        return sorted(files, key=lambda path: self.natural_sort_key(str(path.relative_to(folder))))

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

    def show_play_file(self) -> None:
        start_dir = self.settings.download_folder or str(Path.home())
        with wx.FileDialog(
            self,
            self.t("play_file"),
            defaultDir=start_dir if Path(start_dir).exists() else str(Path.home()),
            wildcard=self.local_media_wildcard(),
            style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                self.show_main_menu()
                return
            path = dialog.GetPath()
        self.open_local_media_file(path, True)

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
        folder_items = [dict(item) for item in items if item.get("kind") == "local_file" and item.get("url")]
        self.current_local_folder_path = str(folder)
        self.current_local_folder_items = list(folder_items)
        self.in_player_screen = False
        if not self.player_is_active():
            self.player_control_mode = False
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
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
        self.search_results_stack = []
        self.loading_more_results = False
        self.dynamic_fetch_enabled = False
        self.last_user_result_index = 0
        self.last_user_result_identity = ""
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        self.cache_local_folder_items(folder, folder_items)
        self.show_results(folder_items, selection=selection, visible_count=len(folder_items))
        self.set_status(self.t("folder_loaded", count=len(folder_items)))
        self.return_results = list(folder_items)
        self.return_all_results = list(folder_items)
        self.return_index = min(max(0, selection), max(0, len(folder_items) - 1))
        self.return_visible_count = len(folder_items)
        self.panel.Layout()
        self.focus_later(self.results_list)

    def selected_local_folder_items(self) -> list[dict]:
        if (self.folder_screen_active or self.player_return_screen == "folder") and getattr(self, "current_local_folder_items", None):
            return [dict(item) for item in self.current_local_folder_items if item.get("kind") == "local_file" and item.get("url")]
        return [dict(item) for item in (self.all_results or self.results) if item.get("kind") == "local_file" and item.get("url")]

    def play_local_folder(self, start_index: int = 0, shuffle: bool = False) -> None:
        items = self.selected_local_folder_items()
        if not items:
            self.announce_player(self.t("folder_no_media"))
            return
        if self.folder_screen_active:
            selected_index = self.current_results_selection(start_index)
            if 0 <= selected_index < len(self.results):
                selected_url = self.results[selected_index].get("url")
                start_index = next((index for index, item in enumerate(items) if item.get("url") == selected_url), start_index)
        start_index = min(max(0, start_index), len(items) - 1)
        ordered = list(items)
        if shuffle:
            random.shuffle(ordered)
            start_index = 0
            self.shuffle_current = True
        else:
            self.shuffle_current = False
        current_item = dict(ordered[start_index])
        self.clear_player_sequence()
        current_source_index = next((index for index, item in enumerate(items) if item.get("url") == current_item.get("url")), start_index)
        queue_items = [self.playback_queue_item_with_folder_return(item, items, auto_folder_queue=True) for item in ordered[start_index + 1 :]]
        self.set_auto_folder_playback_queue(queue_items)
        self.player_return_screen = "folder"
        self.player_return_data = {
            "index": current_source_index,
            "folder": self.current_local_folder_path or self.last_search_query,
        }
        self.return_results = list(items)
        self.return_all_results = list(items)
        self.return_index = current_source_index
        self.return_visible_count = len(items)
        self.current_index = current_source_index
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
        queue_item["_return_folder"] = self.current_local_folder_path or self.last_search_query
        queue_item["_return_index"] = next(
            (index for index, source in enumerate(source_items) if source.get("url") == item.get("url")),
            0,
        )
        if auto_folder_queue:
            queue_item["_auto_folder_queue"] = True
        return queue_item

    def open_local_media_file(self, value: str, activate_after_open: bool = False) -> None:
        try:
            path = self.local_media_path_from_input(value)
            if not path:
                raise FileNotFoundError(value)
            item = self.local_media_item(path)
            self.player_return_screen = "local_file"
            self.player_return_data = {}
            self.current_video_item = item
            self.current_video_info = dict(item)
            if activate_after_open:
                self.set_window_title(item["title"])
                self.set_status(self.t("preparing_stream", title=item["title"]))
                self.ensure_window_visible()
                try:
                    self.foreground_window()
                except Exception:
                    pass
            self.play_url(str(path), item["title"], announce_start=activate_after_open)
            if activate_after_open:
                self.restore_from_tray()
        except Exception as exc:
            self.message(self.t("local_file_open_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def clear_loading_more_if_current(self, generation: int) -> None:
        if generation == self.search_generation:
            self.loading_more_results = False

    def mark_collection_fully_loaded_if_current(self, generation: int, fully_loaded: bool) -> None:
        if generation == self.search_generation:
            self.collection_fully_loaded = bool(fully_loaded)

    def play_url(
        self,
        url: str,
        title: str = "",
        show_player: bool = True,
        announce_start: bool = False,
        focus_target: str = "player",
        keep_current_ui: bool = False,
    ) -> None:
        player = self.resolve_player()
        if not player:
            self.message(self.t("player_missing"), wx.ICON_ERROR)
            return
        if self.current_video_item:
            self.record_history(self.current_video_item, "played")
        self.current_index = max(0, self.current_index)
        continuing_session = bool(getattr(self, "player_session_open", False)) or self.player_is_active() or bool(getattr(self, "playback_start_pending", False))
        self.remember_current_player_volume()
        self.stop_player(silent=True, reset_session=not continuing_session, preserve_panel=keep_current_ui)
        if not continuing_session:
            self.session_equalizer_enabled = None
            self.session_equalizer_gains = {}
            self.session_equalizer_before_bass_boost = None
            self.session_autoplay_next = False
            self.shuffle_current = False
        self.edit_mode_enabled = False
        self.equalizer_filter_active = False
        self.equalizer_filter_ref = EQ_FILTER_REF
        self.clip_start_marker = None
        self.clip_end_marker = None
        self.current_stream_headers = {}
        self.player_fullscreen_results_override = False
        command, kind = player
        if kind != "mpv":
            self.message(self.t("player_missing"), wx.ICON_ERROR)
            return
        if keep_current_ui:
            self.player_control_mode = True
            self.set_window_title(title or self.current_player_title())
        elif show_player:
            self.show_player_page(title, focus_target=focus_target)
            if self.local_media_path_from_input(url):
                wx.CallLater(500, self.focus_player_target_later, focus_target)
        else:
            self.in_player_screen = False
            self.player_control_mode = True
            self.set_window_title(title or self.current_player_title())
        self.set_status(self.t("preparing_stream", title=title or url))
        self.play_request_generation += 1
        request_generation = self.play_request_generation
        self.playback_start_pending = True
        threading.Thread(target=self.resolve_and_start_player, args=(command, url, title, announce_start, request_generation), daemon=True).start()

    def playback_request_is_current(self, generation: int) -> bool:
        return generation == getattr(self, "play_request_generation", 0)

    def merge_current_video_info_for_request(self, info: dict, generation: int) -> None:
        if self.playback_request_is_current(generation):
            self.merge_current_video_info(info)

    def schedule_next_stream_prefetch_for_request(self, generation: int) -> None:
        if self.playback_request_is_current(generation):
            self.schedule_next_stream_prefetch()

    def resolve_and_start_player(self, command: str, url: str, title: str, announce_start: bool = False, request_generation: int = 0) -> None:
        try:
            stream_url, headers, info = self.resolve_stream_url(url)
            if not self.playback_request_is_current(request_generation):
                return
            wx.CallAfter(self.merge_current_video_info_for_request, info, request_generation)
            wx.CallAfter(self.start_mpv, command, stream_url, title or url, headers, announce_start, request_generation)
            wx.CallAfter(self.schedule_next_stream_prefetch_for_request, request_generation)
        except Exception as exc:
            if not self.playback_request_is_current(request_generation):
                return
            self.playback_start_pending = False
            if self.age_restricted_video_support_enabled() and self.is_cookie_auth_error(exc) and self.normalized_cookies_browser():
                wx.CallAfter(self.prompt_cookie_refresh_for_playback, command, url, title, self.friendly_error(exc), announce_start, request_generation)
            else:
                wx.CallAfter(self.message, self.t("player_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def prompt_cookie_refresh_for_playback(self, command: str, url: str, title: str, error: str, announce_start: bool = False, request_generation: int = 0) -> None:
        if not self.playback_request_is_current(request_generation):
            return
        message = f"{self.t('player_failed', error=error)}\n\n{self.t('cookie_refresh_prompt_message')}"
        answer = wx.MessageBox(message, self.t("cookie_refresh_prompt_title"), wx.YES_NO | wx.ICON_QUESTION)
        if answer != wx.YES:
            return
        browser = self.normalized_cookies_browser()
        if not browser:
            self.message(self.t("select_cookies_browser"), wx.ICON_WARNING)
            return
        self.announce_player(self.t("cookie_auto_refresh_start", browser=browser.title()))
        threading.Thread(target=self.refresh_cookies_and_retry_playback_worker, args=(browser, command, url, title, announce_start, request_generation), daemon=True).start()

    def refresh_cookies_and_retry_playback_worker(self, browser: str, command: str, url: str, title: str, announce_start: bool = False, request_generation: int = 0) -> None:
        try:
            result = self.export_browser_cookies_blocking(browser, allow_close=True)
            if not self.playback_request_is_current(request_generation):
                return
            self.playback_start_pending = True
            self.ui_queue.put(("announce", self.t("cookie_auto_refresh_done", profile=result.get("profile_label", self.t("browser_profile_auto")))))
            self.resolve_and_start_player(command, url, title, announce_start, request_generation)
        except Exception as exc:
            if not self.playback_request_is_current(request_generation):
                return
            self.playback_start_pending = False
            wx.CallAfter(self.message, self.t("cookie_auto_refresh_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def stream_url_cache_key(self, url: str) -> str:
        parts = {
            "url": url,
            "video_format": self.normalized_video_format(),
            "max_height": int(getattr(self.settings, "max_video_height", 1080) or 0),
            "restricted": bool(getattr(self.settings, "enable_age_restricted_videos", False)),
            "cookies_file": str(getattr(self.settings, "cookies_file", "") or ""),
            "cookies_browser": str(getattr(self.settings, "cookies_browser", "") or ""),
        }
        return json.dumps(parts, sort_keys=True, ensure_ascii=False)

    def stream_url_cache_minutes_value(self) -> int:
        return self.normalized_stream_url_cache_minutes()

    def cached_stream_url(self, url: str) -> tuple[str, dict, dict] | None:
        if not getattr(self.settings, "enable_stream_url_cache", True):
            return None
        key = self.stream_url_cache_key(url)
        now = time.time()
        with self.stream_url_cache_lock:
            cached = self.stream_url_cache.get(key)
            if not cached:
                return None
            if float(cached.get("expires_at") or 0) <= now:
                self.stream_url_cache.pop(key, None)
                return None
            return str(cached.get("stream_url") or ""), dict(cached.get("headers") or {}), dict(cached.get("info") or {})

    def cache_stream_url(self, source_url: str, stream_url: str, headers: dict, info: dict) -> None:
        if not getattr(self.settings, "enable_stream_url_cache", True) or not source_url or not stream_url:
            return
        minutes = self.stream_url_cache_minutes_value()
        ttl_seconds = (365 * 24 * 60 * 60) if minutes <= 0 else minutes * 60
        expires_at = time.time() + ttl_seconds
        try:
            expire_values = parse_qs(urlparse(stream_url).query).get("expire") or []
            if expire_values:
                remote_expiry = int(expire_values[0]) - 60
                expires_at = min(expires_at, float(remote_expiry))
        except (TypeError, ValueError, OverflowError):
            pass
        if expires_at <= time.time() + 30:
            return
        with self.stream_url_cache_lock:
            now = time.time()
            self.stream_url_cache = {key: value for key, value in self.stream_url_cache.items() if float(value.get("expires_at") or 0) > now}
            if len(self.stream_url_cache) > 120:
                oldest = sorted(self.stream_url_cache, key=lambda item_key: float(self.stream_url_cache[item_key].get("expires_at") or 0))
                for old_key in oldest[: len(self.stream_url_cache) - 100]:
                    self.stream_url_cache.pop(old_key, None)
            self.stream_url_cache[self.stream_url_cache_key(source_url)] = {
                "stream_url": stream_url,
                "headers": dict(headers or {}),
                "info": dict(info or {}),
                "expires_at": expires_at,
            }

    def next_prefetch_candidate(self) -> dict | None:
        if self.current_player_sequence_active():
            return self.relative_player_item(1)
        if self.playback_queue:
            return dict(self.playback_queue[0])
        return self.relative_player_item(1)

    def schedule_next_stream_prefetch(self) -> None:
        if not getattr(self.settings, "prefetch_next_stream_url", True):
            return
        item = self.next_prefetch_candidate()
        url = str((item or {}).get("url") or "")
        if not url or self.local_media_path_from_input(url):
            return
        key = self.stream_url_cache_key(url)
        with self.stream_url_cache_lock:
            if key in self.stream_url_cache:
                return
            if key in self.prefetch_stream_urls:
                return
            self.prefetch_stream_urls.add(key)
        threading.Thread(target=self.prefetch_stream_url_worker, args=(url, key), daemon=True).start()

    def prefetch_stream_url_worker(self, url: str, key: str) -> None:
        try:
            self.resolve_stream_url(url)
        except Exception:
            pass
        finally:
            with self.stream_url_cache_lock:
                self.prefetch_stream_urls.discard(key)

    def resolve_stream_url(self, url: str) -> tuple[str, dict, dict]:
        local_path = self.local_media_path_from_input(url)
        if local_path:
            info = self.local_media_item(local_path)
            return str(local_path), {}, info
        cached = self.cached_stream_url(url)
        if cached and cached[0]:
            return cached
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
                        headers = info.get("http_headers") or {}
                        self.cache_stream_url(url, stream_url, headers, info)
                        return stream_url, headers, info
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
        headers = info.get("http_headers") or {}
        self.cache_stream_url(url, stream_url, headers, info)
        return stream_url, headers, info

    def merge_current_video_info(self, info: dict) -> None:
        if not info:
            return
        is_live = self.metadata_is_live_stream(info) or self.metadata_is_live_stream(self.current_video_info)
        live_status = self.metadata_live_status(info) or self.metadata_live_status(self.current_video_info)
        self.current_video_info.update(
            {
                "title": info.get("title") or self.current_video_info.get("title", ""),
                "id": info.get("id") or self.current_video_info.get("id", ""),
                "channel": info.get("uploader") or info.get("channel") or self.current_video_info.get("channel", ""),
                "channel_url": self.normalize_channel_url(info) or self.current_video_info.get("channel_url", ""),
                "channel_id": info.get("channel_id") or info.get("uploader_id") or self.current_video_info.get("channel_id", ""),
                "url": info.get("webpage_url") or self.current_video_info.get("url", ""),
                "view_count": info.get("view_count", self.current_video_info.get("view_count")),
                "views": self.format_count(info.get("view_count", self.current_video_info.get("view_count"))),
                "timestamp": info.get("timestamp", self.current_video_info.get("timestamp")),
                "upload_date": info.get("upload_date", self.current_video_info.get("upload_date")),
                "age": self.t("live_now") if is_live else (self.format_age(info) or self.current_video_info.get("age", "")),
                "duration_seconds": info.get("duration", self.current_video_info.get("duration_seconds")),
                "duration": self.format_duration(info.get("duration", self.current_video_info.get("duration_seconds"))),
                "description": info.get("description") or self.current_video_info.get("description", ""),
                "ext": info.get("ext") or self.current_video_info.get("ext", ""),
                "artist": info.get("artist") or info.get("creator") or self.current_video_info.get("artist", ""),
                "track": info.get("track") or self.current_video_info.get("track", ""),
                "album": info.get("album") or self.current_video_info.get("album", ""),
                "chapters": self.normalized_chapters(info.get("chapters")) or self.current_video_info.get("chapters", []),
                "live_status": live_status,
                "is_live": bool(is_live),
                "type": self.t("live_stream") if is_live else self.current_video_info.get("type", self.t("video")),
            }
        )
        self.with_live_stream_display_fields(self.current_video_info, info)
        if self.current_video_item is not None:
            self.current_video_item.update(self.current_video_info)
        if self.in_player_screen:
            self.set_window_title(str(self.current_video_info.get("title") or ""))
        self.update_details_text()

    def playback_key(self, item: dict | None = None) -> str:
        item = item or self.current_video_item or self.current_video_info
        return str((item or {}).get("url") or (item or {}).get("webpage_url") or "").strip()

    def playback_resume_position(self) -> float:
        if self.metadata_is_live_stream(self.current_video_info) or self.metadata_is_live_stream(self.current_video_item):
            return 0.0
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

    def session_volume_max(self) -> int:
        if self.volume_boost_enabled or getattr(self.settings, "volume_boost_by_default", False):
            return BOOSTED_VOLUME_MAX
        try:
            if self.session_volume is not None and float(self.session_volume) > NORMAL_VOLUME_MAX:
                return BOOSTED_VOLUME_MAX
        except (TypeError, ValueError):
            pass
        return NORMAL_VOLUME_MAX

    def clamp_session_volume(self, value) -> float:
        try:
            numeric = float(value)
        except (TypeError, ValueError):
            numeric = float(self.default_volume_value())
        return max(0.0, min(float(self.session_volume_max()), numeric))

    def consume_pending_volume_target(self) -> float | None:
        with self.volume_change_lock:
            pending_target = self.volume_change_pending_target
            if pending_target is None:
                return None
            self.volume_change_pending_target = None
            timer = self.volume_change_timer
            if timer is not None and timer.IsRunning():
                timer.Stop()
            self.volume_change_timer = None
        self.session_volume = self.clamp_session_volume(pending_target)
        return self.session_volume

    def remember_current_player_volume(self) -> None:
        if self.consume_pending_volume_target() is not None:
            return
        if self.session_volume is not None:
            self.session_volume = self.clamp_session_volume(self.session_volume)
            return
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            return
        try:
            current = self.mpv_get_property("volume", timeout=0.3)
            if current is not None:
                self.session_volume = self.clamp_session_volume(current)
        except Exception:
            pass

    def current_player_volume(self) -> float:
        self.remember_current_player_volume()
        if self.session_volume is not None:
            return self.clamp_session_volume(self.session_volume)
        return float(self.default_volume_value())

    def cancel_pending_volume_change(self) -> None:
        with self.volume_change_lock:
            self.volume_change_pending_target = None
            timer = self.volume_change_timer
            if timer is not None and timer.IsRunning():
                timer.Stop()
            self.volume_change_timer = None

    def current_player_volume_max(self) -> int:
        boosted = bool(self.volume_boost_enabled)
        return BOOSTED_VOLUME_MAX if boosted else NORMAL_VOLUME_MAX

    def configured_player_start_volume_max(self) -> int:
        boosted = bool(self.volume_boost_enabled or getattr(self.settings, "volume_boost_by_default", False))
        return BOOSTED_VOLUME_MAX if boosted else NORMAL_VOLUME_MAX

    def player_start_volume_value(self) -> float:
        if self.session_volume is not None:
            return self.clamp_session_volume(self.session_volume)
        return float(self.default_volume_value())

    def start_mpv(
        self,
        command: str,
        stream_url: str,
        title: str,
        headers: dict,
        announce_start: bool = False,
        request_generation: int = 0,
    ) -> None:
        if request_generation and not self.playback_request_is_current(request_generation):
            return
        self.playback_start_pending = False
        try:
            self.ipc_path = self.make_ipc_path()
            target_volume = self.player_start_volume_value()
            boost_volume = bool(self.volume_boost_enabled or getattr(self.settings, "volume_boost_by_default", False) or target_volume > NORMAL_VOLUME_MAX)
            volume_max = BOOSTED_VOLUME_MAX if boost_volume else NORMAL_VOLUME_MAX
            target_volume = max(0.0, min(float(volume_max), target_volume))
            self.session_volume = target_volume
            embed_player = False
            hwnd = 0
            try:
                panel = self.live_window(getattr(self, "player_panel", None))
                embed_player = bool(panel is not None)
                if embed_player and panel is not None:
                    panel.Update()
                    hwnd = panel.GetHandle()
            except Exception:
                embed_player = False
            args = [
                command,
                "--no-config",
                "--force-window=yes" if embed_player else "--force-window=no",
                f"--input-ipc-server={self.ipc_path}",
                "--idle=no",
                "--keep-open=yes",
                f"--volume-max={volume_max}",
                f"--volume={target_volume:g}",
                "--pitch=1.0",
                f"--speed={self.settings.player_speed}",
                f"--loop-file={'inf' if self.repeat_current else 'no'}",
                f"--gapless-audio={'yes' if bool(getattr(self.settings, 'gapless_playback', True)) else 'no'}",
                f"--replaygain={self.normalized_replaygain_mode()}",
                "--replaygain-clip=yes",
                "--term-playing-msg=",
                "--msg-level=all=warn",
            ]
            if embed_player and hwnd:
                args.insert(2, f"--wid={hwnd}")
            elif urlparse(str(stream_url)).scheme in {"http", "https"}:
                args.append("--vid=no")
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
            self.player_session_open = True
            self.player_ended = False
            self.player_paused = bool(self.settings.player_start_paused)
            self.player_generation += 1
            self.current_stream_url = stream_url
            self.current_stream_headers = dict(headers or {})
            self.current_audio_device = audio_device
            self.volume_boost_enabled = boost_volume
            self.rubberband_pitch_filter_active = False
            self.equalizer_filter_active = False
            self.equalizer_filter_ref = EQ_FILTER_REF
            self.current_video_info["speed"] = self.format_playback_rate(float(self.settings.player_speed))
            self.current_video_info["pitch"] = self.format_playback_rate(1.0)
            self.update_details_text()
            self.set_status(self.t("playing", title=title))
            if announce_start:
                self.announce_player(self.t("playing", title=title))
            wx.CallAfter(self.update_play_pause_buttons)
            threading.Thread(target=self.apply_initial_volume_worker, args=(self.player_generation, target_volume, volume_max), daemon=True).start()
            wx.CallLater(700, self.apply_equalizer_to_player)
            self.start_player_monitor(self.player_generation)
        except Exception as exc:
            self.playback_start_pending = False
            self.message(self.t("player_failed", error=exc), wx.ICON_ERROR)

    def apply_initial_volume_worker(self, generation: int, target_volume: float, volume_max: int) -> None:
        deadline = time.monotonic() + 2.0
        while time.monotonic() < deadline:
            if generation != self.player_generation or not self.mpv_process_alive():
                return
            try:
                self.mpv_set_property("volume-max", volume_max, timeout=0.4)
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
        for control in navigation_buttons:
            self.bind_player_navigation_control(control)
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
            self.bind_player_navigation_control(self.player_panel)
        try:
            self.player_panel.SetCanFocus(True)
        except Exception:
            pass
        self.bind_player_navigation_control(self.player_panel)
        self.player_panel.SetName(self.t("player"))
        self.player_panel.SetLabel(self.t("player"))
        self.root_sizer.Add(self.player_panel, 1, wx.EXPAND | wx.ALL, 4)
        if focus_target == "player" and not self.settings.show_video_details_by_default:
            self.player_panel.SetFocus()
        is_local_media = self.current_player_is_local_media()
        player_controls = [
            (self.t("previous"), lambda: self.play_relative_item(-1, preserve_focus=True)),
            (self.current_play_pause_label(), self.player_play_pause),
            (self.t("next"), lambda: self.play_relative_item(1, preserve_focus=True)),
            (self.t("playback_queue"), self.show_playback_queue),
            (self.t("add_to_playlist"), lambda: self.add_active_to_playlist(prefer_active=True)),
            (self.t("output_devices"), self.show_output_devices),
            (self.t("equalizer"), self.show_player_equalizer),
            (self.t("chapters"), self.show_chapters),
            (self.t("lyrics"), self.show_lyrics),
            (self.t("comments"), self.show_comments),
            (self.t("edit_mode"), self.toggle_edit_mode),
            (self.t("copy_path" if is_local_media else "copy_link"), self.copy_current_player_url),
            (self.t("show_video_details"), self.show_video_details),
        ]
        if not is_local_media:
            player_controls.insert(-1, (self.t("copy_stream_url"), self.copy_direct_stream_url))
        if background_enabled:
            player_controls.append((self.t("close_player"), self.close_current_player))
        player_action_buttons = self.add_button_row(player_controls)
        self.player_action_controls = list(player_action_buttons)
        for control in player_action_buttons:
            self.bind_player_navigation_control(control)
        self.player_escape_stop_controls.extend(player_action_buttons)
        self.fullscreen_checkbox = wx.CheckBox(self.panel, label=self.t("fullscreen"))
        self.fullscreen_checkbox.SetName(self.t("fullscreen"))
        self.fullscreen_checkbox.SetValue(fullscreen_mode)
        self.fullscreen_checkbox.Bind(wx.EVT_CHECKBOX, self.on_player_fullscreen_changed)
        self.fullscreen_checkbox.Bind(wx.EVT_KEY_DOWN, self.on_fullscreen_checkbox_key)
        self.bind_player_navigation_control(self.fullscreen_checkbox)
        self.root_sizer.Add(self.fullscreen_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.player_action_controls.append(self.fullscreen_checkbox)
        self.player_escape_stop_controls.append(self.fullscreen_checkbox)
        self.repeat_checkbox = wx.CheckBox(self.panel, label=self.t("repeat"))
        self.repeat_checkbox.SetName(self.t("repeat"))
        self.repeat_checkbox.SetValue(self.repeat_current)
        self.repeat_checkbox.Bind(wx.EVT_CHECKBOX, self.on_repeat_changed)
        self.bind_player_navigation_control(self.repeat_checkbox)
        self.root_sizer.Add(self.repeat_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.player_action_controls.append(self.repeat_checkbox)
        self.player_escape_stop_controls.append(self.repeat_checkbox)
        self.session_autoplay_checkbox = None
        if not bool(getattr(self.settings, "autoplay_next", False)):
            self.session_autoplay_checkbox = wx.CheckBox(self.panel, label=self.t("autoplay_next_session"))
            self.session_autoplay_checkbox.SetName(self.t("autoplay_next_session"))
            self.session_autoplay_checkbox.SetValue(bool(self.session_autoplay_next))
            self.session_autoplay_checkbox.Bind(wx.EVT_CHECKBOX, self.on_session_autoplay_next_changed)
            self.bind_player_navigation_control(self.session_autoplay_checkbox)
            self.root_sizer.Add(self.session_autoplay_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
            self.player_action_controls.append(self.session_autoplay_checkbox)
            self.player_escape_stop_controls.append(self.session_autoplay_checkbox)
        self.bass_boost_checkbox = wx.CheckBox(self.panel, label=self.t("bass_boost"))
        self.bass_boost_checkbox.SetName(self.t("bass_boost"))
        self.bass_boost_checkbox.SetValue(self.bass_boost_enabled)
        self.bass_boost_checkbox.Bind(wx.EVT_CHECKBOX, self.on_bass_boost_changed)
        self.bind_player_navigation_control(self.bass_boost_checkbox)
        self.root_sizer.Add(self.bass_boost_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.player_action_controls.append(self.bass_boost_checkbox)
        self.player_escape_stop_controls.append(self.bass_boost_checkbox)
        self.apply_tab_order(self.player_tab_order())
        self.details_label = None
        self.video_details = None
        self.details_button_sizer = None
        self.details_opened_temporarily = False
        self.set_window_title(title)
        self.panel.Layout()
        if focus_target == "results":
            wx.CallAfter(self.focus_results_list, self.return_index)
        elif focus_target != "player" and self.focus_player_target_later(focus_target):
            pass
        elif self.settings.show_video_details_by_default:
            wx.CallAfter(self.show_video_details, False)
        else:
            if not self.focus_player_target_later("player"):
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
        label = wx.StaticText(self.panel, label=self.t("result_list"))
        label.SetName(self.t("result_list"))
        self.root_sizer.Add(label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 4)
        labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
        self.results_list = wx.ListBox(self.panel, choices=labels or [self.t("search_results_empty")])
        self.results_list.SetName(self.t("result_list"))
        if labels:
            self.results_list.SetSelection(min(max(0, selection), len(labels) - 1))
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.bind_player_navigation_control(self.results_list)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)

    def on_player_key(self, event: wx.KeyEvent) -> None:
        self.on_char_hook(event)

    def on_repeat_changed(self, _event=None) -> None:
        checked = bool(getattr(self, "repeat_checkbox", None) and self.repeat_checkbox.GetValue())
        self.set_repeat_enabled(checked)

    def on_fullscreen_checkbox_key(self, event: wx.KeyEvent) -> None:
        key = event.GetKeyCode()
        if key in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}:
            self.request_player_fullscreen_checkbox_toggle()
            return
        event.Skip()

    def request_player_fullscreen_checkbox_toggle(self) -> None:
        now = time.monotonic()
        if now < getattr(self, "fullscreen_checkbox_toggle_block_until", 0.0):
            return
        self.fullscreen_checkbox_toggle_block_until = now + 0.18
        self.toggle_player_fullscreen_checkbox()

    def toggle_player_fullscreen_checkbox(self) -> None:
        checkbox = getattr(self, "fullscreen_checkbox", None)
        if checkbox is None:
            return
        try:
            checkbox.SetValue(not checkbox.GetValue())
        except RuntimeError:
            return
        self.on_player_fullscreen_changed()

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
        self.session_equalizer_before_bass_boost = None
        self.bass_boost_enabled = checked
        if getattr(self, "bass_boost_checkbox", None):
            try:
                self.bass_boost_checkbox.SetValue(checked)
            except RuntimeError:
                pass
        self.schedule_equalizer_apply(30)
        if announce:
            self.announce_player(self.t("bass_boost_on" if checked else "bass_boost_off"))

    def toggle_bass_boost(self) -> None:
        self.set_bass_boost_enabled(not self.bass_boost_enabled)

    def toggle_shuffle(self) -> None:
        self.shuffle_current = not self.shuffle_current
        self.announce_player(self.t("shuffle_on" if self.shuffle_current else "shuffle_off"))

    def effective_autoplay_next(self) -> bool:
        return bool(getattr(self.settings, "autoplay_next", False) or self.session_autoplay_next)

    def on_session_autoplay_next_changed(self, _event=None) -> None:
        checkbox = getattr(self, "session_autoplay_checkbox", None)
        checked = bool(checkbox and checkbox.GetValue())
        self.set_session_autoplay_next(checked)

    def set_session_autoplay_next(self, checked: bool, announce: bool = True) -> None:
        if getattr(self.settings, "autoplay_next", False):
            self.session_autoplay_next = False
            return
        self.session_autoplay_next = bool(checked)
        checkbox = getattr(self, "session_autoplay_checkbox", None)
        if checkbox is not None:
            try:
                checkbox.SetValue(self.session_autoplay_next)
            except RuntimeError:
                pass
        if announce:
            self.announce_player(self.t("autoplay_next_on" if self.session_autoplay_next else "autoplay_next_off"))

    def player_escape_closes_playback(self, focus: wx.Window | None) -> bool:
        if self.focus_in_results_control(focus):
            return False
        if focus is getattr(self, "player_panel", None):
            return True
        if focus is getattr(self, "fullscreen_checkbox", None):
            return True
        if focus is getattr(self, "repeat_checkbox", None):
            return True
        if focus is getattr(self, "session_autoplay_checkbox", None):
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

    def apply_tab_order(self, controls: list[wx.Window]) -> None:
        previous_control = None
        for control in controls:
            live = self.live_window(control)
            if live is None:
                continue
            if previous_control is not None:
                try:
                    live.MoveAfterInTabOrder(previous_control)
                except RuntimeError:
                    pass
            previous_control = live

    def player_tab_order(self) -> list[wx.Window]:
        ordered: list[wx.Window] = []
        ordered.extend(self.visible_player_controls(getattr(self, "player_navigation_controls", [])))
        results = self.live_window(getattr(self, "results_list", None))
        if results is not None and self.in_player_screen:
            ordered.append(results)
        panel = self.live_window(getattr(self, "player_panel", None))
        if panel is not None:
            ordered.append(panel)
        ordered.extend(self.visible_player_controls(getattr(self, "player_action_controls", [])))
        return ordered

    def handle_player_tab_navigation(self, event: wx.KeyEvent, focus: wx.Window | None) -> bool:
        if not self.in_player_screen or event.GetKeyCode() != wx.WXK_TAB:
            return False
        return self.move_player_tab_focus(not event.ShiftDown(), focus)

    def move_player_tab_focus(self, forward: bool, focus: wx.Window | None) -> bool:
        if not self.in_player_screen:
            return False
        panel = getattr(self, "player_panel", None)
        if not self.window_is_or_descendant(focus, panel):
            return False
        order = self.player_tab_order()
        try:
            index = order.index(panel)
        except ValueError:
            return False
        next_index = index + 1 if forward else index - 1
        if 0 <= next_index < len(order):
            self.safe_set_focus(order[next_index])
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
            self.restore_search_return_context(self.player_return_data)
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

    def ensure_player_for_auxiliary_view(self, callback) -> bool:
        if self.in_player_screen:
            return True
        if self.player_is_active():
            self.show_current_player_screen()
            wx.CallAfter(callback)
            return False
        self.announce_player(self.t("no_player"))
        return False

    def current_chapters(self) -> list[dict]:
        chapters = self.normalized_chapters((self.current_video_info or {}).get("chapters"))
        if chapters:
            return chapters
        chapters = self.current_podcast_chapters()
        if chapters:
            return chapters
        if self.player_kind == "mpv" and self.mpv_process_alive():
            try:
                chapters = self.normalized_chapters(self.mpv_get_property("chapter-list", timeout=0.5))
            except Exception:
                chapters = []
        if chapters:
            if not isinstance(self.current_video_info, dict):
                self.current_video_info = {}
            self.current_video_info["chapters"] = chapters
            if self.current_video_item is not None:
                self.current_video_item["chapters"] = chapters
        return chapters

    def current_podcast_chapters(self) -> list[dict]:
        item = self.current_video_info or self.current_video_item or {}
        if not isinstance(item, dict):
            return []
        chapters_url = str(item.get("chapters_url") or "").strip()
        if not chapters_url or bool(item.get("_chapters_url_checked")):
            return []
        try:
            chapters = self.fetch_podcast_chapters(chapters_url)
        except Exception:
            chapters = []
        self.cache_current_podcast_chapters(chapters, checked=True)
        return chapters

    def fetch_podcast_chapters(self, url: str) -> list[dict]:
        request = Request(url, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=20) as response:
            payload = json.loads(response.read(1_000_000).decode("utf-8", errors="replace"))
        if isinstance(payload, dict):
            raw = payload.get("chapters") or payload.get("items") or []
        else:
            raw = payload
        return self.normalized_chapters(raw)

    def cache_current_podcast_chapters(self, chapters: list[dict], checked: bool = False) -> None:
        if not isinstance(self.current_video_info, dict):
            self.current_video_info = {}
        if chapters:
            self.current_video_info["chapters"] = chapters
        if checked:
            self.current_video_info["_chapters_url_checked"] = True
        if self.current_video_item is not None:
            if chapters:
                self.current_video_item["chapters"] = chapters
            if checked:
                self.current_video_item["_chapters_url_checked"] = True

    def chapter_line(self, chapter: dict, index: int) -> str:
        title = str(chapter.get("title") or f"{self.t('chapters')} {index + 1}")
        start = self.format_seconds(float(chapter.get("start_time") or 0.0))
        end = chapter.get("end_time")
        if end is not None:
            return f"{index + 1}. {start} - {self.format_seconds(float(end))}. {title}"
        return f"{index + 1}. {start}. {title}"

    def show_chapters(self) -> None:
        if not self.ensure_player_for_auxiliary_view(self.show_chapters):
            return
        chapters = self.current_chapters()
        if not chapters:
            self.announce_player(self.t("no_chapters_available"))
            return
        dialog = wx.Dialog(self, title=self.t("chapters"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("chapters"))
        dialog.SetMinSize((560, 420))
        outer = wx.BoxSizer(wx.VERTICAL)
        chapter_list = wx.ListBox(dialog, choices=[self.chapter_line(chapter, index) for index, chapter in enumerate(chapters)])
        chapter_list.SetName(self.t("chapter_list"))
        chapter_list.SetSelection(max(0, self.current_chapter_index(chapters)))
        outer.Add(chapter_list, 1, wx.EXPAND | wx.ALL, 8)
        row = wx.BoxSizer(wx.HORIZONTAL)
        play_button = wx.Button(dialog, label=self.t("play"))
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        row.Add(play_button, 0, wx.RIGHT, 8)
        row.Add(close_button, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.ALL, 8)
        dialog.SetSizer(outer)

        def selected_index() -> int:
            try:
                index = chapter_list.GetSelection()
            except RuntimeError:
                return -1
            return index if 0 <= index < len(chapters) else -1

        def play_selected(_event=None) -> None:
            index = selected_index()
            if index >= 0:
                self.seek_to_chapter(chapters[index])
                dialog.EndModal(wx.ID_OK)

        def on_chapter_key(event: wx.KeyEvent) -> None:
            key_code = event.GetKeyCode()
            if self.shortcut_matches(event, "open_selected") or key_code in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}:
                play_selected()
                return
            if self.shortcut_matches(event, "player_back"):
                dialog.EndModal(wx.ID_CANCEL)
                return
            event.Skip()

        chapter_list.Bind(wx.EVT_LISTBOX_DCLICK, play_selected)
        chapter_list.Bind(wx.EVT_KEY_DOWN, on_chapter_key)
        dialog.Bind(wx.EVT_CHAR_HOOK, on_chapter_key)
        play_button.Bind(wx.EVT_BUTTON, play_selected)
        try:
            play_button.SetDefault()
        except RuntimeError:
            pass
        dialog.ShowModal()
        dialog.Destroy()
        self.focus_player_target_later("player")

    def current_chapter_index(self, chapters: list[dict] | None = None) -> int:
        chapters = chapters or self.current_chapters()
        if not chapters:
            return -1
        try:
            position = float(self.mpv_get_property("time-pos", timeout=0.35) or 0.0)
        except Exception:
            position = 0.0
        selected = 0
        for index, chapter in enumerate(chapters):
            if float(chapter.get("start_time") or 0.0) <= position + 0.1:
                selected = index
            else:
                break
        return selected

    def seek_to_chapter(self, chapter: dict) -> None:
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            self.announce_player(self.t("no_player"))
            return
        try:
            start = max(0.0, float(chapter.get("start_time") or 0.0))
            self.cancel_clip_preview()
            self.mpv_send(["seek", start, "absolute+exact"], timeout=0.8)
            title = str(chapter.get("title") or self.t("chapters"))
            self.announce_player(self.t("chapter_selected", title=title, time=self.format_seconds(start)))
        except Exception:
            self.announce_player(self.t("timing_unavailable"))

    def seek_relative_chapter(self, delta: int) -> None:
        chapters = self.current_chapters()
        if not chapters:
            self.announce_player(self.t("no_chapters_available"))
            return
        try:
            position = float(self.mpv_get_property("time-pos", timeout=0.35) or 0.0)
        except Exception:
            position = 0.0
        target_index = -1
        if delta > 0:
            for index, chapter in enumerate(chapters):
                if float(chapter.get("start_time") or 0.0) > position + 0.75:
                    target_index = index
                    break
        else:
            previous = [index for index, chapter in enumerate(chapters) if float(chapter.get("start_time") or 0.0) < position - 1.5]
            target_index = previous[-1] if previous else 0
        if target_index < 0 or target_index >= len(chapters):
            self.announce_player(self.t("no_chapters_available"))
            return
        self.seek_to_chapter(chapters[target_index])

    def show_lyrics(self) -> None:
        if not self.ensure_player_for_auxiliary_view(self.show_lyrics):
            return
        dialog = wx.Dialog(self, title=self.t("lyrics"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("lyrics"))
        dialog.SetMinSize((620, 460))
        outer = wx.BoxSizer(wx.VERTICAL)
        lyrics_text = wx.TextCtrl(dialog, value=self.t("lyrics_fetching"), style=wx.TE_MULTILINE | wx.TE_READONLY | wx.TE_RICH2 | wx.VSCROLL | wx.HSCROLL | wx.WANTS_CHARS)
        lyrics_text.SetName(self.t("lyrics"))
        outer.Add(lyrics_text, 1, wx.EXPAND | wx.ALL, 8)
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        outer.Add(close_button, 0, wx.ALIGN_RIGHT | wx.LEFT | wx.RIGHT | wx.BOTTOM, 8)
        dialog.SetSizer(outer)

        def set_lyrics_text(text: str, source: str = "") -> None:
            try:
                value = text.strip() if text and text.strip() else self.t("no_lyrics_available")
                if source and value != self.t("no_lyrics_available"):
                    value = f"{source}\n\n{value}"
                lyrics_text.SetValue(value)
                lyrics_text.SetInsertionPoint(0)
                lyrics_text.SetFocus()
                self.announce_player(self.t("lyrics") if text else self.t("no_lyrics_available"))
            except RuntimeError:
                pass

        local_lyrics = self.local_lyrics_text()
        if local_lyrics:
            wx.CallAfter(set_lyrics_text, local_lyrics, self.t("lyrics_source_local"))
        elif bool(getattr(self.settings, "enable_online_lyrics", True)):
            threading.Thread(target=self.fetch_lyrics_worker, args=(self.lyrics_search_terms(), set_lyrics_text), daemon=True).start()
        else:
            wx.CallAfter(set_lyrics_text, "", "")
        dialog.ShowModal()
        dialog.Destroy()
        self.focus_player_target_later("player")

    def local_lyrics_text(self) -> str:
        item = self.current_video_item or self.current_video_info or {}
        path = self.local_media_path_from_input(str(item.get("path") or item.get("url") or item.get("webpage_url") or ""))
        if not path:
            return ""
        candidates = [
            path.with_suffix(".lrc"),
            path.with_suffix(".txt"),
            path.with_name(f"{path.stem}.lyrics.txt"),
        ]
        for candidate in candidates:
            try:
                if candidate.exists() and candidate.is_file() and candidate.stat().st_size <= 512_000:
                    return candidate.read_text(encoding="utf-8", errors="replace").strip()
            except OSError:
                continue
        return ""

    def lyrics_search_terms(self) -> tuple[str, str, str, int]:
        info = self.current_video_info or self.current_video_item or {}
        title = str(info.get("track") or info.get("title") or "").strip()
        artist = str(info.get("artist") or info.get("creator") or "").strip()
        album = str(info.get("album") or "").strip()
        if not artist and " - " in title:
            left, right = title.split(" - ", 1)
            artist = left.strip()
            title = right.strip()
        title = re.sub(r"\s*[\(\[]\s*(official\s+)?(music\s+video|video|lyrics?|lyric\s+video|audio|visualizer|remaster(?:ed)?)\s*[\)\]]\s*", " ", title, flags=re.IGNORECASE)
        title = re.sub(r"\s+", " ", title).strip(" -")
        if not artist:
            artist = str(info.get("channel") or "").strip()
        duration = self.to_int(str(info.get("duration_seconds") or 0), 0, 0)
        return artist, title, album, duration

    def fetch_lyrics_worker(self, search_terms: tuple[str, str, str, int], callback) -> None:
        text = ""
        try:
            text = self.fetch_online_lyrics(search_terms)
        except Exception:
            text = ""
        wx.CallAfter(callback, text, self.t("lyrics_source_online") if text else "")

    def fetch_online_lyrics(self, search_terms: tuple[str, str, str, int] | None = None) -> str:
        artist, title, album, duration = search_terms or self.lyrics_search_terms()
        if not title:
            return ""
        params = {"track_name": title}
        if artist:
            params["artist_name"] = artist
        if album:
            params["album_name"] = album
        if duration:
            params["duration"] = str(duration)
        request = Request(f"{LRCLIB_API_GET_URL}?{urlencode(params)}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=20) as response:
            payload = json.loads(response.read().decode("utf-8", errors="replace"))
        if not isinstance(payload, dict):
            return ""
        return str(payload.get("syncedLyrics") or payload.get("plainLyrics") or "").strip()

    def show_comments(self) -> None:
        if not self.ensure_player_for_auxiliary_view(self.show_comments):
            return
        source_item = self.current_video_info or self.current_video_item or {}
        video_id = self.extract_youtube_video_id(source_item)
        if not video_id:
            self.announce_player(self.t("comments_disabled"))
            return
        source_url = self.youtube_comments_source_url(source_item, video_id)
        dialog = wx.Dialog(self, title=self.t("comments"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("comments"))
        dialog.SetMinSize((680, 500))
        outer = wx.BoxSizer(wx.VERTICAL)
        comments_list = wx.ListBox(dialog, choices=[self.t("comments_loading")])
        comments_list.SetName(self.t("comments"))
        comments_list.SetSelection(0)
        outer.Add(comments_list, 1, wx.EXPAND | wx.ALL, 8)
        row = wx.BoxSizer(wx.HORIZONTAL)
        open_button = wx.Button(dialog, label=self.t("open_comment"))
        more_button = wx.Button(dialog, label=self.t("load_more_comments"))
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        row.Add(open_button, 0, wx.RIGHT, 8)
        row.Add(more_button, 0, wx.RIGHT, 8)
        row.Add(close_button, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.LEFT | wx.RIGHT | wx.BOTTOM, 8)
        dialog.SetSizer(outer)
        open_button.Enable(False)
        more_button.Enable(False)
        state: dict[str, object] = {"comments": [], "next_page": "", "loading": False, "loaded_once": False, "source_key": ""}

        def refresh_comments(selection: int = 0) -> None:
            comments = list(state.get("comments") or [])
            labels = [self.comment_line(comment, index) for index, comment in enumerate(comments)] or [self.t("comments_disabled")]
            comments_list.Set(labels)
            comments_list.SetSelection(min(max(0, selection), len(labels) - 1))
            open_button.Enable(bool(comments))
            more_button.Enable(bool(state.get("next_page")) and not bool(state.get("loading")))

        def finish_load(new_comments: list[dict], next_page: str, error: str = "", source_key: str = "") -> None:
            try:
                state["loading"] = False
                if error:
                    comments_list.Set([self.t("comments_failed", error=error)])
                    comments_list.SetSelection(0)
                    more_button.Enable(False)
                    self.announce_player(self.t("comments_failed", error=error))
                    return
                existing = list(state.get("comments") or [])
                state["comments"] = existing + list(new_comments or [])
                state["next_page"] = next_page
                if source_key:
                    state["source_key"] = source_key
                state["loaded_once"] = True
                refresh_comments(len(existing) if existing else 0)
                total = len(state.get("comments") or [])
                if total:
                    source = self.t(str(state.get("source_key") or ""))
                    message = self.t("comments_loaded_from_source", count=total, source=source) if source else self.t("comments_loaded", count=total)
                else:
                    message = self.t("comments_disabled")
                self.announce_player(message)
            except RuntimeError:
                pass

        def load_more(_event=None) -> None:
            if state.get("loading"):
                return
            if (state.get("comments") or state.get("loaded_once")) and not state.get("next_page"):
                self.announce_player(self.t("no_more_comments"))
                return
            state["loading"] = True
            more_button.Enable(False)
            if not state.get("comments"):
                comments_list.Set([self.t("comments_loading")])
                comments_list.SetSelection(0)
            page_token = str(state.get("next_page") or "")
            threading.Thread(target=self.fetch_comments_worker, args=(video_id, page_token, source_url, finish_load), daemon=True).start()

        def open_selected_comment(_event=None) -> None:
            comments = list(state.get("comments") or [])
            try:
                index = comments_list.GetSelection()
            except RuntimeError:
                index = -1
            if 0 <= index < len(comments):
                self.show_comment_details(comments[index])

        def on_comments_key(event: wx.KeyEvent) -> None:
            if self.shortcut_matches(event, "open_selected"):
                open_selected_comment()
                return
            if self.shortcut_matches(event, "player_back"):
                dialog.EndModal(wx.ID_CANCEL)
                return
            event.Skip()

        comments_list.Bind(wx.EVT_LISTBOX_DCLICK, open_selected_comment)
        comments_list.Bind(wx.EVT_KEY_DOWN, on_comments_key)
        open_button.Bind(wx.EVT_BUTTON, open_selected_comment)
        more_button.Bind(wx.EVT_BUTTON, load_more)
        load_more()
        dialog.ShowModal()
        dialog.Destroy()
        self.focus_player_target_later("player")

    def fetch_comments_worker(self, video_id: str, page_token: str, source_url: str, callback) -> None:
        api_error = ""
        try:
            if self.youtube_data_api_key():
                try:
                    comments, next_page = self.fetch_youtube_comments(video_id, page_token)
                    wx.CallAfter(callback, comments, next_page, "", "comments_source_api")
                    return
                except Exception as exc:
                    api_error = self.friendly_error(exc)
                    if page_token:
                        raise
            comments = self.fetch_ytdlp_comments(video_id, source_url)
            wx.CallAfter(callback, comments, "", "", "comments_source_ytdlp")
        except Exception as exc:
            error = self.friendly_error(exc)
            if api_error and api_error != error:
                error = f"{api_error}\n\n{error}"
            wx.CallAfter(callback, [], "", error)

    def fetch_youtube_comments(self, video_id: str, page_token: str = "") -> tuple[list[dict], str]:
        params = {
            "part": "snippet,replies",
            "videoId": video_id,
            "maxResults": "20",
            "order": "relevance",
            "textFormat": "plainText",
            "key": self.youtube_data_api_key(),
        }
        if page_token:
            params["pageToken"] = page_token
        payload = self.youtube_api_json(YOUTUBE_API_COMMENT_THREADS_URL, params)
        comments = [self.normalize_youtube_comment_thread(item) for item in list(payload.get("items") or []) if isinstance(item, dict)]
        comments = [comment for comment in comments if comment.get("text")]
        return comments, str(payload.get("nextPageToken") or "")

    def fetch_ytdlp_comments(self, video_id: str, source_url: str = "") -> list[dict]:
        url = str(source_url or "").strip() or f"https://www.youtube.com/watch?v={video_id}"
        options = {
            "quiet": True,
            "skip_download": True,
            "noplaylist": True,
            "getcomments": True,
            "extractor_args": {"youtube": {"max_comments": ["20"]}},
        }
        info = self.ydl_extract_info(url, options, download=False)
        comments: list[dict] = []
        for raw in list((info or {}).get("comments") or [])[:20]:
            if not isinstance(raw, dict):
                continue
            text = self.strip_html(str(raw.get("text") or ""))
            if not text:
                continue
            comments.append(
                {
                    "id": str(raw.get("id") or ""),
                    "author": str(raw.get("author") or raw.get("author_id") or "").strip(),
                    "text": text,
                    "published": self.format_history_time(raw.get("timestamp")) if raw.get("timestamp") else "",
                    "likes": raw.get("like_count", 0),
                    "reply_count": 0,
                    "replies": [],
                }
            )
        return comments

    def youtube_api_json(self, url: str, params: dict, timeout: int = 25) -> dict:
        request = Request(f"{url}?{urlencode(params)}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=timeout) as response:
            payload = json.loads(response.read().decode("utf-8", errors="replace"))
        if isinstance(payload, dict) and payload.get("error"):
            error = payload.get("error") or {}
            reason = ""
            try:
                reason = str(((error.get("errors") or [{}])[0] or {}).get("reason") or "")
            except Exception:
                reason = ""
            if reason == "commentsDisabled":
                raise RuntimeError(self.t("comments_disabled"))
            message = error.get("message") if isinstance(error, dict) else str(error)
            raise RuntimeError(message or self.t("comments_failed", error=""))
        return payload if isinstance(payload, dict) else {}

    def normalize_comment_snippet(self, snippet: dict) -> dict:
        text = self.strip_html(str(snippet.get("textOriginal") or snippet.get("textDisplay") or ""))
        try:
            text = import_module("html").unescape(text)
        except Exception:
            pass
        return {
            "author": str(snippet.get("authorDisplayName") or "").strip(),
            "text": text.strip(),
            "published": str(snippet.get("publishedAt") or "").strip(),
            "updated": str(snippet.get("updatedAt") or "").strip(),
            "likes": snippet.get("likeCount", 0),
        }

    def normalize_youtube_comment_thread(self, item: dict) -> dict:
        snippet = item.get("snippet") or {}
        top = snippet.get("topLevelComment") or {}
        top_snippet = top.get("snippet") or {}
        comment = self.normalize_comment_snippet(top_snippet)
        comment["id"] = str(top.get("id") or item.get("id") or "")
        comment["reply_count"] = self.to_int(str(snippet.get("totalReplyCount") or 0), 0, 0)
        replies = []
        for reply in list(((item.get("replies") or {}).get("comments") or [])):
            if isinstance(reply, dict):
                reply_data = self.normalize_comment_snippet(reply.get("snippet") or {})
                reply_data["id"] = str(reply.get("id") or "")
                if reply_data.get("text"):
                    replies.append(reply_data)
        comment["replies"] = replies
        return comment

    def comment_line(self, comment: dict, index: int) -> str:
        text = " ".join(str(comment.get("text") or "").split())
        if len(text) > 140:
            text = text[:137].rstrip() + "..."
        author = str(comment.get("author") or self.t("comments"))
        likes = self.format_count(comment.get("likes"))
        replies = self.to_int(str(comment.get("reply_count") or 0), 0, 0)
        parts = [f"{index + 1}. {author}", text]
        if likes:
            parts.append(self.t("comment_likes", count=likes))
        if replies:
            parts.append(self.t("comment_replies_count", count=replies))
        return " | ".join(part for part in parts if part)

    def show_comment_details(self, comment: dict) -> None:
        details = self.comment_details_text(comment)
        dialog = wx.Dialog(self, title=self.t("comment_details"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("comment_details"))
        dialog.SetMinSize((620, 420))
        outer = wx.BoxSizer(wx.VERTICAL)
        text = wx.TextCtrl(dialog, value=details, style=wx.TE_MULTILINE | wx.TE_READONLY | wx.TE_RICH2 | wx.VSCROLL | wx.HSCROLL | wx.WANTS_CHARS)
        text.SetName(self.t("comment_details"))
        outer.Add(text, 1, wx.EXPAND | wx.ALL, 8)
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        outer.Add(close_button, 0, wx.ALIGN_RIGHT | wx.LEFT | wx.RIGHT | wx.BOTTOM, 8)
        dialog.SetSizer(outer)
        wx.CallAfter(text.SetFocus)
        dialog.ShowModal()
        dialog.Destroy()

    def comment_details_text(self, comment: dict) -> str:
        lines = [
            str(comment.get("author") or ""),
            str(comment.get("published") or ""),
            self.t("comment_likes", count=self.format_count(comment.get("likes"))) if comment.get("likes") not in (None, "") else "",
            "",
            str(comment.get("text") or ""),
        ]
        replies = list(comment.get("replies") or [])
        if replies:
            lines.extend(["", self.t("comment_replies")])
            for reply in replies:
                lines.extend(["", str(reply.get("author") or ""), str(reply.get("text") or "")])
        reply_count = self.to_int(str(comment.get("reply_count") or 0), 0, 0)
        if reply_count and reply_count > len(replies):
            lines.extend(["", self.t("comment_more_replies", count=reply_count - len(replies))])
        return "\n".join(line for line in lines if line is not None)

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
            f"{self.t('type')}: {self.item_type_label(info)}",
            f"Duration: {info.get('duration') or self.format_duration(info.get('duration_seconds'))}",
            f"Playback speed: {info.get('speed') or self.settings.player_speed}x",
            f"{self.t('pitch_label')}: {info.get('pitch') or '1.00'}x",
            f"{self.t('description')}:",
            info.get("description") or "",
        ]
        return "\n".join(line for line in lines if line is not None)

    def active_item(self) -> dict | None:
        focus = wx.Window.FindFocus()
        if self.focus_in_results_control(focus):
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

    def copy_path_to_clipboard(self, path: str) -> None:
        if not path:
            return
        self.copy_plain_text_to_clipboard(path)
        self.announce_player(self.t("path_copied"))

    def copy_active_url(self) -> None:
        item = self.active_item()
        if item:
            self.copy_url_to_clipboard(item.get("url", ""))

    def copy_current_player_url(self) -> None:
        item = self.current_player_item()
        if self.item_is_local_media(item):
            self.copy_path_to_clipboard(str(item.get("path") or item.get("url") or item.get("webpage_url") or ""))
            return
        self.copy_url_to_clipboard(str(item.get("webpage_url") or item.get("url") or ""))

    def current_player_position_seconds(self) -> int:
        if self.player_kind == "mpv" and self.mpv_process_alive():
            try:
                return max(0, int(float(self.mpv_get_property("time-pos", timeout=0.35) or 0.0)))
            except Exception:
                pass
        return 0

    def youtube_url_at_timestamp(self, item: dict | None, seconds: int) -> str:
        video_id = self.extract_youtube_video_id(item)
        if not video_id:
            return ""
        source_url = ""
        if isinstance(item, dict):
            for key in ("webpage_url", "original_url", "watch_url", "url"):
                candidate = str(item.get(key) or "").strip()
                if not candidate:
                    continue
                try:
                    host = (urlparse(candidate).netloc or "").lower()
                except Exception:
                    continue
                if ("youtube.com" in host or "youtu.be" in host) and "googlevideo.com" not in host:
                    source_url = candidate
                    break
        params: list[tuple[str, str]] = [("v", video_id)]
        if source_url:
            try:
                for key, value in parse_qsl(urlparse(source_url).query, keep_blank_values=True):
                    if key.lower() in {"v", "t", "start", "time_continue"}:
                        continue
                    params.append((key, value))
            except Exception:
                pass
        params.append(("t", f"{max(0, int(seconds))}s"))
        return f"https://www.youtube.com/watch?{urlencode(params)}"

    def copy_current_player_timestamp_url(self) -> None:
        url = self.youtube_url_at_timestamp(self.current_player_item(), self.current_player_position_seconds())
        if not url:
            self.announce_player(self.t("timestamp_url_unavailable"))
            return
        self.copy_plain_text_to_clipboard(url)
        self.announce_player(self.t("timestamp_url_copied"))

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
        enabled, gains = self.base_equalizer_state()
        if self.bass_boost_enabled:
            return True, self.equalizer_gains_with_bass_boost(gains if enabled else default_equalizer_gains())
        return enabled, gains

    def base_equalizer_state(self) -> tuple[bool, dict[str, float]]:
        if self.session_equalizer_enabled is not None:
            return bool(self.session_equalizer_enabled), self.normalized_equalizer_gains(self.session_equalizer_gains)
        preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        return bool(getattr(self.settings, "global_equalizer_enabled", False)), self.equalizer_gains_for_preset(preset)

    def equalizer_gains_with_bass_boost(self, gains: dict[str, float]) -> dict[str, float]:
        combined = self.normalized_equalizer_gains(gains)
        boost = self.factory_equalizer_gains_for_preset("bass_boost")
        for band_id, _band_label in EQ_BANDS:
            combined[band_id] = round(max(-24.0, min(24.0, combined.get(band_id, 0.0) + boost.get(band_id, 0.0))), 1)
        return combined

    def use_global_equalizer_for_live_preview(self) -> None:
        self.session_equalizer_enabled = None
        self.session_equalizer_gains = {}
        self.session_equalizer_before_bass_boost = None

    def use_visible_equalizer_for_live_preview(self) -> None:
        self.session_equalizer_enabled = True
        self.session_equalizer_gains = self.visible_equalizer_gains()
        self.session_equalizer_before_bass_boost = None

    @staticmethod
    def equalizer_band_filter(band_id: str, gain: float) -> str:
        width = EQ_FILTER_Q_WIDTHS.get(str(band_id), EQ_FILTER_Q_WIDTH)
        return f"equalizer=f={band_id}:t=q:w={width:g}:g={gain:.1f}"

    @staticmethod
    def equalizer_has_positive_gain(gains: dict[str, float]) -> bool:
        for band_id, _band_label in EQ_BANDS:
            try:
                if float(gains.get(band_id, 0.0) or 0.0) > 0.05:
                    return True
            except (TypeError, ValueError):
                continue
        return False

    @classmethod
    def equalizer_filter(cls, gains: dict[str, float], protect_clipping: bool = False, label: str = EQ_FILTER_LABEL) -> str:
        filters = []
        if protect_clipping:
            headroom = cls.equalizer_clipping_headroom_db(gains)
            if headroom <= -0.05:
                filters.append(f"volume={headroom:.1f}dB")
        for band_id, _band_label in EQ_BANDS:
            gain = max(-24.0, min(24.0, float(gains.get(band_id, 0.0) or 0.0)))
            if abs(gain) >= 0.05:
                filters.append(cls.equalizer_band_filter(band_id, gain))
        if protect_clipping and filters:
            filters.append(EQ_LIMITER_FILTER)
        return f"@{label}:lavfi=[{','.join(filters)}]"

    @staticmethod
    def equalizer_clipping_headroom_db(gains: dict[str, float]) -> float:
        max_positive = 0.0
        for band_id, _band_label in EQ_BANDS:
            try:
                max_positive = max(max_positive, float(gains.get(band_id, 0.0) or 0.0))
            except (TypeError, ValueError):
                continue
        return -min(EQ_CLIPPING_HEADROOM_LIMIT_DB, max(0.0, max_positive))

    def equalizer_clipping_protection_active(self, gains: dict[str, float]) -> bool:
        return (
            bool(getattr(self.settings, "equalizer_clipping_protection", False))
            and self.equalizer_has_positive_gain(gains)
        )

    def schedule_equalizer_apply(self, delay_ms: int = EQ_APPLY_DELAY_MS) -> None:
        self.equalizer_apply_generation += 1
        generation = self.equalizer_apply_generation
        timer = getattr(self, "equalizer_apply_timer", None)
        if timer is not None and timer.IsRunning():
            timer.Stop()
        self.equalizer_apply_timer = wx.CallLater(max(0, int(delay_ms)), self.apply_scheduled_equalizer_to_player, generation)

    def apply_scheduled_equalizer_to_player(self, generation: int) -> None:
        if generation != getattr(self, "equalizer_apply_generation", 0):
            return
        self.apply_equalizer_to_player()

    def apply_equalizer_to_player(self, retries: int = 2) -> None:
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            return
        enabled, gains = self.effective_equalizer_state()
        if not enabled or not any(abs(float(value)) >= 0.05 for value in gains.values()):
            self.clear_equalizer_filters()
            return
        current_ref = getattr(self, "equalizer_filter_ref", EQ_FILTER_REF) if self.equalizer_filter_active else ""
        next_ref = EQ_FILTER_ALT_REF if current_ref == EQ_FILTER_REF else EQ_FILTER_REF
        next_label = EQ_FILTER_ALT_LABEL if next_ref == EQ_FILTER_ALT_REF else EQ_FILTER_LABEL
        try:
            self.mpv_request(["af", "remove", next_ref], timeout=0.8)
            response = self.mpv_request(
                ["af", "add", self.equalizer_filter(gains, self.equalizer_clipping_protection_active(gains), next_label)],
                timeout=1.2,
            )
            if response.get("error") == "success":
                if current_ref and current_ref != next_ref:
                    self.mpv_request(["af", "remove", current_ref], timeout=0.8)
                stale_ref = EQ_FILTER_ALT_REF if next_ref == EQ_FILTER_REF else EQ_FILTER_REF
                if stale_ref != current_ref:
                    self.mpv_request(["af", "remove", stale_ref], timeout=0.8)
                self.equalizer_filter_ref = next_ref
                self.equalizer_filter_active = True
                return
        except Exception:
            pass
        if not current_ref:
            self.equalizer_filter_active = False
        if retries > 0:
            wx.CallLater(180, self.apply_equalizer_to_player, retries - 1)

    def clear_equalizer_filters(self) -> None:
        for filter_ref in (EQ_FILTER_REF, EQ_FILTER_ALT_REF):
            try:
                self.mpv_request(["af", "remove", filter_ref], timeout=0.8)
            except Exception:
                pass
        self.equalizer_filter_ref = EQ_FILTER_REF
        self.equalizer_filter_active = False

    def show_player_equalizer(self) -> None:
        if not self.player_is_active():
            return
        original_enabled = self.session_equalizer_enabled
        original_gains = dict(self.session_equalizer_gains)
        original_db_range = self.equalizer_db_range_value()
        _enabled, gains = self.base_equalizer_state()
        active_preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        dialog_db_range = original_db_range
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
        outer.Add(wx.StaticText(dialog, label=self.t("equalizer_db_range")), 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
        range_choice = wx.Choice(dialog, choices=EQ_RANGE_OPTIONS)
        range_choice.SetName(self.t("equalizer_db_range"))
        range_choice.SetSelection(EQ_RANGE_OPTIONS.index(str(dialog_db_range)) if str(dialog_db_range) in EQ_RANGE_OPTIONS else 1)
        outer.Add(range_choice, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)
        name_label = wx.StaticText(dialog, label=self.t("equalizer_preset_name"))
        name_ctrl = wx.TextCtrl(dialog, value=self.equalizer_custom_name(active_preset))
        name_ctrl.SetName(self.t("equalizer_preset_name"))
        outer.Add(name_label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
        outer.Add(name_ctrl, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)
        sliders: dict[str, wx.Slider] = {}
        dialog_gains = self.normalized_equalizer_gains(gains)
        for band_id, band_label in EQ_BANDS:
            label_text = self.t("equalizer_band_gain", band=band_label)
            outer.Add(wx.StaticText(dialog, label=label_text), 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
            band_value = min(max(int(round(dialog_gains.get(band_id, 0.0) * 10)), -dialog_db_range * 10), dialog_db_range * 10)
            slider = wx.Slider(
                dialog,
                value=band_value,
                minValue=-dialog_db_range * 10,
                maxValue=dialog_db_range * 10,
                style=wx.SL_HORIZONTAL,
            )
            slider._apricot_eq_band_id = str(band_id)
            self.configure_equalizer_slider_steps(slider)
            self.set_equalizer_slider_accessibility(slider, label_text)
            sliders[band_id] = slider
            outer.Add(slider, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)

        buttons = wx.StdDialogButtonSizer()
        ok_button = wx.Button(dialog, wx.ID_OK)
        cancel_button = wx.Button(dialog, wx.ID_CANCEL)
        reset_button = wx.Button(dialog, label=self.t("reset_equalizer"))
        save_global_button = wx.Button(dialog, label=self.t("save_equalizer_as_global"))
        add_profile_button = wx.Button(dialog, label=self.t("add_equalizer_profile"))
        delete_profile_button = wx.Button(dialog, label=self.t("delete_equalizer_profile"))
        buttons.AddButton(ok_button)
        buttons.AddButton(cancel_button)
        buttons.Realize()
        row = wx.BoxSizer(wx.HORIZONTAL)
        row.Add(add_profile_button, 0, wx.RIGHT, 8)
        row.Add(delete_profile_button, 0, wx.RIGHT, 8)
        row.Add(save_global_button, 0, wx.RIGHT, 8)
        row.Add(reset_button, 0, wx.RIGHT, 8)
        row.Add(buttons, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.ALL, 8)
        dialog.SetSizer(outer)

        def current_dialog_gains() -> dict[str, float]:
            return self.normalized_equalizer_gains(dialog_gains)

        def current_preset() -> str:
            index = preset_choice.GetSelection()
            return preset_options[index] if 0 <= index < len(preset_options) else EQ_PRESET_FLAT

        def set_dialog_slider_value(slider: wx.Slider, value: int, label: str, *, notify: bool = False) -> None:
            slider._apricot_suppress_accessible_notify = not notify
            slider._apricot_eq_programmatic_update = True
            try:
                slider.SetValue(value)
                band_id = self.equalizer_slider_band_id(slider)
                if band_id:
                    dialog_gains[band_id] = self.equalizer_gain_from_slider_value(value)
                self.set_equalizer_slider_accessibility(slider, label)
            finally:
                slider._apricot_eq_programmatic_update = False
                slider._apricot_suppress_accessible_notify = False

        def update_custom_name_visibility() -> None:
            visible = self.is_custom_equalizer_preset(current_preset())
            name_label.Show(visible)
            name_ctrl.Show(visible)
            delete_profile_button.Enable(visible)
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
            self.schedule_equalizer_apply()

        def load_preset_into_sliders(preset_id: str) -> None:
            nonlocal dialog_visible_preset
            nonlocal dialog_gains
            dialog_visible_preset = self.normalized_equalizer_preset(preset_id)
            preset_gains = self.equalizer_gains_for_preset(preset_id)
            dialog_gains = self.normalized_equalizer_gains(preset_gains)
            for band_id, band_label in EQ_BANDS:
                value = min(max(preset_gains.get(band_id, 0.0), -dialog_db_range), dialog_db_range)
                set_dialog_slider_value(sliders[band_id], int(round(value * 10)), self.t("equalizer_band_gain", band=band_label))
            name_ctrl.SetValue(self.equalizer_custom_name(preset_id))
            update_custom_name_visibility()
            live_apply()

        def apply_dialog_db_range(value: int) -> None:
            nonlocal dialog_db_range
            dialog_db_range = min(24, max(6, int(value or 12)))
            slider_min = -dialog_db_range * 10
            slider_max = dialog_db_range * 10
            for band_id, band_label in EQ_BANDS:
                slider = sliders[band_id]
                current = min(max(int(round(dialog_gains.get(band_id, 0.0) * 10)), slider_min), slider_max)
                slider._apricot_eq_programmatic_update = True
                try:
                    slider.SetRange(slider_min, slider_max)
                finally:
                    slider._apricot_eq_programmatic_update = False
                self.configure_equalizer_slider_steps(slider)
                set_dialog_slider_value(slider, current, self.t("equalizer_band_gain", band=band_label))
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
                if getattr(ctrl, "_apricot_eq_programmatic_update", False):
                    event.Skip()
                    return
                band_id = self.equalizer_slider_band_id(ctrl)
                if not band_id:
                    return
                dialog_gains[band_id] = self.equalizer_gain_from_slider_value(ctrl.GetValue())
                self.set_equalizer_slider_accessibility(ctrl, label)
            live_apply()

        def on_range_changed(_event: wx.CommandEvent) -> None:
            index = range_choice.GetSelection()
            value = EQ_RANGE_OPTIONS[index] if 0 <= index < len(EQ_RANGE_OPTIONS) else str(original_db_range)
            apply_dialog_db_range(self.to_int(value, original_db_range, 6, 24))

        for band_id, band_label in EQ_BANDS:
            self.bind_equalizer_slider_events(sliders[band_id], lambda evt, label=self.t("equalizer_band_gain", band=band_label): on_slider(evt, label))
        preset_choice.Bind(wx.EVT_CHOICE, on_preset_changed)
        range_choice.Bind(wx.EVT_CHOICE, on_range_changed)

        def reset_dialog_equalizer(_event=None) -> None:
            nonlocal dialog_gains
            preset_gains = self.factory_equalizer_gains_for_preset(current_preset())
            dialog_gains = self.normalized_equalizer_gains(preset_gains)
            for band_id, band_label in EQ_BANDS:
                value = min(max(preset_gains.get(band_id, 0.0), -dialog_db_range), dialog_db_range)
                set_dialog_slider_value(sliders[band_id], int(round(value * 10)), self.t("equalizer_band_gain", band=band_label))
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

        def delete_profile_from_dialog(_event=None) -> None:
            preset = current_preset()
            replacement = self.delete_equalizer_profile(preset)
            if not replacement:
                return
            refresh_preset_choices(replacement)
            load_preset_into_sliders(replacement)

        delete_profile_button.Bind(wx.EVT_BUTTON, delete_profile_from_dialog)
        update_custom_name_visibility()
        result = dialog.ShowModal()
        if result == wx.ID_OK:
            save_current_dialog_name()
            self.settings.equalizer_db_range = dialog_db_range
            self.save_settings()
        dialog.Destroy()
        if result == wx.ID_OK:
            self.announce_player(self.t("equalizer_saved"))
            return
        self.session_equalizer_enabled = original_enabled
        self.session_equalizer_gains = original_gains
        self.settings.equalizer_db_range = original_db_range
        self.apply_equalizer_to_player()
        self.announce_player(self.t("equalizer_closed"))

    def play_relative_item(self, delta: int, preserve_focus: bool = False) -> None:
        sequence_active = self.current_player_sequence_active()
        if delta > 0 and not sequence_active:
            queued_item = self.pop_next_playback_queue_item()
            if queued_item:
                self.open_playback_queue_item(queued_item, announce_start=True, preserve_focus=preserve_focus)
                return
        if delta < 0:
            item = self.relative_player_item(-1)
            if not item:
                self.announce_player(self.t("no_previous_item"))
                return
        else:
            item = self.relative_player_item(1)
            if not item:
                if not sequence_active and self.request_player_next_dynamic_load(preserve_focus=preserve_focus):
                    self.set_status(self.t("loading_more_results"))
                    return
                self.announce_player(self.t("no_next_item"))
                return
        self.open_relative_player_item(item, announce_start=True, preserve_focus=preserve_focus)

    def clear_player_sequence(self) -> None:
        self.player_sequence_results = []

    def set_player_sequence(self, items: list[dict]) -> None:
        self.player_sequence_results = [dict(item) for item in items if item.get("url")]

    def player_sequence_contains_url(self, url: str) -> bool:
        if not url:
            return False
        return any(str(item.get("url") or "") == url for item in self.player_sequence_results)

    def player_sequence_contains_item(self, item: dict | None) -> bool:
        return self.player_sequence_contains_url(str((item or {}).get("url") or ""))

    def current_player_sequence_active(self) -> bool:
        return self.player_sequence_contains_url(str((self.current_video_item or {}).get("url") or ""))

    def player_navigation_results(self) -> list[dict]:
        current_url = str((self.current_video_item or {}).get("url") or "")
        if self.player_sequence_results and current_url and self.player_sequence_contains_url(current_url):
            return list(self.player_sequence_results)
        collections = [self.return_all_results, self.all_results, self.return_results, self.results]
        non_empty = [list(items) for items in collections if items]
        if not non_empty:
            return []
        if self.player_return_screen in {"search", "trending", "playback_queue"}:
            if current_url:
                for items in non_empty:
                    if any(str(item.get("url") or "") == current_url for item in items):
                        return items
        return non_empty[0]

    def sync_results_selection_to_player_item(self, item: dict | None) -> None:
        results_list = self.live_window(getattr(self, "results_list", None))
        if results_list is None or not item:
            return
        url = str(item.get("url") or "")
        if not url:
            return
        all_results = list(self.return_all_results or self.all_results or self.return_results or self.results)
        index = next((i for i, result in enumerate(all_results) if str(result.get("url") or "") == url), -1)
        if index < 0:
            return
        if index >= len(self.results) and index < len(all_results):
            previous_count = len(self.results)
            self.all_results = list(all_results)
            self.last_visible_count = min(len(self.all_results), index + 1)
            self.results = self.all_results[: self.last_visible_count]
            labels = [self.result_line(row, result) for row, result in enumerate(self.results)]
            if not self.append_listbox_items(results_list, labels, previous_count, index):
                self.set_listbox_items(results_list, labels, index)
        if index >= len(self.results):
            return
        self.current_index = index
        self.remember_user_result_selection(index)
        try:
            if results_list.GetSelection() != index:
                self.results_selection_update_suppressed = True
                results_list.SetSelection(index)
                wx.CallAfter(self.clear_results_selection_update_suppression)
        except RuntimeError:
            pass

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
        results = self.player_navigation_results()
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

    def open_relative_player_item(self, item: dict, announce_start: bool = False, preserve_focus: bool = False) -> None:
        if not item.get("url"):
            return
        if not self.player_sequence_contains_item(item):
            self.clear_player_sequence()
        data = dict(self.player_return_data or {})
        keep_current_ui = bool(preserve_focus and self.live_window(getattr(self, "player_panel", None)) is not None)
        show_player = (self.in_player_screen or not self.background_playback_enabled()) and not keep_current_ui
        focus_target = "player" if keep_current_ui else ("results" if preserve_focus and self.live_window(getattr(self, "results_list", None)) is not None else "player")
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
            folder = str(data.get("folder") or self.current_local_folder_path or self.last_search_query)
            results = [dict(result) for result in (self.return_all_results or self.current_local_folder_items or self.all_results or self.return_results or self.results) if result.get("kind") == "local_file" and result.get("url")]
            if folder and not results:
                results = self.cached_local_folder_items(Path(folder))
            if results:
                self.current_local_folder_path = folder
                self.current_local_folder_items = [dict(result) for result in results]
                self.return_results = list(results)
                self.return_all_results = list(results)
                self.return_visible_count = len(results)
            self.return_index = next((i for i, result in enumerate(results) if result.get("url") == item.get("url")), self.return_index)
            self.player_return_screen = "folder"
            self.player_return_data = {"index": self.return_index, "folder": folder}
        else:
            results = self.return_all_results or self.all_results or self.return_results or self.results
            self.return_index = next((i for i, result in enumerate(results) if result.get("url") == item.get("url")), self.return_index)
            self.player_return_screen = "search"
            self.player_return_data = self.search_return_data(self.return_index)
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.sync_results_selection_to_player_item(item)
        self.play_url(
            str(item.get("url") or ""),
            str(item.get("title") or ""),
            show_player=show_player,
            announce_start=announce_start,
            focus_target=focus_target,
            keep_current_ui=keep_current_ui,
        )

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

    def set_auto_folder_playback_queue(self, queue_items: list[dict]) -> None:
        manual_items = [item for item in self.playback_queue if not item.get("_auto_folder_queue")]
        self.playback_queue = [dict(item) for item in queue_items] + manual_items
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
            self.item_type_label(item, default=""),
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
        clear_button = wx.Button(dialog, label=self.t("clear_playback_queue"))
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        row.Add(play_button, 0, wx.RIGHT, 8)
        row.Add(move_up_button, 0, wx.RIGHT, 8)
        row.Add(move_down_button, 0, wx.RIGHT, 8)
        row.Add(remove_button, 0, wx.RIGHT, 8)
        row.Add(clear_button, 0, wx.RIGHT, 8)
        row.Add(close_button, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.ALL, 8)
        dialog.SetSizer(outer)
        action: dict[str, int | str] = {}

        def selected_index() -> int:
            try:
                index = queue_list.GetSelection()
            except RuntimeError:
                return -1
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

        def clear_queue(_event=None) -> None:
            if not self.playback_queue:
                self.announce_player(self.t("playback_queue_empty"))
                return
            self.playback_queue = []
            self.save_playback_queue()
            refresh_queue_list(0)
            self.announce_player(self.t("playback_queue_cleared"))
            dialog.EndModal(wx.ID_CANCEL)

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
                (self.t("clear_playback_queue"), clear_queue),
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
        clear_button.Bind(wx.EVT_BUTTON, clear_queue)
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

    def open_playback_queue_item(self, item: dict, announce_start: bool = False, preserve_focus: bool = False) -> None:
        keep_current_ui = bool(preserve_focus and self.live_window(getattr(self, "player_panel", None)) is not None)
        show_player = (self.in_player_screen or not self.background_playback_enabled()) and not keep_current_ui
        focus_target = "player" if keep_current_ui else ("results" if preserve_focus and self.live_window(getattr(self, "results_list", None)) is not None else "player")
        self.open_playback_queue_item_with_mode(
            item,
            show_player=show_player,
            announce_start=announce_start,
            focus_target=focus_target,
            keep_current_ui=keep_current_ui,
        )

    def open_playback_queue_item_with_mode(
        self,
        item: dict,
        show_player: bool = True,
        announce_start: bool = False,
        focus_target: str = "player",
        keep_current_ui: bool = False,
    ) -> None:
        url = str(item.get("url") or "")
        if not url:
            self.announce_player(self.t("no_selection"))
            return
        self.clear_player_sequence()
        source_screen = str(item.get("_return_screen") or "")
        if source_screen == "folder":
            folder = str(item.get("_return_folder") or self.current_local_folder_path or self.last_search_query)
            folder_items = self.current_local_folder_items if folder == self.current_local_folder_path else []
            if folder and not folder_items:
                folder_items = self.cached_local_folder_items(Path(folder))
            if folder_items:
                self.current_local_folder_path = folder
                self.current_local_folder_items = [dict(result) for result in folder_items]
                self.return_results = list(self.current_local_folder_items)
                self.return_all_results = list(self.current_local_folder_items)
                self.return_visible_count = len(self.current_local_folder_items)
            self.player_return_screen = "folder"
            self.player_return_data = {
                "index": int(item.get("_return_index") or 0),
                "folder": folder,
            }
            self.return_index = int(item.get("_return_index") or 0)
        else:
            self.player_return_screen = "playback_queue"
            self.player_return_data = {}
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(
            url,
            str(item.get("title") or ""),
            show_player=show_player,
            announce_start=announce_start,
            focus_target=focus_target,
            keep_current_ui=keep_current_ui,
        )

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
        protect_clipping = self.equalizer_clipping_protection_active(gains)
        if protect_clipping:
            headroom = self.equalizer_clipping_headroom_db(gains)
            if headroom <= -0.05:
                filters.append(f"volume={headroom:.1f}dB")
        for band_id, _band_label in EQ_BANDS:
            gain = max(-24.0, min(24.0, float(gains.get(band_id, 0.0) or 0.0)))
            if abs(gain) >= 0.05:
                filters.append(self.equalizer_band_filter(band_id, gain))
        if protect_clipping and filters:
            filters.append(EQ_LIMITER_FILTER)
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
        self.clear_player_sequence()
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
        playlist_count = self.to_int(str(task.get("playlist_count") or 0), 0, 0)
        if playlist_count and not self.to_int(str(task.get("total") or 0), 0, 0):
            task["total"] = playlist_count
        self.update_download_progress_dialog(task)
        self.refresh_download_views(update_menu=False)

    def finish_download_task(self, task_id: str, status_key: str = "download_state_done") -> None:
        task = self.active_downloads.get(task_id)
        if task:
            task["status_key"] = status_key
        self.close_download_progress_dialog(task_id)
        self.download_cancel_events.pop(task_id, None)
        self.active_downloads.pop(task_id, None)
        self.refresh_download_views()

    def show_download_progress_dialog(self, task_id: str, title: str) -> None:
        self.close_download_progress_dialog()
        self.download_progress_task_id = task_id
        self.download_progress_dialog = wx.ProgressDialog(
            self.t("download_progress_title"),
            self.t("download_progress_message", title=title, completed=0, total=0, remaining=0),
            maximum=100,
            parent=self,
            style=wx.PD_ELAPSED_TIME | wx.PD_ESTIMATED_TIME | wx.PD_REMAINING_TIME,
        )

    def update_download_progress_dialog(self, task: dict) -> None:
        dialog = self.download_progress_dialog
        if not dialog or str(task.get("task_id") or "") != self.download_progress_task_id:
            return
        total = self.to_int(str(task.get("total") or task.get("playlist_count") or 0), 0, 0)
        completed = self.to_int(str(task.get("completed") or 0), 0, 0)
        playlist_index = self.to_int(str(task.get("playlist_index") or 0), 0, 0)
        if total and playlist_index:
            if task.get("status_key") == "download_state_processing":
                completed = max(completed, min(total, playlist_index))
            else:
                completed = max(completed, min(total, max(0, playlist_index - 1)))
        remaining = max(0, total - completed) if total else 0
        if total:
            percent = max(0, min(100, int(round((completed / total) * 100))))
        else:
            percent = self.to_int(str(task.get("percent") or 0), 0, 0, 100)
        title = str(task.get("current_title") or task.get("title") or "")
        message = self.t("download_progress_message", title=title, completed=completed, total=total, remaining=remaining)
        try:
            dialog.Update(percent, message)
        except RuntimeError:
            self.download_progress_dialog = None
            self.download_progress_task_id = ""

    def close_download_progress_dialog(self, task_id: str | None = None) -> None:
        if task_id is not None and task_id != self.download_progress_task_id:
            return
        dialog = self.download_progress_dialog
        self.download_progress_dialog = None
        self.download_progress_task_id = ""
        if dialog:
            try:
                dialog.Destroy()
            except RuntimeError:
                pass

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
        if self.effective_autoplay_next():
            sequence_active = self.current_player_sequence_active()
            if not sequence_active:
                queued_item = self.pop_next_playback_queue_item()
                if queued_item:
                    self.open_playback_queue_item_with_mode(queued_item, show_player=self.in_player_screen or not self.background_playback_enabled())
                    return
            next_item = self.relative_player_item(1)
            if next_item:
                self.open_relative_player_item(next_item)
                return
            if sequence_active:
                queued_item = self.pop_next_playback_queue_item()
                if queued_item:
                    self.open_playback_queue_item_with_mode(queued_item, show_player=self.in_player_screen or not self.background_playback_enabled())
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
        self.cancel_clip_preview()
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
        self.cancel_clip_preview()
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
            shlex_module = import_module("shlex")
            self.mpv_send(shlex_module.split(command), timeout=0.5)
        except Exception:
            pass

    def player_seek(self, seconds: float) -> None:
        if self.player_kind != "mpv" or not self.ipc_path:
            return
        self.cancel_clip_preview()
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
        self.cancel_clip_preview()
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

    def marked_clip_range(self) -> tuple[float, float] | None:
        if self.clip_start_marker is None or self.clip_end_marker is None:
            self.announce_player(self.t("clip_markers_missing"))
            return None
        start = float(self.clip_start_marker)
        end = float(self.clip_end_marker)
        if end - start < 0.25:
            self.announce_player(self.t("clip_marker_invalid"))
            return None
        return start, end

    def cancel_clip_preview(self) -> None:
        self.clip_preview_generation += 1

    def preview_marked_clip(self) -> None:
        clip_range = self.marked_clip_range()
        if clip_range is None:
            return
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            self.announce_player(self.t("no_player"))
            return
        self.clip_preview_generation += 1
        preview_generation = self.clip_preview_generation
        player_generation = self.player_generation
        start, end = clip_range
        threading.Thread(
            target=self.preview_marked_clip_worker,
            args=(player_generation, preview_generation, start, end),
            daemon=True,
        ).start()

    def clip_preview_is_current(self, player_generation: int, preview_generation: int) -> bool:
        return (
            player_generation == self.player_generation
            and preview_generation == self.clip_preview_generation
            and self.player_kind == "mpv"
            and self.mpv_process_alive()
        )

    def preview_marked_clip_worker(self, player_generation: int, preview_generation: int, start: float, end: float) -> None:
        try:
            if not self.clip_preview_is_current(player_generation, preview_generation):
                return
            self.mpv_send(["seek", float(start), "absolute+exact"], timeout=0.8)
            self.mpv_set_property("pause", False, timeout=0.8)
            wx.CallAfter(self.start_clip_preview_ui, player_generation, preview_generation, start, end)
            deadline = time.monotonic() + max(1.0, end - start + 2.0)
            while time.monotonic() < deadline:
                if not self.clip_preview_is_current(player_generation, preview_generation):
                    return
                try:
                    position = self.mpv_get_property("time-pos", timeout=0.25)
                except Exception:
                    position = None
                if position is not None and float(position) >= end - 0.03:
                    break
                time.sleep(0.05)
            if not self.clip_preview_is_current(player_generation, preview_generation):
                return
            try:
                self.mpv_send(["seek", float(end), "absolute+exact"], timeout=0.6)
            except Exception:
                pass
            self.mpv_set_property("pause", True, timeout=0.8)
            wx.CallAfter(self.finish_clip_preview_ui, player_generation, preview_generation)
        except Exception:
            if self.clip_preview_is_current(player_generation, preview_generation):
                wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

    def start_clip_preview_ui(self, player_generation: int, preview_generation: int, start: float, end: float) -> None:
        if not self.clip_preview_is_current(player_generation, preview_generation):
            return
        self.player_ended = False
        self.player_paused = False
        self.update_play_pause_buttons()
        self.announce_player(self.t("clip_preview_started", start=self.format_seconds(start), end=self.format_seconds(end)))

    def finish_clip_preview_ui(self, player_generation: int, preview_generation: int) -> None:
        if not self.clip_preview_is_current(player_generation, preview_generation):
            return
        self.player_paused = True
        self.player_ended = False
        self.update_play_pause_buttons()
        self.announce_player(self.t("clip_preview_finished"))

    def export_marked_clip(self, audio_only: bool = False) -> None:
        clip_range = self.marked_clip_range()
        if clip_range is None:
            return
        start, end = clip_range
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
        with self.volume_change_lock:
            maximum = float(self.current_player_volume_max())
            base = self.session_volume
            if base is None:
                base = self.default_volume_value()
            target = min(max(0.0, float(base) + float(delta)), maximum)
            self.session_volume = target
            self.volume_change_pending_target = target
            timer = self.volume_change_timer
            if timer is not None and timer.IsRunning():
                return
            self.volume_change_timer = wx.CallLater(45, self.apply_pending_volume_change_async)

    def apply_pending_volume_change_async(self) -> None:
        with self.volume_change_lock:
            target = self.volume_change_pending_target
            self.volume_change_pending_target = None
            self.volume_change_timer = None
        if target is None:
            return
        generation = self.player_generation
        threading.Thread(target=self.change_volume_worker, args=(target, generation), daemon=True).start()

    def change_volume_worker(self, target: float, generation: int | None = None) -> None:
        try:
            if generation is not None and generation != self.player_generation:
                return
            if self.player_kind != "mpv" or not self.mpv_process_alive():
                return
            maximum = float(self.current_player_volume_max())
            volume = min(max(0.0, float(target)), maximum)
            self.mpv_set_property("volume-max", maximum)
            if generation is not None and generation != self.player_generation:
                return
            self.mpv_set_property("volume", volume)
            if generation is None or generation == self.player_generation:
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
        generation = self.player_generation
        if self.volume_boost_enabled:
            threading.Thread(target=self.enable_volume_boost_worker, args=(generation,), daemon=True).start()
            self.announce_player(self.t("volume_boost_on"))
        else:
            threading.Thread(target=self.disable_volume_boost_worker, args=(generation,), daemon=True).start()

    def enable_volume_boost_worker(self, generation: int | None = None) -> None:
        try:
            if generation is not None and generation != self.player_generation:
                return
            if self.player_kind != "mpv" or not self.mpv_process_alive():
                return
            self.mpv_set_property("volume-max", BOOSTED_VOLUME_MAX)
        except Exception:
            pass
        wx.CallAfter(self.schedule_equalizer_apply, 40)

    def disable_volume_boost_worker(self, generation: int | None = None) -> None:
        try:
            if generation is not None and generation != self.player_generation:
                return
            if self.player_kind != "mpv" or not self.mpv_process_alive():
                return
            current = self.mpv_get_property("volume")
            if generation is not None and generation != self.player_generation:
                return
            if current is not None and float(current) > 100.0:
                self.mpv_set_property("volume", 100.0)
                self.session_volume = 100.0
            elif current is not None:
                self.session_volume = max(0.0, min(100.0, float(current)))
            if generation is not None and generation != self.player_generation:
                return
            self.mpv_set_property("volume-max", NORMAL_VOLUME_MAX)
        except Exception:
            pass
        wx.CallAfter(self.schedule_equalizer_apply, 40)
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

    def seek_seconds_value(self) -> float:
        return self.to_float(str(getattr(self.settings, "seek_seconds", 5.0)), 5.0, 0.1, 600.0)

    @staticmethod
    def format_seek_seconds_value(value: float) -> str:
        value = round(float(value), 2)
        if abs(value - round(value)) < 0.001:
            return str(int(round(value)))
        return f"{value:.2f}".rstrip("0").rstrip(".")

    @staticmethod
    def default_volume_max_for_boost(boost_enabled: bool) -> int:
        return BOOSTED_VOLUME_MAX if boost_enabled else NORMAL_VOLUME_MAX

    def default_volume_max_value(self) -> int:
        return self.default_volume_max_for_boost(bool(getattr(self.settings, "volume_boost_by_default", False)))

    def default_volume_value(self) -> int:
        return self.to_int(str(getattr(self.settings, "default_volume", 100)), 100, 0, self.default_volume_max_value())

    def on_volume_boost_by_default_settings_changed(self, event: wx.CommandEvent) -> None:
        enabled = bool(event.IsChecked())
        self.settings.volume_boost_by_default = enabled
        slider = self.controls.get("default_volume") if hasattr(self, "controls") else None
        if isinstance(slider, wx.Slider):
            maximum = self.default_volume_max_for_boost(enabled)
            value = min(max(0, int(slider.GetValue())), maximum)
            slider.SetRange(0, maximum)
            if slider.GetValue() != value:
                slider.SetValue(value)
            self.set_integer_slider_accessibility(slider, self.t("default_volume"), "percent")
        event.Skip()

    def normalized_pitch_mode(self) -> str:
        mode = str(getattr(self.settings, "pitch_mode", PITCH_MODE_MPV) or PITCH_MODE_MPV)
        return self.normalize_pitch_mode_value(mode)

    def normalized_speed_audio_mode(self) -> str:
        mode = str(getattr(self.settings, "speed_audio_mode", SPEED_AUDIO_MODE_RUBBERBAND) or SPEED_AUDIO_MODE_RUBBERBAND)
        return self.normalize_speed_audio_mode_value(mode)

    def normalized_direct_link_enter_action(self) -> str:
        action = str(getattr(self.settings, "direct_link_enter_action", DIRECT_LINK_ENTER_PLAY) or DIRECT_LINK_ENTER_PLAY)
        return self.normalize_direct_link_enter_action(action)

    def normalized_replaygain_mode(self, value: str | None = None) -> str:
        mode = str(value if value is not None else getattr(self.settings, "replaygain_mode", REPLAYGAIN_MODE_OFF) or REPLAYGAIN_MODE_OFF).strip().lower()
        aliases = {
            "off": REPLAYGAIN_MODE_OFF,
            "none": REPLAYGAIN_MODE_OFF,
            "disabled": REPLAYGAIN_MODE_OFF,
            "track": REPLAYGAIN_MODE_TRACK,
            "song": REPLAYGAIN_MODE_TRACK,
            "album": REPLAYGAIN_MODE_ALBUM,
        }
        return aliases.get(mode, mode if mode in REPLAYGAIN_MODE_OPTIONS else REPLAYGAIN_MODE_OFF)

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

    def replaygain_mode_labels(self) -> list[str]:
        return [
            self.t("replaygain_off"),
            self.t("replaygain_track"),
            self.t("replaygain_album"),
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

    @staticmethod
    def configure_equalizer_slider_steps(ctrl: wx.Slider) -> None:
        for setter_name, value in (("SetLineSize", 10), ("SetPageSize", 30)):
            setter = getattr(ctrl, setter_name, None)
            if setter:
                try:
                    setter(value)
                except Exception:
                    pass

    @staticmethod
    def bind_equalizer_slider_events(ctrl: wx.Slider, handler) -> None:
        event_names = ("EVT_SLIDER",)
        seen: set[int] = set()
        for event_name in event_names:
            binder = getattr(wx, event_name, None)
            if binder is None or id(binder) in seen:
                continue
            seen.add(id(binder))
            try:
                ctrl.Bind(binder, handler)
            except Exception:
                pass

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
            for preset_id in EQ_CUSTOM_PRESET_IDS:
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
        if preset_id in EQ_FACTORY_PRESET_VALUES:
            return self.factory_equalizer_gains_for_preset(preset_id)
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        return self.normalized_equalizer_gains(presets.get(preset_id) or {})

    def factory_equalizer_gains_for_preset(self, preset_id: str | None) -> dict[str, float]:
        preset_id = self.normalized_equalizer_preset(preset_id)
        if preset_id in EQ_FACTORY_PRESET_VALUES:
            return equalizer_gains_from_values(EQ_FACTORY_PRESET_VALUES[preset_id])
        return default_equalizer_gains()

    def equalizer_slider_band_id(self, ctrl: wx.Window | None) -> str:
        band_id = str(getattr(ctrl, "_apricot_eq_band_id", "") or "")
        return band_id if band_id in EQ_BAND_IDS else ""

    @staticmethod
    def equalizer_gain_from_slider_value(value: int | float) -> float:
        return round(max(-24.0, min(24.0, float(value) / 10.0)), 1)

    def visible_equalizer_base_gains(self) -> dict[str, float]:
        preset = self.normalized_equalizer_preset(
            getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        )
        return self.equalizer_gains_for_preset(preset)

    def update_visible_equalizer_draft_from_slider(self, ctrl: wx.Slider) -> str:
        band_id = self.equalizer_slider_band_id(ctrl)
        if not band_id:
            return ""
        draft = getattr(self, "visible_equalizer_draft_gains", None)
        if not isinstance(draft, dict) or not all(band in draft for band in EQ_BAND_IDS):
            draft = self.visible_equalizer_base_gains()
        draft = self.normalized_equalizer_gains(draft)
        draft[band_id] = self.equalizer_gain_from_slider_value(ctrl.GetValue())
        self.visible_equalizer_draft_gains = draft
        return band_id

    def visible_equalizer_gains_from_controls(self) -> dict[str, float]:
        gains: dict[str, float] = {}
        if not hasattr(self, "controls"):
            return gains
        for band_id, _band_label in EQ_BANDS:
            ctrl = self.controls.get(f"eq_{band_id}")
            if isinstance(ctrl, wx.Slider):
                gains[band_id] = round(float(ctrl.GetValue()) / 10.0, 1)
        return gains

    def visible_equalizer_gains(self) -> dict[str, float]:
        draft = getattr(self, "visible_equalizer_draft_gains", None)
        if isinstance(draft, dict) and all(band_id in draft for band_id in EQ_BAND_IDS):
            return self.normalized_equalizer_gains(draft)
        return self.visible_equalizer_gains_from_controls()

    def save_visible_equalizer_gains_to_preset(self, preset_id: str | None = None) -> None:
        preset_id = self.normalized_equalizer_preset(preset_id or getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        gains = self.visible_equalizer_gains()
        if not gains:
            return
        normalized_gains = self.normalized_equalizer_gains(gains)
        if self.is_custom_equalizer_preset(preset_id):
            presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
            presets[preset_id] = normalized_gains
            self.settings.equalizer_preset_gains = presets
        self.settings.global_equalizer_gains = normalized_gains

    def equalizer_default_profile_name(self) -> str:
        return f"Custom {len(self.equalizer_custom_ids()) + 1}"

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
        names[preset_id] = (name.strip()[:80] if name.strip() else self.equalizer_default_profile_name())
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        presets[preset_id] = self.normalized_equalizer_gains(gains or default_equalizer_gains())
        self.settings.equalizer_custom_names = names
        self.settings.equalizer_preset_gains = presets
        return preset_id

    def create_equalizer_profile_dialog(self, gains: dict[str, float] | None = None) -> str:
        with wx.TextEntryDialog(self, self.t("equalizer_profile_name"), self.t("add_equalizer_profile"), "") as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return ""
            name = dialog.GetValue().strip()
        preset_id = self.create_equalizer_profile(name, gains)
        self.settings.global_equalizer_preset = preset_id
        self.save_settings()
        self.announce_player(self.t("equalizer_profile_saved"))
        return preset_id

    def delete_equalizer_profile(self, preset_id: str | None, confirm: bool = True) -> str:
        preset_id = self.normalized_equalizer_preset(preset_id)
        if not self.is_custom_equalizer_preset(preset_id):
            return ""
        if confirm:
            answer = wx.MessageBox(self.t("equalizer_profile_delete_confirm"), self.t("equalizer"), wx.YES_NO | wx.ICON_QUESTION)
            if answer != wx.YES:
                return ""
        names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
        presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
        names.pop(preset_id, None)
        if preset_id in EQ_CUSTOM_PRESET_IDS:
            presets[preset_id] = default_equalizer_gains()
        else:
            presets.pop(preset_id, None)
        self.settings.equalizer_custom_names = names
        self.settings.equalizer_preset_gains = presets
        replacement = EQ_PRESET_FLAT
        self.settings.global_equalizer_preset = replacement
        self.settings.global_equalizer_gains = self.equalizer_gains_for_preset(replacement)
        self.save_settings()
        self.announce_player(self.t("equalizer_profile_deleted"))
        return replacement

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
        preset_id = self.create_equalizer_profile_dialog(self.visible_equalizer_gains() or default_equalizer_gains())
        if not preset_id:
            return
        self.settings.global_equalizer_preset = preset_id
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_preset")

    def delete_visible_equalizer_profile_from_settings(self) -> None:
        preset_id = self.normalized_equalizer_preset(getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT)))
        replacement = self.delete_equalizer_profile(preset_id)
        if not replacement:
            return
        self.visible_equalizer_preset = replacement
        if self.player_is_active() and self.session_equalizer_enabled is None:
            self.schedule_equalizer_apply(30)
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_preset")

    def on_global_equalizer_toggle(self, _event: wx.CommandEvent) -> None:
        ctrl = self.controls.get("global_equalizer") if hasattr(self, "controls") else None
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        if isinstance(ctrl, wx.CheckBox):
            self.settings.global_equalizer_enabled = ctrl.GetValue()
        if self.player_is_active():
            self.use_global_equalizer_for_live_preview()
            self.schedule_equalizer_apply(30)
        wx.CallAfter(self.render_settings_section_and_focus, "global_equalizer")

    def on_equalizer_clipping_protection_changed(self, _event: wx.CommandEvent) -> None:
        ctrl = self.controls.get("equalizer_clipping_protection") if hasattr(self, "controls") else None
        if isinstance(ctrl, wx.CheckBox):
            self.settings.equalizer_clipping_protection = bool(ctrl.GetValue())
        if self.player_is_active():
            self.schedule_equalizer_apply(30)

    def on_equalizer_settings_preset_changed(self, _event: wx.CommandEvent) -> None:
        previous = getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        self.save_visible_equalizer_gains_to_preset(previous)
        preset = self.selected_choice_value("equalizer_preset")
        self.settings.global_equalizer_preset = self.normalized_equalizer_preset(preset)
        if self.player_is_active():
            self.use_global_equalizer_for_live_preview()
            self.schedule_equalizer_apply(30)
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_preset")

    def on_equalizer_settings_name_changed(self, event: wx.FocusEvent) -> None:
        preset = self.normalized_equalizer_preset(getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT)))
        ctrl = self.controls.get("equalizer_preset_name") if hasattr(self, "controls") else None
        if self.is_custom_equalizer_preset(preset) and isinstance(ctrl, wx.TextCtrl):
            names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
            names[preset] = ctrl.GetValue().strip()[:80] or self.equalizer_custom_name(preset)
            self.settings.equalizer_custom_names = names
            self.save_visible_equalizer_gains_to_preset(preset)
            preset_ctrl = self.controls.get("equalizer_preset") if hasattr(self, "controls") else None
            if isinstance(preset_ctrl, wx.Choice):
                options = self.equalizer_preset_options()
                if preset in options:
                    preset_ctrl.SetString(options.index(preset), self.equalizer_preset_label(preset))
        event.Skip()

    def on_equalizer_settings_slider(self, event: wx.CommandEvent, label: str) -> None:
        if getattr(self, "equalizer_controls_loading", False):
            event.Skip()
            return
        ctrl = event.GetEventObject()
        if isinstance(ctrl, wx.Slider):
            if getattr(ctrl, "_apricot_eq_programmatic_update", False):
                event.Skip()
                return
            if not self.update_visible_equalizer_draft_from_slider(ctrl):
                return
            self.set_equalizer_slider_accessibility(ctrl, label)
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        if self.player_is_active():
            self.settings.global_equalizer_enabled = True
            preset = self.normalized_equalizer_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
            if self.is_custom_equalizer_preset(preset):
                self.use_global_equalizer_for_live_preview()
            else:
                self.use_visible_equalizer_for_live_preview()
            self.schedule_equalizer_apply()

    def on_equalizer_range_changed(self, _event: wx.CommandEvent) -> None:
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        next_range = self.to_int(self.selected_choice_value("equalizer_db_range"), 12, 6, 24)
        self.settings.equalizer_db_range = next_range
        draft = self.visible_equalizer_gains()
        self.visible_equalizer_draft_gains = {
            band_id: round(max(-float(next_range), min(float(next_range), float(draft.get(band_id, 0.0) or 0.0))), 1)
            for band_id, _band_label in EQ_BANDS
        }
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_db_range")

    def reset_visible_equalizer_controls(self) -> None:
        if not hasattr(self, "controls"):
            return
        preset = self.normalized_equalizer_preset(self.selected_choice_value("equalizer_preset") or getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        gains = self.factory_equalizer_gains_for_preset(preset)
        self.visible_equalizer_draft_gains = self.normalized_equalizer_gains(gains)
        if self.is_custom_equalizer_preset(preset):
            presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
            presets[preset] = gains
            self.settings.equalizer_preset_gains = presets
        self.settings.global_equalizer_gains = self.normalized_equalizer_gains(gains)
        for band_id, band_label in EQ_BANDS:
            ctrl = self.controls.get(f"eq_{band_id}")
            if isinstance(ctrl, wx.Slider):
                value = gains.get(band_id, 0.0)
                ctrl._apricot_eq_programmatic_update = True
                try:
                    ctrl.SetValue(int(round(value * 10)))
                    self.set_equalizer_slider_accessibility(ctrl, self.t("equalizer_band_gain", band=band_label))
                finally:
                    ctrl._apricot_eq_programmatic_update = False
        if self.player_is_active():
            if self.is_custom_equalizer_preset(preset):
                self.use_global_equalizer_for_live_preview()
            else:
                self.use_visible_equalizer_for_live_preview()
            self.schedule_equalizer_apply(30)
        self.announce_player(self.t("equalizer_saved"))

    def result_limit_labels(self, options: list[str]) -> list[str]:
        return [self.t("dynamic_results") if option == "0" else option for option in options]

    def stream_url_cache_labels(self, options: list[str]) -> list[str]:
        labels = []
        for option in options:
            try:
                minutes = int(option)
            except (TypeError, ValueError):
                minutes = 20
            if minutes <= 0:
                labels.append(self.t("stream_cache_permanent"))
            elif minutes < 60:
                labels.append(self.t("stream_cache_minutes_label", minutes=minutes))
            elif minutes % 1440 == 0:
                days = minutes // 1440
                labels.append(self.t("stream_cache_days_label", days=days))
            elif minutes % 60 == 0:
                hours = minutes // 60
                labels.append(self.t("stream_cache_hours_label", hours=hours))
            else:
                labels.append(str(minutes))
        return labels

    def normalized_stream_url_cache_minutes(self, value=None) -> int:
        raw = getattr(self.settings, "stream_url_cache_minutes", 20) if value is None else value
        try:
            minutes = int(raw)
        except (TypeError, ValueError):
            minutes = 20
        if minutes <= 0:
            return 0
        return min(10080, max(5, minutes))

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
        try:
            winsound_module = import_module("winsound")
        except ImportError:
            return
        sound_path = self.bundled_path("assets", DEFAULT_REACHED_SOUND)
        try:
            if sound_path.exists():
                winsound_module.PlaySound(str(sound_path), winsound_module.SND_FILENAME | winsound_module.SND_ASYNC)
            else:
                winsound_module.MessageBeep(winsound_module.MB_OK)
        except Exception:
            pass

    def stop_player(self, silent: bool = False, reset_session: bool = True, preserve_panel: bool = False) -> None:
        self.play_request_generation += 1
        self.playback_start_pending = False
        self.cancel_clip_preview()
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
        self.equalizer_filter_ref = EQ_FILTER_REF
        self.current_stream_url = ""
        self.current_stream_headers = {}
        self.current_audio_device = ""
        if reset_session:
            self.player_session_open = False
            if self.player_return_screen == "folder":
                self.clear_auto_folder_playback_queue()
            self.player_fullscreen_session = False
            self.player_fullscreen_results_override = False
            self.manual_background_playback_active = False
            self.session_volume = None
            self.cancel_pending_volume_change()
            self.session_autoplay_next = False
            self.session_equalizer_enabled = None
            self.session_equalizer_gains = {}
            self.session_equalizer_before_bass_boost = None
            self.volume_boost_enabled = False
            self.shuffle_current = False
            self.player_sequence_results = []
        if self.player_panel is not None and not preserve_panel:
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
        return any(self.window_is_or_descendant(focus, control) for control in getattr(self, "background_player_controls", []))

    def focus_in_results_control(self, focus: wx.Window | None) -> bool:
        return self.window_is_or_descendant(focus, getattr(self, "results_list", None))

    def focus_in_player_controls(self, focus: wx.Window | None) -> bool:
        if not focus:
            return False
        if self.window_is_or_descendant(focus, getattr(self, "player_panel", None)):
            return True
        controls = list(getattr(self, "player_action_controls", [])) + list(getattr(self, "player_navigation_controls", []))
        return any(focus is control for control in controls)

    def player_shortcuts_allowed(self, focus: wx.Window | None = None) -> bool:
        if self.focus_in_results_control(focus):
            return True
        if self.in_player_screen and not self.focus_accepts_text(focus):
            return self.focus_in_player_controls(focus)
        return self.focus_in_player_controls(focus) or self.focus_in_background_player_controls(focus)

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

    @staticmethod
    def results_list_native_navigation_key(event: wx.KeyEvent) -> bool:
        if event.ControlDown() or event.AltDown():
            return False
        return event.GetKeyCode() in {
            wx.WXK_UP,
            wx.WXK_DOWN,
            wx.WXK_HOME,
            wx.WXK_END,
            wx.WXK_PAGEUP,
            wx.WXK_PAGEDOWN,
        }

    @staticmethod
    def results_list_owns_key(event: wx.KeyEvent) -> bool:
        if event.ControlDown() or event.AltDown():
            return False
        return True

    def handle_player_shortcut_event(self, event: wx.KeyEvent, focus: wx.Window | None, details_has_focus: bool = False) -> bool:
        if not (self.player_control_mode and self.player_shortcuts_allowed(focus)):
            return False
        if self.focus_in_results_control(focus):
            if self.shortcut_matches(event, "player_previous"):
                self.play_relative_item(-1, preserve_focus=True)
                return True
            if self.shortcut_matches(event, "player_next"):
                self.play_relative_item(1, preserve_focus=True)
                return True
            if self.results_list_owns_key(event):
                event.Skip()
                wx.CallAfter(self.maybe_extend_results)
                return True
            return True
        if self.context_menu_shortcut_matches(event):
            self.open_player_context_menu()
            return True
        player_checkboxes = {
            getattr(self, "fullscreen_checkbox", None),
            getattr(self, "repeat_checkbox", None),
            getattr(self, "bass_boost_checkbox", None),
        }
        player_checkboxes.discard(None)
        if focus is getattr(self, "fullscreen_checkbox", None) and event.GetKeyCode() in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}:
            self.request_player_fullscreen_checkbox_toggle()
            return True
        if focus in player_checkboxes and self.shortcut_matches(event, "player_play_pause"):
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
        if self.shortcut_matches(event, "player_copy_timestamp_link"):
            self.copy_current_player_timestamp_url()
            return True
        if self.shortcut_matches(event, "open_channel"):
            self.open_item_channel(self.current_video_item or self.current_video_info)
            return True
        if self.shortcut_matches(event, "player_equalizer"):
            self.show_player_equalizer()
            return True
        if self.shortcut_matches(event, "player_chapters"):
            self.show_chapters()
            return True
        if self.shortcut_matches(event, "player_lyrics"):
            self.show_lyrics()
            return True
        if self.shortcut_matches(event, "player_comments"):
            self.show_comments()
            return True
        if self.shortcut_matches(event, "player_previous_chapter"):
            self.seek_relative_chapter(-1)
            return True
        if self.shortcut_matches(event, "player_next_chapter"):
            self.seek_relative_chapter(1)
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
        if self.shortcut_matches(event, "player_preview_marked_clip"):
            self.preview_marked_clip()
            return True
        if self.shortcut_matches(event, "player_previous"):
            self.play_relative_item(-1, preserve_focus=True)
            return True
        if self.shortcut_matches(event, "player_next"):
            self.play_relative_item(1, preserve_focus=True)
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
            self.player_seek(-self.seek_seconds_value())
            return True
        if self.shortcut_matches(event, "player_seek_forward"):
            self.player_seek(self.seek_seconds_value())
            return True
        if self.shortcut_matches(event, "player_volume_up"):
            self.change_volume_async(self.settings.volume_step)
            return True
        if self.shortcut_matches(event, "player_volume_down"):
            self.change_volume_async(-self.settings.volume_step)
            return True
        return False

    def handle_active_player_global_shortcut_event(self, event: wx.KeyEvent, focus: wx.Window | None) -> bool:
        if not (self.player_control_mode and self.player_is_active()):
            return False
        if self.player_shortcuts_allowed(focus) or self.focus_accepts_text(focus):
            return False
        if self.shortcut_matches(event, "player_previous"):
            self.play_relative_item(-1, preserve_focus=True)
            return True
        if self.shortcut_matches(event, "player_next"):
            self.play_relative_item(1, preserve_focus=True)
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
            if self.handle_active_player_global_shortcut_event(event, focus):
                return
            event.Skip()
            return
        if self.handle_global_navigation_shortcut(event, focus):
            return
        if self.handle_active_player_global_shortcut_event(event, focus):
            return
        results_focus = self.focus_in_results_control(focus)
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
        if results_focus and self.shortcut_matches(event, "queue_audio"):
            self.toggle_download_queue()
            return
        if results_focus and self.result_details_key(event):
            self.announce_selected_result_details()
            return
        if self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
            return
        if self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
            return
        if self.shortcut_matches(event, "open_selected") and results_focus:
            self.play_selected()
            return
        if results_focus and self.shortcut_matches(event, "copy_link"):
            self.copy_selected_url()
            return
        if results_focus and self.shortcut_matches(event, "add_favorite"):
            self.add_selected_favorite()
            return
        if results_focus and self.shortcut_matches(event, "remove_favorite"):
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
        if self.player_details_shortcut_matches(event) and (self.focus_in_player_controls(focus) or self.focus_in_background_player_controls(focus)):
            self.show_video_details()
            return
        if self.in_player_screen and self.handle_player_shortcut_event(event, focus, details_has_focus):
            return
        if self.in_player_screen and results_focus:
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
        is_local_media = self.item_is_local_media(item)
        if not is_local_media:
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
            (self.menu_label_with_shortcut("copy_path" if is_local_media else "copy_link", "player_copy_link"), self.copy_current_player_url),
            (self.t("output_devices"), self.show_output_devices),
            (self.t("equalizer"), self.show_player_equalizer),
            (self.menu_label_with_shortcut("chapters", "player_chapters"), self.show_chapters),
            (self.menu_label_with_shortcut("lyrics", "player_lyrics"), self.show_lyrics),
            (self.menu_label_with_shortcut("comments", "player_comments"), self.show_comments),
            (self.t("close_player"), self.close_current_player),
        ])
        if not is_local_media:
            actions.insert(-6, (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda: self.copy_direct_stream_url(dict(item))))
        if self.youtube_url_at_timestamp(item, 0):
            actions.insert(-6, (self.menu_label_with_shortcut("copy_timestamp_link", "player_copy_timestamp_link"), self.copy_current_player_timestamp_url))
        if self.item_has_openable_youtube_channel(item):
            actions.insert(6, (self.menu_label_with_shortcut("open_channel", "open_channel"), lambda: self.open_item_channel(dict(item))))
        if not is_local_media:
            actions.insert(-5, (self.t("open_browser"), lambda: import_module("webbrowser").open(str(item.get("webpage_url") or item.get("url") or ""))))
        for label, handler in actions:
            menu_item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), menu_item)
        if self.playlist_item_is_supported(item):
            self.append_add_to_playlist_menu(menu, prefer_active=True)
        self.PopupMenu(menu)
        menu.Destroy()

    def append_collection_download_submenu(self, menu: wx.Menu, item: dict) -> None:
        kind = str(item.get("kind") or "playlist")
        submenu = wx.Menu()
        audio_item = submenu.Append(wx.ID_ANY, self.menu_label_with_shortcut("download_audio", "download_audio"))
        video_item = submenu.Append(wx.ID_ANY, self.menu_label_with_shortcut("download_video", "download_video"))
        self.Bind(wx.EVT_MENU, lambda _evt, selected=dict(item): self.download_collection(selected, audio_only=True), audio_item)
        self.Bind(wx.EVT_MENU, lambda _evt, selected=dict(item): self.download_collection(selected, audio_only=False), video_item)
        menu.AppendSubMenu(submenu, self.t("download_channel" if kind == "channel" else "download_playlist"))

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
                    (self.t("channel_live_streams"), lambda selected=dict(item): self.open_channel_tab(selected, "streams")),
                    (None, None),
                    (self.menu_label_with_shortcut("add_favorite", "add_favorite"), self.add_selected_favorite),
                    (self.menu_label_with_shortcut("remove_favorite", "remove_favorite"), self.remove_selected_favorite_shortcut),
                    (self.t("open_browser"), self.open_selected_in_browser),
                    (self.menu_label_with_shortcut("copy_url", "copy_link"), self.copy_selected_url),
                ]
            else:
                actions = [
                    (self.t("play_playlist"), lambda selected=dict(item): self.play_playlist_from_result(selected, shuffle=False)),
                    (self.t("shuffle_playlist"), lambda selected=dict(item): self.play_playlist_from_result(selected, shuffle=True)),
                    (self.t("open_playlist_videos"), self.play_selected),
                    (None, None),
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
        context_item = dict(item or {})
        for label, handler in actions:
            if label is None:
                self.append_collection_download_submenu(menu, context_item)
                continue
            menu_item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), menu_item)
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
        self.show_download_progress_dialog(task_id, item.get("title") or self.t("channel" if kind == "channel" else "playlist"))
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
            "type": self.item_type_label(item),
            "live_status": item.get("live_status", ""),
            "is_live": bool(item.get("is_live", False)),
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
            if self.favorites_screen_active and not self.favorites:
                wx.CallAfter(self.show_favorites)
            else:
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
                    if self.favorites:
                        self.refresh_favorites()
                    else:
                        wx.CallAfter(self.show_favorites)
                self.announce_player(self.t("favorite_removed"))
                return
        self.announce_player(self.t("not_in_favorites"))

    def open_selected_in_browser(self) -> None:
        item = self.active_item()
        if item:
            import_module("webbrowser").open(str(item.get("webpage_url") or item.get("url") or ""))

    def copy_selected_url(self) -> None:
        item = self.active_item()
        if item:
            self.copy_url_to_clipboard(str(item.get("url") or ""))

    def copy_item_url(self, item: dict | None) -> None:
        if item:
            self.copy_url_to_clipboard(str(item.get("url") or ""))

    def copy_direct_stream_url(self, item: dict | None = None) -> None:
        item = item or self.active_item()
        if self.in_player_screen and self.item_is_local_media(item or self.current_player_item()):
            self.announce_player(self.t("direct_media_link_unavailable_local"))
            return
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
        if len(items) > 5 or any(item.get("kind") in {"playlist", "channel"} for item in items):
            self.show_download_progress_dialog(task_id, batch_title)
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
                import_module("webbrowser").open("https://www.youtube.com/")
            self.announce_player(self.t("youtube_profile_opened"))
        except Exception as exc:
            self.message(self.t("youtube_profile_open_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def open_youtube_api_key_page_from_settings(self) -> None:
        self.apply_settings_from_visible_controls()
        try:
            import_module("webbrowser").open(YOUTUBE_API_CREDENTIALS_URL)
            self.announce_player(self.t("youtube_api_key_page_opened"))
        except Exception as exc:
            self.message(self.t("youtube_api_key_page_open_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

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
            self.settings.seek_seconds = self.to_float(self.selected_choice_value("seek_seconds"), 5.0, 0.1, 600.0)
        if "volume_step" in c:
            self.settings.volume_step = self.to_int(c["volume_step"].GetStringSelection(), 5, 1)
        boost_by_default = bool(c["volume_boost_by_default"].GetValue()) if "volume_boost_by_default" in c else bool(getattr(self.settings, "volume_boost_by_default", False))
        if "default_volume" in c:
            self.settings.default_volume = self.to_int(
                str(c["default_volume"].GetValue()),
                100,
                0,
                self.default_volume_max_for_boost(boost_by_default),
            )
        if "volume_boost_by_default" in c:
            self.settings.volume_boost_by_default = boost_by_default
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
        if "conversion_complete_popup" in c:
            self.settings.popup_when_conversion_complete = c["conversion_complete_popup"].GetValue()
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
        if "equalizer_clipping_protection" in c:
            self.settings.equalizer_clipping_protection = bool(c["equalizer_clipping_protection"].GetValue())
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
        eq_gains: dict[str, float] = self.visible_equalizer_gains() if any(f"eq_{band_id}" in c for band_id, _band_label in EQ_BANDS) else {}
        if eq_gains:
            eq_gains = self.normalized_equalizer_gains(eq_gains)
            if self.is_custom_equalizer_preset(selected_equalizer_preset):
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
        if "enable_stream_url_cache" in c:
            self.settings.enable_stream_url_cache = c["enable_stream_url_cache"].GetValue()
        if "stream_url_cache_minutes" in c:
            self.settings.stream_url_cache_minutes = self.normalized_stream_url_cache_minutes(self.selected_choice_value("stream_url_cache_minutes"))
        if "prefetch_next_stream_url" in c:
            self.settings.prefetch_next_stream_url = c["prefetch_next_stream_url"].GetValue()
        if "gapless_playback" in c:
            self.settings.gapless_playback = c["gapless_playback"].GetValue()
        if "replaygain_mode" in c:
            self.settings.replaygain_mode = self.normalized_replaygain_mode(self.selected_choice_value("replaygain_mode"))
        if "enable_online_lyrics" in c:
            self.settings.enable_online_lyrics = c["enable_online_lyrics"].GetValue()
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

    def start_ytdlp_update_check(self, manual: bool = False) -> None:
        threading.Thread(target=self.update_ytdlp_worker, args=(manual,), daemon=True).start()

    def manual_ytdlp_update_check(self) -> None:
        self.apply_settings_from_visible_controls()
        self.set_status(self.t("checking_updates"))
        self.announce_player(self.t("checking_updates"))
        self.start_ytdlp_update_check(manual=True)

    def update_ytdlp_worker(self, manual: bool = False) -> None:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            self.ui_queue.put(("announce", self.t("missing_ytdlp")))
            return
        try:
            updated = self.update_ytdlp_component_package(ytdlp)
            if updated:
                self.ui_queue.put(("announce", self.t("components_updated")))
            elif manual:
                self.ui_queue.put(("announce", self.t("updates_ok")))
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
            zipfile_module = import_module("zipfile")
            with zipfile_module.ZipFile(wheel_path) as archive:
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
            zipfile_module = import_module("zipfile")
            if not zipfile_module.is_zipfile(path):
                raise RuntimeError("downloaded portable update is not a valid zip file")
            with zipfile_module.ZipFile(path) as archive:
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
        except Exception as exc:
            if exc.__class__.__name__ != "HTTPError" or getattr(exc, "code", None) != 404:
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
        try:
            certifi_module = import_module("certifi")
        except ImportError:
            certifi_module = None
        if certifi_module is not None:
            _SSL_CONTEXT = ssl.create_default_context(cafile=certifi_module.where())
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
                elif kind == "conversion_progress" and isinstance(payload, dict):
                    self.update_conversion_progress_dialog(payload)
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
        for action in list(repaired):
            if self.canonical_shortcut(repaired.get(action, "")) == "f5":
                repaired[action] = DEFAULT_KEYBOARD_SHORTCUTS.get(action, "")
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
                merged["replaygain_mode"] = self.normalized_replaygain_mode(str(merged.get("replaygain_mode") or ""))
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
                merged["seek_seconds"] = self.to_float(str(merged.get("seek_seconds") or "5"), 5.0, 0.1, 600.0)
                merged["default_volume"] = self.to_int(
                    str(merged.get("default_volume") or "100"),
                    100,
                    0,
                    self.default_volume_max_for_boost(bool(merged.get("volume_boost_by_default", False))),
                )
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
                merged["stream_url_cache_minutes"] = self.normalized_stream_url_cache_minutes(merged.get("stream_url_cache_minutes"))
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

    def ensure_rss_feeds_loaded(self) -> None:
        if getattr(self, "rss_feeds_loaded", False):
            return
        self.rss_feeds = self.load_rss_feeds()
        self.rss_feeds_loaded = True

    def save_rss_feeds(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        self.rss_feeds_loaded = True
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
            "type": self.item_type_label(item),
            "live_status": item.get("live_status", ""),
            "is_live": bool(item.get("is_live", False)),
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


def startup_close_to_tray_enabled() -> bool:
    try:
        data = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
        return bool(data.get("close_to_tray", False))
    except Exception:
        return False


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


def activate_existing_instance_window(title_hint: str = "") -> bool:
    if os.name != "nt":
        return False
    try:
        user32 = ctypes.windll.user32
        user32.GetWindowTextLengthW.argtypes = [ctypes.c_void_p]
        user32.GetWindowTextLengthW.restype = ctypes.c_int
        user32.GetWindowTextW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p, ctypes.c_int]
        user32.GetWindowTextW.restype = ctypes.c_int
        user32.SetWindowTextW.argtypes = [ctypes.c_void_p, ctypes.c_wchar_p]
        user32.SetWindowTextW.restype = ctypes.c_int
        target_hwnd = ctypes.c_void_p()

        enum_proc_type = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
        user32.EnumWindows.argtypes = [enum_proc_type, ctypes.c_void_p]
        user32.EnumWindows.restype = ctypes.c_int

        def enum_proc(hwnd, _lparam):
            nonlocal target_hwnd
            title_length = user32.GetWindowTextLengthW(hwnd)
            if title_length <= 0:
                return True
            title_buffer = ctypes.create_unicode_buffer(title_length + 1)
            user32.GetWindowTextW(hwnd, title_buffer, title_length + 1)
            if APP_NAME in str(title_buffer.value):
                target_hwnd = ctypes.c_void_p(hwnd)
                return False
            return True

        callback = enum_proc_type(enum_proc)
        user32.EnumWindows(callback, None)
        hwnd = target_hwnd.value
        if not hwnd:
            return False
        title = re.sub(r"\s+", " ", str(title_hint or "").strip())
        if title:
            try:
                user32.SetWindowTextW(ctypes.c_void_p(hwnd), f"{title} - {WINDOW_TITLE}")
            except Exception:
                pass
        user32.ShowWindow(ctypes.c_void_p(hwnd), 9)
        user32.ShowWindow(ctypes.c_void_p(hwnd), 5)
        user32.BringWindowToTop(ctypes.c_void_p(hwnd))
        user32.SetForegroundWindow(ctypes.c_void_p(hwnd))
        user32.SetActiveWindow(ctypes.c_void_p(hwnd))
        return True
    except Exception:
        return False


def create_startup_mutex(instance_name: str) -> tuple[object | None, bool]:
    if os.name != "nt":
        return None, False
    try:
        safe_name = re.sub(r"[^A-Za-z0-9_.-]+", "_", instance_name).strip("_") or APP_NAME
        mutex_name = f"Local\\{safe_name}"
        kernel32 = ctypes.windll.kernel32
        kernel32.CreateMutexW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_wchar_p]
        kernel32.CreateMutexW.restype = ctypes.c_void_p
        kernel32.GetLastError.restype = ctypes.c_ulong
        handle = kernel32.CreateMutexW(None, 1, mutex_name)
        already_running = bool(handle and kernel32.GetLastError() == WINDOWS_ERROR_ALREADY_EXISTS)
        return handle, already_running
    except Exception:
        return None, False


def close_startup_mutex(handle) -> None:
    if os.name != "nt" or not handle:
        return
    try:
        kernel32 = ctypes.windll.kernel32
        kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
        kernel32.CloseHandle.restype = ctypes.c_int
        kernel32.CloseHandle(handle)
    except Exception:
        pass


def handle_already_running_startup(startup_media_path: str, tray_start: bool) -> bool:
    if suppress_already_open_for_update():
        return False
    if startup_media_path:
        request_existing_instance_activation("open_file", path=startup_media_path)
        activate_existing_instance_window(Path(startup_media_path).stem)
    elif tray_start:
        return False
    elif startup_close_to_tray_enabled():
        request_existing_instance_activation("show")
        activate_existing_instance_window()
    else:
        wx.MessageBox(startup_text("already_open"), APP_NAME, wx.OK | wx.ICON_INFORMATION)
    return False


class App(wx.App):
    def OnInit(self) -> bool:
        update_relaunch = update_relaunch_requested()
        if update_relaunch:
            mark_update_relaunch_window()
        startup_media_path = startup_media_path_argument()
        tray_start = start_in_tray_requested() and not startup_media_path
        instance_name = f"{APP_NAME}-{wx.GetUserId() or 'user'}"
        self.instance_mutex_handle, mutex_already_running = create_startup_mutex(instance_name)
        self.instance_checker = None
        checker_already_running = False
        if not self.instance_mutex_handle:
            self.instance_checker = wx.SingleInstanceChecker(instance_name)
            checker_already_running = self.instance_checker.IsAnotherRunning()
        if mutex_already_running or checker_already_running:
            result = handle_already_running_startup(startup_media_path, tray_start)
            close_startup_mutex(getattr(self, "instance_mutex_handle", None))
            self.instance_mutex_handle = None
            return result
        frame = MainFrame(start_hidden_in_tray=tray_start)
        self.SetTopWindow(frame)
        if tray_start:
            frame.Hide()
        else:
            frame.Show()
            if startup_media_path:
                pass
            elif update_relaunch:
                frame.activate_after_update_relaunch()
            else:
                frame.activate_window_later()
        if startup_media_path:
            wx.CallAfter(frame.open_local_media_file, startup_media_path, True)
        return True

    def OnExit(self) -> int:
        close_startup_mutex(getattr(self, "instance_mutex_handle", None))
        return 0


def main() -> int:
    app = App(False)
    app.MainLoop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
