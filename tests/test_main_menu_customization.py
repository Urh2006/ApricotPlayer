import unittest

from apricot.constants import MAIN_MENU_CUSTOMIZABLE_IDS
from apricot.models import Settings
from apricot.ui.menus import MenusUI


class MainMenuHarness(MenusUI):
    def __init__(self, hidden=None, pending_update=""):
        self.settings = Settings()
        self.settings.main_menu_hidden_actions = list(hidden or [])
        self.pending_update = pending_update
        self.download_queue = {}
        self.active_downloads = {}
        self.playback_queue = []
        self._handlers = {}

    def __getattr__(self, name):
        if name.startswith(("show_", "open_", "copy_", "quit_", "resume_")):
            return self._handlers.setdefault(name, lambda: None)
        raise AttributeError(name)

    @staticmethod
    def t(key, **values):
        return key.format(**values)

    @staticmethod
    def label_with_shortcut(label, _action, _separator=" "):
        return label

    def menu_label_with_shortcut(self, label_key, _action):
        return self.t(label_key)

    def pending_app_update_version(self):
        return self.pending_update

    @staticmethod
    def last_player_session_available():
        return False

    @staticmethod
    def player_is_active():
        return False


class MainMenuCustomizationTests(unittest.TestCase):
    def test_every_catalog_item_is_enabled_by_default(self):
        harness = MainMenuHarness()

        self.assertEqual(Settings().main_menu_hidden_actions, [])
        self.assertNotIn("app_update", MAIN_MENU_CUSTOMIZABLE_IDS)
        self.assertEqual(
            [action_id for action_id, _label in harness.main_menu_customization_options()],
            list(MAIN_MENU_CUSTOMIZABLE_IDS),
        )

    def test_available_update_is_always_visible(self):
        harness = MainMenuHarness({"app_update"}, pending_update="1.0.0-beta.54")

        labels = [label for label, _handler in harness.build_main_menu_actions()]

        self.assertEqual(labels[0], "app_update_menu_item")

    def test_hidden_items_are_removed_but_settings_and_exit_remain(self):
        harness = MainMenuHarness({"search", "audiovault", "diagnostic_report"})

        labels = [label for label, _handler in harness.build_main_menu_actions()]

        self.assertNotIn("search_youtube / soundcloud", labels)
        self.assertNotIn("search_audiovault", labels)
        self.assertNotIn("copy_diagnostic_report", labels)
        self.assertIn("settings", labels)
        self.assertIn("exit", labels)

    def test_hiding_menu_item_does_not_remove_action_finder_action(self):
        harness = MainMenuHarness({"audiovault"})

        main_labels = [label for label, _handler in harness.build_main_menu_actions()]
        finder_labels = [label for label, _handler in harness.action_finder_actions()]

        self.assertNotIn("search_audiovault", main_labels)
        self.assertIn("search_audiovault", finder_labels)

    def test_dynamic_items_respect_visibility_when_available(self):
        harness = MainMenuHarness({"current_downloads", "playback_queue"})
        harness.download_queue = {"one": {}}
        harness.playback_queue = [{"title": "Queued"}]

        labels = [label for label, _handler in harness.build_main_menu_actions()]

        self.assertFalse(any(label.startswith("current_downloads") for label in labels))
        self.assertFalse(any(label.startswith("playback_queue") for label in labels))


if __name__ == "__main__":
    unittest.main()
