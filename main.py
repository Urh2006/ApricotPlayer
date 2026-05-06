from __future__ import annotations

import json
import os
import queue
import shlex
import shutil
import subprocess
import sys
import threading
import time
import webbrowser
from urllib.parse import urlencode
from dataclasses import asdict, dataclass
from pathlib import Path
from tkinter import (
    END,
    LEFT,
    VERTICAL,
    W,
    BooleanVar,
    IntVar,
    Listbox,
    StringVar,
    SINGLE,
    Tk,
    filedialog,
    messagebox,
)
from tkinter import ttk

try:
    import yt_dlp
except ImportError:  # pragma: no cover - shown to the user at runtime.
    yt_dlp = None


APP_NAME = "urhasaurus youtube player"
APP_DIR = Path(os.getenv("APPDATA", Path.home())) / "UrhasaurusYouTubePlayer"
SETTINGS_FILE = APP_DIR / "settings.json"
FAVORITES_FILE = APP_DIR / "favorites.json"


TEXT = {
    "sl": {
        "ready": "Pripravljen.",
        "search_tab": "Iskanje YouTube",
        "downloads_tab": "Prenosi",
        "favorites_tab": "Priljubljeni",
        "settings_tab": "Nastavitve",
        "main_menu": "Glavni meni",
        "menu_search": "Iskanje po YouTube",
        "menu_download_folder": "Izbor mape za prenose",
        "menu_favorites": "Priljubljeni",
        "menu_settings": "Nastavitve",
        "menu_exit": "Izhod",
        "open": "Odpri",
        "back_menu": "Nazaj v glavni meni",
        "query": "Iskalni niz:",
        "type": "Vrsta:",
        "search": "Search",
        "play": "Play",
        "download_video": "Download video",
        "download_audio": "Download audio",
        "add_favorite": "Add to favorites",
        "title": "Naslov",
        "channel": "Kanal",
        "views": "Ogledi",
        "published": "Objavljeno",
        "duration": "Trajanje",
        "download_folder": "Mapa za prenose:",
        "choose_folder": "Izberi mapo",
        "time": "Čas",
        "action": "Dejanje",
        "status": "Stanje",
        "remove": "Remove",
        "refresh": "Refresh list",
        "url": "URL",
        "copy_url": "Copy URL",
        "open_browser": "Open in browser",
        "general": "Splošno",
        "downloads": "Prenosi",
        "player": "Predvajalnik",
        "advanced": "Napredno",
        "language": "Jezik:",
        "results_limit": "Število rezultatov:",
        "seek_seconds": "Seek v sekundah:",
        "volume_step": "Korak glasnosti:",
        "autoplay_next": "Po koncu posnetka samodejno predvajaj naslednjega",
        "confirm_download": "Pred prenosom vprašaj za potrditev",
        "open_after_download": "Po prenosu odpri mapo za prenose",
        "auto_update_ytdlp": "Ob vsakem zagonu preveri posodobitve yt-dlp",
        "checking_updates": "Preverjam posodobitve za YouTube podporo.",
        "updates_ok": "YouTube podpora je posodobljena.",
        "updates_failed": "Posodobitve YouTube podpore ni bilo mogoče preveriti: {error}",
        "audio_format": "Audio format:",
        "audio_quality": "Audio kvaliteta (0 najboljše):",
        "video_format": "Video format yt-dlp:",
        "max_height": "Največja višina videa, npr. 720 ali 1080:",
        "filename_template": "Predloga imena datoteke:",
        "subtitle_langs": "Jeziki podnapisov, npr. sl,en:",
        "quiet_downloads": "Tišji prenosi z manj izpisa",
        "playlist_order": "Pri playlistah ohrani vrstni red",
        "write_thumbnail": "Shrani thumbnail sliko",
        "write_description": "Shrani opis videa",
        "write_info_json": "Shrani info JSON",
        "write_subtitles": "Prenesi ročne podnapise",
        "auto_subtitles": "Prenesi samodejne podnapise",
        "embed_metadata": "Vgradi metapodatke v datoteko",
        "embed_thumbnail": "Vgradi thumbnail, če format to podpira",
        "restrict_filenames": "Uporabi varna ASCII imena datotek",
        "download_archive": "Uporabi download archive in preskoči že prenesene",
        "player_command": "Ukaz ali pot do playerja:",
        "player_speed": "Hitrost predvajanja, npr. 1.0 ali 1.25:",
        "browser_playback": "Za predvajanje vedno uporabi brskalnik",
        "fullscreen": "Začni predvajanje v full screen načinu",
        "start_paused": "Začni predvajanje pavzirano",
        "choose_player": "Izberi player",
        "rate_limit": "Rate limit, npr. 2M ali prazno:",
        "proxy": "Proxy URL:",
        "cookies": "Cookies file:",
        "ffmpeg": "FFmpeg mapa ali ffmpeg.exe:",
        "fragments": "Sočasni fragmenti:",
        "retries": "Število ponovitev ob napaki:",
        "timeout": "Socket timeout v sekundah:",
        "choose_cookies": "Izberi cookies file",
        "choose_ffmpeg": "Izberi FFmpeg",
        "save_settings": "Shrani nastavitve",
        "test_player": "Test playerja",
        "open_data": "Odpri mapo s podatki",
        "enter_query": "Vpiši iskalni niz.",
        "searching": "Iščem: {query}",
        "found": "Najdenih rezultatov: {count}.",
        "select_video": "Izberi posnetek.",
        "invalid_url": "Ta rezultat nima veljavnega URL-ja.",
        "opened_browser": "Odprto v brskalniku: {title}",
        "no_player": "Player ni najden, zato sem odprl brskalnik.",
        "playing": "Predvajam: {title}",
        "player_failed": "Player se ni zagnal, odprt je brskalnik: {error}",
        "stopped": "Predvajanje ustavljeno.",
        "queued": "V čakalni vrsti",
        "done": "Končano",
        "download_done": "Prenos končan: {title}",
        "download_failed": "Prenos ni uspel: {error}",
        "download_confirm": "Prenesem {action}: {title}?",
        "download_cancelled": "Prenos preklican.",
        "favorite_added": "Dodano med priljubljene.",
        "favorite_exists": "Ta posnetek je že med priljubljenimi.",
        "favorite_removed": "Odstranjeno iz priljubljenih.",
        "url_copied": "URL kopiran.",
        "settings_saved": "Nastavitve shranjene.",
        "player_found": "Najden player: {player}",
        "player_not_found": "Player ni najden. Uporabljen bo brskalnik.",
        "missing_ytdlp": "Manjka paket yt-dlp. Namesti ga z ukazom: py -m pip install -r requirements.txt",
    },
    "en": {
        "ready": "Ready.",
        "search_tab": "Search YouTube",
        "downloads_tab": "Downloads",
        "favorites_tab": "Favorites",
        "settings_tab": "Settings",
        "main_menu": "Main menu",
        "menu_search": "Search YouTube",
        "menu_download_folder": "Choose download folder",
        "menu_favorites": "Favorites",
        "menu_settings": "Settings",
        "menu_exit": "Exit",
        "open": "Open",
        "back_menu": "Back to main menu",
        "query": "Search query:",
        "type": "Type:",
        "search": "Search",
        "play": "Play",
        "download_video": "Download video",
        "download_audio": "Download audio",
        "add_favorite": "Add to favorites",
        "title": "Title",
        "channel": "Channel",
        "views": "Views",
        "published": "Published",
        "duration": "Duration",
        "download_folder": "Download folder:",
        "choose_folder": "Choose folder",
        "time": "Time",
        "action": "Action",
        "status": "Status",
        "remove": "Remove",
        "refresh": "Refresh list",
        "url": "URL",
        "copy_url": "Copy URL",
        "open_browser": "Open in browser",
        "general": "General",
        "downloads": "Downloads",
        "player": "Player",
        "advanced": "Advanced",
        "language": "Language:",
        "results_limit": "Number of results:",
        "seek_seconds": "Seek seconds:",
        "volume_step": "Volume step:",
        "autoplay_next": "Automatically play next result after playback ends",
        "confirm_download": "Ask before starting a download",
        "open_after_download": "Open download folder after download",
        "auto_update_ytdlp": "Check yt-dlp updates on every startup",
        "checking_updates": "Checking updates for YouTube support.",
        "updates_ok": "YouTube support is up to date.",
        "updates_failed": "Could not check YouTube support updates: {error}",
        "audio_format": "Audio format:",
        "audio_quality": "Audio quality (0 is best):",
        "video_format": "Video format yt-dlp:",
        "max_height": "Maximum video height, e.g. 720 or 1080:",
        "filename_template": "Filename template:",
        "subtitle_langs": "Subtitle languages, e.g. sl,en:",
        "quiet_downloads": "Quieter downloads with less output",
        "playlist_order": "Keep playlist order",
        "write_thumbnail": "Save thumbnail image",
        "write_description": "Save video description",
        "write_info_json": "Save info JSON",
        "write_subtitles": "Download manual subtitles",
        "auto_subtitles": "Download automatic subtitles",
        "embed_metadata": "Embed metadata into file",
        "embed_thumbnail": "Embed thumbnail when supported",
        "restrict_filenames": "Use safe ASCII filenames",
        "download_archive": "Use download archive and skip already downloaded items",
        "player_command": "Player command or path:",
        "player_speed": "Playback speed, e.g. 1.0 or 1.25:",
        "browser_playback": "Always use browser for playback",
        "fullscreen": "Start playback in full screen",
        "start_paused": "Start playback paused",
        "choose_player": "Choose player",
        "rate_limit": "Rate limit, e.g. 2M or blank:",
        "proxy": "Proxy URL:",
        "cookies": "Cookies file:",
        "ffmpeg": "FFmpeg folder or ffmpeg.exe:",
        "fragments": "Concurrent fragments:",
        "retries": "Retries on error:",
        "timeout": "Socket timeout in seconds:",
        "choose_cookies": "Choose cookies file",
        "choose_ffmpeg": "Choose FFmpeg",
        "save_settings": "Save settings",
        "test_player": "Test player",
        "open_data": "Open data folder",
        "enter_query": "Enter a search query.",
        "searching": "Searching: {query}",
        "found": "Found results: {count}.",
        "select_video": "Select a video.",
        "invalid_url": "This result does not have a valid URL.",
        "opened_browser": "Opened in browser: {title}",
        "no_player": "Player not found, opened browser instead.",
        "playing": "Playing: {title}",
        "player_failed": "Player did not start, opened browser instead: {error}",
        "stopped": "Playback stopped.",
        "queued": "Queued",
        "done": "Done",
        "download_done": "Download finished: {title}",
        "download_failed": "Download failed: {error}",
        "download_confirm": "Download {action}: {title}?",
        "download_cancelled": "Download cancelled.",
        "favorite_added": "Added to favorites.",
        "favorite_exists": "This item is already in favorites.",
        "favorite_removed": "Removed from favorites.",
        "url_copied": "URL copied.",
        "settings_saved": "Settings saved.",
        "player_found": "Found player: {player}",
        "player_not_found": "Player not found. Browser will be used.",
        "missing_ytdlp": "Missing yt-dlp package. Install it with: py -m pip install -r requirements.txt",
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
    filename_template: str = "%(title)s [%(id)s].%(ext)s"
    audio_quality: str = "0"
    seek_seconds: int = 10
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
    confirm_before_download: bool = False
    download_archive: bool = False
    rate_limit: str = ""
    proxy: str = ""
    cookies_file: str = ""
    ffmpeg_location: str = ""
    concurrent_fragments: int = 4
    retries: int = 10
    socket_timeout: int = 20


class AccessibleYouTubeApp:
    def __init__(self, root: Tk) -> None:
        self.root = root
        self.root.title(APP_NAME)
        self.root.geometry("980x680")
        self.root.minsize(760, 520)

        APP_DIR.mkdir(parents=True, exist_ok=True)
        self.settings = self.load_settings()
        self.favorites = self.load_favorites()
        self.results: list[dict] = []
        self.current_index = -1
        self.player_process: subprocess.Popen | None = None
        self.player_kind = ""
        self.player_control_mode = False
        self.ipc_path: str | None = None

        self.ui_queue: queue.Queue[tuple[str, object]] = queue.Queue()
        self.status_var = StringVar(value=self.t("ready"))

        self.build_ui()
        self.bind_shortcuts()
        self.root.after(100, self.process_ui_queue)
        if self.settings.auto_update_ytdlp:
            self.root.after(300, self.start_update_check)

    def t(self, key: str, **kwargs) -> str:
        language = self.settings.language if self.settings.language in TEXT else "sl"
        text = TEXT[language].get(key, TEXT["sl"].get(key, key))
        return text.format(**kwargs) if kwargs else text

    def language_label(self) -> str:
        return "English" if self.settings.language == "en" else "Slovenščina"

    @staticmethod
    def language_code(label: str) -> str:
        return "en" if label == "English" else "sl"

    def search_type_labels(self) -> tuple[str, str, str]:
        return ("Video", "Playlist", "Channel") if self.settings.language == "en" else (
            "Video",
            "Playlist",
            "Kanal",
        )

    @staticmethod
    def search_type_code(label: str) -> str:
        return "Kanal" if label == "Channel" else label

    def rebuild_ui(self) -> None:
        for child in self.root.winfo_children():
            child.destroy()
        self.status_var.set(self.t("ready"))
        self.build_ui()
        self.bind_shortcuts()

    def start_update_check(self) -> None:
        self.status_var.set(self.t("checking_updates"))
        thread = threading.Thread(target=self.update_ytdlp_worker, daemon=True)
        thread.start()

    def update_ytdlp_worker(self) -> None:
        if yt_dlp is None:
            self.ui_queue.put(("status", self.t("missing_ytdlp")))
            return
        try:
            with yt_dlp.YoutubeDL({"quiet": True, "skip_download": True}) as ydl:
                from yt_dlp.update import run_update

                run_update(ydl)
            if not getattr(sys, "frozen", False):
                subprocess.run(
                    [sys.executable, "-m", "pip", "install", "--upgrade", "yt-dlp"],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    check=False,
                )
            self.ui_queue.put(("status", self.t("updates_ok")))
        except Exception as exc:
            self.ui_queue.put(("status", self.t("updates_failed", error=exc)))

    def build_ui(self) -> None:
        self.root.columnconfigure(0, weight=1)
        self.root.rowconfigure(0, weight=1)

        self.content = ttk.Frame(self.root, padding=8)
        self.content.grid(row=0, column=0, sticky="nsew")
        self.content.columnconfigure(0, weight=1)
        self.content.rowconfigure(0, weight=1)

        status = ttk.Label(self.root, textvariable=self.status_var, anchor=W)
        status.grid(row=1, column=0, sticky="ew", padx=8, pady=(0, 6))
        self.show_main_menu()

    def clear_content(self) -> None:
        for child in self.content.winfo_children():
            child.destroy()

    def show_main_menu(self) -> None:
        self.clear_content()
        self.content.rowconfigure(1, weight=1)
        ttk.Label(self.content, text=self.t("main_menu")).grid(row=0, column=0, sticky=W)
        self.main_menu_items = [
            (self.t("menu_search"), self.show_search_page),
            (self.t("menu_download_folder"), self.choose_download_folder),
            (self.t("menu_favorites"), self.show_favorites_page),
            (self.t("menu_settings"), self.show_settings_page),
            (self.t("menu_exit"), self.root.destroy),
        ]
        self.main_menu_list = Listbox(self.content, selectmode=SINGLE, exportselection=False)
        self.main_menu_list.grid(row=1, column=0, sticky="nsew", pady=6)
        for label, _action in self.main_menu_items:
            self.main_menu_list.insert(END, label)
        self.main_menu_list.selection_set(0)
        self.main_menu_list.activate(0)
        self.main_menu_list.bind("<Return>", lambda _event: self.activate_main_menu())
        self.main_menu_list.bind("<Double-1>", lambda _event: self.activate_main_menu())
        ttk.Button(self.content, text=self.t("open"), command=self.activate_main_menu).grid(
            row=2, column=0, sticky=W
        )
        self.root.after(100, self.main_menu_list.focus_set)

    def activate_main_menu(self) -> None:
        selected = self.main_menu_list.curselection()
        if not selected:
            return
        self.main_menu_items[int(selected[0])][1]()

    def show_search_page(self) -> None:
        self.clear_content()
        self.search_tab = ttk.Frame(self.content, padding=0)
        self.search_tab.grid(row=0, column=0, sticky="nsew")
        self.content.rowconfigure(0, weight=1)
        self.search_tab.columnconfigure(0, weight=1)
        self.search_tab.rowconfigure(3, weight=1)
        ttk.Button(self.search_tab, text=self.t("back_menu"), command=self.show_main_menu).grid(
            row=0, column=0, sticky=W, pady=(0, 6)
        )
        self.build_search_tab(start_row=1)

    def show_downloads_page(self) -> None:
        self.clear_content()
        self.downloads_tab = ttk.Frame(self.content, padding=0)
        self.downloads_tab.grid(row=0, column=0, sticky="nsew")
        self.content.rowconfigure(0, weight=1)
        self.downloads_tab.columnconfigure(0, weight=1)
        self.downloads_tab.rowconfigure(2, weight=1)
        ttk.Button(self.downloads_tab, text=self.t("back_menu"), command=self.show_main_menu).grid(
            row=0, column=0, sticky=W, pady=(0, 6)
        )
        self.build_downloads_tab(start_row=1)

    def show_favorites_page(self) -> None:
        self.clear_content()
        self.favorites_tab = ttk.Frame(self.content, padding=0)
        self.favorites_tab.grid(row=0, column=0, sticky="nsew")
        self.content.rowconfigure(0, weight=1)
        self.favorites_tab.columnconfigure(0, weight=1)
        self.favorites_tab.rowconfigure(2, weight=1)
        ttk.Button(self.favorites_tab, text=self.t("back_menu"), command=self.show_main_menu).grid(
            row=0, column=0, sticky=W, pady=(0, 6)
        )
        self.build_favorites_tab(start_row=1)

    def show_settings_page(self) -> None:
        self.clear_content()
        self.settings_tab = ttk.Frame(self.content, padding=0)
        self.settings_tab.grid(row=0, column=0, sticky="nsew")
        self.content.rowconfigure(0, weight=1)
        self.settings_tab.columnconfigure(0, weight=1)
        ttk.Button(self.settings_tab, text=self.t("back_menu"), command=self.show_main_menu).grid(
            row=0, column=0, sticky=W, pady=(0, 6)
        )
        self.build_settings_tab(start_row=1)

    def build_search_tab(self, start_row: int = 0) -> None:
        tab = self.search_tab
        tab.columnconfigure(0, weight=1)
        tab.rowconfigure(start_row + 2, weight=1)

        form = ttk.Frame(tab)
        form.grid(row=start_row, column=0, sticky="ew", pady=(0, 8))
        form.columnconfigure(1, weight=1)

        ttk.Label(form, text=self.t("query")).grid(row=0, column=0, sticky=W)
        self.query_var = StringVar()
        self.query_entry = ttk.Entry(form, textvariable=self.query_var)
        self.query_entry.grid(row=0, column=1, sticky="ew", padx=6)
        self.query_entry.bind("<Return>", lambda _event: self.search())

        ttk.Label(form, text=self.t("type")).grid(row=0, column=2, sticky=W)
        self.search_type_var = StringVar(value="Video")
        self.search_type_combo = ttk.Combobox(
            form,
            textvariable=self.search_type_var,
            state="readonly",
            values=self.search_type_labels(),
            width=12,
        )
        self.search_type_combo.grid(row=0, column=3, sticky="ew", padx=6)

        self.search_button = ttk.Button(form, text=self.t("search"), command=self.search)
        self.search_button.grid(row=0, column=4, sticky="ew")

        controls = ttk.Frame(tab)
        controls.grid(row=start_row + 1, column=0, sticky="ew", pady=(0, 8))
        ttk.Button(controls, text=self.t("play"), command=self.play_selected).pack(side=LEFT)
        ttk.Button(controls, text=self.t("download_video"), command=self.download_video).pack(
            side=LEFT, padx=4
        )
        ttk.Button(controls, text=self.t("download_audio"), command=self.download_audio).pack(
            side=LEFT
        )
        ttk.Button(controls, text=self.t("add_favorite"), command=self.add_selected_favorite).pack(
            side=LEFT, padx=4
        )

        self.results_list = Listbox(tab, selectmode=SINGLE, exportselection=False)
        self.results_list.grid(row=start_row + 2, column=0, sticky="nsew")
        scroll = ttk.Scrollbar(tab, orient=VERTICAL, command=self.results_list.yview)
        self.results_list.configure(yscrollcommand=scroll.set)
        scroll.grid(row=start_row + 2, column=1, sticky="ns")

        self.results_list.bind("<Double-1>", lambda _event: self.play_selected())
        self.results_list.bind("<Return>", lambda _event: self.play_selected())
        self.results_list.bind("<Button-3>", self.open_context_menu)
        self.results_list.bind("<Shift-F10>", self.open_context_menu)
        self.results_list.bind("<Menu>", self.open_context_menu)

        self.context_menu = self.make_context_menu()
        self.query_entry.focus_set()

    def build_downloads_tab(self, start_row: int = 0) -> None:
        tab = self.downloads_tab
        tab.columnconfigure(0, weight=1)
        tab.rowconfigure(start_row + 1, weight=1)

        folder_row = ttk.Frame(tab)
        folder_row.grid(row=start_row, column=0, sticky="ew", pady=(0, 8))
        folder_row.columnconfigure(1, weight=1)
        ttk.Label(folder_row, text=self.t("download_folder")).grid(row=0, column=0, sticky=W)
        self.download_folder_var = StringVar(value=self.settings.download_folder)
        folder_entry = ttk.Entry(folder_row, textvariable=self.download_folder_var)
        folder_entry.grid(row=0, column=1, sticky="ew", padx=6)
        ttk.Button(folder_row, text=self.t("choose_folder"), command=self.choose_download_folder).grid(
            row=0, column=2
        )

        self.download_log = Listbox(tab, selectmode=SINGLE, exportselection=False)
        self.download_log.grid(row=start_row + 1, column=0, sticky="nsew")

    def build_favorites_tab(self, start_row: int = 0) -> None:
        tab = self.favorites_tab
        tab.columnconfigure(0, weight=1)
        tab.rowconfigure(start_row + 1, weight=1)

        row = ttk.Frame(tab)
        row.grid(row=start_row, column=0, sticky="ew", pady=(0, 8))
        ttk.Button(row, text=self.t("play"), command=self.play_favorite).pack(side=LEFT)
        ttk.Button(row, text=self.t("remove"), command=self.remove_favorite).pack(side=LEFT, padx=4)
        ttk.Button(row, text=self.t("refresh"), command=self.refresh_favorites).pack(side=LEFT)

        self.favorites_list = Listbox(tab, selectmode=SINGLE, exportselection=False)
        self.favorites_list.grid(row=start_row + 1, column=0, sticky="nsew")
        self.favorites_list.bind("<Return>", lambda _event: self.play_favorite())
        self.favorites_list.bind("<Double-1>", lambda _event: self.play_favorite())
        self.refresh_favorites()

    def build_settings_tab(self, start_row: int = 0) -> None:
        tab = self.settings_tab
        tab.columnconfigure(0, weight=1)
        tab.rowconfigure(start_row, weight=1)

        self.limit_var = IntVar(value=self.settings.results_limit)
        self.language_var = StringVar(value=self.language_label())
        self.audio_format_var = StringVar(value=self.settings.audio_format)
        self.video_format_var = StringVar(value=self.settings.video_format)
        self.max_video_height_var = IntVar(value=self.settings.max_video_height)
        self.player_command_var = StringVar(value=self.settings.player_command)
        self.autoplay_next_var = BooleanVar(value=self.settings.autoplay_next)
        self.browser_playback_var = BooleanVar(value=self.settings.prefer_browser_playback)
        self.player_fullscreen_var = BooleanVar(value=self.settings.player_fullscreen)
        self.player_start_paused_var = BooleanVar(value=self.settings.player_start_paused)
        self.player_speed_var = StringVar(value=self.settings.player_speed)
        self.quiet_downloads_var = BooleanVar(value=self.settings.quiet_downloads)
        self.playlist_order_var = BooleanVar(value=self.settings.keep_playlist_order)
        self.filename_template_var = StringVar(value=self.settings.filename_template)
        self.audio_quality_var = StringVar(value=self.settings.audio_quality)
        self.seek_seconds_var = IntVar(value=self.settings.seek_seconds)
        self.volume_step_var = IntVar(value=self.settings.volume_step)
        self.write_thumbnail_var = BooleanVar(value=self.settings.write_thumbnail)
        self.write_description_var = BooleanVar(value=self.settings.write_description)
        self.write_info_json_var = BooleanVar(value=self.settings.write_info_json)
        self.write_subtitles_var = BooleanVar(value=self.settings.write_subtitles)
        self.auto_subtitles_var = BooleanVar(value=self.settings.auto_subtitles)
        self.subtitle_languages_var = StringVar(value=self.settings.subtitle_languages)
        self.embed_metadata_var = BooleanVar(value=self.settings.embed_metadata)
        self.embed_thumbnail_var = BooleanVar(value=self.settings.embed_thumbnail)
        self.restrict_filenames_var = BooleanVar(value=self.settings.restrict_filenames)
        self.open_folder_after_download_var = BooleanVar(
            value=self.settings.open_folder_after_download
        )
        self.auto_update_ytdlp_var = BooleanVar(value=self.settings.auto_update_ytdlp)
        self.confirm_before_download_var = BooleanVar(value=self.settings.confirm_before_download)
        self.download_archive_var = BooleanVar(value=self.settings.download_archive)
        self.rate_limit_var = StringVar(value=self.settings.rate_limit)
        self.proxy_var = StringVar(value=self.settings.proxy)
        self.cookies_file_var = StringVar(value=self.settings.cookies_file)
        self.ffmpeg_location_var = StringVar(value=self.settings.ffmpeg_location)
        self.concurrent_fragments_var = IntVar(value=self.settings.concurrent_fragments)
        self.retries_var = IntVar(value=self.settings.retries)
        self.socket_timeout_var = IntVar(value=self.settings.socket_timeout)

        settings_frame = ttk.Frame(tab)
        settings_frame.grid(row=start_row, column=0, sticky="nsew")
        settings_frame.columnconfigure(0, weight=1)

        general = ttk.LabelFrame(settings_frame, text=self.t("general"), padding=8)
        downloads = ttk.LabelFrame(settings_frame, text=self.t("downloads"), padding=8)
        player = ttk.LabelFrame(settings_frame, text=self.t("player"), padding=8)
        advanced = ttk.LabelFrame(settings_frame, text=self.t("advanced"), padding=8)
        for row, frame in enumerate((general, downloads, player, advanced)):
            frame.grid(row=row, column=0, sticky="ew", pady=4)
            frame.columnconfigure(1, weight=1)

        def field(parent, row: int, label: str, var) -> None:
            ttk.Label(parent, text=label).grid(row=row, column=0, sticky=W, pady=3)
            ttk.Entry(parent, textvariable=var).grid(row=row, column=1, sticky="ew", pady=3)

        def check(parent, row: int, text: str, var) -> None:
            ttk.Checkbutton(parent, text=text, variable=var).grid(
                row=row, column=0, columnspan=2, sticky=W, pady=3
            )

        ttk.Label(general, text=self.t("language")).grid(row=0, column=0, sticky=W, pady=3)
        ttk.Combobox(
            general,
            textvariable=self.language_var,
            values=("Slovenščina", "English"),
            state="readonly",
        ).grid(row=0, column=1, sticky="ew", pady=3)
        field(general, 1, self.t("results_limit"), self.limit_var)
        field(general, 2, self.t("seek_seconds"), self.seek_seconds_var)
        field(general, 3, self.t("volume_step"), self.volume_step_var)
        check(general, 4, self.t("autoplay_next"), self.autoplay_next_var)
        check(general, 5, self.t("confirm_download"), self.confirm_before_download_var)
        check(general, 6, self.t("open_after_download"), self.open_folder_after_download_var)
        check(general, 7, self.t("auto_update_ytdlp"), self.auto_update_ytdlp_var)

        field(downloads, 0, self.t("audio_format"), self.audio_format_var)
        field(downloads, 1, self.t("audio_quality"), self.audio_quality_var)
        field(downloads, 2, self.t("video_format"), self.video_format_var)
        field(downloads, 3, self.t("max_height"), self.max_video_height_var)
        field(downloads, 4, self.t("filename_template"), self.filename_template_var)
        field(downloads, 5, self.t("subtitle_langs"), self.subtitle_languages_var)
        check(downloads, 6, self.t("quiet_downloads"), self.quiet_downloads_var)
        check(downloads, 7, self.t("playlist_order"), self.playlist_order_var)
        check(downloads, 8, self.t("write_thumbnail"), self.write_thumbnail_var)
        check(downloads, 9, self.t("write_description"), self.write_description_var)
        check(downloads, 10, self.t("write_info_json"), self.write_info_json_var)
        check(downloads, 11, self.t("write_subtitles"), self.write_subtitles_var)
        check(downloads, 12, self.t("auto_subtitles"), self.auto_subtitles_var)
        check(downloads, 13, self.t("embed_metadata"), self.embed_metadata_var)
        check(downloads, 14, self.t("embed_thumbnail"), self.embed_thumbnail_var)
        check(downloads, 15, self.t("restrict_filenames"), self.restrict_filenames_var)
        check(downloads, 16, self.t("download_archive"), self.download_archive_var)

        field(player, 0, self.t("player_command"), self.player_command_var)
        field(player, 1, self.t("player_speed"), self.player_speed_var)
        check(player, 2, self.t("browser_playback"), self.browser_playback_var)
        check(player, 3, self.t("fullscreen"), self.player_fullscreen_var)
        check(player, 4, self.t("start_paused"), self.player_start_paused_var)
        ttk.Button(player, text=self.t("choose_player"), command=self.choose_player).grid(
            row=5, column=0, sticky=W, pady=6
        )

        field(advanced, 0, self.t("rate_limit"), self.rate_limit_var)
        field(advanced, 1, self.t("proxy"), self.proxy_var)
        field(advanced, 2, self.t("cookies"), self.cookies_file_var)
        field(advanced, 3, self.t("ffmpeg"), self.ffmpeg_location_var)
        field(advanced, 4, self.t("fragments"), self.concurrent_fragments_var)
        field(advanced, 5, self.t("retries"), self.retries_var)
        field(advanced, 6, self.t("timeout"), self.socket_timeout_var)
        ttk.Button(advanced, text=self.t("choose_cookies"), command=self.choose_cookies_file).grid(
            row=7, column=0, sticky=W, pady=6
        )
        ttk.Button(advanced, text=self.t("choose_ffmpeg"), command=self.choose_ffmpeg).grid(
            row=7, column=1, sticky=W, pady=6
        )

        buttons = ttk.Frame(tab)
        buttons.grid(row=start_row + 1, column=0, sticky="ew", pady=12)
        ttk.Button(buttons, text=self.t("save_settings"), command=self.save_settings_from_ui).pack(
            side=LEFT
        )
        ttk.Button(buttons, text=self.t("test_player"), command=self.test_player).pack(
            side=LEFT, padx=4
        )
        ttk.Button(buttons, text=self.t("open_data"), command=self.open_app_folder).pack(
            side=LEFT
        )

    def make_context_menu(self):
        menu = __import__("tkinter").Menu(self.root, tearoff=False)
        menu.add_command(label=self.t("play"), command=self.play_selected)
        menu.add_command(label=self.t("download_audio"), command=self.download_audio)
        menu.add_command(label=self.t("download_video"), command=self.download_video)
        menu.add_separator()
        menu.add_command(label=self.t("add_favorite"), command=self.add_selected_favorite)
        menu.add_command(label=self.t("copy_url"), command=self.copy_selected_url)
        menu.add_command(label=self.t("open_browser"), command=self.open_selected_in_browser)
        return menu

    def bind_shortcuts(self) -> None:
        self.root.bind("<Control-Key-1>", lambda _event: self.show_search_page())
        self.root.bind("<Control-Key-2>", lambda _event: self.show_downloads_page())
        self.root.bind("<Control-Key-3>", lambda _event: self.show_favorites_page())
        self.root.bind("<Control-Key-4>", lambda _event: self.show_settings_page())
        self.root.bind("<Escape>", self.handle_escape)
        self.root.bind("<Control-d>", lambda _event: self.download_video())
        self.root.bind("<Control-Shift-D>", lambda _event: self.download_audio())
        self.root.bind("<Control-Shift-d>", lambda _event: self.download_audio())
        self.root.bind("<Control-f>", lambda _event: self.add_selected_favorite())
        self.root.bind("<Control-s>", lambda _event: self.stop_player())
        self.root.bind_all("<space>", lambda event: self.player_key(event, "cycle pause"))
        self.root.bind_all("<Left>", lambda event: self.player_key(event, "seek -5"))
        self.root.bind_all("<Right>", lambda event: self.player_key(event, "seek 5"))
        self.root.bind_all("<Up>", lambda event: self.player_key(event, f"add volume {self.settings.volume_step}"))
        self.root.bind_all("<Down>", lambda event: self.player_key(event, f"add volume -{self.settings.volume_step}"))
        self.root.bind_all("<Control-Left>", lambda event: self.player_key(event, "seek -60"))
        self.root.bind_all("<Control-Right>", lambda event: self.player_key(event, "seek 60"))

    def handle_escape(self, _event=None) -> None:
        if self.player_control_mode and hasattr(self, "results_list"):
            self.player_control_mode = False
            self.results_list.focus_set()
            return "break"
        if self.player_process and self.player_process.poll() is None and hasattr(self, "results_list"):
            self.results_list.focus_set()
        else:
            self.show_main_menu()

    def player_key(self, event, command: str):
        if not self.player_control_mode:
            return None
        self.player_command(command)
        return "break"

    def search(self) -> None:
        if yt_dlp is None:
            self.show_missing_dependency()
            return
        query = self.query_var.get().strip()
        if not query:
            messagebox.showinfo(APP_NAME, self.t("enter_query"))
            return

        self.save_settings_from_ui(silent=True)
        search_type = self.search_type_code(self.search_type_var.get())
        self.set_busy(self.t("searching", query=query))
        self.search_button.configure(state="disabled")
        thread = threading.Thread(
            target=self.search_worker,
            args=(query, search_type, self.settings.results_limit),
            daemon=True,
        )
        thread.start()

    def search_worker(self, query: str, search_type: str, limit: int) -> None:
        try:
            options = {
                "quiet": True,
                "extract_flat": True,
                "skip_download": True,
                "default_search": "ytsearch",
                "playlistend": limit,
                "noplaylist": False,
            }
            with yt_dlp.YoutubeDL(options) as ydl:
                if search_type == "Video":
                    info = ydl.extract_info(f"ytsearch{limit}:{query}", download=False)
                else:
                    search_url = self.youtube_search_url(query, search_type)
                    info = ydl.extract_info(search_url, download=False)
            entries = list(info.get("entries") or [])
            entries = entries[:limit]
            normalized = [self.normalize_entry(entry, search_type) for entry in entries]
            self.ui_queue.put(("search_results", normalized))
        except Exception as exc:
            self.ui_queue.put(("error", f"Iskanje ni uspelo: {exc}"))

    def normalize_entry(self, entry: dict, search_type: str) -> dict:
        url = entry.get("webpage_url") or entry.get("url") or ""
        if url and not url.startswith("http"):
            ie_key = (entry.get("ie_key") or "").lower()
            if "playlist" in ie_key:
                url = f"https://www.youtube.com/playlist?list={url}"
            elif "tab" in ie_key or search_type in ("Kanal", "Channel"):
                url = f"https://www.youtube.com/{url.lstrip('/')}"
            else:
                url = f"https://www.youtube.com/watch?v={url}"
        display_type = "Channel" if self.settings.language == "en" and search_type == "Kanal" else search_type
        return {
            "title": entry.get("title") or "Brez naslova",
            "channel": entry.get("uploader") or entry.get("channel") or "",
            "views": self.format_count(entry.get("view_count")),
            "age": self.format_age(entry),
            "duration": self.format_duration(entry.get("duration")),
            "type": display_type,
            "url": url,
            "raw": entry,
        }

    @staticmethod
    def youtube_search_url(query: str, search_type: str) -> str:
        filters = {
            "Playlist": "EgIQAw==",
            "Kanal": "EgIQAg==",
            "Channel": "EgIQAg==",
        }
        params = urlencode(
            {
                "search_query": query,
                "sp": filters.get(search_type, ""),
            }
        )
        return f"https://www.youtube.com/results?{params}"

    def process_ui_queue(self) -> None:
        try:
            while True:
                kind, payload = self.ui_queue.get_nowait()
                if kind == "search_results":
                    self.show_results(payload)  # type: ignore[arg-type]
                elif kind == "download_log":
                    self.add_download_log(*payload)  # type: ignore[arg-type]
                elif kind == "status":
                    self.status_var.set(str(payload))
                elif kind == "error":
                    self.status_var.set(str(payload))
                    messagebox.showerror(APP_NAME, str(payload))
                elif kind == "info":
                    self.status_var.set(str(payload))
                    messagebox.showinfo(APP_NAME, str(payload))
                elif kind == "open_folder":
                    os.startfile(str(payload))  # type: ignore[attr-defined]
        except queue.Empty:
            pass
        self.root.after(100, self.process_ui_queue)

    def show_results(self, results: list[dict]) -> None:
        self.results = results
        self.results_list.delete(0, END)
        for index, item in enumerate(results):
            self.results_list.insert(END, self.result_line(index, item))
        if results:
            self.results_list.selection_set(0)
            self.results_list.activate(0)
            self.results_list.focus_set()
        self.search_button.configure(state="normal")
        self.status_var.set(self.t("found", count=len(results)))

    def get_selected_result(self) -> dict | None:
        selected = self.results_list.curselection()
        if not selected:
            return None
        try:
            self.current_index = int(selected[0])
            return self.results[self.current_index]
        except (IndexError, ValueError):
            return None

    def result_line(self, index: int, item: dict) -> str:
        parts = [
            f"{index + 1}. {item['title']}",
            f"{self.t('channel')} {item['channel']}",
            f"{self.t('views')} {item['views']}",
            f"{self.t('published')} {item['age']}",
            f"{self.t('duration')} {item['duration']}",
            f"{self.t('type').rstrip(':')} {item['type']}",
        ]
        return " | ".join(part for part in parts if not part.endswith(" "))

    def play_selected(self) -> None:
        item = self.get_selected_result()
        if not item:
            messagebox.showinfo(APP_NAME, self.t("select_video"))
            return
        self.play_url(item["url"], item["title"])

    def play_url(self, url: str, title: str = "") -> None:
        if not url:
            messagebox.showerror(APP_NAME, self.t("invalid_url"))
            return
        self.save_settings_from_ui(silent=True)
        if self.settings.prefer_browser_playback:
            webbrowser.open(url)
            self.status_var.set(self.t("opened_browser", title=title or url))
            return

        player = self.resolve_player()
        if player is None:
            webbrowser.open(url)
            self.status_var.set(self.t("no_player"))
            return

        self.stop_player(silent=True)
        command, kind = player
        try:
            if kind == "mpv":
                self.ipc_path = self.make_ipc_path()
                args = [
                    command,
                    "--force-window=yes",
                    f"--input-ipc-server={self.ipc_path}",
                    "--idle=no",
                    f"--speed={self.settings.player_speed}",
                ]
                if self.settings.player_fullscreen:
                    args.append("--fullscreen=yes")
                if self.settings.player_start_paused:
                    args.append("--pause=yes")
                args.append(url)
            elif kind == "vlc":
                self.ipc_path = None
                args = [command]
                if self.settings.player_fullscreen:
                    args.append("--fullscreen")
                if self.settings.player_start_paused:
                    args.append("--start-paused")
                args.append(url)
            else:
                self.ipc_path = None
                args = [command, url]
            self.player_process = subprocess.Popen(args)
            self.player_kind = kind
            self.player_control_mode = True
            self.status_var.set(self.t("playing", title=title or url))
        except Exception as exc:
            webbrowser.open(url)
            self.status_var.set(self.t("player_failed", error=exc))

    def resolve_player(self) -> tuple[str, str] | None:
        configured = self.settings.player_command.strip().strip('"')
        if configured:
            lower = configured.lower()
            if "mpv" in lower:
                return configured, "mpv"
            if "vlc" in lower:
                return configured, "vlc"
            return configured, "custom"
        mpv = shutil.which("mpv")
        if mpv:
            return mpv, "mpv"
        vlc = shutil.which("vlc")
        if vlc:
            return vlc, "vlc"
        return None

    def player_command(self, command: str) -> None:
        if self.player_kind != "mpv" or not self.ipc_path:
            return
        try:
            # Windows named pipe support is intentionally simple here.
            if os.name != "nt" and not os.path.exists(self.ipc_path):
                return
            payload = json.dumps({"command": shlex.split(command)}) + "\n"
            with open(self.ipc_path, "w", encoding="utf-8") as pipe:
                pipe.write(payload)
        except OSError:
            pass

    def play_previous(self) -> None:
        if not self.results:
            return
        self.current_index = max(0, self.current_index - 1)
        self.select_and_play_current()

    def play_next(self) -> None:
        if not self.results:
            return
        self.current_index = min(len(self.results) - 1, self.current_index + 1)
        self.select_and_play_current()

    def select_and_play_current(self) -> None:
        self.results_list.selection_clear(0, END)
        self.results_list.selection_set(self.current_index)
        self.results_list.activate(self.current_index)
        self.results_list.see(self.current_index)
        self.results_list.focus_set()
        self.play_selected()

    def stop_player(self, silent: bool = False) -> None:
        if self.player_process and self.player_process.poll() is None:
            self.player_process.terminate()
            if not silent:
                self.status_var.set(self.t("stopped"))
        self.player_process = None
        self.player_kind = ""
        self.player_control_mode = False

    def download_video(self) -> None:
        item = self.get_selected_result()
        if item:
            self.start_download(item, audio_only=False)

    def download_audio(self) -> None:
        item = self.get_selected_result()
        if item:
            self.start_download(item, audio_only=True)

    def start_download(self, item: dict, audio_only: bool) -> None:
        if yt_dlp is None:
            self.show_missing_dependency()
            return
        self.save_settings_from_ui(silent=True)
        action = "Audio" if audio_only else "Video"
        if self.settings.confirm_before_download:
            ok = messagebox.askyesno(
                APP_NAME,
                self.t("download_confirm", action=action.lower(), title=item["title"]),
            )
            if not ok:
                self.status_var.set(self.t("download_cancelled"))
                return
        self.add_download_log(action, item["title"], self.t("queued"))
        thread = threading.Thread(
            target=self.download_worker,
            args=(item, audio_only),
            daemon=True,
        )
        thread.start()

    def download_worker(self, item: dict, audio_only: bool) -> None:
        folder = Path(self.settings.download_folder)
        folder.mkdir(parents=True, exist_ok=True)
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
        }
        if self.settings.rate_limit.strip():
            options["ratelimit"] = self.settings.rate_limit.strip()
        if self.settings.proxy.strip():
            options["proxy"] = self.settings.proxy.strip()
        if self.settings.cookies_file.strip():
            options["cookiefile"] = self.settings.cookies_file.strip()
        if self.settings.ffmpeg_location.strip():
            options["ffmpeg_location"] = self.settings.ffmpeg_location.strip()
        if self.settings.download_archive:
            options["download_archive"] = str(APP_DIR / "download-archive.txt")
        if audio_only:
            options.update(
                {
                    "format": "bestaudio/best",
                    "postprocessors": [
                        {
                            "key": "FFmpegExtractAudio",
                            "preferredcodec": self.settings.audio_format,
                            "preferredquality": self.settings.audio_quality,
                        }
                    ],
                }
            )
        else:
            if self.settings.max_video_height > 0:
                options["format"] = (
                    f"bestvideo[height<={self.settings.max_video_height}]+bestaudio/"
                    f"best[height<={self.settings.max_video_height}]/{self.settings.video_format}"
                )
            else:
                options["format"] = self.settings.video_format
        try:
            with yt_dlp.YoutubeDL(options) as ydl:
                ydl.download([item["url"]])
            action = "Audio" if audio_only else "Video"
            self.ui_queue.put(("download_log", (action, item["title"], self.t("done"))))
            self.ui_queue.put(("status", self.t("download_done", title=item["title"])))
            if self.settings.open_folder_after_download:
                self.ui_queue.put(("open_folder", str(folder)))
        except Exception as exc:
            action = "Audio" if audio_only else "Video"
            self.ui_queue.put(("download_log", (action, item["title"], f"Napaka: {exc}")))
            self.ui_queue.put(("error", self.t("download_failed", error=exc)))

    def add_selected_favorite(self) -> None:
        item = self.get_selected_result()
        if not item:
            return
        favorite = {
            "title": item["title"],
            "channel": item["channel"],
            "url": item["url"],
        }
        if not any(existing["url"] == favorite["url"] for existing in self.favorites):
            self.favorites.append(favorite)
            self.save_favorites()
            self.refresh_favorites()
            self.status_var.set(self.t("favorite_added"))
        else:
            self.status_var.set(self.t("favorite_exists"))

    def play_favorite(self) -> None:
        item = self.get_selected_favorite()
        if item:
            self.play_url(item["url"], item["title"])

    def remove_favorite(self) -> None:
        selected = self.favorites_list.curselection()
        if not selected:
            return
        index = int(selected[0])
        del self.favorites[index]
        self.save_favorites()
        self.refresh_favorites()
        self.status_var.set(self.t("favorite_removed"))

    def get_selected_favorite(self) -> dict | None:
        selected = self.favorites_list.curselection()
        if not selected:
            return None
        try:
            return self.favorites[int(selected[0])]
        except (IndexError, ValueError):
            return None

    def refresh_favorites(self) -> None:
        self.favorites_list.delete(0, END)
        for index, item in enumerate(self.favorites):
            self.favorites_list.insert(
                END,
                f"{index + 1}. {item['title']} | {self.t('channel')} {item['channel']} | {item['url']}",
            )

    def copy_selected_url(self) -> None:
        item = self.get_selected_result()
        if not item:
            return
        self.root.clipboard_clear()
        self.root.clipboard_append(item["url"])
        self.status_var.set(self.t("url_copied"))

    def open_selected_in_browser(self) -> None:
        item = self.get_selected_result()
        if item:
            webbrowser.open(item["url"])

    def open_context_menu(self, event) -> None:
        if getattr(event, "num", None) == 3:
            index = self.results_list.nearest(getattr(event, "y", 0))
            if index >= 0:
                self.results_list.selection_clear(0, END)
                self.results_list.selection_set(index)
                self.results_list.activate(index)
        x = getattr(event, "x_root", self.root.winfo_pointerx())
        y = getattr(event, "y_root", self.root.winfo_pointery())
        self.context_menu.tk_popup(x, y)

    def choose_download_folder(self) -> None:
        current = (
            self.download_folder_var.get()
            if hasattr(self, "download_folder_var")
            else self.settings.download_folder
        )
        folder = filedialog.askdirectory(initialdir=current)
        if folder:
            if hasattr(self, "download_folder_var"):
                self.download_folder_var.set(folder)
                self.save_settings_from_ui()
            else:
                self.settings.download_folder = folder
                SETTINGS_FILE.write_text(
                    json.dumps(asdict(self.settings), indent=2, ensure_ascii=False),
                    encoding="utf-8",
                )
                self.status_var.set(self.t("settings_saved"))

    def choose_player(self) -> None:
        path = filedialog.askopenfilename(
            title="Izberi predvajalnik",
            filetypes=(("Programi", "*.exe"), ("Vse datoteke", "*.*")),
        )
        if path:
            self.player_command_var.set(path)
            self.save_settings_from_ui()

    def choose_cookies_file(self) -> None:
        path = filedialog.askopenfilename(
            title="Izberi cookies file",
            filetypes=(("Text files", "*.txt"), ("Vse datoteke", "*.*")),
        )
        if path:
            self.cookies_file_var.set(path)
            self.save_settings_from_ui()

    def choose_ffmpeg(self) -> None:
        path = filedialog.askopenfilename(
            title="Izberi ffmpeg.exe",
            filetypes=(("Programi", "*.exe"), ("Vse datoteke", "*.*")),
        )
        if path:
            self.ffmpeg_location_var.set(path)
            self.save_settings_from_ui()

    def read_int(self, var, default: int, minimum: int = 0) -> int:
        try:
            value = int(var.get())
        except (TypeError, ValueError):
            value = default
        return max(minimum, value)

    def save_settings_from_ui(self, silent: bool = False) -> None:
        old_language = self.settings.language
        self.settings = Settings(
            language=self.language_code(self.language_var.get()),
            download_folder=self.download_folder_var.get().strip()
            or str(Path.home() / "Downloads"),
            results_limit=self.read_int(self.limit_var, 20, 1),
            audio_format=self.audio_format_var.get().strip() or "mp3",
            video_format=self.video_format_var.get().strip() or "bestvideo+bestaudio/best",
            max_video_height=self.read_int(self.max_video_height_var, 1080, 0),
            player_command=self.player_command_var.get().strip(),
            autoplay_next=bool(self.autoplay_next_var.get()),
            prefer_browser_playback=bool(self.browser_playback_var.get()),
            player_fullscreen=bool(self.player_fullscreen_var.get()),
            player_start_paused=bool(self.player_start_paused_var.get()),
            player_speed=self.player_speed_var.get().strip() or "1.0",
            quiet_downloads=bool(self.quiet_downloads_var.get()),
            keep_playlist_order=bool(self.playlist_order_var.get()),
            filename_template=self.filename_template_var.get().strip()
            or "%(title)s [%(id)s].%(ext)s",
            audio_quality=self.audio_quality_var.get().strip() or "0",
            seek_seconds=self.read_int(self.seek_seconds_var, 10, 1),
            volume_step=self.read_int(self.volume_step_var, 5, 1),
            write_thumbnail=bool(self.write_thumbnail_var.get()),
            write_description=bool(self.write_description_var.get()),
            write_info_json=bool(self.write_info_json_var.get()),
            write_subtitles=bool(self.write_subtitles_var.get()),
            auto_subtitles=bool(self.auto_subtitles_var.get()),
            subtitle_languages=self.subtitle_languages_var.get().strip() or "sl,en",
            embed_metadata=bool(self.embed_metadata_var.get()),
            embed_thumbnail=bool(self.embed_thumbnail_var.get()),
            restrict_filenames=bool(self.restrict_filenames_var.get()),
            open_folder_after_download=bool(self.open_folder_after_download_var.get()),
            auto_update_ytdlp=bool(self.auto_update_ytdlp_var.get()),
            confirm_before_download=bool(self.confirm_before_download_var.get()),
            download_archive=bool(self.download_archive_var.get()),
            rate_limit=self.rate_limit_var.get().strip(),
            proxy=self.proxy_var.get().strip(),
            cookies_file=self.cookies_file_var.get().strip(),
            ffmpeg_location=self.ffmpeg_location_var.get().strip(),
            concurrent_fragments=self.read_int(self.concurrent_fragments_var, 4, 1),
            retries=self.read_int(self.retries_var, 10, 0),
            socket_timeout=self.read_int(self.socket_timeout_var, 20, 1),
        )
        SETTINGS_FILE.write_text(
            json.dumps(asdict(self.settings), indent=2, ensure_ascii=False),
            encoding="utf-8",
        )
        if self.settings.language != old_language:
            self.rebuild_ui()
        if not silent:
            self.status_var.set(self.t("settings_saved"))

    def load_settings(self) -> Settings:
        if SETTINGS_FILE.exists():
            try:
                data = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
                return Settings(**{**asdict(Settings()), **data})
            except Exception:
                return Settings()
        return Settings()

    def load_favorites(self) -> list[dict]:
        if FAVORITES_FILE.exists():
            try:
                data = json.loads(FAVORITES_FILE.read_text(encoding="utf-8"))
                if isinstance(data, list):
                    return data
            except Exception:
                pass
        return []

    def save_favorites(self) -> None:
        FAVORITES_FILE.write_text(
            json.dumps(self.favorites, indent=2, ensure_ascii=False),
            encoding="utf-8",
        )

    def add_download_log(self, action: str, title: str, status: str) -> None:
        now = time.strftime("%H:%M:%S")
        self.download_log.insert(
            END,
            f"{now} | {self.t('action')} {action} | {self.t('title')} {title} | {self.t('status')} {status}",
        )
        self.download_log.see(END)

    def set_busy(self, text: str) -> None:
        self.status_var.set(text)
        self.root.update_idletasks()

    def test_player(self) -> None:
        self.save_settings_from_ui(silent=True)
        player = self.resolve_player()
        if player:
            messagebox.showinfo(APP_NAME, self.t("player_found", player=player[0]))
        else:
            messagebox.showinfo(APP_NAME, self.t("player_not_found"))

    def open_app_folder(self) -> None:
        os.startfile(APP_DIR)  # type: ignore[attr-defined]

    def show_missing_dependency(self) -> None:
        messagebox.showerror(
            APP_NAME,
            self.t("missing_ytdlp"),
        )

    @staticmethod
    def parse_csv(value: str) -> list[str]:
        return [part.strip() for part in value.split(",") if part.strip()]

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
        try:
            seconds = int(seconds)
        except (TypeError, ValueError):
            return ""
        minutes, sec = divmod(seconds, 60)
        hours, minutes = divmod(minutes, 60)
        if hours:
            return f"{hours}:{minutes:02d}:{sec:02d}"
        return f"{minutes}:{sec:02d}"

    @staticmethod
    def format_age(entry: dict) -> str:
        timestamp = entry.get("timestamp")
        if timestamp:
            diff = max(0, int(time.time()) - int(timestamp))
            units = (
                ("year", 31_536_000),
                ("month", 2_592_000),
                ("day", 86_400),
                ("hour", 3_600),
                ("minute", 60),
            )
            for name, size in units:
                if diff >= size:
                    amount = diff // size
                    return f"{amount} {name}{'' if amount == 1 else 's'} ago"
            return "just now"
        upload_date = entry.get("upload_date")
        if upload_date and len(str(upload_date)) == 8:
            return f"{upload_date[0:4]}-{upload_date[4:6]}-{upload_date[6:8]}"
        return ""

    @staticmethod
    def make_ipc_path() -> str:
        name = f"accessible-youtube-{os.getpid()}"
        if os.name == "nt":
            return rf"\\.\pipe\{name}"
        return f"/tmp/{name}.sock"


def main() -> int:
    root = Tk()
    AccessibleYouTubeApp(root)
    root.mainloop()
    return 0


if __name__ == "__main__":
    sys.exit(main())
