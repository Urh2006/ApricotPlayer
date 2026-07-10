from __future__ import annotations
import json
import os
import queue
import random
import re
import secrets
import stat as stat_module
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
from defusedxml.common import DefusedXmlException
from defusedxml.ElementTree import fromstring as safe_xml_fromstring
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

_SSL_CONTEXT = None

# Pre-compiled regex patterns
_RE_VERSION          = re.compile(r"^v?(\d+)\.(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:-([A-Za-z]+)(?:[.-]?(\d+))?)?$")
_RE_ISO8601_DURATION = re.compile(r"P(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?)?$")


class UtilsMixin:

    @staticmethod
    def validate_zip_member_path(member_name: str) -> None:
        normalized = str(member_name or "").replace("\\", "/")
        if not normalized or "\x00" in normalized or normalized.startswith("/") or re.match(r"^[A-Za-z]:", normalized):
            raise RuntimeError("zip package contains an unsafe absolute path")
        parts = [part for part in normalized.split("/") if part]
        if not parts or any(part in {".", ".."} for part in parts):
            raise RuntimeError("zip package contains an unsafe relative path")
        for part in parts:
            if ":" in part or part.rstrip(" .") != part:
                raise RuntimeError("zip package contains an unsafe Windows path")
            if part.split(".", 1)[0].casefold() in WINDOWS_RESERVED_PATH_STEMS:
                raise RuntimeError("zip package contains a reserved Windows path")


    @classmethod
    def validate_zip_archive(
        cls,
        archive: zipfile.ZipFile,
        *,
        max_entries: int = UPDATE_ZIP_MAX_ENTRIES,
        max_uncompressed_bytes: int = UPDATE_ZIP_MAX_UNCOMPRESSED_BYTES,
        max_member_bytes: int = UPDATE_ZIP_MAX_MEMBER_BYTES,
        max_compression_ratio: int = UPDATE_ZIP_MAX_COMPRESSION_RATIO,
    ) -> None:
        members = archive.infolist()
        if len(members) > max_entries:
            raise RuntimeError("zip package contains too many entries")
        total_size = 0
        normalized_names: set[str] = set()
        for member in members:
            cls.validate_zip_member_path(member.filename)
            normalized = "/".join(part for part in member.filename.replace("\\", "/").split("/") if part).casefold()
            if normalized in normalized_names:
                raise RuntimeError("zip package contains duplicate paths")
            normalized_names.add(normalized)
            if member.flag_bits & 0x1:
                raise RuntimeError("zip package contains an encrypted entry")
            unix_mode = (member.external_attr >> 16) & 0xFFFF
            file_type = stat_module.S_IFMT(unix_mode)
            if file_type not in {0, stat_module.S_IFREG, stat_module.S_IFDIR}:
                raise RuntimeError("zip package contains a link or special file")
            if member.file_size < 0 or member.file_size > max_member_bytes:
                raise RuntimeError("zip package member is too large")
            total_size += member.file_size
            if total_size > max_uncompressed_bytes:
                raise RuntimeError("zip package expands beyond the safe size limit")
            if member.file_size > 1024 * 1024:
                if member.compress_size <= 0 or member.file_size / member.compress_size > max_compression_ratio:
                    raise RuntimeError("zip package has an unsafe compression ratio")


    @classmethod
    def safe_extract_zip(
        cls,
        archive: zipfile.ZipFile,
        target_dir: Path,
        **validation_limits: int,
    ) -> None:
        cls.validate_zip_archive(archive, **validation_limits)
        target_root = target_dir.resolve()
        for member in archive.infolist():
            destination = (target_root / member.filename).resolve()
            try:
                destination.relative_to(target_root)
            except ValueError:
                raise RuntimeError("zip package member would extract outside the target directory") from None
        archive.extractall(target_root)


    @staticmethod
    def validate_remote_http_url(value: str, label: str = "URL") -> str:
        text = str(value or "").strip()
        parsed = urlparse(text)
        if parsed.scheme.lower() not in {"http", "https"} or not parsed.hostname:
            raise RuntimeError(f"{label} must use HTTP or HTTPS")
        return text

    @staticmethod
    def validate_trusted_https_url(value: str, allowed_host_roots: set[str], label: str = "URL") -> str:
        text = str(value or "").strip()
        parsed = urlparse(text)
        host = (parsed.hostname or "").lower().rstrip(".")
        roots = {str(root).lower().lstrip(".").rstrip(".") for root in allowed_host_roots}
        if parsed.scheme.lower() != "https" or not host or not any(host == root or host.endswith("." + root) for root in roots):
            raise RuntimeError(f"{label} redirected to an untrusted address")
        return text

    @staticmethod
    def validate_loopback_http_url(value: str, expected_port: int, label: str = "URL") -> str:
        text = str(value or "").strip()
        parsed = urlparse(text)
        try:
            port = parsed.port
        except ValueError:
            port = None
        if parsed.scheme.lower() != "http" or (parsed.hostname or "").lower() not in {"127.0.0.1", "localhost", "::1"} or port != int(expected_port):
            raise RuntimeError(f"{label} left the expected local address")
        return text

    @classmethod
    def open_http_url_in_browser(cls, value: str) -> bool:
        url = cls.validate_remote_http_url(value)
        return bool(import_module("webbrowser").open(url))


    @staticmethod
    def read_response_limited(response, maximum_bytes: int, label: str = "response") -> bytes:
        maximum = max(1, int(maximum_bytes))
        content_length = str(response.headers.get("Content-Length") or "").strip()
        if content_length.isdigit() and int(content_length) > maximum:
            raise RuntimeError(f"{label} is larger than the allowed limit")
        data = response.read(maximum + 1)
        if len(data) > maximum:
            raise RuntimeError(f"{label} is larger than the allowed limit")
        return data


    @staticmethod
    def parse_xml_bytes_safely(data: bytes, label: str = "XML") -> ET.Element:
        raw = bytes(data or b"")
        # Removing NULs also exposes declarations encoded as UTF-16/UTF-32.
        inspection = raw.replace(b"\x00", b"")
        if re.search(br"<!\s*(?:doctype|entity)\b", inspection, flags=re.IGNORECASE):
            raise RuntimeError(f"{label} contains a disallowed DTD or entity declaration")
        try:
            return safe_xml_fromstring(raw, forbid_dtd=True, forbid_entities=True, forbid_external=True)
        except (ET.ParseError, DefusedXmlException) as exc:
            raise RuntimeError(f"{label} is not valid XML: {exc}") from exc


    @staticmethod
    def windows_system_executable(filename: str) -> str:
        name = Path(str(filename or "")).name
        if not name or name != str(filename or ""):
            return ""
        if os.name != "nt":
            return shutil.which(name) or ""
        system_root = Path(str(os.environ.get("SystemRoot") or r"C:\Windows"))
        candidate = system_root / "System32" / name
        return str(candidate) if candidate.is_file() else ""


    @classmethod
    def trusted_powershell_executable(cls) -> str:
        if os.name != "nt":
            return shutil.which("pwsh") or shutil.which("powershell") or ""
        candidates = [
            Path(str(os.environ.get("SystemRoot") or r"C:\Windows")) / "System32" / "WindowsPowerShell" / "v1.0" / "powershell.exe",
            Path(str(os.environ.get("ProgramFiles") or r"C:\Program Files")) / "PowerShell" / "7" / "pwsh.exe",
        ]
        for candidate in candidates:
            if candidate.is_file():
                return str(candidate)
        return ""



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
    def version_is_prerelease(value: str) -> bool:
        return bool(re.search(r"-(alpha|beta|rc)(?:[.-]?\d+)?$", str(value or "").strip().lower()))


    @classmethod
    def current_build_is_prerelease(cls) -> bool:
        return cls.version_is_prerelease(APP_VERSION)


    @classmethod
    def default_update_channel(cls) -> str:
        return "beta" if cls.current_build_is_prerelease() else "stable"


    @classmethod
    def normalized_update_channel_value(cls, value: str | None = None) -> str:
        channel = str(value or "").strip().lower()
        if channel in {"stable", "beta"}:
            return channel
        return cls.default_update_channel()



    @staticmethod
    def parse_version(value: str) -> tuple[int, int, int, int, int, int]:
        match = _RE_VERSION.match(value.strip())
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
        match = _RE_ISO8601_DURATION.match(str(value or ""))
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
        token = secrets.token_hex(8)
        if os.name == "nt":
            return rf"\\.\pipe\apricotplayer-{os.getpid()}-{token}"
        user_id = getattr(os, "getuid", lambda: 0)()
        return str(Path(tempfile.gettempdir()) / f"apricotplayer-{user_id}-{os.getpid()}-{token}.sock")



