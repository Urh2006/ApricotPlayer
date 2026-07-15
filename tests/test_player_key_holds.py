import threading
import unittest
from types import SimpleNamespace
from unittest import mock

import wx

from apricot.constants import PITCH_MODE_MPV
from apricot.ui.misc import MiscUI
from apricot.ui.player import PlayerUI
from apricot.ui.shortcuts import ShortcutsUI


class _KeyEvent:
    def __init__(self, action, key_code, *, control=False, shift=False, alt=False):
        self.action = action
        self.key_code = key_code
        self.control = control
        self.shift = shift
        self.alt = alt
        self.skipped = False

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

    def Skip(self):
        self.skipped = True


class _FakeCallLater:
    instances = []

    def __init__(self, delay, callback, *args):
        self.delay = delay
        self.callback = callback
        self.args = args
        self.running = True
        self.__class__.instances.append(self)

    def IsRunning(self):
        return self.running

    def Stop(self):
        self.running = False


class _ShortcutHarness(ShortcutsUI):
    def __init__(self):
        self.player_control_mode = True
        self.settings = SimpleNamespace(volume_step=5)
        self.holds = []

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

    def start_player_adjustment_hold(self, action, delta, event):
        self.holds.append((action, delta, event))

    @staticmethod
    def change_volume_async(_delta):
        raise AssertionError("Volume bypassed the hold controller")

    @staticmethod
    def change_pitch_async(_delta):
        raise AssertionError("Pitch bypassed the hold controller")


class _HoldHarness(PlayerUI):
    def __init__(self):
        self.adjustment_hold_active = False
        self.adjustment_hold_generation = 0
        self.adjustment_hold_action = ""
        self.adjustment_hold_delta = 0.0
        self.adjustment_hold_key_code = -1
        self.adjustment_hold_raw_key_code = -1
        self.adjustment_hold_ctrl = False
        self.adjustment_hold_shift = False
        self.adjustment_hold_alt = False
        self.adjustment_hold_call = None
        self.key_states = {}
        self.volume_changes = []
        self.pitch_changes = []

    @staticmethod
    def player_is_active():
        return True

    def key_state_down(self, key_code):
        return bool(self.key_states.get(key_code, False))

    def change_volume_async(self, delta):
        self.volume_changes.append(delta)

    def change_pitch_async(self, delta):
        self.pitch_changes.append(delta)


