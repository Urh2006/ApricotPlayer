//! Audio spike: LibreSpot player into a custom capture sink that paces itself
//! like a real output (bounded buffer ahead of the wall clock). Measures time
//! to first audio, PCM format, seek flush and event ordering.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use librespot_core::{session::Session, spotify_uri::SpotifyUri};
use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    config::{AudioFormat, PlayerConfig},
    convert::Converter,
    decoder::AudioPacket,
    mixer::NoOpVolume,
    player::{Player, PlayerEvent},
};

const RATE: f64 = 44_100.0;
const CHANNELS: f64 = 2.0;
/// How far the simulated output may run ahead of real time.
const AHEAD: Duration = Duration::from_millis(500);

#[derive(Default)]
struct Stats {
    first_sample: Option<Instant>,
    samples: u64,
    packets: u64,
    raw_packets: u64,
    peak: f64,
    /// Output clock origin; reset by `stop`, like an output device flush.
    clock: Option<Instant>,
    clock_samples: u64,
    starts: u32,
    stops: u32,
}

struct CaptureSink {
    stats: Arc<Mutex<Stats>>,
}

impl Sink for CaptureSink {
    fn start(&mut self) -> SinkResult<()> {
        let mut stats = self.stats.lock().unwrap();
        stats.starts += 1;
        stats.clock = None;
        stats.clock_samples = 0;
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        self.stats.lock().unwrap().stops += 1;
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        let wait = {
            let mut stats = self.stats.lock().unwrap();
            stats.packets += 1;
            match packet {
                AudioPacket::Samples(samples) => {
                    let now = Instant::now();
                    stats.first_sample.get_or_insert(now);
                    let clock = *stats.clock.get_or_insert(now);
                    stats.samples += samples.len() as u64;
                    stats.clock_samples += samples.len() as u64;
                    for sample in &samples {
                        stats.peak = stats.peak.max(sample.abs());
                    }
                    let written =
                        Duration::from_secs_f64(stats.clock_samples as f64 / RATE / CHANNELS);
                    (clock + written).checked_duration_since(now + AHEAD)
                }
                AudioPacket::Raw(_) => {
                    stats.raw_packets += 1;
                    None
                }
            }
        };
        if let Some(wait) = wait {
            std::thread::sleep(wait);
        }
        Ok(())
    }
}

