import os
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock

from apricot.network.audiovault import (
    AudioVaultMixin,
    _VaultPageParser,
    protect_audiovault_password,
    unprotect_audiovault_password,
)
from apricot.ui.lists import ListsUI


class AudioVaultParserTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
