from __future__ import annotations

import os
import platform
import re
import sys
from datetime import datetime, timezone
from importlib import import_module
from pathlib import Path
from urllib.parse import urlparse, urlunparse

from apricot.constants import *

_RE_URL = re.compile(r"https?://[^\s\"'<>]+", re.IGNORECASE)


class DiagnosticsMixin:
    def copy_diagnostic_report(self) -> None:
        report = self.build_diagnostic_report()
        self.copy_plain_text_to_clipboard(report)
        self.announce_player(self.t("diagnostic_report_copied"))

    def build_diagnostic_report(self) -> str:
        sections = [
            self.diagnostic_app_section(),
            self.diagnostic_player_section(),
            self.diagnostic_audio_section(),
            self.diagnostic_current_item_section(),
            self.diagnostic_queue_section(),
            self.diagnostic_settings_section(),
            self.diagnostic_log_section("mpv.log", APP_DIR / "mpv.log"),
            self.diagnostic_log_section("updater.log", UPDATE_LOG_FILE),
        ]
        return "\n\n".join(section for section in sections if section).strip() + "\n"

    def diagnostic_app_section(self) -> str:
        lines = [
            "# ApricotPlayer diagnostic report",
            self.diagnostic_line("Generated", datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")),
            self.diagnostic_line("App version", APP_VERSION),
            self.diagnostic_line("App label", APP_VERSION_LABEL),
            self.diagnostic_line("Update channel", getattr(self.settings, "update_channel", "")),
            self.diagnostic_line("Frozen build", bool(getattr(sys, "frozen", False))),
            self.diagnostic_line("Executable", sys.executable),
            self.diagnostic_line("Working directory", os.getcwd()),
            self.diagnostic_line("Settings file", SETTINGS_FILE),
            self.diagnostic_line("App data folder", APP_DIR),
            self.diagnostic_line("Python", sys.version.replace("\n", " ")),
            self.diagnostic_line("Platform", platform.platform()),
            self.diagnostic_line("yt-dlp", self.diagnostic_ytdlp_version()),
            self.diagnostic_line("mpv path", self.diagnostic_player_path()),
            self.diagnostic_line("FFmpeg path", self.diagnostic_ffmpeg_path()),
        ]
        return "\n".join(lines)

    def diagnostic_player_section(self) -> str:
        active = self.player_is_active()
        lines = [
            "## Player",
            self.diagnostic_line("Active", active),
            self.diagnostic_line("Kind", getattr(self, "player_kind", "")),
            self.diagnostic_line("Control mode", getattr(self, "player_control_mode", False)),
            self.diagnostic_line("Session open", getattr(self, "player_session_open", False)),
            self.diagnostic_line("Playback pending", getattr(self, "playback_start_pending", False)),
            self.diagnostic_line("In player screen", getattr(self, "in_player_screen", False)),
            self.diagnostic_line("Return screen", getattr(self, "player_return_screen", "")),
            self.diagnostic_line("Fullscreen session", getattr(self, "player_fullscreen_session", False)),
            self.diagnostic_line("Paused", getattr(self, "player_paused", False)),
            self.diagnostic_line("Ended", getattr(self, "player_ended", False)),
            self.diagnostic_line("Process PID", getattr(getattr(self, "player_process", None), "pid", "")),
        ]
        if active:
            for prop in ("pause", "time-pos", "duration", "volume", "volume-max", "speed", "pitch", "audio-device", "audio-params"):
                lines.append(self.diagnostic_line(f"mpv {prop}", self.diagnostic_mpv_property(prop)))
        return "\n".join(lines)

    def diagnostic_audio_section(self) -> str:
        eq_enabled = getattr(self, "session_equalizer_enabled", None)
        if eq_enabled is None:
            eq_enabled = getattr(self.settings, "global_equalizer_enabled", False)
        lines = [
            "## Audio state",
            self.diagnostic_line("Default volume", getattr(self.settings, "default_volume", "")),
            self.diagnostic_line("Session volume", getattr(self, "session_volume", None)),
            self.diagnostic_line("Volume boost enabled", getattr(self, "volume_boost_enabled", False)),
            self.diagnostic_line("Volume boost by default", getattr(self.settings, "volume_boost_by_default", False)),
            self.diagnostic_line("Bass boost enabled", getattr(self, "bass_boost_enabled", False)),
            self.diagnostic_line("Equalizer enabled", eq_enabled),
            self.diagnostic_line("Equalizer range", getattr(self.settings, "equalizer_db_range", "")),
            self.diagnostic_line("Equalizer clipping protection", getattr(self.settings, "equalizer_clipping_protection", False)),
            self.diagnostic_line("Equalizer gains", self.diagnostic_equalizer_gains()),
            self.diagnostic_line("Configured output device", getattr(self.settings, "audio_output_device", "")),
            self.diagnostic_line("Session output device", getattr(self, "session_audio_output_device", "")),
            self.diagnostic_line("Current output device", getattr(self, "current_audio_device", "")),
            self.diagnostic_line("ReplayGain mode", getattr(self.settings, "replaygain_mode", "")),
            self.diagnostic_line("Gapless playback", getattr(self.settings, "gapless_playback", False)),
            self.diagnostic_line("Speed audio mode", getattr(self.settings, "speed_audio_mode", "")),
            self.diagnostic_line("Repeat", getattr(self, "repeat_current", False)),
            self.diagnostic_line("Shuffle", getattr(self, "shuffle_current", False)),
            self.diagnostic_line("Autoplay next setting", getattr(self.settings, "autoplay_next", False)),
            self.diagnostic_line("Autoplay next session", getattr(self, "session_autoplay_next", False)),
        ]
        return "\n".join(lines)

    def diagnostic_current_item_section(self) -> str:
        item = self.current_video_item or self.current_video_info or {}
        lines = [
            "## Current item",
            self.diagnostic_line("Title", item.get("title", "")),
            self.diagnostic_line("Kind", item.get("kind", "")),
            self.diagnostic_line("Type", item.get("type", "")),
            self.diagnostic_line("Channel", item.get("channel", "")),
            self.diagnostic_line("Duration", item.get("duration") or self.format_duration(item.get("duration_seconds"))),
            self.diagnostic_line("URL", item.get("webpage_url") or item.get("url", "")),
            self.diagnostic_line("Local path", item.get("path", "")),
            self.diagnostic_line("Stream URL", self.diagnostic_url_summary(getattr(self, "current_stream_url", ""))),
            self.diagnostic_line("Stream header names", ", ".join(sorted((getattr(self, "current_stream_headers", {}) or {}).keys())) or "none"),
        ]
        return "\n".join(lines)

    def diagnostic_queue_section(self) -> str:
        lines = [
            "## Results and queue",
            self.diagnostic_line("Current index", getattr(self, "current_index", "")),
            self.diagnostic_line("Return index", getattr(self, "return_index", "")),
            self.diagnostic_line("Visible results", len(getattr(self, "results", []) or [])),
            self.diagnostic_line("All results", len(getattr(self, "all_results", []) or [])),
            self.diagnostic_line("Return results", len(getattr(self, "return_results", []) or [])),
            self.diagnostic_line("Return all results", len(getattr(self, "return_all_results", []) or [])),
            self.diagnostic_line("Player sequence count", len(getattr(self, "player_sequence_results", []) or [])),
            self.diagnostic_line("Playback queue count", len(getattr(self, "playback_queue", []) or [])),
            self.diagnostic_line("Dynamic fetch enabled", getattr(self, "dynamic_fetch_enabled", False)),
            self.diagnostic_line("Loading more results", getattr(self, "loading_more_results", False)),
            self.diagnostic_line("Collection URL", getattr(self, "collection_url", "")),
            self.diagnostic_line("Collection fully loaded", getattr(self, "collection_fully_loaded", False)),
        ]
        return "\n".join(lines)

    def diagnostic_settings_section(self) -> str:
        lines = [
            "## Key settings",
            self.diagnostic_line("Language", getattr(self.settings, "language", "")),
            self.diagnostic_line("Results limit", getattr(self.settings, "results_limit", "")),
            self.diagnostic_line("Background playback", getattr(self.settings, "enable_background_playback", False)),
            self.diagnostic_line("Close to tray", getattr(self.settings, "close_to_tray", False)),
            self.diagnostic_line("Stream cache", getattr(self.settings, "enable_stream_cache", False)),
            self.diagnostic_line("Stream URL cache", getattr(self.settings, "enable_stream_url_cache", False)),
            self.diagnostic_line("Stream URL cache minutes", getattr(self.settings, "stream_url_cache_minutes", "")),
            self.diagnostic_line("Cache folder", self.cache_folder_path()),
            self.diagnostic_line("Cache size MB", getattr(self.settings, "cache_size_mb", "")),
            self.diagnostic_line("Cookies file configured", bool(getattr(self.settings, "cookies_file", ""))),
            self.diagnostic_line("Cookies browser", getattr(self.settings, "cookies_from_browser", "")),
            self.diagnostic_line("YouTube API key configured", bool(getattr(self.settings, "youtube_data_api_key", ""))),
            self.diagnostic_line("Player command configured", bool(getattr(self.settings, "player_command", ""))),
            self.diagnostic_line("FFmpeg configured", bool(getattr(self.settings, "ffmpeg_location", ""))),
            self.diagnostic_line("Auto update app", getattr(self.settings, "auto_update_app", False)),
            self.diagnostic_line("Auto update yt-dlp", getattr(self.settings, "auto_update_ytdlp", False)),
        ]
        return "\n".join(lines)

    def diagnostic_log_section(self, title: str, path: Path) -> str:
        tail = self.diagnostic_file_tail(path)
        if not tail:
            return f"## {title}\nnot available"
        return f"## {title}\n{tail}"

    def diagnostic_file_tail(self, path: Path, line_count: int = 50) -> str:
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except Exception:
            return ""
        lines = text.splitlines()[-line_count:]
        return self.diagnostic_redact_text("\n".join(lines)).strip()

    def diagnostic_ytdlp_version(self) -> str:
        try:
            return str(import_module("yt_dlp.version").__version__)
        except Exception:
            try:
                ytdlp = get_yt_dlp()
                return str(getattr(ytdlp, "__version__", "") or "unknown")
            except Exception as exc:
                return f"unavailable: {exc.__class__.__name__}"

    def diagnostic_player_path(self) -> str:
        try:
            player = self.resolve_player()
        except Exception as exc:
            return f"unavailable: {exc.__class__.__name__}"
        if not player:
            return "not found"
        return str(player[0])

    def diagnostic_ffmpeg_path(self) -> str:
        try:
            return str(self.ffmpeg_executable() or "not found")
        except Exception as exc:
            return f"unavailable: {exc.__class__.__name__}"

    def diagnostic_equalizer_gains(self) -> str:
        gains = getattr(self, "session_equalizer_gains", None) or getattr(self.settings, "global_equalizer_gains", {}) or {}
        parts = []
        for band_id, _label in EQ_BANDS:
            value = gains.get(band_id, 0.0)
            parts.append(f"{band_id} Hz={value}")
        return ", ".join(parts)

    def diagnostic_mpv_property(self, prop: str):
        try:
            return self.mpv_get_property(prop, timeout=0.12)
        except Exception as exc:
            return f"unavailable: {exc.__class__.__name__}"

    def diagnostic_line(self, label: str, value) -> str:
        return f"{label}: {self.diagnostic_format_value(value)}"

    def diagnostic_format_value(self, value) -> str:
        if isinstance(value, bool):
            return "yes" if value else "no"
        if value is None:
            return "none"
        if isinstance(value, Path):
            return str(value)
        if isinstance(value, float):
            return f"{value:.3f}".rstrip("0").rstrip(".")
        if isinstance(value, (dict, list, tuple)):
            text = repr(value)
        else:
            text = str(value)
        text = self.diagnostic_redact_text(text.replace("\r\n", "\n").replace("\r", "\n"))
        if len(text) > 1000:
            return text[:1000].rstrip() + "..."
        return text

    def diagnostic_url_summary(self, url: str) -> str:
        url = str(url or "").strip()
        if not url:
            return "none"
        try:
            parsed = urlparse(url)
        except Exception:
            return f"present, length={len(url)}"
        if parsed.scheme not in {"http", "https"}:
            return self.diagnostic_redact_text(url)
        path = parsed.path or "/"
        if len(path) > 100:
            path = path[:97] + "..."
        query_state = "yes" if parsed.query else "no"
        return f"{parsed.scheme}://{parsed.netloc}{path} (query={query_state}, length={len(url)})"

    def diagnostic_redact_text(self, text: str) -> str:
        return _RE_URL.sub(lambda match: self.diagnostic_redact_url(match.group(0)), str(text or ""))

    @staticmethod
    def diagnostic_redact_url(url: str) -> str:
        try:
            parsed = urlparse(url)
        except Exception:
            return url
        if not parsed.query:
            return url
        return urlunparse((parsed.scheme, parsed.netloc, parsed.path, parsed.params, "...", parsed.fragment))
