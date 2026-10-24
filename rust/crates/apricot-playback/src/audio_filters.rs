//! Targeted `af` updates matching Python `apply_equalizer_to_player`,
//! `clear_equalizer_filters` and `apply_rubberband_pitch_filter`: only the
//! equalizer or pitch filter changes, the rest of the chain keeps running.

use crate::{
    EQUALIZER_FILTER_ALT_LABEL, EQUALIZER_FILTER_LABEL, PITCH_FILTER_LABEL, PlaybackCommand,
    PlaybackEngine, PlaybackError, rubberband_pitch_filter, tagged_equalizer_filter,
};

/// Which tagged filters the running chain holds, like Python
/// `equalizer_filter_active`, `equalizer_filter_ref` and
/// `rubberband_pitch_filter_active`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AudioFilterState {
    equalizer_label: Option<&'static str>,
    pitch_active: bool,
}

impl AudioFilterState {
    /// State after the whole chain was replaced with `chain`.
    #[must_use]
    pub fn from_chain(chain: Option<&str>) -> Self {
        let chain = chain.unwrap_or_default();
        let contains = |label: &str| {
            chain
                .split(',')
                .any(|filter| filter.starts_with(&format!("@{label}:")))
        };
        Self {
            equalizer_label: if contains(EQUALIZER_FILTER_LABEL) {
                Some(EQUALIZER_FILTER_LABEL)
            } else if contains(EQUALIZER_FILTER_ALT_LABEL) {
                Some(EQUALIZER_FILTER_ALT_LABEL)
            } else {
                None
            },
            pitch_active: contains(PITCH_FILTER_LABEL),
        }
    }

    /// Runs `command`, expanding the equalizer and pitch updates into
    /// targeted `af` commands and tracking the chain for every other command.
    ///
    /// # Errors
    ///
    /// Returns the engine error of the command that could not be applied.
    pub fn execute(
        &mut self,
        engine: &mut dyn PlaybackEngine,
        command: PlaybackCommand,
    ) -> Result<(), PlaybackError> {
        match command {
            PlaybackCommand::SetEqualizerFilter(graph) => self.set_equalizer(engine, graph),
            PlaybackCommand::SetPitchFilter(pitch) => self.set_pitch(engine, pitch),
            PlaybackCommand::SetAudioFilter(chain) => {
                let next = Self::from_chain(chain.as_deref());
                engine.execute(PlaybackCommand::SetAudioFilter(chain))?;
                *self = next;
                Ok(())
            }
            other => engine.execute(other),
        }
    }

    fn set_equalizer(
        &mut self,
        engine: &mut dyn PlaybackEngine,
        graph: Option<String>,
    ) -> Result<(), PlaybackError> {
        let Some(graph) = graph else {
            for label in [EQUALIZER_FILTER_LABEL, EQUALIZER_FILTER_ALT_LABEL] {
                let _ = engine.execute(PlaybackCommand::RemoveAudioFilter(format!("@{label}")));
            }
            self.equalizer_label = None;
            return Ok(());
        };
        let current = self.equalizer_label;
        let next = if current == Some(EQUALIZER_FILTER_LABEL) {
            EQUALIZER_FILTER_ALT_LABEL
        } else {
            EQUALIZER_FILTER_LABEL
        };
        let _ = engine.execute(PlaybackCommand::RemoveAudioFilter(format!("@{next}")));
        engine.execute(PlaybackCommand::AddAudioFilter(tagged_equalizer_filter(
            next, &graph,
        )))?;
        if let Some(current) = current.filter(|current| *current != next) {
            let _ = engine.execute(PlaybackCommand::RemoveAudioFilter(format!("@{current}")));
        }
        let stale = if next == EQUALIZER_FILTER_LABEL {
            EQUALIZER_FILTER_ALT_LABEL
        } else {
            EQUALIZER_FILTER_LABEL
        };
        if Some(stale) != current {
            let _ = engine.execute(PlaybackCommand::RemoveAudioFilter(format!("@{stale}")));
        }
        self.equalizer_label = Some(next);
        Ok(())
    }

