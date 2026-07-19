from __future__ import annotations

from array import array
import math
import random
import threading
import unittest
from types import SimpleNamespace
from unittest import mock

from apricot.constants import DEFAULT_KEYBOARD_SHORTCUTS, SHORTCUT_DEFINITIONS
from apricot.media.tempo import estimate_tempo_from_pcm16_stereo
from apricot.ui.misc import MiscUI


SAMPLE_RATE = 11025


def stereo_pcm_for_click_track(bpm: float, seconds: float = 42.0, *, offbeats: bool = True) -> bytes:
    sample_count = round(SAMPLE_RATE * seconds)
    beat_period = SAMPLE_RATE * 60.0 / bpm
    values = array("h")
    for index in range(sample_count):
        beat_phase = index % beat_period
        beat = math.exp(-beat_phase / 90.0) if beat_phase < 700 else 0.0
        offbeat_phase = (index - beat_period * 0.5) % beat_period
        offbeat = math.exp(-offbeat_phase / 70.0) * 0.35 if offbeats and offbeat_phase < 500 else 0.0
        tone = math.sin(2.0 * math.pi * 220.0 * index / SAMPLE_RATE) * 0.08
        full = max(-1.0, min(1.0, beat + offbeat + tone))
        low = max(-1.0, min(1.0, beat))
        values.extend((round(full * 26000), round(low * 26000)))
    return values.tobytes()


def stereo_pcm_for_speech_like_bursts(seconds: float = 42.0) -> bytes:
    rng = random.Random(9071)
    sample_count = round(SAMPLE_RATE * seconds)
    burst_starts = []
    cursor = 0.0
    while cursor < sample_count:
        cursor += rng.uniform(0.11, 0.72) * SAMPLE_RATE
        burst_starts.append(round(cursor))
    values = array("h")
    for index in range(sample_count):
        amplitude = 0.0
        for start in burst_starts:
            distance = index - start
            if 0 <= distance < 900:
                amplitude += math.exp(-distance / 260.0) * rng.uniform(0.2, 0.7)
            if start > index:
                break
        voice = math.sin(2.0 * math.pi * (145.0 + 25.0 * math.sin(index / 1300.0)) * index / SAMPLE_RATE)
        full = max(-1.0, min(1.0, amplitude * voice))
        values.extend((round(full * 18000), round(full * 3500)))
    return values.tobytes()


class TempoEstimatorTests(unittest.TestCase):
    def test_bpm_has_a_single_key_default_and_settings_entry(self):
        self.assertEqual(DEFAULT_KEYBOARD_SHORTCUTS["player_bpm"], "B")
        self.assertIn(("player_bpm", "shortcut_player_bpm"), SHORTCUT_DEFINITIONS)
        conflicting = [
            action
            for action, shortcut in DEFAULT_KEYBOARD_SHORTCUTS.items()
            if action != "player_bpm" and shortcut.casefold() == "b"
        ]
        self.assertEqual(conflicting, [])

    def test_detects_common_music_tempos(self):
        for expected in (60.0, 90.0, 120.0, 128.0, 150.0, 180.0):
            with self.subTest(expected=expected):
                estimate = estimate_tempo_from_pcm16_stereo(stereo_pcm_for_click_track(expected))
                self.assertIsNotNone(estimate)
                self.assertAlmostEqual(estimate.bpm, expected, delta=max(1.2, expected * 0.012))

    def test_steady_tone_is_not_reported_as_music_tempo(self):
        values = array("h")
        for index in range(SAMPLE_RATE * 20):
            sample = round(math.sin(2.0 * math.pi * 220.0 * index / SAMPLE_RATE) * 12000)
            values.extend((sample, sample // 4))
        self.assertIsNone(estimate_tempo_from_pcm16_stereo(values.tobytes()))

    def test_irregular_speech_like_bursts_are_rejected(self):
        self.assertIsNone(estimate_tempo_from_pcm16_stereo(stereo_pcm_for_speech_like_bursts()))

    def test_short_audio_is_unavailable(self):
        self.assertIsNone(estimate_tempo_from_pcm16_stereo(b"\0" * SAMPLE_RATE * 2 * 2 * 3))

    def test_live_stream_analysis_starts_at_the_live_edge(self):
        self.assertEqual(MiscUI.bpm_analysis_window(580.0, None), (0.0, 72.0))

    def test_finite_media_analysis_surrounds_the_current_position(self):
        self.assertEqual(MiscUI.bpm_analysis_window(100.0, 300.0), (82.0, 72.0))

    def test_ffmpeg_headers_reject_line_injection(self):
        args = MiscUI.bpm_ffmpeg_args(
            "ffmpeg",
            "https://media.example/audio",
            {"User-Agent": "Apricot", "Bad\r\nInjected": "yes", "Referer": "safe\r\nInjected: no"},
            0.0,
            30.0,
        )
        header_text = args[args.index("-headers") + 1]
        self.assertEqual(header_text, "User-Agent: Apricot\r\n")

    def test_bpm_analysis_state_changes_with_speed_and_pitch(self):
        original = MiscUI.bpm_analysis_state_key("video-id", 1.0, 1.0)

        self.assertNotEqual(original, MiscUI.bpm_analysis_state_key("video-id", 1.25, 1.0))
        self.assertNotEqual(original, MiscUI.bpm_analysis_state_key("video-id", 1.0, 1.10))

    def test_bpm_is_scaled_to_the_current_playback_speed(self):
        self.assertEqual(MiscUI.effective_playback_bpm(120.0, 1.25), 150)
        self.assertEqual(MiscUI.effective_playback_bpm(128.0, 0.75), 96)

    def test_repeated_bpm_request_reanalyzes_unchanged_playback(self):
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
                self.settings = SimpleNamespace(player_speed="1.0")
                self.current_video_info = {"speed": "1.0", "pitch": "1.0"}
                self.current_stream_url = "https://media.example/audio"
                self.current_stream_headers = {}
                self.player_generation = 4
                self.bpm_analysis_lock = threading.Lock()
                self.bpm_analysis_running_keys = set()
                current_key = self.bpm_analysis_state_key("video-id", 1.0, 1.0)
                self.bpm_analysis_cache = {current_key: 120}
                self.announcements = []

            @staticmethod
            def player_is_active():
                return True

            @staticmethod
            def playback_key(_item=None):
                return "video-id"

            @staticmethod
            def t(key, **values):
                return key.format(**values)

            def announce_player(self, text):
                self.announcements.append(text)

        harness = Harness()
        with mock.patch("apricot.ui.misc.threading.Thread", DeferredThread):
            harness.announce_bpm_async()

        self.assertEqual(harness.announcements, ["bpm_analyzing"])
        self.assertEqual(len(DeferredThread.instances), 1)


if __name__ == "__main__":
    unittest.main()
