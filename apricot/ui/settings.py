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

class SettingsMixin:
    def open_settings_shortcut(self) -> None:
        self.run_global_navigation_shortcut(self.open_settings_screen)



    def open_settings_screen(self) -> None:
        self.settings_section_index = 0
        self.show_settings()



    def focus_settings_section_list_later(self) -> None:
        generation = self.settings_render_generation
        self.settings_initial_focus_pending = True
        wx.CallAfter(self.focus_settings_section_list_if_safe, generation)
        wx.CallLater(100, self.focus_settings_section_list_if_safe, generation)


    def focus_settings_section_list_if_safe(self, generation: int) -> None:
        if not getattr(self, "settings_initial_focus_pending", False):
            return
        if generation != getattr(self, "settings_render_generation", -1):
            return
        target = self.live_window(getattr(self, "settings_section_list", None))
        if target is None:
            return
        focus = wx.Window.FindFocus()
        if focus is target:
            self.settings_initial_focus_pending = False
            return
        if focus is not None:
            for control in getattr(self, "settings_control_order", []):
                if self.window_is_or_descendant(focus, control):
                    self.settings_initial_focus_pending = False
                    return
        self.settings_initial_focus_pending = False
        self.safe_set_focus(target)


    def settings_sections(self) -> list[tuple[str, str]]:
        return [
            (self.t("general_section"), "general"),
            (self.t("playback_section"), "playback"),
            (self.t("equalizer_section"), "equalizer"),
            (self.t("downloads_section"), "downloads"),
            (self.t("library_section"), "library"),
            (self.t("podcasts_section"), "podcasts"),
            (self.t("notifications_section"), "notifications"),
            (self.t("cookies_network_section"), "cookies"),
            (self.t("keyboard_shortcuts_section"), "shortcuts"),
        ]


    def settings_section_label(self, section_name: str) -> str:
        for label, name in self.settings_sections():
            if name == section_name:
                return label
        return section_name


    @staticmethod
    def settings_section_fields() -> dict[str, list[str]]:
        return {
            "general": [
                "language",
                "download_folder",
                "results_limit",
                "direct_link_enter_action",
                "show_shortcuts_in_labels",
                "auto_update_ytdlp",
                "auto_update_app",
                "app_update_interval_hours",
                "app_update_notifications",
                "close_to_tray",
                "start_with_windows",
                "tray_notification",
                "skipped_update_version",
                "update_channel",
            ],
            "playback": [
                "autoplay_next",
                "autoplay_related",
                "prefer_browser_playback",
                "player_fullscreen",
                "player_start_paused",
                "announce_play_pause",
                "announce_playback_finished",
                "enable_background_playback",
                "player_speed",
                "speed_audio_mode",
                "show_video_details_by_default",
                "enable_age_restricted_videos",
                "enable_stream_cache",
                "enable_stream_url_cache",
                "stream_url_cache_minutes",
                "prefetch_next_stream_url",
                "gapless_playback",
                "replaygain_mode",
                "enable_online_lyrics",
                "cache_folder",
                "cache_size_mb",
                "resume_playback",
                "audio_output_device",
                "speed_step",
                "pitch_step",
                "pitch_mode",
                "seek_seconds",
                "volume_step",
                "default_volume",
                "volume_boost_by_default",
            ],
            "equalizer": [
                "global_equalizer_enabled",
                "global_equalizer_preset",
                "global_equalizer_gains",
                "equalizer_preset_gains",
                "equalizer_custom_names",
                "equalizer_db_range",
                "equalizer_clipping_protection",
            ],
            "downloads": [
                "audio_format",
                "video_format",
                "max_video_height",
                "ask_download_location_each_time",
                "quiet_downloads",
                "keep_playlist_order",
                "filename_template",
                "audio_quality",
                "write_thumbnail",
                "write_description",
                "write_info_json",
                "write_subtitles",
                "auto_subtitles",
                "subtitle_languages",
                "embed_metadata",
                "embed_thumbnail",
                "restrict_filenames",
                "open_folder_after_download",
                "popup_when_download_complete",
                "popup_when_conversion_complete",
                "confirm_before_download",
                "download_archive",
            ],
            "library": [
                "subscription_check_enabled",
                "subscription_check_interval_hours",
                "last_subscription_check",
                "enable_trending",
                "enable_history",
                "history_limit",
            ],
            "podcasts": [
                "enable_podcasts_rss",
                "podcast_search_provider",
                "podcast_search_country",
                "podcast_search_limit",
                "rss_max_items",
                "rss_refresh_on_startup",
                "rss_auto_refresh_enabled",
                "rss_refresh_interval_hours",
            ],
            "notifications": [
                "windows_notifications",
                "download_notifications",
                "subscription_notifications",
                "app_update_notifications",
            ],
            "cookies": [
                "rate_limit",
                "proxy",
                "youtube_data_api_key",
                "cookies_file",
                "cookies_from_browser",
                "cookies_browser_profile",
                "show_advanced_network_settings",
                "cookie_user_agent",
                "ffmpeg_location",
                "concurrent_fragments",
                "retries",
                "socket_timeout",
            ],
            "shortcuts": ["keyboard_shortcuts"],
        }


    def on_settings_section_changed(self, event) -> None:
        event.Skip()
        if not hasattr(self, "settings_section_list"):
            return
        new_index = self.settings_section_list.GetSelection()
        if new_index < 0 or new_index == self.settings_section_index and self.settings_pending_section_index < 0:
            return
        if not self.settings_controls_applied_for_pending:
            self.apply_settings_from_visible_controls()
            self.settings_controls_applied_for_pending = True
        self.settings_pending_section_index = new_index
        self.settings_render_generation += 1
        wx.CallLater(140, self.render_pending_settings_section, self.settings_render_generation)


    def render_pending_settings_section(self, generation: int) -> None:
        if generation != self.settings_render_generation or self.settings_pending_section_index < 0:
            return
        self.settings_section_index = self.settings_pending_section_index
        self.settings_pending_section_index = -1
        self.settings_controls_applied_for_pending = False
        self.render_settings_section()


    def flush_settings_section_render(self) -> None:
        if self.settings_pending_section_index < 0:
            return
        self.settings_render_generation += 1
        self.settings_section_index = self.settings_pending_section_index
        self.settings_pending_section_index = -1
        self.settings_controls_applied_for_pending = False
        self.render_settings_section()


    def on_settings_section_key(self, event: wx.KeyEvent) -> None:
        key = event.GetKeyCode()
        if key == wx.WXK_TAB and not event.ShiftDown():
            self.settings_initial_focus_pending = False
            self.flush_settings_section_render()
            self.focus_first_settings_control()
            return
        if key == wx.WXK_RETURN:
            self.flush_settings_section_render()
            self.focus_first_settings_control()
            return
        event.Skip()
        if key in {wx.WXK_UP, wx.WXK_DOWN, wx.WXK_HOME, wx.WXK_END, wx.WXK_PAGEUP, wx.WXK_PAGEDOWN}:
            wx.CallAfter(self._sync_settings_section_from_list)

    def _sync_settings_section_from_list(self) -> None:
        if not hasattr(self, "settings_section_list"):
            return
        try:
            new_index = self.settings_section_list.GetSelection()
        except RuntimeError:
            return
        if new_index < 0 or (new_index == self.settings_section_index and self.settings_pending_section_index < 0):
            return
        if not self.settings_controls_applied_for_pending:
            self.apply_settings_from_visible_controls()
            self.settings_controls_applied_for_pending = True
        self.settings_pending_section_index = new_index
        self.settings_render_generation += 1
        wx.CallLater(140, self.render_pending_settings_section, self.settings_render_generation)


    def focus_first_settings_control(self) -> None:
        if self.settings_control_order:
            self.safe_set_focus(self.settings_control_order[0])


    def apply_settings_tab_order(self) -> None:
        section_list = self.live_window(getattr(self, "settings_section_list", None))
        settings_scroller = self.live_window(getattr(self, "settings_scroller", None))
        if section_list is not None and settings_scroller is not None:
            try:
                settings_scroller.MoveAfterInTabOrder(section_list)
            except Exception:
                pass
        self.apply_tab_order(list(getattr(self, "settings_control_order", [])))



    def render_settings_section(self) -> None:
        if not hasattr(self, "settings_scroller"):
            return
        try:
            self.settings_scroller.Freeze()
        except RuntimeError:
            pass
        old_sizer = self.settings_scroller.GetSizer()
        if old_sizer:
            old_sizer.Clear(delete_windows=True)
        self.controls = {}
        self.choice_values = {}
        self.settings_control_order = []
        form = wx.FlexGridSizer(0, 2, 6, 6)
        form.AddGrowableCol(1, 1)
        section_name = self.settings_sections()[self.settings_section_index][1]
        if section_name != "equalizer":
            self.visible_equalizer_draft_gains = {}

        def remember(key: str, ctrl: wx.Window) -> None:
            self.controls[key] = ctrl
            self.settings_control_order.append(ctrl)

        def text(key: str, value: str, style: int = 0):
            form.Add(wx.StaticText(self.settings_scroller, label=self.t(key)), 0, wx.ALIGN_CENTER_VERTICAL)
            ctrl = wx.TextCtrl(self.settings_scroller, value=value, style=style)
            ctrl.SetName(self.t(key))
            form.Add(ctrl, 1, wx.EXPAND)
            remember(key, ctrl)
            return ctrl

        def choice(key: str, value: str, options: list[str], labels: list[str] | None = None):
            form.Add(wx.StaticText(self.settings_scroller, label=self.t(key)), 0, wx.ALIGN_CENTER_VERTICAL)
            visible_options = labels or options
            ctrl = wx.Choice(self.settings_scroller, choices=visible_options)
            ctrl.SetName(self.t(key))
            selected = options.index(value) if value in options else 0
            ctrl.SetSelection(selected)
            form.Add(ctrl, 1, wx.EXPAND)
            if labels:
                self.choice_values[key] = options
            remember(key, ctrl)
            return ctrl

        def check(key: str, value: bool):
            form.AddSpacer(1)
            ctrl = wx.CheckBox(self.settings_scroller, label=self.t(key))
            ctrl.SetName(self.t(key))
            ctrl.SetValue(value)
            form.Add(ctrl, 1, wx.EXPAND)
            remember(key, ctrl)
            return ctrl

        def button(key: str, handler):
            form.AddSpacer(1)
            ctrl = wx.Button(self.settings_scroller, label=self.t(key))
            ctrl.SetName(self.t(key))
            ctrl.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            form.Add(ctrl, 0)
            self.settings_control_order.append(ctrl)
            return ctrl

        def button_label(label: str, handler):
            form.AddSpacer(1)
            ctrl = wx.Button(self.settings_scroller, label=label)
            ctrl.SetName(label)
            ctrl.Bind(wx.EVT_BUTTON, lambda _evt, fn=handler: fn())
            form.Add(ctrl, 0)
            self.settings_control_order.append(ctrl)
            return ctrl

        def slider(key: str, label: str, value: float, minimum: int, maximum: int, band_id: str | None = None):
            form.Add(wx.StaticText(self.settings_scroller, label=label), 0, wx.ALIGN_CENTER_VERTICAL)
            scaled_value = int(round(float(value) * 10))
            ctrl = wx.Slider(
                self.settings_scroller,
                value=min(max(scaled_value, minimum), maximum),
                minValue=minimum,
                maxValue=maximum,
                style=wx.SL_HORIZONTAL,
            )
            if band_id:
                ctrl._apricot_eq_band_id = str(band_id)
            self.configure_equalizer_slider_steps(ctrl)
            self.set_equalizer_slider_accessibility(ctrl, label)
            self.bind_equalizer_slider_events(ctrl, lambda evt, label_text=label: self.on_equalizer_settings_slider(evt, label_text))
            form.Add(ctrl, 1, wx.EXPAND)
            remember(key, ctrl)
            return ctrl

        def int_slider(key: str, label: str, value: int, minimum: int, maximum: int):
            form.Add(wx.StaticText(self.settings_scroller, label=label), 0, wx.ALIGN_CENTER_VERTICAL)
            ctrl = wx.Slider(
                self.settings_scroller,
                value=min(max(int(value), minimum), maximum),
                minValue=minimum,
                maxValue=maximum,
                style=wx.SL_HORIZONTAL,
            )
            unit = "percent" if key == "default_volume" else ""
            self.set_integer_slider_accessibility(ctrl, label, unit)
            ctrl.Bind(wx.EVT_SLIDER, lambda evt, label_text=label, unit_text=unit: self.set_integer_slider_accessibility(evt.GetEventObject(), label_text, unit_text))
            form.Add(ctrl, 1, wx.EXPAND)
            remember(key, ctrl)
            return ctrl

        if section_name == "general":
            form.Add(wx.StaticText(self.settings_scroller, label=self.t("language")), 0, wx.ALIGN_CENTER_VERTICAL)
            language_code = self.settings.language if self.settings.language in LANGUAGE_CODES else "en"
            lang = wx.Choice(self.settings_scroller, choices=[name for _code, name in LANGUAGES])
            lang.SetSelection(LANGUAGE_CODES.index(language_code))
            lang.SetName(self.t("language"))
            form.Add(lang, 1, wx.EXPAND)
            remember("language", lang)
            text("settings_file", str(SETTINGS_FILE), wx.TE_READONLY)
            text("download_folder", self.settings.download_folder)
            button("browse", self.choose_download_folder)
            button("set_default_player", self.open_windows_default_apps_settings)
            results_limit_value = "0" if self.settings.results_limit == 0 else str(min(250, self.settings.results_limit))
            result_limit_options = ["0", "10", "20", "50", "100", "150", "200", "250"]
            choice("results_limit", results_limit_value, result_limit_options, self.result_limit_labels(result_limit_options))
            choice("direct_link_enter_action", self.normalized_direct_link_enter_action(), DIRECT_LINK_ENTER_OPTIONS, self.direct_link_enter_action_labels())
            check("show_shortcuts_in_labels", bool(getattr(self.settings, "show_shortcuts_in_labels", True)))
            check("auto_update", self.settings.auto_update_ytdlp)
            check("auto_update_app", self.settings.auto_update_app)
            choice(
                "update_channel",
                getattr(self.settings, "update_channel", "beta"),
                ["stable", "beta"],
                [self.t("update_channel_stable"), self.t("update_channel_beta")]
            )
            choice(
                "app_update_interval",
                self.format_refresh_interval_value(self.settings.app_update_interval_hours, 6.0),
                REFRESH_INTERVAL_OPTIONS,
                self.refresh_interval_labels(),
            )
            button("check_ytdlp_updates_now", self.manual_ytdlp_update_check)
            button("check_app_updates_now", self.manual_app_update_check)
            check("close_to_tray", self.settings.close_to_tray)
            check("start_with_windows", self.settings.start_with_windows)
            check("tray_notification", self.settings.tray_notification)
            button("reset_all_settings", self.restore_default_settings)
        elif section_name == "playback":
            choice("player_speed", self.settings.player_speed, [self.format_playback_rate(step) for step in PLAYBACK_SPEED_STEPS if step <= 2.0])
            choice("speed_audio_mode", self.normalized_speed_audio_mode(), SPEED_AUDIO_MODE_OPTIONS, self.speed_audio_mode_labels())
            choice("pitch_mode", self.normalized_pitch_mode(), PITCH_MODE_OPTIONS, self.pitch_mode_labels())
            choice("speed_step", self.format_step_value(self.settings.speed_step), RATE_STEP_OPTIONS)
            choice("pitch_step", self.format_step_value(self.settings.pitch_step), RATE_STEP_OPTIONS)
            check("show_video_details_by_default", self.settings.show_video_details_by_default)
            check("enable_age_restricted_videos", self.settings.enable_age_restricted_videos)
            check("enable_stream_cache", self.settings.enable_stream_cache)
            check("enable_stream_url_cache", bool(getattr(self.settings, "enable_stream_url_cache", True)))
            choice(
                "stream_url_cache_minutes",
                str(self.normalized_stream_url_cache_minutes()),
                STREAM_URL_CACHE_OPTIONS,
                self.stream_url_cache_labels(STREAM_URL_CACHE_OPTIONS),
            )
            check("prefetch_next_stream_url", bool(getattr(self.settings, "prefetch_next_stream_url", True)))
            check("gapless_playback", bool(getattr(self.settings, "gapless_playback", True)))
            choice("replaygain_mode", self.normalized_replaygain_mode(), REPLAYGAIN_MODE_OPTIONS, self.replaygain_mode_labels())
            check("enable_online_lyrics", bool(getattr(self.settings, "enable_online_lyrics", True)))
            text("cache_folder", self.settings.cache_folder or str(DEFAULT_CACHE_DIR))
            choice("cache_size_mb", str(self.settings.cache_size_mb), ["128", "256", "512", "1024", "2048", "4096"])
            check("resume_playback", self.settings.resume_playback)
            device_values, device_labels = self.audio_output_device_options(allow_probe=False)
            choice("default_audio_device", self.normalized_audio_output_device(), device_values, device_labels)
            self.refresh_audio_output_devices_async()
            choice("seek_seconds", self.format_seek_seconds_value(self.seek_seconds_value()), SEEK_SECONDS_OPTIONS)
            choice("volume_step", str(self.settings.volume_step), ["1", "2", "5", "10"])
            int_slider("default_volume", self.t("default_volume"), self.default_volume_value(), 0, self.default_volume_max_value())
            volume_boost_default_box = check("volume_boost_by_default", bool(getattr(self.settings, "volume_boost_by_default", False)))
            volume_boost_default_box.Bind(wx.EVT_CHECKBOX, self.on_volume_boost_by_default_settings_changed)
            check("autoplay_next", self.settings.autoplay_next)
            check("autoplay_related", self.settings.autoplay_related)
            check("browser_playback", self.settings.prefer_browser_playback)
            check("fullscreen", self.settings.player_fullscreen)
            check("start_paused", self.settings.player_start_paused)
            check("announce_play_pause", self.settings.announce_play_pause)
            check("announce_playback_finished", bool(getattr(self.settings, "announce_playback_finished", True)))
            check("enable_background_playback", bool(getattr(self.settings, "enable_background_playback", False)))
        elif section_name == "equalizer":
            self.equalizer_controls_loading = True
            try:
                equalizer_enabled = bool(getattr(self.settings, "global_equalizer_enabled", False))
                enabled_box = check("global_equalizer", equalizer_enabled)
                enabled_box.Bind(wx.EVT_CHECKBOX, self.on_global_equalizer_toggle)
                clipping_box = check("equalizer_clipping_protection", bool(getattr(self.settings, "equalizer_clipping_protection", False)))
                clipping_box.Bind(wx.EVT_CHECKBOX, self.on_equalizer_clipping_protection_changed)
                if not equalizer_enabled:
                    self.visible_equalizer_draft_gains = {}
                if equalizer_enabled:
                    preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
                    self.visible_equalizer_preset = preset
                    preset_choice = choice("equalizer_preset", preset, self.equalizer_preset_options(), self.equalizer_preset_labels())
                    preset_choice.Bind(wx.EVT_CHOICE, self.on_equalizer_settings_preset_changed)
                    if self.is_custom_equalizer_preset(preset):
                        name_ctrl = text("equalizer_preset_name", self.equalizer_custom_name(preset))
                        name_ctrl.Bind(wx.EVT_KILL_FOCUS, self.on_equalizer_settings_name_changed)
                    db_range = str(self.equalizer_db_range_value())
                    range_choice = choice("equalizer_db_range", db_range, EQ_RANGE_OPTIONS)
                    range_choice.Bind(wx.EVT_CHOICE, self.on_equalizer_range_changed)
                    gains = self.equalizer_gains_for_preset(preset)
                    self.visible_equalizer_draft_gains = self.normalized_equalizer_gains(gains)
                    slider_min = -int(db_range) * 10
                    slider_max = int(db_range) * 10
                    for band_id, band_label in EQ_BANDS:
                        label = self.t("equalizer_band_gain", band=band_label)
                        slider(f"eq_{band_id}", label, gains.get(band_id, 0.0), slider_min, slider_max, band_id=band_id)
                    button("reset_equalizer", self.reset_visible_equalizer_controls)
                    button("add_equalizer_profile", self.add_equalizer_profile_from_settings)
                    button("import_equalizer_profile", self.import_equalizer_profile_from_settings)
                    button("export_equalizer_profile", self.export_visible_equalizer_profile_from_settings)
                    if self.is_custom_equalizer_preset(preset):
                        button("delete_equalizer_profile", self.delete_visible_equalizer_profile_from_settings)
            finally:
                self.equalizer_controls_loading = False
        elif section_name == "downloads":
            check("confirm_download", self.settings.confirm_before_download)
            check("open_after_download", self.settings.open_folder_after_download)
            check("download_complete_popup", self.settings.popup_when_download_complete)
            check("conversion_complete_popup", bool(getattr(self.settings, "popup_when_conversion_complete", True)))
            check("ask_download_location_each_time", self.settings.ask_download_location_each_time)
            choice("audio_format", self.settings.audio_format, ["mp3", "m4a", "opus", "wav", "flac"])
            choice("audio_quality", self.normalize_audio_quality_value(self.settings.audio_quality), AUDIO_QUALITY_OPTIONS, self.audio_quality_labels())
            choice("video_format", self.normalized_video_format(), VIDEO_FORMAT_OPTIONS, self.video_format_labels())
            choice("max_height", str(self.settings.max_video_height), ["0", "360", "480", "720", "1080", "1440", "2160"])
            text("filename_template", self.settings.filename_template or DEFAULT_FILENAME_TEMPLATE)
            text("subtitle_langs", self.settings.subtitle_languages)
            check("quiet_downloads", self.settings.quiet_downloads)
            check("playlist_order", self.settings.keep_playlist_order)
            check("write_thumbnail", self.settings.write_thumbnail)
            check("write_description", self.settings.write_description)
            check("write_info_json", self.settings.write_info_json)
            check("write_subtitles", self.settings.write_subtitles)
            check("auto_subtitles", self.settings.auto_subtitles)
            check("embed_metadata", self.settings.embed_metadata)
            check("embed_thumbnail", self.settings.embed_thumbnail)
            check("restrict_filenames", self.settings.restrict_filenames)
            check("download_archive", self.settings.download_archive)
        elif section_name == "library":
            check("enable_trending", bool(getattr(self.settings, "enable_trending", False)))
            check("enable_history", self.settings.enable_history)
            choice("history_limit", str(self.settings.history_limit), ["100", "250", "500", "1000", "2000"])
            check("subscription_check_enabled", self.settings.subscription_check_enabled)
            choice(
                "subscription_check_interval",
                self.format_refresh_interval_value(self.settings.subscription_check_interval_hours, 6.0),
                REFRESH_INTERVAL_OPTIONS,
                self.refresh_interval_labels(),
            )
            button("subscription_check_now", lambda: self.check_subscriptions(manual=True))
        elif section_name == "podcasts":
            check("enable_podcasts_rss", self.settings.enable_podcasts_rss)
            text("podcast_source", self.t("podcast_source_info"), wx.TE_READONLY | wx.TE_MULTILINE)
            choice("podcast_search_provider", self.normalized_podcast_search_provider(), PODCAST_DIRECTORY_PROVIDER_OPTIONS, [self.t("podcast_search_provider_apple")])
            choice("podcast_search_country", self.normalized_podcast_search_country(), PODCAST_COUNTRY_OPTIONS)
            choice("podcast_search_limit", str(self.settings.podcast_search_limit), ["10", "20", "50", "100", "150", "200"])
            choice("rss_max_items", str(self.settings.rss_max_items), ["25", "50", "100", "200", "500"])
            check("rss_refresh_on_startup", self.settings.rss_refresh_on_startup)
            check("rss_auto_refresh_enabled", self.settings.rss_auto_refresh_enabled)
            choice(
                "rss_refresh_interval",
                self.format_refresh_interval_value(self.settings.rss_refresh_interval_hours, 12.0),
                REFRESH_INTERVAL_OPTIONS,
                self.refresh_interval_labels(),
            )
        elif section_name == "notifications":
            check("windows_notifications", self.settings.windows_notifications)
            check("download_notifications", self.settings.download_notifications)
            check("subscription_notifications", self.settings.subscription_notifications)
            check("app_update_notifications", self.settings.app_update_notifications)
        elif section_name == "cookies":
            text("cookies", self.settings.cookies_file)
            button("choose_cookies_file", self.choose_cookies_file)
            choice("cookies_from_browser", self.settings.cookies_from_browser or "none", COOKIES_BROWSER_OPTIONS)
            profile_values = self.cookie_profile_choice_values(self.settings.cookies_from_browser or "none")
            profile_value = self.settings.cookies_browser_profile if self.settings.cookies_browser_profile in profile_values else COOKIE_PROFILE_AUTO
            choice("cookies_browser_profile", profile_value, profile_values, self.cookie_profile_choice_labels(profile_values))
            button("open_youtube_login_profile", self.open_youtube_login_profile_from_settings)
            button("export_browser_cookies", self.export_browser_cookies_from_settings)
            text("proxy", self.settings.proxy)
            text("youtube_data_api_key", getattr(self.settings, "youtube_data_api_key", ""))
            button("obtain_youtube_api_key", self.open_youtube_api_key_page_from_settings)
            advanced_box = check("show_advanced_network_settings", bool(getattr(self.settings, "show_advanced_network_settings", False)))
            advanced_box.Bind(wx.EVT_CHECKBOX, self.on_advanced_network_toggle)
            if bool(getattr(self.settings, "show_advanced_network_settings", False)):
                text("cookie_user_agent", getattr(self.settings, "cookie_user_agent", ""))
                text("rate_limit", self.settings.rate_limit)
                text("ffmpeg", self.settings.ffmpeg_location)
                choice("fragments", str(self.settings.concurrent_fragments), ["1", "2", "4", "8", "16"])
            choice("retries", str(self.settings.retries), ["0", "3", "5", "10", "20"])
            choice("timeout", str(self.settings.socket_timeout), ["5", "10", "20", "30", "60"])
        elif section_name == "shortcuts":
            form.Add(wx.StaticText(self.settings_scroller, label=self.t("keyboard_shortcuts_help")), 0, wx.ALIGN_CENTER_VERTICAL)
            form.AddSpacer(1)
            shortcuts = self.normalized_keyboard_shortcuts(getattr(self.settings, "keyboard_shortcuts", {}) or {})
            self.shortcut_editor_values = dict(shortcuts)
            self.shortcut_editor_actions = [action for action, _label_key in SHORTCUT_DEFINITIONS]
            if self.shortcut_editor_current_action not in self.shortcut_editor_actions:
                self.shortcut_editor_current_action = self.shortcut_editor_actions[0] if self.shortcut_editor_actions else ""
            selected_index = self.shortcut_editor_actions.index(self.shortcut_editor_current_action) if self.shortcut_editor_current_action in self.shortcut_editor_actions else 0
            form.Add(wx.StaticText(self.settings_scroller, label=self.t("shortcut_actions")), 0, wx.ALIGN_CENTER_VERTICAL)
            shortcut_list = wx.ListBox(
                self.settings_scroller,
                choices=[self.shortcut_display_label(action, self.shortcut_editor_values.get(action, "")) for action in self.shortcut_editor_actions],
                style=wx.LB_SINGLE,
            )
            shortcut_list.SetName(self.t("shortcut_actions"))
            shortcut_list.SetMinSize((-1, 260))
            if self.shortcut_editor_actions:
                shortcut_list.SetSelection(selected_index)
            shortcut_list.Bind(wx.EVT_LISTBOX, self.on_shortcut_action_selected)
            form.Add(shortcut_list, 1, wx.EXPAND)
            remember("shortcut_action_list", shortcut_list)
            form.Add(wx.StaticText(self.settings_scroller, label=self.t("shortcut_value")), 0, wx.ALIGN_CENTER_VERTICAL)
            active_value = self.shortcut_editor_values.get(self.shortcut_editor_current_action, "")
            shortcut_ctrl = wx.TextCtrl(self.settings_scroller, value=active_value, style=wx.TE_PROCESS_ENTER)
            shortcut_ctrl.SetName(f"{self.t('shortcut_value')}. {self.t('shortcut_capture_hint')}")
            setattr(shortcut_ctrl, "_apricot_shortcut_capture", True)
            setattr(shortcut_ctrl, "_apricot_shortcut_action", self.shortcut_editor_current_action)
            shortcut_ctrl.Bind(wx.EVT_KEY_DOWN, lambda evt, target=shortcut_ctrl: self.on_shortcut_capture_key(evt, target))
            form.Add(shortcut_ctrl, 1, wx.EXPAND)
            remember("shortcut_active_value", shortcut_ctrl)

        button_label(
            self.t("reset_settings_for_section", section=self.settings_section_label(section_name)),
            lambda name=section_name: self.reset_settings_section(name),
        )
        self.settings_scroller.SetSizer(form, True)
        self.settings_scroller.Layout()
        self.settings_scroller.FitInside()
        self.panel.Layout()
        self.apply_settings_tab_order()
        try:
            self.settings_scroller.Thaw()
        except RuntimeError:
            pass


    def render_settings_section_and_focus(self, focus_key: str | None = None) -> None:
        self.render_settings_section()
        focus = self.controls.get(focus_key or "") if hasattr(self, "controls") else None
        if focus is None and self.settings_control_order:
            focus = self.settings_control_order[0]
        if focus is not None:
            self.focus_later(focus)


    def add_equalizer_profile_from_settings(self) -> None:
        preset_id = self.create_equalizer_profile_dialog(self.visible_equalizer_gains() or default_equalizer_gains())
        if not preset_id:
            return
        self.settings.global_equalizer_preset = preset_id
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_preset")


    def import_equalizer_profile_from_settings(self) -> None:
        preset_id = self.import_equalizer_profile_dialog()
        if not preset_id:
            return
        self.visible_equalizer_preset = preset_id
        if self.player_is_active() and self.session_equalizer_enabled is None:
            self.schedule_equalizer_apply(30)
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_preset")


    def export_visible_equalizer_profile_from_settings(self) -> None:
        preset_id = self.normalized_equalizer_preset(getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT)))
        self.save_visible_equalizer_gains_to_preset(preset_id)
        name = self.equalizer_preset_label(preset_id)
        self.export_equalizer_profile_dialog(name, self.visible_equalizer_gains() or self.equalizer_gains_for_preset(preset_id), preset_id)


    def delete_visible_equalizer_profile_from_settings(self) -> None:
        preset_id = self.normalized_equalizer_preset(getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT)))
        replacement = self.delete_equalizer_profile(preset_id)
        if not replacement:
            return
        self.visible_equalizer_preset = replacement
        if self.player_is_active() and self.session_equalizer_enabled is None:
            self.schedule_equalizer_apply(30)
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_preset")



    def on_equalizer_settings_preset_changed(self, _event: wx.CommandEvent) -> None:
        previous = getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        self.save_visible_equalizer_gains_to_preset(previous)
        preset = self.selected_choice_value("equalizer_preset")
        self.settings.global_equalizer_preset = self.normalized_equalizer_preset(preset)
        if self.player_is_active():
            self.use_global_equalizer_for_live_preview()
            self.schedule_equalizer_apply(30)
        wx.CallAfter(self.render_settings_section_and_focus, "equalizer_preset")


    def on_equalizer_settings_name_changed(self, event: wx.FocusEvent) -> None:
        preset = self.normalized_equalizer_preset(getattr(self, "visible_equalizer_preset", getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT)))
        ctrl = self.controls.get("equalizer_preset_name") if hasattr(self, "controls") else None
        if self.is_custom_equalizer_preset(preset) and isinstance(ctrl, wx.TextCtrl):
            names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
            names[preset] = ctrl.GetValue().strip()[:80] or self.equalizer_custom_name(preset)
            self.settings.equalizer_custom_names = names
            self.save_visible_equalizer_gains_to_preset(preset)
            preset_ctrl = self.controls.get("equalizer_preset") if hasattr(self, "controls") else None
            if isinstance(preset_ctrl, wx.Choice):
                options = self.equalizer_preset_options()
                if preset in options:
                    preset_ctrl.SetString(options.index(preset), self.equalizer_preset_label(preset))
        event.Skip()


    def on_equalizer_settings_slider(self, event: wx.CommandEvent, label: str) -> None:
        if getattr(self, "equalizer_controls_loading", False):
            event.Skip()
            return
        ctrl = event.GetEventObject()
        if isinstance(ctrl, wx.Slider):
            if getattr(ctrl, "_apricot_eq_programmatic_update", False):
                event.Skip()
                return
            if not self.update_visible_equalizer_draft_from_slider(ctrl):
                return
            self.set_equalizer_slider_accessibility(ctrl, label)
        self.save_visible_equalizer_gains_to_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
        if self.player_is_active():
            self.settings.global_equalizer_enabled = True
            preset = self.normalized_equalizer_preset(getattr(self, "visible_equalizer_preset", EQ_PRESET_FLAT))
            if self.is_custom_equalizer_preset(preset):
                self.use_global_equalizer_for_live_preview()
            else:
                self.use_visible_equalizer_for_live_preview()
            self.schedule_equalizer_apply()


    def restore_default_settings(self) -> None:
        self.settings = Settings()
        self.cookie_repair_suppressed_until = 0.0
        try:
            if CACHED_COOKIES_FILE.exists():
                CACHED_COOKIES_FILE.unlink()
        except OSError:
            pass
        self.save_settings()
        self.sync_windows_startup_registration(show_error=True)
        self.configure_subscription_timer()
        self.configure_rss_timer()
        self.configure_app_update_timer()
        self.set_status(self.t("defaults_restored"))
        self.speak_text(self.t("defaults_restored"))
        self.show_settings()


    def reset_settings_section(self, section_name: str) -> None:
        section_fields = self.settings_section_fields().get(section_name, [])
        if not section_fields:
            return
        defaults = asdict(Settings())
        for key in section_fields:
            if key in defaults:
                setattr(self.settings, key, defaults[key])
        if section_name == "cookies":
            self.cookie_repair_suppressed_until = 0.0
            try:
                if CACHED_COOKIES_FILE.exists():
                    CACHED_COOKIES_FILE.unlink()
            except OSError:
                pass
        self.save_settings()
        self.sync_windows_startup_registration(show_error=True)
        self.configure_subscription_timer()
        self.configure_rss_timer()
        self.configure_app_update_timer()
        if section_name == "shortcuts":
            self.shortcut_editor_values = dict(DEFAULT_KEYBOARD_SHORTCUTS)
        if section_name == "equalizer":
            self.visible_equalizer_preset = EQ_PRESET_FLAT
            if self.player_is_active() and self.session_equalizer_enabled is None:
                self.apply_equalizer_to_player()
        text = self.t("section_settings_reset", section=self.settings_section_label(section_name))
        self.set_status(text)
        self.speak_text(text)
        self.render_settings_section_and_focus()

    def export_browser_cookies_from_settings(self) -> None:
        if get_yt_dlp() is None:
            self.message(self.t("missing_ytdlp"), wx.ICON_ERROR)
            return
        self.apply_settings_from_visible_controls()
        browser = self.normalized_cookies_browser()
        if not browser:
            self.message(self.t("select_cookies_browser"))
            return
        if self.cookie_browser_is_running(browser):
            label = browser.title()
            with wx.MessageDialog(
                self,
                self.t("close_browser_for_cookie_export_message", browser=label),
                self.t("close_browser_for_cookie_export_title"),
                wx.YES_NO | wx.ICON_WARNING,
            ) as dialog:
                if dialog.ShowModal() != wx.ID_YES:
                    return
            if self.close_cookie_browser_processes(browser):
                self.announce_player(self.t("browser_closed_for_cookie_export"))
            self.wait_for_cookie_browser_exit(browser)
        self.announce_player(self.t("exporting_browser_cookies"))
        threading.Thread(target=self.export_browser_cookies_worker, args=(browser,), daemon=True).start()


    def open_youtube_login_profile_from_settings(self) -> None:
        self.apply_settings_from_visible_controls()
        browser = self.normalized_cookies_browser()
        if not browser:
            self.message(self.t("select_cookies_browser"))
            return
        try:
            if browser in CHROMIUM_COOKIE_BROWSERS:
                executable = self.cookie_browser_executable(browser)
                if not executable:
                    raise RuntimeError(f"{browser} executable not found")
                profile = str(getattr(self.settings, "cookies_browser_profile", COOKIE_PROFILE_AUTO) or COOKIE_PROFILE_AUTO)
                profile_dir = ""
                if profile and profile != COOKIE_PROFILE_AUTO:
                    if os.path.isabs(profile):
                        profile_path = Path(profile)
                        profile_dir = profile_path.name
                    else:
                        profile_dir = profile
                args = [executable]
                if profile_dir and browser != "opera":
                    args.append(f"--profile-directory={profile_dir}")
                args.append("https://www.youtube.com/")
                subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            else:
                import_module("webbrowser").open("https://www.youtube.com/")
            self.announce_player(self.t("youtube_profile_opened"))
        except Exception as exc:
            self.message(self.t("youtube_profile_open_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)


    def open_youtube_api_key_page_from_settings(self) -> None:
        self.apply_settings_from_visible_controls()
        try:
            import_module("webbrowser").open(YOUTUBE_API_CREDENTIALS_URL)
            self.announce_player(self.t("youtube_api_key_page_opened"))
        except Exception as exc:
            self.message(self.t("youtube_api_key_page_open_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def save_settings_from_ui(self) -> None:
        old_language = self.settings.language
        if not self.validate_shortcut_controls():
            return
        self.apply_settings_from_visible_controls()
        self.save_settings()
        self.sync_windows_startup_registration(show_error=True)
        self.trim_history()
        self.configure_subscription_timer()
        self.configure_rss_timer()
        self.configure_app_update_timer()
        self.install_download_accelerators()
        if self.player_is_active() and self.session_equalizer_enabled is None:
            self.apply_equalizer_to_player()
        saved_text = self.t("settings_saved")
        self.set_status(saved_text)
        self.speak_text(saved_text)
        if self.settings.language != old_language:
            self.show_settings()


    def apply_settings_from_visible_controls(self) -> None:
        c = self.controls
        if "language" in c:
            selected = c["language"].GetSelection()
            self.settings.language = LANGUAGE_CODES[selected] if 0 <= selected < len(LANGUAGE_CODES) else "en"
        if "download_folder" in c:
            self.settings.download_folder = c["download_folder"].GetValue()
        if "results_limit" in c:
            self.settings.results_limit = self.to_int(self.selected_choice_value("results_limit"), 0, 0, 250)
        if "direct_link_enter_action" in c:
            self.settings.direct_link_enter_action = self.normalize_direct_link_enter_action(self.selected_choice_value("direct_link_enter_action"))
        if "show_shortcuts_in_labels" in c:
            self.settings.show_shortcuts_in_labels = c["show_shortcuts_in_labels"].GetValue()
        if "seek_seconds" in c:
            self.settings.seek_seconds = self.to_float(self.selected_choice_value("seek_seconds"), 5.0, 0.1, 600.0)
        if "volume_step" in c:
            self.settings.volume_step = self.to_int(c["volume_step"].GetStringSelection(), 5, 1)
        boost_by_default = bool(c["volume_boost_by_default"].GetValue()) if "volume_boost_by_default" in c else bool(getattr(self.settings, "volume_boost_by_default", False))
        if "default_volume" in c:
            self.settings.default_volume = self.to_int(
                str(c["default_volume"].GetValue()),
                100,
                0,
                self.default_volume_max_for_boost(boost_by_default),
            )
        if "volume_boost_by_default" in c:
            self.settings.volume_boost_by_default = boost_by_default
        if "speed_step" in c:
            self.settings.speed_step = self.to_float(c["speed_step"].GetStringSelection(), 0.01, 0.01, 0.25)
        if "pitch_step" in c:
            self.settings.pitch_step = self.to_float(c["pitch_step"].GetStringSelection(), 0.01, 0.01, 0.25)
        if "auto_update" in c:
            self.settings.auto_update_ytdlp = c["auto_update"].GetValue()
        if "auto_update_app" in c:
            self.settings.auto_update_app = c["auto_update_app"].GetValue()
        if "update_channel" in c:
            self.settings.update_channel = self.selected_choice_value("update_channel") or "beta"
        if "app_update_interval" in c:
            self.settings.app_update_interval_hours = self.to_float(self.selected_choice_value("app_update_interval"), 6.0, 0.5, 24.0)
        if "close_to_tray" in c:
            self.settings.close_to_tray = c["close_to_tray"].GetValue()
        if "start_with_windows" in c:
            self.settings.start_with_windows = c["start_with_windows"].GetValue()
        if "tray_notification" in c:
            self.settings.tray_notification = c["tray_notification"].GetValue()
        if "autoplay_next" in c:
            self.settings.autoplay_next = c["autoplay_next"].GetValue()
        if "autoplay_related" in c:
            self.settings.autoplay_related = c["autoplay_related"].GetValue()
        if "confirm_download" in c:
            self.settings.confirm_before_download = c["confirm_download"].GetValue()
        if "open_after_download" in c:
            self.settings.open_folder_after_download = c["open_after_download"].GetValue()
        if "download_complete_popup" in c:
            self.settings.popup_when_download_complete = c["download_complete_popup"].GetValue()
        if "conversion_complete_popup" in c:
            self.settings.popup_when_conversion_complete = c["conversion_complete_popup"].GetValue()
        if "ask_download_location_each_time" in c:
            self.settings.ask_download_location_each_time = c["ask_download_location_each_time"].GetValue()
        if "audio_format" in c:
            self.settings.audio_format = c["audio_format"].GetStringSelection() or "mp3"
        if "audio_quality" in c:
            self.settings.audio_quality = self.normalize_audio_quality_value(self.selected_choice_value("audio_quality"))
        if "video_format" in c:
            self.settings.video_format = self.normalize_video_format_value(self.selected_choice_value("video_format"))
        if "max_height" in c:
            self.settings.max_video_height = self.to_int(c["max_height"].GetStringSelection(), 1080, 0)
        if "filename_template" in c:
            self.settings.filename_template = c["filename_template"].GetValue() or DEFAULT_FILENAME_TEMPLATE
        if "subtitle_langs" in c:
            self.settings.subtitle_languages = c["subtitle_langs"].GetValue() or "sl,en"
        if "quiet_downloads" in c:
            self.settings.quiet_downloads = c["quiet_downloads"].GetValue()
        if "playlist_order" in c:
            self.settings.keep_playlist_order = c["playlist_order"].GetValue()
        if "write_thumbnail" in c:
            self.settings.write_thumbnail = c["write_thumbnail"].GetValue()
        if "write_description" in c:
            self.settings.write_description = c["write_description"].GetValue()
        if "write_info_json" in c:
            self.settings.write_info_json = c["write_info_json"].GetValue()
        if "write_subtitles" in c:
            self.settings.write_subtitles = c["write_subtitles"].GetValue()
        if "auto_subtitles" in c:
            self.settings.auto_subtitles = c["auto_subtitles"].GetValue()
        if "embed_metadata" in c:
            self.settings.embed_metadata = c["embed_metadata"].GetValue()
        if "embed_thumbnail" in c:
            self.settings.embed_thumbnail = c["embed_thumbnail"].GetValue()
        if "restrict_filenames" in c:
            self.settings.restrict_filenames = c["restrict_filenames"].GetValue()
        if "download_archive" in c:
            self.settings.download_archive = c["download_archive"].GetValue()
        if "player_speed" in c:
            self.settings.player_speed = c["player_speed"].GetStringSelection() or "1.0"
        if "speed_audio_mode" in c:
            self.settings.speed_audio_mode = self.normalize_speed_audio_mode_value(self.selected_choice_value("speed_audio_mode"))
        if "pitch_mode" in c:
            self.settings.pitch_mode = self.normalize_pitch_mode_value(self.selected_choice_value("pitch_mode"))
        if "global_equalizer" in c:
            self.settings.global_equalizer_enabled = c["global_equalizer"].GetValue()
        if "equalizer_clipping_protection" in c:
            self.settings.equalizer_clipping_protection = bool(c["equalizer_clipping_protection"].GetValue())
        selected_equalizer_preset = self.normalized_equalizer_preset(getattr(self.settings, "global_equalizer_preset", EQ_PRESET_FLAT))
        if "equalizer_preset" in c:
            selected_equalizer_preset = self.normalized_equalizer_preset(self.selected_choice_value("equalizer_preset"))
            self.settings.global_equalizer_preset = selected_equalizer_preset
        if "equalizer_preset_name" in c and self.is_custom_equalizer_preset(selected_equalizer_preset):
            names = self.normalized_equalizer_custom_names(getattr(self.settings, "equalizer_custom_names", {}) or {})
            names[selected_equalizer_preset] = c["equalizer_preset_name"].GetValue().strip()[:80] or self.equalizer_custom_name(selected_equalizer_preset)
            self.settings.equalizer_custom_names = names
        if "equalizer_db_range" in c:
            self.settings.equalizer_db_range = self.to_int(self.selected_choice_value("equalizer_db_range"), 12, 6, 24)
        eq_gains: dict[str, float] = self.visible_equalizer_gains() if any(f"eq_{band_id}" in c for band_id, _band_label in EQ_BANDS) else {}
        if eq_gains:
            eq_gains = self.normalized_equalizer_gains(eq_gains)
            if self.is_custom_equalizer_preset(selected_equalizer_preset):
                presets = self.normalized_equalizer_preset_gains(getattr(self.settings, "equalizer_preset_gains", {}) or {})
                presets[selected_equalizer_preset] = eq_gains
                self.settings.equalizer_preset_gains = presets
            self.settings.global_equalizer_gains = eq_gains
        if "show_video_details_by_default" in c:
            self.settings.show_video_details_by_default = c["show_video_details_by_default"].GetValue()
        if "enable_age_restricted_videos" in c:
            self.settings.enable_age_restricted_videos = c["enable_age_restricted_videos"].GetValue()
        if "enable_stream_cache" in c:
            self.settings.enable_stream_cache = c["enable_stream_cache"].GetValue()
        if "enable_stream_url_cache" in c:
            self.settings.enable_stream_url_cache = c["enable_stream_url_cache"].GetValue()
        if "stream_url_cache_minutes" in c:
            self.settings.stream_url_cache_minutes = self.normalized_stream_url_cache_minutes(self.selected_choice_value("stream_url_cache_minutes"))
        if "prefetch_next_stream_url" in c:
            self.settings.prefetch_next_stream_url = c["prefetch_next_stream_url"].GetValue()
        if "gapless_playback" in c:
            self.settings.gapless_playback = c["gapless_playback"].GetValue()
        if "replaygain_mode" in c:
            self.settings.replaygain_mode = self.normalized_replaygain_mode(self.selected_choice_value("replaygain_mode"))
        if "enable_online_lyrics" in c:
            self.settings.enable_online_lyrics = c["enable_online_lyrics"].GetValue()
        if "cache_folder" in c:
            self.settings.cache_folder = c["cache_folder"].GetValue().strip() or str(DEFAULT_CACHE_DIR)
        if "cache_size_mb" in c:
            self.settings.cache_size_mb = self.to_int(c["cache_size_mb"].GetStringSelection(), 512, 128, 4096)
        if "resume_playback" in c:
            self.settings.resume_playback = c["resume_playback"].GetValue()
        if "default_audio_device" in c:
            self.settings.audio_output_device = self.selected_choice_value("default_audio_device") or "auto"
        if "browser_playback" in c:
            self.settings.prefer_browser_playback = c["browser_playback"].GetValue()
        if "fullscreen" in c:
            self.settings.player_fullscreen = c["fullscreen"].GetValue()
        if "start_paused" in c:
            self.settings.player_start_paused = c["start_paused"].GetValue()
        if "announce_play_pause" in c:
            self.settings.announce_play_pause = c["announce_play_pause"].GetValue()
        if "announce_playback_finished" in c:
            self.settings.announce_playback_finished = c["announce_playback_finished"].GetValue()
        if "enable_background_playback" in c:
            self.settings.enable_background_playback = c["enable_background_playback"].GetValue()
        if "rate_limit" in c:
            self.settings.rate_limit = c["rate_limit"].GetValue()
        if "proxy" in c:
            self.settings.proxy = c["proxy"].GetValue()
        if "youtube_data_api_key" in c:
            self.settings.youtube_data_api_key = c["youtube_data_api_key"].GetValue().strip()
        if "cookies" in c:
            self.settings.cookies_file = c["cookies"].GetValue()
        if "cookies_from_browser" in c:
            self.settings.cookies_from_browser = c["cookies_from_browser"].GetStringSelection() or "none"
        if "cookies_browser_profile" in c:
            self.settings.cookies_browser_profile = self.selected_choice_value("cookies_browser_profile") or COOKIE_PROFILE_AUTO
        if "show_advanced_network_settings" in c:
            self.settings.show_advanced_network_settings = c["show_advanced_network_settings"].GetValue()
        if "cookie_user_agent" in c:
            self.settings.cookie_user_agent = c["cookie_user_agent"].GetValue().strip()
        if "ffmpeg" in c:
            self.settings.ffmpeg_location = c["ffmpeg"].GetValue()
        if "fragments" in c:
            self.settings.concurrent_fragments = self.to_int(c["fragments"].GetStringSelection(), 4, 1)
        if "retries" in c:
            self.settings.retries = self.to_int(c["retries"].GetStringSelection(), 10, 0)
        if "timeout" in c:
            self.settings.socket_timeout = self.to_int(c["timeout"].GetStringSelection(), 20, 1)
        if "history_limit" in c:
            self.settings.history_limit = self.to_int(c["history_limit"].GetStringSelection(), 500, 100, 5000)
        if "enable_trending" in c:
            self.settings.enable_trending = c["enable_trending"].GetValue()
        if "enable_history" in c:
            self.settings.enable_history = c["enable_history"].GetValue()
        if "enable_podcasts_rss" in c:
            self.settings.enable_podcasts_rss = c["enable_podcasts_rss"].GetValue()
        if "podcast_search_provider" in c:
            self.settings.podcast_search_provider = self.selected_choice_value("podcast_search_provider") or PODCAST_DIRECTORY_PROVIDER_APPLE
        if "podcast_search_country" in c:
            self.settings.podcast_search_country = c["podcast_search_country"].GetStringSelection() or "US"
        if "podcast_search_limit" in c:
            self.settings.podcast_search_limit = self.to_int(c["podcast_search_limit"].GetStringSelection(), 20, 1, 200)
        if "rss_max_items" in c:
            self.settings.rss_max_items = self.to_int(c["rss_max_items"].GetStringSelection(), 100, 1, 500)
        if "rss_refresh_on_startup" in c:
            self.settings.rss_refresh_on_startup = c["rss_refresh_on_startup"].GetValue()
        if "rss_auto_refresh_enabled" in c:
            self.settings.rss_auto_refresh_enabled = c["rss_auto_refresh_enabled"].GetValue()
        if "rss_refresh_interval" in c:
            self.settings.rss_refresh_interval_hours = self.to_float(self.selected_choice_value("rss_refresh_interval"), 12.0, 0.5, 168.0)
        if "subscription_check_enabled" in c:
            self.settings.subscription_check_enabled = c["subscription_check_enabled"].GetValue()
        if "subscription_check_interval" in c:
            self.settings.subscription_check_interval_hours = self.to_float(self.selected_choice_value("subscription_check_interval"), 6.0, 0.5, 168.0)
        if "windows_notifications" in c:
            self.settings.windows_notifications = c["windows_notifications"].GetValue()
        if "download_notifications" in c:
            self.settings.download_notifications = c["download_notifications"].GetValue()
        if "subscription_notifications" in c:
            self.settings.subscription_notifications = c["subscription_notifications"].GetValue()
        if "app_update_notifications" in c:
            self.settings.app_update_notifications = c["app_update_notifications"].GetValue()
        shortcuts = dict(getattr(self.settings, "keyboard_shortcuts", {}) or {})
        if "shortcut_action_list" in c and "shortcut_active_value" in c:
            self.sync_shortcut_editor_value()
            shortcuts.update(self.shortcut_editor_values)
        else:
            for action, _label_key in SHORTCUT_DEFINITIONS:
                control_key = f"shortcut_{action}"
                if control_key in c:
                    shortcuts[action] = c[control_key].GetValue().strip() or DEFAULT_KEYBOARD_SHORTCUTS[action]
        self.settings.keyboard_shortcuts = self.normalized_keyboard_shortcuts(shortcuts)


    def selected_choice_value(self, key: str) -> str:
        ctrl = self.controls.get(key) if hasattr(self, "controls") else None
        if not isinstance(ctrl, wx.Choice):
            return ""
        values = getattr(self, "choice_values", {}).get(key)
        selection = ctrl.GetSelection()
        if values and 0 <= selection < len(values):
            return values[selection]
        return ctrl.GetStringSelection()

