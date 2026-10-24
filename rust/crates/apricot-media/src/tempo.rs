//! Tempo estimation from decoded PCM, a line-by-line port of Python
//! `apricot/media/tempo.py`.
//!
//! Python's `round` rounds halves to even, so every `round` below is
//! `round_ties_even`. Sums run left to right like Python 3.11 `sum`.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoEstimate {
    pub bpm: f64,
    pub confidence: f64,
    pub supporting_segments: usize,
}

/// Python `sample_rate` default of the tempo analysis.
pub const TEMPO_SAMPLE_RATE: usize = 11_025;
const FRAME_SIZE: usize = 256;
const HOP_SIZE: usize = 128;

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn python_round(value: f64) -> i64 {
    value.round_ties_even() as i64
}

#[allow(clippy::cast_precision_loss)]
fn percentile(values: &[f64], fraction: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let last = ordered.len() - 1;
    let index = python_round(last as f64 * fraction).clamp(0, i64::try_from(last).unwrap_or(0));
    ordered[usize::try_from(index).unwrap_or(0)]
}

/// Python `statistics.median`.
fn median(values: &[f64]) -> f64 {
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let middle = ordered.len() / 2;
    if ordered.len() % 2 == 1 {
        ordered[middle]
    } else {
        f64::midpoint(ordered[middle - 1], ordered[middle])
    }
}

fn onset_flux(energies: &[f64]) -> Vec<f64> {
    if energies.len() < 3 {
        return Vec::new();
    }
    let mut flux = vec![0.0, 0.0];
    for index in 2..energies.len() {
        let reference = f64::midpoint(energies[index - 1], energies[index - 2]);
        flux.push((energies[index] - reference).max(0.0));
    }
    let floor = median(&flux);
    let absolute_strength = percentile(&flux, 0.99) - floor;
    if absolute_strength < 0.025 {
        return vec![0.0; flux.len()];
    }
    let scale = 1e-7_f64
        .max(percentile(&flux, 0.90) - floor)
        .max(absolute_strength * 0.35);
    let normalized: Vec<f64> = flux
        .iter()
        .map(|value| ((value - floor) / scale).max(0.0))
        .collect();
    let last = normalized.len() - 1;
    let mut smoothed = Vec::with_capacity(normalized.len());
    smoothed.push(normalized[0]);
    for index in 1..last {
        smoothed.push(
            normalized[index - 1] * 0.2 + normalized[index] * 0.6 + normalized[index + 1] * 0.2,
        );
    }
    smoothed.push(normalized[last]);
    smoothed
}

/// Python `onset_envelope_from_pcm16_stereo`: interleaved little-endian
/// 16-bit stereo, the left channel full band and the right channel low band.
#[allow(clippy::cast_precision_loss)]
pub fn onset_envelope_from_pcm16_stereo(pcm: &[u8], sample_rate: usize) -> (Vec<f64>, f64) {
    let frame_rate = sample_rate as f64 / HOP_SIZE as f64;
    let samples: Vec<i32> = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i32::from(i16::from_le_bytes([pair[0], pair[1]])))
        .collect();
    let channel_samples = samples.len() / 2;
    if channel_samples < sample_rate * 6 {
        return (Vec::new(), frame_rate);
    }
    let mut full_energy = Vec::new();
    let mut low_energy = Vec::new();
    let last_start = channel_samples - FRAME_SIZE;
    for start in (0..=last_start).step_by(HOP_SIZE) {
        let mut full_sum: i64 = 0;
        let mut low_sum: i64 = 0;
        let interleaved = start * 2;
        for offset in 0..FRAME_SIZE {
            full_sum += i64::from(samples[interleaved + offset * 2].abs());
            low_sum += i64::from(samples[interleaved + offset * 2 + 1].abs());
        }
        full_energy.push((full_sum as f64 / FRAME_SIZE as f64).ln_1p());
        low_energy.push((low_sum as f64 / FRAME_SIZE as f64).ln_1p());
    }
    let full_flux = onset_flux(&full_energy);
    let low_flux = onset_flux(&low_energy);
    let envelope = full_flux
        .iter()
        .zip(&low_flux)
        .map(|(&full, &low)| (full * 0.65).max(low * 0.85) + full.min(low) * 0.15)
        .collect();
    (envelope, frame_rate)
}

