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

class DownloaderMixin:

    def install_download_accelerators(self) -> None:
        self.global_accelerator_ids: dict[str, wx.WindowIDRef] = {}
        global_actions = [
            ("open_main_menu", self.open_main_menu_shortcut),
            ("open_search", self.open_search_shortcut),
            ("open_play_from_folder", self.open_play_from_folder_shortcut),
            ("open_direct_link", self.open_direct_link_shortcut),
            ("open_favorites", self.open_favorites_shortcut),
            ("open_bookmarks", self.open_bookmarks_shortcut),
            ("open_playlists", self.open_playlists_shortcut),
            ("open_subscriptions", self.open_subscriptions_shortcut),
            ("open_current_downloads", self.open_current_downloads_shortcut),
            ("open_history", self.open_history_shortcut),
            ("open_podcasts_rss", self.open_podcasts_rss_shortcut),
            ("open_settings", self.open_settings_shortcut),
            ("open_playback_queue", self.open_playback_queue_shortcut),
            ("background_play_pause", self.background_play_pause_shortcut),
            ("download_audio", self.download_audio_shortcut),
            ("download_video", self.download_video_shortcut),
            ("subscribe_channel", self.subscribe_shortcut),
            ("unsubscribe_channel", self.unsubscribe_shortcut),
            ("new_subscription_videos", self.open_notification_center_shortcut),
        ]
        for action, handler in global_actions:
            menu_id = wx.NewIdRef()
            self.global_accelerator_ids[action] = menu_id
            self.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), id=int(menu_id))
        entries = []
        for action, menu_id in self.global_accelerator_ids.items():
            if self.shortcut_is_plain_printable(action):
                continue
            accelerator = self.shortcut_to_accelerator(self.shortcut_for(action))
            if accelerator:
                flags, key_code = accelerator
                entries.append((flags, key_code, menu_id))
        self.SetAcceleratorTable(wx.AcceleratorTable(entries))



    def download_direct_link(self, audio_only: bool) -> None:
        item = self.direct_link_item()
        if not item:
            self.message(self.t("no_selection"))
            return
        self.start_download(audio_only, item=item)

    def open_current_downloads_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.show_download_queue)


    def download_items_snapshot(self) -> list[dict]:
        active = sorted(self.active_downloads.values(), key=lambda item: item.get("created_at", 0))
        queued = list(self.download_queue.values())
        return [dict(item, queue_state="active") for item in active] + [dict(item, queue_state="queued") for item in queued]



    def register_download_task(self, item: dict, audio_only: bool, task_kind: str = "single", total: int = 0) -> tuple[str, threading.Event]:
        task_id = self.next_download_task_id(task_kind)
        cancel_event = threading.Event()
        title = item.get("title") or self.t("download_video_mode")
        self.download_cancel_events[task_id] = cancel_event
        self.active_downloads[task_id] = {
            "task_id": task_id,
            "task_kind": task_kind,
            "title": title,
            "current_title": title,
            "url": item.get("url", ""),
            "kind": item.get("kind", "video"),
            "type": item.get("type", ""),
            "channel": item.get("channel", ""),
            "audio_only": audio_only,
            "status_key": "download_state_downloading",
            "total": total,
            "completed": 0,
            "remaining": total,
            "percent": "",
            "created_at": time.monotonic(),
        }
        self.refresh_download_views()
        return task_id, cancel_event


    def update_download_task(self, task_id: str, **fields) -> None:
        task = self.active_downloads.get(task_id)
        if not task:
            return
        task.update(fields)
        playlist_count = self.to_int(str(task.get("playlist_count") or 0), 0, 0)
        if playlist_count and not self.to_int(str(task.get("total") or 0), 0, 0):
            task["total"] = playlist_count
        self.update_download_progress_dialog(task)
        self.refresh_download_views(update_menu=False)


    def finish_download_task(self, task_id: str, status_key: str = "download_state_done") -> None:
        task = self.active_downloads.get(task_id)
        if task:
            task["status_key"] = status_key
        self.close_download_progress_dialog(task_id)
        self.download_cancel_events.pop(task_id, None)
        self.active_downloads.pop(task_id, None)
        self.refresh_download_views()



    def refresh_download_views(self, update_menu: bool = True) -> None:
        if self.in_queue_screen:
            wx.CallAfter(self.refresh_queue_view)
        if update_menu and hasattr(self, "menu_list"):
            wx.CallAfter(self.refresh_main_menu_download_label)


    def refresh_main_menu_download_label(self) -> None:
        if not getattr(self, "in_main_menu", False) or not hasattr(self, "menu_list"):
            return
        try:
            old_selection = self.menu_list.GetSelection()
            old_label = self.menu_list.GetString(old_selection) if old_selection != wx.NOT_FOUND else ""
            self.menu_actions = self.build_main_menu_actions()
            labels = [item[0] for item in self.menu_actions]
            selection = old_selection
            if old_label in labels:
                selection = labels.index(old_label)
            elif labels:
                selection = min(max(0, old_selection), len(labels) - 1)
            self.set_listbox_items(self.menu_list, labels, selection)
        except RuntimeError:
            pass



    def append_collection_download_submenu(self, menu: wx.Menu, item: dict) -> None:
        kind = str(item.get("kind") or "playlist")
        submenu = wx.Menu()
        audio_item = submenu.Append(wx.ID_ANY, self.menu_label_with_shortcut("download_audio", "download_audio"))
        video_item = submenu.Append(wx.ID_ANY, self.menu_label_with_shortcut("download_video", "download_video"))
        submenu.Bind(wx.EVT_MENU, lambda _evt, selected=dict(item): self.download_collection(selected, audio_only=True), audio_item)
        submenu.Bind(wx.EVT_MENU, lambda _evt, selected=dict(item): self.download_collection(selected, audio_only=False), video_item)
        menu.AppendSubMenu(submenu, self.t("download_channel" if kind == "channel" else "download_playlist"))



    def download_root_folder(self) -> Path:
        folder = Path(str(self.settings.download_folder or DEFAULT_DOWNLOAD_ROOT)).expanduser()
        return folder


    def music_download_folder(self) -> Path:
        root = self.download_root_folder()
        if root.name.lower() == "music":
            return root
        return root / "music"


    def podcasts_download_folder(self) -> Path:
        root = self.download_root_folder()
        root_name = root.name.lower()
        if root_name == "podcasts":
            return root
        if root_name == "music":
            return root.parent / "podcasts"
        return root / "podcasts"


    def download_folder_for_item(self, item: dict, audio_only: bool = True, collection: bool = False) -> Path:
        kind = str(item.get("kind") or "")
        if kind == "rss_item":
            feed_title = item.get("channel") or item.get("podcast_title") or self.t("rss_unknown_feed_title")
            return self.podcasts_download_folder() / self.safe_folder_name(str(feed_title))
        folder = self.music_download_folder()
        if collection or kind in {"playlist", "channel"}:
            title = item.get("title") or self.t("channel" if kind == "channel" else "playlist")
            return folder / self.safe_folder_name(str(title))
        return folder


    def default_download_filename(self, item: dict, audio_only: bool) -> str:
        title = self.safe_folder_name(str(item.get("title") or "download"))
        extension = self.settings.audio_format if audio_only else "mp4"
        return f"{title}.{extension}"


    def choose_download_target_path(self, item: dict, audio_only: bool) -> Path | None:
        folder = self.download_folder_for_item(item, audio_only)
        folder.mkdir(parents=True, exist_ok=True)
        if audio_only:
            wildcard = f"{self.settings.audio_format.upper()} (*.{self.settings.audio_format})|*.{self.settings.audio_format}|{self.t('all_files')} (*.*)|*.*"
        else:
            wildcard = f"MP4 (*.mp4)|*.mp4|{self.t('all_files')} (*.*)|*.*"
        with wx.FileDialog(
            self,
            self.t("choose_save_path"),
            defaultDir=str(folder),
            defaultFile=self.default_download_filename(item, audio_only),
            wildcard=wildcard,
            style=wx.FD_SAVE | wx.FD_OVERWRITE_PROMPT,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return None
            path = Path(dialog.GetPath()).expanduser()
        expected_suffix = f".{self.settings.audio_format}" if audio_only else ".mp4"
        if not path.suffix:
            path = path.with_suffix(expected_suffix)
        return path


    def choose_download_target_folder(self, item: dict, collection: bool = False) -> Path | None:
        folder = self.download_folder_for_item(item, True, collection=collection)
        folder.mkdir(parents=True, exist_ok=True)
        with wx.DirDialog(self, self.t("choose_save_folder"), defaultPath=str(folder), style=wx.DD_DEFAULT_STYLE | wx.DD_DIR_MUST_EXIST) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return None
            return Path(dialog.GetPath()).expanduser()


    def start_download(self, audio_only: bool, item: dict | None = None, remove_queued: bool = False) -> None:
        item = item or self.active_item()
        if not item:
            self.message(self.t("no_selection"))
            return
        if self.in_player_screen and self.clip_markers_are_set():
            self.export_marked_clip(audio_only=audio_only)
            return
        if self.in_player_screen and self.clip_markers_partially_set():
            self.announce_player(self.t("clip_markers_missing"))
            return
        if item.get("kind") in {"playlist", "channel"}:
            self.download_collection(item, audio_only=audio_only, remove_queued=remove_queued)
            return
        action = "audio" if audio_only else "video"
        if self.settings.confirm_before_download:
            if wx.MessageBox(self.t("download_confirm", action=action, title=item["title"]), APP_NAME, wx.YES_NO | wx.ICON_QUESTION) != wx.YES:
                self.set_status(self.t("download_cancelled"))
                return
        if remove_queued:
            self.remove_queued_url(item.get("url", ""), announce=False)
        target_path = self.choose_download_target_path(item, audio_only) if self.settings.ask_download_location_each_time else None
        if self.settings.ask_download_location_each_time and target_path is None:
            self.set_status(self.t("download_cancelled"))
            return
        self.announce_player(self.t("download_started"))
        self.set_status(self.t("download_audio_start" if audio_only else "download_video_start"))
        task_id, cancel_event = self.register_download_task(item, audio_only, "single", total=1)
        wx.CallLater(900, self.start_download_worker_thread, item, audio_only, task_id, cancel_event, target_path)


    def start_download_worker_thread(self, item: dict, audio_only: bool, task_id: str, cancel_event: threading.Event, target_path: Path | None = None) -> None:
        threading.Thread(target=self.download_worker, args=(item, audio_only, task_id, cancel_event, target_path), daemon=True).start()


    def download_audio(self) -> None:
        self.start_download(True)


    def download_video(self) -> None:
        self.start_download(False)


    def download_collection(self, item: dict | None = None, audio_only: bool = False, remove_queued: bool = False) -> None:
        item = item or self.selected_result()
        if not item or item.get("kind") not in {"playlist", "channel"}:
            self.message(self.t("no_selection"))
            return
        kind = str(item.get("kind") or "playlist")
        if self.settings.confirm_before_download:
            action = self.t("download_channel" if kind == "channel" else "download_playlist")
            if wx.MessageBox(self.t("download_confirm", action=action, title=item["title"]), APP_NAME, wx.YES_NO | wx.ICON_QUESTION) != wx.YES:
                self.set_status(self.t("download_cancelled"))
                return
        if remove_queued:
            self.remove_queued_url(item.get("url", ""), announce=False)
        target_folder = self.choose_download_target_folder(item, collection=True) if self.settings.ask_download_location_each_time else None
        if self.settings.ask_download_location_each_time and target_folder is None:
            self.set_status(self.t("download_cancelled"))
            return
        start_key = "download_channel_start" if kind == "channel" else "download_playlist_start"
        self.announce_player(self.t("download_started"))
        self.set_status(self.t(start_key))
        task_id, cancel_event = self.register_download_task(item, audio_only, kind, total=0)
        self.show_download_progress_dialog(task_id, item.get("title") or self.t("channel" if kind == "channel" else "playlist"))
        threading.Thread(target=self.download_collection_worker, args=(dict(item), audio_only, task_id, cancel_event, target_folder), daemon=True).start()


    def download_audio_shortcut(self) -> None:
        self.start_download_shortcut(True)


    def download_video_shortcut(self) -> None:
        self.start_download_shortcut(False)


    def start_download_shortcut(self, audio_only: bool) -> None:
        if self.in_main_menu:
            return
        item = self.active_item()
        url = str(item.get("url", "")) if item else ""
        kind = "audio" if audio_only else "video"
        now = time.monotonic()
        last_kind, last_url, last_time = self.last_download_shortcut
        if kind == last_kind and url == last_url and now - last_time < 0.35:
            return
        self.last_download_shortcut = (kind, url, now)
        self.start_download(audio_only, item=item)


    def download_worker(self, item: dict, audio_only: bool, task_id: str, cancel_event: threading.Event, target_path: Path | None = None) -> None:
        try:
            folder = target_path.parent if target_path else self.download_folder_for_item(item, audio_only)
            folder.mkdir(parents=True, exist_ok=True)
            options = self.download_options(folder, audio_only, item["title"], task_id=task_id, cancel_event=cancel_event, target_path=target_path)
            self.ydl_download_urls([item["url"]], options)
            if cancel_event.is_set():
                raise DownloadCancelled()
            done_text = self.t("download_audio_done" if audio_only else "download_video_done", title=item["title"])
            wx.CallAfter(self.finish_download_task, task_id)
            wx.CallAfter(self.record_history, item, "downloaded audio" if audio_only else "downloaded video")
            wx.CallAfter(self.finish_download, done_text, str(folder))
        except DownloadCancelled:
            wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
            wx.CallAfter(self.announce_player, self.t("download_cancelled"))
        except Exception as exc:
            if cancel_event.is_set():
                wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
                wx.CallAfter(self.announce_player, self.t("download_cancelled"))
                return
            wx.CallAfter(self.finish_download_task, task_id, "download_state_failed")
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)


    def download_collection_worker(self, item: dict, audio_only: bool, task_id: str, cancel_event: threading.Event, target_folder: Path | None = None) -> None:
        try:
            kind = str(item.get("kind") or "playlist")
            title = item.get("title") or self.t("channel" if kind == "channel" else "playlist")
            folder = target_folder or self.download_folder_for_item(item, audio_only, collection=True)
            folder.mkdir(parents=True, exist_ok=True)
            options = self.download_options(folder, audio_only, title, allow_playlist=True, task_id=task_id, cancel_event=cancel_event)
            self.ydl_download_urls([self.collection_download_url(item)], options)
            if cancel_event.is_set():
                raise DownloadCancelled()
            done_key = "download_channel_done" if kind == "channel" else "download_playlist_done"
            wx.CallAfter(self.finish_download_task, task_id)
            wx.CallAfter(self.record_history, item, f"downloaded {kind}")
            wx.CallAfter(self.finish_download, self.t(done_key, title=title), str(folder))
        except DownloadCancelled:
            wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
            wx.CallAfter(self.announce_player, self.t("download_cancelled"))
        except Exception as exc:
            if cancel_event.is_set():
                wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
                wx.CallAfter(self.announce_player, self.t("download_cancelled"))
                return
            wx.CallAfter(self.finish_download_task, task_id, "download_state_failed")
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)


    def finish_download(self, done_text: str, folder: str) -> None:
        self.set_status(done_text)
        focused = self.app_has_focus()
        if not focused:
            self.show_download_complete_notification(done_text)
        elif self.settings.popup_when_download_complete:
            self.message(done_text, wx.ICON_INFORMATION)
        else:
            self.announce_player(done_text)
        if self.settings.open_folder_after_download:
            os.startfile(folder)  # type: ignore[attr-defined]


    def download_options(
        self,
        folder: Path,
        audio_only: bool,
        title: str,
        allow_playlist: bool = False,
        task_id: str | None = None,
        cancel_event: threading.Event | None = None,
        target_path: Path | None = None,
    ) -> dict:
        progress_hook = self.make_download_progress_hook(title, audio_only, task_id=task_id, cancel_event=cancel_event)
        template = self.settings.filename_template or DEFAULT_FILENAME_TEMPLATE
        if allow_playlist and self.settings.keep_playlist_order and "%(playlist_index)" not in template:
            template = "%(playlist_index)s - " + template
        if target_path and audio_only:
            outtmpl = str(target_path.with_suffix("")) + ".%(ext)s"
        else:
            outtmpl = str(target_path) if target_path else str(folder / template)
        options = {
            "outtmpl": outtmpl,
            "quiet": self.settings.quiet_downloads,
            "noplaylist": not allow_playlist,
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
        for key, value in (("ratelimit", self.settings.rate_limit), ("proxy", self.settings.proxy), ("ffmpeg_location", self.settings.ffmpeg_location)):
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
            # Match the throughput tuning used for video downloads: more
            # concurrent DASH fragments, larger HTTP range chunks and a fatter
            # socket buffer.  Audio-only downloads were stuck on yt-dlp defaults
            # (4 fragments, no chunk size, no buffersize) and were several times
            # slower than they need to be.
            options["concurrent_fragment_downloads"] = max(self.settings.concurrent_fragments, VIDEO_DOWNLOAD_MIN_FRAGMENTS)
            options["http_chunk_size"] = VIDEO_DOWNLOAD_HTTP_CHUNK_SIZE
            options["buffersize"] = VIDEO_DOWNLOAD_BUFFER_SIZE
        else:
            video_mode = self.normalized_video_format()
            options["concurrent_fragment_downloads"] = max(self.settings.concurrent_fragments, VIDEO_DOWNLOAD_MIN_FRAGMENTS)
            options["http_chunk_size"] = VIDEO_DOWNLOAD_HTTP_CHUNK_SIZE
            options["buffersize"] = VIDEO_DOWNLOAD_BUFFER_SIZE
            options["progress_delta"] = 0.5
            options["format"] = self.video_format_selector(video_mode)
            if video_mode in {VIDEO_FORMAT_MP4, VIDEO_FORMAT_MP4_SINGLE, VIDEO_FORMAT_SMALLEST}:
                options["merge_output_format"] = "mp4"
        return options



    @staticmethod
    def collection_download_url(item: dict) -> str:
        url = str(item.get("url") or "")
        if item.get("kind") == "channel":
            url = url.rstrip("/")
            if not url.endswith("/videos"):
                url = f"{url}/videos"
        return url


    def make_download_progress_hook(self, title: str, audio_only: bool, task_id: str | None = None, cancel_event: threading.Event | None = None):
        mode = self.t("download_audio_mode" if audio_only else "download_video_mode")
        last_reported_percent = ""
        last_reported_title = ""
        last_reported_at = 0.0

        def hook(data: dict) -> None:
            nonlocal last_reported_percent, last_reported_title, last_reported_at
            if cancel_event and cancel_event.is_set():
                raise DownloadCancelled()
            status = data.get("status")
            info_dict = data.get("info_dict") or {}
            current_title = info_dict.get("title") or title
            playlist_index = info_dict.get("playlist_index")
            playlist_count = info_dict.get("playlist_count") or info_dict.get("n_entries")
            if status == "downloading":
                percent_text = str(data.get("_percent_str") or "").strip().replace("%", "")
                if not percent_text:
                    downloaded = data.get("downloaded_bytes") or 0
                    total = data.get("total_bytes") or data.get("total_bytes_estimate") or 0
                    if total:
                        percent_text = f"{(float(downloaded) / float(total)) * 100:.1f}"
                try:
                    percent_value = float(percent_text)
                    percent_text = f"{percent_value:.1f}".rstrip("0").rstrip(".")
                    percent_bucket = str(min(100, max(0, int(percent_value))))
                except (TypeError, ValueError):
                    percent_bucket = percent_text
                now = time.monotonic()
                should_report = (
                    percent_bucket != last_reported_percent
                    or current_title != last_reported_title
                    or now - last_reported_at >= 0.75
                )
                if not should_report:
                    return
                last_reported_percent = percent_bucket
                last_reported_title = current_title
                last_reported_at = now
                if task_id:
                    self.ui_queue.put(
                        (
                            "download_task",
                            {
                                "task_id": task_id,
                                "status_key": "download_state_downloading",
                                "current_title": current_title,
                                "percent": percent_text,
                                "playlist_index": playlist_index or 0,
                                "playlist_count": playlist_count or 0,
                            },
                        )
                    )
                if percent_text:
                    self.ui_queue.put(("status", self.t("download_progress", mode=mode, percent=percent_text, title=title)))
            elif status == "finished":
                if task_id:
                    self.ui_queue.put(
                        (
                            "download_task",
                            {
                                "task_id": task_id,
                                "status_key": "download_state_processing",
                                "current_title": current_title,
                                "percent": "100",
                                "playlist_index": playlist_index or 0,
                                "playlist_count": playlist_count or 0,
                            },
                        )
                    )
                self.ui_queue.put(("status", self.t("download_processing", mode=mode, title=title)))

        return hook


    def toggle_download_queue(self, audio_only: bool | None = None) -> None:
        item = self.selected_result()
        if not item:
            self.message(self.t("no_selection"))
            return
        url = item.get("url", "")
        if not url:
            self.message(self.t("no_selection"))
            return
        existing = self.download_queue.get(url)
        existing_audio_only = existing.get("audio_only") if existing and "audio_only" in existing else None
        if existing and existing_audio_only == audio_only:
            self.download_queue.pop(url, None)
            self.announce_player(self.t("download_deselected", title=item.get("title", "")))
        else:
            queued = dict(item)
            queued["audio_only"] = audio_only
            self.download_queue[url] = queued
            if audio_only is None:
                key = "selected_for_download_or_playlist"
            elif item.get("kind") in {"playlist", "channel"}:
                key = "collection_audio_selected_download" if audio_only else "collection_video_selected_download"
            else:
                key = "audio_selected_download" if audio_only else "video_selected_download"
            self.announce_player(self.t(key, title=item.get("title", "")))
        self.refresh_result_line(self.current_index)
        self.refresh_download_views()



    def download_selected_queue_item(self, audio_only: bool | None = None) -> None:
        item = self.selected_queue_item()
        if not item:
            self.announce_player(self.t("download_queue_empty"))
            return
        if item.get("queue_state") == "active":
            self.cancel_download_task(str(item.get("task_id") or ""))
            return
        if audio_only is None:
            if item.get("kind") == "rss_item":
                audio_only = True
            elif isinstance(item.get("audio_only"), bool):
                audio_only = bool(item.get("audio_only"))
            else:
                selected_format = self.choose_queue_download_format()
                if selected_format is None:
                    return
                audio_only = selected_format
        elif item.get("kind") == "rss_item":
            audio_only = True
        if item.get("kind") in {"playlist", "channel"}:
            self.download_collection(dict(item), audio_only=audio_only, remove_queued=True)
        else:
            self.start_download(audio_only, item=dict(item), remove_queued=True)


    def choose_queue_download_format(self) -> bool | None:
        choices = [self.t("download_audio"), self.t("download_video")]
        with wx.SingleChoiceDialog(self, self.t("select_download_format_message"), self.t("select_download_format"), choices) as dialog:
            dialog.SetSelection(0)
            if dialog.ShowModal() != wx.ID_OK:
                return None
            return dialog.GetSelection() == 0



    def cancel_selected_download(self) -> None:
        item = self.selected_queue_item()
        if not item or item.get("queue_state") != "active":
            self.announce_player(self.t("no_active_download"))
            return
        self.cancel_download_task(str(item.get("task_id") or ""))


    def cancel_download_task(self, task_id: str) -> None:
        cancel_event = self.download_cancel_events.get(task_id)
        task = self.active_downloads.get(task_id)
        if not cancel_event or not task:
            self.announce_player(self.t("no_active_download"))
            return
        cancel_event.set()
        task["status_key"] = "download_state_cancelled"
        self.announce_player(self.t("download_cancel_requested", title=task.get("title", "")))
        self.refresh_download_views()


    def cancel_all_downloads(self) -> None:
        if not self.active_downloads:
            self.announce_player(self.t("no_active_download"))
            return
        for task_id in list(self.active_downloads):
            cancel_event = self.download_cancel_events.get(task_id)
            if cancel_event:
                cancel_event.set()
            self.active_downloads[task_id]["status_key"] = "download_state_cancelled"
        self.announce_player(self.t("all_downloads_cancel_requested"))
        self.refresh_download_views()


    def download_all_queued(self, audio_only: bool | None = None) -> None:
        if not self.download_queue:
            self.announce_player(self.t("download_queue_empty"))
            return
        if audio_only is None and any(not isinstance(item.get("audio_only"), bool) and item.get("kind") != "rss_item" for item in self.download_queue.values()):
            audio_only = self.choose_queue_download_format()
            if audio_only is None:
                return
        items = []
        for queued_item in self.download_queue.values():
            item = dict(queued_item)
            if item.get("kind") == "rss_item":
                item["audio_only"] = True
            elif audio_only is not None:
                item["audio_only"] = audio_only
            elif not isinstance(item.get("audio_only"), bool):
                item["audio_only"] = True
            items.append(item)
        batch_folder = None
        if self.settings.ask_download_location_each_time:
            batch_folder = self.choose_download_target_folder({"title": self.t("download_all_selected"), "kind": "batch"}, collection=False)
            if batch_folder is None:
                self.set_status(self.t("download_cancelled"))
                return
            for item in items:
                item["download_folder_override"] = str(batch_folder)
        self.download_queue.clear()
        self.refresh_results_list_labels()
        if self.rss_items_screen_active:
            self.refresh_rss_items_list()
        self.announce_player(self.t("batch_download_start", count=len(items)))
        if audio_only is True:
            batch_title = self.t("download_all_as_audio")
        elif audio_only is False:
            batch_title = self.t("download_all_as_video")
        else:
            batch_title = self.t("download_all_selected")
        task_id, cancel_event = self.register_download_task({"title": batch_title, "kind": "batch"}, False, "batch", total=len(items))
        if len(items) > 5 or any(item.get("kind") in {"playlist", "channel"} for item in items):
            self.show_download_progress_dialog(task_id, batch_title)
        if self.in_queue_screen:
            self.show_download_queue()
        threading.Thread(target=self.download_batch_worker, args=(items, task_id, cancel_event, None, str(batch_folder) if batch_folder else None), daemon=True).start()

    def download_batch_worker(self, items: list[dict], task_id: str, cancel_event: threading.Event, done_text: str | None = None, finish_folder: str | None = None) -> None:
        folder = self.download_root_folder()
        last_item_folder = folder
        try:
            ytdlp = get_yt_dlp()
            if ytdlp is None:
                raise RuntimeError(self.t("missing_ytdlp"))
            folder.mkdir(parents=True, exist_ok=True)
            total = len(items)
            for index, item in enumerate(items, start=1):
                if cancel_event.is_set():
                    raise DownloadCancelled()
                audio_only = bool(item.get("audio_only"))
                mode_key = "download_audio_start" if audio_only else "download_video_start"
                self.ui_queue.put(
                    (
                        "download_task",
                        {
                            "task_id": task_id,
                            "status_key": "download_state_downloading",
                            "current_title": item.get("title", ""),
                            "completed": index - 1,
                            "total": total,
                            "percent": "",
                        },
                    )
                )
                wx.CallAfter(self.announce_player, self.t(mode_key))
                override_folder = str(item.get("download_folder_override") or "").strip()
                item_folder = Path(override_folder) if override_folder else self.download_folder_for_item(item, audio_only)
                allow_playlist = False
                url = item["url"]
                if item.get("kind") in {"playlist", "channel"}:
                    if override_folder:
                        title = item.get("title") or self.t("channel" if item.get("kind") == "channel" else "playlist")
                        item_folder = Path(override_folder) / self.safe_folder_name(str(title))
                    else:
                        item_folder = self.download_folder_for_item(item, audio_only, collection=True)
                    allow_playlist = True
                    url = self.collection_download_url(item)
                item_folder.mkdir(parents=True, exist_ok=True)
                last_item_folder = item_folder
                options = self.download_options(item_folder, audio_only, item.get("title", ""), allow_playlist=allow_playlist, task_id=task_id, cancel_event=cancel_event)
                self.ydl_download_urls([url], options)
                self.ui_queue.put(("download_task", {"task_id": task_id, "completed": index, "total": total}))
            wx.CallAfter(self.finish_download_task, task_id)
            wx.CallAfter(self.finish_batch_download, finish_folder or str(last_item_folder if len(items) == 1 else folder), done_text)
        except DownloadCancelled:
            wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
            wx.CallAfter(self.announce_player, self.t("download_cancelled"))
        except Exception as exc:
            if cancel_event.is_set():
                wx.CallAfter(self.finish_download_task, task_id, "download_state_cancelled")
                wx.CallAfter(self.announce_player, self.t("download_cancelled"))
                return
            wx.CallAfter(self.finish_download_task, task_id, "download_state_failed")
            wx.CallAfter(self.message, self.t("download_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)


    def finish_batch_download(self, folder: str, done_text: str | None = None) -> None:
        done_text = done_text or self.t("batch_download_done")
        self.set_status(done_text)
        focused = self.app_has_focus()
        if not focused:
            self.show_download_complete_notification(done_text)
        elif self.settings.popup_when_download_complete:
            self.message(done_text, wx.ICON_INFORMATION)
        else:
            self.announce_player(done_text)
        if self.settings.open_folder_after_download:
            os.startfile(folder)  # type: ignore[attr-defined]



    def choose_download_folder(self) -> None:
        with wx.DirDialog(self, self.t("choose_download_folder"), self.settings.download_folder) as dialog:
            if dialog.ShowModal() == wx.ID_OK:
                self.settings.download_folder = dialog.GetPath()
                if hasattr(self, "controls") and "download_folder" in self.controls:
                    self.controls["download_folder"].SetValue(self.settings.download_folder)
                self.save_settings()
                self.set_status(self.t("settings_saved"))

