from __future__ import annotations

import io
import hashlib
import os
import tempfile
import threading
import time
import unittest
import zipfile
from unittest import mock
from pathlib import Path
from types import SimpleNamespace

from apricot.constants import CUSTOM_MPV_CACHE_SUBDIR, DEFAULT_CACHE_DIR, LOCAL_FOLDER_CACHE_MAX_ENTRIES
from apricot.data.manager import DataManagerMixin
from apricot.download.download import DownloaderMixin
from apricot.library.library import LibraryMixin
from apricot.media.media import MediaMixin
from apricot.network.youtube import YoutubeMixin
from apricot.player.mpv import MpvMixin
from apricot.search.search import SearchMixin
from apricot.system.diagnostics import DiagnosticsMixin
from apricot.ui.cookies import CookiesUI
from apricot.ui.lists import ListsUI
from apricot.ui.misc import MiscUI
from apricot.ui.system import SystemUI
from apricot.updater.updater import AppUpdaterMixin
from apricot.utils import UtilsMixin


class SecurityHarness(AppUpdaterMixin, UtilsMixin):
    pass


class FolderCacheHarness(ListsUI):
    def __init__(self) -> None:
        self.local_folder_cache: dict[str, list[dict]] = {}

    @staticmethod
    def local_folder_cache_key(folder: Path) -> str:
        return str(folder)


class ResultColumnsHarness(ListsUI):
    def t(self, key: str, **values) -> str:
        return key.format(**values)


class SearchHarness(SearchMixin):
    def __init__(self) -> None:
        self.search_generation = 7
        self.requested_url = ""
        self.shown_results: list[dict] = []

    def ydl_extract_info(self, url: str, _options: dict, download: bool = False) -> dict:
        self.assertFalse(download)
        self.requested_url = url
        return {"entries": [{"title": "Track"}]}

    @staticmethod
    def normalize_entry(entry: dict, _search_type: str, _provider: str = "youtube") -> dict:
        return dict(entry)

    def show_results_if_current(self, generation: int, results: list[dict]) -> None:
        if generation == self.search_generation:
            self.shown_results = results

    def assertFalse(self, value) -> None:
        if value:
            raise AssertionError("unexpected truthy value")


class FakeResponse:
    def __init__(self, data: bytes, content_length: int | None = None) -> None:
        self.data = data
        self.headers = {} if content_length is None else {"Content-Length": str(content_length)}

    def read(self, size: int = -1) -> bytes:
        return self.data if size < 0 else self.data[:size]


