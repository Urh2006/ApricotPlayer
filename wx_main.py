from __future__ import annotations

import json
import os
import queue
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import webbrowser
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlencode
from urllib.request import Request, urlopen
from urllib.error import HTTPError, URLError

import wx
import wx.richtext as richtext

try:
    import yt_dlp
except ImportError:
    yt_dlp = None


APP_NAME = "ApricotPlayer"
APP_VERSION = "0.1.0-beta.2"
APP_VERSION_LABEL = "0.1 beta 2"
WINDOW_TITLE = f"{APP_NAME} {APP_VERSION_LABEL}"
LEGACY_APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "UrhasaurusYouTubePlayer"
APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "ApricotPlayer"
SETTINGS_FILE = APP_DIR / "settings.json"
FAVORITES_FILE = APP_DIR / "favorites.json"
LEGACY_SETTINGS_FILE = LEGACY_APP_DIR / "settings.json"
LEGACY_FAVORITES_FILE = LEGACY_APP_DIR / "favorites.json"
DEFAULT_FILENAME_TEMPLATE = "%(title)s.%(ext)s"
OLD_FILENAME_TEMPLATE = "%(title)s [%(id)s].%(ext)s"
RESULTS_PAGE_SIZE = 20
DEFAULT_GITHUB_OWNER = "Urh2006"
DEFAULT_GITHUB_REPO = "ApricotPlayer"
UPDATE_ASSET_NAME = "ApricotPlayer.exe"


TEXT = {
    "sl": {
        "ready": "Pripravljen.",
        "main_menu": "Glavni meni",
        "search_youtube": "Iskanje po YouTube",
        "choose_download_folder": "Izbor mape za prenose",
        "favorites": "Priljubljeni",
        "settings": "Nastavitve",
        "exit": "Izhod",
        "open": "Odpri",
        "back": "Nazaj v glavni meni",
        "back_results": "Nazaj na rezultate",
        "internal_player": "Predvajalnik",
        "player_missing": "Notranji predvajalnik mpv ni najden. Program ne bo odpiral YouTube strani.",
        "player_announcement": "Obvestilo predvajalnika",
        "video_details": "Podrobnosti videa",
        "details_button": "View video details",
        "timing_unavailable": "Timing is not available yet.",
        "time_announcement": "Elapsed {elapsed}, remaining {remaining}, total {total}.",
        "speed_announcement": "Playback speed {speed}x.",
        "pitch_announcement": "Pitch {pitch}x.",
        "details_unavailable": "Video details are not available yet.",
        "version": "Verzija",
        "description": "Description",
        "url": "URL",
        "uploaded": "uploaded",
        "dynamic_results": "0 (dinamično, po 20)",
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
        "auto_update_app": "Ob zagonu preveri posodobitve programa na GitHubu",
        "github_owner": "GitHub owner",
        "github_repo": "GitHub repo",
        "github_token": "GitHub token za private updates",
        "checking_app_updates": "Preverjam posodobitve programa.",
        "app_up_to_date": "Program je posodobljen.",
        "app_update_available": "Na voljo je nova verzija {version}. Želiš prenos in namestitev zdaj?",
        "app_update_disabled": "Samodejne posodobitve programa so izključene.",
        "app_update_failed": "Posodobitve programa ni bilo mogoče preveriti: {error}",
        "downloading_update": "Prenašam posodobitev {version}.",
        "installing_update": "Nameščam posodobitev {version}.",
        "update_ready_restart": "Posodobitev je pripravljena. Program se bo zaprl in znova zagnal.",
        "update_source_only": "Samodejna namestitev deluje samo v .exe verziji. Na voljo je nova izdaja: {version}",
        "pitch_label": "Pitch",
        "whats_new": "Kaj je novega",
        "update_now": "Želiš posodobiti zdaj?",
        "no_changelog": "Za to izdajo ni opisa sprememb.",
        "search_query": "Iskalni niz",
        "type": "Vrsta",
        "search": "Search",
        "video": "Video",
        "playlist": "Playlist",
        "channel": "Kanal",
        "views": "Ogledi",
        "play": "Play",
        "download_audio": "Download audio",
        "download_video": "Download video",
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
        "auto_update": "Ob vsakem zagonu preveri posodobitve yt-dlp",
        "autoplay_next": "Po koncu posnetka samodejno predvajaj naslednjega",
        "confirm_download": "Pred prenosom vprašaj za potrditev",
        "open_after_download": "Po prenosu odpri mapo za prenose",
        "audio_format": "Audio format",
        "audio_quality": "Audio kvaliteta (0 najboljše)",
        "video_format": "Video format yt-dlp",
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
        "no_player": "Player ni najden, odprl sem brskalnik.",
        "player_failed": "Player se ni zagnal, odprt je brskalnik: {error}",
        "stopped": "Predvajanje ustavljeno.",
        "queued": "V čakalni vrsti",
        "done": "Končano",
        "download_confirm": "Prenesem {action}: {title}?",
        "download_cancelled": "Prenos preklican.",
        "download_done": "Prenos končan: {title}",
        "download_failed": "Prenos ni uspel: {error}",
        "favorite_added": "Dodano med priljubljene.",
        "favorite_exists": "Ta element je že med priljubljenimi.",
        "favorite_removed": "Odstranjeno iz priljubljenih.",
        "settings_saved": "Nastavitve shranjene.",
        "checking_updates": "Preverjam posodobitve za YouTube podporo.",
        "updates_ok": "YouTube podpora je posodobljena.",
        "updates_failed": "Posodobitve YouTube podpore ni bilo mogoče preveriti: {error}",
        "missing_ytdlp": "Manjka yt-dlp.",
    },
    "en": {
        "ready": "Ready.",
        "main_menu": "Main menu",
        "search_youtube": "Search YouTube",
        "choose_download_folder": "Choose download folder",
        "favorites": "Favorites",
        "settings": "Settings",
        "exit": "Exit",
        "open": "Open",
        "back": "Back to main menu",
        "back_results": "Back to results",
        "internal_player": "Player",
        "player_missing": "Internal mpv player was not found. The program will not open the YouTube page.",
        "player_announcement": "Player announcement",
        "video_details": "Video details",
        "details_button": "View video details",
        "timing_unavailable": "Timing is not available yet.",
        "time_announcement": "Elapsed {elapsed}, remaining {remaining}, total {total}.",
        "speed_announcement": "Playback speed {speed}x.",
        "pitch_announcement": "Pitch {pitch}x.",
        "details_unavailable": "Video details are not available yet.",
        "version": "Version",
        "description": "Description",
        "url": "URL",
        "uploaded": "uploaded",
        "dynamic_results": "0 (dynamic, by 20)",
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
        "auto_update_app": "Check app updates on startup from GitHub",
        "github_owner": "GitHub owner",
        "github_repo": "GitHub repo",
        "github_token": "GitHub token for private updates",
        "checking_app_updates": "Checking app updates.",
        "app_up_to_date": "The app is up to date.",
        "app_update_available": "Version {version} is available. Download and install now?",
        "app_update_disabled": "Automatic app updates are disabled.",
        "app_update_failed": "Could not check app updates: {error}",
        "downloading_update": "Downloading update {version}.",
        "installing_update": "Installing update {version}.",
        "update_ready_restart": "The update is ready. The app will close and restart.",
        "update_source_only": "Automatic install works only in the .exe build. New release available: {version}",
        "pitch_label": "Pitch",
        "whats_new": "What's new",
        "update_now": "Would you like to update now?",
        "no_changelog": "No changelog was provided for this release.",
        "search_query": "Search query",
        "type": "Type",
        "search": "Search",
        "video": "Video",
        "playlist": "Playlist",
        "channel": "Channel",
        "views": "Views",
        "play": "Play",
        "download_audio": "Download audio",
        "download_video": "Download video",
        "add_favorite": "Add to favorites",
        "open_browser": "Open in browser",
        "copy_url": "Copy URL",
        "remove": "Remove",
        "refresh": "Refresh list",
        "download_folder": "Download folder",
        "browse": "Browse",
        "save": "Save",
        "language": "Language",
        "results_limit": "Number of results",
        "seek_seconds": "Seek seconds",
        "volume_step": "Volume step",
        "auto_update": "Check yt-dlp updates on every startup",
        "autoplay_next": "Automatically play next item",
        "confirm_download": "Ask before starting a download",
        "open_after_download": "Open download folder after download",
        "audio_format": "Audio format",
        "audio_quality": "Audio quality (0 is best)",
        "video_format": "Video format yt-dlp",
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
        "no_player": "Player not found, opened browser.",
        "player_failed": "Player did not start, opened browser: {error}",
        "stopped": "Playback stopped.",
        "queued": "Queued",
        "done": "Done",
        "download_confirm": "Download {action}: {title}?",
        "download_cancelled": "Download cancelled.",
        "download_done": "Download finished: {title}",
        "download_failed": "Download failed: {error}",
        "favorite_added": "Added to favorites.",
        "favorite_exists": "This item is already in favorites.",
        "favorite_removed": "Removed from favorites.",
        "settings_saved": "Settings saved.",
        "checking_updates": "Checking updates for YouTube support.",
        "updates_ok": "YouTube support is up to date.",
        "updates_failed": "Could not check YouTube support updates: {error}",
        "missing_ytdlp": "yt-dlp is missing.",
    },
}


