import unittest
from types import SimpleNamespace
from unittest import mock

import wx

from apricot.ui.misc import MiscUI
from apricot.ui.shortcuts import ShortcutsUI


class _KeyEvent:
    def __init__(self, action, key_code, *, control=False, shift=False, alt=False):
        self.action = action
        self.key_code = key_code
        self.control = control
        self.shift = shift
        self.alt = alt

    def GetKeyCode(self):
        return self.key_code

    def GetRawKeyCode(self):
        return self.key_code

    def ControlDown(self):
        return self.control

    def ShiftDown(self):
        return self.shift

    def AltDown(self):
        return self.alt


class _ShortcutHarness(ShortcutsUI):
    def __init__(self):
        self.player_control_mode = True
        self.settings = SimpleNamespace(volume_step=5)
        self.volume_changes = []
        self.pitch_changes = []
        self.bpm_requests = 0

    @staticmethod
    def player_shortcuts_allowed(_focus):
        return True

    @staticmethod
    def focus_in_results_control(_focus):
        return False

    @staticmethod
    def is_function_key_event(_event, _number):
        return False

    @staticmethod
    def context_menu_shortcut_matches(_event):
        return False

    @staticmethod
    def player_details_shortcut_matches(_event):
        return False

    @staticmethod
    def shortcut_matches(event, action):
        return event.action == action

    @staticmethod
    def pitch_step_value():
        return 0.05

    def announce_bpm_async(self):
        self.bpm_requests += 1

    def change_volume_async(self, delta):
        self.volume_changes.append(delta)

    def change_pitch_async(self, delta):
        self.pitch_changes.append(delta)


class PlayerKeyHoldTests(unittest.TestCase):
    def test_volume_and_pitch_use_native_key_repeat(self):
        harness = _ShortcutHarness()
        volume_event = _KeyEvent("player_volume_up", wx.WXK_UP)
        pitch_event = _KeyEvent("player_pitch_down", wx.WXK_DOWN, control=True)

        self.assertTrue(harness.handle_player_shortcut_event(volume_event, None))
        self.assertTrue(harness.handle_player_shortcut_event(pitch_event, None))

        self.assertEqual(harness.volume_changes, [5])
        self.assertEqual(harness.pitch_changes, [-0.05])

    def test_bpm_shortcut_runs_only_the_bpm_analyzer(self):
        harness = _ShortcutHarness()
        event = _KeyEvent("player_bpm", ord("B"))

        self.assertTrue(harness.handle_player_shortcut_event(event, None))

        self.assertEqual(harness.bpm_requests, 1)
        self.assertEqual(harness.volume_changes, [])
        self.assertEqual(harness.pitch_changes, [])

    def test_each_pitch_request_starts_its_own_worker(self):
        class DeferredThread:
            instances = []

            def __init__(self, target, args=(), **_kwargs):
                self.target = target
                self.args = args
                self.__class__.instances.append(self)

            @staticmethod
            def start():
                pass

        harness = MiscUI()

        with mock.patch("apricot.ui.misc.threading.Thread", DeferredThread):
            harness.change_pitch_async(0.01)
            harness.change_pitch_async(0.01)

        self.assertEqual(len(DeferredThread.instances), 2)
        self.assertEqual(DeferredThread.instances[0].target, harness.change_pitch_worker)
        self.assertEqual(DeferredThread.instances[0].args, (0.01,))
        self.assertEqual(DeferredThread.instances[1].target, harness.change_pitch_worker)
        self.assertEqual(DeferredThread.instances[1].args, (0.01,))


if __name__ == "__main__":
    unittest.main()