#[allow(clippy::cast_precision_loss)]
fn autocorrelation_scores(envelope: &[f64], frame_rate: f64) -> (BTreeMap<i64, f64>, i64, i64) {
    let minimum_lag = python_round(frame_rate * 60.0 / 220.0).max(2);
    let maximum_lag = i64::try_from(envelope.len() / 3)
        .unwrap_or(i64::MAX)
        .min(python_round(frame_rate * 60.0 / 45.0));
    let mut scores = BTreeMap::new();
    if maximum_lag <= minimum_lag {
        return (scores, minimum_lag, maximum_lag);
    }
    let mean = envelope.iter().sum::<f64>() / envelope.len() as f64;
    let centered: Vec<f64> = envelope.iter().map(|value| value - mean).collect();
    for lag in minimum_lag..=maximum_lag {
        let lag_index = usize::try_from(lag).unwrap_or(0);
        let left = &centered[..centered.len() - lag_index];
        let right = &centered[lag_index..];
        let numerator: f64 = left.iter().zip(right).map(|(a, b)| a * b).sum();
        let left_energy: f64 = left.iter().map(|value| value * value).sum();
        let right_energy: f64 = right.iter().map(|value| value * value).sum();
        let denominator = (left_energy * right_energy).sqrt();
        scores.insert(
            lag,
            if denominator > 1e-12 {
                numerator / denominator
            } else {
                0.0
            },
        );
    }
    (scores, minimum_lag, maximum_lag)
}

fn score_near(scores: &BTreeMap<i64, f64>, center: i64) -> f64 {
    [-1, 0, 1]
        .iter()
        .map(|offset| scores.get(&(center + offset)).copied().unwrap_or(0.0))
        .fold(0.0, f64::max)
}

#[allow(clippy::cast_precision_loss)]
fn tempo_candidate(envelope: &[f64], frame_rate: f64) -> Option<(f64, f64, f64)> {
    if (envelope.len() as f64) < (python_round(frame_rate * 6.0) as f64) {
        return None;
    }
    let (scores, minimum_lag, maximum_lag) = autocorrelation_scores(envelope, frame_rate);
    if scores.is_empty() {
        return None;
    }
    let mut ranked: Vec<(f64, i64)> = Vec::with_capacity(scores.len());
    for (&lag, &correlation) in &scores {
        let bpm = frame_rate * 60.0 / lag as f64;
        let slower_harmonic = if lag * 2 <= maximum_lag {
            score_near(&scores, lag * 2)
        } else {
            0.0
        };
        let half_lag = python_round(lag as f64 * 0.5);
        let faster_harmonic = if half_lag >= minimum_lag {
            score_near(&scores, half_lag)
        } else {
            0.0
        };
        let prior = (-0.5 * ((bpm / 120.0).max(1e-6).log2() / 1.25).powi(2)).exp();
        ranked.push((
            correlation + slower_harmonic * 0.28 + faster_harmonic * 0.03 + prior * 0.015,
            lag,
        ));
    }
    // Python `ranked.sort(reverse=True)`: highest score, then the larger lag.
    ranked.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
    });
    let mut lag = ranked[0].1;
    let half = python_round(lag as f64 * 0.5);
    let faster_lags: Vec<i64> = [half - 1, half, half + 1]
        .into_iter()
        .filter(|candidate| scores.contains_key(candidate))
        .collect();
    // Python `max(..., key=...)` keeps the first of equal scores.
    let faster_lag = faster_lags
        .iter()
        .copied()
        .fold(None, |best: Option<i64>, candidate| match best {
            Some(best) if scores[&best] >= scores[&candidate] => Some(best),
            _ => Some(candidate),
        });
    if let Some(faster_lag) = faster_lag
        && scores[&faster_lag] >= scores[&lag] * 0.90
    {
        lag = faster_lag;
    }
    let correlation = scores[&lag];
    let neighboring: Vec<f64> = scores
        .iter()
        .filter(|(candidate, _)| (*candidate - lag).abs() > 2)
        .map(|(_, value)| *value)
        .collect();
    let contrast = correlation
        - if neighboring.is_empty() {
            0.0
        } else {
            median(&neighboring)
        };
    let previous_score = scores.get(&(lag - 1)).copied().unwrap_or(correlation);
    let next_score = scores.get(&(lag + 1)).copied().unwrap_or(correlation);
    let curvature = previous_score - 2.0 * correlation + next_score;
    let offset = if curvature.abs() < 1e-9 {
        0.0
    } else {
        0.5 * (previous_score - next_score) / curvature
    };
    let refined_lag = lag as f64 + offset.clamp(-0.5, 0.5);
    Some((frame_rate * 60.0 / refined_lag, correlation, contrast))
}