@dataclass
class Settings:
    language: str = "sl"
    download_folder: str = str(Path.home() / "Downloads")
    results_limit: int = 20
    audio_format: str = "mp3"
    video_format: str = "bestvideo+bestaudio/best"
    max_video_height: int = 1080
    player_command: str = ""
    autoplay_next: bool = False
    prefer_browser_playback: bool = False
    player_fullscreen: bool = False
    player_start_paused: bool = False
    player_speed: str = "1.0"
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
    auto_update_ytdlp: bool = True
    auto_update_app: bool = True
    confirm_before_download: bool = False
    download_archive: bool = False
    rate_limit: str = ""
    proxy: str = ""
    cookies_file: str = ""
    ffmpeg_location: str = ""
    github_owner: str = DEFAULT_GITHUB_OWNER
    github_repo: str = DEFAULT_GITHUB_REPO
    github_token: str = ""
    concurrent_fragments: int = 4
    retries: int = 10
    socket_timeout: int = 20


class MainFrame(wx.Frame):
    def __init__(self) -> None:
        super().__init__(None, title=WINDOW_TITLE, size=(950, 680))
        APP_DIR.mkdir(parents=True, exist_ok=True)
        self.settings = self.load_settings()
        self.favorites = self.load_favorites()
        self.results: list[dict] = []
        self.all_results: list[dict] = []
        self.return_results: list[dict] = []
        self.return_all_results: list[dict] = []
        self.return_index = 0
        self.return_visible_count = 0
        self.last_search_query = ""
        self.last_search_type_index = 0
        self.last_visible_count = 0
        self.current_index = -1
        self.player_process: subprocess.Popen | None = None
        self.player_log_handle = None
        self.player_kind = ""
        self.player_control_mode = False
        self.in_player_screen = False
        self.current_video_item: dict | None = None
        self.current_video_info: dict = {}
        self.ipc_path: str | None = None
        self.ui_queue: queue.Queue[tuple[str, object]] = queue.Queue()

        self.panel = wx.Panel(self)
        self.root_sizer = wx.BoxSizer(wx.VERTICAL)
        self.panel.SetSizer(self.root_sizer)
        self.status = self.CreateStatusBar()
        self.status.SetStatusText(self.t("ready"))

        self.Bind(wx.EVT_CHAR_HOOK, self.on_char_hook)
        self.show_main_menu()
        self.timer = wx.Timer(self)
        self.Bind(wx.EVT_TIMER, self.process_queue, self.timer)
        self.timer.Start(100)
        if self.settings.auto_update_ytdlp:
            wx.CallLater(300, self.start_ytdlp_update_check)
        if self.settings.auto_update_app:
            wx.CallLater(900, self.start_app_update_check)

    def t(self, key: str, **kwargs) -> str:
        language = self.settings.language if self.settings.language in TEXT else "sl"
        text = TEXT[language].get(key, TEXT["sl"].get(key, key))
        return text.format(**kwargs) if kwargs else text

    def clear(self) -> None:
        self.root_sizer.Clear(delete_windows=True)

    def add_button_row(self, buttons: list[tuple[str, callable]]) -> None:
        row = wx.BoxSizer(wx.HORIZONTAL)
        for label, handler in buttons:
            button = wx.Button(self.panel, label=label)
            button.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            row.Add(button, 0, wx.RIGHT, 6)
        self.root_sizer.Add(row, 0, wx.ALL, 4)

    def show_main_menu(self) -> None:
        self.clear()
        title = wx.StaticText(self.panel, label=self.t("main_menu"))
        self.root_sizer.Add(title, 0, wx.ALL, 4)
        self.menu_actions = [
            (self.t("search_youtube"), self.show_search),
            (self.t("choose_download_folder"), self.choose_download_folder),
            (self.t("favorites"), self.show_favorites),
            (self.t("settings"), self.show_settings),
            (self.t("exit"), self.Close),
        ]
        self.menu_list = wx.ListBox(self.panel, choices=[item[0] for item in self.menu_actions])
        self.menu_list.SetName(self.t("main_menu"))
        self.menu_list.SetSelection(0)
        self.menu_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.activate_menu())
        self.menu_list.Bind(wx.EVT_KEY_DOWN, self.on_menu_key)
        self.root_sizer.Add(self.menu_list, 1, wx.EXPAND | wx.ALL, 4)
        self.add_button_row([(self.t("open"), self.activate_menu)])
        self.panel.Layout()
        wx.CallAfter(self.menu_list.SetFocus)

    def on_menu_key(self, event: wx.KeyEvent) -> None:
        if event.GetKeyCode() == wx.WXK_RETURN:
            self.activate_menu()
            return
        event.Skip()

    def activate_menu(self) -> None:
        index = self.menu_list.GetSelection()
        if index != wx.NOT_FOUND:
            self.menu_actions[index][1]()

    def show_search(self, restore_search: bool = False) -> None:
        self.clear()
        self.add_button_row([(self.t("back"), self.show_main_menu)])
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
            choices=[self.t("video"), self.t("playlist"), self.t("channel")],
        )
        self.search_type.SetName(self.t("type"))
        self.search_type.SetSelection(self.last_search_type_index if restore_search else 0)
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
        self.results_list = wx.ListBox(self.panel, choices=[])
        self.results_list.SetName(self.t("search_youtube"))
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        wx.CallAfter(self.query.SetFocus)

    def on_results_key(self, event: wx.KeyEvent) -> None:
        key = event.GetKeyCode()
        if key == wx.WXK_RETURN:
            self.play_selected()
        elif key == getattr(wx, "WXK_APPS", -1) or (key == wx.WXK_F10 and event.ShiftDown()):
            self.open_context_menu()
        else:
            event.Skip()
            wx.CallAfter(self.maybe_extend_results)

    def on_results_selection(self, event) -> None:
        event.Skip()
        self.maybe_extend_results()

    def show_favorites(self) -> None:
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
        self.favorites_list.Bind(wx.EVT_KEY_DOWN, self.on_favorites_key)
        self.root_sizer.Add(self.favorites_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_favorites()
        self.panel.Layout()
        wx.CallAfter(self.favorites_list.SetFocus)

    def on_favorites_key(self, event: wx.KeyEvent) -> None:
        if event.GetKeyCode() == wx.WXK_RETURN:
            self.play_favorite()
        else:
            event.Skip()

    def show_settings(self) -> None:
        self.clear()
        self.add_button_row([(self.t("back"), self.show_main_menu), (self.t("save"), self.save_settings_from_ui)])
        scroller = wx.ScrolledWindow(self.panel)
        scroller.SetScrollRate(10, 10)
        form = wx.FlexGridSizer(0, 2, 6, 6)
        form.AddGrowableCol(1, 1)
        self.controls = {}

        def text(key: str, value: str, style: int = 0):
            form.Add(wx.StaticText(scroller, label=self.t(key)), 0, wx.ALIGN_CENTER_VERTICAL)
            ctrl = wx.TextCtrl(scroller, value=value, style=style)
            ctrl.SetName(self.t(key))
            form.Add(ctrl, 1, wx.EXPAND)
            self.controls[key] = ctrl

        def choice(key: str, value: str, options: list[str]):
            form.Add(wx.StaticText(scroller, label=self.t(key)), 0, wx.ALIGN_CENTER_VERTICAL)
            ctrl = wx.Choice(scroller, choices=options)
            ctrl.SetName(self.t(key))
            selected = options.index(value) if value in options else 0
            ctrl.SetSelection(selected)
            form.Add(ctrl, 1, wx.EXPAND)
            self.controls[key] = ctrl

        def check(key: str, value: bool):
            form.AddSpacer(1)
            ctrl = wx.CheckBox(scroller, label=self.t(key))
            ctrl.SetName(self.t(key))
            ctrl.SetValue(value)
            form.Add(ctrl, 1, wx.EXPAND)
            self.controls[key] = ctrl

        form.Add(wx.StaticText(scroller, label=self.t("language")), 0, wx.ALIGN_CENTER_VERTICAL)
        lang = wx.Choice(scroller, choices=["Slovenščina", "English"])
        lang.SetSelection(1 if self.settings.language == "en" else 0)
        lang.SetName(self.t("language"))
        form.Add(lang, 1, wx.EXPAND)
        self.controls["language"] = lang
        text("download_folder", self.settings.download_folder)
        browse = wx.Button(scroller, label=self.t("browse"))
        browse.Bind(wx.EVT_BUTTON, lambda _evt: self.choose_download_folder())
        form.AddSpacer(1)
        form.Add(browse, 0)
        results_limit_value = "0" if self.settings.results_limit == 0 else str(min(250, self.settings.results_limit))
        choice("results_limit", results_limit_value, ["0", "10", "20", "50", "100", "150", "200", "250"])
        choice("seek_seconds", str(self.settings.seek_seconds), ["5", "10", "15", "30"])
        choice("volume_step", str(self.settings.volume_step), ["1", "2", "5", "10"])
        check("auto_update", self.settings.auto_update_ytdlp)
        check("auto_update_app", self.settings.auto_update_app)
        check("autoplay_next", self.settings.autoplay_next)
        check("confirm_download", self.settings.confirm_before_download)
        check("open_after_download", self.settings.open_folder_after_download)
        choice("audio_format", self.settings.audio_format, ["mp3", "m4a", "opus", "wav", "flac"])
        choice("audio_quality", self.settings.audio_quality, ["0", "1", "2", "3", "4", "5", "128", "192", "256", "320"])
        choice("video_format", self.settings.video_format, ["bestvideo+bestaudio/best", "best", "best[ext=mp4]", "worst"])
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
        text("player_command", self.settings.player_command)
        choice("player_speed", self.settings.player_speed, ["0.5", "0.75", "1.0", "1.25", "1.5", "1.75", "2.0"])
        check("browser_playback", self.settings.prefer_browser_playback)
        check("fullscreen", self.settings.player_fullscreen)
        check("start_paused", self.settings.player_start_paused)
        text("rate_limit", self.settings.rate_limit)
        text("proxy", self.settings.proxy)
        text("cookies", self.settings.cookies_file)
        text("ffmpeg", self.settings.ffmpeg_location)
        text("github_owner", self.settings.github_owner)
        text("github_repo", self.settings.github_repo)
        text("github_token", self.settings.github_token, wx.TE_PASSWORD)
        choice("fragments", str(self.settings.concurrent_fragments), ["1", "2", "4", "8", "16"])
        choice("retries", str(self.settings.retries), ["0", "3", "5", "10", "20"])
        choice("timeout", str(self.settings.socket_timeout), ["5", "10", "20", "30", "60"])

        scroller.SetSizer(form)
        self.root_sizer.Add(scroller, 1, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        wx.CallAfter(lang.SetFocus)

    def search_type_code(self) -> str:
        index = self.search_type.GetSelection()
        return ("Video", "Playlist", "Kanal")[index if index != wx.NOT_FOUND else 0]

    def search(self) -> None:
        if yt_dlp is None:
            self.message(self.t("missing_ytdlp"), wx.ICON_ERROR)
            return
        query = self.query.GetValue().strip()
        if not query:
            self.message(self.t("enter_query"))
            return
        self.last_search_query = query
        self.last_search_type_index = self.search_type.GetSelection()
        self.set_status(self.t("searching", query=query))
        threading.Thread(target=self.search_worker, args=(query, self.search_type_code()), daemon=True).start()

    def effective_results_limit(self) -> int:
        return 250 if self.settings.results_limit == 0 else self.settings.results_limit

    def search_worker(self, query: str, search_type: str) -> None:
        try:
            limit = self.effective_results_limit()
            options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": limit}
            with yt_dlp.YoutubeDL(options) as ydl:
                if search_type == "Video":
                    info = ydl.extract_info(f"ytsearch{limit}:{query}", download=False)
                else:
                    info = ydl.extract_info(self.youtube_search_url(query, search_type), download=False)
            entries = list(info.get("entries") or [])[:limit]
            wx.CallAfter(self.show_results, [self.normalize_entry(entry, search_type) for entry in entries])
        except Exception as exc:
            wx.CallAfter(self.message, str(exc), wx.ICON_ERROR)

    def normalize_entry(self, entry: dict, search_type: str) -> dict:
        url = entry.get("webpage_url") or entry.get("url") or ""
        if url and not url.startswith("http"):
            ie_key = (entry.get("ie_key") or "").lower()
            if "playlist" in ie_key:
                url = f"https://www.youtube.com/playlist?list={url}"
            elif "tab" in ie_key or search_type == "Kanal":
                url = f"https://www.youtube.com/{url.lstrip('/')}"
            else:
                url = f"https://www.youtube.com/watch?v={url}"
        return {
            "title": entry.get("title") or "",
            "channel": entry.get("uploader") or entry.get("channel") or "",
            "views": self.format_count(entry.get("view_count")),
            "view_count": entry.get("view_count"),
            "age": self.format_age(entry),
            "duration": self.format_duration(entry.get("duration")),
            "duration_seconds": entry.get("duration"),
            "timestamp": entry.get("timestamp"),
            "upload_date": entry.get("upload_date"),
            "description": entry.get("description") or "",
            "type": self.t("channel") if search_type == "Kanal" else search_type,
            "kind": "channel" if search_type == "Kanal" else "playlist" if search_type == "Playlist" else "video",
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
        self.results_list.Clear()
        for index, item in enumerate(self.results):
            self.results_list.Append(self.result_line(index, item))
        if self.results:
            selected_index = min(max(0, selection), len(self.results) - 1)
            self.results_list.SetSelection(selected_index)
            self.results_list.SetFocus()
        self.set_status(self.t("found", count=len(self.results)))

    def maybe_extend_results(self) -> None:
        if self.settings.results_limit != 0 or not hasattr(self, "results_list"):
            return
        selection = self.results_list.GetSelection()
        if selection == wx.NOT_FOUND:
            return
        if selection < len(self.results) - 1:
            return
        if len(self.results) >= len(self.all_results):
            return
        next_count = min(len(self.all_results), len(self.results) + RESULTS_PAGE_SIZE)
        self.show_results(self.all_results, selection=selection, visible_count=next_count)
        self.set_status(self.t("search_more_loaded", count=len(self.results)))

    def result_line(self, index: int, item: dict) -> str:
        parts = [
            item["title"],
            f"{self.t('channel')}: {item['channel']}",
            f"{self.t('views')}: {item['views']}",
            item.get("age", ""),
            item.get("duration", ""),
            item["type"],
        ]
        return " | ".join(part for part in parts if part)

    def selected_result(self) -> dict | None:
        if not hasattr(self, "results_list"):
            return None
        index = self.results_list.GetSelection()
        if index == wx.NOT_FOUND:
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
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(item["url"], item["title"])

    def open_channel_videos(self, item: dict) -> None:
        self.set_status(self.t("loading_channel", title=item["title"]))
        url = item["url"].rstrip("/")
        if not url.endswith("/videos"):
            url = f"{url}/videos"
        threading.Thread(target=self.load_collection_worker, args=(url, "Video"), daemon=True).start()

    def open_playlist_videos(self, item: dict) -> None:
        self.set_status(self.t("loading_playlist", title=item["title"]))
        threading.Thread(target=self.load_collection_worker, args=(item["url"], "Video"), daemon=True).start()

    def load_collection_worker(self, url: str, result_type: str) -> None:
        try:
            limit = self.effective_results_limit()
            options = {
                "quiet": True,
                "extract_flat": True,
                "skip_download": True,
                "playlistend": limit,
            }
            with yt_dlp.YoutubeDL(options) as ydl:
                info = ydl.extract_info(url, download=False)
            entries = list(info.get("entries") or [])[:limit]
            wx.CallAfter(self.show_results, [self.normalize_entry(entry, result_type) for entry in entries])
        except Exception as exc:
            wx.CallAfter(self.message, str(exc), wx.ICON_ERROR)

    def play_url(self, url: str, title: str = "") -> None:
        player = self.resolve_player()
        if not player:
            self.message(self.t("player_missing"), wx.ICON_ERROR)
            return
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
            wx.CallAfter(self.message, self.t("player_failed", error=exc), wx.ICON_ERROR)

    def resolve_stream_url(self, url: str) -> tuple[str, dict, dict]:
        if yt_dlp is None:
            raise RuntimeError(self.t("missing_ytdlp"))
        options = {
            "quiet": True,
            "skip_download": True,
            "format": "best[ext=mp4]/best",
            "noplaylist": True,
        }
        with yt_dlp.YoutubeDL(options) as ydl:
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
                f"--speed={self.settings.player_speed}",
                "--term-playing-msg=",
                "--msg-level=all=warn",
            ]
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
            self.current_video_info["speed"] = self.settings.player_speed
            self.current_video_info["pitch"] = "1.00"
            self.update_details_text()
            self.set_status(self.t("playing", title=title))
        except Exception as exc:
            self.message(self.t("player_failed", error=exc), wx.ICON_ERROR)

    def show_player_page(self, title: str) -> None:
        self.clear()
        self.add_button_row(
            [
                (self.t("back_results"), self.back_to_results),
                (self.t("play"), lambda: self.player_command("cycle pause")),
                (self.t("copy_link"), self.copy_active_url),
                (self.t("details_button"), self.show_video_details),
            ]
        )
        label = wx.StaticText(self.panel, label=f"{self.t('internal_player')}: {title}")
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.player_panel = wx.Panel(self.panel, style=wx.BORDER_SIMPLE)
        self.player_panel.SetName(self.t("internal_player"))
        self.player_panel.SetBackgroundColour(wx.BLACK)
        self.root_sizer.Add(self.player_panel, 1, wx.EXPAND | wx.ALL, 4)
        self.details_label = wx.StaticText(self.panel, label=self.t("video_details"))
        self.video_details = richtext.RichTextCtrl(
            self.panel,
            style=wx.VSCROLL | wx.HSCROLL | wx.WANTS_CHARS | wx.BORDER_THEME,
        )
        self.video_details.SetName(self.t("video_details"))
        self.video_details.SetEditable(False)
        self.video_details.SetMinSize((-1, 140))
        self.root_sizer.Add(self.details_label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 4)
        self.root_sizer.Add(self.video_details, 0, wx.EXPAND | wx.ALL, 4)
        self.details_label.Hide()
        self.video_details.Hide()
        self.in_player_screen = True
        self.player_control_mode = True
        self.update_details_text()
        self.panel.Layout()
        self.player_panel.SetFocus()

    def back_to_results(self) -> None:
        self.stop_player(silent=True)
        self.in_player_screen = False
        results = self.return_all_results or self.all_results or self.return_results or self.results
        if results:
            self.show_search(restore_search=True)
            self.show_results(results, selection=self.return_index, visible_count=self.return_visible_count)
            if results:
                index = min(max(0, self.return_index), len(results) - 1)
                self.current_index = index
                self.results_list.SetSelection(index)
                self.results_list.SetFocus()
        else:
            self.show_main_menu()

    def announce_player(self, text: str) -> None:
        self.set_status(text)

    def show_video_details(self) -> None:
        if not hasattr(self, "video_details"):
            return
        self.update_details_text()
        self.details_label.Show()
        self.video_details.Show()
        self.panel.Layout()
        self.video_details.SetFocus()

    def update_details_text(self) -> None:
        if not hasattr(self, "video_details"):
            return
        self.video_details.Freeze()
        self.video_details.Clear()
        self.video_details.WriteText(self.build_video_details_text())
        self.video_details.ShowPosition(0)
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
        return self.selected_result()

    def copy_url_to_clipboard(self, url: str) -> None:
        if not url:
            return
        if wx.TheClipboard.Open():
            try:
                wx.TheClipboard.SetData(wx.TextDataObject(url))
            finally:
                wx.TheClipboard.Close()
        self.set_status(self.t("url_copied"))

    def copy_active_url(self) -> None:
        item = self.active_item()
        if item:
            self.copy_url_to_clipboard(item.get("url", ""))

    def player_command(self, command: str) -> None:
        if self.player_kind != "mpv" or not self.ipc_path:
            return
        try:
            self.mpv_send(shlex.split(command))
        except OSError:
            pass

    def mpv_send(self, command: list) -> None:
        payload = json.dumps({"command": command}) + "\n"
        with open(self.ipc_path, "w", encoding="utf-8") as pipe:
            pipe.write(payload)

    def mpv_request(self, command: list) -> dict:
        if self.player_kind != "mpv" or not self.ipc_path:
            return {}
        request_id = int(time.time() * 1000000)
        payload = (json.dumps({"command": command, "request_id": request_id}) + "\n").encode("utf-8")
        with open(self.ipc_path, "r+b", buffering=0) as pipe:
            pipe.write(payload)
            for _ in range(25):
                raw = pipe.readline()
                if not raw:
                    break
                try:
                    response = json.loads(raw.decode("utf-8", errors="replace"))
                except json.JSONDecodeError:
                    continue
                if response.get("request_id") == request_id:
                    return response
        return {}

    def mpv_get_property(self, name: str):
        response = self.mpv_request(["get_property", name])
        return response.get("data")

    def mpv_set_property(self, name: str, value) -> None:
        self.mpv_send(["set_property", name, value])

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
            speed = min(4.0, max(0.25, round(speed + delta, 2)))
            self.mpv_set_property("speed", speed)
            self.current_video_info["speed"] = f"{speed:.2f}"
            wx.CallAfter(self.announce_player, self.t("speed_announcement", speed=f"{speed:.2f}"))
            wx.CallAfter(self.update_details_text)
        except Exception:
            wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

    def change_pitch_async(self, delta: float) -> None:
        threading.Thread(target=self.change_pitch_worker, args=(delta,), daemon=True).start()

    def change_pitch_worker(self, delta: float) -> None:
        try:
            current = self.mpv_get_property("pitch")
            pitch = float(current if current is not None else 1.0)
            pitch = min(2.0, max(0.5, round(pitch + delta, 2)))
            self.mpv_set_property("pitch", pitch)
            self.current_video_info["pitch"] = f"{pitch:.2f}"
            wx.CallAfter(self.announce_player, self.t("pitch_announcement", pitch=f"{pitch:.2f}"))
            wx.CallAfter(self.update_details_text)
        except Exception:
            wx.CallAfter(self.announce_player, self.t("timing_unavailable"))

    def stop_player(self, silent: bool = False) -> None:
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
        if not self.in_player_screen:
            self.in_player_screen = False
        if not silent:
            self.set_status(self.t("stopped"))

    def on_char_hook(self, event: wx.KeyEvent) -> None:
        key = event.GetKeyCode()
        focus = wx.Window.FindFocus()
        details_has_focus = focus is getattr(self, "video_details", None)
        if key == wx.WXK_RETURN and focus is getattr(self, "menu_list", None):
            self.activate_menu()
            return
        if key == wx.WXK_RETURN and focus is getattr(self, "results_list", None):
            self.play_selected()
            return
        if event.ControlDown() and event.ShiftDown() and key in (ord("A"), ord("a")):
            self.download_audio()
            return
        if event.ControlDown() and event.ShiftDown() and key in (ord("D"), ord("d")):
            self.download_video()
            return
        if key == wx.WXK_ESCAPE:
            if self.in_player_screen:
                self.back_to_results()
                return
            self.show_main_menu()
            return
        if self.in_player_screen and key in (ord("L"), ord("l")):
            self.copy_active_url()
            return
        if self.player_control_mode and not details_has_focus:
            if key == wx.WXK_SPACE:
                self.player_command("cycle pause")
                return
            if key in (ord("T"), ord("t")):
                self.announce_time_async()
                return
            if key in (ord("S"), ord("s")):
                self.change_speed_async(-0.10)
                return
            if key in (ord("D"), ord("d")):
                self.change_speed_async(0.10)
                return
            if key == wx.WXK_UP and event.ControlDown():
                self.change_pitch_async(0.05)
                return
            if key == wx.WXK_DOWN and event.ControlDown():
                self.change_pitch_async(-0.05)
                return
            if key in (ord("V"), ord("v")):
                self.show_video_details()
                return
            if key == wx.WXK_LEFT and event.ControlDown() and event.ShiftDown():
                self.player_command("seek -600")
                return
            if key == wx.WXK_RIGHT and event.ControlDown() and event.ShiftDown():
                self.player_command("seek 600")
                return
            if key == wx.WXK_LEFT and event.ControlDown():
                self.player_command("seek -60")
                return
            if key == wx.WXK_RIGHT and event.ControlDown():
                self.player_command("seek 60")
                return
            if key == wx.WXK_LEFT:
                self.player_command("seek -5")
                return
            if key == wx.WXK_RIGHT:
                self.player_command("seek 5")
                return
            if key == wx.WXK_UP:
                self.player_command(f"add volume {self.settings.volume_step}")
                return
            if key == wx.WXK_DOWN:
                self.player_command(f"add volume -{self.settings.volume_step}")
                return
        event.Skip()

    def open_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("play"), self.play_selected),
            (self.t("download_audio"), self.download_audio),
            (self.t("download_video"), self.download_video),
            (self.t("add_favorite"), self.add_selected_favorite),
            (self.t("open_browser"), self.open_selected_in_browser),
            (self.t("copy_url"), self.copy_selected_url),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def start_download(self, audio_only: bool) -> None:
        item = self.active_item()
        if not item:
            self.message(self.t("no_selection"))
            return
        action = "audio" if audio_only else "video"
        if self.settings.confirm_before_download:
            if wx.MessageBox(self.t("download_confirm", action=action, title=item["title"]), APP_NAME, wx.YES_NO | wx.ICON_QUESTION) != wx.YES:
                self.set_status(self.t("download_cancelled"))
                return
        self.set_status(f"{item['title']}: {self.t('queued')}")
        threading.Thread(target=self.download_worker, args=(item, audio_only), daemon=True).start()

    def download_audio(self) -> None:
        self.start_download(True)

    def download_video(self) -> None:
        self.start_download(False)

    def download_worker(self, item: dict, audio_only: bool) -> None:
        try:
            folder = Path(self.settings.download_folder)
            folder.mkdir(parents=True, exist_ok=True)
            options = self.download_options(folder, audio_only, item["title"])
            with yt_dlp.YoutubeDL(options) as ydl:
                ydl.download([item["url"]])
            done_text = self.t("download_audio_done" if audio_only else "download_video_done", title=item["title"])
            wx.CallAfter(self.set_status, done_text)
            wx.CallAfter(self.message, done_text, wx.ICON_INFORMATION)
            if self.settings.open_folder_after_download:
                os.startfile(folder)  # type: ignore[attr-defined]
        except Exception as exc:
            wx.CallAfter(self.message, self.t("download_failed", error=exc), wx.ICON_ERROR)

    def download_options(self, folder: Path, audio_only: bool, title: str) -> dict:
        progress_hook = self.make_download_progress_hook(title, audio_only)
        options = {
            "outtmpl": str(folder / self.settings.filename_template),
            "quiet": self.settings.quiet_downloads,
            "noplaylist": not self.settings.keep_playlist_order,
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
        elif self.settings.max_video_height > 0:
            options["format"] = f"bestvideo[height<={self.settings.max_video_height}]+bestaudio/best[height<={self.settings.max_video_height}]/{self.settings.video_format}"
        else:
            options["format"] = self.settings.video_format
        return options

    def make_download_progress_hook(self, title: str, audio_only: bool):
        mode = self.t("download_audio_mode" if audio_only else "download_video_mode")

        def hook(data: dict) -> None:
            status = data.get("status")
            if status == "downloading":
                percent_text = str(data.get("_percent_str") or "").strip().replace("%", "")
                if not percent_text:
                    downloaded = data.get("downloaded_bytes") or 0
                    total = data.get("total_bytes") or data.get("total_bytes_estimate") or 0
                    if total:
                        percent_text = f"{(float(downloaded) / float(total)) * 100:.1f}"
                if percent_text:
                    self.ui_queue.put(("status", self.t("download_progress", mode=mode, percent=percent_text, title=title)))
            elif status == "finished":
                self.ui_queue.put(("status", self.t("download_processing", mode=mode, title=title)))

        return hook

    def add_selected_favorite(self) -> None:
        item = self.selected_result()
        if not item:
            self.message(self.t("no_selection"))
            return
        favorite = {"title": item["title"], "channel": item["channel"], "url": item["url"]}
        if any(existing["url"] == favorite["url"] for existing in self.favorites):
            self.set_status(self.t("favorite_exists"))
            return
        self.favorites.append(favorite)
        self.save_favorites()
        self.set_status(self.t("favorite_added"))

    def refresh_favorites(self) -> None:
        if not hasattr(self, "favorites_list"):
            return
        self.favorites_list.Clear()
        for index, item in enumerate(self.favorites):
            self.favorites_list.Append(f"{index + 1}. {item['title']} | {self.t('channel')}: {item['channel']}")
        if self.favorites:
            self.favorites_list.SetSelection(0)

    def selected_favorite(self) -> dict | None:
        index = self.favorites_list.GetSelection()
        return None if index == wx.NOT_FOUND else self.favorites[index]

    def play_favorite(self) -> None:
        item = self.selected_favorite()
        if item:
            self.current_video_item = item
            self.current_video_info = dict(item)
            self.play_url(item["url"], item["title"])

    def remove_favorite(self) -> None:
        index = self.favorites_list.GetSelection()
        if index != wx.NOT_FOUND:
            del self.favorites[index]
            self.save_favorites()
            self.refresh_favorites()
            self.set_status(self.t("favorite_removed"))

    def open_selected_in_browser(self) -> None:
        item = self.active_item()
        if item:
            webbrowser.open(item["url"])

    def copy_selected_url(self) -> None:
        item = self.active_item()
        if item:
            self.copy_url_to_clipboard(item["url"])

    def choose_download_folder(self) -> None:
        with wx.DirDialog(self, self.t("choose_download_folder"), self.settings.download_folder) as dialog:
            if dialog.ShowModal() == wx.ID_OK:
                self.settings.download_folder = dialog.GetPath()
                if hasattr(self, "controls") and "download_folder" in self.controls:
                    self.controls["download_folder"].SetValue(self.settings.download_folder)
                self.save_settings()
                self.set_status(self.t("settings_saved"))

    def save_settings_from_ui(self) -> None:
        c = self.controls
        old_language = self.settings.language
        self.settings.language = "en" if c["language"].GetSelection() == 1 else "sl"
        self.settings.download_folder = c["download_folder"].GetValue()
        self.settings.results_limit = self.to_int(c["results_limit"].GetStringSelection(), 20, 0, 250)
        self.settings.seek_seconds = self.to_int(c["seek_seconds"].GetStringSelection(), 5, 1)
        self.settings.volume_step = self.to_int(c["volume_step"].GetStringSelection(), 5, 1)
        self.settings.auto_update_ytdlp = c["auto_update"].GetValue()
        self.settings.auto_update_app = c["auto_update_app"].GetValue()
        self.settings.autoplay_next = c["autoplay_next"].GetValue()
        self.settings.confirm_before_download = c["confirm_download"].GetValue()
        self.settings.open_folder_after_download = c["open_after_download"].GetValue()
        self.settings.audio_format = c["audio_format"].GetStringSelection() or "mp3"
        self.settings.audio_quality = c["audio_quality"].GetStringSelection() or "0"
        self.settings.video_format = c["video_format"].GetStringSelection() or "bestvideo+bestaudio/best"
        self.settings.max_video_height = self.to_int(c["max_height"].GetStringSelection(), 1080, 0)
        self.settings.filename_template = c["filename_template"].GetValue() or DEFAULT_FILENAME_TEMPLATE
        self.settings.subtitle_languages = c["subtitle_langs"].GetValue() or "sl,en"
        self.settings.quiet_downloads = c["quiet_downloads"].GetValue()
        self.settings.keep_playlist_order = c["playlist_order"].GetValue()
        self.settings.write_thumbnail = c["write_thumbnail"].GetValue()
        self.settings.write_description = c["write_description"].GetValue()
        self.settings.write_info_json = c["write_info_json"].GetValue()
        self.settings.write_subtitles = c["write_subtitles"].GetValue()
        self.settings.auto_subtitles = c["auto_subtitles"].GetValue()
        self.settings.embed_metadata = c["embed_metadata"].GetValue()
        self.settings.embed_thumbnail = c["embed_thumbnail"].GetValue()
        self.settings.restrict_filenames = c["restrict_filenames"].GetValue()
        self.settings.download_archive = c["download_archive"].GetValue()
        self.settings.player_command = c["player_command"].GetValue()
        self.settings.player_speed = c["player_speed"].GetStringSelection() or "1.0"
        self.settings.prefer_browser_playback = c["browser_playback"].GetValue()
        self.settings.player_fullscreen = c["fullscreen"].GetValue()
        self.settings.player_start_paused = c["start_paused"].GetValue()
        self.settings.rate_limit = c["rate_limit"].GetValue()
        self.settings.proxy = c["proxy"].GetValue()
        self.settings.cookies_file = c["cookies"].GetValue()
        self.settings.ffmpeg_location = c["ffmpeg"].GetValue()
        self.settings.github_owner = c["github_owner"].GetValue().strip() or DEFAULT_GITHUB_OWNER
        self.settings.github_repo = c["github_repo"].GetValue().strip() or DEFAULT_GITHUB_REPO
        self.settings.github_token = c["github_token"].GetValue().strip()
        self.settings.concurrent_fragments = self.to_int(c["fragments"].GetStringSelection(), 4, 1)
        self.settings.retries = self.to_int(c["retries"].GetStringSelection(), 10, 0)
        self.settings.socket_timeout = self.to_int(c["timeout"].GetStringSelection(), 20, 1)
        self.save_settings()
        self.set_status(self.t("settings_saved"))
        if self.settings.language != old_language:
            self.show_settings()

    def start_ytdlp_update_check(self) -> None:
        self.set_status(self.t("checking_updates"))
        threading.Thread(target=self.update_ytdlp_worker, daemon=True).start()

    def update_ytdlp_worker(self) -> None:
        if yt_dlp is None:
            self.ui_queue.put(("status", self.t("missing_ytdlp")))
            return
        try:
            with yt_dlp.YoutubeDL({"quiet": True, "skip_download": True}) as ydl:
                from yt_dlp.update import run_update

                run_update(ydl)
            self.ui_queue.put(("status", self.t("updates_ok")))
        except Exception as exc:
            self.ui_queue.put(("status", self.t("updates_failed", error=exc)))

    def start_app_update_check(self) -> None:
        if not self.settings.auto_update_app:
            self.set_status(self.t("app_update_disabled"))
            return
        self.set_status(self.t("checking_app_updates"))
        threading.Thread(target=self.app_update_worker, daemon=True).start()

    def app_update_worker(self) -> None:
        try:
            release = self.fetch_latest_release()
            if not release:
                self.ui_queue.put(("status", self.t("app_up_to_date")))
                return
            remote_version = self.release_version(release)
            if not self.is_newer_version(remote_version, APP_VERSION):
                self.ui_queue.put(("status", self.t("app_up_to_date")))
                return
            asset = self.find_release_asset(release)
            if not asset:
                self.ui_queue.put(("status", self.t("app_update_failed", error="no Windows asset found in release")))
                return
            wx.CallAfter(self.prompt_for_app_update, release, asset)
        except Exception as exc:
            self.ui_queue.put(("status", self.t("app_update_failed", error=exc)))

    def prompt_for_app_update(self, release: dict, asset: dict) -> None:
        version = self.release_version(release)
        if not getattr(sys, "frozen", False):
            self.message(self.t("update_source_only", version=version))
            return
        changelog = self.release_changelog_text(release)
        if self.show_update_prompt(version, changelog):
            threading.Thread(target=self.download_and_install_update, args=(release, asset), daemon=True).start()

    def show_update_prompt(self, version: str, changelog: str) -> bool:
        dialog = wx.Dialog(self, title=f"{APP_NAME} {version}", style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetMinSize((640, 420))
        root = wx.BoxSizer(wx.VERTICAL)
        intro = wx.StaticText(dialog, label=f"{self.t('whats_new')} - {version}")
        root.Add(intro, 0, wx.ALL, 10)
        details = wx.TextCtrl(dialog, value=changelog, style=wx.TE_MULTILINE | wx.TE_READONLY | wx.TE_RICH2)
        details.SetName(self.t("whats_new"))
        details.SetMinSize((580, 260))
        root.Add(details, 1, wx.EXPAND | wx.LEFT | wx.RIGHT, 10)
        question = wx.StaticText(dialog, label=self.t("update_now"))
        root.Add(question, 0, wx.ALL, 10)
        buttons = dialog.CreateStdDialogButtonSizer(wx.YES | wx.NO)
        root.Add(buttons, 0, wx.EXPAND | wx.ALL, 10)
        dialog.SetSizerAndFit(root)
        wx.CallAfter(details.SetFocus)
        try:
            return dialog.ShowModal() == wx.ID_YES
        finally:
            dialog.Destroy()

    def download_and_install_update(self, release: dict, asset: dict) -> None:
        version = self.release_version(release)
        try:
            token = self.resolve_github_token()
            self.ui_queue.put(("status", self.t("downloading_update", version=version)))
            temp_dir = Path(tempfile.mkdtemp(prefix="apricotplayer-update-"))
            downloaded_path = temp_dir / asset["name"]
            download_url = asset.get("url") or asset.get("browser_download_url") or ""
            if not download_url:
                raise RuntimeError("missing download url")
            headers = self.github_headers(token, accept="application/octet-stream")
            request = Request(download_url, headers=headers)
            with urlopen(request, timeout=120) as response, downloaded_path.open("wb") as handle:
                shutil.copyfileobj(response, handle)
            wx.CallAfter(self.finish_app_update_install, str(downloaded_path), version)
        except Exception as exc:
            self.ui_queue.put(("status", self.t("app_update_failed", error=exc)))

    def finish_app_update_install(self, downloaded_path: str, version: str) -> None:
        if not getattr(sys, "frozen", False):
            self.message(self.t("update_source_only", version=version))
            return
        current_exe = Path(sys.executable)
        script_path = Path(tempfile.gettempdir()) / f"apricotplayer-update-{int(time.time())}.cmd"
        script = "\n".join(
            [
                "@echo off",
                "setlocal",
                ":replace",
                f'copy /Y "{downloaded_path}" "{current_exe}" >nul',
                "if errorlevel 1 (",
                "  ping 127.0.0.1 -n 2 >nul",
                "  goto replace",
                ")",
                f'del "{downloaded_path}" >nul 2>nul',
                f'start "" "{current_exe}"',
                'del "%~f0" >nul 2>nul',
            ]
        )
        script_path.write_text(script, encoding="utf-8-sig")
        creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
        subprocess.Popen(["cmd", "/c", str(script_path)], creationflags=creationflags)
        self.set_status(self.t("installing_update", version=version))
        self.message(self.t("update_ready_restart"))
        self.Close()

    def fetch_latest_release(self) -> dict | None:
        owner = self.settings.github_owner.strip() or DEFAULT_GITHUB_OWNER
        repo = self.settings.github_repo.strip() or DEFAULT_GITHUB_REPO
        token = self.resolve_github_token()
        request = Request(
            f"https://api.github.com/repos/{owner}/{repo}/releases",
            headers=self.github_headers(token),
        )
        with urlopen(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
        if not isinstance(payload, list):
            return None
        for release in payload:
            if not release.get("draft"):
                return release
        return None

    def find_release_asset(self, release: dict) -> dict | None:
        assets = release.get("assets") or []
        preferred_name = Path(sys.executable).name if getattr(sys, "frozen", False) else UPDATE_ASSET_NAME
        for asset in assets:
            if asset.get("name") == preferred_name:
                return asset
        for asset in assets:
            name = str(asset.get("name") or "").lower()
            if name.endswith(".exe"):
                return asset
        return None

    @staticmethod
    def release_version(release: dict) -> str:
        return str(release.get("tag_name") or release.get("name") or "").strip().lstrip("v")

    def release_changelog_text(self, release: dict) -> str:
        body = str(release.get("body") or "").replace("\r\n", "\n").strip()
        if not body:
            return self.t("no_changelog")
        if len(body) > 6000:
            return body[:6000].rstrip() + "\n\n..."
        return body

    @staticmethod
    def parse_version(value: str) -> tuple[int, int, int, int, int]:
        match = re.match(r"^v?(\d+)\.(\d+)\.(\d+)(?:-([A-Za-z]+)(?:[.-]?(\d+))?)?$", value.strip())
        if not match:
            return (0, 0, 0, 0, 0)
        major, minor, patch = (int(match.group(1)), int(match.group(2)), int(match.group(3)))
        stage_name = (match.group(4) or "").lower()
        stage_number = int(match.group(5) or 0)
        stage_rank = {"alpha": 1, "beta": 2, "rc": 3}.get(stage_name, 4)
        return (major, minor, patch, stage_rank, stage_number)

    @classmethod
    def is_newer_version(cls, remote_version: str, current_version: str) -> bool:
        return cls.parse_version(remote_version) > cls.parse_version(current_version)

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

    def resolve_github_token(self) -> str:
        configured = self.settings.github_token.strip()
        if configured:
            return configured
        gh_path = self.find_gh_executable()
        if not gh_path:
            return ""
        try:
            result = subprocess.run(
                [gh_path, "auth", "token"],
                capture_output=True,
                text=True,
                timeout=15,
                check=True,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
            return result.stdout.strip()
        except Exception:
            return ""

    @staticmethod
    def find_gh_executable() -> str:
        candidates = [
            shutil.which("gh"),
            r"C:\Program Files\GitHub CLI\gh.exe",
            str(Path.home() / "AppData" / "Local" / "Programs" / "GitHub CLI" / "gh.exe"),
        ]
        for candidate in candidates:
            if candidate and Path(candidate).exists():
                return str(candidate)
        return ""

    def process_queue(self, _event) -> None:
        try:
            while True:
                kind, payload = self.ui_queue.get_nowait()
                if kind == "results":
                    self.show_results(payload)
                elif kind == "status":
                    self.set_status(str(payload))
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

    def load_settings(self) -> Settings:
        source = SETTINGS_FILE if SETTINGS_FILE.exists() else LEGACY_SETTINGS_FILE
        if source.exists():
            try:
                data = json.loads(source.read_text(encoding="utf-8"))
                merged = {**asdict(Settings()), **data}
                if merged.get("filename_template") == OLD_FILENAME_TEMPLATE:
                    merged["filename_template"] = DEFAULT_FILENAME_TEMPLATE
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

    @staticmethod
    def youtube_search_url(query: str, search_type: str) -> str:
        filters = {"Playlist": "EgIQAw==", "Kanal": "EgIQAg=="}
        return f"https://www.youtube.com/results?{urlencode({'search_query': query, 'sp': filters.get(search_type, '')})}"

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
    def make_ipc_path() -> str:
        return rf"\\.\pipe\urhasaurus-youtube-{os.getpid()}" if os.name == "nt" else f"/tmp/urhasaurus-youtube-{os.getpid()}.sock"


class App(wx.App):
    def OnInit(self) -> bool:
        frame = MainFrame()
        frame.Show()
        return True


def main() -> int:
    app = App(False)
    app.MainLoop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
