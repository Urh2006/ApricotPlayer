//! Bounded, frame-aligned PCM transition mixing. The decoder supplies an
//! unplayed tail; no already-heard audio is replayed or fabricated.

#[derive(Debug)]
pub struct PcmCrossfade {
    tail: Vec<i16>,
    position: usize,
    equal_power: bool,
}

impl PcmCrossfade {
    #[must_use]
    pub fn new(tail: &[u8], equal_power: bool) -> Self {
        let length = (tail.len() & !3).min(44_100 * 4 * 30);
        let tail = tail[tail.len() - length..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|sample| i16::from_le_bytes(*sample))
            .collect();
        Self {
            tail,
            position: 0,
            equal_power,
        }
    }

    #[must_use]
    pub fn finished(&self) -> bool {
        self.position >= self.tail.len()
    }

    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    pub fn mix(&mut self, incoming: &mut [u8]) {
        let total_frames = self.tail.len() / 2;
        for (frame, channels) in incoming.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let at = self.position + frame * 2;
            if at + 1 >= self.tail.len() {
                break;
            }
            let progress = if total_frames <= 1 {
                1.0
            } else {
                (at / 2) as f64 / (total_frames - 1) as f64
            };
            let (outgoing_gain, incoming_gain) = if self.equal_power {
                let angle = progress * std::f64::consts::FRAC_PI_2;
                (angle.cos(), angle.sin())
            } else {
                (1.0 - progress, progress)
            };
            for channel in 0..2 {
                let offset = channel * 2;
                let head = i16::from_le_bytes([channels[offset], channels[offset + 1]]);
                let mixed = (f64::from(self.tail[at + channel]) * outgoing_gain
                    + f64::from(head) * incoming_gain)
                    .round()
                    .clamp(f64::from(i16::MIN), f64::from(i16::MAX))
                    as i16;
                channels[offset..offset + 2].copy_from_slice(&mixed.to_le_bytes());
            }
        }
        self.position = self
            .position
            .saturating_add((incoming.len() & !3) / 2)
            .min(self.tail.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }
    #[test]
    fn linear_mix_preserves_first_tail_and_last_head_without_channel_crosstalk() {
        let mut mixer = PcmCrossfade::new(&pcm(&[1000, -1000, 1000, -1000, 1000, -1000]), false);
        let mut head = pcm(&[3000, -3000, 3000, -3000, 3000, -3000]);
        mixer.mix(&mut head);
        assert_eq!(head, pcm(&[1000, -1000, 2000, -2000, 3000, -3000]));
        assert!(mixer.finished());
    }
    #[test]
    fn packet_boundaries_do_not_restart_the_envelope() {
        let mut mixer = PcmCrossfade::new(&pcm(&[1000, 1000, 1000, 1000, 1000, 1000]), false);
        let mut first = pcm(&[3000, 3000]);
        mixer.mix(&mut first);
        assert_eq!(first, pcm(&[1000, 1000]));
        let mut rest = pcm(&[3000, 3000, 3000, 3000, 4000, 4000]);
        mixer.mix(&mut rest);
        assert_eq!(rest, pcm(&[2000, 2000, 3000, 3000, 4000, 4000]));
    }
}
