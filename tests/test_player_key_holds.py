import threading
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import wx

from apricot.constants import PITCH_MODE_MPV
from apricot.locales import TEXT
from apricot.models import Settings
from apricot.ui.misc import MiscUI
from apricot.ui.player import PlayerUI
from apricot.ui.settings import SettingsMixin
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
        self.volume_changes = []
        self.bpm_requests = 0
        self.format_status_requests = 0

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
    def speed_step_value():
        return 0.10

    @staticmethod
    def pitch_step_value():
        return 0.05

    def announce_bpm_async(self):
        self.bpm_requests += 1

    def announce_format_status_async(self):
        self.format_status_requests += 1

    def start_player_adjustment_hold(self, action, delta, event):
        self.holds.append((action, delta, event))

    def change_volume_async(self, delta):
        self.volume_changes.append(delta)

    @staticmethod
    def change_speed_async(_delta):
        raise AssertionError("Speed bypassed the hold controller")

    @staticmethod
    def change_pitch_async(_delta):
        raise AssertionError("Pitch bypassed the hold controller")


class _HoldHarness(PlayerUI):
    def __init__(self):
        self.settings = Settings()
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
        self.speed_changes = []
        self.pitch_changes = []

    @staticmethod
    def player_is_active():
        return True

    def key_state_down(self, key_code):
        return bool(self.key_states.get(key_code, False))

    def change_speed_async(self, delta):
        self.speed_changes.append(delta)

    def change_pitch_async(self, delta):
        self.pitch_changes.append(delta)


class _DeferredThread:
    instances = []

    def __init__(self, target, args=(), **_kwargs):
        self.target = target
        self.args = args
        self.__class__.instances.append(self)

    @staticmethod
    def start():
        pass


class _RateChangeHarness(MiscUI):
    def __init__(self):
        self.settings = SimpleNamespace(player_speed="1.0")
        self.current_video_info = {"speed": "1.0", "pitch": "1.0"}
        self.speed_change_lock = threading.Lock()
        self.speed_change_pending_target = None
        self.speed_change_worker_running = False
        self.speed_change_generation = 0
        self.pitch_change_lock = threading.Lock()
        self.pitch_change_pending_target = None
        self.pitch_change_worker_running = False
        self.pitch_change_generation = 0
        self.applied_speeds = []
        self.applied_pitches = []

    @staticmethod
    def normalized_pitch_mode():
        return PITCH_MODE_MPV

    @staticmethod
    def next_playback_speed(current, delta):
        return MiscUI.clamp_rate(current + delta, 0.25, 4.0)

    def apply_speed_target_worker(self, speed):
        self.applied_speeds.append(speed)
        self.current_video_info["speed"] = str(speed)
        return True

    def apply_pitch_target_worker(self, pitch):
        self.applied_pitches.append(pitch)
        self.current_video_info["pitch"] = str(pitch)
        return True