    fn set_pitch(
        &mut self,
        engine: &mut dyn PlaybackEngine,
        pitch: Option<f64>,
    ) -> Result<(), PlaybackError> {
        let Some(pitch) = pitch else {
            let _ = engine.execute(PlaybackCommand::RemoveAudioFilter(format!(
                "@{PITCH_FILTER_LABEL}"
            )));
            self.pitch_active = false;
            return Ok(());
        };
        if self.pitch_active {
            let adjusted = engine.execute(PlaybackCommand::AudioFilterCommand {
                label: PITCH_FILTER_LABEL.to_owned(),
                command: "set-pitch".to_owned(),
                argument: format!("{pitch:.4}"),
            });
            if adjusted.is_ok() {
                return Ok(());
            }
            self.pitch_active = false;
        }
        let _ = engine.execute(PlaybackCommand::RemoveAudioFilter(format!(
            "@{PITCH_FILTER_LABEL}"
        )));
        engine.execute(PlaybackCommand::AddAudioFilter(rubberband_pitch_filter(
            pitch,
        )))?;
        self.pitch_active = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::AudioFilterState;
    use crate::{PlaybackCommand, PlaybackEngine, PlaybackError, PlaybackEvent};

    #[derive(Default)]
    struct RecordingEngine {
        commands: Arc<Mutex<Vec<PlaybackCommand>>>,
        reject_filter_commands: bool,
    }

    impl PlaybackEngine for RecordingEngine {
        fn execute(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError> {
            let rejected = self.reject_filter_commands
                && matches!(command, PlaybackCommand::AudioFilterCommand { .. });
            self.commands.lock().expect("commands").push(command);
            if rejected {
                Err(PlaybackError::Operation("no such filter".to_owned()))
            } else {
                Ok(())
            }
        }

        fn poll_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
            Ok(None)
        }
    }

    fn take(engine: &RecordingEngine) -> Vec<PlaybackCommand> {
        std::mem::take(&mut *engine.commands.lock().expect("commands"))
    }

    fn remove(label: &str) -> PlaybackCommand {
        PlaybackCommand::RemoveAudioFilter(format!("@{label}"))
    }

    #[test]
    fn equalizer_updates_alternate_labels_like_python() {
        let mut engine = RecordingEngine::default();
        let mut state = AudioFilterState::from_chain(Some(
            "@apricot_speed:scaletempo2,@apricot_eq:lavfi=[equalizer=f=31:t=q:w=1.7:g=3.0]",
        ));

        state
            .execute(
                &mut engine,
                PlaybackCommand::SetEqualizerFilter(Some("lavfi=[a]".to_owned())),
            )
            .expect("first update");
        assert_eq!(
            take(&engine),
            vec![
                remove("apricot_eq_next"),
                PlaybackCommand::AddAudioFilter("@apricot_eq_next:lavfi=[a]".to_owned()),
                remove("apricot_eq"),
            ]
        );

        state
            .execute(
                &mut engine,
                PlaybackCommand::SetEqualizerFilter(Some("lavfi=[b]".to_owned())),
            )
            .expect("second update");
        assert_eq!(
            take(&engine),
            vec![
                remove("apricot_eq"),
                PlaybackCommand::AddAudioFilter("@apricot_eq:lavfi=[b]".to_owned()),
                remove("apricot_eq_next"),
            ]
        );

        state
            .execute(&mut engine, PlaybackCommand::SetEqualizerFilter(None))
            .expect("clear");
        assert_eq!(
            take(&engine),
            vec![remove("apricot_eq"), remove("apricot_eq_next")]
        );
    }

    #[test]
    fn first_equalizer_without_a_running_filter_clears_both_labels() {
        let mut engine = RecordingEngine::default();
        let mut state = AudioFilterState::from_chain(None);
        state
            .execute(
                &mut engine,
                PlaybackCommand::SetEqualizerFilter(Some("lavfi=[a]".to_owned())),
            )
            .expect("update");
        assert_eq!(
            take(&engine),
            vec![
                remove("apricot_eq"),
                PlaybackCommand::AddAudioFilter("@apricot_eq:lavfi=[a]".to_owned()),
                remove("apricot_eq_next"),
            ]
        );
    }

    #[test]
    fn pitch_is_adjusted_in_place_and_readded_when_the_filter_is_gone() {
        let mut engine = RecordingEngine::default();
        let mut state = AudioFilterState::from_chain(None);
        state
            .execute(&mut engine, PlaybackCommand::SetPitchFilter(Some(1.05)))
            .expect("add pitch");
        assert_eq!(
            take(&engine),
            vec![
                remove("apricot_pitch"),
                PlaybackCommand::AddAudioFilter(crate::rubberband_pitch_filter(1.05)),
            ]
        );
        state
            .execute(&mut engine, PlaybackCommand::SetPitchFilter(Some(1.1)))
            .expect("adjust pitch");
        assert_eq!(
            take(&engine),
            vec![PlaybackCommand::AudioFilterCommand {
                label: "apricot_pitch".to_owned(),
                command: "set-pitch".to_owned(),
                argument: "1.1000".to_owned(),
            }]
        );

        engine.reject_filter_commands = true;
        state
            .execute(&mut engine, PlaybackCommand::SetPitchFilter(Some(1.2)))
            .expect("re-add pitch");
        let commands = take(&engine);
        assert_eq!(commands.len(), 3);
        assert_eq!(
            commands[2],
            PlaybackCommand::AddAudioFilter(crate::rubberband_pitch_filter(1.2))
        );

        state
            .execute(&mut engine, PlaybackCommand::SetPitchFilter(None))
            .expect("clear pitch");
        assert_eq!(take(&engine), vec![remove("apricot_pitch")]);
    }

    #[test]
    fn a_new_chain_resets_the_tracked_filters() {
        let mut engine = RecordingEngine::default();
        let mut state = AudioFilterState::from_chain(Some("@apricot_eq_next:lavfi=[a]"));
        state
            .execute(
                &mut engine,
                PlaybackCommand::SetAudioFilter(Some(
                    "@apricot_pitch:rubberband=pitch-scale=1.1".to_owned(),
                )),
            )
            .expect("chain");
        assert_eq!(
            state,
            AudioFilterState {
                equalizer_label: None,
                pitch_active: true,
            }
        );
    }
}
