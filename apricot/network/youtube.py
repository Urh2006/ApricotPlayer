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

class YoutubeMixin:

    @staticmethod
    def is_youtube_url(url: str) -> bool:
        try:
            host = (urlparse(str(url or "")).netloc or "").lower()
        except Exception:
            return False
        return "youtube.com" in host or "youtu.be" in host


    def cookies_file_has_youtube_login(self, path: str) -> bool:
        if not path:
            return False
        cookie_path = Path(path)
        try:
            stat = cookie_path.stat()
        except OSError:
            return False
        cache = getattr(self, "_youtube_cookie_login_cache", None)
        if cache is None:
            cache = {}
            self._youtube_cookie_login_cache = cache
        key = (str(cookie_path), stat.st_mtime_ns, stat.st_size)
        if key in cache:
            return bool(cache[key])
        try:
            _score, _youtube_count, _total_count, has_login = self.cookie_file_score(cookie_path)
        except Exception:
            has_login = False
        cache.clear()
        cache[key] = bool(has_login)
        return bool(has_login)



    @staticmethod
    def youtube_auth_cookie_names() -> set[str]:
        return {
            "sid",
            "sidcc",
            "lsid",
            "osid",
            "hsid",
            "ssid",
            "apisid",
            "sapisid",
            "login_info",
            "account_chooser",
            "__secure-osid",
            "__secure-1psid",
            "__secure-3psid",
            "__secure-1papisid",
            "__secure-3papisid",
            "__secure-1psidts",
            "__secure-3psidts",
            "__secure-1psidcc",
            "__secure-3psidcc",
        }



    def extract_youtube_video_id(self, item: dict | None = None) -> str:
        item = item or self.current_video_info or self.current_video_item or {}
        video_id = str((item or {}).get("id") or "").strip()
        if video_id and re.fullmatch(r"[\w-]{8,}", video_id):
            return video_id
        url = str((item or {}).get("url") or (item or {}).get("webpage_url") or "").strip()
        if not url:
            return ""
        try:
            parsed = urlparse(url)
        except Exception:
            return ""
        host = (parsed.netloc or "").lower()
        if "youtu.be" in host:
            return parsed.path.strip("/").split("/", 1)[0]
        if "youtube.com" not in host:
            return ""
        query_id = (parse_qs(parsed.query).get("v") or [""])[0]
        if query_id:
            return query_id
        match = re.search(r"/(?:shorts|embed|live)/([\w-]+)", parsed.path or "")
        return match.group(1) if match else ""


    def youtube_comments_source_url(self, item: dict | None, video_id: str) -> str:
        item = item if isinstance(item, dict) else {}
        for key in ("webpage_url", "original_url", "watch_url", "url"):
            url = str(item.get(key) or "").strip()
            if not url:
                continue
            try:
                host = (urlparse(url).netloc or "").lower()
            except Exception:
                continue
            if "googlevideo.com" in host or "youtubei.googleapis.com" in host:
                continue
            if "youtube.com" not in host and "youtu.be" not in host:
                continue
            if self.extract_youtube_video_id({"url": url}) == video_id:
                return url
        return f"https://www.youtube.com/watch?v={video_id}"



    @staticmethod
    def is_youtube_channel_id(value: str) -> bool:
        return bool(re.fullmatch(r"UC[\w-]{20,}", str(value or "").strip()))



    def fetch_youtube_api_videos_by_ids(self, video_ids: list[str]) -> list[dict]:
        api_key = self.youtube_data_api_key()
        ordered_ids = [video_id for video_id in video_ids if video_id]
        if not api_key or not ordered_ids:
            return []
        videos_by_id: dict[str, dict] = {}
        for start in range(0, len(ordered_ids), 50):
            chunk = ordered_ids[start : start + 50]
            params = {
                "part": "snippet,contentDetails,statistics",
                "id": ",".join(chunk),
                "key": api_key,
                "maxResults": str(len(chunk)),
            }
            request = Request(f"{YOUTUBE_API_VIDEOS_URL}?{urlencode(params)}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
            with self.open_url(request, timeout=25) as response:
                payload = json.loads(response.read().decode("utf-8", errors="replace"))
            if isinstance(payload, dict) and payload.get("error"):
                error = payload.get("error") or {}
                message = error.get("message") if isinstance(error, dict) else str(error)
                raise RuntimeError(message or self.t("trending_api_key_required"))
            for item in list((payload or {}).get("items") or []):
                if isinstance(item, dict):
                    video_id = str(item.get("id") or "")
                    if video_id:
                        videos_by_id[video_id] = item
        return [self.normalize_youtube_api_video(videos_by_id[video_id]) for video_id in ordered_ids if video_id in videos_by_id]


    def fetch_youtube_api_channel_popular(self, url: str, limit: int) -> tuple[list[dict], bool]:
        api_key = self.youtube_data_api_key()
        if not api_key:
            return [], False
        channel_id = self.resolve_channel_id_for_popular(url)
        if not channel_id:
            return [], False
        video_ids: list[str] = []
        next_page = ""
        while len(video_ids) < limit:
            params = {
                "part": "snippet",
                "channelId": channel_id,
                "order": "viewCount",
                "type": "video",
                "maxResults": str(min(50, max(1, limit - len(video_ids)))),
                "key": api_key,
            }
            if next_page:
                params["pageToken"] = next_page
            request = Request(f"{YOUTUBE_API_SEARCH_URL}?{urlencode(params)}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
            with self.open_url(request, timeout=25) as response:
                payload = json.loads(response.read().decode("utf-8", errors="replace"))
            if isinstance(payload, dict) and payload.get("error"):
                error = payload.get("error") or {}
                message = error.get("message") if isinstance(error, dict) else str(error)
                raise RuntimeError(message or self.t("trending_api_key_required"))
            for item in list((payload or {}).get("items") or []):
                video_id = str(((item.get("id") or {}) if isinstance(item, dict) else {}).get("videoId") or "")
                if video_id and video_id not in video_ids:
                    video_ids.append(video_id)
            next_page = str((payload or {}).get("nextPageToken") or "")
            if not next_page:
                break
        return self.fetch_youtube_api_videos_by_ids(video_ids[:limit]), not next_page



    def youtube_data_api_key(self) -> str:
        return str(getattr(self.settings, "youtube_data_api_key", "") or "").strip()


    def fetch_youtube_api_trending(self, country_code: str, category_code: str) -> list[dict]:
        api_key = self.youtube_data_api_key()
        if not api_key:
            raise RuntimeError(self.t("trending_api_key_required"))
        limit = self.max_results_limit() or 50
        max_results = min(50, max(1, limit))
        params = {
            "part": "snippet,contentDetails,statistics",
            "chart": "mostPopular",
            "maxResults": str(max_results),
            "key": api_key,
        }
        if country_code and country_code != "global":
            params["regionCode"] = country_code
        category_id = TRENDING_CATEGORY_IDS.get(category_code, "0")
        if category_id and category_id != "0":
            params["videoCategoryId"] = category_id
        request = Request(f"{YOUTUBE_API_VIDEOS_URL}?{urlencode(params)}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=25) as response:
            payload = json.loads(response.read().decode("utf-8", errors="replace"))
        if isinstance(payload, dict) and payload.get("error"):
            error = payload.get("error") or {}
            message = error.get("message") if isinstance(error, dict) else str(error)
            raise RuntimeError(message or self.t("trending_api_key_required"))
        return [self.normalize_youtube_api_video(item) for item in list(payload.get("items") or []) if isinstance(item, dict)]



    def normalize_youtube_api_video(self, item: dict) -> dict:
        snippet = item.get("snippet") or {}
        statistics = item.get("statistics") or {}
        content_details = item.get("contentDetails") or {}
        video_id = str(item.get("id") or "")
        title = str(snippet.get("title") or "")
        channel = str(snippet.get("channelTitle") or "")
        channel_id = str(snippet.get("channelId") or "")
        published_at = str(snippet.get("publishedAt") or "")
        timestamp = self.timestamp_from_iso_datetime(published_at)
        duration_seconds = self.seconds_from_iso8601_duration(str(content_details.get("duration") or ""))
        view_count = statistics.get("viewCount")
        live_status = self.metadata_live_status(snippet)
        is_live = self.metadata_is_live_stream(snippet)
        normalized = {
            "title": title,
            "id": video_id,
            "channel": channel,
            "channel_url": f"https://www.youtube.com/channel/{channel_id}" if channel_id else "",
            "channel_id": channel_id,
            "views": self.format_count(view_count),
            "view_count": view_count,
            "age": self.t("live_now") if is_live else (self.format_age({"timestamp": timestamp}) if timestamp else self.t("uploaded_unknown")),
            "duration": self.format_duration(duration_seconds),
            "duration_seconds": duration_seconds,
            "timestamp": timestamp,
            "upload_date": "",
            "description": snippet.get("description") or "",
            "type": self.t("live_stream") if is_live else self.t("video"),
            "kind": "video",
            "url": f"https://www.youtube.com/watch?v={video_id}" if video_id else "",
            "live_status": live_status,
            "is_live": is_live,
        }
        return self.with_live_stream_display_fields(normalized, snippet)



    def fetch_youtube_comments(self, video_id: str, page_token: str = "") -> tuple[list[dict], str]:
        params = {
            "part": "snippet,replies",
            "videoId": video_id,
            "maxResults": "20",
            "order": "relevance",
            "textFormat": "plainText",
            "key": self.youtube_data_api_key(),
        }
        if page_token:
            params["pageToken"] = page_token
        payload = self.youtube_api_json(YOUTUBE_API_COMMENT_THREADS_URL, params)
        comments = [self.normalize_youtube_comment_thread(item) for item in list(payload.get("items") or []) if isinstance(item, dict)]
        comments = [comment for comment in comments if comment.get("text")]
        return comments, str(payload.get("nextPageToken") or "")



    def youtube_api_json(self, url: str, params: dict, timeout: int = 25) -> dict:
        request = Request(f"{url}?{urlencode(params)}", headers={"User-Agent": f"{APP_NAME}/{APP_VERSION}"})
        with self.open_url(request, timeout=timeout) as response:
            payload = json.loads(response.read().decode("utf-8", errors="replace"))
        if isinstance(payload, dict) and payload.get("error"):
            error = payload.get("error") or {}
            reason = ""
            try:
                reason = str(((error.get("errors") or [{}])[0] or {}).get("reason") or "")
            except Exception:
                reason = ""
            if reason == "commentsDisabled":
                raise RuntimeError(self.t("comments_disabled"))
            message = error.get("message") if isinstance(error, dict) else str(error)
            raise RuntimeError(message or self.t("comments_failed", error=""))
        return payload if isinstance(payload, dict) else {}



    def normalize_youtube_comment_thread(self, item: dict) -> dict:
        snippet = item.get("snippet") or {}
        top = snippet.get("topLevelComment") or {}
        top_snippet = top.get("snippet") or {}
        comment = self.normalize_comment_snippet(top_snippet)
        comment["id"] = str(top.get("id") or item.get("id") or "")
        comment["reply_count"] = self.to_int(str(snippet.get("totalReplyCount") or 0), 0, 0)
        replies = []
        for reply in list(((item.get("replies") or {}).get("comments") or [])):
            if isinstance(reply, dict):
                reply_data = self.normalize_comment_snippet(reply.get("snippet") or {})
                reply_data["id"] = str(reply.get("id") or "")
                if reply_data.get("text"):
                    replies.append(reply_data)
        comment["replies"] = replies
        return comment



    def youtube_url_at_timestamp(self, item: dict | None, seconds: int) -> str:
        video_id = self.extract_youtube_video_id(item)
        if not video_id:
            return ""
        source_url = ""
        if isinstance(item, dict):
            for key in ("webpage_url", "original_url", "watch_url", "url"):
                candidate = str(item.get(key) or "").strip()
                if not candidate:
                    continue
                try:
                    host = (urlparse(candidate).netloc or "").lower()
                except Exception:
                    continue
                if ("youtube.com" in host or "youtu.be" in host) and "googlevideo.com" not in host:
                    source_url = candidate
                    break
        params: list[tuple[str, str]] = [("v", video_id)]
        if source_url:
            try:
                for key, value in parse_qsl(urlparse(source_url).query, keep_blank_values=True):
                    if key.lower() in {"v", "t", "start", "time_continue"}:
                        continue
                    params.append((key, value))
            except Exception:
                pass
        params.append(("t", f"{max(0, int(seconds))}s"))
        return f"https://www.youtube.com/watch?{urlencode(params)}"



    def youtube_channel_item_for_video(self, item: dict | None) -> dict | None:
        if not item or not isinstance(item, dict):
            return None
        kind = str(item.get("kind") or "").strip().lower()
        if kind in {"channel", "playlist", "local_file", "rss_item", "podcast", "feed"}:
            return None
        channel_url = self.normalize_channel_url(item)
        if not channel_url or "youtube.com" not in channel_url.lower():
            return None
        title = str(item.get("channel") or item.get("uploader") or item.get("channel_id") or channel_url).strip()
        return {
            "title": title,
            "channel": title,
            "url": channel_url,
            "channel_url": channel_url,
            "kind": "channel",
            "type": self.t("channel"),
        }


    def item_has_openable_youtube_channel(self, item: dict | None) -> bool:
        return self.youtube_channel_item_for_video(item) is not None


