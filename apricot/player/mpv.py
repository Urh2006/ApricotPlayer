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

class MpvMixin:

    def speed_uses_mpv_auto_pitch_correction(self) -> bool:
        return self.normalized_speed_audio_mode() in {SPEED_AUDIO_MODE_MPV, SPEED_AUDIO_MODE_RUBBERBAND}



    def start_mpv(
        self,
        command: str,
        stream_url: str,
        title: str,
        headers: dict,
        announce_start: bool = False,
        request_generation: int = 0,
    ) -> None:
        if request_generation and not self.playback_request_is_current(request_generation):
            return
        self.playback_start_pending = False
        try:
            self.ipc_path = self.make_ipc_path()
            target_volume = self.player_start_volume_value()
            boost_volume = bool(self.volume_boost_enabled or getattr(self.settings, "volume_boost_by_default", False) or target_volume > NORMAL_VOLUME_MAX)
            volume_max = BOOSTED_VOLUME_MAX if boost_volume else NORMAL_VOLUME_MAX
            target_volume = max(0.0, min(float(volume_max), target_volume))
            target_speed = self.player_start_speed_value()
            self.session_volume = target_volume
            embed_player = False
            hwnd = 0
            try:
                panel = self.live_window(getattr(self, "player_panel", None))
                embed_player = bool(panel is not None)
                if embed_player and panel is not None:
                    panel.Update()
                    hwnd = panel.GetHandle()
            except Exception:
                embed_player = False
            args = [
                command,
                "--no-config",
                "--force-window=yes" if embed_player else "--force-window=no",
                f"--input-ipc-server={self.ipc_path}",
                "--idle=no",
                "--keep-open=yes",
                f"--volume-max={volume_max}",
                f"--volume={target_volume:g}",
                "--pitch=1.0",
                f"--speed={target_speed:g}",
                f"--loop-file={'inf' if self.repeat_current else 'no'}",
                f"--gapless-audio={'yes' if bool(getattr(self.settings, 'gapless_playback', True)) else 'no'}",
                f"--replaygain={self.normalized_replaygain_mode()}",
                "--replaygain-clip=yes",
                "--term-playing-msg=",
                "--msg-level=all=warn",
            ]
            initial_eq_filter = ""
            initial_eq_enabled, initial_eq_gains = self.effective_equalizer_state()
            if initial_eq_enabled and any(abs(float(value)) >= 0.05 for value in initial_eq_gains.values()):
                initial_eq_filter = self.equalizer_filter(
                    initial_eq_gains,
                    self.equalizer_clipping_protection_active(initial_eq_gains),
                    EQ_FILTER_LABEL,
                )
                args.append(f"--af={initial_eq_filter}")
            if embed_player and hwnd:
                args.insert(2, f"--wid={hwnd}")
            elif urlparse(str(stream_url)).scheme in {"http", "https"}:
                args.append("--vid=no")
            args.extend(self.speed_audio_filter_args())
            if getattr(self.settings, "enable_stream_cache", True):
                cache_folder = self.cache_folder_path()
                cache_folder.mkdir(parents=True, exist_ok=True)
                cache_size = max(128, min(4096, int(getattr(self.settings, "cache_size_mb", 512) or 512)))
                back_cache = max(32, min(cache_size, cache_size // 4))
                args.extend(
                    [
                        "--cache=yes",
                        "--cache-on-disk=yes",
                        f"--demuxer-cache-dir={cache_folder}",
                        f"--demuxer-max-bytes={cache_size}MiB",
                        f"--demuxer-max-back-bytes={back_cache}MiB",
                        "--cache-pause=no",
                        # Reconnect seekable streams after a network drop.
                        # reconnect_streamed is intentionally omitted: it causes
                        # ffmpeg to tear down and reconnect the HTTP connection on
                        # every seek, adding 2-5 s of stall after each seek command.
                        "--stream-lavf-o=reconnect=1,reconnect_on_network_error=1,reconnect_delay_max=5",
                    ]
                )
            else:
                args.append("--cache=no")
            audio_device = self.player_audio_output_device()
            if audio_device and audio_device.lower() != "auto":
                args.append(f"--audio-device={audio_device}")
            resume_position = self.playback_resume_position()
            if resume_position:
                args.append(f"--start={resume_position:.1f}")
            if headers.get("User-Agent"):
                args.append(f"--user-agent={headers['User-Agent']}")
            if headers.get("Referer"):
                args.append(f"--referrer={headers['Referer']}")
            for name, value in headers.items():
                if name.lower() not in {"user-agent", "referer"} and value:
                    args.append(f"--http-header-fields-append={name}: {value}")
            if self.player_fullscreen_mode_active():
                args.append("--fullscreen=yes")
            if self.settings.player_start_paused:
                args.append("--pause=yes")
            args.append(stream_url)
            log_file = APP_DIR / "mpv.log"
            if self.player_log_handle:
                self.player_log_handle.close()
            self.player_log_handle = log_file.open("w", encoding="utf-8", errors="replace")
            creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
            self.player_process = subprocess.Popen(
                args,
                cwd=str(Path(command).parent),
                stdout=self.player_log_handle,
                stderr=subprocess.STDOUT,
                creationflags=creationflags,
            )
            self.player_kind = "mpv"
            self.player_control_mode = True
            self.player_session_open = True
            self.player_ended = False
            self.player_paused = bool(self.settings.player_start_paused)
            self.player_generation += 1
            self.current_stream_url = stream_url
            self.current_stream_headers = dict(headers or {})
            self.current_audio_device = audio_device
            self.volume_boost_enabled = boost_volume
            self.rubberband_pitch_filter_active = False
            self.equalizer_filter_active = bool(initial_eq_filter)
            self.equalizer_filter_ref = EQ_FILTER_REF
            self.current_video_info["speed"] = self.format_playback_rate(target_speed)
            self.current_video_info["pitch"] = self.format_playback_rate(1.0)
            self.update_details_text()
            self.set_status(self.t("playing", title=title))
            if announce_start:
                self.announce_player(self.t("playing", title=title))
            wx.CallAfter(self.update_play_pause_buttons)
            threading.Thread(target=self.apply_initial_volume_worker, args=(self.player_generation, target_volume, volume_max), daemon=True).start()
            wx.CallLater(80, self.apply_equalizer_to_player, 6)
            wx.CallLater(700, self.apply_equalizer_to_player)
            self.start_player_monitor(self.player_generation)
        except Exception as exc:
            self.playback_start_pending = False
            if self.player_log_handle is not None:
                try:
                    self.player_log_handle.close()
                except Exception:
                    pass
                self.player_log_handle = None
            self.message(self.t("player_failed", error=exc), wx.ICON_ERROR)



    def mpv_process_alive(self) -> bool:
        return bool(self.player_process and self.player_process.poll() is None)


    def quiet_current_mpv_for_stop(self) -> None:
        if self.player_kind != "mpv" or not self.ipc_path or not self.mpv_process_alive():
            return
        commands = (
            ["set_property", "mute", True],
            ["set_property", "volume", 0],
            ["set_property", "pause", True],
        )
        payload = "".join(json.dumps({"command": command}) + "\n" for command in commands)
        try:
            with self.mpv_ipc_lock:
                with self.open_mpv_pipe("w", timeout=0.0, encoding="utf-8") as pipe:
                    pipe.write(payload)
        except Exception:
            pass


    def open_mpv_pipe(self, mode: str, timeout: float = MPV_IPC_TIMEOUT_SECONDS, buffering: int = -1, encoding: str | None = None):
        if self.player_kind != "mpv" or not self.ipc_path:
            raise RuntimeError("mpv is not running")
        deadline = time.monotonic() + max(0.0, timeout)
        last_error: Exception | None = None
        while True:
            if not self.mpv_process_alive():
                raise RuntimeError("mpv process is not running")
            try:
                if encoding is None:
                    return open(self.ipc_path, mode, buffering=buffering)
                return open(self.ipc_path, mode, buffering=buffering, encoding=encoding)
            except OSError as exc:
                last_error = exc
                if time.monotonic() >= deadline:
                    raise last_error
                time.sleep(0.04)


    def mpv_send(self, command: list, timeout: float = MPV_IPC_TIMEOUT_SECONDS) -> None:
        payload = json.dumps({"command": command}) + "\n"
        with self.mpv_ipc_lock:
            with self.open_mpv_pipe("w", timeout=timeout, encoding="utf-8") as pipe:
                pipe.write(payload)


    def mpv_request(self, command: list, timeout: float = MPV_IPC_TIMEOUT_SECONDS) -> dict:
        if self.player_kind != "mpv" or not self.ipc_path:
            return {}
        request_id = int(time.time() * 1000000)
        payload = (json.dumps({"command": command, "request_id": request_id}) + "\n").encode("utf-8")
        with self.mpv_ipc_lock:
            with self.open_mpv_pipe("r+b", timeout=timeout, buffering=0) as pipe:
                deadline = time.monotonic() + max(0.0, timeout)
                pipe.write(payload)
                while time.monotonic() < deadline:
                    raw = pipe.readline()
                    if not raw:
                        time.sleep(0.01)
                        continue
                    try:
                        response = json.loads(raw.decode("utf-8", errors="replace"))
                    except json.JSONDecodeError:
                        continue
                    if response.get("request_id") == request_id:
                        return response
        return {}


    def mpv_get_property(self, name: str, timeout: float = MPV_IPC_TIMEOUT_SECONDS):
        response = self.mpv_request(["get_property", name], timeout=timeout)
        return response.get("data")


    def mpv_set_property(self, name: str, value, timeout: float = MPV_IPC_TIMEOUT_SECONDS) -> None:
        response = self.mpv_request(["set_property", name, value], timeout=timeout)
        if not response or response.get("error") != "success":
            raise RuntimeError(str(response.get("error")))


