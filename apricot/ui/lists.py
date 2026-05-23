from apricot.constants import *
import re
import wx
import os
from pathlib import Path
from apricot.ui.misc import MiscUI

_RE_URL_SCHEME = re.compile(r"^[a-z][a-z0-9+.-]*://", re.IGNORECASE)

class ListsUI:
    def item_is_local_media(self, item: dict | None) -> bool:
        if not isinstance(item, dict):
            return False
        if str(item.get("kind") or "").strip().lower() == "local_file":
            return True
        value = str(item.get("path") or item.get("url") or "").strip()
        return bool(value and self.local_media_path_from_input(value))

    @staticmethod
    def listbox_matches(listbox: wx.ListBox, labels: list[str]) -> bool:
        try:
            if listbox.GetCount() != len(labels):
                return False
            return all(listbox.GetString(index) == labels[index] for index in range(len(labels)))
        except RuntimeError:
            return False

    def set_listbox_items(self, listbox: wx.ListBox, labels: list[str], selection: int = 0) -> bool:
        labels = [str(label) for label in labels]
        if not labels:
            return False
        target_selection = min(max(0, selection), len(labels) - 1)
        current_selection = listbox.GetSelection()
        suppress_results_selection = listbox is getattr(self, "results_list", None)
        if self.listbox_matches(listbox, labels):
            if current_selection != target_selection:
                if suppress_results_selection:
                    self.results_selection_update_suppressed = True
                try:
                    listbox.SetSelection(target_selection)
                finally:
                    if suppress_results_selection:
                        wx.CallAfter(self.clear_results_selection_update_suppression)
                return True
            return False
        listbox.Freeze()
        try:
            if suppress_results_selection:
                self.results_selection_update_suppressed = True
            listbox.Clear()
            for label in labels:
                listbox.Append(label)
            listbox.SetSelection(target_selection)
        finally:
            listbox.Thaw()
            if suppress_results_selection:
                wx.CallAfter(self.clear_results_selection_update_suppression)
        return True

    def append_listbox_items(self, listbox: wx.ListBox, labels: list[str], previous_count: int, selection: int) -> bool:
        labels = [str(label) for label in labels]
        if previous_count < 0 or previous_count > len(labels):
            return False
        try:
            if listbox.GetCount() != previous_count:
                return False
        except RuntimeError:
            return False
        target_selection = min(max(0, selection), len(labels) - 1)
        suppress_results_selection = listbox is getattr(self, "results_list", None)
        listbox.Freeze()
        try:
            if suppress_results_selection:
                self.results_selection_update_suppressed = True
            for label in labels[previous_count:]:
                listbox.Append(label)
            if listbox.GetSelection() != target_selection:
                listbox.SetSelection(target_selection)
        finally:
            listbox.Thaw()
            if suppress_results_selection:
                wx.CallAfter(self.clear_results_selection_update_suppression)
        return True

    def direct_link_item(self) -> dict | None:
        if not hasattr(self, "direct_link_ctrl"):
            return None
        url = self.direct_link_ctrl.GetValue().strip()
        if not url:
            return None
        if not _RE_URL_SCHEME.match(url):
            url = "https://" + url
        return {
            "title": url,
            "url": url,
            "webpage_url": url,
            "kind": "video",
            "type": self.t("direct_link"),
            "channel": "",
        }

    def show_user_playlists(self) -> None:
        self.last_activated_menu_action = self.show_user_playlists
        self.in_main_menu = False
        self.in_queue_screen = False
        self.search_screen_active = False
        self.favorites_screen_active = False
        self.history_screen_active = False
        self.subscriptions_screen_active = False
        self.rss_feeds_screen_active = False
        self.rss_items_screen_active = False
        self.podcast_search_screen_active = False
        self.user_playlists_screen_active = True
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.folder_screen_active = False
        self.clear()
        self.add_background_player_section()
        playlist_actions = [
            (self.t("back"), self.show_main_menu),
            (self.t("create_playlist"), self.create_user_playlist_dialog),
        ]
        if self.user_playlists:
            playlist_actions.extend(
                [
                    (self.t("open_playlist"), self.open_selected_user_playlist),
                    (self.t("download_user_playlist"), self.download_selected_user_playlist),
                    (self.t("remove_playlist"), self.remove_selected_user_playlist),
                ]
            )
        self.add_button_row(playlist_actions)
        label = wx.StaticText(self.panel, label=self.t("playlists"))
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.user_playlist_list = wx.ListBox(self.panel, choices=[])
        self.user_playlist_list.SetName(self.t("playlists"))
        self.user_playlist_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.open_selected_user_playlist())
        self.user_playlist_list.Bind(wx.EVT_CONTEXT_MENU, self.open_user_playlists_context_menu)
        self.user_playlist_list.Bind(wx.EVT_KEY_DOWN, self.on_user_playlists_key)
        self.root_sizer.Add(self.user_playlist_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_user_playlists()
        self.panel.Layout()
        self.focus_later(self.user_playlist_list)

    def refresh_user_playlists(self) -> None:
        if not hasattr(self, "user_playlist_list"):
            return
        try:
            if self.user_playlists:
                labels = [self.user_playlist_line(playlist) for playlist in self.user_playlists]
                index = min(max(0, self.current_user_playlist_index), len(self.user_playlists) - 1)
                self.set_listbox_items(self.user_playlist_list, labels, index)
            else:
                self.set_listbox_items(self.user_playlist_list, [self.t("no_playlists")], 0)
                self.set_status(self.t("no_playlists"))
        except RuntimeError:
            pass

    def user_playlist_line(self, playlist: dict) -> str:
        count = len(playlist.get("items") or [])
        return f"{playlist.get('title', '')} | {count} {self.t('video')}"

    def selected_user_playlist(self) -> dict | None:
        if not hasattr(self, "user_playlist_list"):
            return None
        index = self.user_playlist_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.user_playlists):
            return None
        self.current_user_playlist_index = index
        return self.user_playlists[index]

    def on_user_playlists_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "create_playlist"):
            self.create_user_playlist_dialog()
        elif self.shortcut_matches(event, "open_selected"):
            self.open_selected_user_playlist()
        elif self.shortcut_matches(event, "remove_selected"):
            self.remove_selected_user_playlist()
        elif self.context_menu_shortcut_matches(event):
            self.open_user_playlists_context_menu()
        else:
            event.Skip()

    def create_user_playlist_dialog(self, initial_item: dict | None = None) -> int | None:
        with wx.TextEntryDialog(self, self.t("playlist_name"), self.t("create_playlist")) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                return None
            title = dialog.GetValue().strip()
        if not title:
            return None
        if any(str(playlist.get("title") or "").lower() == title.lower() for playlist in self.user_playlists):
            self.announce_player(self.t("playlist_exists"))
            return None
        playlist = {"title": title, "items": [], "created_at": time.time(), "updated_at": time.time()}
        self.user_playlists.append(playlist)
        self.current_user_playlist_index = len(self.user_playlists) - 1
        if initial_item:
            playlist["items"].append(self.playlist_item_from_media(initial_item))
        self.save_user_playlists()
        if self.user_playlists_screen_active:
            wx.CallAfter(self.show_user_playlists)
        else:
            self.refresh_user_playlists()
        self.announce_player(self.t("playlist_created", title=title))
        return self.current_user_playlist_index

    def selected_notification(self) -> dict | None:
        if not hasattr(self, "notification_list"):
            return None
        index = self.notification_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.notifications):
            return None
        return self.notifications[index]

    def selected_notification_item(self) -> dict | None:
        notification = self.selected_notification()
        if not notification:
            return None
        item = notification.get("item")
        return dict(item) if isinstance(item, dict) else None

    def open_selected_notification(self) -> None:
        item = self.selected_notification_item()
        if not item or not item.get("url"):
            self.announce_player(self.t("notification_center_empty"))
            return
        self.player_return_screen = "notification_center"
        self.player_return_data = {}
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(str(item.get("url") or ""), str(item.get("title") or ""))

    def clear_selected_notification(self) -> None:
        if not hasattr(self, "notification_list"):
            return
        index = self.notification_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.notifications):
            del self.notifications[index]
            self.save_notifications()
            self.refresh_notification_center()

    def selected_queue_item(self) -> dict | None:
        if not hasattr(self, "queue_list"):
            return None
        try:
            index = self.queue_list.GetSelection()
        except RuntimeError:
            return None
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.queue_items):
            return None
        return self.queue_items[index]

    @staticmethod
    def result_details_key(event: wx.KeyEvent) -> bool:
        return (
            event.GetKeyCode() == wx.WXK_SPACE
            and not event.ControlDown()
            and not event.ShiftDown()
            and not event.AltDown()
        )

    def announce_selected_result_details(self) -> None:
        item = self.selected_result()
        if not item:
            self.announce_player(self.t("no_selection"))
            return
        self.announce_player(self.result_details_text(item))

    def result_details_text(self, item: dict) -> str:
        kind = str(item.get("kind") or "")
        title = str(item.get("title") or "")
        if kind == "local_file":
            return self.t(
                "local_file_result_details",
                title=title,
                format=str(item.get("ext") or self.t("file_format_unknown")),
                folder=str(item.get("folder") or item.get("channel") or self.t("unknown")),
                path=str(item.get("path") or item.get("url") or item.get("webpage_url") or ""),
            )
        if kind == "playlist":
            count = self.playlist_count_text(item) or self.t("playlist")
            return self.t("playlist_result_details", title=title, count=count)
        if kind == "channel":
            return self.t("channel_result_details", title=title)
        return self.t(
            "results_details",
            title=title,
            duration=str(item.get("duration") or self.t("unknown")),
            channel=str(item.get("channel") or self.t("unknown")),
            views=str(item.get("views") or self.t("unknown")),
            age=str(item.get("age") or self.t("uploaded_unknown")),
            type=self.item_type_label(item),
        )

    def normalize_podcast_result(self, item: dict) -> dict:
        title = str(item.get("collectionName") or item.get("trackName") or "").strip()
        author = str(item.get("artistName") or "").strip()
        feed_url = str(item.get("feedUrl") or "").strip()
        page_url = str(item.get("collectionViewUrl") or item.get("trackViewUrl") or "").strip()
        genre = str(item.get("primaryGenreName") or "").strip()
        count = int(item.get("trackCount") or 0)
        return {
            "title": title or feed_url or page_url,
            "channel": author,
            "author": author,
            "genre": genre,
            "episode_count": count,
            "url": feed_url,
            "webpage_url": page_url or feed_url,
            "kind": "podcast",
            "type": self.t("rss_feeds"),
        }

    def podcast_result_line(self, item: dict) -> str:
        count = int(item.get("episode_count") or 0)
        parts = [
            item.get("title", ""),
            f"{self.t('podcast_author')}: {item.get('author', '')}" if item.get("author") else "",
            f"{self.t('podcast_genre')}: {item.get('genre', '')}" if item.get("genre") else "",
            self.t("podcast_episode_count", count=count) if count else "",
        ]
        return " | ".join(part for part in parts if part)

    def selected_podcast_result(self) -> dict | None:
        if not hasattr(self, "podcast_result_list"):
            return None
        index = self.podcast_result_list.GetSelection()
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.podcast_search_results):
            return None
        return self.podcast_search_results[index]

    def add_selected_podcast_result(self) -> None:
        item = self.selected_podcast_result()
        if not item:
            self.announce_player(self.t("podcast_search_empty"))
            return
        self.add_rss_feed_url(str(item.get("url") or ""))

    def parse_atom_item(self, entry: ET.Element, base_url: str, feed_title: str) -> dict:
        title = self.child_text(entry, "title")
        page_url = self.atom_link(entry, base_url, {"alternate", ""})
        media_url = self.atom_link(entry, base_url, {"enclosure"})
        timestamp = self.parse_feed_timestamp(self.child_text(entry, "published") or self.child_text(entry, "updated"))
        description = self.strip_html(self.child_text(entry, "summary") or self.child_text(entry, "content"))
        item_id = self.child_text(entry, "id")
        url = media_url or page_url or self.absolute_url(item_id, base_url)
        duration = self.child_text(entry, "duration")
        chapters_url, chapters_type = self.podcast_chapters_reference(entry, base_url)
        return {
            "title": title or page_url or media_url,
            "url": url,
            "webpage_url": page_url or url,
            "media_url": media_url,
            "description": description,
            "duration": duration,
            "chapters": self.parse_inline_podcast_chapters(entry),
            "chapters_url": chapters_url,
            "chapters_type": chapters_type,
            "timestamp": timestamp,
            "channel": feed_title,
            "kind": "rss_item",
            "type": self.t("podcast_episode"),
        }

    def item_type_label(self, item: dict | None, default: str | None = None) -> str:
        if isinstance(item, dict) and str(item.get("kind") or "video") == "video" and self.metadata_is_live_stream(item):
            return self.t("live_stream")
        return str((item or {}).get("type") or default or self.t("video"))

    def remember_user_result_selection(self, index: int | None = None) -> None:
        if not self.results:
            self.last_user_result_index = 0
            self.last_user_result_identity = ""
            return
        if index is None:
            index = self.current_results_selection(self.last_user_result_index)
        index = min(max(0, int(index)), len(self.results) - 1)
        self.last_user_result_index = index
        self.last_user_result_identity = self.result_identity_at(index)

    def result_identity_at(self, index: int) -> str:
        if index < 0 or index >= len(self.results):
            return ""
        return self.result_identity_for_item(self.results[index])

    @staticmethod
    def result_identity_for_item(item: dict | None) -> str:
        item = item or {}
        return str(item.get("url") or item.get("webpage_url") or item.get("id") or item.get("title") or "")

    def result_index_for_identity(self, identity: str, fallback: int, limit: int | None = None) -> int:
        if identity:
            items = self.results[:limit] if limit is not None else self.results
            for index, item in enumerate(items):
                if self.result_identity_for_item(item) == identity:
                    return index
        if not self.results:
            return 0
        return min(max(0, fallback), len(self.results) - 1)

    def stable_selected_result(self) -> dict | None:
        if not hasattr(self, "results_list") or not self.results:
            return None
        selection = self.current_results_selection(self.last_user_result_index)
        if self.focus_in_results_control(wx.Window.FindFocus()) and self.last_user_result_identity:
            selection = self.result_index_for_identity(self.last_user_result_identity, self.last_user_result_index)
        if selection < 0 or selection >= len(self.results):
            return None
        self.current_index = selection
        try:
            if self.results_list.GetSelection() != selection:
                self.results_selection_update_suppressed = True
                self.results_list.SetSelection(selection)
                wx.CallAfter(self.clear_results_selection_update_suppression)
        except RuntimeError:
            pass
        return self.results[selection]

    def start_result_metadata_hydration(self) -> None:
        candidates: list[dict] = []
        for item in list(self.results):
            url = str(item.get("url") or "")
            if item.get("kind") != "video" or not url or url in self.metadata_hydration_urls:
                continue
            if (item.get("timestamp") or item.get("upload_date")) and item.get("view_count") not in (None, ""):
                continue
            candidates.append(dict(item))
            self.metadata_hydration_urls.add(url)
            if len(candidates) >= RESULT_METADATA_HYDRATION_BATCH:
                break
        if candidates:
            threading.Thread(target=self.result_metadata_worker, args=(candidates,), daemon=True).start()

    def result_metadata_worker(self, items: list[dict]) -> None:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            return
        options = {"quiet": True, "skip_download": True, "noplaylist": True}
        try:
            for item in items:
                url = str(item.get("url") or "")
                if not url:
                    continue
                try:
                    info = self.ydl_extract_info(url, options, download=False, allow_cookie_retry=False)
                    payload = self.metadata_from_info(info, item)
                    self.ui_queue.put(("result_metadata", payload))
                except Exception:
                    continue
        except Exception:
            return

    @staticmethod
    def popular_result_sort_key(item: dict) -> tuple[int, int, str]:
        timestamp = item.get("timestamp")
        upload_date = item.get("upload_date")
        try:
            age_value = int(timestamp or 0)
        except (TypeError, ValueError):
            age_value = 0
        if not age_value:
            try:
                age_value = int(str(upload_date or "0"))
            except (TypeError, ValueError):
                age_value = 0
        return (
            MiscUI.numeric_view_count(item.get("view_count")),
            age_value,
            str(item.get("title") or "").lower(),
        )

    def sorted_popular_channel_results(self, results: list[dict]) -> list[dict]:
        return self.sort_popular_results([self.hydrate_video_metadata_for_popular(item) for item in results])

    def apply_result_metadata(self, payload: dict) -> None:
        url = str(payload.get("url") or "")
        if not url:
            return
        for collection in (self.results, self.all_results, self.return_results, self.return_all_results):
            for item in collection:
                if item.get("url") == url:
                    item.update({key: value for key, value in payload.items() if value not in (None, "")})
        if hasattr(self, "results_list"):
            for index, item in enumerate(self.results):
                if item.get("url") == url:
                    self.refresh_result_line(index)
                    break

    def result_line(self, index: int, item: dict) -> str:
        if item.get("kind") == "local_file":
            title = str(item.get("relative_path") or item.get("title") or "")
            return self.t(
                "local_file_result_line",
                title=title,
                format=str(item.get("ext") or self.t("file_format_unknown")),
                folder=str(item.get("folder") or item.get("channel") or ""),
            )
        if item.get("kind") in {"playlist", "channel"}:
            parts = [item.get("title", ""), item.get("type", self.t("playlist" if item.get("kind") == "playlist" else "channel"))]
            if item.get("kind") == "playlist":
                count_text = self.playlist_count_text(item)
                if count_text:
                    parts.append(count_text)
        else:
            parts = [
                str(item.get("title") or ""),
                f"{self.t('channel')}: {item.get('channel') or ''}",
                f"{self.t('views')}: {item.get('views') or ''}",
                item.get("age") or self.t("uploaded_unknown"),
                item.get("duration", ""),
                self.item_type_label(item),
            ]
        queued = self.download_queue.get(item.get("url", ""))
        if queued:
            parts.append(self.queue_mode_label(queued))
        return " | ".join(part for part in parts if part)

    def selected_result(self) -> dict | None:
        if not hasattr(self, "results_list"):
            return None
        try:
            index = self.results_list.GetSelection()
        except RuntimeError:
            return None
        if self.focus_in_results_control(wx.Window.FindFocus()) and getattr(self, "last_user_result_identity", ""):
            index = self.result_index_for_identity(self.last_user_result_identity, self.last_user_result_index)
        if index == wx.NOT_FOUND or index < 0 or index >= len(self.results):
            return None
        self.current_index = index
        return self.results[index]

    def load_trending_results(self) -> None:
        country_index = self.trending_country_choice.GetSelection() if hasattr(self, "trending_country_choice") else 0
        category_index = self.trending_category_choice.GetSelection() if hasattr(self, "trending_category_choice") else 0
        country_code = TRENDING_COUNTRIES[country_index][0] if 0 <= country_index < len(TRENDING_COUNTRIES) else "global"
        category_code = TRENDING_CATEGORIES[category_index][0] if 0 <= category_index < len(TRENDING_CATEGORIES) else "all"
        self.last_trending_country_index = min(max(0, country_index), len(TRENDING_COUNTRIES) - 1)
        self.last_trending_category_index = min(max(0, category_index), len(TRENDING_CATEGORIES) - 1)
        country_label = dict(TRENDING_COUNTRIES).get(country_code, country_code)
        category_label = self.t(f"trending_{category_code}")
        self.last_search_query = f"official trending {country_code} {category_code}"
        self.last_search_type_index = 1
        self.current_search_type_code = "Video"
        self.collection_url = ""
        self.collection_result_type = ""
        self.collection_sort_mode = ""
        self.collection_channel_id = ""
        self.collection_fully_loaded = False
        self.search_results_stack = []
        self.loading_more_results = False
        self.dynamic_fetch_enabled = False
        self.metadata_hydration_urls.clear()
        self.set_status(self.t("trending_loading_official", country=country_label, category=category_label))
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.trending_worker, args=(country_code, category_code, generation), daemon=True).start()

    def local_media_item(self, path: Path, base_folder: Path | None = None) -> dict:
        folder = path.parent.name
        relative_path = path.name
        if base_folder is not None:
            try:
                relative_path = str(path.relative_to(base_folder))
            except ValueError:
                relative_path = path.name
        return {
            "title": path.stem or path.name,
            "url": str(path),
            "webpage_url": str(path),
            "kind": "local_file",
            "type": self.t("local_media"),
            "channel": folder,
            "folder": folder,
            "ext": path.suffix.lstrip(".").lower(),
            "path": str(path),
            "relative_path": relative_path,
            "description": str(path),
        }

    def cached_local_folder_items(self, folder: Path) -> list[dict]:
        return [dict(item) for item in self.local_folder_cache.get(self.local_folder_cache_key(folder), [])]

    def cache_local_folder_items(self, folder: Path, items: list[dict]) -> None:
        key = self.local_folder_cache_key(folder)
        self.local_folder_cache[key] = [dict(item) for item in items]

    def selected_local_folder_items(self) -> list[dict]:
        if (self.folder_screen_active or self.player_return_screen == "folder") and getattr(self, "current_local_folder_items", None):
            return [dict(item) for item in self.current_local_folder_items if item.get("kind") == "local_file" and item.get("url")]
        return [dict(item) for item in (self.all_results or self.results) if item.get("kind") == "local_file" and item.get("url")]

    @staticmethod
    def show_sizer_items(sizer: wx.Sizer, show: bool) -> None:
        for child in sizer.GetChildren():
            window = child.GetWindow()
            if window:
                window.Show(show)

    def active_item(self) -> dict | None:
        focus = wx.Window.FindFocus()
        if self.focus_in_results_control(focus):
            return self.selected_result()
        if self.current_video_item and (self.in_player_screen or self.focus_in_background_player_controls(focus)):
            return self.current_video_item
        if self.in_queue_screen:
            item = self.selected_queue_item()
            if item:
                return item
        if self.favorites_screen_active:
            return self.selected_favorite()
        if self.history_screen_active:
            return self.selected_history_item()
        if self.subscriptions_screen_active:
            return self.selected_subscription()
        if self.rss_feeds_screen_active:
            return self.selected_rss_feed()
        if self.rss_items_screen_active:
            return self.selected_rss_item()
        if self.podcast_search_screen_active:
            return self.selected_podcast_result()
        if self.user_playlists_screen_active:
            return self.selected_user_playlist()
        if self.user_playlist_items_screen_active:
            return self.selected_user_playlist_item()
        if self.notification_center_screen_active:
            return self.selected_notification_item()
        if self.direct_link_screen_active:
            return self.direct_link_item()
        return self.selected_result()

    def open_item_channel(self, item: dict | None = None) -> None:
        explicit_item = item is not None
        channel_item = self.youtube_channel_item_for_video(item if explicit_item else self.active_item())
        if channel_item is None and not explicit_item and self.player_is_active():
            channel_item = self.youtube_channel_item_for_video(self.current_video_item or self.current_video_info)
        if not channel_item:
            self.announce_player(self.t("no_channel"))
            return
        self.open_channel_tab(channel_item, "videos", push_state=True)

    def play_relative_item(self, delta: int, preserve_focus: bool = False) -> None:
        sequence_active = self.current_player_sequence_active()
        if delta > 0 and not sequence_active:
            queued_item = self.pop_next_playback_queue_item()
            if queued_item:
                self.open_playback_queue_item(queued_item, announce_start=True, preserve_focus=preserve_focus)
                return
        if delta < 0:
            item = self.relative_player_item(-1)
            if not item:
                self.announce_player(self.t("no_previous_item"))
                return
        else:
            item = self.relative_player_item(1)
            if not item:
                if not sequence_active and self.request_player_next_dynamic_load(preserve_focus=preserve_focus):
                    self.set_status(self.t("loading_more_results"))
                    return
                self.announce_player(self.t("no_next_item"))
                return
        self.open_relative_player_item(item, announce_start=True, preserve_focus=preserve_focus)

    def playable_queue_item(self, item: dict | None) -> dict | None:
        if not item or item.get("kind") in {"channel", "playlist"}:
            return None
        url = str(item.get("url") or item.get("webpage_url") or "").strip()
        if not url:
            return None
        return self.playlist_item_from_media(dict(item))

    def open_library_item(self, item: dict, screen: str) -> None:
        kind = item.get("kind")
        if kind in {"channel", "playlist"}:
            self.search_results_stack.append({"screen": screen})
            self.last_search_query = item.get("title", "")
            self.last_search_type_index = 0
            self.current_search_type_code = "Video"
            self.show_search(restore_search=True)
            if kind == "channel":
                self.open_channel_videos(item, push_state=False)
            else:
                self.open_playlist_videos(item, push_state=False)
            return
        url = str(item.get("url") or "")
        if not url:
            self.announce_player(self.t("no_selection"))
            return
        self.clear_player_sequence()
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.player_return_screen = screen
        self.player_return_data = {}
        self.play_url(url, item.get("title", ""))

    def clip_output_folder_for_item(self, item: dict) -> Path:
        if item.get("kind") == "rss_item":
            folder = self.podcasts_download_folder()
        else:
            folder = self.music_download_folder()
        return folder / "clips"

    def result_limit_labels(self, options: list[str]) -> list[str]:
        return [self.t("dynamic_results") if option == "0" else option for option in options]

    def open_selected_in_browser(self) -> None:
        item = self.active_item()
        if item:
            import_module("webbrowser").open(str(item.get("webpage_url") or item.get("url") or ""))

    def copy_selected_url(self) -> None:
        item = self.active_item()
        if item:
            self.copy_url_to_clipboard(str(item.get("url") or ""))

    def copy_item_url(self, item: dict | None) -> None:
        if item:
            self.copy_url_to_clipboard(str(item.get("url") or ""))

    def refresh_result_line(self, index: int) -> None:
        if self.result_line_update_should_defer(index):
            self.deferred_result_line_updates.add(index)
            return
        self.refresh_result_line_now(index)

    def result_line_update_should_defer(self, index: int) -> bool:
        if not hasattr(self, "results_list") or index < 0:
            return False
        try:
            return wx.Window.FindFocus() is self.results_list and self.results_list.GetSelection() == index
        except RuntimeError:
            return False

    def apply_deferred_result_line_updates(self, exclude_index: int | None = None) -> None:
        if not self.deferred_result_line_updates:
            return
        for index in sorted(list(self.deferred_result_line_updates)):
            if exclude_index is not None and index == exclude_index:
                continue
            self.deferred_result_line_updates.discard(index)
            self.refresh_result_line_now(index)

    def refresh_result_line_now(self, index: int) -> None:
        if not hasattr(self, "results_list") or index < 0 or index >= len(self.results):
            return
        try:
            line = self.result_line(index, self.results[index])
            if self.results_list.GetString(index) == line:
                return
            selection = self.results_list.GetSelection()
            self.results_list.Freeze()
            try:
                self.results_list.SetString(index, line)
                if selection != wx.NOT_FOUND and selection != self.results_list.GetSelection():
                    self.results_list.SetSelection(min(max(0, selection), len(self.results) - 1))
            finally:
                self.results_list.Thaw()
        except RuntimeError:
            pass

    def remove_selected_queue_item(self) -> None:
        item = self.selected_queue_item()
        if not item:
            self.announce_player(self.t("download_queue_empty"))
            return
        if item.get("queue_state") == "active":
            self.cancel_download_task(str(item.get("task_id") or ""))
            return
        self.remove_queued_url(item.get("url", ""), announce=True)

    def record_history(self, item: dict, action: str) -> None:
        if not self.settings.enable_history:
            return
        url = str(item.get("url") or "")
        if not url:
            return
        entry = {
            "title": item.get("title", ""),
            "channel": item.get("channel", ""),
            "channel_url": item.get("channel_url", ""),
            "channel_id": item.get("channel_id", ""),
            "url": url,
            "kind": item.get("kind", "video"),
            "type": self.item_type_label(item),
            "live_status": item.get("live_status", ""),
            "is_live": bool(item.get("is_live", False)),
            "action": action,
            "timestamp": time.time(),
        }
        self.history = [existing for existing in self.history if existing.get("url") != url]
        self.history.insert(0, entry)
        self.trim_history()
        self.save_history()

    def trim_history(self) -> None:
        limit = max(10, int(getattr(self.settings, "history_limit", 500) or 500))
        if len(self.history) > limit:
            self.history = self.history[:limit]
            self.save_history()

