from __future__ import annotations

import html
import http.cookiejar
import os
import re
import shutil
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
_AUDIO_EXTENSIONS = {".aac", ".flac", ".m4a", ".m4b", ".mp3", ".ogg", ".opus", ".wav", ".wma"}


class _VaultPageParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.rows: list[list[str]] = []
        self.links: list[str] = []
        self.token = ""
        self._row: list[str] | None = None
        self._cell: list[str] | None = None

    def handle_starttag(self, tag: str, attrs) -> None:
        values = dict(attrs)
        if tag == "tr":
            self._row = []
        elif tag == "td" and self._row is not None:
            self._cell = []
        elif tag == "a" and values.get("href"):
            self.links.append(values["href"])
        elif tag == "input" and values.get("name") == "_token":
            self.token = values.get("value", "")

    def handle_data(self, data: str) -> None:
        if self._cell is not None:
            self._cell.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag == "td" and self._row is not None and self._cell is not None:
            self._row.append(" ".join("".join(self._cell).split()))
            self._cell = None
        elif tag == "tr" and self._row is not None:
            if self._row:
                self.rows.append(self._row)
            self._row = None


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

    def audiovault_request(self, url: str, data: dict | None = None, timeout: int = 30):
        parsed = urllib.parse.urlparse(url)
        if parsed.scheme != "https" or parsed.hostname not in {"direct.audiovault.net", "www.audiovault.net", "audiovault.net"}:
            raise ValueError(self.t("audiovault_untrusted_url"))
        payload = urllib.parse.urlencode(data).encode("utf-8") if data is not None else None
        request = urllib.request.Request(
            url,
            data=payload,
            headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}", "Accept": "text/html,application/octet-stream"},
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
        if dialog.ShowModal() != wx.ID_OK:
            dialog.Destroy()
            return False
        address, secret = email.GetValue().strip(), password.GetValue()
        dialog.Destroy()
        if not address or not secret:
            self.message(self.t("audiovault_credentials_required"), wx.ICON_ERROR)
            return False
        self.set_status(self.t("audiovault_logging_in"))
        threading.Thread(target=self.audiovault_login_worker, args=(address, secret, after_login), daemon=True).start()
        return True

    def audiovault_login_worker(self, email: str, password: str, after_login=None) -> None:
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
            self.save_settings()
            wx.CallAfter(self.set_status, self.t("audiovault_logged_in"))
            if after_login:
                wx.CallAfter(after_login)
        except Exception as exc:
            self.audiovault_logged_in = False
            wx.CallAfter(self.message, self.t("audiovault_login_error", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def open_audiovault_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_audiovault_search)

    def show_audiovault_search(self) -> None:
        if not self.audiovault_logged_in:
            self.show_audiovault_login(self.show_audiovault_search)
            return
        self.last_activated_menu_action = self.show_audiovault_search
        self.in_main_menu = False
        self.in_player_screen = False
        self.search_screen_active = False
        self.audiovault_screen_active = True
        self.clear()
        self.add_background_player_section()
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
        grid.Add(self.audiovault_type, 1, wx.EXPAND)
        self.root_sizer.Add(grid, 0, wx.EXPAND | wx.ALL, 4)
        self.add_button_row([
            (self.t("search"), self.search_audiovault),
            (self.t("play"), self.activate_audiovault_item),
            (self.t("download_audio"), self.download_audiovault_selected),
        ])
        self.results_list = wx.ListBox(self.panel, choices=[self.t("search_results_empty")])
        self.results_list.SetName(self.t("results"))
        self.results_list.SetSelection(0)
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.activate_audiovault_item())
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_audiovault_results_key)
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_audiovault_context_menu)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        self.focus_later(self.audiovault_query)

    def back_from_audiovault(self) -> None:
        if self.audiovault_parent_results:
            results = list(self.audiovault_parent_results)
            self.audiovault_parent_results = []
            self.show_audiovault_results(results)
            return
        self.show_main_menu()

    def search_audiovault(self) -> None:
        query = self.audiovault_query.GetValue().strip()
        if not query:
            self.message(self.t("enter_query"))
            return
        self.audiovault_mode = "movies" if self.audiovault_type.GetSelection() == 0 else "shows"
        self.set_status(self.t("searching", query=query))
        threading.Thread(target=self.search_audiovault_worker, args=(query, self.audiovault_mode), daemon=True).start()

    def search_audiovault_worker(self, query: str, mode: str) -> None:
        try:
            url = f"{AUDIOVAULT_BASE_URL}/{mode}?{urllib.parse.urlencode({'search': query})}"
            with self.audiovault_request(url) as response:
                parser = _VaultPageParser()
                parser.feed(self.audiovault_read_page(response))
            links = [urllib.parse.urljoin(AUDIOVAULT_BASE_URL, link) for link in parser.links if re.search(r"/download/\d+", link)]
            results = []
            for row, link in zip((row for row in parser.rows if len(row) >= 2), links):
                results.append({
                    "id": row[0], "title": html.unescape(row[1]), "url": link,
                    "webpage_url": link, "kind": "audiovault_show" if mode == "shows" else "audiovault_movie",
                    "type": self.t("tv_show") if mode == "shows" else self.t("movie"), "channel": "AudioVault",
                })
            wx.CallAfter(self.show_audiovault_results, results)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("audiovault_search_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def show_audiovault_results(self, results: list[dict]) -> None:
        self.audiovault_results = list(results)
        self.results = list(results)
        self.all_results = list(results)
        labels = [self.result_line(item) for item in results] or [self.t("no_results")]
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
        elif item.get("kind") == "audiovault_episode":
            self.play_audiovault_local_item(item)
        else:
            self.play_audiovault_remote_item(item)

    def audiovault_cookie_header(self) -> str:
        return "; ".join(f"{cookie.name}={cookie.value}" for cookie in self.audiovault_cookie_jar)

    def resolve_audiovault_stream(self, url: str):
        response = self.audiovault_request(url, timeout=60)
        final_url = response.geturl()
        content_type = str(response.headers.get("Content-Type") or "").lower()
        disposition = str(response.headers.get("Content-Disposition") or "")
        if "text/html" in content_type or urllib.parse.urlparse(final_url).path == "/login":
            response.close()
            self.audiovault_logged_in = False
            raise PermissionError(self.t("audiovault_session_expired"))
        headers = {"User-Agent": f"{APP_NAME}/{APP_VERSION}", "Referer": AUDIOVAULT_BASE_URL}
        cookie = self.audiovault_cookie_header()
        if cookie:
            headers["Cookie"] = cookie
        return response, final_url, content_type, disposition, headers

    def play_audiovault_remote_item(self, item: dict) -> None:
        self.set_status(self.t("preparing_stream", title=item.get("title", "")))
        threading.Thread(target=self.play_audiovault_remote_worker, args=(dict(item),), daemon=True).start()

    def play_audiovault_remote_worker(self, item: dict) -> None:
        try:
            response, final_url, content_type, disposition, headers = self.resolve_audiovault_stream(item["url"])
            response.close()
            if "zip" in content_type or ".zip" in disposition.lower():
                wx.CallAfter(self.prepare_audiovault_show, item, False)
                return
            item.update({"stream_url": final_url, "http_headers": headers})
            wx.CallAfter(self.start_audiovault_player, item, final_url, headers)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("player_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def start_audiovault_player(self, item: dict, stream_url: str, headers: dict) -> None:
        self.current_video_item = dict(item)
        self.current_video_info = dict(item)
        self.current_video_item["_audiovault_stream_url"] = stream_url
        self.current_video_item["_audiovault_headers"] = headers
        self.current_video_info.update(self.current_video_item)
        self.player_return_screen = "audiovault"
        self.player_return_data = {"mode": self.audiovault_mode, "results": list(self.audiovault_results)}
        self.play_url(item["url"], item.get("title", ""))

    def prepare_audiovault_show(self, item: dict, download_after: bool = False) -> None:
        cache_dir = self.audiovault_show_cache_dir(item)
        episodes = self.audiovault_episode_items(cache_dir, item)
        if episodes:
            if download_after:
                self.copy_audiovault_show_to_downloads(item, cache_dir)
            else:
                self.show_audiovault_episodes(item, episodes)
            return
        self.set_status(self.t("audiovault_preparing_show", title=item.get("title", "")))
        threading.Thread(target=self.audiovault_show_worker, args=(dict(item), cache_dir, download_after), daemon=True).start()

    def audiovault_show_cache_dir(self, item: dict) -> Path:
        safe_id = re.sub(r"[^0-9A-Za-z._-]", "_", str(item.get("id") or "show"))
        return Path(self.settings.cache_folder).expanduser() / "audiovault" / safe_id

    def audiovault_show_worker(self, item: dict, cache_dir: Path, download_after: bool = False) -> None:
        archive = cache_dir.with_suffix(".zip.part")
        try:
            cache_dir.parent.mkdir(parents=True, exist_ok=True)
            response, _url, content_type, disposition, _headers = self.resolve_audiovault_stream(item["url"])
            length = int(response.headers.get("Content-Length") or 0)
            if length > AUDIOVAULT_MAX_ARCHIVE:
                raise ValueError(self.t("audiovault_archive_too_large"))
            total = 0
            with archive.open("wb") as target:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    total += len(chunk)
                    if total > AUDIOVAULT_MAX_ARCHIVE:
                        raise ValueError(self.t("audiovault_archive_too_large"))
                    target.write(chunk)
            response.close()
            if "zip" not in content_type and ".zip" not in disposition.lower() and not zipfile.is_zipfile(archive):
                raise ValueError(self.t("audiovault_show_format_error"))
            self.safe_extract_audiovault_zip(archive, cache_dir)
            archive.unlink(missing_ok=True)
            episodes = self.audiovault_episode_items(cache_dir, item)
            if not episodes:
                raise ValueError(self.t("audiovault_no_episodes"))
            if download_after:
                wx.CallAfter(self.copy_audiovault_show_to_downloads, item, cache_dir)
            else:
                wx.CallAfter(self.show_audiovault_episodes, item, episodes)
        except Exception as exc:
            archive.unlink(missing_ok=True)
            wx.CallAfter(self.message, self.t("audiovault_show_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    @staticmethod
    def safe_extract_audiovault_zip(archive: Path, destination: Path) -> None:
        temporary = destination.with_name(destination.name + ".extracting")
        shutil.rmtree(temporary, ignore_errors=True)
        temporary.mkdir(parents=True, exist_ok=True)
        total = 0
        try:
            with zipfile.ZipFile(archive) as package:
                for member in package.infolist():
                    total += max(0, member.file_size)
                    if total > AUDIOVAULT_MAX_EXTRACTED:
                        raise ValueError("AudioVault archive expands beyond the safety limit.")
                    relative = Path(member.filename.replace("\\", "/"))
                    if relative.is_absolute() or ".." in relative.parts:
                        raise ValueError("Unsafe path in AudioVault archive.")
                    target = (temporary / relative).resolve()
                    if os.path.commonpath([str(temporary.resolve()), str(target)]) != str(temporary.resolve()):
                        raise ValueError("Unsafe path in AudioVault archive.")
                    if member.is_dir():
                        target.mkdir(parents=True, exist_ok=True)
                    else:
                        target.parent.mkdir(parents=True, exist_ok=True)
                        with package.open(member) as source, target.open("wb") as output:
                            shutil.copyfileobj(source, output, 1024 * 1024)
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

    def show_audiovault_episodes(self, show: dict, episodes: list[dict]) -> None:
        self.audiovault_parent_results = list(self.audiovault_results)
        self.audiovault_results = list(episodes)
        self.results = list(episodes)
        self.all_results = list(episodes)
        self.set_listbox_items(self.results_list, [self.result_line(item) for item in episodes], 0)
        self.set_status(self.t("audiovault_episodes_loaded", count=len(episodes), title=show.get("title", "")))
        self.focus_later(self.results_list)

    def play_audiovault_local_item(self, item: dict) -> None:
        self.current_video_item = dict(item)
        self.current_video_info = dict(item)
        self.player_return_screen = "audiovault"
        self.player_return_data = {"mode": self.audiovault_mode, "results": list(self.audiovault_results)}
        self.play_url(item["url"], item.get("title", ""))
        self.set_player_sequence(self.audiovault_results)

    def copy_audiovault_show_to_downloads(self, item: dict, cache_dir: Path) -> None:
        target = Path(self.settings.download_folder).expanduser() / "AudioVault" / self.safe_folder_name(str(item.get("title") or "TV Show"))
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists():
            shutil.rmtree(target)
        shutil.copytree(cache_dir, target)
        self.announce_player(self.t("download_complete", title=item.get("title", "")))

    def download_audiovault_selected(self) -> None:
        item = self.selected_audiovault_item()
        if item:
            self.download_audiovault_item(item)

    def download_audiovault_item(self, item: dict) -> None:
        default = Path(self.settings.download_folder).expanduser() / "AudioVault"
        default.mkdir(parents=True, exist_ok=True)
        if item.get("kind") == "audiovault_show":
            self.prepare_audiovault_show(item, download_after=True)
            self.announce_player(self.t("audiovault_show_cache_notice"))
            return
        if item.get("kind") == "audiovault_episode":
            target = default / self.safe_folder_name(str(item.get("channel") or "TV Shows")) / Path(item["url"]).name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(item["url"], target)
            self.announce_player(self.t("download_complete", title=item.get("title", "")))
            return
        threading.Thread(target=self.download_audiovault_movie_worker, args=(dict(item), default), daemon=True).start()

    def download_audiovault_movie_worker(self, item: dict, folder: Path) -> None:
        try:
            response, _url, _content_type, disposition, _headers = self.resolve_audiovault_stream(item["url"])
            filename_match = re.search(r"filename\*?=(?:UTF-8''|\")?([^\";]+)", disposition, re.I)
            filename = urllib.parse.unquote(filename_match.group(1).strip()) if filename_match else f"{self.safe_folder_name(item.get('title', 'AudioVault'))}.mp3"
            target = folder / Path(filename).name
            with target.open("wb") as output:
                shutil.copyfileobj(response, output, 1024 * 1024)
            response.close()
            wx.CallAfter(self.announce_player, self.t("download_complete", title=item.get("title", "")))
        except Exception as exc:
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def open_audiovault_registration(self) -> None:
        webbrowser.open(AUDIOVAULT_REGISTER_URL)

    def logout_audiovault(self) -> None:
        self.init_audiovault()
        self.set_status(self.t("audiovault_logged_out"))
