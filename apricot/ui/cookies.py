from apricot.constants import *
import wx
import os
from pathlib import Path
from apricot.ui.misc import MiscUI

class CookiesUI:
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
    def cookie_jar_has_login_cookies(cookie_jar) -> bool:
        auth_names = MiscUI.youtube_auth_cookie_names()
        for cookie in cookie_jar:
            domain = str(getattr(cookie, "domain", "") or "").lower()
            name = str(getattr(cookie, "name", "") or "").lower()
            if ("google.com" in domain or "youtube.com" in domain) and name in auth_names:
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

