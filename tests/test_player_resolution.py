import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from apricot.ui.player import PlayerUI
from apricot.ui.misc import MiscUI
from apricot.ui.system import SystemUI
from apricot.ui.downloads import DownloadsUI


class PlayerResolutionTests(unittest.TestCase):
    def test_installed_internal_mpv_is_found_when_meipass_candidate_is_missing(self):
        class Harness(PlayerUI):
            settings = SimpleNamespace(player_command="")

            @staticmethod
            def bundled_path(*parts):
                return Path("Z:/missing-runtime").joinpath(*parts)

        with tempfile.TemporaryDirectory() as temporary:
            install = Path(temporary)
            executable = install / "ApricotPlayer.exe"
            executable.touch()
            mpv = install / "_internal" / "mpv" / "mpv.exe"
            mpv.parent.mkdir(parents=True)
            mpv.write_bytes(b"mpv")
            with patch("apricot.ui.player.sys.executable", str(executable)), patch("apricot.ui.player.shutil.which", return_value=None):
                resolved = Harness().resolve_player()
                self.assertIsNotNone(resolved)
                self.assertEqual(resolved[1], "mpv")
                self.assertTrue(Path(resolved[0]).samefile(mpv))

    def test_installed_internal_node_is_found_when_meipass_is_missing(self):
        class Harness(MiscUI):
            @staticmethod
            def bundled_path(*parts):
                return Path("Z:/missing-runtime").joinpath(*parts)

        with tempfile.TemporaryDirectory() as temporary:
            install = Path(temporary)
            executable = install / "ApricotPlayer.exe"
            executable.touch()
            node = install / "_internal" / "node" / "node.exe"
            node.parent.mkdir(parents=True)
            node.write_bytes(b"node")
            with patch("apricot.ui.misc.sys.executable", str(executable)), patch("apricot.ui.misc.shutil.which", return_value=None):
                resolved = Harness().bundled_node_executable()
                self.assertTrue(resolved)
                self.assertTrue(Path(resolved).samefile(node))

    def test_installed_internal_ffmpeg_is_found_when_meipass_is_missing(self):
        class Harness(MiscUI):
            settings = SimpleNamespace(ffmpeg_location="")

            @staticmethod
            def bundled_path(*parts):
                return Path("Z:/missing-runtime").joinpath(*parts)

        with tempfile.TemporaryDirectory() as temporary:
            install = Path(temporary)
            executable = install / "ApricotPlayer.exe"
            executable.touch()
            ffmpeg = install / "_internal" / "ffmpeg" / "ffmpeg.exe"
            ffmpeg.parent.mkdir(parents=True)
            ffmpeg.write_bytes(b"ffmpeg")
            with patch("apricot.ui.misc.sys.executable", str(executable)), patch("apricot.ui.misc.shutil.which", return_value=None):
                resolved = Harness().ffmpeg_executable()
                self.assertTrue(resolved)
                self.assertTrue(Path(resolved).samefile(ffmpeg))

    def test_direct_media_url_falls_back_to_direct_stream_when_ytdlp_fails(self):
        class Harness(SystemUI, DownloadsUI):
            settings = SimpleNamespace(enable_stream_cache=False, cache_folder="")

            def cached_stream_url(self, url):
                return None

            def local_media_path_from_input(self, url):
                return None

            def is_youtube_url(self, url):
                return False

            def is_cookie_auth_error(self, exc):
                return False

            def is_age_or_js_playback_error(self, exc):
                return False

            def is_requested_format_error(self, exc):
                return False

            def is_youtube_download_recoverable_error(self, exc):
                return False

            def age_restricted_video_support_enabled(self):
                return False

            def playback_cookies_file_for_url(self, url):
                return ""

            def ydl_extract_info(self, url, options=None, **kwargs):
                raise RuntimeError("generic extractor failed HTTP 403")

        url = "https://example.com/audio/sample.mp3"
        stream_url, headers, info = Harness().resolve_stream_url(url)
        self.assertEqual(stream_url, url)
        self.assertEqual(info.get("url"), url)
        self.assertEqual(info.get("title"), "sample.mp3")


if __name__ == "__main__":
    unittest.main()