class PlayerKeyHoldTests(unittest.TestCase):
    def setUp(self):
        _FakeCallLater.instances = []
        _DeferredThread.instances = []

    def test_volume_keeps_native_key_repeat(self):
        harness = _ShortcutHarness()
        event = _KeyEvent("player_volume_up", wx.WXK_UP)

        self.assertTrue(harness.handle_player_shortcut_event(event, None))

        self.assertEqual(harness.volume_changes, [5])
        self.assertEqual(harness.holds, [])

    def test_pitch_and_speed_shortcuts_enter_hold_controller(self):
        harness = _ShortcutHarness()
        pitch_event = _KeyEvent("player_pitch_down", wx.WXK_DOWN, control=True)
        speed_event = _KeyEvent("player_speed_up", ord("D"))

        self.assertTrue(harness.handle_player_shortcut_event(pitch_event, None))
        self.assertTrue(harness.handle_player_shortcut_event(speed_event, None))

        self.assertEqual(harness.holds[0], ("pitch", -0.05, pitch_event))
        self.assertEqual(harness.holds[1], ("speed", 0.10, speed_event))

    def test_pitch_hold_uses_beta_60_timing_and_ignores_native_repeat(self):
        harness = _HoldHarness()
        harness.key_states[wx.WXK_UP] = True
        harness.key_states[wx.WXK_CONTROL] = True
        event = _KeyEvent("player_pitch_up", wx.WXK_UP, control=True)

        with mock.patch("apricot.ui.player.wx.CallLater", _FakeCallLater):
            harness.start_player_adjustment_hold("pitch", 0.05, event)
            harness.start_player_adjustment_hold("pitch", 0.05, event)
            first_timer = _FakeCallLater.instances[-1]
            first_timer.callback(*first_timer.args)
            repeat_timer = _FakeCallLater.instances[-1]

        self.assertEqual(harness.pitch_changes, [0.05, 0.05])
        self.assertEqual(first_timer.delay, 180)
        self.assertEqual(repeat_timer.delay, 110)
        self.assertEqual(len(_FakeCallLater.instances), 2)

    def test_user_can_restore_beta_61_hold_timing(self):
        harness = _HoldHarness()
        harness.settings.speed_pitch_hold_delay_ms = 90
        harness.settings.speed_pitch_hold_interval_ms = 45
        harness.key_states[wx.WXK_UP] = True
        harness.key_states[wx.WXK_CONTROL] = True
        event = _KeyEvent("player_pitch_up", wx.WXK_UP, control=True)

        with mock.patch("apricot.ui.player.wx.CallLater", _FakeCallLater):
            harness.start_player_adjustment_hold("pitch", 0.05, event)
            first_timer = _FakeCallLater.instances[-1]
            first_timer.callback(*first_timer.args)
            repeat_timer = _FakeCallLater.instances[-1]

        self.assertEqual(first_timer.delay, 90)
        self.assertEqual(repeat_timer.delay, 45)

    def test_hold_timing_settings_are_bounded(self):
        harness = _HoldHarness()
        harness.settings.speed_pitch_hold_delay_ms = 1
        harness.settings.speed_pitch_hold_interval_ms = 5000

        self.assertEqual(harness.speed_pitch_hold_delay_ms(), 50)
        self.assertEqual(harness.speed_pitch_hold_interval_ms(), 500)

    def test_hold_timing_controls_are_part_of_playback_settings(self):
        fields = SettingsMixin.settings_section_fields()["playback"]

        self.assertEqual(Settings().speed_pitch_hold_delay_ms, 180)
        self.assertEqual(Settings().speed_pitch_hold_interval_ms, 110)
        self.assertIn("speed_pitch_hold_delay_ms", fields)
        self.assertIn("speed_pitch_hold_interval_ms", fields)
        for language in ("en", "sl"):
            self.assertIn("speed_pitch_hold_delay_ms", TEXT[language])
            self.assertIn("speed_pitch_hold_interval_ms", TEXT[language])

    def test_speed_hold_repeats_until_key_up(self):
        harness = _HoldHarness()
        harness.key_states[ord("D")] = True
        event = _KeyEvent("player_speed_up", ord("D"))

        with mock.patch("apricot.ui.player.wx.CallLater", _FakeCallLater):
            harness.start_player_adjustment_hold("speed", 0.10, event)
            first_timer = _FakeCallLater.instances[-1]
            first_timer.callback(*first_timer.args)
            repeat_timer = _FakeCallLater.instances[-1]
            harness.on_player_key_up(event)
            repeat_timer.callback(*repeat_timer.args)

        self.assertEqual(harness.speed_changes, [0.10, 0.10])
        self.assertEqual(first_timer.delay, 180)
        self.assertEqual(repeat_timer.delay, 110)
        self.assertFalse(harness.adjustment_hold_active)
        self.assertFalse(repeat_timer.running)

    def test_rapid_speed_and_pitch_changes_use_one_worker_each(self):
        harness = _RateChangeHarness()

        with mock.patch("apricot.ui.misc.threading.Thread", _DeferredThread):
            for _index in range(10):
                harness.change_speed_async(0.01)
                harness.change_pitch_async(0.01)

        self.assertEqual(len(_DeferredThread.instances), 2)
        for thread in _DeferredThread.instances:
            thread.target(*thread.args)
        self.assertEqual(harness.applied_speeds, [1.10])
        self.assertEqual(harness.applied_pitches, [1.10])

    def test_bpm_shortcut_runs_only_the_bpm_analyzer(self):
        harness = _ShortcutHarness()
        event = _KeyEvent("player_bpm", ord("B"))

        self.assertTrue(harness.handle_player_shortcut_event(event, None))

        self.assertEqual(harness.bpm_requests, 1)
        self.assertEqual(harness.volume_changes, [])
        self.assertEqual(harness.holds, [])

    def test_format_status_shortcut_triggers_format_announcement(self):
        harness = _ShortcutHarness()
        event = _KeyEvent("player_format_status", ord("F"))

        self.assertTrue(harness.handle_player_shortcut_event(event, None))

        self.assertEqual(harness.format_status_requests, 1)
        self.assertEqual(harness.volume_changes, [])
        self.assertEqual(harness.holds, [])

    def test_local_edit_audio_filters_uses_high_quality_rubberband_for_pitch(self):
        class _FilterHarness(PlayerUI, MiscUI):
            def __init__(self):
                self.current_video_info = {"speed": "1.0", "pitch": "1.10"}
                self.settings = SimpleNamespace(player_speed="1.0")

            def effective_equalizer_state(self):
                return False, {}

        harness = _FilterHarness()
        filters = harness.local_edit_audio_filters()

        self.assertEqual(filters, ["rubberband=pitch=1.100000:pitchq=quality"])

    def test_local_edit_audio_filters_uses_rubberband_tempo_for_combined_pitch_speed(self):
        class _FilterHarness(PlayerUI, MiscUI):
            def __init__(self):
                self.current_video_info = {"speed": "1.25", "pitch": "1.15"}
                self.settings = SimpleNamespace(player_speed="1.25")

            def effective_equalizer_state(self):
                return False, {}

        harness = _FilterHarness()
        filters = harness.local_edit_audio_filters()

        self.assertEqual(filters, ["rubberband=pitch=1.150000:tempo=1.250000:pitchq=quality"])

    def test_local_edit_audio_filters_uses_atempo_for_pure_speed(self):
        class _FilterHarness(PlayerUI, MiscUI):
            def __init__(self):
                self.current_video_info = {"speed": "1.50", "pitch": "1.0"}
                self.settings = SimpleNamespace(player_speed="1.50")

            def effective_equalizer_state(self):
                return False, {}

        harness = _FilterHarness()
        filters = harness.local_edit_audio_filters()

        self.assertEqual(filters, ["atempo=1.500000"])

    def test_local_edit_mpv_args_uses_mpv_built_in_pitch_and_speed(self):
        class _MpvHarness(PlayerUI, MiscUI):
            def __init__(self):
                self.current_video_info = {"speed": "1.10", "pitch": "1.20"}
                self.settings = SimpleNamespace(player_speed="1.10")

            def effective_equalizer_state(self):
                return False, {}

        harness = _MpvHarness()
        args = harness.local_edit_mpv_args("mpv.exe", Path("music.mp3"), Path("music_edited.mp3"))

        self.assertIn("--audio-pitch-correction=yes", args)
        self.assertIn("--pitch=1.200000", args)
        self.assertIn("--speed=1.100000", args)
        self.assertIn("--oac=libmp3lame", args)
        self.assertIn("--oacopts=b=320k", args)
        self.assertIn("--video=no", args)
        self.assertEqual(args[-1], "--o=music_edited.mp3")


if __name__ == "__main__":
    unittest.main()
