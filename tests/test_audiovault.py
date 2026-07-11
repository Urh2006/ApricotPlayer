import tempfile
import unittest
import zipfile
from pathlib import Path

from apricot.network.audiovault import AudioVaultMixin, _VaultPageParser


class AudioVaultParserTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
