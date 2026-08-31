import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from apricot.ui.player import PlayerUI
from apricot.ui.misc import MiscUI
from apricot.ui.system import (
    FAST_SEEK_AUDIO_ONLY_FORMAT,
    FAST_SEEK_STREAM_FORMAT,
    FAST_SEEK_VIDEO_ONLY_FORMAT,
    SystemUI,
)
from apricot.ui.downloads import DownloadsUI


class PlayerResolutionTests(unittest.TestCase):
    def test_automatic_stream_format_prefers_seekable_hls_with_full_quality_audio(self):
        self.assertTrue(FAST_SEEK_STREAM_FORMAT.startswith("bestvideo[protocol^=m3u8]"))
        self.assertIn("+bestaudio[protocol^=m3u8]", FAST_SEEK_STREAM_FORMAT)
        self.assertLess(FAST_SEEK_STREAM_FORMAT.index("18"), FAST_SEEK_STREAM_FORMAT.index("bestaudio[ext=m4a]"))

    def test_audio_stream_format_prefers_seekable_hls_before_direct_dash_audio(self):
        self.assertTrue(FAST_SEEK_AUDIO_ONLY_FORMAT.startswith("234/233/"))
        self.assertIn("bestaudio[protocol^=m3u8]", FAST_SEEK_AUDIO_ONLY_FORMAT)
        self.assertNotIn("bestaudio[ext=m4a]", FAST_SEEK_AUDIO_ONLY_FORMAT)

    def test_video_stream_format_keeps_separate_high_quality_hls_audio(self):
        self.assertIn("+bestaudio[protocol^=m3u8]", FAST_SEEK_VIDEO_ONLY_FORMAT)

    def test_requested_hls_video_and_audio_are_preserved_as_two_mpv_inputs(self):
        info = {
            "title": "Example",
            "webpage_url": "https://www.youtube.com/watch?v=example",
            "format_id": "230+234",
            "requested_formats": [
                {
                    "format_id": "230",
                    "url": "https://video.example/manifest.m3u8?expire=2000000000",
                    "protocol": "m3u8_native",
                    "ext": "mp4",
                    "height": 360,
                    "vcodec": "avc1.4D401E",
                    "acodec": "none",
                    "http_headers": {"User-Agent": "Apricot test"},
                },
                {
                    "format_id": "234",
                    "url": "https://audio.example/manifest.m3u8?expire=1999999900",
                    "protocol": "m3u8_native",
                    "ext": "mp4",
                    "vcodec": "none",
                    "acodec": None,
                    "resolution": "audio only",
                    "format": "234 - audio only (Default, high)",
                    "abr": 130,
                    "http_headers": {"User-Agent": "Apricot test"},
                },
            ],
        }

        selected = SystemUI.resolved_playable_stream_info(
            SystemUI(),
            info["webpage_url"],
            info,
            True,
        )

        self.assertEqual(selected["url"], info["requested_formats"][0]["url"])
        self.assertEqual(selected["format_id"], "230+234")
        self.assertEqual(selected["external_audio_url"], info["requested_formats"][1]["url"])
        self.assertEqual(selected["external_audio_format_id"], "234")
        self.assertEqual(selected["abr"], 130)

    def test_external_audio_url_is_passed_to_mpv(self):
        args = PlayerUI.external_audio_mpv_args(
            {
                "external_audio_url": "https://audio.example/manifest.m3u8",
            }
        )

        self.assertEqual(args, ["--audio-file=https://audio.example/manifest.m3u8"])

    def test_audio_bitrate_is_derived_from_hls_manifest_metadata(self):
        stream_url = (
            "https://manifest.example/audio/"
            "sgoap/clen%3D17882249%3Bdur%3D1104.898%3Bitag%3D140/playlist.m3u8"
        )

        bitrate = SystemUI.stream_url_audio_bitrate_kbps(stream_url)

        self.assertAlmostEqual(bitrate, 129.48, places=2)

    def test_recovery_prefers_audio_hls_over_direct_dash_audio(self):
        info = {
            "formats": [
                {
                    "format_id": "140",
                    "url": "https://audio.example/direct.m4a",
                    "protocol": "https",
                    "ext": "m4a",
                    "vcodec": "none",
                    "acodec": "mp4a.40.2",
                    "abr": 129,
                },
                {
                    "format_id": "234",
                    "url": "https://audio.example/manifest.m3u8",
                    "protocol": "m3u8_native",
                    "ext": "mp4",
                    "vcodec": "none",
                    "acodec": None,
                    "resolution": "audio only",
                    "format": "234 - audio only (Default, high)",
                },
            ],
        }

        selected = SystemUI.playable_stream_info(info, True)

        self.assertEqual(selected["format_id"], "234")

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
