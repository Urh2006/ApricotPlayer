from __future__ import annotations
from apricot.models import Settings
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

class DataManagerMixin:

    def load_settings(self) -> Settings:
        backup_settings = SETTINGS_FILE.with_suffix(".json.bak")
        sources = [SETTINGS_FILE, backup_settings, LEGACY_SETTINGS_FILE]
        load_errors: list[str] = []
        for source in sources:
            if not source.exists():
                continue
            try:
                raw_text = source.read_text(encoding="utf-8")
                if not raw_text.strip():
                    raise ValueError("settings file is empty")
                raw_data = json.loads(raw_text)
                if not isinstance(raw_data, dict):
                    raise ValueError(f"settings file must contain an object, got {type(raw_data).__name__}")
                data = dict(raw_data)
                allowed_keys = {field.name for field in fields(Settings)}
                data = {key: value for key, value in data.items() if key in allowed_keys}
                if not data:
                    raise ValueError("settings file contains no recognized settings")
                merged = {**asdict(Settings()), **data}
                if merged.get("language") not in LANGUAGE_CODES:
                    merged["language"] = "en"
                if merged.get("filename_template") == OLD_FILENAME_TEMPLATE:
                    merged["filename_template"] = DEFAULT_FILENAME_TEMPLATE
                merged["pitch_mode"] = self.normalize_pitch_mode_value(str(merged.get("pitch_mode") or ""))
                merged["speed_audio_mode"] = self.normalize_speed_audio_mode_value(str(merged.get("speed_audio_mode") or ""))
                merged["direct_link_enter_action"] = self.normalize_direct_link_enter_action(str(merged.get("direct_link_enter_action") or ""))
                merged["replaygain_mode"] = self.normalized_replaygain_mode(str(merged.get("replaygain_mode") or ""))
                merged["video_format"] = self.normalize_video_format_value(str(merged.get("video_format") or ""))
                merged["global_equalizer_gains"] = self.normalized_equalizer_gains(merged.get("global_equalizer_gains"))
                merged["global_equalizer_preset"] = self.normalized_equalizer_preset(str(merged.get("global_equalizer_preset") or EQ_PRESET_FLAT))
                merged["equalizer_preset_gains"] = self.normalized_equalizer_preset_gains(merged.get("equalizer_preset_gains"))
                merged["equalizer_custom_names"] = self.normalized_equalizer_custom_names(merged.get("equalizer_custom_names"))
                if "global_equalizer_preset" not in data and any(abs(value) >= 0.05 for value in merged["global_equalizer_gains"].values()):
                    merged["global_equalizer_preset"] = "custom1"
                    merged["equalizer_custom_names"]["custom1"] = "Imported"
                    merged["equalizer_preset_gains"]["custom1"] = merged["global_equalizer_gains"]
                    self.settings_migrated = True
                merged["equalizer_db_range"] = self.to_int(str(merged.get("equalizer_db_range") or "12"), 12, 6, 24)
                merged["seek_seconds"] = self.to_float(str(merged.get("seek_seconds") or "5"), 5.0, 0.1, 600.0)
                merged["default_volume"] = self.to_int(
                    str(merged.get("default_volume") or "100"),
                    100,
                    0,
                    self.default_volume_max_for_boost(bool(merged.get("volume_boost_by_default", False))),
                )
                old_audio_quality = str(merged.get("audio_quality") or "")
                merged["audio_quality"] = self.normalize_audio_quality_value(old_audio_quality)
                if old_audio_quality and merged["audio_quality"] != old_audio_quality:
                    self.settings_migrated = True
                provider = str(merged.get("podcast_search_provider") or PODCAST_DIRECTORY_PROVIDER_APPLE)
                merged["podcast_search_provider"] = provider if provider in PODCAST_DIRECTORY_PROVIDER_OPTIONS else PODCAST_DIRECTORY_PROVIDER_APPLE
                country = str(merged.get("podcast_search_country") or "US").upper()
                merged["podcast_search_country"] = country if country in PODCAST_COUNTRY_OPTIONS else "US"
                if not str(merged.get("cookies_browser_profile") or "").strip():
                    merged["cookies_browser_profile"] = COOKIE_PROFILE_AUTO
                shortcuts = self.normalized_keyboard_shortcuts(merged.get("keyboard_shortcuts"))
                repaired_shortcuts = self.repair_keyboard_shortcut_conflicts(shortcuts)
                if repaired_shortcuts != shortcuts:
                    self.settings_migrated = True
                merged["keyboard_shortcuts"] = repaired_shortcuts
                skipped_version = str(merged.get("skipped_update_version") or "")
                if skipped_version and not self.is_newer_version(skipped_version, APP_VERSION):
                    merged["skipped_update_version"] = ""
                if merged.get("update_channel") not in ("stable", "beta"):
                    merged["update_channel"] = "beta"
                # During pre-release cycles no stable builds exist, so a stored
                # "stable" channel means the updater silently reports "up to date"
                # on every check. Migrate existing users to "beta" automatically
                # whenever the running build is itself a pre-release.
                _is_prerelease = any(
                    tag in APP_VERSION.lower()
                    for tag in ("alpha", "beta", "rc")
                )
                if _is_prerelease and merged.get("update_channel") == "stable":
                    merged["update_channel"] = "beta"
                    self.settings_migrated = True
                merged["stream_url_cache_minutes"] = self.normalized_stream_url_cache_minutes(merged.get("stream_url_cache_minutes"))
                self.settings_loaded_from_path = source
                if source != SETTINGS_FILE:
                    self.settings_migrated = True
                return Settings(**merged)
            except Exception as exc:
                load_errors.append(f"{source}: {exc}")
                continue
        self.settings_load_errors = load_errors
        if SETTINGS_FILE.exists() or backup_settings.exists():
            self.settings_save_blocked = True
            self.log_update_event("Settings load failed; automatic settings saves are blocked to avoid overwriting user preferences. " + " | ".join(load_errors[-3:]))
        return Settings()


    def save_settings(self) -> None:
        if getattr(self, "settings_save_blocked", False):
            self.log_update_event("Settings save skipped because settings could not be loaded safely.")
            return
        APP_DIR.mkdir(parents=True, exist_ok=True)
        payload = json.dumps(asdict(self.settings), indent=2, ensure_ascii=False)
        temp_file = SETTINGS_FILE.with_suffix(".json.tmp")
        backup_file = SETTINGS_FILE.with_suffix(".json.bak")
        temp_file.write_text(payload, encoding="utf-8")
        if SETTINGS_FILE.exists():
            try:
                shutil.copy2(SETTINGS_FILE, backup_file)
            except OSError:
                pass
        os.replace(temp_file, SETTINGS_FILE)


    def load_favorites(self) -> list[dict]:
        source = FAVORITES_FILE if FAVORITES_FILE.exists() else LEGACY_FAVORITES_FILE
        if source.exists():
            try:
                data = json.loads(source.read_text(encoding="utf-8"))
                return data if isinstance(data, list) else []
            except Exception:
                return []
        return []


    def save_favorites(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        FAVORITES_FILE.write_text(json.dumps(self.favorites, indent=2, ensure_ascii=False), encoding="utf-8")


    def load_history(self) -> list[dict]:
        return self.load_json_list(HISTORY_FILE)

    def history_save_mutex(self) -> threading.Lock:
        lock = getattr(self, "history_save_lock", None)
        if lock is None:
            lock = threading.Lock()
            self.history_save_lock = lock
        return lock

    def write_history_snapshot(self, snapshot: list[dict]) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        payload = json.dumps(snapshot, indent=2, ensure_ascii=False)
        temp_file = HISTORY_FILE.with_suffix(".json.tmp")
        temp_file.write_text(payload, encoding="utf-8")
        os.replace(temp_file, HISTORY_FILE)

    def next_history_save_generation(self) -> int:
        generation = int(getattr(self, "history_save_generation", 0) or 0) + 1
        self.history_save_generation = generation
        return generation

    def save_history(self) -> None:
        snapshot = list(self.history)
        generation = self.next_history_save_generation()
        with self.history_save_mutex():
            if generation != getattr(self, "history_save_generation", generation):
                return
            self.write_history_snapshot(snapshot)

    def save_history_async(self) -> None:
        snapshot = list(self.history)
        generation = self.next_history_save_generation()
        threading.Thread(target=self.save_history_snapshot_worker, args=(snapshot, generation), daemon=True).start()

    def save_history_snapshot_worker(self, snapshot: list[dict], generation: int) -> None:
        try:
            with self.history_save_mutex():
                if generation != getattr(self, "history_save_generation", generation):
                    return
                self.write_history_snapshot(snapshot)
        except Exception:
            pass


    def load_subscriptions(self) -> list[dict]:
        return self.load_json_list(SUBSCRIPTIONS_FILE)


    def save_subscriptions(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        SUBSCRIPTIONS_FILE.write_text(json.dumps(self.subscriptions, indent=2, ensure_ascii=False), encoding="utf-8")


    def load_rss_feeds(self) -> list[dict]:
        return self.load_json_list(RSS_FEEDS_FILE)


    def ensure_rss_feeds_loaded(self) -> None:
        if getattr(self, "rss_feeds_loaded", False):
            return
        self.rss_feeds = self.load_rss_feeds()
        self.rss_feeds_loaded = True


    def save_rss_feeds(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        self.rss_feeds_loaded = True
        RSS_FEEDS_FILE.write_text(json.dumps(self.rss_feeds, indent=2, ensure_ascii=False), encoding="utf-8")


    def load_user_playlists(self) -> list[dict]:
        return self.load_json_list(USER_PLAYLISTS_FILE)


    def save_user_playlists(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        USER_PLAYLISTS_FILE.write_text(json.dumps(self.user_playlists, indent=2, ensure_ascii=False), encoding="utf-8")


    def load_notifications(self) -> list[dict]:
        return self.load_json_list(NOTIFICATIONS_FILE)


    def save_notifications(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        NOTIFICATIONS_FILE.write_text(json.dumps(self.notifications, indent=2, ensure_ascii=False), encoding="utf-8")


    def load_playback_positions(self) -> dict:
        return self.load_json_dict(PLAYBACK_POSITIONS_FILE)


    def save_playback_positions(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        PLAYBACK_POSITIONS_FILE.write_text(json.dumps(self.playback_positions, indent=2, ensure_ascii=False), encoding="utf-8")


    def load_playback_queue(self) -> list[dict]:
        return self.load_json_list(PLAYBACK_QUEUE_FILE)


    def save_playback_queue(self) -> None:
        APP_DIR.mkdir(parents=True, exist_ok=True)
        PLAYBACK_QUEUE_FILE.write_text(json.dumps(self.playback_queue, indent=2, ensure_ascii=False), encoding="utf-8")


    def load_stream_url_cache(self) -> dict:
        """Load the persisted stream-URL cache from disk.

        Expired entries are filtered out at load time so stale data never
        reaches the running session.  Any read / parse error silently returns
        an empty dict — the cache is a pure performance optimisation and loss
        is never fatal.
        """
        try:
            if STREAM_URL_CACHE_FILE.exists():
                data = json.loads(STREAM_URL_CACHE_FILE.read_text(encoding="utf-8"))
                if isinstance(data, dict):
                    now = time.time()
                    return {
                        k: v
                        for k, v in data.items()
                        if isinstance(v, dict) and float(v.get("expires_at") or 0) > now
                    }
        except Exception:
            pass
        return {}


    def save_stream_url_cache(self) -> None:
        """Persist the in-memory stream-URL cache to disk.

        Takes a snapshot under the lock, filters expired entries, then writes
        outside the lock so IO never blocks the UI thread.
        """
        try:
            lock = getattr(self, "stream_url_cache_lock", None)
            if lock is not None:
                with lock:
                    snapshot = dict(self.stream_url_cache)
            else:
                snapshot = dict(getattr(self, "stream_url_cache", {}))
            now = time.time()
            to_save = {k: v for k, v in snapshot.items() if isinstance(v, dict) and float(v.get("expires_at") or 0) > now}
            APP_DIR.mkdir(parents=True, exist_ok=True)
            STREAM_URL_CACHE_FILE.write_text(json.dumps(to_save, ensure_ascii=False), encoding="utf-8")
        except Exception:
            pass

