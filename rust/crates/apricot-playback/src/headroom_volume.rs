//! Clipping protection without needless quietness (2.1.2).
//!
//! A protected equalizer lowers everything by its largest boost before the
//! bands and limits what is left (Python `equalizer_filter`). mpv applies its
//! volume after the filters, as (volume / 100)^3, so at a lower volume the
//! boost could never reach full scale and that headroom only made the whole
//! sound quieter. This engine wrapper gives the volume back as much of the
//! headroom as still keeps the output at or below full scale
//! ([`protected_volume`]). The filter graph stays exactly as before, so it
//! survives seeks (mpv rebuilds `lavfi` graphs from their text on every seek,
//! which loses runtime `af-command` changes) and needs no new mpv feature.

use crate::{
    EQUALIZER_FILTER_ALT_LABEL, EQUALIZER_FILTER_LABEL, MpvLaunchOptions, PlaybackCommand,
    PlaybackEngine, PlaybackError, PlaybackEvent, clipping_headroom_db, protected_volume,
};

const LABELS: [&str; 2] = [EQUALIZER_FILTER_LABEL, EQUALIZER_FILTER_ALT_LABEL];

/// Every engine of the playback worker runs inside this wrapper.
pub struct HeadroomVolume {
    inner: Box<dyn PlaybackEngine>,
    /// Headroom of the equalizer under each label; both exist for a moment
    /// while a new equalizer replaces the running one.
    headroom: [Option<f64>; 2],
    /// The volume the worker asked for, before protection.
    requested: f64,
    /// The share of it a running fade lets through (1 without a fade). It
    /// applies after the protection, so a fade at a high volume still fades.
    fade: f64,
}

impl HeadroomVolume {
    /// Wraps a freshly started engine, which already plays the launch
    /// options' filter chain at their initial volume.
    pub fn wrap(inner: Box<dyn PlaybackEngine>, options: &MpvLaunchOptions) -> Self {
        let mut engine = Self {
            inner,
            headroom: [None, None],
            requested: options.initial_volume,
            fade: 1.0,
        };
        engine.headroom = chain_headroom(options.initial_audio_filter.as_deref());
        if (engine.mpv_volume() - engine.requested).abs() > f64::EPSILON {
            // Failing here only leaves the protection as quiet as before.
            let _ = engine
                .inner
                .execute(PlaybackCommand::SetVolume(engine.mpv_volume()));
        }
        engine
    }

    fn largest_headroom(headroom: [Option<f64>; 2]) -> f64 {
        headroom.into_iter().flatten().fold(0.0, f64::max)
    }

    fn volume_for(&self, headroom: [Option<f64>; 2]) -> f64 {
        protected_volume(self.requested, Self::largest_headroom(headroom)) * self.fade
    }

    fn mpv_volume(&self) -> f64 {
        self.volume_for(self.headroom)
    }

    /// The headroom after `command`, when it changes the equalizer.
    fn headroom_after(&self, command: &PlaybackCommand) -> Option<[Option<f64>; 2]> {
        let mut headroom = self.headroom;
        match command {
            PlaybackCommand::SetAudioFilter(chain) => {
                return Some(chain_headroom(chain.as_deref()));
            }
            PlaybackCommand::AddAudioFilter(filter) => {
                let index = LABELS
                    .iter()
                    .position(|label| filter.starts_with(&format!("@{label}:")))?;
                headroom[index] = clipping_headroom_db(filter, LABELS[index]);
            }
            PlaybackCommand::RemoveAudioFilter(filter) => {
                let index = LABELS
                    .iter()
                    .position(|label| filter == &format!("@{label}"))?;
                headroom[index] = None;
            }
            _ => return None,
        }
        Some(headroom)
    }
}

fn chain_headroom(chain: Option<&str>) -> [Option<f64>; 2] {
    let chain = chain.unwrap_or_default();
    LABELS.map(|label| clipping_headroom_db(chain, label))
}