class SecurityRegressionTests(unittest.TestCase):
    def test_remote_urls_reject_local_and_custom_schemes(self) -> None:
        self.assertEqual(UtilsMixin.validate_remote_http_url("https://example.com/feed"), "https://example.com/feed")
        for value in ("file:///C:/secret.txt", "javascript:alert(1)", "data:text/plain,test", "https:///missing-host"):
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                UtilsMixin.validate_remote_http_url(value)
        with mock.patch("apricot.utils.import_module") as import_module_mock:
            with self.assertRaises(RuntimeError):
                UtilsMixin.open_http_url_in_browser("file:///C:/secret.txt")
            import_module_mock.assert_not_called()

    def test_youtube_detection_rejects_domain_confusion(self) -> None:
        self.assertTrue(YoutubeMixin.is_youtube_url("https://www.youtube.com/watch?v=test"))
        self.assertTrue(YoutubeMixin.is_youtube_url("https://youtu.be/test"))
        self.assertFalse(YoutubeMixin.is_youtube_url("https://youtube.com.attacker.example/watch?v=test"))
        self.assertFalse(YoutubeMixin.is_youtube_url("https://notyoutube.com/watch?v=test"))
        self.assertFalse(YoutubeMixin.is_youtube_url("https://youtube.com@example.com/watch?v=test"))

    def test_xml_rejects_dtd_and_entities(self) -> None:
        self.assertEqual(UtilsMixin.parse_xml_bytes_safely(b"<rss><channel /></rss>").tag, "rss")
        malicious = b'<!DOCTYPE rss [<!ENTITY payload "expanded">]><rss>&payload;</rss>'
        with self.assertRaises(RuntimeError):
            UtilsMixin.parse_xml_bytes_safely(malicious)
        with self.assertRaises(RuntimeError):
            UtilsMixin.parse_xml_bytes_safely(malicious.decode("ascii").encode("utf-16"))
        with self.assertRaises(RuntimeError):
            UtilsMixin.parse_xml_bytes_safely(b'<!DOCTYPE rss SYSTEM "https://example.com/rss.dtd"><rss />')

    def test_limited_response_rejects_oversized_content(self) -> None:
        with self.assertRaises(RuntimeError):
            UtilsMixin.read_response_limited(FakeResponse(b"12345", content_length=5), 4)
        with self.assertRaises(RuntimeError):
            UtilsMixin.read_response_limited(FakeResponse(b"12345"), 4)
        self.assertEqual(UtilsMixin.read_response_limited(FakeResponse(b"1234"), 4), b"1234")

    def test_zip_validation_rejects_traversal_ads_devices_and_duplicates(self) -> None:
        for names in (
            ["../escape.txt"],
            ["folder/file.txt:payload.exe"],
            ["folder/NUL.txt"],
            ["A.txt", "a.txt"],
        ):
            with self.subTest(names=names):
                payload = io.BytesIO()
                with zipfile.ZipFile(payload, "w", zipfile.ZIP_DEFLATED) as archive:
                    for name in names:
                        archive.writestr(name, b"test")
                payload.seek(0)
                with zipfile.ZipFile(payload) as archive, self.assertRaises(RuntimeError):
                    SecurityHarness.validate_zip_archive(archive)

    def test_zip_validation_rejects_extreme_compression_ratio(self) -> None:
        payload = io.BytesIO()
        with zipfile.ZipFile(payload, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("large-zero-file.bin", b"\0" * (2 * 1024 * 1024))
        payload.seek(0)
        with zipfile.ZipFile(payload) as archive, self.assertRaises(RuntimeError):
            SecurityHarness.validate_zip_archive(archive)

    def test_zip_validation_supports_stricter_component_limits(self) -> None:
        payload = io.BytesIO()
        with zipfile.ZipFile(payload, "w", zipfile.ZIP_STORED) as archive:
            archive.writestr("yt_dlp/module.py", b"0123456789")
        payload.seek(0)
        with zipfile.ZipFile(payload) as archive, self.assertRaises(RuntimeError):
            SecurityHarness.validate_zip_archive(archive, max_uncompressed_bytes=5)

    def test_custom_cache_uses_app_owned_subdirectory(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            system = SystemUI()
            system.settings = SimpleNamespace(cache_folder=temp_dir)
            cache_path = system.cache_folder_path()
            self.assertEqual(cache_path.parent, Path(temp_dir))
            self.assertEqual(cache_path.name, CUSTOM_MPV_CACHE_SUBDIR)

            system.settings.cache_folder = str(DEFAULT_CACHE_DIR)
            self.assertEqual(system.cache_folder_path().resolve(), DEFAULT_CACHE_DIR.resolve())

            system.settings.cache_folder = temp_dir
            with mock.patch.object(system, "cache_path_is_link", return_value=True):
                self.assertEqual(system.cache_folder_path().resolve(), DEFAULT_CACHE_DIR.resolve())

    def test_local_folder_cache_is_bounded(self) -> None:
        cache = FolderCacheHarness()
        for index in range(LOCAL_FOLDER_CACHE_MAX_ENTRIES + 2):
            cache.cache_local_folder_items(Path(f"folder-{index}"), [{"title": str(index)}])
        self.assertEqual(len(cache.local_folder_cache), LOCAL_FOLDER_CACHE_MAX_ENTRIES)
        self.assertNotIn("folder-0", cache.local_folder_cache)
        self.assertIn(f"folder-{LOCAL_FOLDER_CACHE_MAX_ENTRIES + 1}", cache.local_folder_cache)

    def test_atomic_json_write_replaces_complete_file(self) -> None:
        manager = DataManagerMixin()
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "state.json"
            manager.atomic_write_json(path, {"value": 1})
            manager.atomic_write_json(path, {"value": 2})
            self.assertEqual(path.read_text(encoding="utf-8"), '{\n  "value": 2\n}')
            self.assertFalse(any(candidate.suffix == ".tmp" for candidate in path.parent.iterdir()))

    def test_saved_subscriptions_and_rss_feeds_sort_by_title(self) -> None:
        manager = DataManagerMixin()
        manager.load_json_list = lambda _path: [
            {"title": "zebra", "url": "https://example.test/z", "category": "Music"},
            {"title": "Alpha", "url": "https://example.test/a", "category": "Audio"},
            {"title": "beta", "url": "https://example.test/b", "category": "Music"},
        ]
        self.assertEqual([item["title"] for item in manager.load_subscriptions()], ["Alpha", "beta", "zebra"])
        self.assertEqual([item["title"] for item in manager.load_rss_feeds()], ["Alpha", "beta", "zebra"])

        manager.subscriptions = [{"title": "Zulu"}, {"title": "apple"}]
        manager.rss_feeds = [{"title": "Podcast", "category": "Shows"}, {"title": "Audiobook", "category": "Books"}]
        manager.current_rss_feed_index = 0
        manager.atomic_write_json = mock.Mock()
        manager.save_subscriptions()
        manager.save_rss_feeds()
        self.assertEqual([item["title"] for item in manager.subscriptions], ["apple", "Zulu"])
        self.assertEqual([item["title"] for item in manager.rss_feeds], ["Audiobook", "Podcast"])
        self.assertEqual(manager.current_rss_feed_index, 1)

    def test_collection_categories_filter_and_normalize(self) -> None:
        library = LibraryMixin()
        items = [
            {"title": "One", "category": "  Music  "},
            {"title": "Two", "category": "Podcasts"},
            {"title": "Four", "category": "music"},
            {"title": "Three"},
        ]
        self.assertEqual(library.normalized_collection_category("  Music\tLibrary  "), "Music Library")
        self.assertEqual([item["title"] for item in library.collection_items_for_category(items, "music")], ["One", "Four"])
        self.assertEqual(library.collection_category_names(items), ["Music", "Podcasts"])

    def test_result_metadata_columns_are_individual_and_complete(self) -> None:
        columns = ResultColumnsHarness().result_metadata_columns(
            {
                "title": "A track",
                "type": "Video",
                "channel": "A channel",
                "duration": "3:45",
                "views": "1,234",
                "age": "today",
                "album": "An album",
                "playlist_count": 12,
            }
        )
        self.assertEqual(
            columns,
            [
                ("media_field_title", "A track"),
                ("media_field_type", "Video"),
                ("media_field_channel", "A channel"),
                ("media_field_duration", "3:45"),
                ("media_field_views", "1,234"),
                ("media_field_uploaded", "today"),
                ("media_field_album", "An album"),
                ("media_field_playlist_count", "12"),
            ],
        )

    def test_soundcloud_search_uses_the_dedicated_ytdlp_search_path(self) -> None:
        search = SearchHarness()
        with mock.patch("apricot.search.search.wx.CallAfter", side_effect=lambda callback, *args: callback(*args)):
            search.search_worker("artist track", "Video", 5, 7, provider="soundcloud")
        self.assertEqual(search.requested_url, "scsearch5:artist track")
        self.assertEqual(search.shown_results, [{"title": "Track"}])

    def test_youtube_shorts_urls_resolve_to_playable_video_ids(self) -> None:
        youtube = YoutubeMixin()
        self.assertEqual(
            youtube.extract_youtube_video_id({"url": "https://www.youtube.com/shorts/AbCdEfGhI12"}),
            "AbCdEfGhI12",
        )

    def test_stale_last_session_save_is_ignored(self) -> None:
        manager = DataManagerMixin()
        manager.last_player_session_save_generation = 2
        with mock.patch.object(manager, "write_last_player_session_snapshot") as write_snapshot:
            manager.save_last_player_session_snapshot_worker({"title": "old"}, 1)
            write_snapshot.assert_not_called()
            manager.save_last_player_session_snapshot_worker({"title": "new"}, 2)
            write_snapshot.assert_called_once_with({"title": "new"})

    def test_update_scripts_are_random_and_reverify_hash(self) -> None:
        paths: list[Path] = []
        try:
            for _ in range(2):
                path = SecurityHarness.write_installer_update_script(
                    "C:/Temp/ApricotPlayerSetup.exe",
                    "C:/Program Files/ApricotPlayer",
                    123,
                    "C:/Temp/updater.log",
                    restart=False,
                    expected_sha256="a" * 64,
                )
                paths.append(path)
                text = path.read_text(encoding="utf-8-sig")
                self.assertIn("Get-FileHash", text)
                self.assertIn("Get-FileHash -InputStream", text)
                self.assertIn("[IO.FileShare]::Read", text)
                self.assertIn("a" * 64, text)
            self.assertNotEqual(paths[0], paths[1])
        finally:
            for path in paths:
                path.unlink(missing_ok=True)

    def test_file_update_scripts_keep_complete_rollback_copies(self) -> None:
        paths: list[Path] = []
        try:
            executable_script = SecurityHarness.write_update_script(
                "C:/Temp/ApricotPlayer.exe",
                "C:/Apps/ApricotPlayer/ApricotPlayer.exe",
                0,
                "C:/Temp/updater.log",
                restart=False,
                expected_sha256="a" * 64,
            )
            paths.append(executable_script)
            executable_text = executable_script.read_text(encoding="utf-8-sig")
            self.assertIn(".apricot-old-", executable_text)
            self.assertIn("Executable rollback failed", executable_text)

            portable_script = SecurityHarness.write_portable_zip_update_script(
                "C:/Temp/ApricotPlayer.zip",
                "C:/Apps/ApricotPlayer",
                "C:/Apps/ApricotPlayer/ApricotPlayer.exe",
                0,
                "C:/Temp/updater.log",
                restart=False,
                expected_sha256="b" * 64,
            )
            paths.append(portable_script)
            portable_text = portable_script.read_text(encoding="utf-8-sig")
            self.assertIn("Get-FileHash -InputStream", portable_text)
            self.assertIn("[IO.FileShare]::Read", portable_text)
            self.assertIn("$oldExeMoved", portable_text)
            self.assertIn("$oldInternalMoved", portable_text)
        finally:
            for path in paths:
                path.unlink(missing_ok=True)

    def test_portable_update_rejects_unexpected_root_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            package = Path(temp_dir) / "ApricotPlayer.zip"
            with zipfile.ZipFile(package, "w", zipfile.ZIP_STORED) as archive:
                archive.writestr("ApricotPlayer/ApricotPlayer.exe", b"MZ" + b"x" * 1_100_000)
                archive.writestr("ApricotPlayer/_internal/runtime.dat", b"runtime")
                archive.writestr("ApricotPlayer/unexpected.dll", b"unexpected")
            with self.assertRaises(RuntimeError):
                SecurityHarness.validate_update_package(package)

    def test_release_asset_requires_a_published_digest(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "update.bin"
            path.write_bytes(b"verified update")
            with self.assertRaises(RuntimeError):
                SecurityHarness.verify_release_asset_file({"size": path.stat().st_size}, path)
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            SecurityHarness.verify_release_asset_file(
                {"size": path.stat().st_size, "digest": f"sha256:{digest}"},
                path,
            )

    def test_converter_keeps_original_if_atomic_replace_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            folder = Path(temp_dir)
            source = folder / "song.wav"
            work = folder / ".song.apricot-converting.mp3"
            final = folder / "song.mp3"
            source.write_bytes(b"original")
            work.write_bytes(b"converted")
            with mock.patch("apricot.media.media.os.replace", side_effect=OSError("disk full")):
                with self.assertRaises(OSError):
                    MediaMixin.replace_converted_original(source, work, final)
            self.assertEqual(source.read_bytes(), b"original")
            self.assertEqual(work.read_bytes(), b"converted")

            MediaMixin.replace_converted_original(source, work, final)
            self.assertFalse(source.exists())
            self.assertEqual(final.read_bytes(), b"converted")

    def test_ipc_paths_are_unpredictable(self) -> None:
        first = UtilsMixin.make_ipc_path()
        second = UtilsMixin.make_ipc_path()
        self.assertNotEqual(first, second)
        self.assertIn(str(os.getpid()), first)

    def test_mpv_request_timeout_does_not_block_on_readline(self) -> None:
        player = MpvMixin()
        player.player_kind = "mpv"
        player.ipc_path = "test-pipe"
        player.mpv_ipc_lock = threading.Lock()
        pipe = io.BytesIO()
        started = time.monotonic()
        with mock.patch.object(player, "open_mpv_pipe", return_value=pipe), mock.patch.object(
            player, "mpv_pipe_available_bytes", return_value=0
        ):
            self.assertEqual(player.mpv_request(["get_property", "time-pos"], timeout=0.02), {})
        self.assertLess(time.monotonic() - started, 0.2)

    def test_devtools_websocket_must_stay_on_expected_loopback_port(self) -> None:
        valid = "ws://127.0.0.1:45678/devtools/browser/session"
        self.assertEqual(CookiesUI.validate_devtools_websocket_url(valid, 45678), valid)
        for value in (
            "ws://127.0.0.1:45679/devtools/browser/session",
            "ws://example.com:45678/devtools/browser/session",
            "ws://user@127.0.0.1:45678/devtools/browser/session",
        ):
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                CookiesUI.validate_devtools_websocket_url(value, 45678)

    def test_browser_cookie_export_discards_unrelated_sites(self) -> None:
        ui = CookiesUI()
        jar = ui.cdp_cookies_to_cookie_jar(
            [
                {"name": "SID", "value": "youtube-secret", "domain": ".google.com", "path": "/"},
                {"name": "session", "value": "bank-secret", "domain": ".bank.example", "path": "/"},
                {"name": "unsafe", "value": "value\n.evil.example\tTRUE\t/\tFALSE\t0\tsession\tsecret", "domain": ".youtube.com", "path": "/"},
            ]
        )
        cookies = {(cookie.domain, cookie.name, cookie.value) for cookie in jar}
        self.assertIn((".google.com", "SID", "youtube-secret"), cookies)
        self.assertFalse(any("bank" in domain for domain, _name, _value in cookies))
        self.assertFalse(any(name == "unsafe" for _domain, name, _value in cookies))
        self.assertFalse(CookiesUI.cookie_domain_matches(".google.com.attacker.example", "google.com"))

    def test_download_template_cannot_escape_selected_folder(self) -> None:
        default = DownloaderMixin.safe_download_filename_template("")
        self.assertEqual(DownloaderMixin.safe_download_filename_template("albums/%(title)s.%(ext)s"), "albums/%(title)s.%(ext)s")
        for value in ("../%(title)s.%(ext)s", "C:\\Temp\\%(title)s.%(ext)s", "/tmp/%(title)s.%(ext)s", "file.txt:stream", "CON.%(ext)s"):
            with self.subTest(value=value):
                self.assertEqual(DownloaderMixin.safe_download_filename_template(value), default)

    def test_generated_media_names_avoid_windows_devices_and_unsafe_extensions(self) -> None:
        self.assertEqual(MiscUI.safe_folder_name("CON.mp3"), "_CON.mp3")
        misc = MiscUI()
        misc.settings = SimpleNamespace(audio_format="mp3")
        misc.local_media_path_from_input = lambda _value: None
        self.assertEqual(misc.clip_output_extension("", {"ext": "../../outside"}), ".mp4")

    def test_local_media_rejects_windows_device_namespaces(self) -> None:
        for value in (r"\\.\PhysicalDrive0", r"\\.\pipe\name", r"\\?\GLOBALROOT\Device\Harddisk0", r"\\?\pipe\name"):
            with self.subTest(value=value):
                self.assertTrue(SystemUI.local_path_is_device_namespace(value))
        self.assertFalse(SystemUI.local_path_is_device_namespace(r"\\server\share\song.mp3"))
        self.assertFalse(SystemUI.local_path_is_device_namespace(r"\\?\C:\Music\song.mp3"))

    def test_redirect_host_validation(self) -> None:
        SecurityHarness.validate_https_response_url(
            "https://release-assets.githubusercontent.com/file",
            {"github.com", "githubusercontent.com"},
        )
        with self.assertRaises(RuntimeError):
            SecurityHarness.validate_https_response_url(
                "https://attacker.example/file",
                {"github.com", "githubusercontent.com"},
            )
        UtilsMixin.validate_trusted_https_url("https://www.googleapis.com/youtube/v3/videos", {"googleapis.com"})
        with self.assertRaises(RuntimeError):
            UtilsMixin.validate_trusted_https_url("https://googleapis.com.attacker.example/", {"googleapis.com"})
        UtilsMixin.validate_loopback_http_url("http://127.0.0.1:45678/json/version", 45678)
        with self.assertRaises(RuntimeError):
            UtilsMixin.validate_loopback_http_url("http://example.com:45678/json/version", 45678)

    def test_diagnostics_redact_tokens_and_user_paths(self) -> None:
        diagnostics = DiagnosticsMixin()
        home = str(Path.home())
        redacted = diagnostics.diagnostic_redact_text(
            f"Cookie: secret-value\nAuthorization: Bearer token\n{home}\\Music\\song.mp3\n"
            "https://example.com/media?sig=secret#access-token\n"
            "http://proxy-user:proxy-password@proxy.example:8080/media"
        )
        self.assertNotIn("secret-value", redacted)
        self.assertNotIn("Bearer token", redacted)
        self.assertNotIn(home.lower(), redacted.lower())
        self.assertNotIn("sig=secret", redacted)
        self.assertNotIn("access-token", redacted)
        self.assertNotIn("proxy-user", redacted)
        self.assertNotIn("proxy-password", redacted)

    @unittest.skipUnless(os.name == "nt", "Windows system executable test")
    def test_system_executable_uses_system32(self) -> None:
        tasklist = Path(UtilsMixin.windows_system_executable("tasklist.exe"))
        self.assertTrue(tasklist.is_absolute())
        self.assertEqual(tasklist.name.lower(), "tasklist.exe")
        self.assertEqual(tasklist.parent.name.lower(), "system32")


if __name__ == "__main__":
    unittest.main()
