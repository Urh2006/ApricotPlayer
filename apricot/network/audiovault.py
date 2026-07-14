from __future__ import annotations

import html
import http.cookiejar
import base64
import ctypes
import io
import os
import re
import shutil
import stat
import threading
import urllib.error
import urllib.parse
import urllib.request
import webbrowser
import zipfile
from html.parser import HTMLParser
from pathlib import Path

import wx

from apricot.constants import APP_NAME, APP_VERSION


AUDIOVAULT_BASE_URL = "https://direct.audiovault.net"
AUDIOVAULT_REGISTER_URL = f"{AUDIOVAULT_BASE_URL}/register"
AUDIOVAULT_MAX_RESPONSE = 8 * 1024 * 1024
AUDIOVAULT_MAX_ARCHIVE = 8 * 1024 * 1024 * 1024
AUDIOVAULT_MAX_EXTRACTED = 16 * 1024 * 1024 * 1024
AUDIOVAULT_MAX_RANGE_READ = 8 * 1024 * 1024
AUDIOVAULT_RANGE_CACHE = 4 * 1024 * 1024
AUDIOVAULT_MAX_ARCHIVE_ENTRIES = 10_000
_AUDIO_EXTENSIONS = {".aac", ".flac", ".m4a", ".m4b", ".mp3", ".ogg", ".opus", ".wav", ".wma"}
_AUDIOVAULT_DPAPI_ENTROPY = b"ApricotPlayer AudioVault credentials v1"


class AudioVaultSessionExpired(PermissionError):
    pass


class AudioVaultRangeUnsupported(RuntimeError):
    pass


class _AudioVaultRangeReader(io.RawIOBase):
    def __init__(self, size: int, read_range, cache_size: int = AUDIOVAULT_RANGE_CACHE) -> None:
        super().__init__()
        self.size = max(0, int(size))
        self.read_range = read_range
        self.cache_size = max(1, min(int(cache_size), AUDIOVAULT_MAX_RANGE_READ))
        self.position = 0
        self.cache_start = 0
        self.cache = b""

    def readable(self) -> bool:
        return True

    def seekable(self) -> bool:
        return True

    def tell(self) -> int:
        return self.position

    def seek(self, offset: int, whence: int = io.SEEK_SET) -> int:
        if whence == io.SEEK_SET:
            position = int(offset)
        elif whence == io.SEEK_CUR:
            position = self.position + int(offset)
        elif whence == io.SEEK_END:
            position = self.size + int(offset)
        else:
            raise ValueError("Invalid seek mode")
        if position < 0:
            raise ValueError("Negative seek position")
        self.position = min(position, self.size)
        return self.position

    def read(self, size: int = -1) -> bytes:
        if self.position >= self.size or size == 0:
            return b""
        requested = self.size - self.position if size is None or size < 0 else min(int(size), self.size - self.position)
        if requested <= 0:
            return b""
        cache_end = self.cache_start + len(self.cache)
        requested_end = self.position + requested
        if self.cache_start <= self.position and requested_end <= cache_end:
            offset = self.position - self.cache_start
            data = self.cache[offset : offset + requested]
            self.position += len(data)
            return data
        fetch_size = min(AUDIOVAULT_MAX_RANGE_READ, max(requested, self.cache_size))
        fetch_end = min(self.size - 1, self.position + fetch_size - 1)
        data = bytes(self.read_range(self.position, fetch_end))
        expected_max = fetch_end - self.position + 1
        if not data or len(data) > expected_max:
            raise OSError("AudioVault returned an invalid byte range.")
        self.cache_start = self.position
        self.cache = data
        result = data[:requested]
        self.position += len(result)
        return result


class _DataBlob(ctypes.Structure):
    _fields_ = [("cbData", ctypes.c_ulong), ("pbData", ctypes.POINTER(ctypes.c_ubyte))]


def _blob_from_bytes(data: bytes):
    buffer = ctypes.create_string_buffer(data)
    blob = _DataBlob(len(data), ctypes.cast(buffer, ctypes.POINTER(ctypes.c_ubyte)))
    return blob, buffer


def _windows_crypto_libraries():
    crypt32 = ctypes.windll.crypt32
    kernel32 = ctypes.windll.kernel32
    blob_pointer = ctypes.POINTER(_DataBlob)
    crypt32.CryptProtectData.argtypes = [
        blob_pointer,
        ctypes.c_wchar_p,
        blob_pointer,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_ulong,
        blob_pointer,
    ]
    crypt32.CryptProtectData.restype = ctypes.c_bool
    crypt32.CryptUnprotectData.argtypes = [
        blob_pointer,
        ctypes.c_void_p,
        blob_pointer,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_ulong,
        blob_pointer,
    ]
    crypt32.CryptUnprotectData.restype = ctypes.c_bool
    kernel32.LocalFree.argtypes = [ctypes.c_void_p]
    kernel32.LocalFree.restype = ctypes.c_void_p
    return crypt32, kernel32


def protect_audiovault_password(password: str) -> str:
    if os.name != "nt" or not password:
        return ""
    raw_blob, raw_buffer = _blob_from_bytes(password.encode("utf-8"))
    entropy_blob, entropy_buffer = _blob_from_bytes(_AUDIOVAULT_DPAPI_ENTROPY)
    output = _DataBlob()
    crypt32, kernel32 = _windows_crypto_libraries()
    success = crypt32.CryptProtectData(
        ctypes.byref(raw_blob),
        "ApricotPlayer AudioVault",
        ctypes.byref(entropy_blob),
        None,
        None,
        0x1,
        ctypes.byref(output),
    )
    _ = raw_buffer, entropy_buffer
    if not success:
        raise ctypes.WinError()
    try:
        return base64.b64encode(ctypes.string_at(output.pbData, output.cbData)).decode("ascii")
    finally:
        kernel32.LocalFree(ctypes.cast(output.pbData, ctypes.c_void_p))


def unprotect_audiovault_password(value: str) -> str:
    if os.name != "nt" or not value:
        return ""
    try:
        encrypted = base64.b64decode(value.encode("ascii"), validate=True)
    except (ValueError, UnicodeError):
        return ""
    encrypted_blob, encrypted_buffer = _blob_from_bytes(encrypted)
    entropy_blob, entropy_buffer = _blob_from_bytes(_AUDIOVAULT_DPAPI_ENTROPY)
    output = _DataBlob()
    crypt32, kernel32 = _windows_crypto_libraries()
    success = crypt32.CryptUnprotectData(
        ctypes.byref(encrypted_blob),
        None,
        ctypes.byref(entropy_blob),
        None,
        None,
        0x1,
        ctypes.byref(output),
    )
    _ = encrypted_buffer, entropy_buffer
    if not success:
        return ""
    try:
        return ctypes.string_at(output.pbData, output.cbData).decode("utf-8")
    except UnicodeError:
        return ""
    finally:
        kernel32.LocalFree(ctypes.cast(output.pbData, ctypes.c_void_p))


