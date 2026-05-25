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

class SearchMixin:

    def clear_results_selection_update_suppression(self) -> None:
        self.results_selection_update_suppressed = False



    def exit_fullscreen_to_results(self) -> None:
        if not self.player_is_active():
            self.back_to_results(stop_playback=False)
            return
        self.exit_fullscreen_window()
        self.show_player_page(self.current_player_title(), focus_target="results")
        wx.CallAfter(self.focus_results_list, self.return_index)
        wx.CallLater(100, self.focus_results_list, self.return_index)
        wx.CallLater(300, self.focus_results_list, self.return_index)



    def open_search_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_search)



    def show_search(self, restore_search: bool = False) -> None:
        self.last_activated_menu_action = self.show_search
        self.in_player_screen = False
        if not self.player_is_active():
            self.player_control_mode = False
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = True
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
        self.clear()
        if not restore_search:
            self.results = []
            self.all_results = []
            self.last_visible_count = 0
        self.add_background_player_section()
        self.add_button_row([(self.t("back"), self.back_from_search)])
        grid = wx.FlexGridSizer(3, 2, 6, 6)
        grid.AddGrowableCol(1, 1)
        grid.Add(wx.StaticText(self.panel, label=self.t("search_query")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.query = wx.TextCtrl(self.panel, style=wx.TE_PROCESS_ENTER)
        self.query.SetName(self.t("search_query"))
        if restore_search:
            self.query.SetValue(self.last_search_query)
        self.query.Bind(wx.EVT_TEXT_ENTER, lambda _evt: self.search())
        grid.Add(self.query, 1, wx.EXPAND)

        grid.Add(wx.StaticText(self.panel, label=self.t("search_provider")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.search_provider = wx.Choice(
            self.panel,
            choices=[self.t("youtube"), self.t("soundcloud")],
        )
        self.search_provider.SetName(self.t("search_provider"))
        restored_provider_index = getattr(self, "last_search_provider_index", 0) if restore_search else 0
        self.search_provider.SetSelection(restored_provider_index if 0 <= restored_provider_index < self.search_provider.GetCount() else 0)
        self.search_provider.Bind(wx.EVT_CHOICE, self.on_search_provider_change)
        grid.Add(self.search_provider, 1, wx.EXPAND)

        grid.Add(wx.StaticText(self.panel, label=self.t("type")), 0, wx.ALIGN_CENTER_VERTICAL)
        self.search_type = wx.Choice(
            self.panel,
            choices=[self.t("all"), self.t("video"), self.t("playlist"), self.t("channel")],
        )
        self.search_type.SetName(self.t("type"))
        restored_type_index = self.last_search_type_index if restore_search else 0
        self.search_type.SetSelection(restored_type_index if 0 <= restored_type_index < self.search_type.GetCount() else 0)
        grid.Add(self.search_type, 1, wx.EXPAND)

        if self.search_provider.GetSelection() == 1:
            self.search_type.Disable()
        else:
            self.search_type.Enable()

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
        self.results_list = wx.ListBox(self.panel, choices=[self.t("search_results_empty")])
        self.results_list.SetName(self.t("result_list"))
        self.results_list.SetSelection(0)
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        if not restore_search:
            self.focus_later(self.query)

    def on_search_provider_change(self, event=None) -> None:
        idx = self.search_provider.GetSelection()
        if idx == 1:
            self.search_type.Disable()
        else:
            self.search_type.Enable()

    def back_from_search(self) -> None:
        if self.search_results_stack:
            self.restore_previous_search_results()
        else:
            self.show_main_menu()


    def on_results_key(self, event: wx.KeyEvent) -> None:
        try:
            if self.result_details_key(event):
                self.announce_selected_result_details()
            elif self.shortcut_matches(event, "queue_audio"):
                self.toggle_download_queue()
            elif self.shortcut_matches(event, "add_to_playback_queue"):
                self.add_active_to_playback_queue()
            elif self.shortcut_matches(event, "remove_from_playback_queue"):
                self.remove_active_from_playback_queue()
            elif self.shortcut_matches(event, "add_favorite"):
                self.add_selected_favorite()
            elif self.shortcut_matches(event, "remove_favorite"):
                self.remove_selected_favorite_shortcut()
            elif self.shortcut_matches(event, "add_to_playlist"):
                self.add_active_to_playlist()
            elif self.shortcut_matches(event, "remove_from_playlist"):
                self.remove_active_from_playlist()
            elif self.shortcut_matches(event, "download_audio"):
                self.download_audio_shortcut()
            elif self.shortcut_matches(event, "download_video"):
                self.download_video_shortcut()
            elif self.shortcut_matches(event, "subscribe_channel"):
                self.subscribe_shortcut()
            elif self.shortcut_matches(event, "unsubscribe_channel"):
                self.unsubscribe_shortcut()
            elif self.shortcut_matches(event, "open_channel"):
                self.open_item_channel(self.selected_result())
            elif self.shortcut_matches(event, "copy_link"):
                self.copy_selected_url()
            elif self.shortcut_matches(event, "open_selected"):
                self.play_selected()
            elif self.context_menu_shortcut_matches(event):
                self.open_context_menu()
            else:
                event.Skip()
                wx.CallAfter(self.maybe_extend_results)
        except Exception:
            pass



    def on_results_selection(self, event) -> None:
        event.Skip()
        selection = self.current_results_selection(-1)
        if not getattr(self, "results_selection_update_suppressed", False):
            self.remember_user_result_selection(selection)
        self.apply_deferred_result_line_updates(exclude_index=selection)
        self.maybe_extend_results()



    def search_podcasts(self) -> None:
        with wx.TextEntryDialog(self, self.t("podcast_search_query"), self.t("search_podcasts")) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            query = dialog.GetValue().strip()
        if not query:
            return
        self.announce_player(self.t("podcast_searching", query=query))
        threading.Thread(target=self.search_podcasts_worker, args=(query,), daemon=True).start()


    def search_podcasts_worker(self, query: str) -> None:
        try:
            provider = self.normalized_podcast_search_provider()
            if provider != PODCAST_DIRECTORY_PROVIDER_APPLE:
                provider = PODCAST_DIRECTORY_PROVIDER_APPLE
            limit = min(200, max(1, int(self.settings.podcast_search_limit or 20)))
            params = {
                "media": "podcast",
                "entity": "podcast",
                "term": query,
                "country": self.normalized_podcast_search_country(),
                "limit": str(limit),
            }
            url = f"https://itunes.apple.com/search?{urlencode(params)}"
            request = Request(url, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
            with self.open_url(request, timeout=30) as response:
                payload = json.loads(response.read().decode("utf-8", errors="replace"))
            results = [self.normalize_podcast_result(item) for item in payload.get("results") or []]
            results = [item for item in results if item.get("url")]
            self.ui_queue.put(("podcast_results", {"query": query, "results": results}))
        except Exception as exc:
            self.ui_queue.put(("announce", self.t("podcast_search_failed", error=self.friendly_error(exc))))



    def show_podcast_search_results(self, results: list[dict], query: str = "") -> None:
        self.podcast_search_results = list(results)
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = True
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_background_player_section()
        self.add_button_row(
            [
                (self.t("back"), self.show_rss_feeds),
                (self.t("add_podcast"), self.add_selected_podcast_result),
                (self.t("open_browser"), self.open_selected_in_browser),
            ]
        )
        label = wx.StaticText(self.panel, label=self.t("podcast_search_results"))
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.podcast_result_list = wx.ListBox(self.panel, choices=[])
        self.podcast_result_list.SetName(self.t("podcast_search_results"))
        self.podcast_result_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.add_selected_podcast_result())
        self.podcast_result_list.Bind(wx.EVT_CONTEXT_MENU, self.open_podcast_search_context_menu)
        self.podcast_result_list.Bind(wx.EVT_KEY_DOWN, self.on_podcast_search_key)
        self.root_sizer.Add(self.podcast_result_list, 1, wx.EXPAND | wx.ALL, 4)
        for item in self.podcast_search_results:
            self.podcast_result_list.Append(self.podcast_result_line(item))
        if self.podcast_search_results:
            self.podcast_result_list.SetSelection(0)
            self.set_status(self.t("podcast_search_done", count=len(self.podcast_search_results)))
        else:
            self.podcast_result_list.Append(self.t("podcast_search_empty"))
            self.podcast_result_list.SetSelection(0)
            self.set_status(self.t("podcast_search_empty"))
        self.panel.Layout()
        self.focus_later(self.podcast_result_list)



    def on_podcast_search_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.add_selected_podcast_result()
        elif self.context_menu_shortcut_matches(event):
            self.open_podcast_search_context_menu()
        else:
            event.Skip()


    def open_podcast_search_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("add_podcast"), self.add_selected_podcast_result),
            (self.t("open_browser"), self.open_selected_in_browser),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_podcast_result())),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            menu.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()



    def search_type_code(self) -> str:
        index = self.search_type.GetSelection()
        options = ("All", "Video", "Playlist", "Channel")
        return options[index] if 0 <= index < len(options) else "All"


    def search(self) -> None:
        if get_yt_dlp() is None:
            self.message(self.t("missing_ytdlp"), wx.ICON_ERROR)
            return
        query = self.query.GetValue().strip()
        if not query:
            self.message(self.t("enter_query"))
            return
        self.last_search_query = query
        self.last_search_type_index = self.search_type.GetSelection()
        self.last_search_provider_index = self.search_provider.GetSelection()
        self.current_search_type_code = self.search_type_code()
        self.current_search_provider = "youtube" if self.last_search_provider_index == 0 else "soundcloud"
        self.collection_url = ""
        self.collection_result_type = ""
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
        self.search_results_stack = []
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.set_status(self.t("searching", query=query))
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.search_worker, args=(query, self.current_search_type_code, self.initial_results_limit(), generation, self.current_search_provider), daemon=True).start()


    def effective_results_limit(self) -> int:
        return min(250, max(1, self.settings.results_limit))


    def initial_results_limit(self) -> int:
        return RESULTS_PAGE_SIZE if self.settings.results_limit == 0 else self.effective_results_limit()


    def max_results_limit(self) -> int:
        return DYNAMIC_RESULTS_MAX if self.settings.results_limit == 0 else self.effective_results_limit()


    def search_worker(self, query: str, search_type: str, limit: int, generation: int, provider: str = "youtube") -> None:
        try:
            options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": limit}
            if provider == "soundcloud":
                info = self.ydl_extract_info(f"scsearch{limit}:{query}", options, download=False, allow_cookie_retry=False)
            elif search_type == "Video":
                info = self.ydl_extract_info(f"ytsearch{limit}:{query}", options, download=False, allow_cookie_retry=False)
            else:
                info = self.ydl_extract_info(self.youtube_search_url(query, search_type), options, download=False, allow_cookie_retry=False)
            entries = list(info.get("entries") or [])[:limit]
            wx.CallAfter(self.show_results_if_current, generation, [self.normalize_entry(entry, search_type, provider) for entry in entries])
        except Exception as exc:
            wx.CallAfter(self.show_search_error_if_current, generation, self.friendly_error(exc, include_youtube_auth_hint=False))


    def show_results_if_current(self, generation: int, results: list[dict]) -> None:
        if generation == self.search_generation:
            self.show_results(results)


    def show_search_error_if_current(self, generation: int, error: str) -> None:
        if generation == self.search_generation:
            self.message(error, wx.ICON_ERROR)



    def normalize_entry(self, entry: dict, search_type: str, provider: str = "youtube") -> dict:
        url = entry.get("webpage_url") or entry.get("url") or ""
        ie_key = str(entry.get("ie_key") or "").lower()
        entry_type = str(entry.get("_type") or entry.get("result_type") or "").lower()
        url_text = str(url)
        is_soundcloud = provider == "soundcloud" or "soundcloud" in ie_key or "soundcloud" in url_text
        is_playlist = not is_soundcloud and (search_type == "Playlist" or "playlist" in ie_key or "playlist" in entry_type or "list=" in url_text)
        is_channel = not is_soundcloud and (
            search_type in {"Channel", "Kanal"}
            or "channel" in ie_key
            or "channel" in entry_type
            or ("tab" in ie_key and not is_playlist)
            or "/channel/" in url_text
            or url_text.startswith("@")
            or url_text.startswith("/@")
        )
        if is_channel:
            kind = "channel"
            display_type = self.t("channel")
        elif is_playlist:
            kind = "playlist"
            display_type = self.t("playlist")
        else:
            kind = "video"
            display_type = self.t("live_stream") if (not is_soundcloud and self.metadata_is_live_stream(entry)) else self.t("video")
        if url and not url.startswith("http"):
            if is_soundcloud:
                if "soundcloud.com" in url:
                    url = f"https://{url}"
                else:
                    url = f"https://soundcloud.com/{url}"
            else:
                if kind == "playlist":
                    clean_url = url.lstrip("/")
                    if url.startswith("/") or "list=" in clean_url:
                        url = f"https://www.youtube.com/{clean_url}"
                    else:
                        url = f"https://www.youtube.com/playlist?list={clean_url}"
                elif kind == "channel":
                    url = f"https://www.youtube.com/{url.lstrip('/')}"
                else:
                    url = f"https://www.youtube.com/watch?v={url}"
        timestamp = entry.get("timestamp") or entry.get("release_timestamp") or entry.get("modified_timestamp")
        upload_date = entry.get("upload_date")
        is_live = kind == "video" and (not is_soundcloud and self.metadata_is_live_stream(entry))
        age = self.t("live_now") if is_live else (self.format_age({"timestamp": timestamp, "upload_date": upload_date}) if kind == "video" else "")
        playlist_count = entry.get("playlist_count") or entry.get("n_entries") or entry.get("video_count") or entry.get("playlist_count_text")
        item = {
            "title": entry.get("title") or "",
            "id": entry.get("id") or "",
            "channel": entry.get("uploader") or entry.get("channel") or "",
            "channel_url": self.normalize_channel_url(entry),
            "channel_id": entry.get("channel_id") or entry.get("uploader_id") or "",
            "views": self.format_count(entry.get("view_count")),
            "view_count": entry.get("view_count"),
            "age": age or (self.t("uploaded_unknown") if kind == "video" else ""),
            "duration": self.format_duration(entry.get("duration")),
            "duration_seconds": entry.get("duration"),
            "timestamp": timestamp,
            "upload_date": upload_date,
            "description": entry.get("description") or "",
            "artist": entry.get("artist") or entry.get("creator") or "",
            "track": entry.get("track") or "",
            "album": entry.get("album") or "",
            "chapters": self.normalized_chapters(entry.get("chapters")),
            "type": display_type,
            "kind": kind,
            "playlist_count": playlist_count if kind == "playlist" else "",
            "url": url,
            "live_status": self.metadata_live_status(entry),
            "is_live": is_live,
        }
        return self.with_live_stream_display_fields(item, entry)


    def show_results(self, results: list[dict], selection: int = 0, visible_count: int | None = None, focus_results: bool = True) -> None:
        self.deferred_result_line_updates.clear()
        self.all_results = list(results)
        if self.settings.results_limit == 0:
            count = visible_count if visible_count is not None else min(RESULTS_PAGE_SIZE, len(self.all_results))
            self.last_visible_count = min(len(self.all_results), max(0, count))
            self.results = self.all_results[: self.last_visible_count]
        else:
            self.last_visible_count = len(self.all_results)
            self.results = list(self.all_results)
        if self.results:
            selected_index = min(max(0, selection), len(self.results) - 1)
            labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
            self.set_listbox_items(self.results_list, labels, selected_index)
            self.remember_user_result_selection(selected_index)
            if focus_results:
                self.safe_set_focus(self.results_list)
            self.set_status(self.t("found", count=len(self.results)))
            self.start_result_metadata_hydration()
        else:
            self.set_listbox_items(self.results_list, [self.t("no_results")], 0)
            if focus_results:
                self.safe_set_focus(self.results_list)
            self.set_status(self.t("no_results"))


    def current_results_selection(self, fallback: int = 0) -> int:
        if not hasattr(self, "results_list"):
            return max(0, fallback)
        try:
            selection = self.results_list.GetSelection()
        except RuntimeError:
            return max(0, fallback)
        if selection == wx.NOT_FOUND:
            return max(0, fallback)
        return max(0, int(selection))



    def search_return_data(self, index: int | None = None) -> dict:
        return {
            "index": self.current_index if index is None else int(index),
            "collection_url": str(self.collection_url or ""),
            "collection_result_type": str(self.collection_result_type or ""),
            "collection_sort_mode": str(self.collection_sort_mode or ""),
            "collection_channel_id": str(self.collection_channel_id or ""),
            "collection_fully_loaded": bool(self.collection_fully_loaded),
            "dynamic_fetch_enabled": bool(self.dynamic_fetch_enabled),
        }


    def restore_search_return_context(self, data: dict | None = None) -> None:
        data = data if isinstance(data, dict) else {}
        self.collection_url = str(data.get("collection_url") or "")
        self.collection_result_type = str(data.get("collection_result_type") or "")
        self.collection_sort_mode = str(data.get("collection_sort_mode") or "")
        self.collection_channel_id = str(data.get("collection_channel_id") or "")
        self.collection_fully_loaded = bool(data.get("collection_fully_loaded", False))
        self.dynamic_fetch_enabled = bool(data.get("dynamic_fetch_enabled", True))
        self.loading_more_results = False


    def maybe_extend_results(self) -> None:
        if not self.dynamic_fetch_enabled or self.settings.results_limit != 0 or not hasattr(self, "results_list"):
            return
        if not self.results and not self.all_results:
            return
        try:
            selection = self.results_list.GetSelection()
        except RuntimeError:
            return
        if selection == wx.NOT_FOUND:
            return
        if selection < len(self.results) - 1:
            return
        if len(self.results) >= len(self.all_results):
            self.fetch_more_dynamic_results(selection)
            return
        next_count = min(len(self.all_results), len(self.results) + RESULTS_PAGE_SIZE)
        current_selection = self.current_results_selection(selection)
        current_identity = self.result_identity_at(current_selection)
        previous_count = len(self.results)
        self.last_visible_count = next_count
        self.results = self.all_results[: self.last_visible_count]
        selected_index = self.result_index_for_identity(current_identity, current_selection, next_count)
        labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
        if not self.append_listbox_items(self.results_list, labels, previous_count, selected_index):
            self.set_listbox_items(self.results_list, labels, selected_index)
        self.remember_user_result_selection(selected_index)
        self.set_status(self.t("search_more_loaded", count=len(self.results)))
        self.start_result_metadata_hydration()


    def fetch_more_dynamic_results(self, selection: int) -> None:
        if self.loading_more_results:
            return
        if getattr(self, "collection_fully_loaded", False) and len(self.results) >= len(self.all_results):
            self.set_status(self.t("no_more_results"))
            return
        current_count = len(self.all_results)
        max_limit = self.max_results_limit()
        if max_limit and current_count >= max_limit:
            self.set_status(self.t("no_more_results"))
            return
        next_limit = current_count + RESULTS_PAGE_SIZE
        if max_limit:
            next_limit = min(max_limit, next_limit)
        self.loading_more_results = True
        self.set_status(self.t("loading_more_results"))
        generation = self.search_generation
        if self.collection_url:
            threading.Thread(
                target=self.load_collection_worker,
                args=(self.collection_url, self.collection_result_type or "Video", next_limit, selection, generation, self.collection_sort_mode),
                daemon=True,
            ).start()
        else:
            threading.Thread(target=self.search_more_worker, args=(self.last_search_query, self.current_search_type_code, next_limit, selection, generation, getattr(self, "current_search_provider", "youtube")), daemon=True).start()


    def search_more_worker(self, query: str, search_type: str, limit: int, selection: int, generation: int, provider: str = "youtube") -> None:
        try:
            options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": limit}
            if provider == "soundcloud":
                info = self.ydl_extract_info(f"scsearch{limit}:{query}", options, download=False, allow_cookie_retry=False)
            elif search_type == "Video":
                info = self.ydl_extract_info(f"ytsearch{limit}:{query}", options, download=False, allow_cookie_retry=False)
            else:
                info = self.ydl_extract_info(self.youtube_search_url(query, search_type), options, download=False, allow_cookie_retry=False)
            entries = list(info.get("entries") or [])[:limit]
            wx.CallAfter(self.show_more_results_if_current, generation, [self.normalize_entry(entry, search_type, provider) for entry in entries], selection)
        except Exception as exc:
            wx.CallAfter(self.dynamic_fetch_failed_if_current, generation, self.friendly_error(exc, include_youtube_auth_hint=False))


    def merge_dynamic_results(self, existing: list[dict], fetched: list[dict], anchor_identity: str = "") -> list[dict]:
        existing_items = [dict(item) for item in existing]
        fetched_items = [dict(item) for item in fetched]
        if not existing_items:
            return fetched_items
        merged = list(existing_items)
        seen = {identity for identity in (self.result_identity_for_item(item) for item in merged) if identity}
        ordered_fetched = list(fetched_items)
        if anchor_identity:
            anchor_index = next(
                (index for index, item in enumerate(fetched_items) if self.result_identity_for_item(item) == anchor_identity),
                -1,
            )
            if anchor_index >= 0:
                ordered_fetched = fetched_items[anchor_index + 1 :] + fetched_items[: anchor_index + 1]
        for item in ordered_fetched:
            identity = self.result_identity_for_item(item)
            if identity and identity in seen:
                continue
            merged.append(item)
            if identity:
                seen.add(identity)
        return merged


    def show_more_results(self, results: list[dict], selection: int) -> None:
        self.loading_more_results = False
        current_selection = self.current_results_selection(selection)
        current_identity = str(self.pending_player_next_current_url or "") if self.pending_player_next_after_dynamic_load else self.result_identity_at(current_selection)
        merged_results = self.merge_dynamic_results(self.all_results, results, current_identity)
        if len(merged_results) <= len(self.all_results):
            self.set_status(self.t("no_more_results"))
            if self.pending_player_next_after_dynamic_load:
                self.pending_player_next_after_dynamic_load = False
                self.pending_player_next_preserve_focus = False
                self.pending_player_next_current_url = ""
                self.announce_player(self.t("no_next_item"))
            return
        previous_count = len(self.results)
        self.all_results = list(merged_results)
        visible_count = min(len(self.all_results), max(previous_count, previous_count + RESULTS_PAGE_SIZE))
        self.last_visible_count = visible_count
        self.results = self.all_results[:visible_count]
        selected_index = self.result_index_for_identity(current_identity, current_selection, visible_count)
        labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
        if not self.append_listbox_items(self.results_list, labels, previous_count, selected_index):
            self.set_listbox_items(self.results_list, labels, selected_index)
        self.remember_user_result_selection(selected_index)
        self.set_status(self.t("search_more_loaded", count=len(self.results)))
        self.start_result_metadata_hydration()
        if self.player_return_screen in {"search", "trending"}:
            self.return_all_results = list(self.all_results)
            self.return_results = list(self.results)
            self.return_visible_count = self.last_visible_count
        self.finish_pending_player_next_after_dynamic_load()


    def show_more_results_if_current(self, generation: int, results: list[dict], selection: int) -> None:
        if generation == self.search_generation:
            self.show_more_results(results, selection)



    def sort_popular_results(self, results: list[dict]) -> list[dict]:
        return sorted(results, key=self.popular_result_sort_key, reverse=True)


    def dedupe_results_by_url(self, results: list[dict]) -> list[dict]:
        deduped: list[dict] = []
        seen: set[str] = set()
        for item in results:
            url = str(item.get("url") or item.get("webpage_url") or "").strip()
            key = url or str(item.get("title") or "")
            if not key or key in seen:
                continue
            seen.add(key)
            deduped.append(item)
        return deduped


    def push_search_state(self) -> None:
        if not self.search_screen_active or not self.results:
            return
        self.search_results_stack.append(
            {
                "screen": "trending" if getattr(self, "trending_screen_active", False) else "search",
                "results": list(self.results),
                "all_results": list(self.all_results or self.results),
                "index": max(0, self.current_index),
                "visible_count": self.last_visible_count or len(self.results),
                "query": self.last_search_query,
                "type_index": self.last_search_type_index,
                "search_type": self.current_search_type_code,
                "collection_url": self.collection_url,
                "collection_result_type": self.collection_result_type,
                "collection_sort_mode": self.collection_sort_mode,
                "collection_channel_id": getattr(self, "collection_channel_id", ""),
                "collection_fully_loaded": bool(getattr(self, "collection_fully_loaded", False)),
                "country_index": getattr(self, "last_trending_country_index", 0),
                "category_index": getattr(self, "last_trending_category_index", 0),
            }
        )


    def restore_previous_search_results(self) -> None:
        if not self.search_results_stack:
            self.show_main_menu()
            return
        state = self.search_results_stack.pop()
        if state.get("screen") == "subscriptions":
            self.show_subscriptions()
            return
        if state.get("screen") == "history":
            self.show_history()
            return
        if state.get("screen") == "favorites":
            self.show_favorites()
            return
        if state.get("screen") == "trending":
            results = list(state.get("all_results") or state.get("results") or [])
            selection = int(state.get("index") or 0)
            visible_count = int(state.get("visible_count") or len(results))
            country_index = int(state.get("country_index") or 0)
            category_index = int(state.get("category_index") or 0)
            self.loading_more_results = False
            self.dynamic_fetch_enabled = False
            self.metadata_hydration_urls.clear()
            self.search_generation += 1
            self.show_trending(auto_load=False, country_index=country_index, category_index=category_index)
            self.show_results(results, selection=selection, visible_count=visible_count)
            wx.CallAfter(self.focus_results_list, selection)
            return
        self.last_search_query = str(state.get("query") or self.last_search_query)
        self.last_search_type_index = int(state.get("type_index") or 0)
        self.current_search_type_code = str(state.get("search_type") or "All")
        self.collection_url = str(state.get("collection_url") or "")
        self.collection_result_type = str(state.get("collection_result_type") or "")
        self.collection_sort_mode = str(state.get("collection_sort_mode") or "")
        self.collection_channel_id = str(state.get("collection_channel_id") or "")
        self.collection_fully_loaded = bool(state.get("collection_fully_loaded", False))
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        results = list(state.get("all_results") or state.get("results") or [])
        selection = int(state.get("index") or 0)
        visible_count = int(state.get("visible_count") or len(results))
        self.show_search(restore_search=True)
        self.show_results(results, selection=selection, visible_count=visible_count)
        wx.CallAfter(self.focus_results_list, selection)



    def player_results_snapshot(self) -> tuple[list[dict], int, int]:
        results = list(self.return_all_results or self.all_results or self.return_results or self.results)
        if not results:
            return [], 0, 0
        visible_count = self.return_visible_count or len(self.return_results or self.results or results)
        visible_count = min(max(1, int(visible_count or len(results))), len(results))
        current_url = str((self.current_video_item or self.current_video_info or {}).get("url") or "")
        selection = min(max(0, self.return_index), visible_count - 1)
        if current_url:
            for index, item in enumerate(results[:visible_count]):
                if str(item.get("url") or "") == current_url:
                    selection = index
                    break
        return results, visible_count, selection


    def add_player_results_section(self) -> None:
        results, visible_count, selection = self.player_results_snapshot()
        if not results:
            return
        self.deferred_result_line_updates.clear()
        self.all_results = list(results)
        self.last_visible_count = visible_count
        self.results = self.all_results[:visible_count]
        label = wx.StaticText(self.panel, label=self.t("result_list"))
        label.SetName(self.t("result_list"))
        self.root_sizer.Add(label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 4)
        labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
        self.results_list = wx.ListBox(self.panel, choices=labels or [self.t("search_results_empty")])
        self.results_list.SetName(self.t("result_list"))
        if labels:
            self.results_list.SetSelection(min(max(0, selection), len(labels) - 1))
        self.results_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected())
        self.results_list.Bind(wx.EVT_CONTEXT_MENU, self.open_context_menu)
        self.results_list.Bind(wx.EVT_KEY_DOWN, self.on_results_key)
        self.results_list.Bind(wx.EVT_LISTBOX, self.on_results_selection)
        self.bind_player_navigation_control(self.results_list)
        self.root_sizer.Add(self.results_list, 1, wx.EXPAND | wx.ALL, 4)



    def back_to_results(self, stop_playback: bool = True) -> None:
        keep_playing = self.background_playback_enabled() and not stop_playback
        self.exit_fullscreen_window()
        self.in_player_screen = False
        self.player_control_mode = keep_playing and self.player_control_mode
        if not keep_playing:
            self.stop_player(silent=True)
        if self.player_return_screen == "rss_items":
            feed_index = int(self.player_return_data.get("feed_index", self.current_rss_feed_index) or 0)
            item_index = int(self.player_return_data.get("item_index", 0) or 0)
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_rss_items(feed_index, selection=item_index)
            return
        if self.player_return_screen == "history":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_history()
            return
        if self.player_return_screen == "user_playlist_items":
            playlist_index = int(self.player_return_data.get("playlist_index", self.current_user_playlist_index) or 0)
            item_index = int(self.player_return_data.get("item_index", 0) or 0)
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_user_playlist_items(playlist_index, selection=item_index)
            return
        if self.player_return_screen == "notification_center":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_notification_center()
            return
        if self.player_return_screen == "direct_link":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_direct_link()
            return
        if self.player_return_screen == "folder":
            folder = Path(str(self.player_return_data.get("folder") or self.last_search_query or Path.home())).expanduser()
            index = int(self.player_return_data.get("index", self.return_index) or 0)
            results = list(self.return_all_results or self.all_results or self.return_results or self.results)
            if not results:
                results = self.cached_local_folder_items(folder)
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            if results:
                self.show_local_media_folder(folder, results, selection=index)
            else:
                self.open_local_media_folder(str(folder))
            return
        if self.player_return_screen == "local_file":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_main_menu()
            return
        if self.player_return_screen == "playback_queue":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_main_menu()
            return
        if self.player_return_screen == "favorites":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_favorites()
            return
        if self.player_return_screen == "subscriptions":
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_subscriptions()
            return
        if self.player_return_screen == "trending":
            country_index = int(self.player_return_data.get("country_index", getattr(self, "last_trending_country_index", 0)) or 0)
            category_index = int(self.player_return_data.get("category_index", getattr(self, "last_trending_category_index", 0)) or 0)
            results = self.return_all_results or self.all_results or self.return_results or self.results
            visible_count = self.return_visible_count or len(self.return_results or self.results)
            index = min(max(0, self.return_index), max(0, len(results) - 1))
            if not keep_playing:
                self.player_return_screen = ""
                self.player_return_data = {}
            self.show_trending(auto_load=False, country_index=country_index, category_index=category_index)
            self.show_results(results, selection=index, visible_count=visible_count)
            wx.CallAfter(self.focus_results_list, index)
            return
        results = self.return_all_results or self.all_results or self.return_results or self.results
        if results:
            self.metadata_hydration_urls.clear()
            self.search_generation += 1
            self.restore_search_return_context(self.player_return_data)
            self.show_search(restore_search=True)
            self.show_results(results, selection=self.return_index, visible_count=self.return_visible_count)
            if results:
                index = min(max(0, self.return_index), len(results) - 1)
                self.current_index = index
                wx.CallAfter(self.focus_results_list, index)
        else:
            self.show_main_menu()


    def focus_results_list(self, index: int | None = None) -> None:
        if not hasattr(self, "results_list"):
            return
        try:
            if index is not None and self.results_list.GetCount():
                target = min(max(0, index), self.results_list.GetCount() - 1)
                if self.results_list.GetSelection() != target:
                    self.results_list.SetSelection(target)
            self.safe_set_focus(self.results_list)
        except RuntimeError:
            pass



    def focus_in_results_control(self, focus: wx.Window | None) -> bool:
        return self.window_is_or_descendant(focus, getattr(self, "results_list", None))



    @staticmethod
    def results_list_native_navigation_key(event: wx.KeyEvent) -> bool:
        if event.ControlDown() or event.AltDown():
            return False
        return event.GetKeyCode() in {
            wx.WXK_UP,
            wx.WXK_DOWN,
            wx.WXK_HOME,
            wx.WXK_END,
            wx.WXK_PAGEUP,
            wx.WXK_PAGEDOWN,
        }


    @staticmethod
    def results_list_owns_key(event: wx.KeyEvent) -> bool:
        if event.ControlDown() or event.AltDown():
            return False
        key = event.GetKeyCode()
        if key in {
            wx.WXK_UP,
            wx.WXK_DOWN,
            wx.WXK_LEFT,
            wx.WXK_RIGHT,
            wx.WXK_HOME,
            wx.WXK_END,
            wx.WXK_PAGEUP,
            wx.WXK_PAGEDOWN,
            wx.WXK_SPACE,
            wx.WXK_BACK,
            wx.WXK_DELETE,
        }:
            return True
        if key in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER, wx.WXK_ESCAPE}:
            return False
        try:
            unicode_key = event.GetUnicodeKey()
        except Exception:
            unicode_key = 0
        if unicode_key and chr(unicode_key).isprintable():
            return True
        return False



    def refresh_results_list_labels(self) -> None:
        if not hasattr(self, "results_list"):
            return
        try:
            selection = self.results_list.GetSelection()
            labels = [self.result_line(index, item) for index, item in enumerate(self.results)]
            if not labels:
                labels = [self.t("no_results")]
            if wx.Window.FindFocus() is self.results_list and self.results_list.GetCount() == len(labels):
                self.results_list.Freeze()
                try:
                    for index, label in enumerate(labels):
                        if index == selection:
                            if self.results_list.GetString(index) != label:
                                self.deferred_result_line_updates.add(index)
                            continue
                        if self.results_list.GetString(index) != label:
                            self.results_list.SetString(index, label)
                finally:
                    self.results_list.Thaw()
                return
            self.set_listbox_items(self.results_list, labels, selection)
        except RuntimeError:
            pass


