from apricot.constants import *
import wx
import os
from pathlib import Path
from apricot.ui.misc import MiscUI

class PlayerUI:
    def age_restricted_video_support_enabled(self) -> bool:
        return bool(getattr(self.settings, "enable_age_restricted_videos", False))

    def is_age_or_js_playback_error(self, exc: Exception | str) -> bool:
        lowered = str(exc).lower()
        checks = (
            "requested format is not available",
            "no video formats found",
            "nsig extraction failed",
            "signature extraction failed",
            "n challenge",
            "age restricted",
            "age-restricted",
            "this video may be inappropriate",
            "only available to registered users",
        )
        return any(check in lowered for check in checks)

    def background_playback_enabled(self) -> bool:
        return bool(getattr(self.settings, "enable_background_playback", False))

    def background_player_section_enabled(self) -> bool:
        return self.background_playback_enabled()

    def current_player_title(self) -> str:
        info = self.current_video_info or {}
        item = self.current_video_item or {}
        return str(info.get("title") or item.get("title") or self.t("player")).strip()

    def current_player_item(self) -> dict:
        item = self.current_video_item or self.current_video_info or {}
        return item if isinstance(item, dict) else {}

    def current_player_is_local_media(self) -> bool:
        return self.item_is_local_media(self.current_player_item())

    def add_background_player_section(self, defer: bool = True) -> None:
        if self.background_player_section_added:
            return
        if defer and not self.in_player_screen:
            if self.background_player_section_pending:
                return
            if not self.background_player_section_enabled() or not self.player_is_active():
                return
            self.background_player_section_pending = True
            generation = self.background_player_section_generation
            wx.CallAfter(self.flush_background_player_section, generation)
            return
        self.background_player_controls = []
        self.background_player_section_pending = False
        if not self.background_player_section_enabled() or not self.player_is_active():
            return
        self.background_player_section_added = True
        self.background_player_previous_control = self.background_player_previous_target()
        title = self.current_player_title()
        label = wx.StaticText(self.panel, label=self.t("background_player_now_playing", title=title))
        label.SetName(self.t("background_player"))
        self.root_sizer.Add(label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 4)

        if self.player_panel is not None:
            try:
                if not self.player_panel.IsBeingDeleted():
                    self.player_panel.Show()
                    self.player_panel.SetCanFocus(True)
                    self.player_panel.SetName(self.t("player"))
                    self.player_panel.SetLabel(self.t("player"))
                    self.bind_player_navigation_control(self.player_panel)
                    self.player_panel.SetMinSize((-1, 96))
                    self.root_sizer.Add(self.player_panel, 0, wx.EXPAND | wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
                    self.background_player_controls.append(self.player_panel)
            except RuntimeError:
                pass

        row = wx.BoxSizer(wx.HORIZONTAL)
        controls = [
            (self.t("previous"), lambda: self.play_relative_item(-1)),
            (self.current_play_pause_label(), self.player_play_pause),
            (self.t("next"), lambda: self.play_relative_item(1)),
            (self.t("playback_queue"), self.show_playback_queue),
            (self.t("add_to_playlist"), lambda: self.add_active_to_playlist(prefer_active=True)),
            (self.t("output_devices"), self.show_output_devices),
            (self.t("equalizer"), self.show_player_equalizer),
            (self.t("fullscreen"), lambda: self.toggle_player_fullscreen(announce=True)),
            (self.t("bass_boost"), self.toggle_bass_boost),
            (self.t("repeat"), self.toggle_repeat),
            (self.t("shuffle"), self.toggle_shuffle),
            (self.t("copy_link"), self.copy_current_player_url),
            (self.t("close_player"), self.close_current_player),
        ]
        for label_text, handler in controls:
            button = wx.Button(self.panel, label=label_text)
            button.SetName(f"{self.t('background_player')}: {label_text}")
            button._apricot_background_player_handler = handler
            button.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            button.Bind(wx.EVT_KEY_DOWN, self.on_background_player_key)
            self.bind_player_navigation_control(button)
            if getattr(handler, "__name__", "") == "player_play_pause":
                self.player_play_pause_buttons.append(button)
            row.Add(button, 0, wx.RIGHT, 6)
            self.background_player_controls.append(button)
        self.root_sizer.Add(row, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        previous_control = self.live_window(self.background_player_previous_control)
        for control in self.background_player_controls:
            if previous_control is not None:
                try:
                    control.MoveAfterInTabOrder(previous_control)
                except RuntimeError:
                    pass
            previous_control = control

    def flush_background_player_section(self, generation: int) -> None:
        if generation != getattr(self, "background_player_section_generation", -1):
            return
        self.background_player_section_pending = False
        if self.in_player_screen:
            return
        self.add_background_player_section(defer=False)
        try:
            self.panel.Layout()
        except Exception:
            pass

    def on_background_player_key(self, event: wx.KeyEvent) -> None:
        key = event.GetKeyCode()
        if key in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER, wx.WXK_SPACE} and not event.ControlDown() and not event.AltDown():
            control = event.GetEventObject()
            handler = getattr(control, "_apricot_background_player_handler", None)
            if callable(handler):
                handler()
                return
        self.on_char_hook(event)

    def bind_player_navigation_control(self, control: wx.Window | None) -> None:
        if control is None or getattr(control, "_apricot_navigation_bound", False):
            return
        try:
            control.Bind(wx.EVT_NAVIGATION_KEY, self.on_player_navigation_key)
            control._apricot_navigation_bound = True
        except Exception:
            pass

    def background_player_previous_target(self) -> wx.Window | None:
        for control in reversed(getattr(self, "last_button_row_controls", [])):
            target = self.live_window(control)
            if target is not None and not self.focus_in_background_player_controls(target):
                return target
        if getattr(self, "in_main_menu", False):
            return self.live_window(getattr(self, "menu_list", None))
        candidate_names = [
            "results_list",
            "queue_list",
            "rss_items_list",
            "rss_feed_list",
            "podcast_result_list",
            "user_playlist_items_list",
            "user_playlist_list",
            "notification_list",
            "history_list",
            "direct_link_ctrl",
        ]
        for name in candidate_names:
            target = self.live_window(getattr(self, name, None))
            if target is not None and not self.focus_in_background_player_controls(target):
                return target
        return None

    def handle_background_player_tab_navigation(self, event: wx.KeyEvent, focus: wx.Window | None) -> bool:
        if event.GetKeyCode() != wx.WXK_TAB:
            return False
        return self.move_background_player_tab_focus(not event.ShiftDown(), focus)

    def move_background_player_tab_focus(self, forward: bool, focus: wx.Window | None) -> bool:
        controls = [
            control
            for control in getattr(self, "background_player_controls", [])
            if control is not None and not getattr(control, "IsBeingDeleted", lambda: False)()
        ]
        if not controls or not self.window_is_or_descendant(focus, controls[0]):
            return False
        if forward and len(controls) > 1:
            self.safe_set_focus(controls[1])
            return True
        if not forward:
            target = self.live_window(self.background_player_previous_control) or self.background_player_previous_target()
            if target is None:
                return False
            self.safe_set_focus(target)
            return True
        return False

    def on_player_navigation_key(self, event: wx.NavigationKeyEvent) -> None:
        try:
            if event.IsWindowChange():
                event.Skip()
                return
            focus = event.GetCurrentFocus() or wx.Window.FindFocus()
            forward = bool(event.GetDirection())
        except Exception:
            event.Skip()
            return
        if self.move_background_player_tab_focus(forward, focus):
            return
        if self.move_player_tab_focus(forward, focus):
            return
        event.Skip()

    def show_current_player_screen(self) -> None:
        if not self.player_is_active():
            self.announce_player(self.t("no_player"))
            self.show_main_menu()
            return
        self.show_player_page(self.current_player_title())

    def focus_player_target_later(self, focus_target: str) -> bool:
        targets = {
            "player": "player_panel",
            "fullscreen_checkbox": "fullscreen_checkbox",
            "repeat_checkbox": "repeat_checkbox",
            "bass_boost_checkbox": "bass_boost_checkbox",
        }
        attr_name = targets.get(focus_target)
        if not attr_name:
            return False
        control = self.live_window(getattr(self, attr_name, None))
        if control is None:
            return False
        wx.CallAfter(self.safe_set_focus, control)
        wx.CallLater(100, self.safe_set_focus, control)
        wx.CallLater(300, self.safe_set_focus, control)
        return True

    def exit_fullscreen_to_player(self, focus_target: str = "player", announce: bool = False) -> None:
        if not self.player_is_active():
            self.back_to_results(stop_playback=False)
            return
        self.exit_fullscreen_window()
        self.show_player_page(self.current_player_title(), focus_target=focus_target)
        self.focus_player_target_later(focus_target)
        if announce:
            self.announce_player(self.t("fullscreen_off"))

    def enter_player_fullscreen(self, focus_target: str = "player", announce: bool = False) -> None:
        if not self.player_is_active():
            self.announce_player(self.t("no_player"))
            return
        self.player_fullscreen_session = True
        self.player_fullscreen_results_override = False
        try:
            self.show_player_page(self.current_player_title(), focus_target=focus_target)
            if self.player_kind == "mpv":
                self.mpv_request(["set_property", "fullscreen", True], timeout=0.5)
            self.ShowFullScreen(True)
            self.focus_player_target_later(focus_target)
            if announce:
                self.announce_player(self.t("fullscreen_on"))
        except Exception:
            try:
                self.ShowFullScreen(True)
            except Exception:
                pass
            if announce:
                self.announce_player(self.t("fullscreen_on"))

    def toggle_player_fullscreen(self, focus_target: str = "player", announce: bool = False) -> None:
        try:
            fullscreen_active = bool(self.player_fullscreen_mode_active() or self.IsFullScreen())
        except Exception:
            fullscreen_active = bool(self.player_fullscreen_mode_active())
        if fullscreen_active:
            self.exit_fullscreen_to_player(focus_target=focus_target, announce=announce)
        else:
            self.enter_player_fullscreen(focus_target=focus_target, announce=announce)

    def on_player_fullscreen_changed(self, _event=None) -> None:
        checked = bool(getattr(self, "fullscreen_checkbox", None) and self.fullscreen_checkbox.GetValue())
        if checked:
            self.enter_player_fullscreen(focus_target="fullscreen_checkbox", announce=True)
        else:
            self.exit_fullscreen_to_player(focus_target="fullscreen_checkbox", announce=True)

    def folder_has_audio_inputs(self, folder: Path) -> bool:
        try:
            return any(path.is_file() and path.suffix.lower() in AUDIO_INPUT_EXTENSIONS for path in folder.rglob("*"))
        except OSError:
            return False

    def leave_player_for_global_navigation(self) -> None:
        if not self.in_player_screen:
            return
        keep_playing = self.background_playback_enabled()
        if not keep_playing:
            self.stop_player(silent=True)
        self.in_player_screen = False
        self.player_control_mode = keep_playing and self.player_control_mode

    def request_player_next_dynamic_load(self, preserve_focus: bool = False) -> bool:
        if self.player_return_screen not in {"search", "trending"}:
            return False
        if not self.dynamic_fetch_enabled or self.settings.results_limit != 0 or not hasattr(self, "results_list"):
            return False
        if self.loading_more_results:
            self.pending_player_next_after_dynamic_load = True
            self.pending_player_next_preserve_focus = bool(preserve_focus)
            self.pending_player_next_current_url = str((self.current_video_item or {}).get("url") or "")
            return True
        current_count = len(self.all_results)
        if current_count <= 0:
            return False
        max_limit = self.max_results_limit()
        if max_limit and current_count >= max_limit:
            return False
        if getattr(self, "collection_fully_loaded", False) and len(self.results) >= len(self.all_results):
            return False
        self.pending_player_next_after_dynamic_load = True
        self.pending_player_next_preserve_focus = bool(preserve_focus)
        self.pending_player_next_current_url = str((self.current_video_item or {}).get("url") or "")
        selection = max(0, len(self.results) - 1)
        self.fetch_more_dynamic_results(selection)
        return True

    def finish_pending_player_next_after_dynamic_load(self) -> None:
        if not self.pending_player_next_after_dynamic_load:
            return
        preserve_focus = bool(self.pending_player_next_preserve_focus)
        current_url = str(self.pending_player_next_current_url or "")
        self.pending_player_next_after_dynamic_load = False
        self.pending_player_next_preserve_focus = False
        self.pending_player_next_current_url = ""
        if not self.player_is_active():
            return
        next_item = self.next_player_item_after_url(current_url)
        if next_item:
            wx.CallAfter(self.open_relative_player_item, next_item, True, preserve_focus)
            return
        self.announce_player(self.t("no_next_item"))

    def next_player_item_after_url(self, current_url: str) -> dict | None:
        if not current_url:
            return None
        playable = [item for item in self.player_navigation_results() if item.get("kind") not in {"channel", "playlist"}]
        current_pos = next((index for index, item in enumerate(playable) if str(item.get("url") or "") == current_url), -1)
        target = current_pos + 1
        if 0 <= current_pos and target < len(playable):
            return dict(playable[target])
        return None

    def hydrate_video_metadata_for_popular(self, item: dict) -> dict:
        updated = dict(item)
        url = str(updated.get("url") or "")
        if updated.get("kind") != "video" or not url or self.numeric_view_count(updated.get("view_count")) >= 0:
            return updated
        options = {"quiet": True, "skip_download": True, "noplaylist": True}
        try:
            info = self.ydl_extract_info(url, options, download=False, allow_cookie_retry=False)
            updated.update({key: value for key, value in self.metadata_from_info(info, updated).items() if value not in (None, "")})
        except Exception:
            pass
        return updated

    def open_channel_videos(self, item: dict, push_state: bool = True) -> None:
        self.open_channel_tab(item, "videos", push_state=push_state)

    def add_local_folder_to_playback_queue(self) -> None:
        source_items = self.selected_local_folder_items()
        items = [self.playback_queue_item_with_folder_return(item, source_items) for item in source_items]
        if not items:
            self.announce_player(self.t("folder_no_media"))
            return
        existing_urls = {str(item.get("url") or "") for item in self.playback_queue}
        added = 0
        for item in items:
            url = str(item.get("url") or "")
            if url and url not in existing_urls:
                self.playback_queue.append(item)
                existing_urls.add(url)
                added += 1
        self.save_playback_queue()
        self.announce_player(self.t("folder_queue_added", count=added))

    def merge_current_video_info_for_request(self, info: dict, generation: int) -> None:
        if self.playback_request_is_current(generation):
            self.merge_current_video_info(info)

    def resolve_and_start_player(self, command: str, url: str, title: str, announce_start: bool = False, request_generation: int = 0) -> None:
        try:
            stream_url, headers, info = self.resolve_stream_url(url)
            if not self.playback_request_is_current(request_generation):
                return
            wx.CallAfter(self.merge_current_video_info_for_request, info, request_generation)
            wx.CallAfter(self.start_mpv, command, stream_url, title or url, headers, announce_start, request_generation)
            wx.CallAfter(self.schedule_next_stream_prefetch_for_request, request_generation)
        except Exception as exc:
            if not self.playback_request_is_current(request_generation):
                return
            self.playback_start_pending = False
            if self.age_restricted_video_support_enabled() and self.is_cookie_auth_error(exc) and self.normalized_cookies_browser():
                wx.CallAfter(self.prompt_cookie_refresh_for_playback, command, url, title, self.friendly_error(exc), announce_start, request_generation)
            else:
                wx.CallAfter(self.message, self.t("player_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def merge_current_video_info(self, info: dict) -> None:
        if not info:
            return
        is_live = self.metadata_is_live_stream(info) or self.metadata_is_live_stream(self.current_video_info)
        live_status = self.metadata_live_status(info) or self.metadata_live_status(self.current_video_info)
        self.current_video_info.update(
            {
                "title": info.get("title") or self.current_video_info.get("title", ""),
                "id": info.get("id") or self.current_video_info.get("id", ""),
                "channel": info.get("uploader") or info.get("channel") or self.current_video_info.get("channel", ""),
                "channel_url": self.normalize_channel_url(info) or self.current_video_info.get("channel_url", ""),
                "channel_id": info.get("channel_id") or info.get("uploader_id") or self.current_video_info.get("channel_id", ""),
                "url": info.get("webpage_url") or self.current_video_info.get("url", ""),
                "view_count": info.get("view_count", self.current_video_info.get("view_count")),
                "views": self.format_count(info.get("view_count", self.current_video_info.get("view_count"))),
                "timestamp": info.get("timestamp", self.current_video_info.get("timestamp")),
                "upload_date": info.get("upload_date", self.current_video_info.get("upload_date")),
                "age": self.t("live_now") if is_live else (self.format_age(info) or self.current_video_info.get("age", "")),
                "duration_seconds": info.get("duration", self.current_video_info.get("duration_seconds")),
                "duration": self.format_duration(info.get("duration", self.current_video_info.get("duration_seconds"))),
                "description": info.get("description") or self.current_video_info.get("description", ""),
                "ext": info.get("ext") or self.current_video_info.get("ext", ""),
                "artist": info.get("artist") or info.get("creator") or self.current_video_info.get("artist", ""),
                "track": info.get("track") or self.current_video_info.get("track", ""),
                "album": info.get("album") or self.current_video_info.get("album", ""),
                "chapters": self.normalized_chapters(info.get("chapters")) or self.current_video_info.get("chapters", []),
                "live_status": live_status,
                "is_live": bool(is_live),
                "type": self.t("live_stream") if is_live else self.current_video_info.get("type", self.t("video")),
            }
        )
        self.with_live_stream_display_fields(self.current_video_info, info)
        if self.current_video_item is not None:
            self.current_video_item.update(self.current_video_info)
        if self.in_player_screen:
            self.set_window_title(str(self.current_video_info.get("title") or ""))
        self.update_details_text()

    def playback_resume_position(self) -> float:
        if self.metadata_is_live_stream(self.current_video_info) or self.metadata_is_live_stream(self.current_video_item):
            return 0.0
        key = self.playback_key()
        if not key or not getattr(self.settings, "resume_playback", True):
            return 0.0
        try:
            position = float(self.playback_positions.get(key, 0.0) or 0.0)
        except (TypeError, ValueError):
            return 0.0
        return position if position >= 5.0 else 0.0

    def audio_output_device_options(self, force_refresh: bool = False, allow_probe: bool = True) -> tuple[list[str], list[str]]:
        now = time.monotonic()
        if not force_refresh and self.audio_device_options_cache and now - self.audio_device_options_cache[0] < 20:
            return list(self.audio_device_options_cache[1]), list(self.audio_device_options_cache[2])
        values = ["auto"]
        labels = ["auto"]
        if not allow_probe:
            current = self.normalized_audio_output_device()
            if current and current.lower() != "auto":
                values.append(current)
                labels.append(current)
            return values, labels
        player = self.resolve_player()
        if player:
            command, kind = player
            if kind == "mpv":
                try:
                    creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
                    result = subprocess.run(
                        [command, "--audio-device=help", "--idle=yes", "--frames=0"],
                        stdout=subprocess.PIPE,
                        stderr=subprocess.STDOUT,
                        text=True,
                        encoding="utf-8",
                        errors="replace",
                        timeout=5,
                        creationflags=creationflags,
                    )
                    for line in result.stdout.splitlines():
                        match = re.match(r"\s*'([^']+)'\s+\((.*)\)\s*$", line)
                        if not match:
                            continue
                        value, label = match.group(1).strip(), match.group(2).strip()
                        if value and value not in values:
                            values.append(value)
                            labels.append(f"{label} ({value})" if label and label != value else value)
                except Exception:
                    pass
        current = self.normalized_audio_output_device()
        if current and current not in values:
            values.append(current)
            labels.append(f"{current} ({self.t('no_output_devices')})")
        self.audio_device_options_cache = (now, list(values), list(labels))
        return values, labels

    def refresh_audio_output_devices_async(self) -> None:
        if self.audio_device_refresh_running:
            return
        if self.audio_device_options_cache and time.monotonic() - self.audio_device_options_cache[0] < 60:
            return
        self.audio_device_refresh_running = True
        threading.Thread(target=self.refresh_audio_output_devices_worker, daemon=True).start()

    def refresh_audio_output_devices_worker(self) -> None:
        values: list[str] = ["auto"]
        labels: list[str] = ["auto"]
        try:
            values, labels = self.audio_output_device_options(force_refresh=True, allow_probe=True)
        finally:
            wx.CallAfter(self.finish_audio_output_device_refresh, values, labels)

    def finish_audio_output_device_refresh(self, values: list[str], labels: list[str]) -> None:
        self.audio_device_refresh_running = False
        if not hasattr(self, "settings_sections") or not hasattr(self, "controls"):
            return
        try:
            section_name = self.settings_sections()[self.settings_section_index][1]
        except Exception:
            return
        if section_name != "playback":
            return
        ctrl = self.controls.get("default_audio_device")
        if not isinstance(ctrl, wx.Choice) or (hasattr(ctrl, "IsBeingDeleted") and ctrl.IsBeingDeleted()):
            return
        current = self.selected_choice_value("default_audio_device") or self.normalized_audio_output_device()
        if current and current not in values:
            values = [*values, current]
            labels = [*labels, current]
        selected = values.index(current) if current in values else 0
        try:
            ctrl.Freeze()
            ctrl.Set(labels)
            ctrl.SetSelection(selected)
            self.choice_values["default_audio_device"] = list(values)
        finally:
            ctrl.Thaw()

    def check_saved_audio_device_available(self) -> None:
        device = self.normalized_audio_output_device()
        if not device or device.lower() == "auto":
            return
        values, labels = self.audio_output_device_options(force_refresh=True)
        if device in values and labels[values.index(device)] != f"{device} ({self.t('no_output_devices')})":
            return
        self.prompt_for_new_default_audio_device(values, labels)

    def player_audio_output_device(self) -> str:
        return (self.session_audio_output_device or self.normalized_audio_output_device() or "auto").strip() or "auto"

    def speed_audio_filter_args(self) -> list[str]:
        mode = self.normalized_speed_audio_mode()
        if mode == SPEED_AUDIO_MODE_MPV:
            return ["--audio-pitch-correction=yes"]
        if mode == SPEED_AUDIO_MODE_SCALETEMPO:
            return ["--audio-pitch-correction=no", "--af=@apricot_speed:scaletempo=stride=30:overlap=.50:search=10"]
        if mode == SPEED_AUDIO_MODE_RUBBERBAND:
            return [
                "--audio-pitch-correction=yes",
                "--af=@apricot_speed:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer",
            ]
        return ["--audio-pitch-correction=no", "--af=@apricot_speed:scaletempo2=search-interval=50:window-size=20:max-speed=8.0"]

    def show_player_page(self, title: str, focus_target: str = "player") -> None:
        fullscreen_mode = self.player_fullscreen_mode_active()
        background_enabled = self.background_playback_enabled()
        embedded_results = background_enabled and not fullscreen_mode
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
        self.notification_center_screen_active = False
        self.direct_link_screen_active = False
        self.folder_screen_active = False
        self.clear()
        self.in_player_screen = True
        self.player_control_mode = True
        self.player_navigation_controls = []
        self.player_action_controls = []
        self.player_escape_stop_controls = []
        navigation_controls = []
        if fullscreen_mode and background_enabled:
            navigation_controls.append((self.t("back_results"), self.exit_fullscreen_to_results))
        elif not embedded_results:
            navigation_controls.append((self.t("back_results"), self.leave_player_to_previous_screen))
            if not background_enabled:
                navigation_controls.append((self.t("back"), lambda: self.leave_player_to_main_menu(force_keep_playing=False)))
        else:
            navigation_controls.append((self.t("back"), lambda: self.leave_player_to_main_menu(force_keep_playing=True)))
        navigation_buttons = self.add_button_row(navigation_controls)
        self.player_navigation_controls = list(navigation_buttons)
        for control in navigation_buttons:
            self.bind_player_navigation_control(control)
        if not embedded_results and not (fullscreen_mode and self.background_playback_enabled()):
            self.player_escape_stop_controls.extend(navigation_buttons)
        if embedded_results:
            self.add_player_results_section()
        label = wx.StaticText(self.panel, label=f"{self.t('internal_player')}: {title}")
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        existing_panel = None
        if self.player_is_active() and self.player_panel is not None:
            try:
                if not self.player_panel.IsBeingDeleted():
                    existing_panel = self.player_panel
            except RuntimeError:
                existing_panel = None
        if existing_panel is not None:
            self.player_panel = existing_panel
            self.player_panel.Show()
        else:
            self.player_panel = PlayerPanel(self.panel, style=wx.BORDER_SIMPLE | wx.WANTS_CHARS)
            self.player_panel.SetBackgroundColour(wx.BLACK)
            self.player_panel.Bind(wx.EVT_KEY_DOWN, self.on_player_key)
            self.player_panel.Bind(wx.EVT_CONTEXT_MENU, self.open_player_context_menu)
            self.bind_player_navigation_control(self.player_panel)
        try:
            self.player_panel.SetCanFocus(True)
        except Exception:
            pass
        self.bind_player_navigation_control(self.player_panel)
        self.player_panel.SetName(self.t("player"))
        self.player_panel.SetLabel(self.t("player"))
        self.root_sizer.Add(self.player_panel, 1, wx.EXPAND | wx.ALL, 4)
        if focus_target == "player" and not self.settings.show_video_details_by_default:
            self.player_panel.SetFocus()
        is_local_media = self.current_player_is_local_media()
        player_controls = [
            (self.t("previous"), lambda: self.play_relative_item(-1, preserve_focus=True)),
            (self.current_play_pause_label(), self.player_play_pause),
            (self.t("next"), lambda: self.play_relative_item(1, preserve_focus=True)),
            (self.t("playback_queue"), self.show_playback_queue),
            (self.t("add_to_playlist"), lambda: self.add_active_to_playlist(prefer_active=True)),
            (self.t("output_devices"), self.show_output_devices),
            (self.t("equalizer"), self.show_player_equalizer),
            (self.t("chapters"), self.show_chapters),
            (self.t("lyrics"), self.show_lyrics),
            (self.t("comments"), self.show_comments),
            (self.t("edit_mode"), self.toggle_edit_mode),
            (self.t("copy_path" if is_local_media else "copy_link"), self.copy_current_player_url),
            (self.t("show_video_details"), self.show_video_details),
        ]
        if not is_local_media:
            player_controls.insert(-1, (self.t("copy_stream_url"), self.copy_direct_stream_url))
        if background_enabled:
            player_controls.append((self.t("close_player"), self.close_current_player))
        player_action_buttons = self.add_button_row(player_controls)
        self.player_action_controls = list(player_action_buttons)
        for control in player_action_buttons:
            self.bind_player_navigation_control(control)
        self.player_escape_stop_controls.extend(player_action_buttons)
        self.fullscreen_checkbox = wx.CheckBox(self.panel, label=self.t("fullscreen"))
        self.fullscreen_checkbox.SetName(self.t("fullscreen"))
        self.fullscreen_checkbox.SetValue(fullscreen_mode)
        self.fullscreen_checkbox.Bind(wx.EVT_CHECKBOX, self.on_player_fullscreen_changed)
        self.fullscreen_checkbox.Bind(wx.EVT_KEY_DOWN, self.on_fullscreen_checkbox_key)
        self.bind_player_navigation_control(self.fullscreen_checkbox)
        self.root_sizer.Add(self.fullscreen_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.player_action_controls.append(self.fullscreen_checkbox)
        self.player_escape_stop_controls.append(self.fullscreen_checkbox)
        self.repeat_checkbox = wx.CheckBox(self.panel, label=self.t("repeat"))
        self.repeat_checkbox.SetName(self.t("repeat"))
        self.repeat_checkbox.SetValue(self.repeat_current)
        self.repeat_checkbox.Bind(wx.EVT_CHECKBOX, self.on_repeat_changed)
        self.bind_player_navigation_control(self.repeat_checkbox)
        self.root_sizer.Add(self.repeat_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.player_action_controls.append(self.repeat_checkbox)
        self.player_escape_stop_controls.append(self.repeat_checkbox)
        self.session_autoplay_checkbox = None
        if not bool(getattr(self.settings, "autoplay_next", False)):
            self.session_autoplay_checkbox = wx.CheckBox(self.panel, label=self.t("autoplay_next_session"))
            self.session_autoplay_checkbox.SetName(self.t("autoplay_next_session"))
            self.session_autoplay_checkbox.SetValue(bool(self.session_autoplay_next))
            self.session_autoplay_checkbox.Bind(wx.EVT_CHECKBOX, self.on_session_autoplay_next_changed)
            self.bind_player_navigation_control(self.session_autoplay_checkbox)
            self.root_sizer.Add(self.session_autoplay_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
            self.player_action_controls.append(self.session_autoplay_checkbox)
            self.player_escape_stop_controls.append(self.session_autoplay_checkbox)
        self.bass_boost_checkbox = wx.CheckBox(self.panel, label=self.t("bass_boost"))
        self.bass_boost_checkbox.SetName(self.t("bass_boost"))
        self.bass_boost_checkbox.SetValue(self.bass_boost_enabled)
        self.bass_boost_checkbox.Bind(wx.EVT_CHECKBOX, self.on_bass_boost_changed)
        self.bind_player_navigation_control(self.bass_boost_checkbox)
        self.root_sizer.Add(self.bass_boost_checkbox, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.player_action_controls.append(self.bass_boost_checkbox)
        self.player_escape_stop_controls.append(self.bass_boost_checkbox)
        self.apply_tab_order(self.player_tab_order())
        self.details_label = None
        self.video_details = None
        self.details_button_sizer = None
        self.details_opened_temporarily = False
        self.set_window_title(title)
        self.panel.Layout()
        if focus_target == "results":
            wx.CallAfter(self.focus_results_list, self.return_index)
        elif focus_target != "player" and self.focus_player_target_later(focus_target):
            pass
        elif self.settings.show_video_details_by_default:
            wx.CallAfter(self.show_video_details, False)
        else:
            if not self.focus_player_target_later("player"):
                self.player_panel.SetFocus()
        if fullscreen_mode:
            wx.CallAfter(self.ShowFullScreen, True)

    def on_player_key(self, event: wx.KeyEvent) -> None:
        self.on_char_hook(event)

    def request_player_fullscreen_checkbox_toggle(self) -> None:
        now = time.monotonic()
        if now < getattr(self, "fullscreen_checkbox_toggle_block_until", 0.0):
            return
        self.fullscreen_checkbox_toggle_block_until = now + 0.18
        self.toggle_player_fullscreen_checkbox()

    def toggle_player_fullscreen_checkbox(self) -> None:
        checkbox = getattr(self, "fullscreen_checkbox", None)
        if checkbox is None:
            return
        try:
            checkbox.SetValue(not checkbox.GetValue())
        except RuntimeError:
            return
        self.on_player_fullscreen_changed()

    def player_escape_closes_playback(self, focus: wx.Window | None) -> bool:
        if self.focus_in_results_control(focus):
            return False
        if focus is getattr(self, "player_panel", None):
            return True
        if focus is getattr(self, "fullscreen_checkbox", None):
            return True
        if focus is getattr(self, "repeat_checkbox", None):
            return True
        if focus is getattr(self, "session_autoplay_checkbox", None):
            return True
        if focus is getattr(self, "bass_boost_checkbox", None):
            return True
        return focus in getattr(self, "player_escape_stop_controls", [])

    def visible_player_controls(self, controls: list[wx.Window] | tuple[wx.Window, ...]) -> list[wx.Window]:
        visible_controls: list[wx.Window] = []
        for control in controls:
            live = self.live_window(control)
            if live is not None:
                visible_controls.append(live)
        return visible_controls

    def player_tab_order(self) -> list[wx.Window]:
        ordered: list[wx.Window] = []
        ordered.extend(self.visible_player_controls(getattr(self, "player_navigation_controls", [])))
        results = self.live_window(getattr(self, "results_list", None))
        if results is not None and self.in_player_screen:
            ordered.append(results)
        panel = self.live_window(getattr(self, "player_panel", None))
        if panel is not None:
            ordered.append(panel)
        ordered.extend(self.visible_player_controls(getattr(self, "player_action_controls", [])))
        return ordered

    def handle_player_tab_navigation(self, event: wx.KeyEvent, focus: wx.Window | None) -> bool:
        if not self.in_player_screen or event.GetKeyCode() != wx.WXK_TAB:
            return False
        return self.move_player_tab_focus(not event.ShiftDown(), focus)

    def move_player_tab_focus(self, forward: bool, focus: wx.Window | None) -> bool:
        if not self.in_player_screen:
            return False
        panel = getattr(self, "player_panel", None)
        if not self.window_is_or_descendant(focus, panel):
            return False
        order = self.player_tab_order()
        try:
            index = order.index(panel)
        except ValueError:
            return False
        next_index = index + 1 if forward else index - 1
        if 0 <= next_index < len(order):
            self.safe_set_focus(order[next_index])
            return True
        return False

    def leave_player_to_previous_screen(self) -> None:
        self.back_to_results(stop_playback=True)

    def announce_player(self, text: str) -> None:
        self.set_status(text)
        self.speak_text(text)

    def show_video_details(self, temporary: bool | None = None) -> None:
        if not self.in_player_screen:
            if self.player_is_active():
                self.show_current_player_screen()
                wx.CallAfter(self.show_video_details, temporary)
            else:
                self.announce_player(self.t("no_player"))
            return
        self.details_opened_temporarily = (not self.settings.show_video_details_by_default) if temporary is None else bool(temporary)
        if self.video_details is None:
            self.details_label = wx.StaticText(self.panel, label=self.t("video_details"))
            self.video_details = wx.TextCtrl(
                self.panel,
                style=wx.TE_MULTILINE | wx.TE_READONLY | wx.TE_RICH2 | wx.TE_DONTWRAP | wx.VSCROLL | wx.HSCROLL | wx.WANTS_CHARS,
            )
            self.video_details.SetName(self.t("video_details"))
            self.video_details.SetMinSize((-1, 160))
            self.root_sizer.Add(self.details_label, 0, wx.LEFT | wx.RIGHT | wx.TOP, 4)
            self.root_sizer.Add(self.video_details, 0, wx.EXPAND | wx.ALL, 4)
            self.details_button_sizer = wx.BoxSizer(wx.HORIZONTAL)
            copy_button = wx.Button(self.panel, label=self.t("copy_details"))
            copy_button.SetName(self.t("copy_details"))
            copy_button.Bind(wx.EVT_BUTTON, lambda _evt: self.copy_video_details())
            back_button = wx.Button(self.panel, label=self.t("back"))
            back_button.SetName(self.t("back"))
            back_button.Bind(wx.EVT_BUTTON, lambda _evt: self.hide_video_details())
            self.details_button_sizer.Add(copy_button, 0, wx.RIGHT, 6)
            self.details_button_sizer.Add(back_button, 0)
            self.root_sizer.Add(self.details_button_sizer, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.update_details_text()
        if self.details_label:
            self.details_label.Show()
        if self.video_details:
            self.video_details.Show()
        if self.details_button_sizer:
            self.show_sizer_items(self.details_button_sizer, True)
        self.panel.Layout()
        self.video_details.SetInsertionPoint(0)
        self.safe_set_focus(self.video_details)
        self.announce_player(self.t("video_details"))

    def hide_video_details(self) -> None:
        if not self.video_details:
            return
        if self.details_label:
            self.details_label.Hide()
        self.video_details.Hide()
        if self.details_button_sizer:
            self.show_sizer_items(self.details_button_sizer, False)
        self.panel.Layout()
        if hasattr(self, "player_panel"):
            self.safe_set_focus(self.player_panel)
        self.announce_player(self.t("details_closed"))

    def copy_video_details(self) -> None:
        details = self.build_video_details_text()
        self.copy_plain_text_to_clipboard(details)
        self.announce_player(self.t("details_copied"))

    def ensure_player_for_auxiliary_view(self, callback) -> bool:
        if self.in_player_screen:
            return True
        if self.player_is_active():
            self.show_current_player_screen()
            wx.CallAfter(callback)
            return False
        self.announce_player(self.t("no_player"))
        return False

    def video_details_visible(self) -> bool:
        try:
            return bool(self.video_details and self.video_details.IsShown())
        except RuntimeError:
            return False

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
            f"{self.t('type')}: {self.item_type_label(info)}",
            f"Duration: {info.get('duration') or self.format_duration(info.get('duration_seconds'))}",
            f"Playback speed: {info.get('speed') or self.settings.player_speed}x",
            f"{self.t('pitch_label')}: {info.get('pitch') or '1.00'}x",
            f"{self.t('description')}:",
            info.get("description") or "",
        ]
        return "\n".join(line for line in lines if line is not None)

    def copy_current_player_url(self) -> None:
        item = self.current_player_item()
        if self.item_is_local_media(item):
            self.copy_path_to_clipboard(str(item.get("path") or item.get("url") or item.get("webpage_url") or ""))
            return
        self.copy_url_to_clipboard(str(item.get("webpage_url") or item.get("url") or ""))

    def current_player_position_seconds(self) -> int:
        if self.player_kind == "mpv" and self.mpv_process_alive():
            try:
                return max(0, int(float(self.mpv_get_property("time-pos", timeout=0.35) or 0.0)))
            except Exception:
                pass
        return 0

    def copy_current_player_timestamp_url(self) -> None:
        url = self.youtube_url_at_timestamp(self.current_player_item(), self.current_player_position_seconds())
        if not url:
            self.announce_player(self.t("timestamp_url_unavailable"))
            return
        self.copy_plain_text_to_clipboard(url)
        self.announce_player(self.t("timestamp_url_copied"))

    def clear_player_sequence(self) -> None:
        self.player_sequence_results = []

    def set_player_sequence(self, items: list[dict]) -> None:
        self.player_sequence_results = [dict(item) for item in items if item.get("url")]

    def player_sequence_contains_url(self, url: str) -> bool:
        if not url:
            return False
        return any(str(item.get("url") or "") == url for item in self.player_sequence_results)

    def player_sequence_contains_item(self, item: dict | None) -> bool:
        return self.player_sequence_contains_url(str((item or {}).get("url") or ""))

    def current_player_sequence_active(self) -> bool:
        return self.player_sequence_contains_url(str((self.current_video_item or {}).get("url") or ""))

    def player_navigation_results(self) -> list[dict]:
        current_url = str((self.current_video_item or {}).get("url") or "")
        if self.player_sequence_results and current_url and self.player_sequence_contains_url(current_url):
            return list(self.player_sequence_results)
        collections = [self.return_all_results, self.all_results, self.return_results, self.results]
        non_empty = [list(items) for items in collections if items]
        if not non_empty:
            return []
        if self.player_return_screen in {"search", "trending", "playback_queue"}:
            if current_url:
                for items in non_empty:
                    if any(str(item.get("url") or "") == current_url for item in items):
                        return items
        return non_empty[0]

    def sync_results_selection_to_player_item(self, item: dict | None) -> None:
        results_list = self.live_window(getattr(self, "results_list", None))
        if results_list is None or not item:
            return
        url = str(item.get("url") or "")
        if not url:
            return
        all_results = list(self.return_all_results or self.all_results or self.return_results or self.results)
        index = next((i for i, result in enumerate(all_results) if str(result.get("url") or "") == url), -1)
        if index < 0:
            return
        if index >= len(self.results) and index < len(all_results):
            previous_count = len(self.results)
            self.all_results = list(all_results)
            self.last_visible_count = min(len(self.all_results), index + 1)
            self.results = self.all_results[: self.last_visible_count]
            labels = [self.result_line(row, result) for row, result in enumerate(self.results)]
            if not self.append_listbox_items(results_list, labels, previous_count, index):
                self.set_listbox_items(results_list, labels, index)
        if index >= len(self.results):
            return
        self.current_index = index
        self.remember_user_result_selection(index)
        try:
            if results_list.GetSelection() != index:
                self.results_selection_update_suppressed = True
                results_list.SetSelection(index)
                wx.CallAfter(self.clear_results_selection_update_suppression)
        except RuntimeError:
            pass

    def relative_player_item(self, delta: int) -> dict | None:
        screen = self.player_return_screen
        data = dict(self.player_return_data or {})
        if screen == "rss_items":
            feed_index = int(data.get("feed_index", self.current_rss_feed_index) or 0)
            item_index = int(data.get("item_index", 0) or 0) + delta
            if 0 <= feed_index < len(self.rss_feeds):
                items = list(self.rss_feeds[feed_index].get("items") or [])
                if 0 <= item_index < len(items):
                    return dict(items[item_index], rss_feed_index=feed_index, rss_item_index=item_index)
        if screen == "user_playlist_items":
            playlist_index = int(data.get("playlist_index", self.current_user_playlist_index) or 0)
            item_index = int(data.get("item_index", 0) or 0) + delta
            if 0 <= playlist_index < len(self.user_playlists):
                items = list(self.user_playlists[playlist_index].get("items") or [])
                if 0 <= item_index < len(items):
                    return dict(items[item_index], user_playlist_index=playlist_index, user_playlist_item_index=item_index)
        results = self.player_navigation_results()
        item_index = int(data.get("index", self.return_index) or self.return_index) + delta
        playable = [item for item in results if item.get("kind") not in {"channel", "playlist"}]
        if not playable:
            return None
        current_url = str((self.current_video_item or {}).get("url") or "")
        current_pos = next((i for i, item in enumerate(playable) if item.get("url") == current_url), -1)
        if self.shuffle_current and delta > 0 and playable:
            choices = [item for item in playable if str(item.get("url") or "") != current_url] or playable
            return dict(random.choice(choices))
        if current_pos >= 0:
            item_index = current_pos + delta
        if 0 <= item_index < len(playable):
            return dict(playable[item_index])
        return None

    def open_relative_player_item(self, item: dict, announce_start: bool = False, preserve_focus: bool = False) -> None:
        if not item.get("url"):
            return
        if not self.player_sequence_contains_item(item):
            self.clear_player_sequence()
        data = dict(self.player_return_data or {})
        keep_current_ui = bool(preserve_focus and self.live_window(getattr(self, "player_panel", None)) is not None)
        show_player = (self.in_player_screen or not self.background_playback_enabled()) and not keep_current_ui
        focus_target = "player" if keep_current_ui else ("results" if preserve_focus and self.live_window(getattr(self, "results_list", None)) is not None else "player")
        if item.get("kind") == "rss_item":
            self.player_return_screen = "rss_items"
            self.player_return_data = {
                "feed_index": int(item.get("rss_feed_index", self.current_rss_feed_index) or 0),
                "item_index": int(item.get("rss_item_index", 0) or 0),
            }
        elif "user_playlist_index" in item:
            self.player_return_screen = "user_playlist_items"
            self.player_return_data = {
                "playlist_index": int(item.get("user_playlist_index", self.current_user_playlist_index) or 0),
                "item_index": int(item.get("user_playlist_item_index", 0) or 0),
            }
        elif self.player_return_screen == "folder" or item.get("kind") == "local_file":
            folder = str(data.get("folder") or self.current_local_folder_path or self.last_search_query)
            results = [dict(result) for result in (self.return_all_results or self.current_local_folder_items or self.all_results or self.return_results or self.results) if result.get("kind") == "local_file" and result.get("url")]
            if folder and not results:
                results = self.cached_local_folder_items(Path(folder))
            if results:
                self.current_local_folder_path = folder
                self.current_local_folder_items = [dict(result) for result in results]
                self.return_results = list(results)
                self.return_all_results = list(results)
                self.return_visible_count = len(results)
            self.return_index = next((i for i, result in enumerate(results) if result.get("url") == item.get("url")), self.return_index)
            self.player_return_screen = "folder"
            self.player_return_data = {"index": self.return_index, "folder": folder}
        else:
            results = self.return_all_results or self.all_results or self.return_results or self.results
            self.return_index = next((i for i, result in enumerate(results) if result.get("url") == item.get("url")), self.return_index)
            self.player_return_screen = "search"
            self.player_return_data = self.search_return_data(self.return_index)
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.sync_results_selection_to_player_item(item)
        self.play_url(
            str(item.get("url") or ""),
            str(item.get("title") or ""),
            show_player=show_player,
            announce_start=announce_start,
            focus_target=focus_target,
            keep_current_ui=keep_current_ui,
        )

    def add_active_to_playback_queue(self) -> None:
        item = self.playable_queue_item(self.active_item())
        if not item:
            self.announce_player(self.t("no_selection"))
            return
        url = str(item.get("url") or "")
        if any(str(existing.get("url") or "") == url for existing in self.playback_queue):
            self.announce_player(self.t("playback_queue_already_added", title=item.get("title", "")))
            return
        self.playback_queue.append(item)
        self.save_playback_queue()
        self.refresh_main_menu_after_playback_queue_change()
        self.announce_player(self.t("playback_queue_added", title=item.get("title", "")))

    def remove_active_from_playback_queue(self) -> None:
        item = self.playable_queue_item(self.active_item())
        if not item:
            self.announce_player(self.t("no_selection"))
            return
        if self.remove_playback_queue_url(str(item.get("url") or "")):
            self.announce_player(self.t("playback_queue_removed", title=item.get("title", "")))
        else:
            self.announce_player(self.t("playback_queue_not_found"))

    def remove_playback_queue_url(self, url: str) -> bool:
        before = len(self.playback_queue)
        self.playback_queue = [item for item in self.playback_queue if str(item.get("url") or "") != url]
        changed = len(self.playback_queue) != before
        if changed:
            self.save_playback_queue()
            self.refresh_main_menu_after_playback_queue_change()
        return changed

    def clear_auto_folder_playback_queue(self) -> None:
        before = len(self.playback_queue)
        self.playback_queue = [item for item in self.playback_queue if not item.get("_auto_folder_queue")]
        if len(self.playback_queue) == before:
            return
        self.save_playback_queue()
        self.refresh_main_menu_after_playback_queue_change()

    def set_auto_folder_playback_queue(self, queue_items: list[dict]) -> None:
        manual_items = [item for item in self.playback_queue if not item.get("_auto_folder_queue")]
        self.playback_queue = [dict(item) for item in queue_items] + manual_items
        self.save_playback_queue()
        self.refresh_main_menu_after_playback_queue_change()

    def playback_queue_line(self, item: dict, index: int) -> str:
        parts = [
            str(index + 1),
            item.get("title", ""),
            f"{self.t('channel')}: {item.get('channel', '')}" if item.get("channel") else "",
            self.item_type_label(item, default=""),
        ]
        return ". ".join([parts[0], " | ".join(part for part in parts[1:] if part)])

    def show_playback_queue(self) -> None:
        self.last_activated_menu_action = self.show_playback_queue
        if not self.playback_queue:
            self.announce_player(self.t("playback_queue_empty"))
            self.refresh_main_menu_after_playback_queue_change()
            return
        dialog = wx.Dialog(self, title=self.t("playback_queue"), style=wx.DEFAULT_DIALOG_STYLE | wx.RESIZE_BORDER)
        dialog.SetName(self.t("playback_queue"))
        dialog.SetMinSize((560, 420))
        outer = wx.BoxSizer(wx.VERTICAL)
        instructions = wx.StaticText(dialog, label=self.t("playback_queue_instructions"))
        outer.Add(instructions, 0, wx.ALL, 8)
        queue_list = wx.ListBox(dialog, choices=[self.playback_queue_line(item, index) for index, item in enumerate(self.playback_queue)])
        queue_list.SetName(self.t("playback_queue"))
        queue_list.SetSelection(0)
        outer.Add(queue_list, 1, wx.EXPAND | wx.LEFT | wx.RIGHT | wx.BOTTOM, 8)
        row = wx.BoxSizer(wx.HORIZONTAL)
        play_button = wx.Button(dialog, label=self.t("play"))
        move_up_button = wx.Button(dialog, label=self.t("move_up"))
        move_down_button = wx.Button(dialog, label=self.t("move_down"))
        remove_button = wx.Button(dialog, label=self.t("remove_from_playback_queue"))
        clear_button = wx.Button(dialog, label=self.t("clear_playback_queue"))
        close_button = wx.Button(dialog, wx.ID_CANCEL, label=self.t("back"))
        row.Add(play_button, 0, wx.RIGHT, 8)
        row.Add(move_up_button, 0, wx.RIGHT, 8)
        row.Add(move_down_button, 0, wx.RIGHT, 8)
        row.Add(remove_button, 0, wx.RIGHT, 8)
        row.Add(clear_button, 0, wx.RIGHT, 8)
        row.Add(close_button, 0)
        outer.Add(row, 0, wx.ALIGN_RIGHT | wx.ALL, 8)
        dialog.SetSizer(outer)
        action: dict[str, int | str] = {}

        def selected_index() -> int:
            try:
                index = queue_list.GetSelection()
            except RuntimeError:
                return -1
            return index if 0 <= index < len(self.playback_queue) else -1

        def play_selected(_event=None) -> None:
            index = selected_index()
            if index >= 0:
                action.update({"action": "play", "index": index})
                dialog.EndModal(wx.ID_OK)

        def remove_selected(_event=None) -> None:
            index = selected_index()
            if index < 0:
                return
            title = str(self.playback_queue[index].get("title") or "")
            del self.playback_queue[index]
            self.save_playback_queue()
            queue_list.Set([self.playback_queue_line(item, item_index) for item_index, item in enumerate(self.playback_queue)] or [self.t("playback_queue_empty")])
            if self.playback_queue:
                queue_list.SetSelection(min(index, len(self.playback_queue) - 1))
            self.announce_player(self.t("playback_queue_removed", title=title))
            if not self.playback_queue:
                dialog.EndModal(wx.ID_CANCEL)

        def refresh_queue_list(selection: int) -> None:
            labels = [self.playback_queue_line(item, item_index) for item_index, item in enumerate(self.playback_queue)] or [self.t("playback_queue_empty")]
            queue_list.Set(labels)
            if self.playback_queue:
                queue_list.SetSelection(min(max(0, selection), len(self.playback_queue) - 1))

        def clear_queue(_event=None) -> None:
            if not self.playback_queue:
                self.announce_player(self.t("playback_queue_empty"))
                return
            self.playback_queue = []
            self.save_playback_queue()
            refresh_queue_list(0)
            self.announce_player(self.t("playback_queue_cleared"))
            dialog.EndModal(wx.ID_CANCEL)

        def move_selected(delta: int) -> None:
            index = selected_index()
            target = index + delta
            if index < 0 or target < 0 or target >= len(self.playback_queue):
                return
            self.playback_queue[index], self.playback_queue[target] = self.playback_queue[target], self.playback_queue[index]
            self.save_playback_queue()
            refresh_queue_list(target)
            self.announce_player(self.t("playback_queue_reordered"))

        def open_queue_context_menu(event=None) -> None:
            menu = wx.Menu()
            actions = [
                (self.t("play"), play_selected),
                (self.t("move_up"), lambda _evt=None: move_selected(-1)),
                (self.t("move_down"), lambda _evt=None: move_selected(1)),
                (self.t("remove_from_playback_queue"), remove_selected),
                (self.t("clear_playback_queue"), clear_queue),
            ]
            for label, handler in actions:
                menu_item = menu.Append(wx.ID_ANY, label)
                dialog.Bind(wx.EVT_MENU, lambda _evt, fn=handler: fn(), menu_item)
            queue_list.PopupMenu(menu)
            menu.Destroy()

        queue_list.Bind(wx.EVT_LISTBOX_DCLICK, play_selected)
        def on_queue_key(evt: wx.KeyEvent) -> None:
            if self.shortcut_matches(evt, "open_selected"):
                play_selected()
                return
            if self.context_menu_shortcut_matches(evt):
                open_queue_context_menu(evt)
                return
            evt.Skip()

        queue_list.Bind(wx.EVT_KEY_DOWN, on_queue_key)
        queue_list.Bind(wx.EVT_CONTEXT_MENU, open_queue_context_menu)
        play_button.Bind(wx.EVT_BUTTON, play_selected)
        move_up_button.Bind(wx.EVT_BUTTON, lambda _evt: move_selected(-1))
        move_down_button.Bind(wx.EVT_BUTTON, lambda _evt: move_selected(1))
        remove_button.Bind(wx.EVT_BUTTON, remove_selected)
        clear_button.Bind(wx.EVT_BUTTON, clear_queue)
        result = dialog.ShowModal()
        dialog.Destroy()
        self.refresh_main_menu_after_playback_queue_change()
        if result == wx.ID_OK and action.get("action") == "play":
            self.play_playback_queue_index(int(action.get("index", -1)))

    def play_playback_queue_index(self, index: int) -> None:
        if index < 0 or index >= len(self.playback_queue):
            self.announce_player(self.t("playback_queue_empty"))
            return
        item = dict(self.playback_queue.pop(index))
        self.save_playback_queue()
        self.refresh_main_menu_after_playback_queue_change()
        self.open_playback_queue_item(item)

    def pop_next_playback_queue_item(self) -> dict | None:
        if not self.playback_queue:
            return None
        item = dict(self.playback_queue.pop(0))
        self.save_playback_queue()
        self.refresh_main_menu_after_playback_queue_change()
        return item

    def open_playback_queue_item(self, item: dict, announce_start: bool = False, preserve_focus: bool = False) -> None:
        keep_current_ui = bool(preserve_focus and self.live_window(getattr(self, "player_panel", None)) is not None)
        show_player = (self.in_player_screen or not self.background_playback_enabled()) and not keep_current_ui
        focus_target = "player" if keep_current_ui else ("results" if preserve_focus and self.live_window(getattr(self, "results_list", None)) is not None else "player")
        self.open_playback_queue_item_with_mode(
            item,
            show_player=show_player,
            announce_start=announce_start,
            focus_target=focus_target,
            keep_current_ui=keep_current_ui,
        )

    def open_playback_queue_item_with_mode(
        self,
        item: dict,
        show_player: bool = True,
        announce_start: bool = False,
        focus_target: str = "player",
        keep_current_ui: bool = False,
    ) -> None:
        url = str(item.get("url") or "")
        if not url:
            self.announce_player(self.t("no_selection"))
            return
        self.clear_player_sequence()
        source_screen = str(item.get("_return_screen") or "")
        if source_screen == "folder":
            folder = str(item.get("_return_folder") or self.current_local_folder_path or self.last_search_query)
            folder_items = self.current_local_folder_items if folder == self.current_local_folder_path else []
            if folder and not folder_items:
                folder_items = self.cached_local_folder_items(Path(folder))
            if folder_items:
                self.current_local_folder_path = folder
                self.current_local_folder_items = [dict(result) for result in folder_items]
                self.return_results = list(self.current_local_folder_items)
                self.return_all_results = list(self.current_local_folder_items)
                self.return_visible_count = len(self.current_local_folder_items)
            self.player_return_screen = "folder"
            self.player_return_data = {
                "index": int(item.get("_return_index") or 0),
                "folder": folder,
            }
            self.return_index = int(item.get("_return_index") or 0)
        else:
            self.player_return_screen = "playback_queue"
            self.player_return_data = {}
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(
            url,
            str(item.get("title") or ""),
            show_player=show_player,
            announce_start=announce_start,
            focus_target=focus_target,
            keep_current_ui=keep_current_ui,
        )

    @staticmethod
    def is_video_file_extension(path: Path) -> bool:
        return path.suffix.lower() in {".3g2", ".3gp", ".avi", ".m4v", ".mkv", ".mov", ".mp4", ".mpeg", ".mpg", ".webm", ".wmv"}

    def local_edit_audio_filters(self) -> list[str]:
        speed = max(0.25, min(4.0, self.current_speed_value()))
        pitch = max(0.5, min(2.0, self.current_pitch_value()))
        enabled, gains = self.effective_equalizer_state()
        filters = self.ffmpeg_equalizer_filters(gains) if enabled else []
        if abs(pitch - 1.0) >= 0.001:
            filters.extend([f"asetrate=48000*{pitch:.6f}", "aresample=48000"])
        filters.extend(self.ffmpeg_atempo_chain(speed / pitch))
        return filters

    def local_edit_audio_codec_args(self, suffix: str) -> list[str]:
        suffix = suffix.lower()
        if suffix == ".mp3":
            return ["-c:a", "libmp3lame", "-b:a", "320k"]
        if suffix in {".m4a", ".mp4", ".m4v", ".mov"}:
            return ["-c:a", "aac", "-b:a", "256k"]
        if suffix == ".opus":
            return ["-c:a", "libopus", "-b:a", "160k"]
        if suffix == ".wav":
            return ["-c:a", "pcm_s16le"]
        if suffix == ".flac":
            return ["-c:a", "flac"]
        return ["-c:a", "aac", "-b:a", "256k"]

    def start_player_monitor(self, generation: int) -> None:
        threading.Thread(target=self.player_monitor_worker, args=(generation,), daemon=True).start()

    def player_monitor_worker(self, generation: int) -> None:
        while generation == self.player_generation and self.mpv_process_alive():
            time.sleep(0.5)
            if generation != self.player_generation or not self.mpv_process_alive():
                return
            try:
                eof_reached = bool(self.mpv_get_property("eof-reached", timeout=0.25))
            except Exception:
                eof_reached = False
            if eof_reached:
                wx.CallAfter(self.handle_player_eof, generation)
                return

    def handle_player_eof(self, generation: int) -> None:
        if generation != self.player_generation:
            return
        if self.mpv_process_alive():
            try:
                if not bool(self.mpv_get_property("eof-reached", timeout=0.15)):
                    return
            except Exception:
                pass
        if self.repeat_current:
            self.player_ended = False
            self.player_paused = False
            self.update_play_pause_buttons()
            self.restart_current_playback(announce=False)
            return
        if self.effective_autoplay_next():
            if getattr(self.settings, "autoplay_related", False) and self.current_video_item and self.is_youtube_url(self.current_video_item.get("url")):
                threading.Thread(target=self.fetch_related_and_play_next, args=(self.current_video_item, generation), daemon=True).start()
                return
            sequence_active = self.current_player_sequence_active()
            if not sequence_active:
                queued_item = self.pop_next_playback_queue_item()
                if queued_item:
                    self.open_playback_queue_item_with_mode(queued_item, show_player=self.in_player_screen or not self.background_playback_enabled())
                    return
            next_item = self.relative_player_item(1)
            if next_item:
                self.open_relative_player_item(next_item)
                return
            if sequence_active:
                queued_item = self.pop_next_playback_queue_item()
                if queued_item:
                    self.open_playback_queue_item_with_mode(queued_item, show_player=self.in_player_screen or not self.background_playback_enabled())
                    return
        self.player_ended = True
        self.player_paused = True
        self.update_play_pause_buttons()

    def apply_related_videos_and_play(self, normalized_results: list[dict], generation: int) -> None:
        if generation != self.player_generation:
            return
        self.show_results(normalized_results, focus_results=False)
        first_item = normalized_results[0]
        self.open_relative_player_item(first_item)

    def player_play_pause(self) -> None:
        if self.player_kind != "mpv":
            return
        if not self.mpv_process_alive():
            self.restart_current_playback()
            return
        try:
            eof_reached = bool(self.mpv_get_property("eof-reached", timeout=0.25))
        except Exception:
            eof_reached = False
        if self.player_ended or eof_reached:
            if self.player_should_restart_from_end(eof_reached):
                self.restart_current_playback()
                return
            self.player_ended = False
            try:
                self.mpv_set_property("pause", False, timeout=0.5)
                self.start_player_monitor(self.player_generation)
                self.player_paused = False
                self.update_play_pause_buttons()
                self.announce_play_pause_state(False)
            except Exception:
                self.toggle_player_pause_fallback()
            return
        self.toggle_player_pause()

    def toggle_player_pause(self) -> None:
        self.cancel_clip_preview()
        try:
            paused = bool(self.mpv_get_property("pause", timeout=0.35))
            new_paused = not paused
            self.mpv_set_property("pause", new_paused, timeout=0.5)
            self.player_paused = new_paused
            self.update_play_pause_buttons()
            self.announce_play_pause_state(new_paused)
        except Exception:
            self.toggle_player_pause_fallback()

    def toggle_player_pause_fallback(self) -> None:
        self.player_command("cycle pause")
        wx.CallLater(140, self.refresh_play_pause_button_state)
        if self.settings.announce_play_pause:
            wx.CallLater(120, self.announce_current_play_pause_state)

    def player_should_restart_from_end(self, eof_reached: bool) -> bool:
        if self.player_ended and not eof_reached:
            return True
        if not eof_reached:
            return False
        try:
            elapsed = self.mpv_get_property("time-pos", timeout=0.2)
            duration = self.mpv_get_property("duration", timeout=0.2)
            if elapsed is not None and duration is not None:
                return float(elapsed) >= max(0.0, float(duration) - 0.35)
        except Exception:
            pass
        return True

    def restart_current_playback(self, announce: bool = True) -> None:
        self.cancel_clip_preview()
        self.player_ended = False
        self.player_paused = False
        if self.mpv_process_alive():
            try:
                self.mpv_send(["seek", 0, "absolute+exact"], timeout=0.8)
                self.mpv_set_property("pause", False, timeout=0.8)
                self.start_player_monitor(self.player_generation)
                self.update_play_pause_buttons()
                if announce:
                    self.announce_player(self.t("playback_restarted"))
                return
            except Exception:
                pass
        item = dict(self.current_video_item or self.current_video_info or {})
        url = str(item.get("url") or "")
        if not url:
            return
        key = self.playback_key(item)
        if key and key in self.playback_positions:
            self.playback_positions.pop(key, None)
            self.save_playback_positions()
        self.current_video_item = item
        self.current_video_info = dict(item)
        self.play_url(url, str(item.get("title") or ""))

    def player_command(self, command: str) -> None:
        if self.player_kind != "mpv" or not self.ipc_path:
            return
        try:
            shlex_module = import_module("shlex")
            self.mpv_send(shlex_module.split(command), timeout=0.5)
        except Exception:
            pass

    def player_seek(self, seconds: float) -> None:
        if self.player_kind != "mpv" or not self.ipc_path:
            return
        self.cancel_clip_preview()
        was_ended = self.player_ended
        try:
            response = self.mpv_request(["seek", float(seconds), "relative+exact"], timeout=0.8)
            if response.get("error") == "success":
                self.after_player_seek(seconds, was_ended)
                return
        except Exception:
            pass
        try:
            self.mpv_send(["seek", float(seconds), "relative+exact"], timeout=0.8)
            self.after_player_seek(seconds, was_ended)
        except Exception:
            pass

    def after_player_seek(self, seconds: float, was_ended: bool) -> None:
        if seconds < 0 and was_ended:
            self.player_ended = False
            self.start_player_monitor(self.player_generation)

    def audio_export_codec_args(self) -> list[str]:
        fmt = str(self.settings.audio_format or "mp3").lower()
        quality = self.normalize_audio_quality_value(self.settings.audio_quality)
        if fmt == "mp3":
            bitrate = "320" if quality == "0" else quality
            return ["-vn", "-c:a", "libmp3lame", "-b:a", f"{bitrate}k"]
        if fmt == "m4a":
            bitrate = "256" if quality == "0" else quality
            return ["-vn", "-c:a", "aac", "-b:a", f"{bitrate}k"]
        if fmt == "opus":
            bitrate = "160" if quality == "0" else quality
            return ["-vn", "-c:a", "libopus", "-b:a", f"{bitrate}k"]
        if fmt == "wav":
            return ["-vn", "-c:a", "pcm_s16le"]
        if fmt == "flac":
            return ["-vn", "-c:a", "flac"]
        return ["-vn"]

    @staticmethod
    def next_playback_speed(current: float, delta: float) -> float:
        return MiscUI.clamp_rate(current + delta, 0.25, 4.0)

    @staticmethod
    def format_playback_rate(value: float) -> str:
        if abs(value - round(value)) < 0.001:
            return f"{value:.1f}"
        return f"{value:.2f}".rstrip("0").rstrip(".")

    def normalized_speed_audio_mode(self) -> str:
        mode = str(getattr(self.settings, "speed_audio_mode", SPEED_AUDIO_MODE_RUBBERBAND) or SPEED_AUDIO_MODE_RUBBERBAND)
        return self.normalize_speed_audio_mode_value(mode)

    def normalized_audio_output_device(self) -> str:
        device = str(getattr(self.settings, "audio_output_device", "auto") or "auto").strip()
        return device or "auto"

    def normalized_video_format(self) -> str:
        return self.normalize_video_format_value(getattr(self.settings, "video_format", VIDEO_FORMAT_MP4))

    def speed_audio_mode_labels(self) -> list[str]:
        return [
            self.t("speed_audio_mode_rubberband"),
            self.t("speed_audio_mode_scaletempo2"),
            self.t("speed_audio_mode_mpv"),
            self.t("speed_audio_mode_scaletempo"),
        ]

    def video_format_labels(self) -> list[str]:
        return [
            self.t("video_format_mp4_recommended"),
            self.t("video_format_best_available"),
            self.t("video_format_mp4_single"),
            self.t("video_format_smallest"),
        ]

    @staticmethod
    def audio_quality_label(value: str) -> str:
        value = str(value or "").strip()
        if value == "0":
            return "Best variable quality (VBR 0)"
        if value in {"1", "2", "3", "4", "5", "6", "7", "8", "9", "10"}:
            return f"Variable quality (VBR {value})"
        return f"{value} kbps"

    def audio_quality_labels(self) -> list[str]:
        return [self.audio_quality_label(value) for value in AUDIO_QUALITY_OPTIONS]

    @staticmethod
    def normalize_audio_quality_value(value: str) -> str:
        normalized = str(value or "").strip().lower().replace("kbps", "").replace("k", "").strip()
        try:
            numeric = float(normalized)
        except (TypeError, ValueError):
            return "0"
        if numeric.is_integer():
            normalized = str(int(numeric))
        else:
            normalized = str(numeric)
        return normalized if normalized in AUDIO_QUALITY_OPTIONS else "0"

    @staticmethod
    def normalize_speed_audio_mode_value(mode: str) -> str:
        normalized = str(mode or "").strip()
        if normalized in SPEED_AUDIO_MODE_OPTIONS:
            return normalized
        lowered = normalized.lower()
        if "rubber" in lowered:
            return SPEED_AUDIO_MODE_RUBBERBAND
        if "classic" in lowered or lowered == "scaletempo":
            return SPEED_AUDIO_MODE_SCALETEMPO
        if "mpv" in lowered or "default" in lowered:
            return SPEED_AUDIO_MODE_MPV
        return SPEED_AUDIO_MODE_RUBBERBAND

    @staticmethod
    def normalize_video_format_value(value: str) -> str:
        normalized = str(value or "").strip()
        if normalized in VIDEO_FORMAT_OPTIONS:
            return normalized
        return LEGACY_VIDEO_FORMAT_MAP.get(normalized, VIDEO_FORMAT_MP4)

    def close_current_player(self) -> None:
        was_player_screen = self.in_player_screen
        was_main_menu = self.in_main_menu
        had_background_section = bool(getattr(self, "background_player_section_added", False))
        self.stop_player(silent=False)
        self.in_player_screen = False
        self.current_stream_url = ""
        self.current_stream_headers = {}
        self.current_audio_device = ""
        self.announce_player(self.t("player_closed"))
        if was_main_menu or was_player_screen or had_background_section:
            self.show_main_menu()

    def focus_in_background_player_controls(self, focus: wx.Window | None) -> bool:
        if not focus:
            return False
        return any(self.window_is_or_descendant(focus, control) for control in getattr(self, "background_player_controls", []))

    def focus_in_player_controls(self, focus: wx.Window | None) -> bool:
        if not focus:
            return False
        if self.window_is_or_descendant(focus, getattr(self, "player_panel", None)):
            return True
        controls = list(getattr(self, "player_action_controls", [])) + list(getattr(self, "player_navigation_controls", []))
        return any(focus is control for control in controls)

    def save_current_playback_position(self) -> None:
        if not getattr(self.settings, "resume_playback", True) or not self.mpv_process_alive():
            return
        key = self.playback_key()
        if not key:
            return
        try:
            elapsed = self.mpv_get_property("time-pos", timeout=0.35)
            duration = self.mpv_get_property("duration", timeout=0.35)
            if elapsed is None:
                return
            position = float(elapsed)
            total = float(duration or 0.0)
            if position < 5.0:
                self.playback_positions.pop(key, None)
            elif total and position > max(5.0, total - 8.0):
                self.playback_positions.pop(key, None)
            else:
                self.playback_positions[key] = round(position, 1)
            self.save_playback_positions()
        except Exception:
            pass

    def video_format_selector(self, video_mode: str) -> str:
        height = self.settings.max_video_height if self.settings.max_video_height > 0 else 0
        limit = f"[height<={height}]" if height else ""
        if video_mode == VIDEO_FORMAT_BEST_ANY:
            return f"bestvideo{limit}+bestaudio/best{limit}/best"
        if video_mode == VIDEO_FORMAT_MP4_SINGLE:
            return f"best[ext=mp4][vcodec!=none][acodec!=none]{limit}/best[ext=mp4][vcodec!=none][acodec!=none]/best{limit}/best"
        if video_mode == VIDEO_FORMAT_SMALLEST:
            return f"worst[ext=mp4][vcodec!=none][acodec!=none]{limit}/worst[ext=mp4][vcodec!=none][acodec!=none]/worst{limit}/worst"
        return f"best[ext=mp4][vcodec!=none][acodec!=none]{limit}/best[ext=mp4][vcodec!=none][acodec!=none]/bestvideo[ext=mp4]{limit}+bestaudio[ext=m4a]/bestvideo{limit}+bestaudio/best{limit}/best"

    def resolve_player(self) -> tuple[str, str] | None:
        configured = self.settings.player_command.strip().strip('"')
        if configured and (Path(configured).exists() or shutil.which(configured)):
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