pub async fn play_capture(session: Session, uri: &str, secs: u64) -> Result<()> {
    let track = SpotifyUri::from_uri(uri).map_err(|e| anyhow!("uri: {e}"))?;
    let stats = Arc::new(Mutex::new(Stats::default()));
    let sink_stats = stats.clone();
    let config = PlayerConfig {
        position_update_interval: Some(Duration::from_secs(1)),
        ..PlayerConfig::default()
    };
    println!(
        "player config: bitrate={:?} gapless={} normalisation={} format={:?}",
        config.bitrate,
        config.gapless,
        config.normalisation,
        AudioFormat::default()
    );
    let player = Player::new(config, session, Box::new(NoOpVolume), move || {
        Box::new(CaptureSink { stats: sink_stats }) as Box<dyn Sink>
    });
    let mut events = player.get_player_event_channel();
    let started = Instant::now();
    player.load(track, true, 0);

    let seek_at = Duration::from_secs(secs / 2);
    let mut seeked = false;
    let deadline = started + Duration::from_secs(secs);
    loop {
        let timeout = deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            break;
        }
        match tokio::time::timeout(timeout.min(Duration::from_millis(250)), events.recv()).await {
            Ok(Some(event)) => {
                let label = match &event {
                    PlayerEvent::PositionChanged { position_ms, .. } => {
                        format!("PositionChanged {position_ms} ms")
                    }
                    other => format!("{other:?}").chars().take(160).collect(),
                };
                println!("{:>6} ms  {label}", started.elapsed().as_millis());
                if matches!(
                    event,
                    PlayerEvent::EndOfTrack { .. } | PlayerEvent::Unavailable { .. }
                ) {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {}
        }
        if !seeked && started.elapsed() >= seek_at {
            seeked = true;
            let before = stats.lock().unwrap().samples;
            println!(
                "{:>6} ms  >>> seek to 60000 ms (samples so far {before})",
                started.elapsed().as_millis()
            );
            player.seek(60_000);
        }
    }
    player.stop();
    let stats = stats.lock().unwrap();
    println!(
        "time_to_first_audio_ms={:?} samples={} audio_s={:.2} packets={} raw={} peak={:.3} sink_starts={} sink_stops={}",
        stats
            .first_sample
            .map(|t| t.duration_since(started).as_millis()),
        stats.samples,
        stats.samples as f64 / RATE / CHANNELS,
        stats.packets,
        stats.raw_packets,
        stats.peak,
        stats.starts,
        stats.stops
    );
    Ok(())
}

/// Gapless check: start `first` six seconds before its end, preload `second`
/// when LibreSpot asks, load it on EndOfTrack, and track the minimum lead of
/// the paced capture sink. A lead that never reaches zero means the output
/// never ran dry at the track boundary.
pub async fn gapless(session: Session, first: &str, second: &str) -> Result<()> {
    use librespot_metadata::{Metadata, Track};
    let first_uri = SpotifyUri::from_uri(first).map_err(|e| anyhow!("uri: {e}"))?;
    let second_uri = SpotifyUri::from_uri(second).map_err(|e| anyhow!("uri: {e}"))?;
    let track = Track::get(&session, &first_uri)
        .await
        .map_err(|e| anyhow!("metadata: {e}"))?;
    let start_ms = (track.duration as u32).saturating_sub(6_000);
    let stats = Arc::new(Mutex::new(Stats::default()));
    let sink_stats = stats.clone();
    let player = Player::new(
        PlayerConfig::default(),
        session,
        Box::new(NoOpVolume),
        move || Box::new(CaptureSink { stats: sink_stats }) as Box<dyn Sink>,
    );
    let mut events = player.get_player_event_channel();
    let started = Instant::now();
    player.load(first_uri, true, start_ms);
    let mut min_lead: Option<f64> = None;
    let mut boundary: Option<u128> = None;
    let deadline = started + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(20), events.recv()).await
        {
            match &event {
                PlayerEvent::TimeToPreloadNextTrack { .. } => {
                    println!(
                        "{:>6} ms  TimeToPreloadNextTrack -> preload",
                        started.elapsed().as_millis()
                    );
                    player.preload(second_uri.clone());
                }
                PlayerEvent::EndOfTrack { .. } => {
                    boundary = Some(started.elapsed().as_millis());
                    println!(
                        "{:>6} ms  EndOfTrack -> load next",
                        started.elapsed().as_millis()
                    );
                    player.load(second_uri.clone(), true, 0);
                }
                PlayerEvent::Playing { .. }
                | PlayerEvent::Loading { .. }
                | PlayerEvent::Preloading { .. } => {
                    let text: String = format!("{event:?}").chars().take(90).collect();
                    println!("{:>6} ms  {text}", started.elapsed().as_millis());
                }
                _ => {}
            }
        }
        let s = stats.lock().unwrap();
        if let (Some(first), Some(clock)) = (s.first_sample, s.clock)
            && first.elapsed() > Duration::from_secs(1)
        {
            let written = s.clock_samples as f64 / RATE / CHANNELS;
            let lead = written - clock.elapsed().as_secs_f64();
            min_lead = Some(min_lead.map_or(lead, |m: f64| m.min(lead)));
        }
    }
    player.stop();
    let s = stats.lock().unwrap();
    println!(
        "boundary_ms={boundary:?} min_output_lead_ms={:.0} sink_starts={} sink_stops={} audio_s={:.2}",
        min_lead.unwrap_or(f64::NAN) * 1000.0,
        s.starts,
        s.stops,
        s.samples as f64 / RATE / CHANNELS
    );
    Ok(())
}

/// Real-time paced sink that discards audio (Connect spike: no sound).
#[derive(Default)]
pub struct SilentSink {
    inner: Option<CaptureSink>,
}

impl Sink for SilentSink {
    fn start(&mut self) -> SinkResult<()> {
        self.inner
            .get_or_insert_with(|| CaptureSink {
                stats: Arc::default(),
            })
            .start()
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        self.inner
            .get_or_insert_with(|| CaptureSink {
                stats: Arc::default(),
            })
            .write(packet, converter)
    }
}
