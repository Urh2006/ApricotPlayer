import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from apricot.download.download import DownloaderMixin
from apricot.library.library import LibraryMixin


class DownloadLocationTests(unittest.TestCase):
    def test_audiovault_location_prompt_starts_in_audiovault_folder(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            class Harness(DownloaderMixin):
                settings = SimpleNamespace(download_folder=temp_dir)

                @staticmethod
                def safe_folder_name(value):
                    return str(value)

            harness = Harness()
            root = Path(temp_dir) / "AudioVault"

            self.assertEqual(
                harness.download_folder_for_item({"kind": "audiovault_movie", "title": "Movie"}),
                root,
            )
            self.assertEqual(
                harness.download_folder_for_item(
                    {"kind": "audiovault_remote_episode", "title": "Episode", "channel": "Show"}
                ),
                root / "Show",
            )
            self.assertEqual(
                harness.download_folder_for_item(
                    {"kind": "audiovault_show", "title": "Show"}, collection=True
                ),
                root / "Show",
            )

    def test_full_podcast_feed_uses_global_download_location_prompt(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            chosen = Path(temp_dir) / "chosen-feed"

            class Harness(LibraryMixin):
                settings = SimpleNamespace(ask_download_location_each_time=True)

                def __init__(self):
                    self.prompted = []

                def choose_download_target_folder(self, item, collection=False):
                    self.prompted.append((dict(item), collection))
                    return chosen

                @staticmethod
                def t(key, **_values):
                    return key

                @staticmethod
                def announce_player(_text):
                    pass

                @staticmethod
                def set_status(_text):
                    pass

                @staticmethod
                def register_download_task(_item, _audio_only, _kind, total=0):
                    return "task", object()

                @staticmethod
                def refresh_download_views():
                    pass

                @staticmethod
                def download_batch_worker(*_args):
                    pass

                @staticmethod
                def podcasts_download_folder():
                    return Path(temp_dir) / "default-podcasts"

                @staticmethod
                def safe_folder_name(value):
                    return str(value)

            feed = {
                "title": "Example feed",
                "items": [
                    {"title": "One", "url": "https://example.test/one.mp3"},
                    {"title": "Two", "url": "https://example.test/two.mp3"},
                ],
            }
            harness = Harness()

            with mock.patch("apricot.library.library.threading.Thread") as thread_class:
                harness.download_rss_feed(feed)

            self.assertEqual(len(harness.prompted), 1)
            self.assertTrue(harness.prompted[0][1])
            worker_args = thread_class.call_args.kwargs["args"]
            self.assertEqual(worker_args[4], str(chosen))
            self.assertTrue(
                all(item["download_folder_override"] == str(chosen) for item in worker_args[0])
            )

    def test_cancelling_podcast_feed_location_prompt_stops_download(self):
        class Harness(LibraryMixin):
            settings = SimpleNamespace(ask_download_location_each_time=True)

            def __init__(self):
                self.statuses = []
                self.registered = False

            @staticmethod
            def choose_download_target_folder(_item, collection=False):
                return None

            @staticmethod
            def t(key, **_values):
                return key

            @staticmethod
            def announce_player(_text):
                pass

            def set_status(self, text):
                self.statuses.append(text)

            def register_download_task(self, *_args, **_kwargs):
                self.registered = True
                return "task", object()

            @staticmethod
            def refresh_download_views():
                pass

            @staticmethod
            def download_batch_worker(*_args):
                pass

        harness = Harness()
        with mock.patch("apricot.library.library.threading.Thread") as thread_class:
            harness.download_rss_feed(
                {"title": "Feed", "items": [{"title": "One", "url": "https://example.test/one.mp3"}]}
            )

        self.assertFalse(harness.registered)
        thread_class.assert_not_called()
        self.assertEqual(harness.statuses, ["download_cancelled"])


if __name__ == "__main__":
    unittest.main()
