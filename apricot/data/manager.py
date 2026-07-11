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

_DATA_FILE_LOCKS_GUARD = threading.Lock()

class DataManagerMixin:

    def data_file_lock(self, path: Path) -> threading.RLock:
        locks = getattr(self, "_data_file_locks", None)
        if locks is None:
            with _DATA_FILE_LOCKS_GUARD:
                locks = getattr(self, "_data_file_locks", None)
                if locks is None:
                    locks = {}
                    self._data_file_locks = locks
        key = os.path.normcase(os.path.abspath(str(path)))
        with _DATA_FILE_LOCKS_GUARD:
            lock = locks.get(key)
            if lock is None:
                lock = threading.RLock()
                locks[key] = lock
            return lock


    def atomic_write_text(self, path: Path, text: str) -> None:
        path = Path(path)
        path.parent.mkdir(parents=True, exist_ok=True)
        with self.data_file_lock(path):
            descriptor, temp_name = tempfile.mkstemp(prefix=f".{path.name}.", suffix=".tmp", dir=str(path.parent))
            temp_path = Path(temp_name)
            try:
                handle = os.fdopen(descriptor, "w", encoding="utf-8", newline="\n")
                descriptor = -1
                with handle:
                    handle.write(text)
                    handle.flush()
                os.replace(temp_path, path)
            finally:
                if descriptor >= 0:
                    try:
                        os.close(descriptor)
                    except OSError:
                        pass
                try:
                    temp_path.unlink(missing_ok=True)
                except OSError:
                    pass


    def atomic_write_json(self, path: Path, value, *, indent: int | None = 2) -> None:
        self.atomic_write_text(path, json.dumps(value, indent=indent, ensure_ascii=False))

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
                merged["equalizer_device_presets"] = self.normalized_equalizer_device_presets(merged.get("equalizer_device_presets"))
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
                default_channel = self.default_update_channel()
                if "update_channel" not in data or merged.get("update_channel") not in ("stable", "beta"):
                    merged["update_channel"] = default_channel
                # During pre-release cycles no stable builds exist, so a stored
                # "stable" channel means the updater silently reports "up to date"
                # on every check. Migrate existing users to "beta" automatically
                # whenever the running build is itself a pre-release.
                if self.current_build_is_prerelease() and merged.get("update_channel") == "stable":
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
        settings = Settings()
        settings.update_channel = self.default_update_channel()
        return settings


    def save_settings(self) -> None:
        if getattr(self, "settings_save_blocked", False):
            self.log_update_event("Settings save skipped because settings could not be loaded safely.")
            return
        APP_DIR.mkdir(parents=True, exist_ok=True)
        payload = json.dumps(asdict(self.settings), indent=2, ensure_ascii=False)
        backup_file = SETTINGS_FILE.with_suffix(".json.bak")
        with self.data_file_lock(SETTINGS_FILE):
            if SETTINGS_FILE.exists():
                try:
                    self.atomic_write_text(backup_file, SETTINGS_FILE.read_text(encoding="utf-8"))
                except (OSError, UnicodeError):
                    pass
            self.atomic_write_text(SETTINGS_FILE, payload)


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
        self.atomic_write_json(FAVORITES_FILE, self.favorites)


    def load_bookmarks(self) -> list[dict]:
        return self.load_json_list(BOOKMARKS_FILE)


    def save_bookmarks(self) -> None:
        self.atomic_write_json(BOOKMARKS_FILE, self.bookmarks)


    def load_history(self) -> list[dict]:
        return self.load_json_list(HISTORY_FILE)

    def history_save_mutex(self) -> threading.Lock:
        lock = getattr(self, "history_save_lock", None)
        if lock is None:
            with _DATA_FILE_LOCKS_GUARD:
                lock = getattr(self, "history_save_lock", None)
                if lock is None:
                    lock = threading.Lock()
                    self.history_save_lock = lock
        return lock

    def write_history_snapshot(self, snapshot: list[dict]) -> None:
        self.atomic_write_json(HISTORY_FILE, snapshot)

    def next_history_save_generation(self) -> int:
        with _DATA_FILE_LOCKS_GUARD:
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
        return self.sorted_saved_collection_items(self.load_json_list(SUBSCRIPTIONS_FILE))


    def save_subscriptions(self) -> None:
        self.subscriptions = self.sorted_saved_collection_items(self.subscriptions)
        self.atomic_write_json(SUBSCRIPTIONS_FILE, self.subscriptions)


    def load_rss_feeds(self) -> list[dict]:
        return self.sorted_saved_collection_items(self.load_json_list(RSS_FEEDS_FILE))


    def ensure_rss_feeds_loaded(self) -> None:
        if getattr(self, "rss_feeds_loaded", False):
            return
        self.rss_feeds = self.load_rss_feeds()
        self.rss_feeds_loaded = True


    def save_rss_feeds(self) -> None:
        self.rss_feeds_loaded = True
        current_feed = None
        try:
            current_index = int(getattr(self, "current_rss_feed_index", -1))
        except (TypeError, ValueError):
            current_index = -1
        if 0 <= current_index < len(self.rss_feeds):
            current_feed = self.rss_feeds[current_index]
        self.rss_feeds = self.sorted_saved_collection_items(self.rss_feeds)
        if current_feed is not None:
            self.current_rss_feed_index = next(
                (index for index, candidate in enumerate(self.rss_feeds) if candidate is current_feed),
                -1,
            )
        self.atomic_write_json(RSS_FEEDS_FILE, self.rss_feeds)


    @staticmethod
    def saved_collection_sort_key(item: dict) -> tuple[str, str, str]:
        if not isinstance(item, dict):
            return "", "", ""
        category = str(item.get("category") or "").strip()
        title = str(item.get("title") or item.get("channel") or item.get("name") or "").strip()
        url = str(item.get("url") or "").strip()
        return category.casefold(), title.casefold(), url.casefold()


    def sorted_saved_collection_items(self, items: list[dict]) -> list[dict]:
        return sorted(items, key=self.saved_collection_sort_key)


    def load_user_playlists(self) -> list[dict]:
        return self.load_json_list(USER_PLAYLISTS_FILE)


    def save_user_playlists(self) -> None:
        self.atomic_write_json(USER_PLAYLISTS_FILE, self.user_playlists)


    def load_notifications(self) -> list[dict]:
        return self.load_json_list(NOTIFICATIONS_FILE)


    def save_notifications(self) -> None:
        self.atomic_write_json(NOTIFICATIONS_FILE, self.notifications)


    def load_playback_positions(self) -> dict:
        return self.load_json_dict(PLAYBACK_POSITIONS_FILE)


    def save_playback_positions(self) -> None:
        self.atomic_write_json(PLAYBACK_POSITIONS_FILE, self.playback_positions)


    def load_playback_queue(self) -> list[dict]:
        return self.load_json_list(PLAYBACK_QUEUE_FILE)


    def save_playback_queue(self) -> None:
        self.atomic_write_json(PLAYBACK_QUEUE_FILE, self.playback_queue)

    def load_last_player_session(self) -> dict:
        data = self.load_json_dict(LAST_PLAYER_SESSION_FILE)
        item = data.get("item") if isinstance(data, dict) else None
        if not isinstance(item, dict):
            return {}
        url = str(item.get("url") or item.get("webpage_url") or item.get("local_path") or item.get("path") or "").strip()
        return data if url else {}


    def last_player_session_save_mutex(self) -> threading.Lock:
        lock = getattr(self, "last_player_session_save_lock", None)
        if lock is None:
            with _DATA_FILE_LOCKS_GUARD:
                lock = getattr(self, "last_player_session_save_lock", None)
                if lock is None:
                    lock = threading.Lock()
                    self.last_player_session_save_lock = lock
        return lock

    def next_last_player_session_save_generation(self) -> int:
        with _DATA_FILE_LOCKS_GUARD:
            generation = int(getattr(self, "last_player_session_save_generation", 0) or 0) + 1
            self.last_player_session_save_generation = generation
            return generation

    def write_last_player_session_snapshot(self, snapshot: dict) -> None:
        try:
            self.atomic_write_json(LAST_PLAYER_SESSION_FILE, snapshot)
        except Exception:
            pass

    def save_last_player_session_snapshot_async(self, snapshot: dict) -> None:
        generation = self.next_last_player_session_save_generation()
        threading.Thread(
            target=self.save_last_player_session_snapshot_worker,
            args=(snapshot, generation),
            daemon=True,
        ).start()

    def save_last_player_session_snapshot_worker(self, snapshot: dict, generation: int) -> None:
        try:
            with self.last_player_session_save_mutex():
                if generation != getattr(self, "last_player_session_save_generation", generation):
                    return
                self.write_last_player_session_snapshot(snapshot)
        except Exception:
            pass


    def save_last_player_session(self) -> None:
        snapshot = getattr(self, "last_player_session", {})
        if not isinstance(snapshot, dict):
            snapshot = {}
        generation = self.next_last_player_session_save_generation()
        with self.last_player_session_save_mutex():
            if generation == getattr(self, "last_player_session_save_generation", generation):
                self.write_last_player_session_snapshot(snapshot)


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
            with self.data_file_lock(STREAM_URL_CACHE_FILE):
                lock = getattr(self, "stream_url_cache_lock", None)
                if lock is not None:
                    with lock:
                        snapshot = dict(self.stream_url_cache)
                else:
                    snapshot = dict(getattr(self, "stream_url_cache", {}))
                now = time.time()
                to_save = {k: v for k, v in snapshot.items() if isinstance(v, dict) and float(v.get("expires_at") or 0) > now}
                self.atomic_write_json(STREAM_URL_CACHE_FILE, to_save, indent=None)
        except Exception:
            pass

