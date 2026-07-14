import io
import os
import tempfile
import unittest
import zipfile
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import wx

import apricot.network.audiovault as audiovault_module
from apricot.network.audiovault import (
    AudioVaultRangeUnsupported,
    AudioVaultSessionExpired,
    AudioVaultMixin,
    _VaultPageParser,
    protect_audiovault_password,
    unprotect_audiovault_password,
)
from apricot.ui.events import EventsUI
from apricot.ui.lists import ListsUI
from apricot.ui.player import PlayerUI


class _EnterEvent:
    def __init__(self, key_code=wx.WXK_RETURN):
        self.skipped = False
        self.key_code = key_code

    def GetKeyCode(self):
        return self.key_code

    @staticmethod
    def ControlDown():
        return False

    @staticmethod
    def AltDown():
        return False

    @staticmethod
    def ShiftDown():
        return False

    def Skip(self):
        self.skipped = True


class _AudioVaultKeyHarness:
    def __init__(self, focus, results_focus=False, native_results_key=False):
        self.video_details = object()
        self.controls = {}
        self.in_main_menu = False
        self.in_player_screen = False
        self.player_control_mode = False
        self.current_video_item = {}
        self.audiovault_screen_active = True
        self.audiovault_menu_list = object() if results_focus else focus
        self.menu_list = object()
        self.results_list = focus if results_focus else object()
        self.results_focus = results_focus
        self.native_results_key = native_results_key
        self.calls = []

    @staticmethod
    def is_modifier_only_event(_event):
        return False

    @staticmethod
    def is_shortcut_capture_control(_focus):
        return False

    @staticmethod
    def focus_accepts_text(_focus):
        return False

    @staticmethod
    def handle_background_player_tab_navigation(_event, _focus):
        return False

    @staticmethod
    def handle_player_tab_navigation(_event, _focus):
        return False

    def focus_in_results_control(self, _focus):
        return self.results_focus

    @staticmethod
    def focus_in_media_list_control(_focus):
        return False

    @staticmethod
    def handle_global_navigation_shortcut(_event, _focus):
        return False

    @staticmethod
    def handle_active_player_global_shortcut_event(_event, _focus):
        return False

    def results_list_owns_key(self, _event):
        return self.native_results_key

    @staticmethod
    def result_details_key(_event):
        return False

    @staticmethod
    def player_details_shortcut_matches(_event):
        return False

    @staticmethod
    def handle_player_shortcut_event(_event, _focus, _details=False):
        return False

    @staticmethod
    def shortcut_matches(event, action):
        return action == "open_selected" and event.GetKeyCode() in {wx.WXK_RETURN, wx.WXK_NUMPAD_ENTER}

    def activate_audiovault_menu_item(self):
        self.calls.append("menu")

    def activate_audiovault_item(self):
        self.calls.append("audiovault_item")

    def play_selected(self):
        self.calls.append("generic_result")

    def maybe_extend_results(self):
        self.calls.append("extend_results")