class _VaultPageParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.rows: list[list[str]] = []
        self.links: list[str] = []
        self.records: list[dict] = []
        self.token = ""
        self.section = ""
        self._row: list[str] | None = None
        self._cell: list[str] | None = None
        self._row_links: list[str] = []
        self._heading: list[str] | None = None

    def handle_starttag(self, tag: str, attrs) -> None:
        values = dict(attrs)
        if tag == "tr":
            self._row = []
            self._row_links = []
        elif tag == "td" and self._row is not None:
            self._cell = []
        elif tag in {"h1", "h2", "h3", "h4", "h5", "h6"}:
            self._heading = []
        elif tag == "a" and values.get("href"):
            self.links.append(values["href"])
            if self._row is not None:
                self._row_links.append(values["href"])
        elif tag == "input" and values.get("name") == "_token":
            self.token = values.get("value", "")

    def handle_data(self, data: str) -> None:
        if self._cell is not None:
            self._cell.append(data)
        if self._heading is not None:
            self._heading.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag == "td" and self._row is not None and self._cell is not None:
            self._row.append(" ".join("".join(self._cell).split()))
            self._cell = None
        elif tag == "tr" and self._row is not None:
            if self._row:
                self.rows.append(self._row)
                download_link = next((link for link in self._row_links if re.search(r"/download/\d+", link)), "")
                self.records.append({"section": self.section, "row": list(self._row), "link": download_link})
            self._row = None
            self._row_links = []
        elif tag in {"h1", "h2", "h3", "h4", "h5", "h6"} and self._heading is not None:
            self.section = " ".join("".join(self._heading).split()).rstrip(":").lower()
            self._heading = None


