from apricot.constants import *
import wx
import os
from pathlib import Path

class MenusUI:
    def show_main_menu(self) -> None:
        self.in_main_menu = True
        self.in_queue_screen = False
        self.search_screen_active = False
        self.trending_screen_active = False
        self.favorites_screen_active = False
        self.bookmarks_screen_active = False
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
        self.settings_section_index = 0
        self.clear()
        title = wx.StaticText(self.panel, label=self.t("main_menu"))
        self.root_sizer.Add(title, 0, wx.ALL, 4)
        self.menu_actions = self.build_main_menu_actions()
        self.menu_list = wx.ListBox(self.panel, choices=[item[0] for item in self.menu_actions])
        self.menu_list.SetName(self.t("main_menu"))
        selection_index = 0
        if hasattr(self, "last_activated_menu_action"):
            for i, item in enumerate(self.menu_actions):
                if item[1] == self.last_activated_menu_action:
                    selection_index = i
                    break
        self.menu_list.SetSelection(selection_index)
        self.menu_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.activate_menu())
        self.menu_list.Bind(wx.EVT_KEY_DOWN, self.on_menu_key)
        self.root_sizer.Add(self.menu_list, 1, wx.EXPAND | wx.ALL, 4)
        self.add_button_row([(self.t("open"), self.activate_menu)])
        self.panel.Layout()
        self.focus_later(self.menu_list)

    def build_main_menu_actions(self) -> list[tuple[str, callable]]:
        actions: list[tuple[str, callable]] = []
        pending_version = self.pending_app_update_version()
        if pending_version:
            actions.append((self.t("app_update_menu_item", version=pending_version), self.open_pending_app_update))
        download_count = len(self.download_queue) + len(self.active_downloads)
        if download_count:
            label = self.label_with_shortcut(f"{self.t('current_downloads')} ({download_count})", "open_current_downloads", "\t")
            actions.append((label, self.show_download_queue))
        if self.playback_queue:
            label = self.label_with_shortcut(f"{self.t('playback_queue')} ({len(self.playback_queue)})", "open_playback_queue", "\t")
            actions.append((label, self.show_playback_queue))
        if self.last_player_session_available():
            actions.append((self.t("resume_last_session"), self.resume_last_player_session))
        primary_actions = [
            (self.menu_label_with_shortcut("search_youtube", "open_search"), self.show_search),
            (self.menu_label_with_shortcut("play_folder", "open_play_from_folder"), self.show_play_from_folder),
            (self.menu_label_with_shortcut("play_file", "open_play_file"), self.show_play_file),
            (self.menu_label_with_shortcut("direct_link", "open_direct_link"), self.show_direct_link),
            (self.menu_label_with_shortcut("favorites", "open_favorites"), self.show_favorites),
            (self.menu_label_with_shortcut("bookmarks", "open_bookmarks"), self.show_bookmarks),
            (self.menu_label_with_shortcut("playlists", "open_playlists"), self.show_user_playlists),
            (self.menu_label_with_shortcut("subscriptions", "open_subscriptions"), self.show_subscriptions),
            (self.menu_label_with_shortcut("notification_center", "new_subscription_videos"), self.show_notification_center),
        ]
        if getattr(self.settings, "enable_trending", False):
            primary_actions.insert(1, (self.t("trending"), self.show_trending))
        actions.extend(primary_actions)
        if self.settings.enable_history:
            actions.append((self.menu_label_with_shortcut("history", "open_history"), self.show_history))
        if self.settings.enable_podcasts_rss:
            actions.append((self.menu_label_with_shortcut("rss_feeds", "open_podcasts_rss"), self.show_rss_feeds))
        actions.extend([
            (self.t("file_converter"), self.show_file_converter),
            (self.t("folder_converter"), self.show_folder_converter),
            (self.menu_label_with_shortcut("copy_diagnostic_report", "copy_diagnostic_report"), self.copy_diagnostic_report),
            (self.menu_label_with_shortcut("settings", "open_settings"), self.show_settings),
            (self.t("exit"), self.quit_application),
        ])
        return actions

    def on_menu_key(self, event: wx.KeyEvent) -> None:
        if self.is_modifier_only_event(event):
            return
        if self.shortcut_matches(event, "open_selected"):
            self.activate_menu()
            return
        if self.handle_global_navigation_shortcut(event, self.menu_list):
            return
        event.Skip()

    def activate_menu(self) -> None:
        index = self.menu_list.GetSelection()
        if index != wx.NOT_FOUND and 0 <= index < len(self.menu_actions):
            self.last_activated_menu_action = self.menu_actions[index][1]
            self.menu_actions[index][1]()

    def action_finder_actions(self) -> list[tuple[str, callable]]:
        actions = [
            (self.menu_label_with_shortcut("main_menu", "open_main_menu"), self.show_main_menu),
            (self.menu_label_with_shortcut("search_youtube", "open_search"), self.show_search),
            (self.menu_label_with_shortcut("play_folder", "open_play_from_folder"), self.show_play_from_folder),
            (self.menu_label_with_shortcut("play_file", "open_play_file"), self.show_play_file),
            (self.menu_label_with_shortcut("direct_link", "open_direct_link"), self.show_direct_link),
            (self.menu_label_with_shortcut("favorites", "open_favorites"), self.show_favorites),
            (self.menu_label_with_shortcut("bookmarks", "open_bookmarks"), self.show_bookmarks),
            (self.menu_label_with_shortcut("playlists", "open_playlists"), self.show_user_playlists),
            (self.menu_label_with_shortcut("subscriptions", "open_subscriptions"), self.show_subscriptions),
            (self.menu_label_with_shortcut("notification_center", "new_subscription_videos"), self.show_notification_center),
            (self.menu_label_with_shortcut("playback_queue", "open_playback_queue"), self.show_playback_queue),
            (self.t("file_converter"), self.show_file_converter),
            (self.t("folder_converter"), self.show_folder_converter),
            (self.menu_label_with_shortcut("copy_diagnostic_report", "copy_diagnostic_report"), self.copy_diagnostic_report),
            (self.menu_label_with_shortcut("settings", "open_settings"), self.show_settings),
        ]
        if self.last_player_session_available():
            actions.insert(2, (self.t("resume_last_session"), self.resume_last_player_session))
        if getattr(self.settings, "enable_trending", False):
            actions.insert(2, (self.t("trending"), self.show_trending))
        if self.settings.enable_history:
            actions.append((self.menu_label_with_shortcut("history", "open_history"), self.show_history))
        if self.settings.enable_podcasts_rss:
            actions.append((self.menu_label_with_shortcut("rss_feeds", "open_podcasts_rss"), self.show_rss_feeds))
        if self.player_is_active():
            item = dict(self.current_video_item or self.current_video_info or {})
            is_local_media = self.is_local_media_item(item)
            is_youtube = self.is_youtube_url(str(item.get("url") or item.get("webpage_url") or ""))
            player_actions = [
                (self.menu_label_with_shortcut("pause" if not self.player_paused else "play", "player_play_pause"), self.player_play_pause),
                (self.menu_label_with_shortcut("previous", "player_previous"), lambda: self.play_relative_item(-1, preserve_focus=True)),
                (self.menu_label_with_shortcut("next", "player_next"), lambda: self.play_relative_item(1, preserve_focus=True)),
                (self.menu_label_with_shortcut("copy_path" if is_local_media else "copy_link", "player_copy_link"), self.copy_current_player_url),
                (self.menu_label_with_shortcut("show_video_details", "player_details"), self.show_video_details),
                (self.menu_label_with_shortcut("output_devices", "player_output_devices"), self.show_output_devices),
                (self.menu_label_with_shortcut("fullscreen", "player_fullscreen"), lambda: self.toggle_player_fullscreen(announce=True)),
                (self.menu_label_with_shortcut("equalizer", "player_equalizer"), self.show_player_equalizer),
                (self.menu_label_with_shortcut("audio_normalization", "player_replaygain"), self.cycle_replaygain_mode),
                (self.menu_label_with_shortcut("add_bookmark", "player_add_bookmark"), self.add_current_bookmark),
                (self.menu_label_with_shortcut("bookmarks", "player_bookmarks"), self.show_player_bookmarks),
                (self.menu_label_with_shortcut("chapters", "player_chapters"), self.show_chapters),
                (self.menu_label_with_shortcut("transcript", "player_transcript"), self.show_transcript),
                (self.menu_label_with_shortcut("lyrics", "player_lyrics"), self.show_lyrics),
                (self.menu_label_with_shortcut("close_player", "player_back"), self.close_current_player),
            ]
            if is_youtube:
                player_actions.insert(3, (self.menu_label_with_shortcut("play_related_video", "player_next_related"), self.play_related_item))
                player_actions.insert(5, (self.menu_label_with_shortcut("copy_timestamp_link", "player_copy_timestamp_link"), self.copy_current_player_timestamp_url))
                player_actions.insert(-1, (self.menu_label_with_shortcut("comments", "player_comments"), self.show_comments))
            actions.extend(player_actions)
        return actions

    def show_action_finder(self) -> None:
        actions = self.action_finder_actions()
        state: dict[str, object] = {"filtered": actions, "handler": None}
        dialog = wx.Dialog(self, title=self.t("action_finder"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("action_finder"))
        dialog.SetMinSize((520, 420))
        outer = wx.BoxSizer(wx.VERTICAL)
        outer.Add(wx.StaticText(dialog, label=self.t("action_finder_search")), 0, wx.LEFT | wx.RIGHT | wx.TOP, 8)
        query = wx.TextCtrl(dialog, style=wx.TE_PROCESS_ENTER)
        query.SetName(self.t("action_finder_search"))
        outer.Add(query, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)
        action_list = wx.ListBox(dialog, choices=[])
        action_list.SetName(self.t("action_finder_results"))
        outer.Add(action_list, 1, wx.LEFT | wx.RIGHT | wx.BOTTOM | wx.EXPAND, 8)

        def filtered_actions() -> list[tuple[str, callable]]:
            needle = query.GetValue().strip().lower()
            if not needle:
                return list(actions)
            words = [part for part in needle.split() if part]
            return [(label, handler) for label, handler in actions if all(word in label.lower() for word in words)]

        def refresh_actions(_event=None) -> None:
            filtered = filtered_actions()
            state["filtered"] = filtered
            labels = [label.replace("\t", ", ") for label, _handler in filtered] or [self.t("action_finder_no_results")]
            action_list.Set(labels)
            action_list.SetSelection(0)

        def activate(_event=None) -> None:
            filtered = state.get("filtered")
            if not isinstance(filtered, list) or not filtered:
                return
            index = action_list.GetSelection()
            if index == wx.NOT_FOUND:
                index = 0
            if 0 <= index < len(filtered):
                _label, handler = filtered[index]
                state["handler"] = handler
                dialog.EndModal(wx.ID_OK)

        def on_query_key(event: wx.KeyEvent) -> None:
            if event.GetKeyCode() in {wx.WXK_DOWN, wx.WXK_UP}:
                action_list.SetFocus()
                return
            if event.GetKeyCode() == wx.WXK_ESCAPE:
                dialog.EndModal(wx.ID_CANCEL)
                return
            event.Skip()

        def on_list_key(event: wx.KeyEvent) -> None:
            if event.GetKeyCode() in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}:
                activate()
                return
            if event.GetKeyCode() == wx.WXK_ESCAPE:
                dialog.EndModal(wx.ID_CANCEL)
                return
            event.Skip()

        query.Bind(wx.EVT_TEXT, refresh_actions)
        query.Bind(wx.EVT_TEXT_ENTER, activate)
        query.Bind(wx.EVT_KEY_DOWN, on_query_key)
        action_list.Bind(wx.EVT_LISTBOX_DCLICK, activate)
        action_list.Bind(wx.EVT_KEY_DOWN, on_list_key)
        button_sizer = wx.StdDialogButtonSizer()
        open_button = wx.Button(dialog, wx.ID_OK, self.t("open"))
        cancel_button = wx.Button(dialog, wx.ID_CANCEL)
        open_button.Bind(wx.EVT_BUTTON, activate)
        button_sizer.AddButton(open_button)
        button_sizer.AddButton(cancel_button)
        button_sizer.Realize()
        outer.Add(button_sizer, 0, wx.ALIGN_RIGHT | wx.ALL, 8)
        dialog.SetSizer(outer)
        refresh_actions()
        query.SetFocus()
        result = dialog.ShowModal()
        handler = state.get("handler") if result == wx.ID_OK else None
        dialog.Destroy()
        if callable(handler):
            wx.CallAfter(handler)

    def open_user_playlists_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [(self.menu_label_with_shortcut("create_playlist", "create_playlist"), self.create_user_playlist_dialog)]
        if self.user_playlists:
            actions = [
                (self.t("open_playlist"), self.open_selected_user_playlist),
                *actions,
                (self.t("download_user_playlist"), self.download_selected_user_playlist),
                (self.t("remove_playlist"), self.remove_selected_user_playlist),
            ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            menu.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def open_notification_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        actions = [
            (self.t("play"), self.open_selected_notification),
            (self.t("copy_url"), lambda: self.copy_item_url(self.selected_notification_item())),
            (self.t("clear_notifications"), self.clear_notifications),
        ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            menu.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def open_queue_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        item = self.selected_queue_item()
        if item and item.get("queue_state") == "active":
            actions = [
                (self.t("cancel_download"), self.cancel_selected_download),
                (self.t("cancel_all_downloads"), self.cancel_all_downloads),
            ]
        else:
            actions = [
                (self.t("download_selected_queued"), lambda: self.download_selected_queue_item()),
                (self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.download_selected_queue_item(True)),
                (self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.download_selected_queue_item(False)),
                (self.t("download_all_as_audio"), lambda: self.download_all_queued(True)),
                (self.t("download_all_as_video"), lambda: self.download_all_queued(False)),
                (self.t("remove_from_queue"), self.remove_selected_queue_item),
            ]
        for label, handler in actions:
            item = menu.Append(wx.ID_ANY, label)
            menu.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), item)
        self.PopupMenu(menu)
        menu.Destroy()

    def leave_player_to_main_menu(self, force_keep_playing: bool = False) -> None:
        keep_playing = bool(force_keep_playing and self.background_playback_enabled())
        if keep_playing and force_keep_playing:
            self.manual_background_playback_active = True
        self.exit_fullscreen_window()
        self.in_player_screen = False
        self.player_control_mode = keep_playing and self.player_control_mode
        if not keep_playing:
            self.stop_player(silent=True)
        self.show_main_menu()

    def refresh_main_menu_after_playback_queue_change(self) -> None:
        if not self.in_main_menu or not hasattr(self, "menu_list"):
            return
        wx.CallAfter(self.refresh_main_menu_download_label)

    def open_player_context_menu(self, _event=None) -> None:
        item = self.current_video_item or self.current_video_info or {}
        menu = wx.Menu()
        actions = []
        is_local_media = self.item_is_local_media(item)

        if not is_local_media:
            actions.append((self.menu_label_with_shortcut("download_audio", "download_audio"), lambda: self.start_download(True, item=dict(item))))
            actions.append((self.menu_label_with_shortcut("download_video", "download_video"), lambda: self.start_download(False, item=dict(item))))

        actions.append((self.menu_label_with_shortcut("add_favorite", "add_favorite"), lambda: self.add_favorite_item(dict(item))))
        actions.append((self.menu_label_with_shortcut("remove_favorite", "remove_favorite"), lambda: self.remove_favorite_item(dict(item))))

        if not is_local_media:
            actions.append((self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), lambda: self.subscribe_to_selected_channel(dict(item))))
            actions.append((self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), lambda: self.unsubscribe_from_selected_channel(dict(item))))
            if self.item_has_openable_youtube_channel(item):
                actions.append((self.menu_label_with_shortcut("open_channel", "open_channel"), lambda: self.open_item_channel(dict(item))))

        actions.append((self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue))
        actions.append((self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue))
        actions.append((self.menu_label_with_shortcut("remove_from_playlist", "remove_from_playlist"), self.remove_active_from_playlist))
        actions.append((self.menu_label_with_shortcut("copy_path" if is_local_media else "copy_link", "player_copy_link"), self.copy_current_player_url))

        if not is_local_media:
            actions.append((self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda: self.copy_direct_stream_url(dict(item))))
            if self.youtube_url_at_timestamp(item, 0):
                actions.append((self.menu_label_with_shortcut("copy_timestamp_link", "player_copy_timestamp_link"), self.copy_current_player_timestamp_url))

        actions.append((self.t("output_devices"), self.show_output_devices))
        actions.append((self.menu_label_with_shortcut("fullscreen", "player_fullscreen"), lambda: self.toggle_player_fullscreen(announce=True)))
        actions.append((self.t("equalizer"), self.show_player_equalizer))
        actions.append((self.menu_label_with_shortcut("audio_normalization", "player_replaygain"), self.cycle_replaygain_mode))
        if self.is_youtube_url(str(item.get("url") or item.get("webpage_url") or "")):
            actions.append((self.menu_label_with_shortcut("play_related_video", "player_next_related"), self.play_related_item))
        actions.append((self.menu_label_with_shortcut("add_bookmark", "player_add_bookmark"), self.add_current_bookmark))
        actions.append((self.menu_label_with_shortcut("bookmarks", "player_bookmarks"), self.show_player_bookmarks))
        actions.append((self.menu_label_with_shortcut("chapters", "player_chapters"), self.show_chapters))
        actions.append((self.menu_label_with_shortcut("transcript", "player_transcript"), self.show_transcript))
        actions.append((self.menu_label_with_shortcut("lyrics", "player_lyrics"), self.show_lyrics))

        if not is_local_media:
            actions.append((self.menu_label_with_shortcut("comments", "player_comments"), self.show_comments))
            actions.append((self.t("open_browser"), lambda: import_module("webbrowser").open(str(item.get("webpage_url") or item.get("url") or ""))))

        actions.append((self.t("close_player"), self.close_current_player))

        for label, handler in actions:
            menu_item = menu.Append(wx.ID_ANY, label)
            menu.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), menu_item)
        if self.playlist_item_is_supported(item):
            self.append_add_to_playlist_menu(menu, prefer_active=True)
        self.PopupMenu(menu)
        menu.Destroy()

    def open_context_menu(self, _event=None) -> None:
        menu = wx.Menu()
        item = self.selected_result(stable=False)
        if item and item.get("kind") in {"playlist", "channel"}:
            is_channel = item.get("kind") == "channel"
            if is_channel:
                actions = [
                    (self.t("channel_options"), lambda selected=dict(item): self.show_channel_options(selected)),
                    (self.t("channel_videos"), lambda selected=dict(item): self.open_channel_tab(selected, "videos")),
                    (self.t("channel_popular"), lambda selected=dict(item): self.open_channel_tab(selected, "popular")),
                    (self.t("channel_playlists"), lambda selected=dict(item): self.open_channel_tab(selected, "playlists")),
                    (self.t("channel_live_streams"), lambda selected=dict(item): self.open_channel_tab(selected, "streams")),
                    (None, None),
                    (self.menu_label_with_shortcut("add_favorite", "add_favorite"), self.add_selected_favorite),
                    (self.menu_label_with_shortcut("remove_favorite", "remove_favorite"), self.remove_selected_favorite_shortcut),
                    (self.t("open_browser"), self.open_selected_in_browser),
                    (self.menu_label_with_shortcut("copy_url", "copy_link"), self.copy_selected_url),
                ]
            else:
                actions = [
                    (self.t("play_playlist"), lambda selected=dict(item): self.play_playlist_from_result(selected, shuffle=False)),
                    (self.t("shuffle_playlist"), lambda selected=dict(item): self.play_playlist_from_result(selected, shuffle=True)),
                    (self.t("open_playlist_videos"), self.play_selected),
                    (None, None),
                    (self.menu_label_with_shortcut("add_favorite", "add_favorite"), self.add_selected_favorite),
                    (self.menu_label_with_shortcut("remove_favorite", "remove_favorite"), self.remove_selected_favorite_shortcut),
                    (self.t("open_browser"), self.open_selected_in_browser),
                    (self.menu_label_with_shortcut("copy_url", "copy_link"), self.copy_selected_url),
                ]
            if is_channel:
                actions.insert(5, (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), self.subscribe_shortcut))
                actions.insert(6, (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), self.unsubscribe_shortcut))
        else:
            is_local = self.item_is_local_media(item)
            if is_local:
                actions = [
                    (self.t("play"), self.play_selected),
                    (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
                    (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
                    (self.menu_label_with_shortcut("remove_from_playlist", "remove_from_playlist"), self.remove_active_from_playlist),
                    (self.menu_label_with_shortcut("copy_path", "copy_link"), self.copy_selected_url),
                ]
            else:
                actions = [
                    (self.t("play"), self.play_selected),
                    (self.menu_label_with_shortcut("download_audio", "download_audio"), self.download_audio),
                    (self.menu_label_with_shortcut("download_video", "download_video"), self.download_video),
                    (self.menu_label_with_shortcut("add_favorite", "add_favorite"), self.add_selected_favorite),
                    (self.menu_label_with_shortcut("remove_favorite", "remove_favorite"), self.remove_selected_favorite_shortcut),
                    (self.menu_label_with_shortcut("subscribe_channel", "subscribe_channel"), self.subscribe_shortcut),
                    (self.menu_label_with_shortcut("unsubscribe_channel", "unsubscribe_channel"), self.unsubscribe_shortcut),
                    (self.menu_label_with_shortcut("add_to_playback_queue", "add_to_playback_queue"), self.add_active_to_playback_queue),
                    (self.menu_label_with_shortcut("remove_from_playback_queue", "remove_from_playback_queue"), self.remove_active_from_playback_queue),
                    (self.menu_label_with_shortcut("remove_from_playlist", "remove_from_playlist"), self.remove_active_from_playlist),
                    (self.t("open_browser"), self.open_selected_in_browser),
                    (self.menu_label_with_shortcut("copy_stream_url", "copy_stream_url"), lambda selected=dict(item or {}): self.copy_direct_stream_url(selected)),
                    (self.menu_label_with_shortcut("copy_url", "copy_link"), self.copy_selected_url),
                ]
                if self.item_has_openable_youtube_channel(item):
                    actions.insert(7, (self.menu_label_with_shortcut("open_channel", "open_channel"), lambda selected=dict(item or {}): self.open_item_channel(selected)))
        if len(self.download_queue) > 1:
            actions[1:1] = [
                (self.t("download_all_as_audio"), lambda: self.download_all_queued(True)),
                (self.t("download_all_as_video"), lambda: self.download_all_queued(False)),
            ]
        context_item = dict(item or {})
        for label, handler in actions:
            if label is None:
                self.append_collection_download_submenu(menu, context_item)
                continue
            menu_item = menu.Append(wx.ID_ANY, label)
            menu.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), menu_item)
        selected = self.selected_result(stable=False)
        if selected and selected.get("kind") not in {"playlist", "channel"}:
            self.append_add_to_playlist_menu(menu)
        self.PopupMenu(menu)
        menu.Destroy()