class AudioVaultParserTests(unittest.TestCase):
    def test_global_enter_opens_selected_audiovault_menu_action(self):
        focus = object()
        harness = _AudioVaultKeyHarness(focus)
        event = _EnterEvent()

        with mock.patch("apricot.ui.events.wx.Window.FindFocus", return_value=focus):
            EventsUI.on_char_hook(harness, event)

        self.assertEqual(harness.calls, ["menu"])

    def test_global_enter_uses_audiovault_item_activation_not_generic_results(self):
        focus = object()
        harness = _AudioVaultKeyHarness(focus, results_focus=True)
        event = _EnterEvent()

        with mock.patch("apricot.ui.events.wx.Window.FindFocus", return_value=focus):
            EventsUI.on_char_hook(harness, event)

        self.assertEqual(harness.calls, ["audiovault_item"])

    def test_audiovault_native_navigation_never_requests_generic_dynamic_results(self):
        focus = object()
        harness = _AudioVaultKeyHarness(focus, results_focus=True, native_results_key=True)
        event = _EnterEvent(wx.WXK_DOWN)

        with (
            mock.patch("apricot.ui.events.wx.Window.FindFocus", return_value=focus),
            mock.patch("apricot.ui.events.wx.CallAfter", side_effect=lambda callback, *args: callback(*args)),
        ):
            EventsUI.on_char_hook(harness, event)

        self.assertTrue(event.skipped)
        self.assertNotIn("extend_results", harness.calls)

    def test_preparing_audiovault_screen_clears_generic_dynamic_fetch_state(self):
        class Harness(AudioVaultMixin):
            def __init__(self):
                self.in_main_menu = True
                self.in_player_screen = True
                self.search_screen_active = True
                self.audiovault_screen_active = False
                self.dynamic_fetch_enabled = True
                self.loading_more_results = True
                self.collection_url = "https://www.youtube.com/channel/example/videos"
                self.collection_result_type = "Video"
                self.collection_sort_mode = "popular"
                self.collection_channel_id = "channel"
                self.collection_fully_loaded = False
                self.pending_player_next_after_dynamic_load = True
                self.pending_player_next_preserve_focus = True
                self.pending_player_next_current_url = "https://www.youtube.com/watch?v=example"
                self.search_generation = 7

            @staticmethod
            def clear():
                pass

            @staticmethod
            def add_background_player_section():
                pass

        harness = Harness()
        harness.prepare_audiovault_screen("recent_shows")

        self.assertFalse(harness.dynamic_fetch_enabled)
        self.assertFalse(harness.loading_more_results)
        self.assertEqual(harness.collection_url, "")
        self.assertEqual(harness.collection_result_type, "")
        self.assertEqual(harness.collection_sort_mode, "")
        self.assertEqual(harness.collection_channel_id, "")
        self.assertTrue(harness.collection_fully_loaded)
        self.assertFalse(harness.pending_player_next_after_dynamic_load)
        self.assertFalse(harness.pending_player_next_preserve_focus)
        self.assertEqual(harness.pending_player_next_current_url, "")
        self.assertEqual(harness.search_generation, 8)

    def test_expired_stream_session_requests_login_and_retry(self):
        class Harness(AudioVaultMixin):
            def __init__(self):
                self.retry_callback = None
                self.messages = []

            @staticmethod
            def resolve_audiovault_stream(_url):
                raise AudioVaultSessionExpired("session expired")

            def retry_audiovault_after_login(self, callback):
                self.retry_callback = callback

            def message(self, *args):
                self.messages.append(args)

            @staticmethod
            def t(key, **values):
                return key.format(**values)

            @staticmethod
            def friendly_error(exc):
                return str(exc)

        harness = Harness()
        with mock.patch("apricot.network.audiovault.wx.CallAfter", side_effect=lambda callback, *args: callback(*args)):
            harness.play_audiovault_remote_worker({"url": "https://direct.audiovault.net/download/1", "title": "Movie"})

        self.assertIsNotNone(harness.retry_callback)
        self.assertEqual(harness.messages, [])

    def test_file_permission_error_does_not_trigger_audiovault_login(self):
        class Harness(AudioVaultMixin):
            def __init__(self):
                self.retry_callback = None
                self.messages = []

            @staticmethod
            def resolve_audiovault_stream(_url):
                raise PermissionError("download folder is not writable")

            def retry_audiovault_after_login(self, callback):
                self.retry_callback = callback

            def message(self, *args):
                self.messages.append(args)

            @staticmethod
            def t(key, **values):
                return key.format(**values)

            @staticmethod
            def friendly_error(exc):
                return str(exc)

        harness = Harness()
        with mock.patch("apricot.network.audiovault.wx.CallAfter", side_effect=lambda callback, *args: callback(*args)):
            harness.download_audiovault_movie_worker(
                {"url": "https://direct.audiovault.net/download/1", "title": "Movie"},
                Path("unused"),
            )

        self.assertIsNone(harness.retry_callback)
        self.assertEqual(len(harness.messages), 1)

    def test_catalog_redirect_to_login_requests_login_and_retry(self):
        class LoginResponse:
            headers = {"Content-Type": "text/html"}

            @staticmethod
            def geturl():
                return "https://direct.audiovault.net/login"

            @staticmethod
            def read(_size=-1):
                return b'<form><input name="password"></form>'

            def __enter__(self):
                return self

            @staticmethod
            def __exit__(_exc_type, _exc, _tb):
                return False

        class Harness(AudioVaultMixin):
            def __init__(self):
                self.retry_callback = None
                self.shown_results = None

            @staticmethod
            def audiovault_request(_url, **_kwargs):
                return LoginResponse()

            def retry_audiovault_after_login(self, callback):
                self.retry_callback = callback

            def show_audiovault_results(self, results):
                self.shown_results = results

            @staticmethod
            def t(key, **values):
                return key.format(**values)

            @staticmethod
            def friendly_error(exc):
                return str(exc)

        harness = Harness()
        with mock.patch("apricot.network.audiovault.wx.CallAfter", side_effect=lambda callback, *args: callback(*args)):
            harness.search_audiovault_worker("movie", "movies")

        self.assertIsNotNone(harness.retry_callback)
        self.assertIsNone(harness.shown_results)

    def test_missing_saved_credentials_open_login_dialog(self):
        class Harness(AudioVaultMixin):
            def __init__(self):
                self.audiovault_logged_in = False
                self.settings = SimpleNamespace(audiovault_email="", audiovault_password_protected="")
                self.after_login = None

            def show_audiovault_login(self, after_login=None):
                self.after_login = after_login

        callback = object()
        harness = Harness()

        self.assertFalse(harness.ensure_audiovault_login(callback))
        self.assertIs(harness.after_login, callback)

    @unittest.skipUnless(os.name == "nt", "AudioVault credentials use Windows DPAPI")
    def test_password_protection_round_trip_is_not_plaintext(self):
        password = "not-written-in-plain-text"
        protected = protect_audiovault_password(password)
        self.assertTrue(protected)
        self.assertNotIn(password, protected)
        self.assertEqual(unprotect_audiovault_password(protected), password)

    def test_parser_reads_result_rows_links_and_csrf_token(self):
        parser = _VaultPageParser()
        parser.feed(
            '<input type="hidden" name="_token" value="token123">'
            '<table><tr><td>42</td><td>A &amp; B</td><td>'
            '<a href="/download/42">Download</a></td></tr></table>'
        )
        self.assertEqual(parser.token, "token123")
        self.assertEqual(parser.rows, [["42", "A & B", "Download"]])
        self.assertIn("/download/42", parser.links)
        self.assertEqual(parser.records[0]["link"], "/download/42")

    def test_parser_keeps_recent_movies_and_shows_separate(self):
        parser = _VaultPageParser()
        parser.feed(
            "<h5>Recent Shows:</h5>"
            '<table><tr><td>11</td><td>A Show</td><td><a href="/download/11">Download</a></td></tr></table>'
            "<h5>Recent Movies:</h5>"
            '<table><tr><td>22</td><td>A Movie</td><td><a href="/download/22">Download</a></td></tr></table>'
        )

        self.assertEqual(
            [(record["section"], record["row"][1], record["link"]) for record in parser.records],
            [
                ("recent shows", "A Show", "/download/11"),
                ("recent movies", "A Movie", "/download/22"),
            ],
        )

    def test_show_results_passes_index_and_item_to_result_line(self):
        class Harness(AudioVaultMixin):
            def __init__(self):
                self.results_list = object()
                self.rendered = []

            def result_line(self, index, item):
                self.rendered.append((index, item["title"]))
                return item["title"]

            def set_listbox_items(self, _control, labels, _selection):
                self.labels = labels

            def set_status(self, _message):
                pass

            def focus_later(self, _control):
                pass

            @staticmethod
            def t(key, **values):
                return key.format(**values)

        harness = Harness()
        harness.show_audiovault_results([{"title": "One"}, {"title": "Two"}])

        self.assertEqual(harness.rendered, [(0, "One"), (1, "Two")])
        self.assertEqual(harness.labels, ["One", "Two"])

    def test_audiovault_search_url_uses_shows_route(self):
        self.assertEqual(
            AudioVaultMixin.audiovault_catalog_url("shows", "doctor who"),
            "https://direct.audiovault.net/shows?search=doctor+who",
        )

    def test_recent_results_only_use_the_requested_section(self):
        class Harness(AudioVaultMixin):
            @staticmethod
            def t(key, **values):
                return key.format(**values)

        records = [
            {"section": "recent shows", "row": ["11", "A Show"], "link": "/download/11"},
            {"section": "recent movies", "row": ["22", "A Movie"], "link": "/download/22"},
        ]

        shows = Harness().audiovault_results_from_records(records, "shows", section="recent shows")
        movies = Harness().audiovault_results_from_records(records, "movies", section="recent movies")

        self.assertEqual([(item["title"], item["kind"]) for item in shows], [("A Show", "audiovault_show")])
        self.assertEqual([(item["title"], item["kind"]) for item in movies], [("A Movie", "audiovault_movie")])

    def test_safe_zip_extraction_rejects_parent_paths(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "unsafe.zip"
            with zipfile.ZipFile(archive, "w") as package:
                package.writestr("../escape.mp3", b"not audio")
            with self.assertRaises(ValueError):
                AudioVaultMixin.safe_extract_audiovault_zip(archive, root / "output")
            self.assertFalse((root / "escape.mp3").exists())

    def test_safe_zip_extraction_accepts_nested_audio(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "show.zip"
            with zipfile.ZipFile(archive, "w") as package:
                package.writestr("Season 1/Episode 01.mp3", b"audio")
            output = root / "output"
            AudioVaultMixin.safe_extract_audiovault_zip(archive, output)
            self.assertEqual((output / "Season 1" / "Episode 01.mp3").read_bytes(), b"audio")

    def test_range_reader_lists_zip_without_fetching_the_whole_archive(self):
        payload = io.BytesIO()
        with zipfile.ZipFile(payload, "w", zipfile.ZIP_DEFLATED) as package:
            package.writestr("Season 1/Episode 01.mp3", b"audio" * 2000)
            package.writestr("Season 1/Episode 02.mp3", b"more audio" * 2000)
        archive = payload.getvalue()
        requests = []

        def read_range(start, end):
            requests.append((start, end))
            return archive[start : end + 1]

        reader = audiovault_module._AudioVaultRangeReader(len(archive), read_range, cache_size=128)
        with zipfile.ZipFile(reader) as package:
            names = package.namelist()

        self.assertEqual(names, ["Season 1/Episode 01.mp3", "Season 1/Episode 02.mp3"])
        self.assertTrue(requests)
        self.assertNotIn((0, len(archive) - 1), requests)

    def test_remote_show_manifest_creates_on_demand_episode_items(self):
        payload = io.BytesIO()
        with zipfile.ZipFile(payload, "w", zipfile.ZIP_DEFLATED) as package:
            package.writestr("Season 1/Episode 01.mp3", b"first episode")
            package.writestr("Season 1/Episode 02.mp3", b"second episode")
        archive = payload.getvalue()

        class Harness(AudioVaultMixin):
            def __init__(self, cache_folder):
                self.settings = SimpleNamespace(cache_folder=cache_folder)

            @staticmethod
            def t(key, **values):
                return key.format(**values)

            @staticmethod
            def safe_folder_name(value):
                return str(value).replace(":", "_")

            @staticmethod
            def natural_sort_key(value):
                return [str(value)]

            @staticmethod
            def audiovault_archive_size(_url):
                return len(archive)

            @staticmethod
            def audiovault_read_range(_url, start, end):
                return archive[start : end + 1]

        with tempfile.TemporaryDirectory() as temporary:
            show = {"id": "42", "title": "Example Show", "url": "https://direct.audiovault.net/download/42"}
            episodes = Harness(temporary).audiovault_remote_episode_items(show)

            self.assertEqual([item["title"] for item in episodes], ["Episode 01", "Episode 02"])
            self.assertTrue(all(item["kind"] == "audiovault_remote_episode" for item in episodes))
            self.assertEqual(episodes[0]["archive_member"], "Season 1/Episode 01.mp3")
            self.assertTrue(str(episodes[0]["url"]).endswith("0001 - Episode 01.mp3"))
            self.assertFalse(Path(episodes[0]["url"]).exists())

    def test_remote_episode_worker_extracts_only_selected_episode_before_playback(self):
        payload = io.BytesIO()
        with zipfile.ZipFile(payload, "w", zipfile.ZIP_DEFLATED) as package:
            package.writestr("Season 1/Episode 01.mp3", b"first episode")
            package.writestr("Season 1/Episode 02.mp3", b"second episode")
        archive = payload.getvalue()

        class Harness(AudioVaultMixin):
            def __init__(self, cache_folder):
                self.settings = SimpleNamespace(cache_folder=cache_folder, download_folder=cache_folder)
                self.played = []
                self.progress = []

            @staticmethod
            def t(key, **values):
                return key.format(**values)

            @staticmethod
            def safe_folder_name(value):
                return str(value).replace(":", "_")

            @staticmethod
            def natural_sort_key(value):
                return [str(value)]

            @staticmethod
            def audiovault_archive_size(_url):
                return len(archive)

            @staticmethod
            def audiovault_read_range(_url, start, end):
                return archive[start : end + 1]

            def update_audiovault_progress(self, task_id, percent, message):
                self.progress.append((task_id, percent, message))

            def close_audiovault_progress(self, task_id):
                self.progress.append((task_id, "closed", ""))

            def play_audiovault_local_item(self, item, **_kwargs):
                self.played.append(dict(item))

        with tempfile.TemporaryDirectory() as temporary:
            harness = Harness(temporary)
            show = {"id": "42", "title": "Example Show", "url": "https://direct.audiovault.net/download/42"}
            episodes = harness.audiovault_remote_episode_items(show)
            with mock.patch("apricot.network.audiovault.wx.CallAfter", side_effect=lambda callback, *args: callback(*args)):
                harness.audiovault_remote_episode_worker(episodes[0], False, True, "task-1")

            self.assertEqual(Path(episodes[0]["url"]).read_bytes(), b"first episode")
            self.assertFalse(Path(episodes[1]["url"]).exists())
            self.assertEqual(harness.played[0]["kind"], "audiovault_episode")
            self.assertEqual(harness.progress[-1][1], "closed")

    def test_show_manifest_falls_back_to_full_archive_with_progress_when_ranges_are_unavailable(self):
        class Harness(AudioVaultMixin):
            def __init__(self):
                self.audiovault_manifest_loading = {"42"}
                self.full_jobs = []

            @staticmethod
            def audiovault_remote_episode_items(_item):
                raise AudioVaultRangeUnsupported("no ranges")

            def start_audiovault_full_show_job(self, *args):
                self.full_jobs.append(args)

            @staticmethod
            def audiovault_show_manifest_key(_item):
                return "42"

            @staticmethod
            def t(key, **values):
                return key.format(**values)

            @staticmethod
            def friendly_error(exc):
                return str(exc)

        harness = Harness()
        item = {"id": "42", "title": "Show", "url": "https://direct.audiovault.net/download/42"}
        with mock.patch("apricot.network.audiovault.wx.CallAfter", side_effect=lambda callback, *args: callback(*args)):
            harness.load_audiovault_show_manifest_worker(item, Path("cache"), True, 9)

        self.assertEqual(len(harness.full_jobs), 1)
        self.assertEqual(harness.full_jobs[0][0], item)
        self.assertFalse(harness.audiovault_manifest_loading)

    def test_player_next_dispatches_remote_audiovault_episode_to_on_demand_loader(self):
        class Harness:
            def __init__(self):
                self.player_return_data = {}
                self.in_player_screen = True
                self.player_panel = object()
                self.results_list = object()
                self.calls = []

            @staticmethod
            def player_sequence_contains_item(_item):
                return True

            @staticmethod
            def live_window(control):
                return control

            @staticmethod
            def background_playback_enabled():
                return False

            def prepare_audiovault_remote_episode(self, item, **kwargs):
                self.calls.append((dict(item), kwargs))

        harness = Harness()
        item = {
            "kind": "audiovault_remote_episode",
            "title": "Episode 02",
            "url": "C:/cache/Episode 02.mp3",
        }
        PlayerUI.open_relative_player_item(harness, item, announce_start=True, preserve_focus=False)

        self.assertEqual(harness.calls[0][0], item)
        self.assertTrue(harness.calls[0][1]["show_player"])
        self.assertTrue(harness.calls[0][1]["announce_start"])

    def test_show_activation_uses_current_download_after_argument(self):
        class Harness(AudioVaultMixin):
            def selected_audiovault_item(self):
                return {"kind": "audiovault_show", "title": "Test show"}

            def prepare_audiovault_show(self, item, download_after=False):
                self.prepared = (item, download_after)

        harness = Harness()
        harness.activate_audiovault_item()
        self.assertEqual(harness.prepared[0]["title"], "Test show")
        self.assertFalse(harness.prepared[1])

    def test_audiovault_result_line_omits_uploaded_metadata(self):
        class Harness(ListsUI):
            @staticmethod
            def item_type_label(item):
                return item.get("type", "")

        line = Harness().result_line(
            0,
            {
                "kind": "audiovault_movie",
                "title": "Example movie",
                "type": "Movie",
                "age": "Uploaded 2 days ago",
            },
        )
        self.assertEqual(line, "Example movie | Movie")

    def test_metadata_hydration_continues_through_all_visible_results(self):
        class Harness(ListsUI):
            def __init__(self):
                self.results = [
                    {"kind": "video", "url": f"https://example.test/{index}"}
                    for index in range(12)
                ]
                self.metadata_hydration_urls = set()
                self.metadata_hydration_running = False
                self.search_generation = 4
                self.batches = []

            def result_metadata_worker(self, items, generation=None):
                self.batches.append([item["url"] for item in items])
                self.finish_result_metadata_hydration(generation)

        class ImmediateThread:
            def __init__(self, target, args=(), **_kwargs):
                self.target = target
                self.args = args

            def start(self):
                self.target(*self.args)

        harness = Harness()
        with mock.patch("apricot.ui.lists.threading.Thread", ImmediateThread):
            harness.start_result_metadata_hydration()

        self.assertEqual([len(batch) for batch in harness.batches], [5, 5, 2])
        self.assertEqual(len(harness.metadata_hydration_urls), 12)
        self.assertFalse(harness.metadata_hydration_running)

    def test_youtube_api_hydration_preserves_original_result_url(self):
        class Harness(ListsUI):
            @staticmethod
            def result_video_id(item):
                return str(item.get("id") or "")

            @staticmethod
            def fetch_youtube_api_videos_by_ids(video_ids):
                return [
                    {
                        "id": video_id,
                        "url": f"https://www.youtube.com/watch?v={video_id}",
                        "age": "Uploaded 1 day ago",
                        "timestamp": 123,
                        "view_count": 456,
                    }
                    for video_id in video_ids
                ]

        original_url = "https://www.youtube.com/watch?v=abcdefghijk&list=channel"
        hydrated, hydrated_ids = Harness().hydrate_results_with_youtube_api(
            [{"id": "abcdefghijk", "url": original_url, "title": "Original"}]
        )

        self.assertEqual(hydrated_ids, {"abcdefghijk"})
        self.assertEqual(hydrated[0]["url"], original_url)
        self.assertEqual(hydrated[0]["age"], "Uploaded 1 day ago")

    def test_soundcloud_results_do_not_enter_youtube_api_hydration(self):
        class Harness(ListsUI):
            @staticmethod
            def extract_youtube_video_id(item):
                return str(item.get("id") or "")

        self.assertEqual(
            Harness().result_video_id(
                {"id": "123456789012", "provider": "soundcloud", "url": "https://soundcloud.com/example/track"}
            ),
            "",
        )


if __name__ == "__main__":
    unittest.main()