class AudioVaultMixin:
    def init_audiovault(self) -> None:
        self.audiovault_cookie_jar = http.cookiejar.CookieJar()
        self.audiovault_opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(self.audiovault_cookie_jar)
        )
        self.audiovault_logged_in = False
        self.audiovault_screen_active = False
        self.audiovault_mode = "movies"
        self.audiovault_results: list[dict] = []
        self.audiovault_parent_results: list[dict] = []
        self.audiovault_view = "menu"
        self.audiovault_results_title = ""
        self.audiovault_parent_view = ""
        self.audiovault_parent_title = ""
        self.audiovault_show_manifests: dict[str, list[dict]] = {}
        self.audiovault_manifest_loading: set[str] = set()
        self.audiovault_episode_loading: set[str] = set()
        self.audiovault_progress_task_id = ""
        self.audiovault_progress_generation = 0
        self.audiovault_show_request_generation = 0

    def audiovault_request(
        self,
        url: str,
        data: dict | None = None,
        timeout: int = 30,
        headers: dict | None = None,
    ):
        parsed = urllib.parse.urlparse(url)
        if parsed.scheme != "https" or parsed.hostname not in {"direct.audiovault.net", "www.audiovault.net", "audiovault.net"}:
            raise ValueError(self.t("audiovault_untrusted_url"))
        payload = urllib.parse.urlencode(data).encode("utf-8") if data is not None else None
        request_headers = {"User-Agent": f"{APP_NAME}/{APP_VERSION}", "Accept": "text/html,application/octet-stream"}
        request_headers.update(headers or {})
        request = urllib.request.Request(
            url,
            data=payload,
            headers=request_headers,
        )
        return self.audiovault_opener.open(request, timeout=timeout)

    @staticmethod
    def audiovault_read_page(response) -> str:
        length = int(response.headers.get("Content-Length") or 0)
        if length > AUDIOVAULT_MAX_RESPONSE:
            raise ValueError("AudioVault response is unexpectedly large.")
        data = response.read(AUDIOVAULT_MAX_RESPONSE + 1)
        if len(data) > AUDIOVAULT_MAX_RESPONSE:
            raise ValueError("AudioVault response is unexpectedly large.")
        return data.decode("utf-8", errors="replace")

    @staticmethod
    def audiovault_response_is_login(response, page: str = "") -> bool:
        path = urllib.parse.urlparse(str(response.geturl() or "")).path.rstrip("/") or "/"
        return path == "/login" or 'name="password"' in page or "name='password'" in page

    def audiovault_read_authenticated_page(self, response) -> str:
        page = self.audiovault_read_page(response)
        if self.audiovault_response_is_login(response, page):
            self.audiovault_logged_in = False
            raise AudioVaultSessionExpired(self.t("audiovault_session_expired"))
        return page

    def show_audiovault_login(self, after_login=None) -> bool:
        dialog = wx.Dialog(self, title=self.t("audiovault_login"), style=wx.DEFAULT_DIALOG_STYLE)
        dialog.SetName(self.t("audiovault_login"))
        outer = wx.BoxSizer(wx.VERTICAL)
        form = wx.FlexGridSizer(2, 2, 6, 6)
        form.AddGrowableCol(1, 1)
        form.Add(wx.StaticText(dialog, label=self.t("email")), 0, wx.ALIGN_CENTER_VERTICAL)
        email = wx.TextCtrl(dialog, value=str(getattr(self.settings, "audiovault_email", "") or ""))
        email.SetName(self.t("email"))
        form.Add(email, 1, wx.EXPAND)
        form.Add(wx.StaticText(dialog, label=self.t("password")), 0, wx.ALIGN_CENTER_VERTICAL)
        password = wx.TextCtrl(dialog, style=wx.TE_PASSWORD | wx.TE_PROCESS_ENTER)
        password.SetName(self.t("password"))
        form.Add(password, 1, wx.EXPAND)
        outer.Add(form, 1, wx.ALL | wx.EXPAND, 10)
        buttons = dialog.CreateButtonSizer(wx.OK | wx.CANCEL)
        register = wx.Button(dialog, label=self.t("register"))
        register.Bind(wx.EVT_BUTTON, lambda _evt: webbrowser.open(AUDIOVAULT_REGISTER_URL))
        outer.Add(register, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 10)
        outer.Add(buttons, 0, wx.ALL | wx.ALIGN_RIGHT, 10)
        dialog.SetSizerAndFit(outer)
        password.Bind(wx.EVT_TEXT_ENTER, lambda _evt: dialog.EndModal(wx.ID_OK))
        email.SetFocus()
        wx.CallAfter(email.SetFocus)
        if dialog.ShowModal() != wx.ID_OK:
            dialog.Destroy()
            return False
        address, secret = email.GetValue().strip(), password.GetValue()
        dialog.Destroy()
        if not address or not secret:
            self.message(self.t("audiovault_credentials_required"), wx.ICON_ERROR)
            return False
        self.set_status(self.t("audiovault_logging_in"))
        threading.Thread(target=self.audiovault_login_worker, args=(address, secret, after_login, True), daemon=True).start()
        return True

    def audiovault_login_worker(self, email: str, password: str, after_login=None, remember: bool = True) -> None:
        try:
            with self.audiovault_request(f"{AUDIOVAULT_BASE_URL}/login") as response:
                parser = _VaultPageParser()
                parser.feed(self.audiovault_read_page(response))
            if not parser.token:
                raise ValueError(self.t("audiovault_login_page_error"))
            with self.audiovault_request(
                f"{AUDIOVAULT_BASE_URL}/login",
                {"_token": parser.token, "email": email, "password": password, "remember": "on"},
            ) as response:
                final_url = response.geturl()
                page = self.audiovault_read_page(response)
            if urllib.parse.urlparse(final_url).path == "/login" or 'name="password"' in page:
                raise ValueError(self.t("audiovault_login_failed"))
            self.audiovault_logged_in = True
            self.settings.audiovault_email = email
            if remember:
                self.settings.audiovault_password_protected = protect_audiovault_password(password)
            self.save_settings()
            wx.CallAfter(self.set_status, self.t("audiovault_logged_in"))
            if after_login:
                wx.CallAfter(after_login)
        except Exception as exc:
            self.audiovault_logged_in = False
            retry_interactively = not remember
            if retry_interactively:
                self.settings.audiovault_password_protected = ""
                self.save_settings()
            wx.CallAfter(
                self.finish_audiovault_login_error,
                self.t("audiovault_login_error", error=self.friendly_error(exc)),
                after_login,
                retry_interactively,
            )

    def finish_audiovault_login_error(self, message: str, after_login=None, retry_interactively: bool = False) -> None:
        self.message(message, wx.ICON_ERROR)
        if retry_interactively:
            self.show_audiovault_login(after_login)

    def open_audiovault_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_audiovault_menu)

    def ensure_audiovault_login(self, after_login) -> bool:
        if self.audiovault_logged_in:
            return True
        email = str(getattr(self.settings, "audiovault_email", "") or "").strip()
        password = unprotect_audiovault_password(str(getattr(self.settings, "audiovault_password_protected", "") or ""))
        if email and password:
            self.set_status(self.t("audiovault_logging_in"))
            threading.Thread(
                target=self.audiovault_login_worker,
                args=(email, password, after_login, False),
                daemon=True,
            ).start()
        else:
            self.show_audiovault_login(after_login)
        return False

    def retry_audiovault_after_login(self, callback) -> None:
        self.audiovault_logged_in = False
        try:
            self.audiovault_cookie_jar.clear()
        except Exception:
            pass
        self.ensure_audiovault_login(callback)

    def prepare_audiovault_screen(self, view: str) -> None:
        self.in_main_menu = False
        self.in_player_screen = False
        self.search_screen_active = False
        self.audiovault_screen_active = True
        self.audiovault_view = view
        self.search_generation += 1
        self.dynamic_fetch_enabled = False
        self.loading_more_results = False
        self.collection_url = ""
        self.collection_result_type = ""
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = True
        self.pending_player_next_after_dynamic_load = False
        self.pending_player_next_preserve_focus = False
        self.pending_player_next_current_url = ""
        if hasattr(self, "metadata_hydration_urls"):
            self.metadata_hydration_urls.clear()
        self.clear()
        self.add_background_player_section()

    def show_audiovault_menu(self) -> None:
        if not self.ensure_audiovault_login(self.show_audiovault_menu):
            return
        self.last_activated_menu_action = self.show_audiovault_menu
        self.audiovault_parent_results = []
        self.audiovault_parent_view = ""
        self.audiovault_parent_title = ""
        self.prepare_audiovault_screen("menu")
        self.add_button_row([(self.t("back"), self.show_main_menu), (self.t("open"), self.activate_audiovault_menu_item)])
        self.audiovault_menu_actions = [
            (self.t("search"), self.show_audiovault_search),
            (self.t("audiovault_recent_tv_shows"), lambda: self.show_audiovault_recent("shows")),
            (self.t("audiovault_recent_movies"), lambda: self.show_audiovault_recent("movies")),
        ]
        self.audiovault_menu_list = wx.ListBox(self.panel, choices=[label for label, _handler in self.audiovault_menu_actions])
        self.audiovault_menu_list.SetName(self.t("audiovault"))
        self.audiovault_menu_list.SetSelection(0)
        self.audiovault_menu_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.activate_audiovault_menu_item())
        self.audiovault_menu_list.Bind(wx.EVT_KEY_DOWN, self.on_audiovault_menu_key)
        self.root_sizer.Add(self.audiovault_menu_list, 1, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        self.focus_later(self.audiovault_menu_list)

    def on_audiovault_menu_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.activate_audiovault_menu_item()
        else:
            event.Skip()

    def activate_audiovault_menu_item(self) -> None:
        index = self.audiovault_menu_list.GetSelection() if hasattr(self, "audiovault_menu_list") else wx.NOT_FOUND
        if 0 <= index < len(self.audiovault_menu_actions):
            self.audiovault_menu_actions[index][1]()

    def show_audiovault_search(self) -> None:
        if not self.ensure_audiovault_login(self.show_audiovault_search):
            return
        self.last_activated_menu_action = self.show_audiovault_search
        self.prepare_audiovault_screen("search")
        self.audiovault_results_title = self.t("search")
        self.add_button_row([(self.t("back"), self.back_from_audiovault)])
        grid = wx.FlexGridSizer(2, 2, 6, 6)
        grid.AddGrowableCol(1, 1)
        grid.Add(wx.StaticText(self.panel, label=self.t("search_query")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.audiovault_query = wx.TextCtrl(self.panel, style=wx.TE_PROCESS_ENTER)
        self.audiovault_query.SetName(self.t("search_query"))
        self.audiovault_query.Bind(wx.EVT_TEXT_ENTER, lambda _evt: self.search_audiovault())
        grid.Add(self.audiovault_query, 1, wx.EXPAND)
        grid.Add(wx.StaticText(self.panel, label=self.t("type")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.audiovault_type = wx.Choice(self.panel, choices=[self.t("movies"), self.t("tv_shows")])
        self.audiovault_type.SetName(self.t("type"))
        self.audiovault_type.SetSelection(0 if self.audiovault_mode == "movies" else 1)
        self.audiovault_type.Bind(wx.EVT_KEY_DOWN, self.on_audiovault_type_key)
        grid.Add(self.audiovault_type, 1, wx.EXPAND)
        self.root_sizer.Add(grid, 0, wx.EXPAND | wx.ALL, 4)
        self.add_button_row([
            (self.t("search"), self.search_audiovault),
            (self.t("play"), self.activate_audiovault_item),
            (self.t("download_audio"), self.download_audiovault_selected),
        ])
        self.add_audiovault_results_list()
        self.panel.Layout()
        self.focus_later(self.audiovault_query)

    def on_audiovault_type_key(self, event: wx.KeyEvent) -> None:
        if event.GetKeyCode() in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}:
            self.search_audiovault()
        else:
            event.Skip()

    def add_audiovault_results_list(self, title: str = "") -> None:
        if title:
            label = wx.StaticText(self.panel, label=title)
            label.SetName(title)
            self.root_sizer.Add(label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 4)
        self.results_list = wx.ListBox(self.panel, choices=[self.t("search_results_empty")])
        self.results_list.SetName(self.t("results"))
        self.results_list.SetSelection(0)
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.activate_audiovault_item())
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_audiovault_results_key)
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_audiovault_context_menu)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)

    def show_audiovault_results_screen(self, title: str, view: str) -> None:
        self.prepare_audiovault_screen(view)
        self.audiovault_results_title = title
        self.add_button_row([
            (self.t("back"), self.back_from_audiovault),
            (self.t("play"), self.activate_audiovault_item),
            (self.t("download_audio"), self.download_audiovault_selected),
        ])
        self.add_audiovault_results_list(title)
        self.panel.Layout()

    def show_audiovault_recent(self, mode: str, allow_auth_retry: bool = True) -> None:
        if mode not in {"shows", "movies"}:
            return
        self.audiovault_mode = mode
        title = self.t("audiovault_recent_tv_shows" if mode == "shows" else "audiovault_recent_movies")
        self.show_audiovault_results_screen(title, f"recent_{mode}")
        self.set_status(self.t("audiovault_loading_recent"))
        threading.Thread(target=self.load_audiovault_recent_worker, args=(mode, allow_auth_retry), daemon=True).start()

    def load_audiovault_recent_worker(self, mode: str, allow_auth_retry: bool = True) -> None:
        try:
            with self.audiovault_request(f"{AUDIOVAULT_BASE_URL}/") as response:
                parser = _VaultPageParser()
                parser.feed(self.audiovault_read_authenticated_page(response))
            section = "recent shows" if mode == "shows" else "recent movies"
            results = self.audiovault_results_from_records(parser.records, mode, section=section)
            wx.CallAfter(self.show_audiovault_results, results)
        except AudioVaultSessionExpired as exc:
            if allow_auth_retry:
                wx.CallAfter(
                    self.retry_audiovault_after_login,
                    lambda: self.show_audiovault_recent(mode, allow_auth_retry=False),
                )
            else:
                wx.CallAfter(self.message, self.t("audiovault_search_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("audiovault_search_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def back_from_audiovault(self) -> None:
        self.audiovault_show_request_generation += 1
        if self.audiovault_parent_results:
            results = list(self.audiovault_parent_results)
            self.audiovault_parent_results = []
            view = self.audiovault_parent_view or "search"
            title = self.audiovault_parent_title or self.t("search")
            self.audiovault_parent_view = ""
            self.audiovault_parent_title = ""
            if view == "search":
                self.show_audiovault_search()
            else:
                self.show_audiovault_results_screen(title, view)
            self.show_audiovault_results(results)
            return
        if self.audiovault_view == "menu":
            self.show_main_menu()
        else:
            self.show_audiovault_menu()

    @staticmethod
    def audiovault_catalog_url(mode: str, query: str) -> str:
        catalog = "shows" if mode == "shows" else "movies"
        return f"{AUDIOVAULT_BASE_URL}/{catalog}?{urllib.parse.urlencode({'search': query})}"

    def audiovault_results_from_records(self, records: list[dict], mode: str, section: str = "") -> list[dict]:
        results: list[dict] = []
        for record in records:
            row = list(record.get("row") or [])
            link = str(record.get("link") or "")
            if section and str(record.get("section") or "") != section:
                continue
            if len(row) < 2 or not re.search(r"/download/\d+", link):
                continue
            absolute_link = urllib.parse.urljoin(AUDIOVAULT_BASE_URL, link)
            results.append({
                "id": row[0], "title": html.unescape(row[1]), "url": absolute_link,
                "webpage_url": absolute_link, "kind": "audiovault_show" if mode == "shows" else "audiovault_movie",
                "type": self.t("tv_show") if mode == "shows" else self.t("movie"), "channel": "AudioVault",
            })
        return results

    def search_audiovault(self) -> None:
        query = self.audiovault_query.GetValue().strip()
        if not query:
            self.message(self.t("enter_query"))
            return
        self.audiovault_mode = "movies" if self.audiovault_type.GetSelection() == 0 else "shows"
        self.set_status(self.t("searching", query=query))
        threading.Thread(target=self.search_audiovault_worker, args=(query, self.audiovault_mode), daemon=True).start()

    def search_audiovault_worker(self, query: str, mode: str, allow_auth_retry: bool = True) -> None:
        try:
            url = self.audiovault_catalog_url(mode, query)
            with self.audiovault_request(url) as response:
                parser = _VaultPageParser()
                parser.feed(self.audiovault_read_authenticated_page(response))
            results = self.audiovault_results_from_records(parser.records, mode)
            wx.CallAfter(self.show_audiovault_results, results)
        except AudioVaultSessionExpired as exc:
            if allow_auth_retry:
                wx.CallAfter(
                    self.retry_audiovault_after_login,
                    lambda: threading.Thread(
                        target=self.search_audiovault_worker,
                        args=(query, mode, False),
                        daemon=True,
                    ).start(),
                )
            else:
                wx.CallAfter(self.message, self.t("audiovault_search_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("audiovault_search_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def show_audiovault_results(self, results: list[dict]) -> None:
        self.audiovault_results = list(results)
        self.results = list(results)
        self.all_results = list(results)
        labels = [self.result_line(index, item) for index, item in enumerate(results)] or [self.t("no_results")]
        self.set_listbox_items(self.results_list, labels, 0)
        self.set_status(self.t("found", count=len(results)))
        self.focus_later(self.results_list)

    def selected_audiovault_item(self) -> dict | None:
        index = self.results_list.GetSelection() if hasattr(self, "results_list") else wx.NOT_FOUND
        return self.audiovault_results[index] if 0 <= index < len(self.audiovault_results) else None

    def on_audiovault_results_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.activate_audiovault_item()
        elif self.shortcut_matches(event, "download_audio"):
            self.download_audiovault_selected()
        elif self.shortcut_matches(event, "download_video"):
            self.announce_player(self.t("audiovault_video_unavailable"))
        elif self.context_menu_shortcut_matches(event):
            self.open_audiovault_context_menu()
        else:
            event.Skip()

    def open_audiovault_context_menu(self, _event=None) -> None:
        item = self.selected_audiovault_item()
        if not item:
            return
        menu = wx.Menu()
        actions = [
            (self.t("open"), self.activate_audiovault_item),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), self.download_audiovault_selected),
        ]
        if item.get("kind") == "audiovault_show":
            actions[1] = (self.t("download_tv_show"), self.download_audiovault_selected)
        for label, handler in actions:
            menu_item = menu.Append(wx.ID_ANY, label)
            menu.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), menu_item)
        self.PopupMenu(menu)
        menu.Destroy()

    def activate_audiovault_item(self) -> None:
        item = self.selected_audiovault_item()
        if not item:
            return
        if item.get("kind") == "audiovault_show":
            self.prepare_audiovault_show(item, download_after=False)
        elif item.get("kind") == "audiovault_remote_episode":
            self.prepare_audiovault_remote_episode(item)
        elif item.get("kind") == "audiovault_episode":
            self.play_audiovault_local_item(item)
        else:
            self.play_audiovault_remote_item(item)

    def audiovault_cookie_header(self) -> str:
        return "; ".join(f"{cookie.name}={cookie.value}" for cookie in self.audiovault_cookie_jar)

    def audiovault_archive_size(self, url: str) -> int:
        response = self.audiovault_request(url, timeout=60, headers={"Range": "bytes=0-0"})
        try:
            content_type = str(response.headers.get("Content-Type") or "").lower()
            if "text/html" in content_type or self.audiovault_response_is_login(response):
                self.audiovault_logged_in = False
                raise AudioVaultSessionExpired(self.t("audiovault_session_expired"))
            content_range = str(response.headers.get("Content-Range") or "")
            match = re.fullmatch(r"bytes\s+0-0/(\d+)", content_range, re.I)
            if int(getattr(response, "status", 0) or 0) != 206 or not match:
                raise AudioVaultRangeUnsupported("AudioVault does not support partial archive reads.")
            archive_size = int(match.group(1))
            if archive_size <= 0 or archive_size > AUDIOVAULT_MAX_ARCHIVE:
                raise ValueError(self.t("audiovault_archive_too_large"))
            if len(response.read(2)) != 1:
                raise OSError("AudioVault returned an invalid byte range.")
            return archive_size
        finally:
            response.close()

    def audiovault_read_range(self, url: str, start: int, end: int) -> bytes:
        start = int(start)
        end = int(end)
        if start < 0 or end < start or end - start + 1 > AUDIOVAULT_MAX_RANGE_READ:
            raise ValueError("Invalid AudioVault byte range.")
        response = self.audiovault_request(url, timeout=60, headers={"Range": f"bytes={start}-{end}"})
        try:
            content_type = str(response.headers.get("Content-Type") or "").lower()
            if "text/html" in content_type or self.audiovault_response_is_login(response):
                self.audiovault_logged_in = False
                raise AudioVaultSessionExpired(self.t("audiovault_session_expired"))
            content_range = str(response.headers.get("Content-Range") or "")
            match = re.fullmatch(r"bytes\s+(\d+)-(\d+)/(\d+)", content_range, re.I)
            if int(getattr(response, "status", 0) or 0) != 206 or not match:
                raise AudioVaultRangeUnsupported("AudioVault does not support partial archive reads.")
            actual_start, actual_end = int(match.group(1)), int(match.group(2))
            if actual_start != start or actual_end > end:
                raise OSError("AudioVault returned the wrong byte range.")
            expected = actual_end - actual_start + 1
            data = response.read(expected + 1)
            if len(data) != expected:
                raise OSError("AudioVault returned an incomplete byte range.")
            return data
        finally:
            response.close()

    @staticmethod
    def validate_audiovault_zip_infos(infos: list[zipfile.ZipInfo]) -> None:
        if len(infos) > AUDIOVAULT_MAX_ARCHIVE_ENTRIES:
            raise ValueError("AudioVault archive contains too many entries.")
        total = 0
        seen: set[str] = set()
        for member in infos:
            name = str(member.filename or "").replace("\\", "/")
            relative = Path(name)
            if (
                not name
                or name.startswith("/")
                or relative.is_absolute()
                or ".." in relative.parts
                or any(":" in part or part.rstrip(" .") != part for part in relative.parts)
            ):
                raise ValueError("Unsafe path in AudioVault archive.")
            normalized = name.rstrip("/").casefold()
            if normalized and normalized in seen:
                raise ValueError("AudioVault archive contains duplicate paths.")
            seen.add(normalized)
            if member.flag_bits & 0x1:
                raise ValueError("Encrypted AudioVault archives are not supported.")
            mode = member.external_attr >> 16
            if mode and stat.S_ISLNK(mode):
                raise ValueError("AudioVault archive contains an unsafe link.")
            total += max(0, int(member.file_size))
            if total > AUDIOVAULT_MAX_EXTRACTED:
                raise ValueError("AudioVault archive expands beyond the safety limit.")
            if member.file_size > 1024 * 1024 and (
                member.compress_size <= 0 or member.file_size / member.compress_size > 1000
            ):
                raise ValueError("AudioVault archive has an unsafe compression ratio.")

    def audiovault_show_manifest_key(self, show: dict) -> str:
        return str(show.get("id") or show.get("url") or "")

    def audiovault_remote_episode_cache_path(self, show: dict, member_name: str, index: int) -> Path:
        source_name = Path(str(member_name).replace("\\", "/")).name
        suffix = Path(source_name).suffix.lower()
        stem = Path(source_name).stem
        safe_stem = self.safe_folder_name(stem)[:120]
        filename = f"{index + 1:04d} - {safe_stem}{suffix}"
        return self.audiovault_show_cache_dir(show) / "_episodes" / filename

    @staticmethod
    def audiovault_cache_path_is_link(path: Path) -> bool:
        try:
            metadata = os.lstat(path)
        except OSError:
            return False
        attributes = int(getattr(metadata, "st_file_attributes", 0) or 0)
        reparse_flag = int(getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400) or 0x400)
        return stat.S_ISLNK(metadata.st_mode) or bool(attributes & reparse_flag)

    def trim_audiovault_episode_cache(self, protected_paths=None) -> None:
        root = Path(self.settings.cache_folder).expanduser() / "audiovault"
        if not root.is_dir() or self.audiovault_cache_path_is_link(root):
            return
        protected = {
            os.path.normcase(os.path.abspath(str(path)))
            for path in (protected_paths or set())
            if path
        }
        limit_mb = max(1, int(getattr(self.settings, "cache_size_mb", 512) or 512))
        limit = limit_mb * 1024 * 1024
        candidates = []
        total = 0
        try:
            show_folders = list(root.iterdir())
        except OSError:
            return
        for show_folder in show_folders:
            episode_folder = show_folder / "_episodes"
            if (
                not show_folder.is_dir()
                or self.audiovault_cache_path_is_link(show_folder)
                or not episode_folder.is_dir()
                or self.audiovault_cache_path_is_link(episode_folder)
            ):
                continue
            try:
                episode_files = list(episode_folder.iterdir())
            except OSError:
                continue
            for path in episode_files:
                if (
                    not path.is_file()
                    or path.suffix.lower() not in _AUDIO_EXTENSIONS
                    or self.audiovault_cache_path_is_link(path)
                ):
                    continue
                try:
                    metadata = path.stat()
                except OSError:
                    continue
                size = max(0, int(metadata.st_size))
                total += size
                candidates.append((float(metadata.st_mtime), str(path).casefold(), path, size))
        if total <= limit:
            return
        for _modified, _name, path, size in sorted(candidates):
            if total <= limit:
                break
            if os.path.normcase(os.path.abspath(str(path))) in protected:
                continue
            try:
                path.unlink()
                total -= size
            except OSError:
                continue

    def audiovault_remote_episode_items(self, show: dict) -> list[dict]:
        archive_url = str(show.get("url") or "")
        archive_size = self.audiovault_archive_size(archive_url)
        reader = _AudioVaultRangeReader(
            archive_size,
            lambda start, end: self.audiovault_read_range(archive_url, start, end),
        )
        with zipfile.ZipFile(reader) as package:
            infos = package.infolist()
            self.validate_audiovault_zip_infos(infos)
        audio_infos = [member for member in infos if not member.is_dir() and Path(member.filename).suffix.lower() in _AUDIO_EXTENSIONS]
        audio_infos.sort(key=lambda member: self.natural_sort_key(member.filename))
        episodes = []
        for index, member in enumerate(audio_infos):
            target = self.audiovault_remote_episode_cache_path(show, member.filename, index)
            episodes.append({
                "title": Path(member.filename).stem,
                "url": str(target),
                "path": str(target),
                "local_path": str(target),
                "webpage_url": archive_url,
                "kind": "audiovault_remote_episode",
                "type": self.t("episode"),
                "channel": show.get("title", "AudioVault"),
                "audiovault_show": dict(show),
                "archive_url": archive_url,
                "archive_size": archive_size,
                "archive_member": member.filename,
                "archive_crc": int(member.CRC),
                "archive_file_size": int(member.file_size),
                "archive_compress_size": int(member.compress_size),
            })
        return episodes

    def next_audiovault_progress_task_id(self) -> str:
        self.audiovault_progress_generation += 1
        return f"audiovault-{self.audiovault_progress_generation}"

    def show_audiovault_progress(self, task_id: str, message: str) -> None:
        self.audiovault_progress_task_id = task_id
        self.set_status(message)

    def update_audiovault_progress(self, task_id: str, percent: int | None, message: str) -> None:
        if task_id != getattr(self, "audiovault_progress_task_id", ""):
            return
        self.set_status(message)

    def close_audiovault_progress(self, task_id: str) -> None:
        if task_id != getattr(self, "audiovault_progress_task_id", ""):
            return
        self.audiovault_progress_task_id = ""

    def resolve_audiovault_stream(self, url: str):
        response = self.audiovault_request(url, timeout=60)
        final_url = response.geturl()
        content_type = str(response.headers.get("Content-Type") or "").lower()
        disposition = str(response.headers.get("Content-Disposition") or "")
        if "text/html" in content_type or self.audiovault_response_is_login(response):
            response.close()
            self.audiovault_logged_in = False
            raise AudioVaultSessionExpired(self.t("audiovault_session_expired"))
        headers = {"User-Agent": f"{APP_NAME}/{APP_VERSION}", "Referer": AUDIOVAULT_BASE_URL}
        cookie = self.audiovault_cookie_header()
        if cookie:
            headers["Cookie"] = cookie
        return response, final_url, content_type, disposition, headers

    def play_audiovault_remote_item(self, item: dict, allow_auth_retry: bool = True) -> None:
        self.set_status(self.t("preparing_stream", title=item.get("title", "")))
        threading.Thread(target=self.play_audiovault_remote_worker, args=(dict(item), allow_auth_retry), daemon=True).start()

    def play_audiovault_remote_worker(self, item: dict, allow_auth_retry: bool = True) -> None:
        try:
            response, final_url, content_type, disposition, headers = self.resolve_audiovault_stream(item["url"])
            response.close()
            if "zip" in content_type or ".zip" in disposition.lower():
                wx.CallAfter(self.prepare_audiovault_show, item, False)
                return
            item.update({"stream_url": final_url, "http_headers": headers})
            wx.CallAfter(self.start_audiovault_player, item, final_url, headers)
        except AudioVaultSessionExpired as exc:
            if allow_auth_retry:
                wx.CallAfter(
                    self.retry_audiovault_after_login,
                    lambda: self.play_audiovault_remote_item(item, allow_auth_retry=False),
                )
            else:
                wx.CallAfter(self.message, self.t("player_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("player_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def start_audiovault_player(self, item: dict, stream_url: str, headers: dict) -> None:
        self.current_video_item = dict(item)
        self.current_video_info = dict(item)
        self.current_video_item["_audiovault_stream_url"] = stream_url
        self.current_video_item["_audiovault_headers"] = headers
        self.current_video_info.update(self.current_video_item)
        self.player_return_screen = "audiovault"
        self.player_return_data = self.audiovault_player_return_state()
        self.play_url(item["url"], item.get("title", ""))

    def audiovault_player_return_state(self) -> dict:
        return {
            "mode": self.audiovault_mode,
            "results": list(self.audiovault_results),
            "view": self.audiovault_view,
            "title": self.audiovault_results_title,
            "parent_results": list(self.audiovault_parent_results),
            "parent_view": self.audiovault_parent_view,
            "parent_title": self.audiovault_parent_title,
        }

    def restore_audiovault_player_results(self, data: dict, results: list[dict]) -> None:
        self.audiovault_mode = str(data.get("mode") or self.audiovault_mode)
        view = str(data.get("view") or "search")
        title = str(data.get("title") or self.t("search"))
        if view == "search":
            self.show_audiovault_search()
        else:
            self.show_audiovault_results_screen(title, view)
        self.audiovault_parent_results = list(data.get("parent_results") or [])
        self.audiovault_parent_view = str(data.get("parent_view") or "")
        self.audiovault_parent_title = str(data.get("parent_title") or "")
        self.show_audiovault_results(results)

    def prepare_audiovault_show(
        self,
        item: dict,
        download_after: bool = False,
        allow_auth_retry: bool = True,
    ) -> None:
        cache_dir = self.audiovault_show_cache_dir(item)
        episodes = self.audiovault_episode_items(cache_dir, item)
        if episodes and self.audiovault_show_cache_is_complete(cache_dir):
            if download_after:
                self.copy_audiovault_show_to_downloads(item, cache_dir)
            else:
                self.show_audiovault_episodes(item, episodes)
            return
        if download_after:
            self.start_audiovault_full_show_job(item, cache_dir, True, allow_auth_retry)
            return
        manifest_key = self.audiovault_show_manifest_key(item)
        cached_manifest = list(self.audiovault_show_manifests.get(manifest_key) or [])
        if cached_manifest:
            self.show_audiovault_episodes(item, cached_manifest)
            return
        if manifest_key in self.audiovault_manifest_loading:
            self.set_status(self.t("audiovault_loading_episodes", title=item.get("title", "")))
            return
        self.set_status(self.t("audiovault_loading_episodes", title=item.get("title", "")))
        self.audiovault_manifest_loading.add(manifest_key)
        self.audiovault_show_request_generation += 1
        generation = self.audiovault_show_request_generation
        threading.Thread(
            target=self.load_audiovault_show_manifest_worker,
            args=(dict(item), cache_dir, allow_auth_retry, generation),
            daemon=True,
        ).start()

    def audiovault_show_cache_is_complete(self, cache_dir: Path) -> bool:
        if not cache_dir.is_dir():
            return False
        complete_marker = cache_dir / ".apricot-complete"
        partial_marker = cache_dir / ".apricot-partial"
        return complete_marker.exists() or not partial_marker.exists()

    def load_audiovault_show_manifest_worker(
        self,
        item: dict,
        cache_dir: Path,
        allow_auth_retry: bool = True,
        generation: int = 0,
    ) -> None:
        manifest_key = self.audiovault_show_manifest_key(item)
        try:
            episodes = self.audiovault_remote_episode_items(item)
            if not episodes:
                raise ValueError(self.t("audiovault_no_episodes"))
            self.audiovault_show_manifests[manifest_key] = [dict(episode) for episode in episodes]
            wx.CallAfter(self.show_audiovault_episodes_if_current, generation, item, episodes)
        except AudioVaultRangeUnsupported:
            wx.CallAfter(
                self.start_audiovault_full_show_job,
                item,
                cache_dir,
                False,
                allow_auth_retry,
                generation,
            )
        except AudioVaultSessionExpired as exc:
            if allow_auth_retry:
                wx.CallAfter(
                    self.retry_audiovault_after_login,
                    lambda: self.prepare_audiovault_show(item, download_after=False, allow_auth_retry=False),
                )
            else:
                wx.CallAfter(self.message, self.t("audiovault_show_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("audiovault_show_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)
        finally:
            getattr(self, "audiovault_manifest_loading", set()).discard(manifest_key)

    def show_audiovault_episodes_if_current(self, generation: int, show: dict, episodes: list[dict]) -> None:
        if generation and generation != self.audiovault_show_request_generation:
            return
        if not getattr(self, "audiovault_screen_active", False):
            return
        self.show_audiovault_episodes(show, episodes)

    def start_audiovault_full_show_job(
        self,
        item: dict,
        cache_dir: Path,
        download_after: bool = False,
        allow_auth_retry: bool = True,
        generation: int = 0,
    ) -> None:
        if generation and (
            generation != self.audiovault_show_request_generation
            or not getattr(self, "audiovault_screen_active", False)
        ):
            return
        task_id = self.next_audiovault_progress_task_id()
        message = self.t("audiovault_progress_downloading", title=item.get("title", ""), percent=0)
        self.show_audiovault_progress(task_id, message)
        threading.Thread(
            target=self.audiovault_show_worker,
            args=(dict(item), cache_dir, download_after, allow_auth_retry, task_id, generation),
            daemon=True,
        ).start()

    def audiovault_show_cache_dir(self, item: dict) -> Path:
        safe_id = re.sub(r"[^0-9A-Za-z._-]", "_", str(item.get("id") or "show"))
        return Path(self.settings.cache_folder).expanduser() / "audiovault" / safe_id

    def audiovault_show_worker(
        self,
        item: dict,
        cache_dir: Path,
        download_after: bool = False,
        allow_auth_retry: bool = True,
        task_id: str = "",
        generation: int = 0,
    ) -> None:
        archive = cache_dir.with_suffix(".zip.part")
        try:
            cache_dir.parent.mkdir(parents=True, exist_ok=True)
            response, _url, content_type, disposition, _headers = self.resolve_audiovault_stream(item["url"])
            length = int(response.headers.get("Content-Length") or 0)
            if length > AUDIOVAULT_MAX_ARCHIVE:
                raise ValueError(self.t("audiovault_archive_too_large"))
            total = 0
            last_percent = -1
            try:
                with archive.open("wb") as target:
                    while True:
                        chunk = response.read(1024 * 1024)
                        if not chunk:
                            break
                        total += len(chunk)
                        if total > AUDIOVAULT_MAX_ARCHIVE:
                            raise ValueError(self.t("audiovault_archive_too_large"))
                        target.write(chunk)
                        percent = min(80, int((total * 80) / length)) if length > 0 else None
                        if percent is None or percent != last_percent:
                            last_percent = percent if percent is not None else last_percent
                            wx.CallAfter(
                                self.update_audiovault_progress,
                                task_id,
                                percent,
                                self.t("audiovault_progress_downloading", title=item.get("title", ""), percent=percent or 0),
                            )
            finally:
                response.close()
            if "zip" not in content_type and ".zip" not in disposition.lower() and not zipfile.is_zipfile(archive):
                raise ValueError(self.t("audiovault_show_format_error"))
            def extraction_progress(done: int, extraction_total: int) -> None:
                percent = 80 + (int((done * 20) / extraction_total) if extraction_total > 0 else 0)
                wx.CallAfter(
                    self.update_audiovault_progress,
                    task_id,
                    percent,
                    self.t("audiovault_progress_extracting", title=item.get("title", ""), percent=percent),
                )

            self.safe_extract_audiovault_zip(archive, cache_dir, progress=extraction_progress)
            (cache_dir / ".apricot-complete").write_text("complete\n", encoding="ascii")
            (cache_dir / ".apricot-partial").unlink(missing_ok=True)
            archive.unlink(missing_ok=True)
            episodes = self.audiovault_episode_items(cache_dir, item)
            if not episodes:
                raise ValueError(self.t("audiovault_no_episodes"))
            wx.CallAfter(self.close_audiovault_progress, task_id)
            if download_after:
                wx.CallAfter(self.copy_audiovault_show_to_downloads, item, cache_dir)
            else:
                wx.CallAfter(self.show_audiovault_episodes_if_current, generation, item, episodes)
        except AudioVaultSessionExpired as exc:
            archive.unlink(missing_ok=True)
            wx.CallAfter(self.close_audiovault_progress, task_id)
            if allow_auth_retry:
                wx.CallAfter(
                    self.retry_audiovault_after_login,
                    lambda: self.prepare_audiovault_show(
                        item,
                        download_after=download_after,
                        allow_auth_retry=False,
                    ),
                )
            else:
                wx.CallAfter(self.message, self.t("audiovault_show_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)
        except Exception as exc:
            archive.unlink(missing_ok=True)
            wx.CallAfter(self.close_audiovault_progress, task_id)
            wx.CallAfter(self.message, self.t("audiovault_show_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    @staticmethod
    def safe_extract_audiovault_zip(archive: Path, destination: Path, progress=None) -> None:
        temporary = destination.with_name(destination.name + ".extracting")
        shutil.rmtree(temporary, ignore_errors=True)
        temporary.mkdir(parents=True, exist_ok=True)
        try:
            with zipfile.ZipFile(archive) as package:
                infos = package.infolist()
                AudioVaultMixin.validate_audiovault_zip_infos(infos)
                total = sum(max(0, int(member.file_size)) for member in infos)
                extracted = 0
                for member in infos:
                    relative = Path(member.filename.replace("\\", "/"))
                    target = (temporary / relative).resolve()
                    if os.path.commonpath([str(temporary.resolve()), str(target)]) != str(temporary.resolve()):
                        raise ValueError("Unsafe path in AudioVault archive.")
                    if member.is_dir():
                        target.mkdir(parents=True, exist_ok=True)
                    else:
                        target.parent.mkdir(parents=True, exist_ok=True)
                        with package.open(member) as source, target.open("wb") as output:
                            while True:
                                chunk = source.read(1024 * 1024)
                                if not chunk:
                                    break
                                output.write(chunk)
                                extracted += len(chunk)
                                if progress:
                                    progress(extracted, total)
            shutil.rmtree(destination, ignore_errors=True)
            temporary.replace(destination)
        except Exception:
            shutil.rmtree(temporary, ignore_errors=True)
            raise

    def audiovault_episode_items(self, folder: Path, show: dict) -> list[dict]:
        if not folder.is_dir():
            return []
        paths = [path for path in folder.rglob("*") if path.is_file() and path.suffix.lower() in _AUDIO_EXTENSIONS]
        paths.sort(key=lambda path: self.natural_sort_key(str(path.relative_to(folder))))
        return [{
            "title": path.stem, "url": str(path), "path": str(path), "local_path": str(path),
            "kind": "audiovault_episode", "type": self.t("episode"), "channel": show.get("title", "AudioVault"),
            "audiovault_show": dict(show),
        } for path in paths]

    def prepare_audiovault_remote_episode(
        self,
        item: dict,
        download_after: bool = False,
        allow_auth_retry: bool = True,
        show_player: bool = True,
        announce_start: bool = False,
        focus_target: str = "player",
        keep_current_ui: bool = False,
    ) -> None:
        target = Path(str(item.get("url") or ""))
        expected_size = int(item.get("archive_file_size") or 0)
        if target.is_file() and (expected_size <= 0 or target.stat().st_size == expected_size):
            try:
                os.utime(target, None)
            except OSError:
                pass
            self.finish_audiovault_remote_episode(
                item,
                download_after,
                show_player,
                announce_start,
                focus_target,
                keep_current_ui,
            )
            return
        target.unlink(missing_ok=True)
        loading_key = str(target)
        if loading_key in self.audiovault_episode_loading:
            self.set_status(self.t("audiovault_episode_preparing", title=item.get("title", "")))
            return
        self.audiovault_episode_loading.add(loading_key)
        task_id = ""
        if download_after:
            task_id = self.next_audiovault_progress_task_id()
            message = self.t("audiovault_progress_downloading", title=item.get("title", ""), percent=0)
            self.show_audiovault_progress(task_id, message)
        else:
            self.set_status(self.t("audiovault_episode_cache_notice", title=item.get("title", "")))
        threading.Thread(
            target=self.audiovault_remote_episode_worker,
            args=(
                dict(item),
                download_after,
                allow_auth_retry,
                task_id,
                show_player,
                announce_start,
                focus_target,
                keep_current_ui,
            ),
            daemon=True,
        ).start()

    def audiovault_remote_episode_worker(
        self,
        item: dict,
        download_after: bool = False,
        allow_auth_retry: bool = True,
        task_id: str = "",
        show_player: bool = True,
        announce_start: bool = False,
        focus_target: str = "player",
        keep_current_ui: bool = False,
    ) -> None:
        target = Path(str(item.get("url") or ""))
        temporary = target.with_name(target.name + ".part")
        try:
            archive_url = str(item.get("archive_url") or item.get("webpage_url") or "")
            archive_size = self.audiovault_archive_size(archive_url)
            reader = _AudioVaultRangeReader(
                archive_size,
                lambda start, end: self.audiovault_read_range(archive_url, start, end),
            )
            with zipfile.ZipFile(reader) as package:
                infos = package.infolist()
                self.validate_audiovault_zip_infos(infos)
                member = package.getinfo(str(item.get("archive_member") or ""))
                expected_crc = int(item.get("archive_crc") or 0)
                if expected_crc and int(member.CRC) != expected_crc:
                    raise ValueError(self.t("audiovault_show_format_error"))
                target.parent.mkdir(parents=True, exist_ok=True)
                show = dict(item.get("audiovault_show") or {})
                cache_dir = self.audiovault_show_cache_dir(show)
                cache_dir.mkdir(parents=True, exist_ok=True)
                (cache_dir / ".apricot-partial").write_text("partial\n", encoding="ascii")
                temporary.unlink(missing_ok=True)
                completed = 0
                last_status_percent = -1
                with package.open(member) as source, temporary.open("wb") as output:
                    while True:
                        chunk = source.read(1024 * 1024)
                        if not chunk:
                            break
                        output.write(chunk)
                        completed += len(chunk)
                        percent = int((completed * 100) / member.file_size) if member.file_size > 0 else None
                        if task_id:
                            wx.CallAfter(
                                self.update_audiovault_progress,
                                task_id,
                                percent,
                                self.t(
                                    "audiovault_progress_downloading",
                                    title=item.get("title", ""),
                                    percent=percent or 0,
                                ),
                            )
                        else:
                            status_percent = min(100, max(0, int(percent or 0)))
                            status_percent = (status_percent // 5) * 5
                            if status_percent != last_status_percent:
                                last_status_percent = status_percent
                                wx.CallAfter(
                                    self.set_status,
                                    self.t(
                                        "audiovault_episode_cache_progress",
                                        title=item.get("title", ""),
                                        percent=status_percent,
                                    ),
                                )
            temporary.replace(target)
            protected = {target}
            current_path = str((getattr(self, "current_video_item", {}) or {}).get("local_path") or "")
            if current_path:
                protected.add(Path(current_path))
            self.trim_audiovault_episode_cache(protected)
            if task_id:
                wx.CallAfter(self.close_audiovault_progress, task_id)
            wx.CallAfter(
                self.finish_audiovault_remote_episode,
                item,
                download_after,
                show_player,
                announce_start,
                focus_target,
                keep_current_ui,
            )
        except AudioVaultSessionExpired as exc:
            temporary.unlink(missing_ok=True)
            if task_id:
                wx.CallAfter(self.close_audiovault_progress, task_id)
            if allow_auth_retry:
                wx.CallAfter(
                    self.retry_audiovault_after_login,
                    lambda: self.prepare_audiovault_remote_episode(
                        item,
                        download_after=download_after,
                        allow_auth_retry=False,
                        show_player=show_player,
                        announce_start=announce_start,
                        focus_target=focus_target,
                        keep_current_ui=keep_current_ui,
                    ),
                )
            else:
                key = "download_failed" if download_after else "player_failed"
                wx.CallAfter(self.message, self.t(key, error=self.friendly_error(exc)), wx.ICON_ERROR)
        except Exception as exc:
            temporary.unlink(missing_ok=True)
            if task_id:
                wx.CallAfter(self.close_audiovault_progress, task_id)
            key = "download_failed" if download_after else "player_failed"
            wx.CallAfter(self.message, self.t(key, error=self.friendly_error(exc)), wx.ICON_ERROR)
        finally:
            getattr(self, "audiovault_episode_loading", set()).discard(str(target))

    def finish_audiovault_remote_episode(
        self,
        item: dict,
        download_after: bool = False,
        show_player: bool = True,
        announce_start: bool = False,
        focus_target: str = "player",
        keep_current_ui: bool = False,
    ) -> None:
        playable = dict(item)
        playable["kind"] = "audiovault_episode"
        playable["path"] = str(item.get("url") or "")
        playable["local_path"] = str(item.get("url") or "")
        if download_after:
            self.copy_audiovault_episode_to_downloads(playable)
        else:
            self.play_audiovault_local_item(
                playable,
                show_player=show_player,
                announce_start=announce_start,
                focus_target=focus_target,
                keep_current_ui=keep_current_ui,
            )

    def show_audiovault_episodes(self, show: dict, episodes: list[dict]) -> None:
        self.audiovault_parent_results = list(self.audiovault_results)
        self.audiovault_parent_view = self.audiovault_view
        self.audiovault_parent_title = self.audiovault_results_title
        self.audiovault_view = "episodes"
        self.audiovault_results_title = str(show.get("title") or self.t("tv_show"))
        self.audiovault_results = list(episodes)
        self.results = list(episodes)
        self.all_results = list(episodes)
        self.set_listbox_items(self.results_list, [self.result_line(index, item) for index, item in enumerate(episodes)], 0)
        self.set_status(self.t("audiovault_episodes_loaded", count=len(episodes), title=show.get("title", "")))
        self.focus_later(self.results_list)

    def play_audiovault_local_item(
        self,
        item: dict,
        show_player: bool = True,
        announce_start: bool = False,
        focus_target: str = "player",
        keep_current_ui: bool = False,
    ) -> None:
        self.current_video_item = dict(item)
        self.current_video_info = dict(item)
        self.player_return_screen = "audiovault"
        self.player_return_data = self.audiovault_player_return_state()
        self.set_player_sequence(self.audiovault_results)
        self.play_url(
            item["url"],
            item.get("title", ""),
            show_player=show_player,
            announce_start=announce_start,
            focus_target=focus_target,
            keep_current_ui=keep_current_ui,
        )

    def copy_audiovault_episode_to_downloads(self, item: dict) -> None:
        target = (
            Path(self.settings.download_folder).expanduser()
            / "AudioVault"
            / self.safe_folder_name(str(item.get("channel") or "TV Shows"))
            / Path(str(item.get("url") or "episode.mp3")).name
        )
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(str(item.get("url") or ""), target)
        self.announce_player(
            self.t("audiovault_download_complete_path", title=item.get("title", ""), path=str(target))
        )

    def copy_audiovault_show_to_downloads(self, item: dict, cache_dir: Path) -> None:
        target = Path(self.settings.download_folder).expanduser() / "AudioVault" / self.safe_folder_name(str(item.get("title") or "TV Show"))
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists():
            shutil.rmtree(target)
        shutil.copytree(cache_dir, target)
        (target / ".apricot-complete").unlink(missing_ok=True)
        (target / ".apricot-partial").unlink(missing_ok=True)
        self.announce_player(
            self.t("audiovault_download_complete_path", title=item.get("title", ""), path=str(target))
        )

    def download_audiovault_selected(self) -> None:
        item = self.selected_audiovault_item()
        if item:
            self.download_audiovault_item(item)

    def download_audiovault_item(self, item: dict, allow_auth_retry: bool = True) -> None:
        default = Path(self.settings.download_folder).expanduser() / "AudioVault"
        default.mkdir(parents=True, exist_ok=True)
        if item.get("kind") == "audiovault_show":
            self.prepare_audiovault_show(item, download_after=True, allow_auth_retry=allow_auth_retry)
            self.announce_player(self.t("audiovault_show_cache_notice"))
            return
        if item.get("kind") == "audiovault_remote_episode":
            self.prepare_audiovault_remote_episode(
                item,
                download_after=True,
                allow_auth_retry=allow_auth_retry,
            )
            return
        if item.get("kind") == "audiovault_episode":
            self.copy_audiovault_episode_to_downloads(item)
            return
        threading.Thread(
            target=self.download_audiovault_movie_worker,
            args=(dict(item), default, allow_auth_retry),
            daemon=True,
        ).start()

    def download_audiovault_movie_worker(self, item: dict, folder: Path, allow_auth_retry: bool = True) -> None:
        try:
            response, _url, _content_type, disposition, _headers = self.resolve_audiovault_stream(item["url"])
            filename_match = re.search(r"filename\*?=(?:UTF-8''|\")?([^\";]+)", disposition, re.I)
            filename = urllib.parse.unquote(filename_match.group(1).strip()) if filename_match else f"{self.safe_folder_name(item.get('title', 'AudioVault'))}.mp3"
            target = folder / Path(filename).name
            with target.open("wb") as output:
                shutil.copyfileobj(response, output, 1024 * 1024)
            response.close()
            wx.CallAfter(self.announce_player, self.t("download_complete", title=item.get("title", "")))
        except AudioVaultSessionExpired as exc:
            if allow_auth_retry:
                wx.CallAfter(
                    self.retry_audiovault_after_login,
                    lambda: self.download_audiovault_item(item, allow_auth_retry=False),
                )
            else:
                wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def open_audiovault_registration(self) -> None:
        webbrowser.open(AUDIOVAULT_REGISTER_URL)

    def login_audiovault_from_settings(self) -> None:
        self.apply_settings_from_visible_controls()
        self.show_audiovault_login()

    def logout_audiovault(self) -> None:
        self.audiovault_cookie_jar.clear()
        self.audiovault_logged_in = False
        self.settings.audiovault_email = ""
        self.settings.audiovault_password_protected = ""
        self.save_settings()
        self.set_status(self.t("audiovault_logged_out"))