class PlayerKeyHoldTests(unittest.TestCase):
    def setUp(self):
        _FakeCallLater.instances = []

    def test_volume_and_pitch_shortcuts_enter_hold_controller(self):
        harness = _ShortcutHarness()
        volume_event = _KeyEvent("player_volume_up", wx.WXK_UP)
        pitch_event = _KeyEvent("player_pitch_down", wx.WXK_DOWN, control=True)

        self.assertTrue(harness.handle_player_shortcut_event(volume_event, None))
        self.assertTrue(harness.handle_player_shortcut_event(pitch_event, None))

        self.assertEqual(harness.holds[0], ("volume", 5, volume_event))
        self.assertEqual(harness.holds[1], ("pitch", -0.05, pitch_event))

    def test_short_press_changes_volume_once_and_key_up_cancels_repeat(self):
        harness = _HoldHarness()
        event = _KeyEvent("player_volume_up", wx.WXK_UP)

        with mock.patch("apricot.ui.player.wx.CallLater", _FakeCallLater):
            harness.start_player_adjustment_hold("volume", 5, event)
            harness.on_player_key_up(event)
            _FakeCallLater.instances[0].callback(*_FakeCallLater.instances[0].args)

        self.assertEqual(harness.volume_changes, [5])
        self.assertFalse(harness.adjustment_hold_active)

    def test_held_volume_repeats_without_native_key_repeat_events(self):
        harness = _HoldHarness()
        harness.key_states[wx.WXK_UP] = True
        event = _KeyEvent("player_volume_up", wx.WXK_UP)

        with mock.patch("apricot.ui.player.wx.CallLater", _FakeCallLater):
            harness.start_player_adjustment_hold("volume", 5, event)
            first_timer = _FakeCallLater.instances[-1]
            first_timer.callback(*first_timer.args)
            repeat_timer = _FakeCallLater.instances[-1]
            repeat_timer.callback(*repeat_timer.args)

        self.assertEqual(harness.volume_changes, [5, 5, 5])
        self.assertEqual(first_timer.delay, 180)
        self.assertEqual(repeat_timer.delay, 110)

    def test_native_repeat_does_not_double_custom_pitch_hold(self):
        harness = _HoldHarness()
        harness.key_states[wx.WXK_UP] = True
        harness.key_states[wx.WXK_CONTROL] = True
        event = _KeyEvent("player_pitch_up", wx.WXK_UP, control=True)

        with mock.patch("apricot.ui.player.wx.CallLater", _FakeCallLater):
            harness.start_player_adjustment_hold("pitch", 0.05, event)
            harness.start_player_adjustment_hold("pitch", 0.05, event)
            timer = _FakeCallLater.instances[-1]
            timer.callback(*timer.args)

        self.assertEqual(harness.pitch_changes, [0.05, 0.05])
        self.assertEqual(len(_FakeCallLater.instances), 2)

    def test_rapid_pitch_changes_accumulate_instead_of_racing(self):
        count = 10

        class DeferredThread:
            instances = []

            def __init__(self, target, args=(), **_kwargs):
                self.target = target
                self.args = args
                self.__class__.instances.append(self)

            @staticmethod
            def start():
                pass

        class Harness(MiscUI):
            def __init__(self):
                self.current_video_info = {"pitch": "1.0"}
                self.pitch_change_lock = threading.Lock()
                self.pitch_change_pending_target = None
                self.pitch_change_worker_running = False
                self.pitch_change_generation = 0

            @staticmethod
            def normalized_pitch_mode():
                return PITCH_MODE_MPV

            def apply_pitch_value(self, pitch, speed_delta=None):
                self.current_video_info["pitch"] = str(pitch)

            @staticmethod
            def mpv_process_alive():
                return True

            @staticmethod
            def announce_player(_text):
                pass

            @staticmethod
            def t(key, **_values):
                return key

            @staticmethod
            def format_rate_for_speech(value):
                return str(value)

            @staticmethod
            def is_default_rate(_value):
                return False

            @staticmethod
            def play_default_sound():
                pass

            @staticmethod
            def update_details_text():
                pass

        harness = Harness()
        with (
            mock.patch("apricot.ui.misc.threading.Thread", DeferredThread),
            mock.patch("apricot.ui.misc.wx.CallAfter", lambda fn, *args: fn(*args)),
        ):
            for _index in range(count):
                harness.change_pitch_async(0.01)
            self.assertEqual(len(DeferredThread.instances), 1)
            DeferredThread.instances[0].target(*DeferredThread.instances[0].args)

        self.assertAlmostEqual(harness.current_pitch_value(), 1.10, places=2)

    def test_cancelled_pitch_worker_cannot_change_the_next_player(self):
        class DeferredThread:
            instances = []

            def __init__(self, target, args=(), **_kwargs):
                self.target = target
                self.args = args
                self.__class__.instances.append(self)

            @staticmethod
            def start():
                pass

        class Harness(MiscUI):
            def __init__(self):
                self.current_video_info = {"pitch": "1.0"}
                self.pitch_change_lock = threading.Lock()
                self.pitch_change_pending_target = None
                self.pitch_change_worker_running = False
                self.pitch_change_generation = 0

            @staticmethod
            def normalized_pitch_mode():
                return PITCH_MODE_MPV

            def apply_pitch_value(self, pitch, speed_delta=None):
                self.current_video_info["pitch"] = str(pitch)

            @staticmethod
            def mpv_process_alive():
                return True

            @staticmethod
            def announce_player(_text):
                pass

            @staticmethod
            def t(key, **_values):
                return key

            @staticmethod
            def format_rate_for_speech(value):
                return str(value)

            @staticmethod
            def is_default_rate(_value):
                return False

            @staticmethod
            def play_default_sound():
                pass

            @staticmethod
            def update_details_text():
                pass

        harness = Harness()
        with mock.patch("apricot.ui.misc.threading.Thread", DeferredThread):
            harness.change_pitch_async(0.05)
            harness.cancel_pending_pitch_changes()
            DeferredThread.instances[0].target(*DeferredThread.instances[0].args)

        self.assertEqual(harness.current_pitch_value(), 1.0)
        self.assertFalse(harness.pitch_change_worker_running)


if __name__ == "__main__":
    unittest.main()
