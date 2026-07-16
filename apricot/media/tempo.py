from __future__ import annotations

from array import array
from dataclasses import dataclass
import math
import statistics
import sys


@dataclass(frozen=True)
class TempoEstimate:
    bpm: float
    confidence: float
    supporting_segments: int


def _percentile(values: list[float], fraction: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    index = max(0, min(len(ordered) - 1, round((len(ordered) - 1) * fraction)))
    return float(ordered[index])


def _onset_flux(energies: list[float]) -> list[float]:
    if len(energies) < 3:
        return []
    flux = [0.0, 0.0]
    for index in range(2, len(energies)):
        reference = (energies[index - 1] + energies[index - 2]) * 0.5
        flux.append(max(0.0, energies[index] - reference))
    floor = statistics.median(flux)
    absolute_strength = _percentile(flux, 0.99) - floor
    if absolute_strength < 0.025:
        return [0.0] * len(flux)
    scale = max(1e-7, _percentile(flux, 0.90) - floor, absolute_strength * 0.35)
    normalized = [max(0.0, (value - floor) / scale) for value in flux]
    if len(normalized) < 3:
        return normalized
    return [
        normalized[0],
        *(
            normalized[index - 1] * 0.2
            + normalized[index] * 0.6
            + normalized[index + 1] * 0.2
            for index in range(1, len(normalized) - 1)
        ),
        normalized[-1],
    ]


def onset_envelope_from_pcm16_stereo(
    pcm: bytes,
    sample_rate: int = 11025,
    frame_size: int = 256,
    hop_size: int = 128,
) -> tuple[list[float], float]:
    samples = array("h")
    samples.frombytes(pcm[: len(pcm) - (len(pcm) % 2)])
    if sys.byteorder != "little":
        samples.byteswap()
    channel_samples = len(samples) // 2
    if channel_samples < sample_rate * 6:
        return [], sample_rate / hop_size

    full_energy: list[float] = []
    low_energy: list[float] = []
    last_start = channel_samples - frame_size
    for start in range(0, last_start + 1, hop_size):
        full_sum = 0
        low_sum = 0
        interleaved = start * 2
        for offset in range(frame_size):
            full_sum += abs(samples[interleaved + offset * 2])
            low_sum += abs(samples[interleaved + offset * 2 + 1])
        full_energy.append(math.log1p(full_sum / frame_size))
        low_energy.append(math.log1p(low_sum / frame_size))

    full_flux = _onset_flux(full_energy)
    low_flux = _onset_flux(low_energy)
    envelope = [max(full * 0.65, low * 0.85) + min(full, low) * 0.15 for full, low in zip(full_flux, low_flux)]
    return envelope, sample_rate / hop_size


def _autocorrelation_scores(envelope: list[float], frame_rate: float) -> tuple[dict[int, float], int, int]:
    minimum_lag = max(2, round(frame_rate * 60.0 / 220.0))
    maximum_lag = min(len(envelope) // 3, round(frame_rate * 60.0 / 45.0))
    if maximum_lag <= minimum_lag:
        return {}, minimum_lag, maximum_lag
    mean = sum(envelope) / len(envelope)
    centered = [value - mean for value in envelope]
    scores: dict[int, float] = {}
    for lag in range(minimum_lag, maximum_lag + 1):
        left = centered[:-lag]
        right = centered[lag:]
        numerator = sum(a * b for a, b in zip(left, right))
        left_energy = sum(value * value for value in left)
        right_energy = sum(value * value for value in right)
        denominator = math.sqrt(left_energy * right_energy)
        scores[lag] = numerator / denominator if denominator > 1e-12 else 0.0
    return scores, minimum_lag, maximum_lag


def _tempo_candidate(envelope: list[float], frame_rate: float) -> tuple[float, float, float] | None:
    if len(envelope) < round(frame_rate * 6):
        return None
    scores, minimum_lag, maximum_lag = _autocorrelation_scores(envelope, frame_rate)
    if not scores:
        return None

    ranked: list[tuple[float, int]] = []
    for lag, correlation in scores.items():
        bpm = frame_rate * 60.0 / lag
        slower_harmonic = (
            max(0.0, *(scores.get(lag * 2 + offset, 0.0) for offset in (-1, 0, 1)))
            if lag * 2 <= maximum_lag
            else 0.0
        )
        half_lag = round(lag * 0.5)
        faster_harmonic = (
            max(0.0, *(scores.get(half_lag + offset, 0.0) for offset in (-1, 0, 1)))
            if half_lag >= minimum_lag
            else 0.0
        )
        prior = math.exp(-0.5 * (math.log2(max(1e-6, bpm / 120.0)) / 1.25) ** 2)
        ranked.append((correlation + slower_harmonic * 0.28 + faster_harmonic * 0.03 + prior * 0.015, lag))
    ranked.sort(reverse=True)
    _weighted_score, lag = ranked[0]
    faster_lags = [candidate for candidate in (round(lag * 0.5) - 1, round(lag * 0.5), round(lag * 0.5) + 1) if candidate in scores]
    if faster_lags:
        faster_lag = max(faster_lags, key=lambda candidate: scores[candidate])
        if scores[faster_lag] >= scores[lag] * 0.90:
            lag = faster_lag
    correlation = scores[lag]
    neighboring = [value for candidate_lag, value in scores.items() if abs(candidate_lag - lag) > 2]
    contrast = correlation - (statistics.median(neighboring) if neighboring else 0.0)

    previous_score = scores.get(lag - 1, correlation)
    next_score = scores.get(lag + 1, correlation)
    curvature = previous_score - 2.0 * correlation + next_score
    offset = 0.0 if abs(curvature) < 1e-9 else 0.5 * (previous_score - next_score) / curvature
    refined_lag = lag + max(-0.5, min(0.5, offset))
    return frame_rate * 60.0 / refined_lag, correlation, contrast


def _harmonic_distance(first: float, second: float) -> float:
    distances = []
    for multiplier in (0.5, 1.0, 2.0):
        adjusted = second * multiplier
        distances.append(abs(first - adjusted) / max(first, adjusted, 1e-9))
    return min(distances)


def estimate_tempo_from_envelope(envelope: list[float], frame_rate: float) -> TempoEstimate | None:
    if not envelope or _percentile(envelope, 0.95) < 0.08:
        return None
    overall = _tempo_candidate(envelope, frame_rate)
    if overall is None:
        return None
    bpm, correlation, contrast = overall
    if correlation < 0.105 or contrast < 0.035:
        return None

    segment_frames = max(round(frame_rate * 14.0), 1)
    segments: list[tuple[float, float, float]] = []
    for start in range(0, len(envelope), segment_frames):
        segment = envelope[start : start + segment_frames]
        if len(segment) < round(frame_rate * 8.0):
            continue
        candidate = _tempo_candidate(segment, frame_rate)
        if candidate is not None and candidate[1] >= 0.075 and candidate[2] >= 0.02:
            segments.append(candidate)

    duration = len(envelope) / frame_rate
    supporting = sum(1 for segment_bpm, _corr, _contrast in segments if _harmonic_distance(bpm, segment_bpm) <= 0.055)
    if duration >= 24.0 and (supporting < 2 or supporting < math.ceil(len(segments) * 0.5)):
        return None
    if duration < 24.0 and correlation < 0.16:
        return None

    stability = supporting / max(1, len(segments))
    confidence = min(
        1.0,
        max(0.0, correlation / 0.42) * 0.45
        + max(0.0, contrast / 0.24) * 0.25
        + stability * 0.30,
    )
    if confidence < 0.42:
        return None
    return TempoEstimate(bpm=bpm, confidence=confidence, supporting_segments=supporting)


def estimate_tempo_from_pcm16_stereo(pcm: bytes, sample_rate: int = 11025) -> TempoEstimate | None:
    envelope, frame_rate = onset_envelope_from_pcm16_stereo(pcm, sample_rate=sample_rate)
    return estimate_tempo_from_envelope(envelope, frame_rate)
