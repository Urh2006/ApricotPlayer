from __future__ import annotations

import unittest
from types import SimpleNamespace

import wx

from apricot.library.library import LibraryMixin


class _Response:
    def __enter__(self):
        return self

    def __exit__(self, _exc_type, _exc, _tb) -> None:
        pass

    @staticmethod
    def geturl() -> str:
        return "https://podcast.example/feed.xml"


class _EpisodeList:
    def __init__(self, count: int, selection: int) -> None:
        self.labels = [f"Episode {index}" for index in range(count)]
        self.selection = selection

    def GetCount(self) -> int:
        return len(self.labels)

    def GetSelection(self) -> int:
        return self.selection


class _KeyEvent:
    def __init__(self, key_code: int) -> None:
        self.key_code = key_code
        self.skipped = False

    def GetKeyCode(self) -> int:
        return self.key_code

    def Skip(self) -> None:
        self.skipped = True


class PodcastEpisodeHarness(LibraryMixin):
    def __init__(self) -> None:
        self.settings = SimpleNamespace(rss_max_items=500)
        self.rss_all_items: list[dict] = []
        self.rss_items: list[dict] = []
        self.rss_items_list = _EpisodeList(0, wx.NOT_FOUND)
        self.status = ""

    @staticmethod
    def validate_remote_http_url(url: str, _label: str) -> str:
        return url

    @staticmethod
    def open_url(_request, timeout: int = 30) -> _Response:
        return _Response()

    @staticmethod
    def read_response_limited(_response, _limit: int, _label: str) -> bytes:
        return b"<rss />"

    @staticmethod
    def parse_xml_bytes_safely(_raw: bytes, _label: str):
        return object()

    @staticmethod
    def parse_feed_root(_root, _url: str):
        items = [{"title": f"Episode {index}", "url": f"https://media.example/{index}.mp3"} for index in range(809)]
        return "Archive", "https://podcast.example", items

    @staticmethod
    def t(key: str, **values) -> str:
        return key.format(**values)

    @staticmethod
    def shortcut_matches(_event, _action: str) -> bool:
        return False

    @staticmethod
    def context_menu_shortcut_matches(_event) -> bool:
        return False

    @staticmethod
    def rss_item_line(item: dict) -> str:
        return str(item.get("title") or "")

    def append_listbox_items(self, listbox, labels: list[str], previous_count: int, selection: int) -> bool:
        if listbox.GetCount() != previous_count:
            return False
        if isinstance(listbox, _EpisodeList):
            listbox.labels.extend(labels[previous_count:])
            listbox.selection = selection
        else:
            for label in labels[previous_count:]:
                listbox.Append(label)
            listbox.SetSelection(selection)
        return True

    def set_listbox_items(self, listbox, labels: list[str], selection: int) -> None:
        if isinstance(listbox, _EpisodeList):
            listbox.labels = list(labels)
            listbox.selection = selection
        else:
            listbox.Set(labels)
            listbox.SetSelection(selection)

    def set_status(self, text: str) -> None:
        self.status = text


class PodcastEpisodeLoadingTests(unittest.TestCase):
    def test_feed_keeps_every_episode_present_in_the_xml(self) -> None:
        harness = PodcastEpisodeHarness()

        feed = harness.fetch_rss_feed("https://podcast.example/feed.xml")

        self.assertEqual(len(feed["items"]), 809)

    def test_end_reveals_the_next_episode_batch(self) -> None:
        harness = PodcastEpisodeHarness()
        harness.rss_all_items = harness.parse_feed_root(None, "")[2]
        harness.rss_items = harness.rss_all_items[:500]
        harness.rss_items_list = _EpisodeList(500, 499)
        event = _KeyEvent(wx.WXK_END)

        harness.on_rss_item_key(event)

        self.assertEqual(len(harness.rss_items), 809)
        self.assertEqual(harness.rss_items_list.GetCount(), 809)
        self.assertTrue(event.skipped)

    def test_end_appends_the_next_batch_to_a_real_wx_list(self) -> None:
        app = wx.App.Get() or wx.App(False)
        frame = wx.Frame(None)
        try:
            harness = PodcastEpisodeHarness()
            harness.rss_all_items = harness.parse_feed_root(None, "")[2]
            harness.rss_items = harness.rss_all_items[:500]
            harness.rss_items_list = wx.ListBox(
                frame,
                choices=[harness.rss_item_line(item) for item in harness.rss_items],
            )
            harness.rss_items_list.SetSelection(499)
            event = _KeyEvent(wx.WXK_END)

            harness.on_rss_item_key(event)

            self.assertEqual(harness.rss_items_list.GetCount(), 809)
            self.assertEqual(harness.rss_items_list.GetSelection(), 499)
            self.assertTrue(event.skipped)
        finally:
            frame.Destroy()
            app.Yield()

    def test_older_archive_items_are_not_reported_as_new_episodes(self) -> None:
        harness = PodcastEpisodeHarness()
        existing = harness.parse_feed_root(None, "")[2][:500]
        refreshed = harness.parse_feed_root(None, "")[2]
        known_urls = {item["url"] for item in existing}

        self.assertEqual(harness.new_rss_entries(refreshed, known_urls), [])

        genuinely_new = [
            {"title": "New one", "url": "https://media.example/new-1.mp3"},
            {"title": "New two", "url": "https://media.example/new-2.mp3"},
        ]
        self.assertEqual(harness.new_rss_entries(genuinely_new + refreshed, known_urls), genuinely_new)


if __name__ == "__main__":
    unittest.main()