fn harmonic_distance(first: f64, second: f64) -> f64 {
    [0.5, 1.0, 2.0]
        .iter()
        .map(|multiplier| {
            let adjusted = second * multiplier;
            (first - adjusted).abs() / first.max(adjusted).max(1e-9)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Python `estimate_tempo_from_envelope`.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn estimate_tempo_from_envelope(envelope: &[f64], frame_rate: f64) -> Option<TempoEstimate> {
    if envelope.is_empty() || percentile(envelope, 0.95) < 0.08 {
        return None;
    }
    let (bpm, correlation, contrast) = tempo_candidate(envelope, frame_rate)?;
    if correlation < 0.105 || contrast < 0.035 {
        return None;
    }
    let segment_frames = usize::try_from(python_round(frame_rate * 14.0).max(1)).unwrap_or(1);
    let minimum_segment = python_round(frame_rate * 8.0);
    let segments: Vec<(f64, f64, f64)> = envelope
        .chunks(segment_frames)
        .filter(|segment| i64::try_from(segment.len()).unwrap_or(i64::MAX) >= minimum_segment)
        .filter_map(|segment| tempo_candidate(segment, frame_rate))
        .filter(|candidate| candidate.1 >= 0.075 && candidate.2 >= 0.02)
        .collect();
    let duration = envelope.len() as f64 / frame_rate;
    let supporting = segments
        .iter()
        .filter(|(segment_bpm, _, _)| harmonic_distance(bpm, *segment_bpm) <= 0.055)
        .count();
    if duration >= 24.0
        && (supporting < 2 || (supporting as f64) < (segments.len() as f64 * 0.5).ceil())
    {
        return None;
    }
    if duration < 24.0 && correlation < 0.16 {
        return None;
    }
    let stability = supporting as f64 / segments.len().max(1) as f64;
    let confidence = ((correlation / 0.42).max(0.0) * 0.45
        + (contrast / 0.24).max(0.0) * 0.25
        + stability * 0.30)
        .min(1.0);
    if confidence < 0.42 {
        return None;
    }
    Some(TempoEstimate {
        bpm,
        confidence,
        supporting_segments: supporting,
    })
}

/// Python `estimate_tempo_from_pcm16_stereo`.
pub fn estimate_tempo_from_pcm16_stereo(pcm: &[u8], sample_rate: usize) -> Option<TempoEstimate> {
    let (envelope, frame_rate) = onset_envelope_from_pcm16_stereo(pcm, sample_rate);
    estimate_tempo_from_envelope(&envelope, frame_rate)
}

#[cfg(test)]
mod tests {
    use super::{
        TEMPO_SAMPLE_RATE, estimate_tempo_from_pcm16_stereo, harmonic_distance, median, percentile,
        python_round,
    };

    /// A click track: short full-band bursts on every beat, with a matching
    /// low-band burst in the right channel, like the decoder filter output.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn click_track(bpm: f64, seconds: f64) -> Vec<u8> {
        let rate = TEMPO_SAMPLE_RATE as f64;
        let total = (rate * seconds) as usize;
        let beat = rate * 60.0 / bpm;
        let mut pcm = Vec::with_capacity(total * 4);
        for index in 0..total {
            let phase = (index as f64) % beat;
            let value: i16 = if phase < 220.0 {
                let decay = 1.0 - phase / 220.0;
                let wave = if (index / 6) % 2 == 0 { 1.0 } else { -1.0 };
                (wave * decay * 20_000.0) as i16
            } else {
                0
            };
            pcm.extend_from_slice(&value.to_le_bytes());
            pcm.extend_from_slice(&value.to_le_bytes());
        }
        pcm
    }

    #[test]
    fn python_rounding_and_statistics_match() {
        assert_eq!(python_round(12.5), 12);
        assert_eq!(python_round(13.5), 14);
        assert_eq!(python_round(23.488), 23);
        assert!((median(&[3.0, 1.0, 2.0, 4.0]) - 2.5).abs() < f64::EPSILON);
        assert!((median(&[3.0, 1.0, 2.0]) - 2.0).abs() < f64::EPSILON);
        let values: Vec<f64> = (0..10).map(f64::from).collect();
        assert!((percentile(&values, 0.5) - 4.0).abs() < f64::EPSILON);
        assert!((percentile(&values, 0.95) - 9.0).abs() < f64::EPSILON);
        assert!(harmonic_distance(120.0, 60.0) < 1e-12);
        assert!(harmonic_distance(120.0, 240.0) < 1e-12);
    }

    #[test]
    fn click_tracks_report_their_tempo() {
        for bpm in [90.0, 120.0, 128.0, 140.0] {
            let estimate = estimate_tempo_from_pcm16_stereo(&click_track(bpm, 40.0), 11_025)
                .expect("tempo estimate");
            assert!(
                (estimate.bpm - bpm).abs() < 1.5,
                "expected {bpm}, got {}",
                estimate.bpm
            );
            assert!(estimate.confidence >= 0.42);
        }
    }

    #[test]
    fn silence_and_short_audio_have_no_tempo() {
        assert!(estimate_tempo_from_pcm16_stereo(&vec![0; 11_025 * 4 * 30], 11_025).is_none());
        assert!(estimate_tempo_from_pcm16_stereo(&click_track(120.0, 5.0), 11_025).is_none());
    }
}