impl PlaybackEngine for HeadroomVolume {
    fn execute(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError> {
        let fade = match command {
            PlaybackCommand::SetVolume(volume) => Some((volume, 1.0)),
            PlaybackCommand::SetFadedVolume { volume, factor } => {
                Some((volume, factor.clamp(0.0, 1.0)))
            }
            _ => None,
        };
        if let Some((volume, factor)) = fade {
            self.requested = volume;
            self.fade = factor;
            return self
                .inner
                .execute(PlaybackCommand::SetVolume(self.mpv_volume()));
        }
        let Some(after) = self.headroom_after(&command) else {
            return self.inner.execute(command);
        };
        let before = self.mpv_volume();
        let next = self.volume_for(after);
        // Less headroom is louder: the volume goes down before the filter
        // changes and up only after it, so no moment is louder than either.
        if next < before {
            let _ = self.inner.execute(PlaybackCommand::SetVolume(next));
        }
        let result = self.inner.execute(command);
        if result.is_err() {
            if next < before {
                let _ = self.inner.execute(PlaybackCommand::SetVolume(before));
            }
            return result;
        }
        self.headroom = after;
        if next > before {
            let _ = self.inner.execute(PlaybackCommand::SetVolume(next));
        }
        result
    }

    fn poll_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
        self.inner.poll_event()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use super::HeadroomVolume;
    use crate::{
        EQUALIZER_FILTER_ALT_LABEL, EQUALIZER_FILTER_LABEL, MpvLaunchOptions, PlaybackCommand,
        PlaybackEngine, PlaybackError, PlaybackEvent, equalizer_filter_graph,
        tagged_equalizer_filter,
    };

    #[derive(Default)]
    struct RecordingEngine {
        commands: Arc<Mutex<Vec<PlaybackCommand>>>,
    }

    impl PlaybackEngine for RecordingEngine {
        fn execute(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError> {
            self.commands.lock().expect("commands").push(command);
            Ok(())
        }

        fn poll_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
            Ok(None)
        }
    }

    fn protected(label: &str, boost: f64) -> String {
        let gains = BTreeMap::from([("62".to_owned(), boost)]);
        tagged_equalizer_filter(label, &equalizer_filter_graph(&gains, true).expect("graph"))
    }

    fn plain(label: &str) -> String {
        let gains = BTreeMap::from([("62".to_owned(), 6.0)]);
        tagged_equalizer_filter(
            label,
            &equalizer_filter_graph(&gains, false).expect("graph"),
        )
    }

    fn wrapped(
        chain: Option<String>,
        volume: f64,
    ) -> (HeadroomVolume, Arc<Mutex<Vec<PlaybackCommand>>>) {
        let recording = RecordingEngine::default();
        let commands = Arc::clone(&recording.commands);
        let mut options = MpvLaunchOptions::new("mpv");
        options.initial_audio_filter = chain;
        options.initial_volume = volume;
        (
            HeadroomVolume::wrap(Box::new(recording), &options),
            commands,
        )
    }

    fn take(commands: &Arc<Mutex<Vec<PlaybackCommand>>>) -> Vec<PlaybackCommand> {
        std::mem::take(&mut *commands.lock().expect("commands"))
    }

    fn volume_of(command: &PlaybackCommand) -> f64 {
        match command {
            PlaybackCommand::SetVolume(volume) => *volume,
            other => panic!("expected a volume, got {other:?}"),
        }
    }

    #[test]
    fn without_protection_every_command_passes_unchanged() {
        let (mut engine, commands) = wrapped(Some(plain(EQUALIZER_FILTER_LABEL)), 50.0);
        assert!(take(&commands).is_empty());
        engine
            .execute(PlaybackCommand::SetVolume(40.0))
            .expect("volume");
        engine
            .execute(PlaybackCommand::AddAudioFilter(plain(
                EQUALIZER_FILTER_ALT_LABEL,
            )))
            .expect("filter");
        assert_eq!(
            take(&commands),
            [
                PlaybackCommand::SetVolume(40.0),
                PlaybackCommand::AddAudioFilter(plain(EQUALIZER_FILTER_ALT_LABEL)),
            ]
        );
    }

    #[test]
    fn a_protected_launch_chain_gets_its_headroom_back_at_once() {
        let (mut engine, commands) = wrapped(Some(protected(EQUALIZER_FILTER_LABEL, 6.0)), 50.0);
        let start = take(&commands);
        assert_eq!(start.len(), 1);
        assert!((volume_of(&start[0]) - 50.0 * 10_f64.powf(0.1)).abs() < 1e-9);
        // Fades and crossfades scale with the requested volume.
        engine
            .execute(PlaybackCommand::SetVolume(25.0))
            .expect("volume");
        assert!((volume_of(&take(&commands)[0]) - 25.0 * 10_f64.powf(0.1)).abs() < 1e-9);
        engine
            .execute(PlaybackCommand::SetVolume(0.0))
            .expect("volume");
        assert!(volume_of(&take(&commands)[0]).abs() < 1e-9);
        engine
            .execute(PlaybackCommand::SetVolume(150.0))
            .expect("volume");
        assert!((volume_of(&take(&commands)[0]) - 150.0).abs() < 1e-9);
    }

    #[test]
    fn removing_protection_lowers_the_volume_before_the_filter_changes() {
        let (mut engine, commands) = wrapped(Some(protected(EQUALIZER_FILTER_LABEL, 6.0)), 50.0);
        take(&commands);
        let next = plain(EQUALIZER_FILTER_ALT_LABEL);
        engine
            .execute(PlaybackCommand::AddAudioFilter(next.clone()))
            .expect("add");
        // The old protected graph still runs, the volume stays.
        assert_eq!(take(&commands), [PlaybackCommand::AddAudioFilter(next)]);
        engine
            .execute(PlaybackCommand::RemoveAudioFilter(format!(
                "@{EQUALIZER_FILTER_LABEL}"
            )))
            .expect("remove");
        assert_eq!(
            take(&commands),
            [
                PlaybackCommand::SetVolume(50.0),
                PlaybackCommand::RemoveAudioFilter(format!("@{EQUALIZER_FILTER_LABEL}")),
            ]
        );
    }

    #[test]
    fn adding_protection_raises_the_volume_after_the_filter_changes() {
        let (mut engine, commands) = wrapped(None, 50.0);
        assert!(take(&commands).is_empty());
        let filter = protected(EQUALIZER_FILTER_LABEL, 6.0);
        engine
            .execute(PlaybackCommand::AddAudioFilter(filter.clone()))
            .expect("add");
        let sent = take(&commands);
        assert_eq!(sent[0], PlaybackCommand::AddAudioFilter(filter));
        assert!((volume_of(&sent[1]) - 50.0 * 10_f64.powf(0.1)).abs() < 1e-9);
        // A whole new chain without the equalizer gives the volume back.
        engine
            .execute(PlaybackCommand::SetAudioFilter(None))
            .expect("chain");
        assert_eq!(
            take(&commands),
            [
                PlaybackCommand::SetVolume(50.0),
                PlaybackCommand::SetAudioFilter(None)
            ]
        );
    }

    #[test]
    fn a_fade_at_full_volume_still_fades_under_protection() {
        let (mut engine, commands) = wrapped(Some(protected(EQUALIZER_FILTER_LABEL, 6.0)), 100.0);
        assert!(take(&commands).is_empty(), "full volume needs no change");
        // The middle of an equal-power crossfade at 100 % (review of 2.1.2):
        // protection must not lift both items back to full scale.
        let factor = std::f64::consts::FRAC_1_SQRT_2.cbrt();
        engine
            .execute(PlaybackCommand::SetFadedVolume {
                volume: 100.0,
                factor,
            })
            .expect("fade");
        assert!((volume_of(&take(&commands)[0]) - 100.0 * factor).abs() < 1e-9);
        // At a lower volume the fade scales the protected volume.
        engine
            .execute(PlaybackCommand::SetFadedVolume {
                volume: 50.0,
                factor: 0.5,
            })
            .expect("fade");
        assert!((volume_of(&take(&commands)[0]) - 25.0 * 10_f64.powf(0.1)).abs() < 1e-9);
        // An equalizer change during the fade keeps the fade's share.
        engine
            .execute(PlaybackCommand::SetAudioFilter(None))
            .expect("chain");
        assert_eq!(
            take(&commands),
            [
                PlaybackCommand::SetVolume(25.0),
                PlaybackCommand::SetAudioFilter(None)
            ]
        );
    }

    #[test]
    fn a_stronger_boost_takes_more_headroom_back() {
        let (mut engine, commands) = wrapped(Some(protected(EQUALIZER_FILTER_LABEL, 3.0)), 40.0);
        take(&commands);
        engine
            .execute(PlaybackCommand::AddAudioFilter(protected(
                EQUALIZER_FILTER_ALT_LABEL,
                9.0,
            )))
            .expect("add");
        engine
            .execute(PlaybackCommand::RemoveAudioFilter(format!(
                "@{EQUALIZER_FILTER_LABEL}"
            )))
            .expect("remove");
        let volumes: Vec<f64> = take(&commands)
            .iter()
            .filter(|command| matches!(command, PlaybackCommand::SetVolume(_)))
            .map(volume_of)
            .collect();
        assert_eq!(volumes.len(), 1);
        assert!((volumes[0] - 40.0 * 10_f64.powf(9.0 / 60.0)).abs() < 1e-9);
    }
}
