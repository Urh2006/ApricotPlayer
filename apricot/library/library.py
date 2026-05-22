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

class LibraryMixin:

    def open_selected_user_playlist(self) -> None:
        if self.selected_user_playlist() is None:
            self.announce_player(self.t("no_playlists"))
            return
        self.show_user_playlist_items(self.current_user_playlist_index)


    def show_user_playlist_items(self, playlist_index: int, selection: int = 0) -> None:
        if playlist_index < 0 or playlist_index >= len(self.user_playlists):
            self.show_user_playlists()
            return
        self.current_user_playlist_index = playlist_index
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = True
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_background_player_section()
        playlist = self.user_playlists[playlist_index]
        item_actions = [(self.t("back"), self.show_user_playlists)]
        if playlist.get("items"):
            item_actions.extend(
                [
                    (self.t("play"), self.play_selected_user_playlist_item),
                    (self.t("download_user_playlist"), self.download_current_user_playlist),
                    (self.t("remove_from_playlist"), self.remove_selected_user_playlist_item),
                ]
            )
        self.add_button_row(item_actions)
        label = wx.StaticText(self.panel, label=f"{self.t('playlist_items')}: {playlist.get('title', '')}")
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.user_playlist_items = list(playlist.get("items") or [])
        self.user_playlist_items_list = wx.ListBox(self.panel, choices=[])
        self.user_playlist_items_list.SetName(self.t("playlist_items"))
        self.user_playlist_items_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected_user_playlist_item())
        self.user_playlist_items_list.Bind(wx.EVT_CONTEXT_MENU, self.open_user_playlist_items_context_menu)
        self.user_playlist_items_list.Bind(wx.EVT_KEY_DOWN, self.on_user_playlist_items_key)
        self.root_sizer.Add(self.user_playlist_items_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_user_playlist_items(selection)
        self.panel.Layout()
        self.focus_later(self.user_playlist_items_list)


    def refresh_user_playlist_items(self, selection: int = 0) -> None:
        if not hasattr(self, "user_playlist_items_list"):
            return
        try:
            self.user_playlist_items = list(self.user_playlists[self.current_user_playlist_index].get("items") or [])
            if self.user_playlist_items:
                labels = [self.result_line(index, item) for index, item in enumerate(self.user_playlist_items)]
                self.set_listbox_items(self.user_playlist_items_list, labels, selection)
            else:
                self.set_listbox_items(self.user_playlist_items_list, [self.t("playlist_empty")], 0)
                self.set_status(self.t("playlist_empty"))
        except RuntimeError:
            pass


    def selected_user_playlist_item(self) -> dict | None:
        if not hasattr(self, "user_playlist_items_list"):
            return None
        index = self.user_playlist_items_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.user_playlist_items):
            return None
        return dict(self.user_playlist_items[index], user_playlist_index=self.current_user_playlist_index, user_playlist_item_index=index)


    def on_user_playlist_items_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.play_selected_user_playlist_item()
        elif self.shortcut_matches(event, "download_audio"):
            self.start_download(True, item=self.selected_user_playlist_item())
        elif self.shortcut_matches(event, "download_video"):
            self.start_download(False, item=self.selected_user_playlist_item())
        elif self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
        elif self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
        elif self.shortcut_matches(event, "open_channel"):
            self.open_item_channel(self.selected_user_playlist_item())
        elif self.shortcut_matches(event, "remove_from_playlist") or self.shortcut_matches(event, "remove_selected"):
            self.remove_selected_user_playlist_item()
        elif self.context_menu_shortcut_matches(event):
            self.open_user_playlist_items_context_menu()
        else:
            event.Skip()


    def open_user_playlist_items_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        selected = self.selected_user_playlist_item()
        if not selected:
            item = menu.Append(wx.ID_ANY, self.t("playlist_empty"))
            item.Enable(False)
            self.PopupMenu(menu)
            menu.Destroy()
            return
        actions = [
            (self.t("play"), self.play_selected_user_playlist_item),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda selected=dict(selected): self.start_download(True, item=selected)),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda selected=dict(selected): self.start_download(False, item=selected)),
            (self.t("download_user_playlist"), self.download_current_user_playlist),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playlist", "remove_from_playlist"), self.remove_selected_user_playlist_item),
            (self.t("copy_url"), lambda selected=dict(selected): self.copy_item_url(selected)),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda selected=dict(selected): self.copy_direct_stream_url(selected)),
        ]
        if self.item_has_openable_youtube_channel(selected):
            actions.insert(6, (self.menu_label_with_shortcut("open_channel", "open_channel"), lambda selected=dict(selected or {}): self.open_item_channel(selected)))
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def remove_selected_user_playlist_item(self) -> None:
        item = self.selected_user_playlist_item()
        if not item:
            self.announce_player(self.t("playlist_empty"))
            return
        index = int(item.get("user_playlist_item_index") or 0)
        playlist = self.user_playlists[self.current_user_playlist_index]
        items = list(playlist.get("items") or [])
        if 0 <= index < len(items):
            del items[index]
            playlist["items"] = items
            playlist["updated_at"] = time.time()
            self.save_user_playlists()
            if self.user_playlist_items_screen_active and not items:
                wx.CallAfter(self.show_user_playlist_items, self.current_user_playlist_index)
            else:
                self.refresh_user_playlist_items(min(index, len(items) - 1))
            self.announce_player(self.t("removed_from_playlist"))


    def remove_selected_user_playlist(self) -> None:
        if not hasattr(self, "user_playlist_list"):
            return
        index = self.user_playlist_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.user_playlists):
            self.announce_player(self.t("no_playlists"))
            return
        del self.user_playlists[index]
        self.current_user_playlist_index = min(index, len(self.user_playlists) - 1)
        self.save_user_playlists()
        if self.user_playlists_screen_active and not self.user_playlists:
            wx.CallAfter(self.show_user_playlists)
        else:
            self.refresh_user_playlists()
        self.announce_player(self.t("playlist_removed"))


    def download_selected_user_playlist(self) -> None:
        playlist = self.selected_user_playlist()
        if not playlist:
            self.announce_player(self.t("no_playlists"))
            return
        self.download_user_playlist(playlist)


    def download_current_user_playlist(self) -> None:
        if self.current_user_playlist_index < 0 or self.current_user_playlist_index >= len(self.user_playlists):
            self.announce_player(self.t("no_playlists"))
            return
        self.download_user_playlist(self.user_playlists[self.current_user_playlist_index])


    def download_user_playlist(self, playlist: dict) -> None:
        title = str(playlist.get("title") or self.t("playlists"))
        folder = str(self.music_download_folder() / self.safe_folder_name(title))
        items = [
            dict(item, audio_only=False, download_folder_override=folder)
            for item in list(playlist.get("items") or [])
            if item.get("url") and item.get("kind") != "local_file"
        ]
        if not items:
            self.announce_player(self.t("playlist_empty"))
            return
        self.announce_player(self.t("batch_download_start", count=len(items)))
        task_id, cancel_event = self.register_download_task({"title": title, "kind": "playlist"}, False, "batch", total=len(items))
        done_text = self.t("download_playlist_done", title=title)
        threading.Thread(target=self.download_batch_worker, args=(items, task_id, cancel_event, done_text, folder), daemon=True).start()


    def add_active_to_playlist(self, prefer_active: bool = False) -> None:
        items = self.playlist_candidate_items(prefer_active=prefer_active)
        if not items:
            self.message(self.t("no_selection"))
            return
        playlist_index = self.choose_or_create_playlist_index()
        if playlist_index is None:
            return
        self.add_items_to_playlist(playlist_index, items)

    def choose_or_create_playlist_index(self, initial_item: dict | None = None) -> int | None:
        if not self.user_playlists:
            return self.create_user_playlist_dialog(initial_item=initial_item)
        if len(self.user_playlists) == 1:
            return 0
        choices = [str(playlist.get("title") or "") for playlist in self.user_playlists]
        with wx.SingleChoiceDialog(self, self.t("select_playlist"), self.t("add_to_playlist"), choices) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return None
            index = dialog.GetSelection()
        return index if 0 <= index < len(self.user_playlists) else None


    def add_items_to_playlist(self, playlist_index: int, items: list[dict]) -> None:
        if playlist_index < 0 or playlist_index >= len(self.user_playlists):
            return
        playlist = self.user_playlists[playlist_index]
        existing_urls = {str(item.get("url") or "") for item in playlist.get("items") or []}
        added: list[dict] = []
        for item in items:
            url = str(item.get("url") or "")
            if not url or url in existing_urls:
                continue
            playlist_item = self.playlist_item_from_media(item)
            playlist.setdefault("items", []).append(playlist_item)
            existing_urls.add(url)
            added.append(playlist_item)
        self.clear_playlist_candidate_download_queue_items(items)
        if not added:
            self.announce_player(self.t("playlist_exists"))
            return
        playlist["updated_at"] = time.time()
        self.save_user_playlists()
        title = str(playlist.get("title") or "")
        if len(added) == 1:
            self.announce_player(self.t("added_to_playlist", playlist=title, title=added[0].get("title", "")))
        else:
            self.announce_player(self.t("added_to_playlist_count", playlist=title, count=len(added)))
        if self.user_playlist_items_screen_active:
            self.refresh_user_playlist_items()


    def remove_active_from_playlist(self) -> None:
        if self.user_playlist_items_screen_active:
            self.remove_selected_user_playlist_item()
            return
        item = self.active_item()
        if not self.playlist_item_is_supported(item):
            self.announce_player(self.t("no_selection"))
            return
        url = str(item.get("url") or "")
        matches: list[tuple[int, int, str]] = []
        for playlist_index, playlist in enumerate(self.user_playlists):
            for item_index, playlist_item in enumerate(list(playlist.get("items") or [])):
                if str(playlist_item.get("url") or "") == url:
                    matches.append((playlist_index, item_index, str(playlist.get("title") or "")))
                    break
        if not matches:
            self.announce_player(self.t("not_in_playlist"))
            return
        selected_match = matches[0]
        if len(matches) > 1:
            choices = [title or self.t("playlists") for _playlist_index, _item_index, title in matches]
            with wx.SingleChoiceDialog(self, self.t("select_playlist"), self.t("remove_from_playlist"), choices) as dialog:
                if dialog.ShowModal() != wx.ID_OK:
                    return
                selected_index = dialog.GetSelection()
            if 0 <= selected_index < len(matches):
                selected_match = matches[selected_index]
        playlist_index, item_index, _title = selected_match
        playlist = self.user_playlists[playlist_index]
        items = list(playlist.get("items") or [])
        if 0 <= item_index < len(items):
            del items[item_index]
            playlist["items"] = items
            playlist["updated_at"] = time.time()
            self.save_user_playlists()
            if self.user_playlist_items_screen_active and playlist_index == self.current_user_playlist_index:
                self.refresh_user_playlist_items(min(item_index, len(items) - 1))
            self.announce_player(self.t("removed_from_playlist"))


    def clear_playlist_candidate_download_queue_items(self, items: list[dict]) -> None:
        changed = False
        for item in items:
            url = str(item.get("url") or "")
            if url and url in self.download_queue and self.playlist_item_is_supported(self.download_queue.get(url)):
                self.download_queue.pop(url, None)
                changed = True
        if not changed:
            return
        self.refresh_results_list_labels()
        if self.in_queue_screen:
            self.refresh_queue_view()
        self.refresh_download_views()

    def append_add_to_playlist_menu(self, menu: wx.Menu, prefer_active: bool = False) -> None:
        if self.user_playlists:
            submenu = wx.Menu()
            for index, playlist in enumerate(self.user_playlists):
                menu_item = submenu.Append(wx.ID_ANY, str(playlist.get("title") or self.t("playlists")))
                self.Bind(wx.EVT_MENU, lambda _evt, idx=index, prefer=prefer_active: self.add_items_to_playlist(idx, self.playlist_candidate_items(prefer_active=prefer)), menu_item)
            create_item = submenu.Append(wx.ID_ANY, self.t("create_playlist"))
            self.Bind(wx.EVT_MENU, lambda _evt, prefer=prefer_active: self.add_active_to_playlist(prefer_active=prefer), create_item)
            menu.AppendSubMenu(submenu, self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"))
        else:
            item = menu.Append(wx.ID_ANY, self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"))
            self.Bind(wx.EVT_MENU, lambda _evt, prefer=prefer_active: self.add_active_to_playlist(prefer_active=prefer), item)



    def open_favorites_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_favorites)


    def open_playlists_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_user_playlists)


    def open_subscriptions_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_subscriptions)



    def open_history_shortcut(self) -> None:
        if self.settings.enable_history:
            self.run_global_navigation_shortcut(self.show_history)


    def open_podcasts_rss_shortcut(self) -> None:
        if self.settings.enable_podcasts_rss:
            self.run_global_navigation_shortcut(self.show_rss_feeds)


    def show_favorites(self) -> None:
        self.in_main_menu = False
        self.search_screen_active = False
        self.favorites_screen_active = True
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
        self.add_background_player_section()
        favorite_actions = [(self.t("back"), self.show_main_menu)]
        if self.favorites:
            favorite_actions.extend(
                [
                    (self.t("play"), self.play_favorite),
                    (self.t("remove"), self.remove_favorite),
                    (self.t("refresh"), self.refresh_favorites),
                ]
            )
        self.add_button_row(favorite_actions)
        self.favorites_list = wx.ListBox(self.panel, choices=[])
        self.favorites_list.SetName(self.t("favorites"))
        self.favorites_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_favorite())
        self.favorites_list.Bind(wx.EVT_CONTEXT_MENU, self.open_favorites_context_menu)
        self.favorites_list.Bind(wx.EVT_KEY_DOWN, self.on_favorites_key)
        self.root_sizer.Add(self.favorites_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_favorites()
        self.panel.Layout()
        self.focus_later(self.favorites_list)


    def on_favorites_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.play_favorite()
        elif self.shortcut_matches(event, "subscribe_channel"):
            self.subscribe_to_selected_channel(self.selected_favorite())
        elif self.shortcut_matches(event, "unsubscribe_channel"):
            self.unsubscribe_from_selected_channel(self.selected_favorite())
        elif self.shortcut_matches(event, "open_channel"):
            self.open_item_channel(self.selected_favorite())
        elif self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
        elif self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
        elif self.context_menu_shortcut_matches(event):
            self.open_favorites_context_menu()
        else:
            event.Skip()


    def open_favorites_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        selected = self.selected_favorite()
        if not selected:
            item = menu.Append(wx.ID_ANY, self.t("favorites_empty"))
            item.Enable(False)
            self.PopupMenu(menu)
            menu.Destroy()
            return
        actions = [
            (self.t("play"), self.play_favorite),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda selected=dict(selected): self.start_download(True, item=selected)),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda selected=dict(selected): self.start_download(False, item=selected)),
            (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), lambda selected=dict(selected): self.subscribe_to_selected_channel(selected)),
            (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), lambda selected=dict(selected): self.unsubscribe_from_selected_channel(selected)),
            (self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"), self.add_active_to_playlist),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda selected=dict(selected): self.copy_direct_stream_url(selected)),
            (self.t("copy_url"), lambda selected=dict(selected): self.copy_item_url(selected)),
            (self.t("remove"), self.remove_favorite),
        ]
        if self.item_has_openable_youtube_channel(selected):
            actions.insert(5, (self.menu_label_with_shortcut("open_channel", "open_channel"), lambda selected=dict(selected or {}): self.open_item_channel(selected)))
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()


    def show_history(self) -> None:
        if not self.settings.enable_history:
            self.show_main_menu()
            return
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = True
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
        self.add_background_player_section()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("play"), self.play_history_item),
                (self.t("remove_history_item"), self.remove_history_item),
                (self.t("clear_history"), self.clear_history),
            ]
        )
        self.history_list = wx.ListBox(self.panel, choices=[])
        self.history_list.SetName(self.t("history"))
        self.history_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_history_item())
        self.history_list.Bind(wx.EVT_CONTEXT_MENU, self.open_history_context_menu)
        self.history_list.Bind(wx.EVT_KEY_DOWN, self.on_history_key)
        self.root_sizer.Add(self.history_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_history()
        self.panel.Layout()
        self.focus_later(self.history_list)


    def refresh_history(self) -> None:
        if not hasattr(self, "history_list"):
            return
        try:
            if self.history:
                self.set_listbox_items(self.history_list, [self.history_line(item) for item in self.history], 0)
            else:
                self.set_listbox_items(self.history_list, [self.t("history_empty")], 0)
                self.set_status(self.t("history_empty"))
        except RuntimeError:
            pass


    def history_line(self, item: dict) -> str:
        when = self.format_history_time(item.get("timestamp"))
        action = str(item.get("action") or "")
        parts = [when, action, item.get("title", ""), f"{self.t('channel')}: {item.get('channel', '')}" if item.get("channel") else ""]
        return " | ".join(part for part in parts if part)


    def selected_history_item(self) -> dict | None:
        if not hasattr(self, "history_list"):
            return None
        index = self.history_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.history):
            return None
        return self.history[index]


    def on_history_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.play_history_item()
        elif self.shortcut_matches(event, "download_audio"):
            self.start_download(True, item=self.selected_history_item())
        elif self.shortcut_matches(event, "download_video"):
            self.start_download(False, item=self.selected_history_item())
        elif self.shortcut_matches(event, "subscribe_channel"):
            self.subscribe_to_selected_channel(self.selected_history_item())
        elif self.shortcut_matches(event, "unsubscribe_channel"):
            self.unsubscribe_from_selected_channel(self.selected_history_item())
        elif self.shortcut_matches(event, "open_channel"):
            self.open_item_channel(self.selected_history_item())
        elif self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
        elif self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
        elif self.shortcut_matches(event, "remove_selected"):
            self.remove_history_item()
        elif self.context_menu_shortcut_matches(event):
            self.open_history_context_menu()
        else:
            event.Skip()


    def open_history_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        selected = self.selected_history_item()
        actions = [
            (self.t("play"), self.play_history_item),
            (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.start_download(True, item=self.selected_history_item())),
            (self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.start_download(False, item=self.selected_history_item())),
            (self.t("add_favorite"), lambda: self.add_favorite_item(self.selected_history_item())),
            (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), lambda: self.subscribe_to_selected_channel(self.selected_history_item())),
            (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), lambda: self.unsubscribe_from_selected_channel(self.selected_history_item())),
            (self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"), self.add_active_to_playlist),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
            (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda: self.copy_direct_stream_url(self.selected_history_item())),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_history_item())),
            (self.t("remove_history_item"), self.remove_history_item),
            (self.t("clear_history"), self.clear_history),
        ]
        if self.item_has_openable_youtube_channel(selected):
            actions.insert(6, (self.menu_label_with_shortcut("open_channel", "open_channel"), lambda selected=dict(selected or {}): self.open_item_channel(selected)))
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def remove_history_item(self) -> None:
        if not hasattr(self, "history_list"):
            return
        index = self.history_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.history):
            del self.history[index]
            self.save_history()
            self.refresh_history()
            self.announce_player(self.t("history_removed"))


    def clear_history(self) -> None:
        self.history = []
        self.save_history()
        self.refresh_history()
        self.announce_player(self.t("history_cleared"))


    def show_subscriptions(self) -> None:
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = True
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.folder_screen_active = False
        self.clear()
        self.add_background_player_section()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("subscription_check_now"), lambda: self.check_subscriptions(manual=True)),
                (self.t("subscription_open_videos"), self.open_selected_subscription_videos),
                (self.t("subscription_new_videos_button"), self.open_selected_subscription_new_videos),
                (self.t("remove"), self.remove_subscription),
            ]
        )
        self.subscriptions_list = wx.ListBox(self.panel, choices=[])
        self.subscriptions_list.SetName(self.t("subscriptions"))
        self.subscriptions_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.open_selected_subscription_videos())
        self.subscriptions_list.Bind(wx.EVT_CONTEXT_MENU, self.open_subscriptions_context_menu)
        self.subscriptions_list.Bind(wx.EVT_KEY_DOWN, self.on_subscriptions_key)
        self.root_sizer.Add(self.subscriptions_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_subscriptions()
        self.panel.Layout()
        self.focus_later(self.subscriptions_list)


    def refresh_subscriptions(self) -> None:
        if not hasattr(self, "subscriptions_list"):
            return
        try:
            selection = self.subscriptions_list.GetSelection()
            if selection == wx.NOT_FOUND:
                selection = 0
            if self.subscriptions:
                self.set_listbox_items(self.subscriptions_list, [self.subscription_line(item) for item in self.subscriptions], selection)
            else:
                self.set_listbox_items(self.subscriptions_list, [self.t("subscription_empty")], 0)
                self.set_status(self.t("subscription_empty"))
        except RuntimeError:
            pass


    def subscription_line(self, item: dict) -> str:
        checked = self.format_history_time(item.get("last_checked")) if item.get("last_checked") else self.t("subscription_never_checked")
        new_count = int(item.get("last_new_count") or 0)
        parts = [
            item.get("title", ""),
            self.t("subscription_last_checked", time=checked) if item.get("last_checked") else checked,
            self.t("subscription_new_videos", count=new_count, title=item.get("title", "")) if new_count else "",
        ]
        return " | ".join(part for part in parts if part)


    def selected_subscription(self) -> dict | None:
        if not hasattr(self, "subscriptions_list"):
            return None
        index = self.subscriptions_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.subscriptions):
            return None
        return self.subscriptions[index]


    def on_subscriptions_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.open_selected_subscription_videos()
        elif self.shortcut_matches(event, "new_subscription_videos"):
            self.open_notification_center_shortcut()
        elif self.shortcut_matches(event, "unsubscribe_channel"):
            self.remove_subscription()
        elif self.shortcut_matches(event, "remove_selected"):
            self.remove_subscription()
        elif self.context_menu_shortcut_matches(event):
            self.open_subscriptions_context_menu()
        else:
            event.Skip()


    def open_subscriptions_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("subscription_open_videos"), self.open_selected_subscription_videos),
            (self.t("subscription_new_videos_button"), self.open_selected_subscription_new_videos),
            (self.t("subscription_check_now"), lambda: self.check_subscriptions(manual=True)),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_subscription())),
            (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), self.remove_subscription),
            (self.t("remove"), self.remove_subscription),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()


    def open_selected_subscription_videos(self) -> None:
        item = self.selected_subscription()
        if not item:
            self.message(self.t("no_selection"))
            return
        channel_item = {
            "title": item.get("title", ""),
            "url": item.get("url", ""),
            "kind": "channel",
            "type": self.t("channel"),
            "channel": item.get("title", ""),
        }
        self.open_library_item(channel_item, "subscriptions")


    def open_selected_subscription_new_videos(self) -> None:
        item = self.selected_subscription()
        if not item:
            self.message(self.t("no_selection"))
            return
        new_items = list(item.get("last_new_items") or [])
        if not new_items:
            self.announce_player(self.t("subscription_no_saved_new_videos"))
            return
        self.search_results_stack.append({"screen": "subscriptions"})
        self.last_search_query = self.t("subscription_new_videos_title", title=item.get("title", ""))
        self.last_search_type_index = 0
        self.current_search_type_code = "Video"
        self.collection_url = ""
        self.collection_result_type = ""
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        self.show_search(restore_search=True)
        self.show_results(new_items, selection=0, visible_count=len(new_items))
        wx.CallAfter(self.focus_results_list, 0)


    def remove_subscription(self) -> None:
        if not hasattr(self, "subscriptions_list"):
            return
        index = self.subscriptions_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.subscriptions):
            item = self.subscriptions.pop(index)
            self.save_subscriptions()
            self.refresh_subscriptions()
            self.announce_player(self.t("subscription_removed", title=item.get("title", "")))



    def subscribe_to_selected_channel(self, item: dict | None) -> None:
        if not item:
            self.message(self.t("no_selection"))
            return
        subscription = self.subscription_from_item(item)
        if not subscription:
            self.message(self.t("no_selection"))
            return
        url = subscription["url"]
        existing = next((sub for sub in self.subscriptions if sub.get("url") == url), None)
        if existing:
            self.announce_player(self.t("subscription_exists", title=existing.get("title", "")))
            return
        self.subscriptions.insert(0, subscription)
        self.save_subscriptions()
        self.refresh_subscriptions()
        self.announce_player(self.t("subscription_added", title=subscription.get("title", "")))


    def unsubscribe_from_selected_channel(self, item: dict | None) -> None:
        if not item:
            self.message(self.t("no_selection"))
            return
        subscription = self.subscription_from_item(item)
        if not subscription:
            self.message(self.t("no_selection"))
            return
        target_url = self.canonical_channel_url(subscription.get("url", ""))
        target_title = subscription.get("title") or self.t("channel")
        for index, existing in enumerate(list(self.subscriptions)):
            existing_url = self.canonical_channel_url(str(existing.get("url") or ""))
            if existing_url and existing_url == target_url:
                removed = self.subscriptions.pop(index)
                self.save_subscriptions()
                self.refresh_subscriptions()
                self.announce_player(self.t("subscription_removed", title=removed.get("title") or target_title))
                return
        self.announce_player(self.t("subscription_not_found", title=target_title))


    def subscription_from_item(self, item: dict) -> dict | None:
        kind = item.get("kind")
        channel_url = self.normalize_channel_url(item)
        if kind == "channel":
            channel_url = str(item.get("url") or channel_url).strip()
        if not channel_url and item.get("latest_urls") is not None:
            channel_url = str(item.get("url") or "").strip()
        channel_url = self.canonical_channel_url(channel_url)
        if not channel_url:
            return None
        title = item.get("channel") or item.get("title") or channel_url
        latest_urls = [item.get("url")] if item.get("kind") == "video" and item.get("url") else []
        return {
            "title": title,
            "url": channel_url,
            "latest_urls": latest_urls,
            "last_checked": 0.0,
            "last_new_count": 0,
            "created_at": time.time(),
        }



    def configure_subscription_timer(self) -> None:
        if not hasattr(self, "subscription_timer"):
            return
        try:
            self.subscription_timer.Stop()
        except Exception:
            pass
        if self.settings.subscription_check_enabled:
            interval_ms = int(self.refresh_interval_seconds(self.settings.subscription_check_interval_hours, 6.0) * 1000)
            self.subscription_timer.Start(interval_ms)


    def on_subscription_timer(self, _event) -> None:
        self.check_subscriptions_if_due()


    def configure_rss_timer(self) -> None:
        if not hasattr(self, "rss_timer"):
            return
        try:
            self.rss_timer.Stop()
        except Exception:
            pass
        if self.settings.enable_podcasts_rss and self.settings.rss_auto_refresh_enabled:
            interval_ms = int(self.refresh_interval_seconds(self.settings.rss_refresh_interval_hours, 12.0) * 1000)
            self.rss_timer.Start(interval_ms)


    def on_rss_timer(self, _event) -> None:
        self.refresh_all_rss_feeds_background()



    def check_subscriptions_if_due(self) -> None:
        if not self.settings.subscription_check_enabled or not self.subscriptions:
            return
        interval_seconds = self.refresh_interval_seconds(self.settings.subscription_check_interval_hours, 6.0)
        last_check = float(getattr(self.settings, "last_subscription_check", 0.0) or 0.0)
        if time.time() - last_check >= interval_seconds:
            self.check_subscriptions(manual=False)


    def check_subscriptions(self, manual: bool = False) -> None:
        if self.subscription_check_running:
            return
        if not self.subscriptions:
            if manual:
                self.announce_player(self.t("subscription_empty"))
            return
        self.subscription_check_running = True
        if manual:
            self.announce_player(self.t("subscription_checking"))
        threading.Thread(target=self.check_subscriptions_worker, args=(manual,), daemon=True).start()


    def check_subscriptions_worker(self, manual: bool = False) -> None:
        try:
            total_new = 0
            failures = 0
            successes = 0
            updated_subscriptions = []
            for subscription in self.subscriptions:
                try:
                    updated, new_items = self.check_one_subscription(subscription)
                except Exception as exc:
                    failures += 1
                    updated = dict(subscription)
                    updated["last_checked"] = time.time()
                    updated["last_error"] = self.friendly_error(exc)
                    updated_subscriptions.append(updated)
                    if manual:
                        self.ui_queue.put(("announce", self.t("subscription_check_failed", error=self.friendly_error(exc))))
                    continue
                successes += 1
                updated_subscriptions.append(updated)
                if new_items:
                    total_new += len(new_items)
                    message = self.t("subscription_new_videos", count=len(new_items), title=updated.get("title", ""))
                    self.ui_queue.put(("announce", message))
                    for entry in new_items[:20]:
                        title = str(entry.get("title") or "")
                        notification_message = self.t("notification_new_video", channel=updated.get("title", ""), title=title)
                        self.ui_queue.put(
                            (
                                "app_notification",
                                {
                                    "kind": "subscription",
                                    "title": self.t("notification_subscription_title"),
                                    "message": notification_message,
                                    "item": entry,
                                },
                            )
                        )
            if updated_subscriptions:
                self.subscriptions = updated_subscriptions
                self.save_subscriptions()
            if successes:
                self.settings.last_subscription_check = time.time()
                self.save_settings()
            self.ui_queue.put(("subscriptions_changed", None))
            if manual and not (failures and not successes):
                key = "subscription_check_complete" if total_new or failures else "subscription_no_new"
                self.ui_queue.put(("announce", self.t(key)))
        except Exception as exc:
            if manual:
                self.ui_queue.put(("announce", self.t("subscription_check_failed", error=exc)))
        finally:
            self.subscription_check_running = False


    def check_one_subscription(self, subscription: dict) -> tuple[dict, list[dict]]:
        entries = self.fetch_subscription_entries(subscription)
        current_urls = [entry.get("url", "") for entry in entries if entry.get("url")]
        known_urls = set(subscription.get("latest_urls") or [])
        if not known_urls:
            new_items: list[dict] = []
        else:
            new_items = [entry for entry in entries if entry.get("url") and entry.get("url") not in known_urls]
        updated = dict(subscription)
        updated["latest_urls"] = current_urls[:20]
        updated["last_checked"] = time.time()
        updated["last_new_count"] = len(new_items)
        updated["last_new_items"] = new_items[:20]
        return updated, new_items


    def fetch_subscription_entries(self, subscription: dict) -> list[dict]:
        url = self.collection_download_url({"kind": "channel", "url": subscription.get("url", "")})
        options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": 5}
        info = self.ydl_extract_info(url, options, download=False)
        entries = list(info.get("entries") or [])[:5]
        return [self.normalize_entry(entry, "Video") for entry in entries]


    def show_rss_feeds(self) -> None:
        if not self.settings.enable_podcasts_rss:
            self.show_main_menu()
            return
        self.ensure_rss_feeds_loaded()
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = True
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_background_player_section()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("search_podcasts"), self.search_podcasts),
                (self.t("add_rss_feed"), self.add_rss_feed),
                (self.t("refresh_feeds"), self.refresh_all_rss_feeds),
            ]
        )
        self.add_button_row(
            [
                (self.t("open_feed"), self.open_selected_rss_feed),
                (self.t("remove_feed"), self.remove_rss_feed),
                (self.t("import_opml"), self.import_rss_from_opml),
                (self.t("export_opml"), self.export_rss_to_opml),
            ]
        )
        self.rss_feed_list = wx.ListBox(self.panel, choices=[])
        self.rss_feed_list.SetName(self.t("rss_feeds"))
        self.rss_feed_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.open_selected_rss_feed())
        self.rss_feed_list.Bind(wx.EVT_CONTEXT_MENU, self.open_rss_feed_context_menu)
        self.rss_feed_list.Bind(wx.EVT_KEY_DOWN, self.on_rss_feed_key)
        self.root_sizer.Add(self.rss_feed_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_rss_feed_list()
        self.panel.Layout()
        self.focus_later(self.rss_feed_list)


    def refresh_rss_feed_list(self) -> None:
        if not hasattr(self, "rss_feed_list"):
            return
        try:
            if self.rss_feeds:
                labels = [self.rss_feed_line(feed) for feed in self.rss_feeds]
                index = min(max(0, self.current_rss_feed_index), len(self.rss_feeds) - 1)
                self.set_listbox_items(self.rss_feed_list, labels, index)
            else:
                self.set_listbox_items(self.rss_feed_list, [self.t("rss_feeds_empty")], 0)
                self.set_status(self.t("rss_feeds_empty"))
        except RuntimeError:
            pass


    def rss_feed_line(self, feed: dict) -> str:
        checked = self.format_history_time(feed.get("last_checked")) if feed.get("last_checked") else self.t("rss_feed_never_checked")
        count = len(feed.get("items") or [])
        parts = [
            feed.get("title") or self.t("rss_unknown_feed_title"),
            self.t("rss_feed_item_count", count=count),
            self.t("rss_feed_last_checked", time=checked) if feed.get("last_checked") else checked,
        ]
        return " | ".join(part for part in parts if part)


    def selected_rss_feed(self) -> dict | None:
        if not hasattr(self, "rss_feed_list"):
            return None
        index = self.rss_feed_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.rss_feeds):
            return None
        self.current_rss_feed_index = index
        return self.rss_feeds[index]


    def on_rss_feed_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.open_selected_rss_feed()
        elif self.shortcut_matches(event, "remove_selected"):
            self.remove_rss_feed()
        elif self.context_menu_shortcut_matches(event):
            self.open_rss_feed_context_menu()
        else:
            event.Skip()


    def open_rss_feed_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("open_feed"), self.open_selected_rss_feed),
            (self.t("download_feed"), self.download_selected_rss_feed),
            (self.t("refresh_feed"), self.refresh_selected_rss_feed),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_rss_feed())),
            (self.t("remove_feed"), self.remove_rss_feed),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()


    def add_rss_feed(self) -> None:
        with wx.TextEntryDialog(self, self.t("rss_feed_url"), self.t("add_rss_feed")) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            url = dialog.GetValue().strip()
        self.add_rss_feed_url(url)


    def add_rss_feed_url(self, url: str) -> None:
        if not url:
            return
        self.ensure_rss_feeds_loaded()
        if not re.match(r"^https?://", url, flags=re.IGNORECASE):
            url = "https://" + url
        if any(str(feed.get("url") or "").rstrip("/") == url.rstrip("/") for feed in self.rss_feeds):
            self.announce_player(self.t("rss_feed_exists"))
            return
        self.announce_player(self.t("rss_refresh_started"))
        threading.Thread(target=self.add_rss_feed_worker, args=(url,), daemon=True).start()


    def add_rss_feed_worker(self, url: str) -> None:
        try:
            feed = self.fetch_rss_feed(url)
            self.rss_feeds.insert(0, feed)
            self.save_rss_feeds()
            self.ui_queue.put(("rss_feeds_changed", None))
            self.ui_queue.put(("announce", self.t("rss_feed_added", title=feed.get("title") or self.t("rss_unknown_feed_title"))))
        except Exception as exc:
            self.ui_queue.put(("announce", self.t("rss_refresh_failed", error=self.friendly_error(exc))))


    def export_rss_to_opml(self) -> None:
        self.ensure_rss_feeds_loaded()
        if not self.rss_feeds:
            self.message(self.t("opml_no_feeds"))
            return
        
        start_dir = self.settings.download_folder or str(Path.home())
        with wx.FileDialog(
            self,
            self.t("export_opml"),
            defaultDir=start_dir if Path(start_dir).exists() else str(Path.home()),
            defaultFile="apricot_feeds.opml",
            wildcard=f"{self.t('opml_files')} (*.opml)|*.opml",
            style=wx.FD_SAVE | wx.FD_OVERWRITE_PROMPT,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            path = dialog.GetPath()
            
        try:
            opml = ET.Element("opml", version="2.0")
            head = ET.SubElement(opml, "head")
            title = ET.SubElement(head, "title")
            title.text = "ApricotPlayer RSS Feeds Export"
            
            body = ET.SubElement(opml, "body")
            outline_parent = ET.SubElement(body, "outline", text="Apricot RSS Feeds", title="Apricot RSS Feeds")
            
            for feed in self.rss_feeds:
                feed_title = feed.get("title") or "Unknown Feed"
                feed_url = feed.get("url") or ""
                if not feed_url:
                    continue
                ET.SubElement(
                    outline_parent,
                    "outline",
                    type="rss",
                    text=feed_title,
                    title=feed_title,
                    xmlUrl=feed_url,
                    htmlUrl=feed_url,
                )
                
            tree = ET.ElementTree(opml)
            ET.indent(tree, space="  ", level=0)
            with open(path, "wb") as f:
                tree.write(f, encoding="utf-8", xml_declaration=True)
                
            self.message(self.t("opml_export_success"))
        except Exception as exc:
            self.message(self.t("opml_export_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)


    def import_rss_from_opml(self) -> None:
        start_dir = self.settings.download_folder or str(Path.home())
        with wx.FileDialog(
            self,
            self.t("import_opml"),
            defaultDir=start_dir if Path(start_dir).exists() else str(Path.home()),
            wildcard=f"{self.t('opml_files')} (*.opml)|*.opml",
            style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return
            path = dialog.GetPath()
            
        try:
            tree = ET.parse(path)
            root = tree.getroot()
            
            feed_urls = []
            for outline in root.findall(".//outline"):
                xml_url = outline.get("xmlUrl")
                if xml_url:
                    xml_url = xml_url.strip()
                    title = outline.get("title") or outline.get("text") or "RSS Feed"
                    if xml_url:
                        feed_urls.append((xml_url, title))
                        
            if not feed_urls:
                self.message(self.t("opml_no_feeds"), wx.ICON_WARNING)
                return
                
            self.ensure_rss_feeds_loaded()
            existing_urls = {str(feed.get("url") or "").rstrip("/").lower() for feed in self.rss_feeds}
            
            to_import = []
            for url, title in feed_urls:
                normalized = url.rstrip("/").lower()
                if normalized not in existing_urls:
                    to_import.append((url, title))
                    existing_urls.add(normalized)
                    
            if not to_import:
                self.message(self.t("opml_all_feeds_exist"))
                return
                
            self.announce_player(self.t("opml_import_started", count=len(to_import)))
            threading.Thread(target=self.import_opml_worker, args=(to_import,), daemon=True).start()
        except Exception as exc:
            self.message(self.t("opml_import_failed_msg", error=self.friendly_error(exc)), wx.ICON_ERROR)



    def refresh_all_rss_feeds(self) -> None:
        self.ensure_rss_feeds_loaded()
        if not self.rss_feeds:
            self.announce_player(self.t("rss_feeds_empty"))
            return
        if self.rss_refresh_running:
            return
        self.rss_refresh_running = True
        self.announce_player(self.t("rss_refresh_started"))
        threading.Thread(target=self.refresh_rss_feeds_worker, args=(None, False), daemon=True).start()


    def refresh_all_rss_feeds_background(self) -> None:
        if not self.settings.enable_podcasts_rss:
            return
        self.ensure_rss_feeds_loaded()
        if not self.rss_feeds or self.rss_refresh_running:
            return
        self.rss_refresh_running = True
        threading.Thread(target=self.refresh_rss_feeds_worker, args=(None, True), daemon=True).start()


    def refresh_selected_rss_feed(self) -> None:
        feed = self.selected_rss_feed()
        if not feed:
            self.message(self.t("no_selection"))
            return
        if self.rss_refresh_running:
            return
        self.rss_refresh_running = True
        self.announce_player(self.t("rss_refresh_started"))
        threading.Thread(target=self.refresh_rss_feeds_worker, args=(self.current_rss_feed_index, False), daemon=True).start()


    def refresh_rss_feeds_worker(self, feed_index: int | None, silent: bool = False) -> None:
        try:
            if feed_index is None:
                indexes = range(len(self.rss_feeds))
            else:
                indexes = [feed_index]
            updated_feeds = list(self.rss_feeds)
            failures: list[str] = []
            for index in indexes:
                if index < 0 or index >= len(updated_feeds):
                    continue
                existing = updated_feeds[index]
                try:
                    known_urls = {str(item.get("url") or "") for item in existing.get("items") or [] if item.get("url")}
                    refreshed = self.fetch_rss_feed(str(existing.get("url") or ""))
                    refreshed["created_at"] = existing.get("created_at", refreshed.get("created_at", time.time()))
                    updated_feeds[index] = refreshed
                    if known_urls:
                        for entry in list(refreshed.get("items") or []):
                            url = str(entry.get("url") or "")
                            if url and url not in known_urls:
                                notification_message = self.t("notification_new_podcast", feed=refreshed.get("title", ""), title=entry.get("title", ""))
                                self.ui_queue.put(
                                    (
                                        "app_notification",
                                        {
                                            "kind": "podcast",
                                            "title": self.t("rss_feeds"),
                                            "message": notification_message,
                                            "item": entry,
                                        },
                                    )
                                )
                except Exception as exc:
                    preserved = dict(existing)
                    preserved["last_error"] = self.friendly_error(exc)
                    preserved["last_checked"] = time.time()
                    updated_feeds[index] = preserved
                    failures.append(preserved.get("title") or preserved.get("url") or self.t("rss_unknown_feed_title"))
            self.rss_feeds = updated_feeds
            self.save_rss_feeds()
            self.ui_queue.put(("rss_feeds_changed", None))
            if not silent and failures:
                self.ui_queue.put(("announce", self.t("rss_refresh_failed", error=", ".join(failures[:3]))))
            elif not silent:
                self.ui_queue.put(("announce", self.t("rss_refresh_done")))
        except Exception as exc:
            if not silent:
                self.ui_queue.put(("announce", self.t("rss_refresh_failed", error=self.friendly_error(exc))))
        finally:
            self.rss_refresh_running = False


    def open_selected_rss_feed(self) -> None:
        feed = self.selected_rss_feed()
        if not feed:
            self.message(self.t("no_selection"))
            return
        self.show_rss_items(self.current_rss_feed_index)


    def remove_rss_feed(self) -> None:
        if not hasattr(self, "rss_feed_list"):
            return
        index = self.rss_feed_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.rss_feeds):
            del self.rss_feeds[index]
            self.current_rss_feed_index = min(index, len(self.rss_feeds) - 1)
            self.save_rss_feeds()
            self.refresh_rss_feed_list()
            self.announce_player(self.t("rss_feed_removed"))


    def show_rss_items(self, feed_index: int, selection: int = 0) -> None:
        self.ensure_rss_feeds_loaded()
        if feed_index < 0 or feed_index >= len(self.rss_feeds):
            self.show_rss_feeds()
            return
        self.current_rss_feed_index = feed_index
        feed = self.rss_feeds[feed_index]
        self.rss_items = list(feed.get("items") or [])
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = True
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = False
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.clear()
        self.add_button_row(
            [
                (self.t("back"), self.show_rss_feeds),
                (self.t("refresh_feed"), self.refresh_selected_rss_feed_from_items),
                (self.t("play_episode"), self.play_selected_rss_item),
                (self.t("download_episode_audio"), self.download_selected_rss_item),
                (self.t("download_feed"), self.download_current_rss_feed),
            ]
        )
        label = wx.StaticText(self.panel, label=feed.get("title") or self.t("rss_feed_items"))
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.rss_items_list = wx.ListBox(self.panel, choices=[])
        self.rss_items_list.SetName(self.t("rss_feed_items"))
        self.rss_items_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.play_selected_rss_item())
        self.rss_items_list.Bind(wx.EVT_CONTEXT_MENU, self.open_rss_item_context_menu)
        self.rss_items_list.Bind(wx.EVT_KEY_DOWN, self.on_rss_item_key)
        self.root_sizer.Add(self.rss_items_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_rss_items_list(selection)
        self.panel.Layout()
        self.focus_later(self.rss_items_list)


    def refresh_rss_items_list(self, selection: int = 0) -> None:
        if not hasattr(self, "rss_items_list"):
            return
        try:
            if self.rss_items:
                self.set_listbox_items(self.rss_items_list, [self.rss_item_line(item) for item in self.rss_items], selection)
            else:
                self.set_listbox_items(self.rss_items_list, [self.t("rss_items_empty")], 0)
                self.set_status(self.t("rss_items_empty"))
        except RuntimeError:
            pass


    def rss_item_line(self, item: dict) -> str:
        published = self.format_history_time(item.get("timestamp")) if item.get("timestamp") else ""
        queued = self.t("podcast_audio_queued_marker") if str(item.get("url") or "") in self.download_queue else ""
        parts = [
            item.get("title", ""),
            f"{self.t('published')}: {published}" if published else "",
            item.get("duration", ""),
            item.get("type", self.t("podcast_episode")),
            queued,
        ]
        return " | ".join(part for part in parts if part)


    def selected_rss_item(self) -> dict | None:
        if not hasattr(self, "rss_items_list"):
            return None
        index = self.rss_items_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.rss_items):
            return None
        item = dict(self.rss_items[index])
        item["rss_feed_index"] = self.current_rss_feed_index
        item["rss_item_index"] = index
        return item


    def on_rss_item_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.play_selected_rss_item()
        elif self.shortcut_matches(event, "queue_audio"):
            self.toggle_rss_item_queue()
        elif self.shortcut_matches(event, "download_audio"):
            self.download_selected_rss_item()
        elif self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
        elif self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
        elif self.context_menu_shortcut_matches(event):
            self.open_rss_item_context_menu()
        else:
            event.Skip()


    def open_rss_item_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("play_episode"), self.play_selected_rss_item),
            (self.menu_label_with_shortcut("download_episode_audio", "download_audio"), self.download_selected_rss_item),
            (self.menu_label_with_shortcut("queue_episode_audio", "queue_audio"), self.toggle_rss_item_queue),
            (self.menu_label_with_shortcut("add_to_playlist", "add_to_playlist"), self.add_active_to_playlist),
            (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
            (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
            (self.t("download_feed"), self.download_current_rss_feed),
            (self.t("open_episode_page"), self.open_selected_in_browser),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_rss_item())),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def download_selected_rss_item(self) -> None:
        item = self.selected_rss_item()
        if item and item.get("url"):
            self.start_download(True, item=item)
        else:
            self.message(self.t("no_selection"))


    def toggle_rss_item_queue(self) -> None:
        item = self.selected_rss_item()
        if not item or not item.get("url"):
            self.message(self.t("no_selection"))
            return
        url = str(item.get("url") or "")
        if url in self.download_queue:
            self.download_queue.pop(url, None)
            self.announce_player(self.t("download_deselected", title=item.get("title", "")))
        else:
            queued = dict(item)
            queued["audio_only"] = True
            self.download_queue[url] = queued
            self.announce_player(self.t("podcast_episode_audio_selected_download", title=item.get("title", "")))
        self.refresh_rss_items_list(int(item.get("rss_item_index") or 0))
        self.refresh_download_views()


    def download_selected_rss_feed(self) -> None:
        feed = self.selected_rss_feed()
        if not feed:
            self.message(self.t("no_selection"))
            return
        self.download_rss_feed(feed)


    def download_current_rss_feed(self) -> None:
        if self.current_rss_feed_index < 0 or self.current_rss_feed_index >= len(self.rss_feeds):
            self.message(self.t("no_selection"))
            return
        self.download_rss_feed(self.rss_feeds[self.current_rss_feed_index])


    def download_rss_feed(self, feed: dict) -> None:
        items = [dict(item, audio_only=True) for item in list(feed.get("items") or []) if item.get("url")]
        if not items:
            self.announce_player(self.t("rss_items_empty"))
            return
        title = feed.get("title") or self.t("rss_unknown_feed_title")
        for item in items:
            item.setdefault("channel", title)
            item.setdefault("kind", "rss_item")
            item.setdefault("type", self.t("podcast_episode"))
        self.announce_player(self.t("download_feed_start"))
        self.set_status(self.t("download_feed_start"))
        task_id, cancel_event = self.register_download_task({"title": title, "kind": "rss_feed"}, True, "rss_feed", total=len(items))
        self.refresh_download_views()
        finish_folder = str(self.podcasts_download_folder() / self.safe_folder_name(str(title)))
        done_text = self.t("download_feed_done", title=title)
        threading.Thread(target=self.download_batch_worker, args=(items, task_id, cancel_event, done_text, finish_folder), daemon=True).start()


    def refresh_selected_rss_feed_from_items(self) -> None:
        if self.current_rss_feed_index < 0 or self.current_rss_feed_index >= len(self.rss_feeds):
            self.show_rss_feeds()
            return
        if self.rss_refresh_running:
            return
        self.rss_refresh_running = True
        self.announce_player(self.t("rss_refresh_started"))
        threading.Thread(target=self.refresh_rss_feeds_worker, args=(self.current_rss_feed_index, False), daemon=True).start()


    def fetch_rss_feed(self, url: str) -> dict:
        request = Request(url, headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=30) as response:
            final_url = response.geturl()
            raw = response.read(3_000_000)
        ET = import_module("xml.etree.ElementTree")
        root = ET.fromstring(raw)
        title, site_url, items = self.parse_feed_root(root, final_url)
        return {
            "title": title or self.t("rss_unknown_feed_title"),
            "url": final_url or url,
            "site_url": site_url,
            "items": items[: min(500, max(1, int(self.settings.rss_max_items or 100)))],
            "last_checked": time.time(),
            "created_at": time.time(),
        }



    def parse_rss_item(self, item: ET.Element, base_url: str, feed_title: str) -> dict:
        title = self.child_text(item, "title")
        page_url = self.absolute_url(self.child_text(item, "link"), base_url)
        media_url = ""
        for child in list(item):
            name = self.xml_local_name(child.tag)
            if name in {"enclosure", "content"} and child.get("url"):
                media_url = self.absolute_url(str(child.get("url") or ""), base_url)
                break
        timestamp = self.parse_feed_timestamp(self.child_text(item, "pubDate") or self.child_text(item, "published"))
        description = self.strip_html(self.child_text(item, "description") or self.child_text(item, "summary") or self.child_text(item, "content"))
        guid = self.child_text(item, "guid")
        url = media_url or page_url or self.absolute_url(guid, base_url)
        duration = self.child_text(item, "duration")
        chapters_url, chapters_type = self.podcast_chapters_reference(item, base_url)
        return {
            "title": title or page_url or media_url,
            "url": url,
            "webpage_url": page_url or url,
            "media_url": media_url,
            "description": description,
            "duration": duration,
            "chapters": self.parse_inline_podcast_chapters(item),
            "chapters_url": chapters_url,
            "chapters_type": chapters_type,
            "timestamp": timestamp,
            "channel": feed_title,
            "kind": "rss_item",
            "type": self.t("podcast_episode"),
        }


    def open_playlist_videos(self, item: dict, push_state: bool = True) -> None:
        if push_state:
            self.push_search_state()
        self.trending_screen_active = False
        self.set_status(self.t("loading_playlist", title=item["title"]))
        self.collection_url = item["url"]
        self.collection_result_type = "Video"
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.load_collection_worker, args=(item["url"], "Video", self.initial_results_limit(), 0, generation, ""), daemon=True).start()

    def start_playlist_playback_if_current(self, generation: int, playlist_item: dict, items: list[dict], shuffle: bool) -> None:
        if generation != self.playlist_play_generation:
            return
        self.start_playlist_playback(playlist_item, items, shuffle)


    def start_playlist_playback(self, playlist_item: dict, items: list[dict], shuffle: bool) -> None:
        if not items:
            self.announce_player(self.t("playlist_no_videos"))
            return
        ordered = [dict(item) for item in items]
        if shuffle:
            random.shuffle(ordered)
        self.shuffle_current = False
        sequence = [self.playlist_item_from_media(item) for item in ordered]
        self.set_player_sequence(sequence)
        current_item = dict(sequence[0])
        self.player_return_screen = "search"
        self.player_return_data = {"index": self.return_index, "playlist_title": playlist_item.get("title", "")}
        self.current_video_item = current_item
        self.current_video_info = dict(current_item)
        self.play_url(str(current_item.get("url") or ""), str(current_item.get("title") or ""))



    def add_selected_favorite(self) -> None:
        self.add_favorite_item(self.active_item())


    def add_favorite_item(self, item: dict | None) -> None:
        if not item:
            self.message(self.t("no_selection"))
            return
        favorite = {
            "title": item.get("title", ""),
            "channel": item.get("channel", ""),
            "channel_url": item.get("channel_url", ""),
            "kind": item.get("kind", "video"),
            "type": self.item_type_label(item),
            "live_status": item.get("live_status", ""),
            "is_live": bool(item.get("is_live", False)),
            "url": item.get("url", ""),
        }
        if any(existing["url"] == favorite["url"] for existing in self.favorites):
            self.announce_player(self.t("favorite_exists"))
            return
        self.favorites.append(favorite)
        self.save_favorites()
        self.announce_player(self.t("favorite_added"))


    def refresh_favorites(self) -> None:
        if not hasattr(self, "favorites_list"):
            return
        if self.favorites:
            labels = [f"{index + 1}. {item['title']} | {self.t('channel')}: {item['channel']}" for index, item in enumerate(self.favorites)]
            self.set_listbox_items(self.favorites_list, labels, 0)
        else:
            self.set_listbox_items(self.favorites_list, [self.t("favorites_empty")], 0)
            self.set_status(self.t("favorites_empty"))


    def selected_favorite(self) -> dict | None:
        index = self.favorites_list.GetSelection()
        return None if index == wx.NOT_FOUND or index < 0 or index >= len(self.favorites) else self.favorites[index]



    def remove_favorite(self) -> None:
        index = self.favorites_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.favorites):
            del self.favorites[index]
            self.save_favorites()
            if self.favorites_screen_active and not self.favorites:
                wx.CallAfter(self.show_favorites)
            else:
                self.refresh_favorites()
            self.announce_player(self.t("favorite_removed"))


    def remove_selected_favorite_shortcut(self) -> None:
        self.remove_favorite_item(self.active_item())


    def remove_favorite_item(self, item: dict | None) -> None:
        if not item:
            self.announce_player(self.t("no_selection"))
            return
        url = str(item.get("url") or "")
        if not url:
            self.announce_player(self.t("not_in_favorites"))
            return
        for index, favorite in enumerate(list(self.favorites)):
            if str(favorite.get("url") or "") == url:
                del self.favorites[index]
                self.save_favorites()
                if self.favorites_screen_active:
                    if self.favorites:
                        self.refresh_favorites()
                    else:
                        wx.CallAfter(self.show_favorites)
                self.announce_player(self.t("favorite_removed"))
                return
        self.announce_player(self.t("not_in_favorites"))


