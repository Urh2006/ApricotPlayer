from apricot.constants import *
import wx
import os
from pathlib import Path
from apricot.ui.misc import MiscUI

class CookiesUI:
    @staticmethod
    def cookie_source_signature(path: Path) -> str:
        digest = hashlib.sha256()
        with path.open("rb") as source:
            while chunk := source.read(1024 * 1024):
                digest.update(chunk)
        return digest.hexdigest()

    @staticmethod
    def paths_match(first: str | Path, second: str | Path) -> bool:
        def expanded(value: str | Path) -> Path:
            return Path(os.path.expandvars(str(value).strip('"'))).expanduser()

        try:
            return expanded(first).resolve() == expanded(second).resolve()
        except OSError:
            return os.path.normcase(os.path.abspath(str(expanded(first)))) == os.path.normcase(os.path.abspath(str(expanded(second))))

    @staticmethod
    def windows_documents_folders() -> list[Path]:
        folders: list[Path] = []
        if os.name == "nt" and winreg is not None:
            try:
                with winreg.OpenKey(
                    winreg.HKEY_CURRENT_USER,
                    r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders",
                ) as key:
                    value, _value_type = winreg.QueryValueEx(key, "Personal")
                    if value:
                        folders.append(Path(os.path.expandvars(str(value))).expanduser())
            except OSError:
                pass
        folders.append(Path.home() / "Documents")
        for variable in ("OneDrive", "OneDriveConsumer", "OneDriveCommercial"):
            root = str(os.getenv(variable, "") or "").strip()
            if root:
                folders.append(Path(root) / "Documents")
        unique: list[Path] = []
        seen: set[str] = set()
        for folder in folders:
            key = os.path.normcase(os.path.abspath(str(folder)))
            if key not in seen:
                seen.add(key)
                unique.append(folder)
        return unique

    def discover_legacy_cookie_source(self) -> Path | None:
        if str(getattr(self.settings, "cookies_source_file", "") or "").strip():
            return None
        if self.normalized_cookies_browser():
            return None
        configured = str(getattr(self.settings, "cookies_file", "") or "").strip()
        if not configured or not self.paths_match(configured, CACHED_COOKIES_FILE):
            return None
        candidates: list[Path] = []
        for folder in self.windows_documents_folders():
            if not folder.is_dir():
                continue
            exact = folder / CACHED_COOKIES_FILE.name
            if exact.is_file():
                candidates.append(exact)
            try:
                candidates.extend(
                    path for path in folder.glob("*cookies*.txt")
                    if path.is_file() and path not in candidates
                )
            except OSError:
                pass
        valid: list[tuple[int, int, Path]] = []
        for candidate in candidates:
            try:
                _score, _youtube_count, total_count, _has_login = self.cookie_file_score(candidate)
                if total_count > 0:
                    exact_name = int(candidate.name.lower() == CACHED_COOKIES_FILE.name.lower())
                    valid.append((exact_name, candidate.stat().st_mtime_ns, candidate))
            except (OSError, ValueError):
                continue
            except Exception:
                continue
        if not valid:
            return None
        return max(valid, key=lambda item: (item[0], item[1]))[2]

    def migrate_legacy_cookie_source(self) -> str:
        source = self.discover_legacy_cookie_source()
        if source is None:
            return ""
        try:
            result = self.import_cookie_file_to_cache(source)
            self.remember_cookie_source(source, str(result["path"]))
            self.save_settings()
            return str(source)
        except Exception:
            return ""

    def configured_cookies_display_path(self) -> str:
        source = str(getattr(self.settings, "cookies_source_file", "") or "").strip()
        if not source:
            source = self.migrate_legacy_cookie_source()
        return source or str(getattr(self.settings, "cookies_file", "") or "").strip()

    def clear_cookie_login_cache(self) -> None:
        cache = getattr(self, "_youtube_cookie_login_cache", None)
        if isinstance(cache, dict):
            cache.clear()

    def clear_cookie_dependent_stream_cache(self) -> None:
        cache = getattr(self, "stream_url_cache", None)
        if not isinstance(cache, dict):
            return
        lock = getattr(self, "stream_url_cache_lock", None)
        if lock is None:
            cache.clear()
        else:
            with lock:
                cache.clear()
        try:
            self.save_stream_url_cache()
        except Exception:
            pass

    def remember_cookie_source(self, source_path: str | Path, imported_path: str) -> None:
        source = Path(os.path.expandvars(str(source_path).strip('"'))).expanduser()
        self.settings.cookies_source_file = str(source)
        try:
            self.settings.cookies_source_signature = self.cookie_source_signature(source)
        except OSError:
            self.settings.cookies_source_signature = ""
        self.settings.cookies_file = imported_path
        self.settings.cookies_from_browser = "none"
        self.settings.cookies_browser_profile = COOKIE_PROFILE_AUTO

    def effective_cookies_file(self) -> str:
        configured = str(getattr(self.settings, "cookies_file", "") or "").strip()
        source_value = str(getattr(self.settings, "cookies_source_file", "") or "").strip()
        self.cookie_source_refresh_error = ""
        if not source_value:
            source_value = self.migrate_legacy_cookie_source()
        if source_value:
            source_path = Path(os.path.expandvars(source_value.strip('"'))).expanduser()
            try:
                signature = self.cookie_source_signature(source_path)
            except OSError as exc:
                signature = ""
                self.cookie_source_refresh_error = str(exc)
            cache_ready = False
            try:
                cache_ready = CACHED_COOKIES_FILE.exists() and CACHED_COOKIES_FILE.stat().st_size > 0
            except OSError:
                pass
            if signature and (
                signature != str(getattr(self.settings, "cookies_source_signature", "") or "")
                or not cache_ready
            ):
                try:
                    result = self.import_cookie_file_to_cache(source_path)
                    self.remember_cookie_source(source_path, str(result["path"]))
                    self.save_settings()
                    return str(result["path"])
                except Exception as exc:
                    self.cookie_source_refresh_error = self.friendly_error(exc)
            if cache_ready:
                return str(CACHED_COOKIES_FILE)
        if configured:
            configured_path = Path(os.path.expandvars(configured.strip('"'))).expanduser()
            try:
                same_as_cache = configured_path.resolve() == CACHED_COOKIES_FILE.resolve()
            except OSError:
                same_as_cache = False
            if not same_as_cache and configured_path.exists():
                try:
                    result = self.import_cookie_file_to_cache(configured_path)
                    self.remember_cookie_source(configured_path, str(result["path"]))
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
        value = str(value)
        if not self.cookie_fields_are_safe(name, value, domain, path):
            return None
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
            value=value,
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

    @staticmethod
    def cookie_fields_are_safe(name: str, value: str, domain: str, path: str) -> bool:
        return all(not re.search(r"[\x00-\x1f\x7f]", str(field or "")) for field in (name, value, domain, path))

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
        CACHED_COOKIES_FILE.parent.mkdir(parents=True, exist_ok=True)
        descriptor, temp_name = tempfile.mkstemp(prefix=".cookies-import-", suffix=".tmp", dir=str(CACHED_COOKIES_FILE.parent))
        temp_path = Path(temp_name)
        try:
            with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as handle:
                descriptor = -1
                handle.write(normalized)
            cookiejar = import_module("http.cookiejar")
            jar = cookiejar.MozillaCookieJar()
            jar.load(str(temp_path), ignore_discard=True, ignore_expires=True)
            return jar
        finally:
            if descriptor >= 0:
                try:
                    os.close(descriptor)
                except OSError:
                    pass
            try:
                temp_path.unlink(missing_ok=True)
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
        cookie_jar = self.youtube_cookie_jar(cookie_jar)
        CACHED_COOKIES_FILE.parent.mkdir(parents=True, exist_ok=True)
        descriptor, temp_name = tempfile.mkstemp(prefix=".cookies-save-", suffix=".tmp", dir=str(CACHED_COOKIES_FILE.parent))
        os.close(descriptor)
        temp_path = Path(temp_name)
        try:
            cookie_jar.save(str(temp_path), ignore_discard=True, ignore_expires=True)
            os.replace(temp_path, CACHED_COOKIES_FILE)
        finally:
            try:
                temp_path.unlink(missing_ok=True)
            except OSError:
                pass
        self.clear_cookie_login_cache()
        self.clear_cookie_dependent_stream_cache()

    def import_cookie_file_to_cache(self, source_path: str | Path) -> dict:
        source = Path(source_path)
        if source.stat().st_size > COOKIES_FILE_MAX_BYTES:
            raise RuntimeError(self.t("cookies_file_unsupported"))
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
        jar = self.youtube_cookie_jar(jar)
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

    @staticmethod
    def validate_devtools_websocket_url(websocket_url: str, expected_port: int) -> str:
        websocket = urlparse(str(websocket_url or ""))
        try:
            port = websocket.port
        except ValueError:
            port = None
        if (
            websocket.scheme.lower() not in {"ws", "wss"}
            or (websocket.hostname or "").lower() not in {"127.0.0.1", "localhost", "::1"}
            or websocket.username is not None
            or websocket.password is not None
            or port != int(expected_port)
        ):
            raise RuntimeError("browser devtools websocket is missing")
        return websocket_url

    async def devtools_get_all_cookies(self, websocket_url: str) -> list[dict]:
        websockets = import_module("websockets")
        async with websockets.connect(websocket_url, max_size=32_000_000) as ws:
            await ws.send(
                json.dumps(
                    {
                        "id": 1,
                        "method": "Network.getCookies",
                        "params": {
                            "urls": [
                                "https://www.youtube.com/",
                                "https://music.youtube.com/",
                                "https://accounts.google.com/",
                                "https://www.google.com/",
                                "https://redirector.googlevideo.com/",
                            ]
                        },
                    }
                )
            )
            while True:
                payload = json.loads(await ws.recv())
                if payload.get("id") != 1:
                    continue
                if payload.get("error"):
                    raise RuntimeError(str(payload["error"]))
                return list((payload.get("result") or {}).get("cookies") or [])

    def cdp_cookies_to_cookie_jar(self, cookies: list[dict]) -> http.cookiejar.MozillaCookieJar:
        cookiejar = import_module("http.cookiejar")
        jar = cookiejar.MozillaCookieJar()
        for item in cookies:
            cookie = self.cookie_from_mapping(item)
            if cookie and self.cookie_domain_is_youtube_related(cookie.domain):
                jar.set_cookie(cookie)
        return jar

    @staticmethod
    def cookie_domain_is_youtube_related(domain: str) -> bool:
        return any(CookiesUI.cookie_domain_matches(domain, root) for root in YOUTUBE_COOKIE_DOMAIN_ROOTS)

    @staticmethod
    def cookie_domain_matches(domain: str, root: str) -> bool:
        host = str(domain or "").strip().lower().lstrip(".").rstrip(".")
        normalized_root = str(root or "").strip().lower().lstrip(".").rstrip(".")
        return bool(host and normalized_root) and (host == normalized_root or host.endswith("." + normalized_root))

    def youtube_cookie_jar(self, cookie_jar) -> http.cookiejar.MozillaCookieJar:
        cookiejar = import_module("http.cookiejar")
        filtered = cookiejar.MozillaCookieJar()
        for cookie in cookie_jar:
            if self.cookie_domain_is_youtube_related(str(getattr(cookie, "domain", "") or "")) and self.cookie_fields_are_safe(
                str(getattr(cookie, "name", "") or ""),
                str(getattr(cookie, "value", "") or ""),
                str(getattr(cookie, "domain", "") or ""),
                str(getattr(cookie, "path", "") or "/"),
            ):
                filtered.set_cookie(cookie)
        return filtered

    def export_chromium_cookies_via_devtools(self, browser: str, profile: str | None, headless: bool = True) -> tuple[str, object]:
        executable = self.cookie_browser_executable(browser)
        if not executable:
            raise RuntimeError(f"{browser} executable not found")
        profile_label, base_args = self.chromium_profile_launch_args(browser, profile, headless=headless)
        port = self.free_local_port()
        args = [
            executable,
            f"--remote-debugging-port={port}",
            "--remote-debugging-address=127.0.0.1",
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
            self.validate_devtools_websocket_url(websocket_url, port)
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
    def cookie_jar_has_login_cookies(cookie_jar) -> bool:
        auth_names = MiscUI.youtube_auth_cookie_names()
        for cookie in cookie_jar:
            domain = str(getattr(cookie, "domain", "") or "").lower()
            name = str(getattr(cookie, "name", "") or "").lower()
            if (
                CookiesUI.cookie_domain_matches(domain, "google.com")
                or CookiesUI.cookie_domain_matches(domain, "youtube.com")
            ) and name in auth_names:
                return True
        return False

    @staticmethod
    def cookie_jar_score(cookie_jar) -> tuple[int, int, int]:
        auth_names = MiscUI.youtube_auth_cookie_names()
        score = 0
        youtube_count = 0
        total_count = 0
        for cookie in cookie_jar:
            total_count += 1
            domain = str(getattr(cookie, "domain", "") or "").lower()
            name = str(getattr(cookie, "name", "") or "").lower()
            is_youtube = CookiesUI.cookie_domain_matches(domain, "youtube.com")
            is_google = CookiesUI.cookie_domain_matches(domain, "google.com")
            if is_youtube:
                youtube_count += 1
                score += 3
            if is_google or is_youtube:
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
        if allow_close and browser in CHROMIUM_COOKIE_BROWSERS and browser != "chrome" and needs_devtools_fallback:
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
        self.save_cookie_jar_to_cache(cookie_jar)
        self.settings.cookies_file = str(CACHED_COOKIES_FILE)
        self.settings.cookies_source_file = ""
        self.settings.cookies_source_signature = ""
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

