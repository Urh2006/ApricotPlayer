from apricot.constants import *
import wx
import os
from pathlib import Path

class DownloadsUI:
    def ydl_options(self, options: dict | None = None, use_cookies: bool = False, use_js_solver: bool = False) -> dict:
        disable_external_ytdlp_plugins()
        merged = {
            "logger": YTDLP_LOGGER,
            "no_warnings": True,
        }
        if use_js_solver:
            merged["js_runtimes"] = self.ytdlp_js_runtimes()
            if not self.ytdlp_ejs_available():
                merged["remote_components"] = ["ejs:github"]
        if options:
            merged.update(options)
        cookiefile = str(merged.get("cookiefile") or "").strip()
        if use_cookies and not cookiefile:
            cookiefile = self.effective_cookies_file()
        if cookiefile:
            merged["cookiefile"] = str(Path(os.path.expandvars(cookiefile.strip('"'))).expanduser())
            cookie_user_agent = str(getattr(self.settings, "cookie_user_agent", "") or "").strip()
            if cookie_user_agent:
                headers = dict(merged.get("http_headers") or {})
                headers["User-Agent"] = cookie_user_agent
                merged["http_headers"] = headers
        return merged

    def is_requested_format_error(self, exc: Exception | str) -> bool:
        return "requested format is not available" in str(exc).lower()

    def ydl_extract_info(
        self,
        url: str,
        options: dict | None = None,
        download: bool = False,
        use_cookies: bool = False,
        use_js_solver: bool = False,
        allow_cookie_retry: bool = True,
    ) -> dict:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            raise RuntimeError(self.t("missing_ytdlp"))

        def run_once(run_with_cookies: bool = False):
            with ytdlp.YoutubeDL(self.ydl_options(options, use_cookies=run_with_cookies, use_js_solver=use_js_solver)) as ydl:
                return ydl.extract_info(url, download=download)

        try:
            return run_once(use_cookies)
        except Exception as exc:
            if not allow_cookie_retry or not self.is_cookie_auth_error(exc):
                raise
            retry_error: Exception | str = exc
            if not use_cookies and self.effective_cookies_file():
                try:
                    return run_once(True)
                except Exception as cookie_exc:
                    retry_error = cookie_exc
                    if not self.is_cookie_auth_error(cookie_exc):
                        raise
            if self.repair_cookies_for_error(retry_error):
                return run_once(True)
            raise retry_error if isinstance(retry_error, Exception) else exc

    def ydl_download_urls(self, urls: list[str], options: dict | None = None) -> None:
        ytdlp = get_yt_dlp()
        if ytdlp is None:
            raise RuntimeError(self.t("missing_ytdlp"))

        def run_once(use_cookies: bool = False) -> None:
            with ytdlp.YoutubeDL(self.ydl_options(options, use_cookies=use_cookies)) as ydl:
                ydl.download(urls)

        try:
            run_once()
        except Exception as exc:
            if not self.is_cookie_auth_error(exc):
                raise
            retry_error: Exception | str = exc
            if self.effective_cookies_file():
                try:
                    run_once(use_cookies=True)
                    return
                except Exception as cookie_exc:
                    retry_error = cookie_exc
                    if not self.is_cookie_auth_error(cookie_exc):
                        raise
            if self.repair_cookies_for_error(retry_error):
                run_once(use_cookies=True)
                return
            raise retry_error if isinstance(retry_error, Exception) else exc

    def fetch_devtools_json(self, port: int, endpoint: str, timeout: float = 1.0) -> dict:
        request = Request(f"http://127.0.0.1:{port}{endpoint}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with urlopen(request, timeout=timeout) as response:
            return json.loads(response.read().decode("utf-8", errors="replace"))

    def show_download_complete_notification(self, message: str) -> bool:
        return self.show_desktop_notification(
            self.t("notification_download_title"),
            message,
            enabled=self.settings.download_notifications,
            only_when_unfocused=True,
        )

    def show_download_queue(self) -> None:
        self.last_activated_menu_action = self.show_download_queue
        self.in_main_menu = False
        self.in_queue_screen = True
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
        self.add_background_player_section()
        buttons = [(self.t("back"), self.show_main_menu)]
        if self.download_queue:
            buttons.append((self.t("download_all_as_audio"), lambda: self.download_all_queued(True)))
            buttons.append((self.t("download_all_as_video"), lambda: self.download_all_queued(False)))
        if self.active_downloads:
            buttons.append((self.t("cancel_download"), self.cancel_selected_download))
            buttons.append((self.t("cancel_all_downloads"), self.cancel_all_downloads))
        self.add_button_row(buttons)
        title = wx.StaticText(self.panel, label=self.t("current_downloads"))
        self.root_sizer.Add(title, 0, wx.ALL, 4)
        instructions = wx.StaticText(self.panel, label=self.t("queued_download_instructions"))
        self.root_sizer.Add(instructions, 0, wx.LEFT | wx.RIGHT | wx.BOTTOM, 4)
        self.queue_items = self.download_items_snapshot()
        queue_choices = [self.queue_line(item) for item in self.queue_items] or [self.t("no_queued_downloads")]
        self.queue_list = wx.ListBox(self.panel, choices=queue_choices)
        self.queue_list.SetName(self.t("current_downloads"))
        self.queue_list.Bind(wx.EVT_CONTEXT_MENU, self.open_queue_context_menu)
        self.queue_list.Bind(wx.EVT_KEY_DOWN, self.on_queue_key)
        self.root_sizer.Add(self.queue_list, 1, wx.EXPAND | wx.ALL, 4)
        self.queue_list.SetSelection(0)
        self.panel.Layout()
        self.focus_later(self.queue_list)

    def dynamic_fetch_failed(self, error: str) -> None:
        self.loading_more_results = False
        if self.pending_player_next_after_dynamic_load:
            self.pending_player_next_after_dynamic_load = False
            self.pending_player_next_preserve_focus = False
            self.pending_player_next_current_url = ""
            self.set_status(error)
            self.announce_player(error)
            return
        self.set_status(error)
        self.announce_player(error)

    def dynamic_fetch_failed_if_current(self, generation: int, error: str) -> None:
        if generation == self.search_generation:
            self.dynamic_fetch_failed(error)

    def fetch_ytdlp_channel_popular(self, url: str, generation: int) -> tuple[list[dict], bool]:
        options = {"quiet": True, "extract_flat": True, "skip_download": True}
        info = self.ydl_extract_info(url, options, download=False)
        entries = [entry for entry in list((info or {}).get("entries") or []) if isinstance(entry, dict)]
        normalized = self.dedupe_results_by_url([self.normalize_entry(entry, "Video") for entry in entries])
        total = len(normalized)
        if not total:
            return [], True
        wx.CallAfter(self.set_status_if_current, generation, self.t("popular_scan_status", done=0, total=total))
        hydrated: list[dict] = []
        workers = min(POPULAR_CHANNEL_METADATA_WORKERS, total)
        done = 0
        futures_module = import_module("concurrent.futures")
        with futures_module.ThreadPoolExecutor(max_workers=workers) as executor:
            futures = [executor.submit(self.hydrate_video_metadata_for_popular, item) for item in normalized]
            for future in futures_module.as_completed(futures):
                try:
                    hydrated.append(future.result())
                except Exception:
                    pass
                done += 1
                if done == total or done % POPULAR_CHANNEL_PROGRESS_INTERVAL == 0:
                    wx.CallAfter(self.set_status_if_current, generation, self.t("popular_scan_status", done=done, total=total))
        return self.sort_popular_results(hydrated), True

    def fetch_popular_channel_results(self, url: str, limit: int, generation: int) -> tuple[list[dict], bool]:
        if self.youtube_data_api_key():
            try:
                results, fully_loaded = self.fetch_youtube_api_channel_popular(url, limit)
                if results:
                    return self.sort_popular_results(results), fully_loaded
            except Exception:
                pass
        return self.fetch_ytdlp_channel_popular(url, generation)

    def fetch_public_official_trending(self, country_code: str, category_code: str) -> list[dict]:
        if country_code == "global":
            country_code = "US"
        urls = TRENDING_PUBLIC_URLS.get(category_code) or TRENDING_PUBLIC_URLS.get("all", [])
        last_error = self.t("trending_api_key_required")
        for template in urls:
            url = template.format(country=country_code, country_lower=country_code.lower())
            try:
                limit = self.max_results_limit() or 50
                options = {"quiet": True, "extract_flat": True, "skip_download": True, "playlistend": min(50, limit)}
                info = self.ydl_extract_info(url, options, download=False, allow_cookie_retry=False)
                entries = list((info or {}).get("entries") or [])
                normalized = [self.normalize_entry(entry, "Video") for entry in entries if isinstance(entry, dict)]
                if normalized:
                    return normalized
            except Exception as exc:
                last_error = self.friendly_error(exc)
        raise RuntimeError(f"{self.t('trending_api_key_required')}\n\n{last_error}")

    def schedule_next_stream_prefetch_for_request(self, generation: int) -> None:
        if self.playback_request_is_current(generation):
            self.schedule_next_stream_prefetch()

    def next_prefetch_candidate(self) -> dict | None:
        if self.current_player_sequence_active():
            return self.relative_player_item(1)
        if self.playback_queue:
            return dict(self.playback_queue[0])
        return self.relative_player_item(1)

    def schedule_next_stream_prefetch(self) -> None:
        if not getattr(self.settings, "prefetch_next_stream_url", True):
            return
        item = self.next_prefetch_candidate()
        url = str((item or {}).get("url") or "")
        if not url or self.local_media_path_from_input(url):
            return
        key = self.stream_url_cache_key(url)
        with self.stream_url_cache_lock:
            if key in self.stream_url_cache:
                return
            if key in self.prefetch_stream_urls:
                return
            self.prefetch_stream_urls.add(key)
        threading.Thread(target=self.prefetch_stream_url_worker, args=(url, key), daemon=True).start()

    def prefetch_stream_url_worker(self, url: str, key: str) -> None:
        try:
            self.resolve_stream_url(url)
        except Exception:
            pass
        finally:
            with self.stream_url_cache_lock:
                self.prefetch_stream_urls.discard(key)

    def fetch_comments_worker(self, video_id: str, page_token: str, source_url: str, callback) -> None:
        api_error = ""
        try:
            if self.youtube_data_api_key():
                try:
                    comments, next_page = self.fetch_youtube_comments(video_id, page_token)
                    wx.CallAfter(callback, comments, next_page, "", "comments_source_api")
                    return
                except Exception as exc:
                    api_error = self.friendly_error(exc)
                    if page_token:
                        raise
            comments = self.fetch_ytdlp_comments(video_id, source_url)
            wx.CallAfter(callback, comments, "", "", "comments_source_ytdlp")
        except Exception as exc:
            error = self.friendly_error(exc)
            if api_error and api_error != error:
                error = f"{api_error}\n\n{error}"
            wx.CallAfter(callback, [], "", error)

    def fetch_ytdlp_comments(self, video_id: str, source_url: str = "") -> list[dict]:
        url = str(source_url or "").strip() or f"https://www.youtube.com/watch?v={video_id}"
        options = {
            "quiet": True,
            "skip_download": True,
            "noplaylist": True,
            "getcomments": True,
            "extractor_args": {"youtube": {"max_comments": ["20"]}},
        }
        info = self.ydl_extract_info(url, options, download=False)
        comments: list[dict] = []
        for raw in list((info or {}).get("comments") or [])[:20]:
            if not isinstance(raw, dict):
                continue
            text = self.strip_html(str(raw.get("text") or ""))
            if not text:
                continue
            comments.append(
                {
                    "id": str(raw.get("id") or ""),
                    "author": str(raw.get("author") or raw.get("author_id") or "").strip(),
                    "text": text,
                    "published": self.format_history_time(raw.get("timestamp")) if raw.get("timestamp") else "",
                    "likes": raw.get("like_count", 0),
                    "reply_count": 0,
                    "replies": [],
                }
            )
        return comments

    def next_download_task_id(self, prefix: str = "download") -> str:
        self.download_task_counter += 1
        return f"{prefix}-{self.download_task_counter}-{int(time.time() * 1000)}"

    def show_download_progress_dialog(self, task_id: str, title: str) -> None:
        self.close_download_progress_dialog()
        self.download_progress_task_id = task_id
        self.download_progress_dialog = wx.ProgressDialog(
            self.t("download_progress_title"),
            self.t("download_progress_message", title=title, completed=0, total=0, remaining=0),
            maximum=100,
            parent=self,
            style=wx.PD_ELAPSED_TIME | wx.PD_ESTIMATED_TIME | wx.PD_REMAINING_TIME,
        )

    def update_download_progress_dialog(self, task: dict) -> None:
        dialog = self.download_progress_dialog
        if not dialog or str(task.get("task_id") or "") != self.download_progress_task_id:
            return
        total = self.to_int(str(task.get("total") or task.get("playlist_count") or 0), 0, 0)
        completed = self.to_int(str(task.get("completed") or 0), 0, 0)
        playlist_index = self.to_int(str(task.get("playlist_index") or 0), 0, 0)
        if total and playlist_index:
            if task.get("status_key") == "download_state_processing":
                completed = max(completed, min(total, playlist_index))
            else:
                completed = max(completed, min(total, max(0, playlist_index - 1)))
        remaining = max(0, total - completed) if total else 0
        if total:
            percent = max(0, min(100, int(round((completed / total) * 100))))
        else:
            percent = self.to_int(str(task.get("percent") or 0), 0, 0, 100)
        title = str(task.get("current_title") or task.get("title") or "")
        message = self.t("download_progress_message", title=title, completed=completed, total=total, remaining=remaining)
        try:
            dialog.Update(percent, message)
        except RuntimeError:
            self.download_progress_dialog = None
            self.download_progress_task_id = ""

    def close_download_progress_dialog(self, task_id: str | None = None) -> None:
        if task_id is not None and task_id != self.download_progress_task_id:
            return
        dialog = self.download_progress_dialog
        self.download_progress_dialog = None
        self.download_progress_task_id = ""
        if dialog:
            try:
                dialog.Destroy()
            except RuntimeError:
                pass

    def fetch_related_and_play_next(self, current_item: dict, generation: int) -> None:
        try:
            url = current_item.get("url") or ""
            video_id = current_item.get("id") or ""
            if not url and video_id:
                url = f"https://www.youtube.com/watch?v={video_id}"
            
            if not url or not self.is_youtube_url(url):
                wx.CallAfter(self.play_next_standard_fallback)
                return
            
            req = Request(
                url,
                headers={"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"}
            )
            with urlopen(req, timeout=10) as response:
                html = response.read().decode('utf-8')
            
            m = re.search(r'var ytInitialData\s*=\s*({.*?});\s*</script>', html)
            if not m:
                m = re.search(r'window\["ytInitialData"\]\s*=\s*({.*?});', html)
            if not m:
                m = re.search(r'ytInitialData\s*=\s*({.*?});', html)
            
            if not m:
                wx.CallAfter(self.play_next_standard_fallback)
                return
            
            data = json.loads(m.group(1))
            videos = []
            
            def recurse(item):
                if isinstance(item, dict):
                    if 'lockupViewModel' in item:
                        lvm = item['lockupViewModel']
                        vid = None
                        title = None
                        channel = None
                        
                        overlays = lvm.get('contentImage', {}).get('thumbnailViewModel', {}).get('overlays', [])
                        for ov in overlays:
                            badge = ov.get('thumbnailBottomOverlayViewModel', {}).get('badges', [])
                            for b in badge:
                                badge_vm = b.get('thumbnailBadgeViewModel', {})
                                if 'animationActivationTargetId' in badge_vm:
                                    vid = badge_vm['animationActivationTargetId']
                        
                        if not vid:
                            def find_vid_in_dict(d):
                                for k, v in d.items():
                                    if k == 'videoId':
                                        return v
                                    elif k == 'watchEndpoint' and isinstance(v, dict):
                                        if 'videoId' in v:
                                            return v['videoId']
                                    elif isinstance(v, dict):
                                        res = find_vid_in_dict(v)
                                        if res:
                                            return res
                                    elif isinstance(v, list):
                                        for x in v:
                                            if isinstance(x, dict):
                                                res = find_vid_in_dict(x)
                                                if res:
                                                    return res
                                return None
                            vid = find_vid_in_dict(lvm)
                        
                        meta = lvm.get('metadata', {}).get('lockupMetadataViewModel', {})
                        title_obj = meta.get('title', {})
                        if isinstance(title_obj, dict):
                            title = title_obj.get('content')
                        
                        byline = meta.get('byline', {})
                        if isinstance(byline, dict):
                            channel = byline.get('content')
                        elif isinstance(byline, list) and len(byline) > 0:
                            channel = byline[0].get('content')
                        
                        if vid and title:
                            videos.append({
                                'id': vid,
                                'title': title,
                                'channel': channel or "Unknown"
                            })
                    else:
                        if 'compactVideoRenderer' in item:
                            cvr = item['compactVideoRenderer']
                            title_text = ""
                            title = cvr.get('title', {})
                            if 'runs' in title and len(title['runs']) > 0:
                                title_text = title['runs'][0].get('text', '')
                            elif 'simpleText' in title:
                                title_text = title.get('simpleText', '')
                            
                            channel_name = ""
                            long_channel = cvr.get('longBylineText', {})
                            if 'runs' in long_channel and len(long_channel['runs']) > 0:
                                channel_name = long_channel['runs'][0].get('text', '')
                            
                            videos.append({
                                'id': cvr.get('videoId'),
                                'title': title_text,
                                'channel': channel_name or "Unknown"
                            })
                        
                        for k, v in item.items():
                            recurse(v)
                elif isinstance(item, list):
                    for x in item:
                        recurse(x)
            
            recurse(data)
            
            seen = {video_id}
            deduped = []
            for v in videos:
                if v['id'] not in seen:
                    seen.add(v['id'])
                    deduped.append(v)
            
            if not deduped:
                wx.CallAfter(self.play_next_standard_fallback)
                return
            
            normalized_results = []
            for entry in deduped:
                raw_entry = {
                    "webpage_url": f"https://www.youtube.com/watch?v={entry['id']}",
                    "id": entry['id'],
                    "title": entry['title'],
                    "uploader": entry['channel'],
                }
                try:
                    normalized = self.normalize_entry(raw_entry, "Video")
                    normalized_results.append(normalized)
                except Exception:
                    pass
            
            if not normalized_results:
                wx.CallAfter(self.play_next_standard_fallback)
                return
            
            wx.CallAfter(self.apply_related_videos_and_play, normalized_results, generation)
            
        except Exception:
            wx.CallAfter(self.play_next_standard_fallback)

    @staticmethod
    def format_rate_for_speech(value: float) -> str:
        return f"{value:.2f}"

    @staticmethod
    def format_step_value(value: float) -> str:
        try:
            return f"{float(value):.2f}"
        except (TypeError, ValueError):
            return "0.01"

    @staticmethod
    def format_seek_seconds_value(value: float) -> str:
        value = round(float(value), 2)
        if abs(value - round(value)) < 0.001:
            return str(int(round(value)))
        return f"{value:.2f}".rstrip("0").rstrip(".")

    @staticmethod
    def format_refresh_interval_value(value, default: float) -> str:
        try:
            hours = max(0.5, min(168.0, float(value)))
        except (TypeError, ValueError):
            hours = default
        if hours.is_integer():
            return str(int(hours))
        return f"{hours:.1f}".rstrip("0").rstrip(".")

    @staticmethod
    def format_ago(timestamp: int) -> str:
        diff = max(0, int(time.time()) - int(timestamp))
        for name, size in (("year", 31536000), ("month", 2592000), ("day", 86400), ("hour", 3600), ("minute", 60)):
            if diff >= size:
                amount = diff // size
                return f"{amount} {name}{'' if amount == 1 else 's'} ago"
        return "just now"

    @staticmethod
    def format_history_time(timestamp) -> str:
        try:
            return datetime.fromtimestamp(float(timestamp)).strftime("%Y-%m-%d %H:%M")
        except Exception:
            return ""

