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

class PlaybackMixin:
    def playback_cookies_file_for_url(self, url: str) -> str:
        cookie_file = self.effective_cookies_file()
        if not cookie_file:
            return ""
        if not self.is_youtube_url(url):
            return cookie_file
        return cookie_file if self.cookies_file_has_youtube_login(cookie_file) else ""



    def player_is_active(self) -> bool:
        return self.player_kind == "mpv" and self.mpv_process_alive()



    def player_copy_reference_label_key(self) -> str:
        return "copy_path" if self.current_player_is_local_media() else "copy_link"



    def player_fullscreen_mode_active(self) -> bool:
        if self.player_fullscreen_session:
            return True
        return bool(getattr(self.settings, "player_fullscreen", False) and not self.player_fullscreen_results_override)



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



    def play_history_item(self) -> None:
        item = self.selected_history_item()
        if not item or not item.get("url"):
            self.announce_player(self.t("no_selection"))
            return
        self.open_library_item(dict(item), "history")



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



    def playlist_count_text(self, item: dict) -> str:
        raw_count = item.get("playlist_count") or item.get("n_entries") or item.get("video_count")
        if raw_count in (None, ""):
            return ""
        try:
            count = int(str(raw_count).replace(",", "").strip())
        except (TypeError, ValueError):
            return str(raw_count)
        return self.t("playlist_video_count", count=count)



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
            info = self.ydl_extract_info(str(item.get("url") or ""), options, download=False, allow_cookie_retry=False)
            entries = [entry for entry in list((info or {}).get("entries") or []) if isinstance(entry, dict)]
            playable = [
                result
                for result in (self.normalize_entry(entry, "Video") for entry in entries)
                if result.get("kind") == "video" and result.get("url")
            ]
            wx.CallAfter(self.start_playlist_playback_if_current, generation, item, playable, shuffle)
        except Exception as exc:
            wx.CallAfter(self.dynamic_fetch_failed_if_current, self.search_generation, self.friendly_error(exc))



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
        continuing_session = (
            bool(getattr(self, "player_session_open", False))
            or self.player_is_active()
            or bool(getattr(self, "playback_start_pending", False))
        )
        if continuing_session:
            self.player_session_open = True
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



    def playback_key(self, item: dict | None = None) -> str:
        item = item or self.current_video_item or self.current_video_info
        return str((item or {}).get("url") or (item or {}).get("webpage_url") or "").strip()



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
                self.player_process.wait(timeout=0.5)
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
            self.session_audio_output_device = ""
            self.session_autoplay_next = False
            self.session_equalizer_enabled = None
            self.session_equalizer_gains = {}
            self.session_equalizer_before_bass_boost = None
            self.volume_boost_enabled = False
            self.bass_boost_enabled = False
            self.repeat_current = False
            self.shuffle_current = False
            self.player_sequence_results = []
        else:
            self.player_session_open = True
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


