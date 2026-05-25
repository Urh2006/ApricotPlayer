from apricot.constants import *
import wx
import os
from pathlib import Path
from apricot.ui.misc import MiscUI

# Fields that are large in a raw yt-dlp info dict but not needed after stream
# URL resolution.  Stripping them before caching keeps the stream URL cache
# from growing into the gigabytes over a long session.
_INFO_CACHE_STRIP_KEYS: frozenset[str] = frozenset({
    "formats",
    "requested_formats",
    "thumbnails",
    "automatic_captions",
    "subtitles",
    "requested_subtitles",
    "comments",
    "heatmap",
    "entries",
    "requested_entries",
    "_format_sort_fields",
    "_formats_info",
})

def _slim_info_for_cache(info: dict) -> dict:
    """Return a copy of *info* with the bulk fields removed.

    The full yt-dlp info dict for a YouTube video can be 10–50 MB in Python
    memory (100+ format entries, 30+ languages of automatic captions, dozens
    of thumbnail URLs, heatmap data, …).  For the stream URL cache we only
    need the small metadata fields used by the player UI.
    """
    return {k: v for k, v in info.items() if k not in _INFO_CACHE_STRIP_KEYS}

class SystemUI:
    def chromium_profile_launch_args(self, browser: str, profile: str | None, headless: bool = True) -> tuple[str, list[str]]:
        root = self.cookie_browser_root(browser)
        if not root:
            raise RuntimeError(f"browser profile root not found for {browser}")
        profile_value = str(profile or "").strip()
        profile_dir = ""
        user_data_dir = root
        if profile_value and os.path.isabs(profile_value):
            profile_path = Path(profile_value)
            if profile_path.exists() and profile_path.parent.exists():
                user_data_dir = profile_path.parent
                profile_dir = profile_path.name
        elif profile_value:
            profile_dir = profile_value
        elif browser != "opera":
            profile_dir = "Default"
        args = [
            f"--user-data-dir={user_data_dir}",
            "--remote-allow-origins=*",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--disable-features=LockProfileCookieDatabase",
        ]
        if headless:
            args.append("--headless=new")
        else:
            args.extend(["--window-position=-32000,-32000", "--window-size=800,600"])
        if profile_dir and browser != "opera":
            args.append(f"--profile-directory={profile_dir}")
        return profile_dir or root.name, args

    def update_play_pause_buttons(self) -> None:
        label = self.current_play_pause_label()
        changed = False
        for button in list(getattr(self, "player_play_pause_buttons", [])):
            try:
                if button and not button.IsBeingDeleted():
                    if button.GetLabel() != label:
                        button.SetLabel(label)
                        changed = True
                    if button.GetName() != label:
                        button.SetName(label)
                        changed = True
                    if button.GetToolTipText() != label:
                        button.SetToolTip(label)
                        changed = True
            except RuntimeError:
                continue
        if changed:
            try:
                self.panel.Layout()
            except Exception:
                pass

    def activate_after_update_relaunch(self) -> None:
        self.activate_window_later((0, 100, 350, 900, 1800, 3000))

    @staticmethod
    def windows_startup_run_key_path() -> str:
        return r"Software\Microsoft\Windows\CurrentVersion\Run"

    def pending_app_update_version(self) -> str:
        if not self.pending_app_update_release:
            return ""
        return self.release_version(self.pending_app_update_release)

    def open_pending_app_update(self) -> None:
        release = self.pending_app_update_release
        asset = self.pending_app_update_asset
        if not release or not asset:
            self.start_app_update_check(manual=True)
            return
        version = self.release_version(release)
        if not getattr(sys, "frozen", False):
            self.message(self.t("update_source_only", version=version))
            return
        changelog = self.release_changelog_text(release)
        if self.show_update_prompt(version, changelog):
            self.log_update_event(f"User selected pending update now for {version}")
            if self.settings.skipped_update_version:
                self.settings.skipped_update_version = ""
                self.save_settings()
            self.begin_app_update_install(release, asset)
        else:
            self.log_update_event(f"User skipped pending update {version}")
            self.settings.skipped_update_version = version
            self.pending_app_update_release = None
            self.pending_app_update_asset = None
            self.save_settings()
            self.announce_player(self.t("update_skipped", version=version))
            if self.in_main_menu:
                self.show_main_menu()

    def show_direct_link(self) -> None:
        self.last_activated_menu_action = self.show_direct_link
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
        self.direct_link_screen_active = True
        self.clear()
        self.add_background_player_section()
        self.add_button_row(
            [
                (self.t("back"), self.show_main_menu),
                (self.t("play_direct_link"), self.play_direct_link),
                (self.t("download_direct_audio"), lambda: self.download_direct_link(True)),
                (self.t("download_direct_video"), lambda: self.download_direct_link(False)),
                (self.t("copy_stream_url"), self.copy_direct_stream_url),
            ]
        )
        label = wx.StaticText(self.panel, label=self.t("direct_link_url"))
        self.root_sizer.Add(label, 0, wx.ALL, 4)
        self.direct_link_ctrl = wx.TextCtrl(self.panel, style=wx.TE_PROCESS_ENTER)
        self.direct_link_ctrl.SetName(self.t("direct_link_url"))
        self.direct_link_ctrl.Bind(wx.EVT_TEXT_ENTER, lambda _evt: self.activate_direct_link_enter_action())
        self.root_sizer.Add(self.direct_link_ctrl, 0, wx.EXPAND | wx.ALL, 4)
        self.panel.Layout()
        self.focus_later(self.direct_link_ctrl)

    def activate_direct_link_enter_action(self) -> None:
        action = self.normalized_direct_link_enter_action()
        if action == DIRECT_LINK_ENTER_AUDIO:
            self.download_direct_link(True)
        elif action == DIRECT_LINK_ENTER_VIDEO:
            self.download_direct_link(False)
        elif action == DIRECT_LINK_ENTER_STREAM:
            self.copy_direct_stream_url(self.direct_link_item())
        else:
            self.play_direct_link()

    def start_file_conversion(self, source: Path, output: Path, target_format: str, image_path: Path | None = None, replace_original: bool = False) -> None:
        output = output if replace_original else self.unique_converter_output_path(output, source)
        self.announce_player(self.t("conversion_started"))
        self.set_status(self.t("conversion_started"))
        threading.Thread(target=self.file_conversion_worker, args=(source, output, target_format, image_path, replace_original), daemon=True).start()

    def file_conversion_worker(self, source: Path, output: Path, target_format: str, image_path: Path | None = None, replace_original: bool = False) -> None:
        try:
            ffmpeg = self.ffmpeg_executable()
            if not ffmpeg:
                raise RuntimeError("FFmpeg was not found")
            output.parent.mkdir(parents=True, exist_ok=True)
            final_output = output
            work_output = self.temporary_conversion_path(output) if replace_original else output
            args = self.converter_ffmpeg_args(ffmpeg, source, work_output, target_format, image_path)
            self.run_ffmpeg_conversion(args)
            if replace_original:
                self.replace_converted_original(source, work_output, final_output)
            done_text = self.t("conversion_done", title=final_output.name)
            wx.CallAfter(self.set_status, done_text)
            wx.CallAfter(self.finish_conversion_message, done_text)
        except Exception as exc:
            wx.CallAfter(self.message, self.t("conversion_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def update_conversion_progress_dialog(self, payload: dict) -> None:
        dialog = self.conversion_progress_dialog
        if not dialog:
            return
        total = max(1, int(payload.get("total") or 1))
        converted = max(0, min(total, int(payload.get("converted") or 0)))
        remaining = max(0, total - converted)
        message = self.t("conversion_progress_message", file=str(payload.get("file") or ""), converted=converted, total=total, remaining=remaining)
        try:
            dialog.Update(converted, message)
        except RuntimeError:
            self.conversion_progress_dialog = None

    @staticmethod
    def unique_folder_path(path: Path) -> Path:
        candidate = path
        counter = 2
        while candidate.exists():
            candidate = path.with_name(f"{path.name} ({counter})")
            counter += 1
        return candidate

    @staticmethod
    def temporary_conversion_path(path: Path) -> Path:
        return path.with_name(f"{path.stem}.apricot-converting{path.suffix}")

    @staticmethod
    def canonical_channel_url(url: str) -> str:
        text = str(url or "").strip()
        if not text:
            return ""
        if text.startswith("@") or text.startswith("/@"):
            text = f"https://www.youtube.com/{text.lstrip('/')}"
        elif text and not text.startswith("http"):
            text = f"https://www.youtube.com/{text.lstrip('/')}"
        base = text.split("?", 1)[0].split("#", 1)[0].rstrip("/")
        base = re.sub(r"/(videos|playlists|featured|streams|shorts|community|about)$", "", base, flags=re.IGNORECASE)
        return base.rstrip("/")

    def configure_app_update_timer(self) -> None:
        if not hasattr(self, "app_update_timer"):
            return
        try:
            self.app_update_timer.Stop()
        except Exception:
            pass
        if self.settings.auto_update_app:
            interval_ms = int(self.refresh_interval_seconds(self.settings.app_update_interval_hours, 6.0, maximum_hours=24.0) * 1000)
            self.app_update_timer.Start(interval_ms)

    def on_app_update_timer(self, _event) -> None:
        self.start_app_update_check(manual=False, prompt=False, notify=True)

    @staticmethod
    def absolute_url(value: str, base_url: str) -> str:
        value = str(value or "").strip()
        if not value:
            return ""
        return urljoin(base_url, value)

    def channel_tab_url(self, item: dict, tab: str) -> str:
        url = str(item.get("url") or item.get("channel_url") or "").strip()
        if not url:
            return ""
        base = url.split("?", 1)[0].rstrip("/")
        base = re.sub(r"/(videos|playlists|featured|streams|shorts)$", "", base, flags=re.IGNORECASE)
        if tab == "popular":
            return f"{base}/videos"
        if tab == "playlists":
            return f"{base}/playlists"
        if tab == "streams":
            return f"{base}/streams"
        return f"{base}/videos"

    def open_channel_tab(self, item: dict, tab: str = "videos", push_state: bool = True) -> None:
        if push_state:
            self.push_search_state()
        self.trending_screen_active = False
        url = self.channel_tab_url(item, tab)
        if not url:
            self.message(self.t("no_selection"))
            return
        title = str(item.get("title") or self.t("channel"))
        if tab == "playlists":
            result_type = "Playlist"
            label = self.t("channel_playlists")
        elif tab == "popular":
            result_type = "Video"
            label = self.t("channel_popular")
        elif tab == "streams":
            result_type = "Video"
            label = self.t("channel_live_streams")
        else:
            result_type = "Video"
            label = self.t("channel_videos")
        self.set_status(self.t("loading_channel", title=f"{title} - {label}"))
        self.collection_url = url
        self.collection_result_type = result_type
        self.collection_sort_mode = "popular" if tab == "popular" else ""
        self.collection_channel_id = str(item.get("channel_id") or "")
        self.collection_fully_loaded = False
        self.loading_more_results = False
        self.dynamic_fetch_enabled = True
        self.metadata_hydration_urls.clear()
        self.search_generation += 1
        generation = self.search_generation
        threading.Thread(target=self.load_collection_worker, args=(url, result_type, self.initial_results_limit(), 0, generation, self.collection_sort_mode), daemon=True).start()

    @staticmethod
    def local_media_path_from_input(value: str) -> Path | None:
        text = str(value or "").strip().strip('"')
        if not text:
            return None
        if text.lower().startswith("file:"):
            parsed = urlparse(text)
            path_text = unquote(parsed.path or "")
            if parsed.netloc:
                text = f"//{parsed.netloc}{path_text}"
            elif os.name == "nt" and re.match(r"^/[A-Za-z]:/", path_text):
                text = path_text[1:]
            else:
                text = path_text
        candidate = Path(text).expanduser()
        try:
            if candidate.exists() and candidate.is_file():
                return candidate.resolve()
        except OSError:
            return None
        return None

    @staticmethod
    def looks_like_local_media_path(value: str) -> bool:
        path = SystemUI.local_media_path_from_input(value)
        return bool(path and (path.suffix.lower() in LOCAL_MEDIA_EXTENSIONS or path.is_file()))

    def local_media_files_in_folder(self, folder: Path) -> list[Path]:
        files: list[Path] = []

        def ignore_walk_error(_error: OSError) -> None:
            return

        try:
            for root, directories, names in os.walk(folder, onerror=ignore_walk_error):
                directories.sort(key=self.natural_sort_key)
                for name in sorted(names, key=self.natural_sort_key):
                    path = Path(root) / name
                    try:
                        if path.is_file() and path.suffix.lower() in LOCAL_MEDIA_EXTENSIONS:
                            files.append(path)
                    except OSError:
                        continue
        except OSError:
            return []
        return sorted(files, key=lambda path: self.natural_sort_key(str(path.relative_to(folder))))

    def show_play_file(self) -> None:
        self.last_activated_menu_action = self.show_play_file
        start_dir = self.settings.download_folder or str(Path.home())
        with wx.FileDialog(
            self,
            self.t("play_file"),
            defaultDir=start_dir if Path(start_dir).exists() else str(Path.home()),
            wildcard=self.local_media_wildcard(),
            style=wx.FD_OPEN | wx.FD_FILE_MUST_EXIST,
        ) as dialog:
            if dialog.ShowModal() != wx.ID_OK:
                self.show_main_menu()
                return
            path = dialog.GetPath()
        self.open_local_media_file(path, True)

    def open_local_media_folder(self, value: str) -> None:
        folder = Path(str(value or "")).expanduser()
        if not folder.exists() or not folder.is_dir():
            self.message(self.t("folder_no_media"), wx.ICON_WARNING)
            self.show_main_menu()
            return
        cached_items = self.cached_local_folder_items(folder)
        if cached_items:
            self.show_local_media_folder(folder, cached_items, selection=0)
            return
        files = self.local_media_files_in_folder(folder)
        if not files:
            self.message(self.t("folder_no_media"), wx.ICON_INFORMATION)
            self.show_main_menu()
            return
        items = [self.local_media_item(path, folder) for path in files]
        self.cache_local_folder_items(folder, items)
        self.show_local_media_folder(folder, items, selection=0)

    def open_local_media_file(self, value: str, activate_after_open: bool = False) -> None:
        try:
            path = self.local_media_path_from_input(value)
            if not path:
                raise FileNotFoundError(value)
            item = self.local_media_item(path)
            self.player_return_screen = "local_file"
            self.player_return_data = {}
            self.current_video_item = item
            self.current_video_info = dict(item)
            if activate_after_open:
                self.set_window_title(item["title"])
                self.set_status(self.t("preparing_stream", title=item["title"]))
                self.ensure_window_visible()
                try:
                    self.foreground_window()
                except Exception:
                    pass
            self.play_url(str(path), item["title"], announce_start=activate_after_open)
            if activate_after_open:
                self.restore_from_tray()
        except Exception as exc:
            self.message(self.t("local_file_open_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def stream_url_cache_key(self, url: str) -> str:
        parts = {
            "url": url,
            "video_format": self.normalized_video_format(),
            "max_height": int(getattr(self.settings, "max_video_height", 1080) or 0),
            "restricted": bool(getattr(self.settings, "enable_age_restricted_videos", False)),
            "cookies_file": str(getattr(self.settings, "cookies_file", "") or ""),
            "cookies_browser": str(getattr(self.settings, "cookies_from_browser", "none") or "none"),
        }
        return json.dumps(parts, sort_keys=True, ensure_ascii=False)

    def stream_url_cache_minutes_value(self) -> int:
        return self.normalized_stream_url_cache_minutes()

    def cached_stream_url(self, url: str) -> tuple[str, dict, dict] | None:
        if not getattr(self.settings, "enable_stream_url_cache", True):
            return None
        key = self.stream_url_cache_key(url)
        now = time.time()
        with self.stream_url_cache_lock:
            cached = self.stream_url_cache.get(key)
            if not cached:
                return None
            if float(cached.get("expires_at") or 0) <= now:
                self.stream_url_cache.pop(key, None)
                return None
            return str(cached.get("stream_url") or ""), dict(cached.get("headers") or {}), dict(cached.get("info") or {})

    def cache_stream_url(self, source_url: str, stream_url: str, headers: dict, info: dict) -> None:
        if not getattr(self.settings, "enable_stream_url_cache", True) or not source_url or not stream_url:
            return
        minutes = self.stream_url_cache_minutes_value()
        ttl_seconds = (365 * 24 * 60 * 60) if minutes <= 0 else minutes * 60
        expires_at = time.time() + ttl_seconds
        try:
            expire_values = parse_qs(urlparse(stream_url).query).get("expire") or []
            if expire_values:
                remote_expiry = int(expire_values[0]) - 60
                expires_at = min(expires_at, float(remote_expiry))
        except (TypeError, ValueError, OverflowError):
            pass
        if expires_at <= time.time() + 30:
            return
        with self.stream_url_cache_lock:
            now = time.time()
            self.stream_url_cache = {key: value for key, value in self.stream_url_cache.items() if float(value.get("expires_at") or 0) > now}
            if len(self.stream_url_cache) > 120:
                oldest = sorted(self.stream_url_cache, key=lambda item_key: float(self.stream_url_cache[item_key].get("expires_at") or 0))
                for old_key in oldest[: len(self.stream_url_cache) - 100]:
                    self.stream_url_cache.pop(old_key, None)
            self.stream_url_cache[self.stream_url_cache_key(source_url)] = {
                "stream_url": stream_url,
                "headers": dict(headers or {}),
                "info": _slim_info_for_cache(dict(info or {})),
                "expires_at": expires_at,
            }

    def resolve_stream_url(self, url: str) -> tuple[str, dict, dict]:
        local_path = self.local_media_path_from_input(url)
        if local_path:
            info = self.local_media_item(local_path)
            return str(local_path), {}, info
        cached = self.cached_stream_url(url)
        if cached and cached[0]:
            return cached
        # Prefer a single progressive HTTPS file (whole-file range-requestable) so
        # mpv can seek instantly and buffer the full track at full network speed.
        # [protocol=https] excludes DASH segment URLs — those cause mpv to play one
        # short segment then stop, with no backward buffer and no seeking support.
        # Audio-only m4a is tried first: it is ~30x smaller than a combined 720p
        # video+audio stream, fills the demuxer cache much faster, and keeps mpv
        # RAM low because no video codec is initialised.
        options = {
            "quiet": True,
            "skip_download": True,
            "format": (
                "bestaudio[ext=m4a][protocol=https]"
                "/bestaudio[protocol=https]"
                "/best[ext=mp4][protocol=https]"
                "/best[acodec!=none][vcodec!=none][protocol=https]"
                "/best[ext=mp4]"
                "/best[acodec!=none][vcodec!=none]"
                "/best"
            ),
            "noplaylist": True,
        }
        format_fallback_options = dict(options)
        format_fallback_options["format"] = (
            "bestaudio[protocol=https]"
            "/best[acodec!=none][protocol=https]"
            "/best[acodec!=none][vcodec!=none]"
            "/18/22/17/best"
        )
        try:
            info = self.ydl_extract_info(url, options, download=False, allow_cookie_retry=False)
        except Exception as exc:
            cookie_file = self.playback_cookies_file_for_url(url)
            cookie_error = self.is_cookie_auth_error(exc)
            age_or_js_error = self.is_age_or_js_playback_error(exc)
            requested_format_error = self.is_requested_format_error(exc)
            retry_error: Exception | str = exc
            if requested_format_error:
                try:
                    info = self.ydl_extract_info(url, format_fallback_options, download=False, allow_cookie_retry=False)
                    stream_url = info.get("url")
                    if stream_url:
                        headers = info.get("http_headers") or {}
                        self.cache_stream_url(url, stream_url, headers, info)
                        return stream_url, headers, info
                except Exception as format_exc:
                    retry_error = format_exc
                    cookie_error = cookie_error or self.is_cookie_auth_error(format_exc)
                    age_or_js_error = age_or_js_error or self.is_age_or_js_playback_error(format_exc)
            can_retry_with_cookies = bool(cookie_file) and (cookie_error or age_or_js_error)
            can_retry_with_restricted_fallback = self.age_restricted_video_support_enabled() and (cookie_error or age_or_js_error)
            can_retry_with_js_format_fallback = requested_format_error and age_or_js_error
            if not (can_retry_with_cookies or can_retry_with_restricted_fallback):
                if can_retry_with_js_format_fallback:
                    try:
                        info = self.ydl_extract_info(
                            url,
                            format_fallback_options,
                            download=False,
                            use_cookies=False,
                            use_js_solver=True,
                            allow_cookie_retry=False,
                        )
                    except Exception:
                        raise retry_error if isinstance(retry_error, Exception) else exc
                else:
                    raise retry_error if isinstance(retry_error, Exception) else exc
            else:
                info = None
            if cookie_file:
                try:
                    info = self.ydl_extract_info(
                        url,
                        format_fallback_options if requested_format_error else options,
                        download=False,
                        use_cookies=True,
                        use_js_solver=False,
                        allow_cookie_retry=False,
                    )
                except Exception as cookie_exc:
                    retry_error = cookie_exc
                    if not self.is_age_or_js_playback_error(cookie_exc) and not self.is_cookie_auth_error(cookie_exc):
                        raise
                    info = self.ydl_extract_info(
                        url,
                        format_fallback_options if requested_format_error else options,
                        download=False,
                        use_cookies=True,
                        use_js_solver=True,
                        allow_cookie_retry=False,
                    )
            elif can_retry_with_restricted_fallback and info is None:
                try:
                    info = self.ydl_extract_info(
                        url,
                        format_fallback_options if requested_format_error else options,
                        download=False,
                        use_cookies=False,
                        use_js_solver=True,
                        allow_cookie_retry=False,
                    )
                except Exception:
                    raise retry_error if isinstance(retry_error, Exception) else exc
        stream_url = info.get("url")
        if not stream_url and info.get("formats"):
            fmts = info["formats"]
            # Prefer: audio-only progressive → combined progressive → any with audio
            _candidates = (
                [f for f in fmts if f.get("url") and f.get("acodec") not in (None, "none", "") and f.get("protocol", "").startswith("http")],
                [f for f in fmts if f.get("url") and f.get("vcodec") not in (None, "none", "") and f.get("acodec") not in (None, "none", "")],
                [f for f in fmts if f.get("url") and f.get("acodec") not in (None, "none", "")],
            )
            for _group in _candidates:
                if _group:
                    stream_url = _group[-1]["url"]
                    break
        if not stream_url:
            raise RuntimeError("No playable stream URL found")
        headers = info.get("http_headers") or {}
        self.cache_stream_url(url, stream_url, headers, info)
        return stream_url, headers, info

    def cache_folder_path(self) -> Path:
        return Path(str(getattr(self.settings, "cache_folder", "") or DEFAULT_CACHE_DIR)).expanduser()

    def update_details_text(self) -> None:
        if not self.video_details:
            return
        details = self.build_video_details_text()
        self.video_details.Freeze()
        self.video_details.SetValue(details)
        self.video_details.SetInsertionPoint(0)
        self.video_details.Thaw()

    def copy_url_to_clipboard(self, url: str) -> None:
        if not url:
            return
        if wx.TheClipboard.Open():
            try:
                wx.TheClipboard.SetData(wx.TextDataObject(url))
            finally:
                wx.TheClipboard.Close()
        self.announce_player(self.t("url_copied"))

    def copy_path_to_clipboard(self, path: str) -> None:
        if not path:
            return
        self.copy_plain_text_to_clipboard(path)
        self.announce_player(self.t("path_copied"))

    def copy_active_url(self) -> None:
        item = self.active_item()
        if item:
            self.copy_url_to_clipboard(item.get("url", ""))

    def current_local_media_path(self) -> Path | None:
        item = self.current_video_item or self.current_video_info or {}
        if str(item.get("kind") or "") != "local_file":
            return None
        return self.local_media_path_from_input(str(item.get("url") or item.get("webpage_url") or ""))

    def save_edited_local_file(self, replace_original: bool = False) -> None:
        if not self.edit_mode_enabled:
            return
        source = self.current_local_media_path()
        if not source:
            self.announce_player(self.t("edit_mode_local_only"))
            return
        self.announce_player(self.t("edit_save_started"))
        if replace_original:
            self.stop_player(silent=True)
        threading.Thread(target=self.save_edited_local_file_worker, args=(source, replace_original), daemon=True).start()

    def save_edited_local_file_worker(self, source: Path, replace_original: bool = False) -> None:
        try:
            ffmpeg = self.ffmpeg_executable()
            if not ffmpeg:
                raise RuntimeError("FFmpeg was not found")
            output = self.edited_output_path(source, replace_original)
            temp_output = output.with_name(f"{output.stem}.apricot-temp{output.suffix}") if replace_original else output
            args = self.local_edit_ffmpeg_args(ffmpeg, source, temp_output)
            creationflags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
            result = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace", creationflags=creationflags)
            if result.returncode != 0:
                error = (result.stderr or result.stdout or "").strip() or f"FFmpeg exited with code {result.returncode}"
                raise RuntimeError(error[-600:])
            if replace_original:
                os.replace(temp_output, source)
                wx.CallAfter(self.announce_player, self.t("edit_replace_done", title=source.name))
            else:
                wx.CallAfter(self.announce_player, self.t("edit_save_done", title=output.name))
        except Exception as exc:
            wx.CallAfter(self.message, self.t("edit_save_failed", error=self.friendly_error(exc)), wx.ICON_ERROR)

    def edited_output_path(self, source: Path, replace_original: bool = False) -> Path:
        if replace_original:
            return source
        output = source.with_name(f"{source.stem} - edited{source.suffix}")
        counter = 2
        while output.exists():
            output = source.with_name(f"{source.stem} - edited ({counter}){source.suffix}")
            counter += 1
        return output

    def normalized_direct_link_enter_action(self) -> str:
        action = str(getattr(self.settings, "direct_link_enter_action", DIRECT_LINK_ENTER_PLAY) or DIRECT_LINK_ENTER_PLAY)
        return self.normalize_direct_link_enter_action(action)

    def direct_link_enter_action_labels(self) -> list[str]:
        return [
            self.t("direct_link_enter_play"),
            self.t("direct_link_enter_audio"),
            self.t("direct_link_enter_video"),
            self.t("direct_link_enter_stream"),
        ]

    def stream_url_cache_labels(self, options: list[str]) -> list[str]:
        labels = []
        for option in options:
            try:
                minutes = int(option)
            except (TypeError, ValueError):
                minutes = 20
            if minutes <= 0:
                labels.append(self.t("stream_cache_permanent"))
            elif minutes < 60:
                labels.append(self.t("stream_cache_minutes_label", minutes=minutes))
            elif minutes % 1440 == 0:
                days = minutes // 1440
                labels.append(self.t("stream_cache_days_label", days=days))
            elif minutes % 60 == 0:
                hours = minutes // 60
                labels.append(self.t("stream_cache_hours_label", hours=hours))
            else:
                labels.append(str(minutes))
        return labels

    def normalized_stream_url_cache_minutes(self, value=None) -> int:
        raw = getattr(self.settings, "stream_url_cache_minutes", 20) if value is None else value
        try:
            minutes = int(raw)
        except (TypeError, ValueError):
            minutes = 20
        if minutes <= 0:
            return 0
        return min(10080, max(5, minutes))

    @staticmethod
    def normalize_direct_link_enter_action(action: str) -> str:
        normalized = str(action or "").strip()
        return normalized if normalized in DIRECT_LINK_ENTER_OPTIONS else DIRECT_LINK_ENTER_PLAY

    def copy_direct_stream_url(self, item: dict | None = None) -> None:
        item = item or self.active_item()
        if self.in_player_screen and self.item_is_local_media(item or self.current_player_item()):
            self.announce_player(self.t("direct_media_link_unavailable_local"))
            return
        if self.in_player_screen and not item and self.current_stream_url:
            self.copy_url_to_clipboard(self.current_stream_url)
            self.announce_player(self.t("stream_url_copied"))
            return
        if self.in_player_screen and item and self.current_video_item and item.get("url") == self.current_video_item.get("url") and self.current_stream_url:
            self.copy_plain_text_to_clipboard(self.current_stream_url)
            self.announce_player(self.t("stream_url_copied"))
            return
        if not item or not item.get("url"):
            self.message(self.t("no_selection"))
            return
        self.announce_player(self.t("resolving_stream_url"))
        threading.Thread(target=self.copy_direct_stream_url_worker, args=(dict(item),), daemon=True).start()

    def copy_direct_stream_url_worker(self, item: dict) -> None:
        try:
            stream_url, _headers, _info = self.resolve_stream_url(str(item.get("url") or ""))
            wx.CallAfter(self.copy_plain_text_to_clipboard, stream_url)
            wx.CallAfter(self.announce_player, self.t("stream_url_copied"))
        except Exception as exc:
            wx.CallAfter(self.announce_player, self.t("stream_url_failed", error=self.friendly_error(exc)))

    def remove_queued_url(self, url: str, announce: bool = True) -> None:
        if not url:
            return
        item = self.download_queue.pop(url, None)
        if item and announce:
            self.announce_player(self.t("download_deselected", title=item.get("title", "")))
        self.refresh_results_list_labels()
        if self.rss_items_screen_active:
            self.refresh_rss_items_list()
        if self.in_queue_screen:
            self.refresh_queue_view()
        self.refresh_download_views()

