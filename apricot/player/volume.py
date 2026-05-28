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

class VolumeMixin:
    def session_volume_max(self) -> int:
        if self.volume_boost_enabled or getattr(self.settings, "volume_boost_by_default", False):
            return BOOSTED_VOLUME_MAX
        try:
            if self.session_volume is not None and float(self.session_volume) > NORMAL_VOLUME_MAX:
                return BOOSTED_VOLUME_MAX
        except (TypeError, ValueError):
            pass
        return NORMAL_VOLUME_MAX


    def clamp_session_volume(self, value) -> float:
        try:
            numeric = float(value)
        except (TypeError, ValueError):
            numeric = float(self.default_volume_value())
        return max(0.0, min(float(self.session_volume_max()), numeric))


    def consume_pending_volume_target(self) -> float | None:
        with self.volume_change_lock:
            pending_target = self.volume_change_pending_target
            if pending_target is None:
                return None
            self.volume_change_pending_target = None
            timer = self.volume_change_timer
            if timer is not None and timer.IsRunning():
                timer.Stop()
            self.volume_change_timer = None
        self.session_volume = self.clamp_session_volume(pending_target)
        return self.session_volume


    def remember_current_player_volume(self) -> None:
        if self.consume_pending_volume_target() is not None:
            return
        if self.session_volume is not None:
            self.session_volume = self.clamp_session_volume(self.session_volume)
            return
        if self.player_kind != "mpv" or not self.mpv_process_alive():
            return
        try:
            current = self.mpv_get_property("volume", timeout=0.3)
            if current is not None:
                self.session_volume = self.clamp_session_volume(current)
        except Exception:
            pass


    def current_player_volume(self) -> float:
        self.remember_current_player_volume()
        if self.session_volume is not None:
            return self.clamp_session_volume(self.session_volume)
        return float(self.default_volume_value())


    def cancel_pending_volume_change(self) -> None:
        with self.volume_change_lock:
            self.volume_change_pending_target = None
            timer = self.volume_change_timer
            if timer is not None and timer.IsRunning():
                timer.Stop()
            self.volume_change_timer = None


    def current_player_volume_max(self) -> int:
        boosted = bool(self.volume_boost_enabled)
        return BOOSTED_VOLUME_MAX if boosted else NORMAL_VOLUME_MAX


    def configured_player_start_volume_max(self) -> int:
        boosted = bool(self.volume_boost_enabled or getattr(self.settings, "volume_boost_by_default", False))
        return BOOSTED_VOLUME_MAX if boosted else NORMAL_VOLUME_MAX


    def player_start_volume_value(self) -> float:
        if self.session_volume is not None:
            return self.clamp_session_volume(self.session_volume)
        return float(self.default_volume_value())

    def apply_initial_volume_worker(self, generation: int, target_volume: float, volume_max: int) -> None:
        deadline = time.monotonic() + 2.0
        while time.monotonic() < deadline:
            if generation != self.player_generation or not self.mpv_process_alive():
                return
            try:
                self.mpv_set_property("volume-max", volume_max, timeout=0.4)
                self.mpv_set_property("volume", target_volume, timeout=0.4)
                return
            except Exception:
                time.sleep(0.12)


    def apply_initial_audio_startup_worker(
        self,
        generation: int,
        target_volume: float,
        volume_max: int,
        release_pause: bool,
    ) -> None:
        deadline = time.monotonic() + 2.0
        startup_ready = False
        while time.monotonic() < deadline:
            if generation != self.player_generation or not self.mpv_process_alive():
                return
            try:
                self.mpv_set_property("volume-max", volume_max, timeout=0.25)
                self.mpv_set_property("volume", target_volume, timeout=0.25)
                startup_ready = True
                break
            except Exception:
                time.sleep(0.04)
        if generation != self.player_generation or not self.mpv_process_alive():
            return
        try:
            self.mpv_set_property("mute", False, timeout=0.25)
            if release_pause:
                self.mpv_set_property("pause", False, timeout=0.25)
                self.player_paused = False
                wx.CallAfter(self.update_play_pause_buttons)
            elif startup_ready:
                self.player_paused = True
                wx.CallAfter(self.update_play_pause_buttons)
        except Exception:
            pass



    def change_volume_async(self, delta: int) -> None:
        with self.volume_change_lock:
            maximum = float(self.current_player_volume_max())
            base = self.session_volume
            if base is None:
                base = self.default_volume_value()
            target = min(max(0.0, float(base) + float(delta)), maximum)
            self.session_volume = target
            self.volume_change_pending_target = target
            timer = self.volume_change_timer
            if timer is not None and timer.IsRunning():
                return
            self.volume_change_timer = wx.CallLater(45, self.apply_pending_volume_change_async)


    def apply_pending_volume_change_async(self) -> None:
        with self.volume_change_lock:
            target = self.volume_change_pending_target
            self.volume_change_pending_target = None
            self.volume_change_timer = None
        if target is None:
            return
        generation = self.player_generation
        threading.Thread(target=self.change_volume_worker, args=(target, generation), daemon=True).start()


    def change_volume_worker(self, target: float, generation: int | None = None) -> None:
        try:
            if generation is not None and generation != self.player_generation:
                return
            if self.player_kind != "mpv" or not self.mpv_process_alive():
                return
            maximum = float(self.current_player_volume_max())
            volume = min(max(0.0, float(target)), maximum)
            self.mpv_set_property("volume-max", maximum)
            if generation is not None and generation != self.player_generation:
                return
            self.mpv_set_property("volume", volume)
            if generation is None or generation == self.player_generation:
                self.session_volume = volume
        except Exception:
            pass


    def announce_volume_async(self) -> None:
        threading.Thread(target=self.announce_volume_worker, daemon=True).start()


    def announce_volume_worker(self) -> None:
        try:
            current = self.mpv_get_property("volume", timeout=0.5)
            if current is None:
                raise RuntimeError("volume unavailable")
            volume = int(round(float(current)))
            wx.CallAfter(self.announce_player, self.t("volume_announcement", volume=volume))
        except Exception:
            wx.CallAfter(self.announce_player, self.t("timing_unavailable"))


    def toggle_volume_boost(self) -> None:
        self.volume_boost_enabled = not self.volume_boost_enabled
        generation = self.player_generation
        if self.volume_boost_enabled:
            threading.Thread(target=self.enable_volume_boost_worker, args=(generation,), daemon=True).start()
            self.announce_player(self.t("volume_boost_on"))
        else:
            threading.Thread(target=self.disable_volume_boost_worker, args=(generation,), daemon=True).start()


    def enable_volume_boost_worker(self, generation: int | None = None) -> None:
        try:
            if generation is not None and generation != self.player_generation:
                return
            if self.player_kind != "mpv" or not self.mpv_process_alive():
                return
            self.mpv_set_property("volume-max", BOOSTED_VOLUME_MAX)
        except Exception:
            pass
        wx.CallAfter(self.schedule_equalizer_apply, 40)


    def disable_volume_boost_worker(self, generation: int | None = None) -> None:
        try:
            if generation is not None and generation != self.player_generation:
                return
            if self.player_kind != "mpv" or not self.mpv_process_alive():
                return
            current = self.mpv_get_property("volume")
            if generation is not None and generation != self.player_generation:
                return
            if current is not None and float(current) > 100.0:
                self.mpv_set_property("volume", 100.0)
                self.session_volume = 100.0
            elif current is not None:
                self.session_volume = max(0.0, min(100.0, float(current)))
            if generation is not None and generation != self.player_generation:
                return
            self.mpv_set_property("volume-max", NORMAL_VOLUME_MAX)
        except Exception:
            pass
        wx.CallAfter(self.schedule_equalizer_apply, 40)
        wx.CallAfter(self.announce_player, self.t("volume_boost_off"))



    @staticmethod
    def default_volume_max_for_boost(boost_enabled: bool) -> int:
        return BOOSTED_VOLUME_MAX if boost_enabled else NORMAL_VOLUME_MAX


    def default_volume_max_value(self) -> int:
        return self.default_volume_max_for_boost(bool(getattr(self.settings, "volume_boost_by_default", False)))


    def default_volume_value(self) -> int:
        return self.to_int(str(getattr(self.settings, "default_volume", 100)), 100, 0, self.default_volume_max_value())


    def on_volume_boost_by_default_settings_changed(self, event: wx.CommandEvent) -> None:
        enabled = bool(event.IsChecked())
        self.settings.volume_boost_by_default = enabled
        slider = self.controls.get("default_volume") if hasattr(self, "controls") else None
        if isinstance(slider, wx.Slider):
            maximum = self.default_volume_max_for_boost(enabled)
            value = min(max(0, int(slider.GetValue())), maximum)
            slider.SetRange(0, maximum)
            if slider.GetValue() != value:
                slider.SetValue(value)
            self.set_integer_slider_accessibility(slider, self.t("default_volume"), "percent")
        event.Skip()


