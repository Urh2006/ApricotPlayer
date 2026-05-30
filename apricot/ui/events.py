from apricot.constants import *
import wx
import os
from pathlib import Path
from apricot.ui.misc import MiscUI

class EventsUI:
    def activate_focused_button_from_key(self, event: wx.KeyEvent, focus: wx.Window | None) -> bool:
        if event.ControlDown() or event.AltDown():
            return False
        if event.GetKeyCode() not in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER, wx.WXK_SPACE}:
            return False
        if not isinstance(focus, wx.Button):
            return False
        handler = getattr(focus, "_apricot_button_handler", None)
        if callable(handler):
            handler()
            return True
        try:
            command = wx.CommandEvent(wx.EVT_BUTTON.typeId, focus.GetId())
            command.SetEventObject(focus)
            focus.Command(command)
            return True
        except Exception:
            event.Skip()
            return True

    def add_button_row(self, buttons: list[tuple[str, callable]]) -> list[wx.Button]:
        row = wx.BoxSizer(wx.HORIZONTAL)
        created_buttons = []
        for label, handler in buttons:
            is_play_pause = getattr(handler, "__name__", "") == "player_play_pause"
            button_label = self.current_play_pause_label() if is_play_pause else label
            button = wx.Button(self.panel, label=button_label)
            button._apricot_button_handler = handler
            if is_play_pause:
                button.SetName(button_label)
                button.SetToolTip(button_label)
                self.player_play_pause_buttons.append(button)
            button.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            row.Add(button, 0, wx.RIGHT, 6)
            created_buttons.append(button)
        self.root_sizer.Add(row, 0, wx.ALL, 4)
        self.last_button_row_controls = list(created_buttons)
        if self.background_player_section_enabled() and not self.in_player_screen:
            self.add_background_player_section()
        return created_buttons

    def on_close(self, event: wx.CloseEvent) -> None:
        if self.exiting or not self.settings.close_to_tray:
            self.shutdown_runtime()
            event.Skip()
            return
        event.Veto()
        self.setup_taskbar_icon()
        self.Hide()
        self.announce_player(self.t("tray_still_running"))
        self.show_desktop_notification(APP_NAME, self.t("tray_still_running"), enabled=self.settings.tray_notification)

    def check_activation_signal(self) -> None:
        now = time.monotonic()
        if now - self.last_activation_check < 0.05:
            return
        self.last_activation_check = now
        try:
            if not ACTIVATE_SIGNAL_FILE.exists():
                return
            payload = json.loads(ACTIVATE_SIGNAL_FILE.read_text(encoding="utf-8"))
            ACTIVATE_SIGNAL_FILE.unlink(missing_ok=True)
        except Exception:
            return
        action = str(payload.get("action") or "show")
        if action == "open_file":
            path = str(payload.get("path") or "")
            if path:
                wx.CallAfter(self.open_local_media_file, path, True)
        elif action == "settings":
            self.show_settings_from_tray()
        else:
            self.restore_from_tray()

    def folder_conversion_worker(self, source_folder: Path, output_folder: Path, target_format: str, image_path: Path | None = None, replace_originals: bool = False) -> None:
        try:
            ffmpeg = self.ffmpeg_executable()
            if not ffmpeg:
                raise RuntimeError("FFmpeg was not found")
            files = self.converter_media_files_in_folder(source_folder)
            if not files:
                wx.CallAfter(self.message, self.t("conversion_no_media_files"), wx.ICON_INFORMATION)
                return
            if not replace_originals:
                output_folder.mkdir(parents=True, exist_ok=True)
            wx.CallAfter(self.show_conversion_progress_dialog, len(files))
            converted = 0
            failed = 0
            for index, source in enumerate(files, start=1):
                if replace_originals:
                    target = source.with_suffix(f".{self.converter_output_extension(target_format)}")
                    work_target = self.temporary_conversion_path(target)
                else:
                    try:
                        relative = source.relative_to(source_folder)
                    except ValueError:
                        relative = Path(source.name)
                    target = self.unique_converter_output_path(output_folder / relative.with_suffix(f".{self.converter_output_extension(target_format)}"), source)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    work_target = target
                self.ui_queue.put(("status", f"{self.t('conversion_started')} {index}/{len(files)}: {source.name}"))
                self.ui_queue.put(("conversion_progress", {"file": source.name, "converted": converted, "total": len(files)}))
                try:
                    args = self.converter_ffmpeg_args(ffmpeg, source, work_target, target_format, image_path)
                    self.run_ffmpeg_conversion(args)
                    if replace_originals:
                        self.replace_converted_original(source, work_target, target)
                    converted += 1
                except Exception:
                    failed += 1
                    continue
                self.ui_queue.put(("conversion_progress", {"file": source.name, "converted": converted, "total": len(files)}))
            text = self.t("conversion_folder_done_with_errors", count=converted, failed=failed) if failed else self.t("conversion_folder_done", count=converted)
            wx.CallAfter(self.set_status, text)
            wx.CallAfter(self.close_conversion_progress_dialog)
            wx.CallAfter(self.finish_conversion_message, text)
        except Exception as exc:
            wx.CallAfter(self.close_conversion_progress_dialog)
            wx.CallAfter(self.message, self.t("conversion_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def finish_conversion_message(self, text: str) -> None:
        if getattr(self.settings, "popup_when_conversion_complete", True):
            self.message(text, wx.ICON_INFORMATION)
        else:
            self.announce_player(text)

    def show_conversion_progress_dialog(self, total: int) -> None:
        self.close_conversion_progress_dialog()
        maximum = max(1, int(total or 1))
        self.conversion_progress_dialog = wx.ProgressDialog(
            self.t("conversion_progress_title"),
            self.t("conversion_progress_message", file="", converted=0, total=maximum, remaining=maximum),
            maximum=maximum,
            parent=self,
            style=wx.PD_ELAPSED_TIME | wx.PD_ESTIMATED_TIME | wx.PD_REMAINING_TIME,
        )

    def close_conversion_progress_dialog(self) -> None:
        dialog = self.conversion_progress_dialog
        self.conversion_progress_dialog = None
        if dialog:
            try:
                dialog.Destroy()
            except RuntimeError:
                pass

    def show_notification_center(self) -> None:
        self.last_activated_menu_action = self.show_notification_center
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
        self.user_playlist_items_screen_active = False
        self.notification_center_screen_active = True
        self.direct_link_screen_active = False
        self.clear()
        self.add_background_player_section()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("play"), self.open_selected_notification),
                (self.t("clear_notifications"), self.clear_notifications),
            ]
        )
        label = wx.StaticText(self.panel, label=self.t("notification_center"))
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.notification_list = wx.ListBox(self.panel, choices=[])
        self.notification_list.SetName(self.t("notification_center"))
        self.notification_list.Bind(wx.EVT_LISTBOX_DCLICK, lambda _evt: self.open_selected_notification())
        self.notification_list.Bind(wx.EVT_CONTEXT_MENU, self.open_notification_context_menu)
        self.notification_list.Bind(wx.EVT_KEY_DOWN, self.on_notification_key)
        self.root_sizer.Add(self.notification_list, 1, wx.EXPAND | wx.ALL, 4)
        self.refresh_notification_center()
        self.panel.Layout()
        self.focus_later(self.notification_list)

    def refresh_notification_center(self) -> None:
        if not hasattr(self, "notification_list"):
            return
        try:
            selection = self.notification_list.GetSelection()
            if self.notifications:
                self.set_listbox_items(self.notification_list, [self.notification_line(notification) for notification in self.notifications], max(0, selection))
            else:
                self.set_listbox_items(self.notification_list, [self.t("notification_center_empty")], 0)
                self.set_status(self.t("notification_center_empty"))
        except RuntimeError:
            pass

    def notification_line(self, notification: dict) -> str:
        when = self.format_history_time(notification.get("timestamp"))
        item = notification.get("item") or {}
        parts = [
            notification.get("title", ""),
            notification.get("message", ""),
            item.get("title", ""),
            f"{self.t('channel')}: {item.get('channel', '')}" if item.get("channel") else "",
            when,
        ]
        return " | ".join(part for part in parts if part)

    def on_notification_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "open_selected"):
            self.open_selected_notification()
        elif self.shortcut_matches(event, "remove_selected"):
            self.clear_selected_notification()
        elif self.context_menu_shortcut_matches(event):
            self.open_notification_context_menu()
        else:
            event.Skip()

    def on_queue_key(self, event: wx.KeyEvent) -> None:
        if self.shortcut_matches(event, "download_audio"):
            self.download_selected_queue_item(True)
        elif self.shortcut_matches(event, "download_video"):
            self.download_selected_queue_item(False)
        elif self.shortcut_matches(event, "open_selected"):
            self.download_selected_queue_item()
        elif self.context_menu_shortcut_matches(event):
            self.open_queue_context_menu()
        else:
            event.Skip()

    def on_advanced_network_toggle(self, _event: wx.CommandEvent) -> None:
        self.apply_settings_from_visible_controls()
        self.render_settings_section_and_focus("show_advanced_network_settings")

    def on_trending_filter_key(self, event: wx.KeyEvent) -> None:
        if event.GetKeyCode() == wx.WXK_RETURN:
            self.load_trending_results()
            return
        event.Skip()

    def load_collection_worker(
        self,
        url: str,
        result_type: str,
        limit: int | None = None,
        selection: int = 0,
        generation: int | None = None,
        sort_mode: str = "",
    ) -> None:
        try:
            generation = self.search_generation if generation is None else generation
            limit = limit or self.initial_results_limit()
            if sort_mode == "popular" and result_type == "Video":
                normalized, fully_loaded = self.fetch_popular_channel_results(url, limit, generation)
                if self.settings.results_limit != 0:
                    normalized = normalized[:limit]
                wx.CallAfter(self.mark_collection_fully_loaded_if_current, generation, fully_loaded)
                if self.settings.results_limit == 0 and selection:
                    wx.CallAfter(self.show_more_results_if_current, generation, normalized, selection)
                else:
                    wx.CallAfter(self.show_results_if_current, generation, normalized)
                    wx.CallAfter(self.clear_loading_more_if_current, generation)
                return
            options = {
                "quiet": True,
                "extract_flat": True,
                "skip_download": True,
                "playlistend": limit,
            }
            info = self.ydl_extract_info(url, options, download=False)
            entries = list(info.get("entries") or [])[:limit]
            normalized = [self.normalize_entry(entry, result_type) for entry in entries]
            if sort_mode == "popular":
                normalized = self.sorted_popular_channel_results(normalized)
            if self.settings.results_limit == 0 and selection:
                wx.CallAfter(self.show_more_results_if_current, generation, normalized, selection)
            else:
                wx.CallAfter(self.show_results_if_current, generation, normalized)
                wx.CallAfter(self.clear_loading_more_if_current, generation)
        except Exception as exc:
            wx.CallAfter(self.dynamic_fetch_failed_if_current, generation or self.search_generation, self.friendly_error(exc))

    def mark_collection_fully_loaded_if_current(self, generation: int, fully_loaded: bool) -> None:
        if generation == self.search_generation:
            self.collection_fully_loaded = bool(fully_loaded)

    def on_repeat_changed(self, _event=None) -> None:
        checked = bool(getattr(self, "repeat_checkbox", None) and self.repeat_checkbox.GetValue())
        self.set_repeat_enabled(checked)

    def on_fullscreen_checkbox_key(self, event: wx.KeyEvent) -> None:
        key = event.GetKeyCode()
        if key in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}:
            self.request_player_fullscreen_checkbox_toggle()
            return
        event.Skip()

    def on_bass_boost_changed(self, _event=None) -> None:
        checked = bool(getattr(self, "bass_boost_checkbox", None) and self.bass_boost_checkbox.GetValue())
        self.set_bass_boost_enabled(checked)

    def on_session_autoplay_next_changed(self, _event=None) -> None:
        checkbox = getattr(self, "session_autoplay_checkbox", None)
        checked = bool(checkbox and checkbox.GetValue())
        self.set_session_autoplay_next(checked)

    def set_session_autoplay_next(self, checked: bool, announce: bool = True) -> None:
        if getattr(self.settings, "autoplay_next", False):
            self.session_autoplay_next = False
            return
        self.session_autoplay_next = bool(checked)
        checkbox = getattr(self, "session_autoplay_checkbox", None)
        if checkbox is not None:
            try:
                checkbox.SetValue(self.session_autoplay_next)
            except RuntimeError:
                pass
        if announce:
            self.announce_player(self.t("autoplay_next_on" if self.session_autoplay_next else "autoplay_next_off"))

    def refresh_play_pause_button_state(self) -> None:
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            self.update_play_pause_buttons()
            return
        try:
            self.player_paused = bool(self.mpv_get_property("pause", timeout=0.35))
        except Exception:
            pass
        self.update_play_pause_buttons()

    @classmethod
    def is_function_key_event(cls, event: wx.KeyEvent, number: int) -> bool:
        if not 1 <= number <= 24:
            return False
        target = wx.WXK_F1 + number - 1
        raw_target = 0x70 + number - 1
        return MiscUI.event_key_code(event) == target or MiscUI.event_raw_key_code(event) == raw_target

    @staticmethod
    def details_text_navigation_key(event: wx.KeyEvent) -> bool:
        key = event.GetKeyCode()
        navigation = {
            wx.WXK_UP,
            wx.WXK_DOWN,
            wx.WXK_LEFT,
            wx.WXK_RIGHT,
            wx.WXK_HOME,
            wx.WXK_END,
            wx.WXK_PAGEUP,
            wx.WXK_PAGEDOWN,
            wx.WXK_TAB,
        }
        if key in navigation:
            return True
        if event.ControlDown() and MiscUI.key_event_matches_letter(event, "c"):
            return True
        if event.ControlDown() and MiscUI.key_event_matches_letter(event, "a"):
            return True
        return False

    def on_char_hook(self, event: wx.KeyEvent) -> None:
        focus = wx.Window.FindFocus()
        details_has_focus = focus is self.video_details
        if self.is_modifier_only_event(event):
            return
        if self.is_shortcut_capture_control(focus):
            self.on_shortcut_capture_key(event, focus)
            return

        # SpinCtrl handles Up/Down/Home/End/PageUp/PageDown natively for value increment.
        # CheckBox and Slider are intentionally excluded so player shortcuts (seek, play/pause)
        # fire when those controls are focused — matching pre-refactoring behaviour.
        if isinstance(focus, wx.SpinCtrl):
            if event.GetKeyCode() in {
                wx.WXK_UP, wx.WXK_DOWN, wx.WXK_HOME, wx.WXK_END,
                wx.WXK_PAGEUP, wx.WXK_PAGEDOWN,
            }:
                event.Skip()
                return

        # Ensure editable text fields accept native typing and navigation (arrows, tab, backspace, etc.)
        key = event.GetKeyCode()
        if self.focus_accepts_text(focus):
            if key in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}:
                if focus is getattr(self, "query", None):
                    self.search()
                    return
                if focus is getattr(self, "direct_link_ctrl", None):
                    self.activate_direct_link_enter_action()
                    return
                event.Skip()
                return
            # OLD behaviour: Ctrl/Alt-modified shortcuts (e.g. Ctrl+L copy link, Ctrl+D
            # download) fire even when a text field is focused. Plain navigation keys
            # (arrows, backspace, typing) go through native handling as before.
            if not (event.ControlDown() or event.AltDown()):
                if key not in {wx.WXK_ESCAPE, wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}:
                    event.Skip()
                    return

        if self.handle_background_player_tab_navigation(event, focus):
            return
        if self.handle_player_tab_navigation(event, focus):
            return

        # Tab was not claimed by any custom navigation handler.  Without an
        # explicit Skip here, on_char_hook consumes the event and EVT_NAVIGATION_KEY
        # never fires — so Tab from background-player buttons and most other
        # non-text controls produced no focus movement at all.
        if key == wx.WXK_TAB:
            event.Skip()
            return

        results_focus = self.focus_in_results_control(focus)
        if self.in_main_menu:
            if self.handle_player_shortcut_event(event, focus, details_has_focus):
                return
            # Player shortcuts have priority; results-list native navigation comes after.
            # Matches pre-refactoring behaviour where player_shortcut_event was checked first.
            if results_focus and self.results_list_owns_key(event):
                event.Skip()
                wx.CallAfter(self.maybe_extend_results)
                return
            if self.shortcut_matches(event, "open_channel"):
                self.open_item_channel()
                return
            if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "menu_list", None):
                self.activate_menu()
                return
            if self.handle_global_navigation_shortcut(event, focus):
                return
            if self.handle_active_player_global_shortcut_event(event, focus):
                return
            event.Skip()
            return
        if self.handle_global_navigation_shortcut(event, focus):
            return
        if self.handle_active_player_global_shortcut_event(event, focus):
            return
        # Player shortcuts have priority; results-list native navigation comes after.
        # Matches pre-refactoring behaviour where player_shortcut_event was checked first.
        if results_focus and self.results_list_owns_key(event):
            event.Skip()
            wx.CallAfter(self.maybe_extend_results)
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "menu_list", None):
            self.activate_menu()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "rss_feed_list", None):
            self.open_selected_rss_feed()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "podcast_result_list", None):
            self.add_selected_podcast_result()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "rss_items_list", None):
            self.play_selected_rss_item()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "user_playlist_list", None):
            self.open_selected_user_playlist()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "user_playlist_items_list", None):
            self.play_selected_user_playlist_item()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "notification_list", None):
            self.open_selected_notification()
            return
        if self.shortcut_matches(event, "open_selected") and focus is getattr(self, "history_list", None):
            self.play_history_item()
            return
        if focus is getattr(self, "queue_list", None) and self.shortcut_matches(event, "download_audio"):
            self.download_selected_queue_item(True)
            return
        if focus is getattr(self, "queue_list", None) and self.shortcut_matches(event, "download_video"):
            self.download_selected_queue_item(False)
            return
        if focus is getattr(self, "queue_list", None) and self.shortcut_matches(event, "open_selected"):
            self.download_selected_queue_item()
            return
        if results_focus and self.shortcut_matches(event, "queue_audio"):
            self.toggle_download_queue()
            return
        if results_focus and self.result_details_key(event):
            self.announce_selected_result_details()
            return
        if self.shortcut_matches(event, "add_to_playback_queue"):
            self.add_active_to_playback_queue()
            return
        if self.shortcut_matches(event, "remove_from_playback_queue"):
            self.remove_active_from_playback_queue()
            return
        if self.shortcut_matches(event, "open_selected") and results_focus:
            self.play_selected()
            return
        if results_focus and self.shortcut_matches(event, "copy_link"):
            self.copy_selected_url()
            return
        if results_focus and self.shortcut_matches(event, "add_favorite"):
            self.add_selected_favorite()
            return
        if results_focus and self.shortcut_matches(event, "remove_favorite"):
            self.remove_selected_favorite_shortcut()
            return
        if self.shortcut_matches(event, "download_audio"):
            self.download_audio_shortcut()
            return
        if self.shortcut_matches(event, "download_video"):
            self.download_video_shortcut()
            return
        if self.shortcut_matches(event, "subscribe_channel"):
            self.subscribe_shortcut()
            return
        if self.shortcut_matches(event, "unsubscribe_channel"):
            self.unsubscribe_shortcut()
            return
        if self.shortcut_matches(event, "open_channel"):
            self.open_item_channel()
            return
        if self.shortcut_matches(event, "create_playlist"):
            self.create_user_playlist_dialog()
            return
        if self.shortcut_matches(event, "add_to_playlist"):
            prefer_player_item = bool(self.current_video_item and (self.in_player_screen or self.focus_in_background_player_controls(focus)))
            self.add_active_to_playlist(prefer_active=prefer_player_item)
            return
        if self.shortcut_matches(event, "remove_from_playlist"):
            self.remove_active_from_playlist()
            return
        if self.shortcut_matches(event, "player_back"):
            if self.in_player_screen and self.video_details_visible():
                self.hide_video_details()
                return
            if self.in_player_screen:
                if self.player_fullscreen_mode_active() or self.IsFullScreen():
                    self.exit_fullscreen_to_player()
                    return
                if self.player_escape_closes_playback(focus):
                    if self.IsFullScreen():
                        self.ShowFullScreen(False)
                    self.leave_player_to_previous_screen()
                else:
                    self.leave_player_to_main_menu(force_keep_playing=True)
                return
            if self.search_screen_active and self.search_results_stack:
                self.restore_previous_search_results()
                return
            if self.rss_items_screen_active:
                self.show_rss_feeds()
                return
            if self.user_playlist_items_screen_active:
                self.show_user_playlists()
                return
            if self.podcast_search_screen_active:
                self.show_rss_feeds()
                return
            if getattr(self, "podcast_categories_screen_active", False):
                self.show_rss_feeds()
                return
            if self.rss_feeds_screen_active:
                self.show_main_menu()
                return
            if self.user_playlists_screen_active or self.notification_center_screen_active or self.direct_link_screen_active:
                self.show_main_menu()
                return
            self.show_main_menu()
            return
        if self.shortcut_matches(event, "copy_stream_url"):
            self.copy_direct_stream_url()
            return
        if self.player_details_shortcut_matches(event) and (self.focus_in_player_controls(focus) or self.focus_in_background_player_controls(focus)):
            self.show_video_details()
            return
        if self.in_player_screen and self.handle_player_shortcut_event(event, focus, details_has_focus):
            return
        if self.in_player_screen and results_focus:
            event.Skip()
            wx.CallAfter(self.maybe_extend_results)
            return
        if self.handle_player_shortcut_event(event, focus, details_has_focus):
            return
        event.Skip()

