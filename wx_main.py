from __future__ import annotations

from apricot.ui.settings import SettingsMixin
from apricot.download.download import DownloaderMixin
from apricot.media.media import MediaMixin
from apricot.library.library import LibraryMixin
from apricot.search.search import SearchMixin
from apricot.player.volume import VolumeMixin
from apricot.network.cookies import CookiesMixin
from apricot.ui.dialogs import DialogsMixin
from apricot.ui.cookies import CookiesUI
from apricot.ui.downloads import DownloadsUI
from apricot.ui.equalizer import EqualizerUI
from apricot.ui.events import EventsUI
from apricot.ui.lists import ListsUI
from apricot.ui.menus import MenusUI
from apricot.ui.misc import MiscUI
from apricot.ui.player import PlayerUI
from apricot.ui.search import SearchUI
from apricot.ui.shortcuts import ShortcutsUI
from apricot.ui.system import SystemUI
from apricot.player.playback import PlaybackMixin
from apricot.player.mpv import MpvMixin
from apricot.network.youtube import YoutubeMixin
from apricot.system.registry import RegistryMixin
from apricot.updater.updater import AppUpdaterMixin
from apricot.data.manager import DataManagerMixin
from apricot.utils import UtilsMixin
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

from apricot.constants import *

class MainFrame(CookiesUI, DownloadsUI, EqualizerUI, EventsUI, ListsUI, MenusUI, MiscUI, PlayerUI, SearchUI, ShortcutsUI, SystemUI, SettingsMixin, DownloaderMixin, MediaMixin, LibraryMixin, SearchMixin, VolumeMixin, CookiesMixin, DialogsMixin, PlaybackMixin, MpvMixin, YoutubeMixin, RegistryMixin, AppUpdaterMixin, DataManagerMixin, UtilsMixin, wx.Frame):
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
        self.history_save_lock = threading.Lock()
        self.history_save_generation = 0
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
        self.stream_url_cache_lock = threading.Lock()
        self.stream_url_cache: dict[str, dict] = self.load_stream_url_cache()
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
        self.controls: dict = {}
        self.choice_values: dict = {}
        self.settings_control_order: list = []
        self.seek_hold_active = False
        self.seek_hold_generation = 0
        self.seek_hold_seconds = 0.0
        self.seek_hold_key_code = -1
        self.seek_hold_raw_key_code = -1
        self.seek_hold_ctrl = False
        self.seek_hold_shift = False
        self.seek_hold_alt = False
        self.seek_hold_call: wx.CallLater | None = None

        self.panel = wx.Panel(self)
        self.root_sizer = wx.BoxSizer(wx.VERTICAL)
        self.panel.SetSizer(self.root_sizer)
        self.status = self.CreateStatusBar()
        self.status.SetStatusText(self.t("ready"))

        self.Bind(wx.EVT_CHAR_HOOK, self.on_char_hook)
        self.Bind(wx.EVT_KEY_UP, self.on_player_key_up)
        self.Bind(wx.EVT_NAVIGATION_KEY, self.on_player_navigation_key)
        self.panel.Bind(wx.EVT_NAVIGATION_KEY, self.on_player_navigation_key)
        self.panel.Bind(wx.EVT_KEY_UP, self.on_player_key_up)
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
        import traceback
        try:
            frame = MainFrame(start_hidden_in_tray=tray_start)
        except Exception:
            try:
                APP_DIR.mkdir(parents=True, exist_ok=True)
                error_log_path = APP_DIR / "error.log"
            except Exception:
                error_log_path = Path("error.log")
            with open(error_log_path, "w", encoding="utf-8") as f:
                traceback.print_exc(file=f)
            raise
        self.SetTopWindow(frame)
        if tray_start:
            frame.Hide()
        else:
            frame.Show()
            if not startup_media_path:
                if update_relaunch:
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

