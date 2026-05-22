from __future__ import annotations
_SSL_CONTEXT = None
_URLLIB_REQUEST_MODULE = None
_PARSEDATE_TO_DATETIME = None
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
import shutil
import tempfile
import urllib.request
import urllib.parse
from urllib.request import Request
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
    pass
try:
    import ctypes
except ImportError:
    pass

from apricot.constants import *
from apricot.locales import TEXT

class UtilsMixin:

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



    @staticmethod
    def powershell_literal(value: str) -> str:
        return "'" + value.replace("'", "''") + "'"



    @staticmethod
    def current_executable_path() -> Path:
        try:
            return Path(sys.executable if getattr(sys, "frozen", False) else __file__).resolve()
        except Exception:
            return Path(sys.executable if getattr(sys, "frozen", False) else __file__)



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



    @staticmethod
    def bundled_path(*parts: str) -> Path:
        base = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent))
        return base.joinpath(*parts)



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
    def format_ago(timestamp: int) -> str:
        diff = max(0, int(time.time()) - int(timestamp))
        for name, size in (("year", 31536000), ("month", 2592000), ("day", 86400), ("hour", 3600), ("minute", 60)):
            if diff >= size:
                amount = diff // size
                return f"{amount} {name}{'' if amount == 1 else 's'} ago"
        return "just now"


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
            return f"uploaded {UtilsMixin.format_ago(int(timestamp))}"
        return ""



    @staticmethod
    def make_ipc_path() -> str:
        return rf"\\.\pipe\urhasaurus-youtube-{os.getpid()}" if os.name == "nt" else f"/tmp/urhasaurus-youtube-{os.getpid()}.sock"



