import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from apricot.ui.player import PlayerUI


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


if __name__ == "__main__":
    unittest.main()
