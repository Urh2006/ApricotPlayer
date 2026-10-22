//! On-demand analysis of Spotify audio, independent of the Connect transport.
use librespot_core::{Session, SpotifyUri};
use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    config::PlayerConfig,
    convert::Converter,
    decoder::AudioPacket,
    mixer::NoOpVolume,
    player::{Player, PlayerEvent},
};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// A snapshot of the authenticated decoder session. Created on the UI thread;
/// decoding and `FFmpeg` run only on the existing BPM worker.
pub struct SpotifyBpmAnalyzer {
    session: Session,
    runtime: tokio::runtime::Handle,
    config: PlayerConfig,
}

struct Capture {
    audio: Mutex<Vec<u8>>,
    ready: Condvar,
    limit: usize,
}

struct AnalysisSink(Arc<Capture>);

impl Sink for AnalysisSink {
    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        if let AudioPacket::Samples(samples) = packet {
            let pcm = converter.f64_to_s16(&samples);
            if let Ok(mut audio) = self.0.audio.lock() {
                let remaining = self.0.limit.saturating_sub(audio.len()) / 2;
                audio.extend(
                    pcm.iter()
                        .take(remaining)
                        .flat_map(|sample| sample.to_le_bytes()),
                );
                self.0.ready.notify_one();
            }
        }
        Ok(())
    }
}

impl SpotifyBpmAnalyzer {
    pub(crate) fn new(
        session: Session,
        runtime: tokio::runtime::Handle,
        mut config: PlayerConfig,
    ) -> Self {
        config.listening_history = false;
        Self {
            session,
            runtime,
            config,
        }
    }

    /// Decodes the requested window silently. This player has no Connect
    /// controller or output device and never reports listening history.
    #[must_use]
    pub fn analyze(
        &self,
        request: &apricot_platform::BpmAnalysisRequest,
        cancelled: &AtomicBool,
    ) -> Option<f64> {
        let pcm = self.decode(request, cancelled)?;
        apricot_platform::analyze_pcm_bpm(request, pcm, cancelled)
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn decode(
        &self,
        request: &apricot_platform::BpmAnalysisRequest,
        cancelled: &AtomicBool,
    ) -> Option<Vec<u8>> {
        if cancelled.load(Ordering::Acquire) || self.session.is_invalid() {
            return None;
        }
        let reference = apricot_core::SpotifyRef::parse(&request.source)?;
        let uri = SpotifyUri::from_uri(&reference.to_uri()).ok()?;
        let capture = Arc::new(Capture {
            audio: Mutex::new(Vec::new()),
            ready: Condvar::new(),
            limit: (request.duration_seconds.clamp(0.0, 72.0) * 44_100.0) as usize * 4,
        });
        let _entered = self.runtime.enter();
        let sink = capture.clone();
        let player = Player::new(
            self.config.clone(),
            self.session.clone(),
            Box::new(NoOpVolume),
            move || Box::new(AnalysisSink(sink.clone())),
        );
        let mut events = player.get_player_event_channel();
        player.load(uri, true, (request.start_seconds.max(0.0) * 1000.0) as u32);
        let deadline = Instant::now() + Duration::from_secs(35);
        let mut complete = false;
        while !cancelled.load(Ordering::Acquire) && Instant::now() < deadline {
            while let Ok(event) = events.try_recv() {
                match event {
                    PlayerEvent::EndOfTrack { .. } => complete = true,
                    PlayerEvent::Unavailable { .. } | PlayerEvent::LoadingFailed { .. } => {
                        player.stop();
                        return None;
                    }
                    _ => {}
                }
            }
            let audio = capture.audio.lock().ok()?;
            if audio.len() >= capture.limit {
                complete = true;
            }
            if complete {
                break;
            }
            drop(
                capture
                    .ready
                    .wait_timeout(audio, Duration::from_millis(100))
                    .ok()?,
            );
        }
        player.stop();
        drop(player);
        if !complete || cancelled.load(Ordering::Acquire) {
            return None;
        }
        Some(std::mem::take(&mut *capture.audio.lock().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_sink_keeps_only_the_requested_pcm_window() {
        let capture = Arc::new(Capture {
            audio: Mutex::new(Vec::new()),
            ready: Condvar::new(),
            limit: 16,
        });
        let mut sink = AnalysisSink(capture.clone());
        let mut converter = Converter::new(None);
        sink.write(AudioPacket::Samples(vec![0.25; 32]), &mut converter)
            .unwrap();
        let first = capture.audio.lock().unwrap().clone();
        sink.write(AudioPacket::Samples(vec![0.5; 32]), &mut converter)
            .unwrap();
        assert_eq!(first.len(), 16);
        assert_eq!(*capture.audio.lock().unwrap(), first);
    }

    #[test]
    fn cancelled_analysis_does_not_start_a_decoder_or_enable_history() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _entered = runtime.enter();
        let analyzer = SpotifyBpmAnalyzer::new(
            Session::new(librespot_core::SessionConfig::default(), None),
            runtime.handle().clone(),
            PlayerConfig {
                listening_history: true,
                ..PlayerConfig::default()
            },
        );
        assert!(!analyzer.config.listening_history);
        let request = apricot_platform::BpmAnalysisRequest {
            ffmpeg: None,
            source: "spotify:track:0000000000000000000001".into(),
            start_seconds: 0.0,
            duration_seconds: 72.0,
        };
        assert!(analyzer.analyze(&request, &AtomicBool::new(true)).is_none());
    }

    #[test]
    #[ignore = "requires a saved Spotify login and APRICOT_TEST_FFMPEG"]
    fn real_spotify_bpm_without_connect_or_listening_history() {
        use crate::accounts::{SpotifyAccounts, unprotect_credentials};
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let accounts: SpotifyAccounts = serde_json::from_slice(
            &std::fs::read(
                std::env::var_os("APRICOT_SPOTIFY_TEST_ACCOUNT_FILE").expect("account file"),
            )
            .unwrap(),
        )
        .unwrap();
        let account = accounts
            .accounts
            .iter()
            .find(|account| Some(&account.key) == accounts.active.as_ref())
            .unwrap();
        let credentials =
            serde_json::from_slice(&unprotect_credentials(&account.credentials).unwrap()).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _entered = runtime.enter();
        let session = Session::new(
            librespot_core::SessionConfig {
                device_id: format!("apricot-bpm-test-{}", rand::random::<u64>()),
                ..Default::default()
            },
            None,
        );
        runtime
            .block_on(session.connect(credentials, false))
            .expect("Spotify connection");
        let analyzer = SpotifyBpmAnalyzer::new(
            session.clone(),
            runtime.handle().clone(),
            PlayerConfig {
                listening_history: false,
                ..PlayerConfig::default()
            },
        );
        let request = apricot_platform::BpmAnalysisRequest {
            ffmpeg: Some(
                std::env::var_os("APRICOT_TEST_FFMPEG")
                    .expect("FFmpeg")
                    .into(),
            ),
            source: std::env::var("APRICOT_SPOTIFY_TEST_URI").expect("track URI"),
            start_seconds: 10.0,
            duration_seconds: 40.0,
        };
        let bpm = analyzer.analyze(&request, &AtomicBool::new(false));
        session.shutdown();
        let bpm = bpm.expect("Spotify BPM");
        assert!(bpm.is_finite() && bpm > 0.0);
        eprintln!("Spotify BPM analysis: {bpm:.1}");
    }
}
