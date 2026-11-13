//! Playback tools and deterministic state transitions shared by both native UIs.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.

use std::{collections::BTreeMap, time::Instant};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum TimerClock {
    #[default]
    Wall,
    Playing,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum TimerAction {
    #[default]
    Pause,
    Stop,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum TimerMode {
    #[default]
    Duration,
    TrackEnd,
    ChapterEnd,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct SleepTimerOptions {
    pub mode: TimerMode,
    pub seconds: f64,
    pub clock: TimerClock,
    pub action: TimerAction,
    pub fade_seconds: f64,
}

impl Default for SleepTimerOptions {
    fn default() -> Self {
        Self {
            mode: TimerMode::Duration,
            seconds: 1_800.0,
            clock: TimerClock::Wall,
            action: TimerAction::Pause,
            fade_seconds: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TimerEffect {
    Warning,
    Volume(f64),
    Expire(TimerAction),
}

#[derive(Debug)]
pub struct SleepTimer {
    options: SleepTimerOptions,
    previous_tick: Instant,
    elapsed: f64,
    previous_playing: bool,
    original_volume: f64,
    faded: bool,
    warned: bool,
}

impl SleepTimer {
    pub fn new(
        options: SleepTimerOptions,
        now: Instant,
        volume: f64,
        playing: bool,
    ) -> Result<Self, String> {
        if !options.seconds.is_finite()
            || options.seconds <= 0.0
            || options.seconds > 604_800.0
            || !options.fade_seconds.is_finite()
            || !(0.0..=120.0).contains(&options.fade_seconds)
        {
            return Err("The timer duration or fade is outside the supported range.".to_owned());
        }
        // A short timer needs no "one minute remaining" right after it starts.
        let warned = options.seconds <= 75.0;
        Ok(Self {
            options,
            previous_tick: now,
            elapsed: 0.0,
            previous_playing: playing,
            original_volume: volume,
            faded: false,
            warned,
        })
    }

    pub fn options(&self) -> &SleepTimerOptions {
        &self.options
    }
    pub fn remaining(&self) -> Option<f64> {
        (self.options.mode == TimerMode::Duration)
            .then_some((self.options.seconds - self.elapsed).max(0.0))
    }
    pub fn restore_volume(&self) -> Option<f64> {
        self.faded.then_some(self.original_volume)
    }

    pub fn tick(&mut self, now: Instant, playing: bool) -> Vec<TimerEffect> {
        let delta = now
            .saturating_duration_since(self.previous_tick)
            .as_secs_f64();
        self.previous_tick = now;
        if self.options.clock == TimerClock::Wall || self.previous_playing {
            self.elapsed += delta;
        }
        self.previous_playing = playing;
        let Some(remaining) = self.remaining() else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        if !self.warned && remaining <= 60.0 && remaining > 0.0 {
            self.warned = true;
            effects.push(TimerEffect::Warning);
        }
        if remaining <= 0.0 {
            effects.push(TimerEffect::Expire(self.options.action));
            if self.faded {
                effects.push(TimerEffect::Volume(self.original_volume));
            }
        } else if self.options.fade_seconds > 0.0 && remaining <= self.options.fade_seconds {
            self.faded = true;
            effects.push(TimerEffect::Volume(
                self.original_volume * remaining / self.options.fade_seconds,
            ));
        }
        effects
    }

    /// Only natural media/chapter completion can trigger an end timer. A manual
    /// change deliberately leaves it armed for the newly chosen media.
    pub fn natural_end(&self, chapter: bool) -> Option<TimerAction> {
        match (self.options.mode, chapter) {
            (TimerMode::TrackEnd, false) | (TimerMode::ChapterEnd, true) => {
                Some(self.options.action)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct SilenceSettings {
    pub enabled: bool,
    pub threshold_db: f64,
    pub minimum_seconds: f64,
    pub leading_parts: i32,
    pub leading_sound_seconds: f64,
    pub later_parts: i32,
    pub keep_seconds: f64,
    pub window_seconds: f64,
    pub rms: bool,
}
impl Default for SilenceSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold_db: -30.0,
            minimum_seconds: 0.5,
            leading_parts: 1,
            leading_sound_seconds: 0.2,
            later_parts: -1,
            keep_seconds: 0.2,
            window_seconds: 0.02,
            rms: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct SpatialSettings {
    pub enabled: bool,
    pub file: String,
    pub gain_db: f64,
    pub rotation: f64,
    pub elevation: f64,
    pub step: f64,
}
impl Default for SpatialSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            file: String::new(),
            gain_db: 0.0,
            rotation: 0.0,
            elevation: 0.0,
            step: 5.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct TransitionSettings {
    pub crossfade: bool,
    pub crossfade_seconds: f64,
    pub manual_crossfade: bool,
    pub equal_power: bool,
    pub seek_fade: bool,
    pub pause_fade: bool,
    pub start_fade: bool,
    pub end_fade: bool,
    pub fade_seconds: f64,
}
impl Default for TransitionSettings {
    fn default() -> Self {
        Self {
            crossfade: false,
            crossfade_seconds: 3.0,
            manual_crossfade: false,
            equal_power: true,
            seek_fade: false,
            pause_fade: false,
            start_fade: false,
            end_fade: false,
            fade_seconds: 0.5,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct AudioProcessingSettings {
    pub pan: f64,
    pub pan_step: f64,
    pub mono: bool,
    pub normalize: bool,
    pub silence: SilenceSettings,
    pub spatial: SpatialSettings,
    pub transitions: TransitionSettings,
}

impl Default for AudioProcessingSettings {
    fn default() -> Self {
        Self {
            pan: 0.0,
            pan_step: 5.0,
            mono: false,
            normalize: false,
            silence: SilenceSettings::default(),
            spatial: SpatialSettings::default(),
            transitions: TransitionSettings::default(),
        }
    }
}

/// Errors are translation keys, so every native UI speaks them localized.
impl AudioProcessingSettings {
    /// # Errors
    ///
    /// `audio_processing_unreadable` when the saved settings are damaged.
    pub fn from_settings(settings: &apricot_storage::SettingsDocument) -> Result<Self, String> {
        settings.preserved.get("audio_processing_v1").map_or_else(
            || Ok(Self::default()),
            |value| {
                serde_json::from_value(value.clone())
                    .map_err(|_| "audio_processing_unreadable".to_owned())
            },
        )
    }
    /// The tagged `af` filter, replaced on its own without touching the
    /// equalizer or pitch filters; `None` when nothing is enabled.
    ///
    /// # Errors
    ///
    /// `audio_processing_invalid` for a value outside its range and
    /// `spatial_sofa_missing` when spatialization has no readable file.
    pub fn filter(&self) -> Result<Option<String>, String> {
        let mut filters = Vec::new();
        if !self.pan.is_finite() || !(-100.0..=100.0).contains(&self.pan) {
            return Err("audio_processing_invalid".to_owned());
        }
        if self.mono {
            filters.push("pan=stereo|c0=0.5*c0+0.5*c1|c1=0.5*c0+0.5*c1".to_owned());
        }
        if self.pan != 0.0 {
            let left = 1.0 - (self.pan / 100.0).max(0.0);
            let right = 1.0 + (self.pan / 100.0).min(0.0);
            filters.push(format!("pan=stereo|c0={left:.4}*c0|c1={right:.4}*c1"));
        }
        if self.normalize {
            // About 0.7 s of look-ahead keeps starts and seeks responsive.
            filters.push(
                "dynaudnorm=f=100:g=15:p=0.95:m=10,alimiter=limit=0.95:level=false".to_owned(),
            );
        }
        if self.silence.enabled {
            let s = &self.silence;
            if !s.threshold_db.is_finite()
                || !(-100.0..=0.0).contains(&s.threshold_db)
                || !s.minimum_seconds.is_finite()
                || !(0.01..=60.0).contains(&s.minimum_seconds)
                || !s.leading_sound_seconds.is_finite()
                || !(0.0..=60.0).contains(&s.leading_sound_seconds)
                || !s.keep_seconds.is_finite()
                || !(0.0..=60.0).contains(&s.keep_seconds)
                || !s.window_seconds.is_finite()
                || !(0.0..=10.0).contains(&s.window_seconds)
                || !(0..=100).contains(&s.leading_parts)
                || !(-100..=100).contains(&s.later_parts)
            {
                return Err("audio_processing_invalid".to_owned());
            }
            filters.push(format!("silenceremove=start_periods={}:start_duration={:.4}:start_threshold={:.2}dB:stop_periods={}:stop_duration={:.4}:stop_threshold={:.2}dB:stop_silence={:.4}:window={:.4}:detection={}", s.leading_parts,s.leading_sound_seconds,s.threshold_db,s.later_parts,s.minimum_seconds,s.threshold_db,s.keep_seconds,s.window_seconds,if s.rms {"rms"} else {"peak"}));
        }
        if self.spatial.enabled {
            let s = &self.spatial;
            if !std::path::Path::new(&s.file).is_file() {
                return Err("spatial_sofa_missing".to_owned());
            }
            if !s.gain_db.is_finite()
                || !(-24.0..=24.0).contains(&s.gain_db)
                || !s.rotation.is_finite()
                || !(-360.0..=360.0).contains(&s.rotation)
                || !s.elevation.is_finite()
                || !(-90.0..=90.0).contains(&s.elevation)
            {
                return Err("audio_processing_invalid".to_owned());
            }
            let file = s
                .file
                .replace('\\', "/")
                .replace(':', "\\:")
                .replace('\'', "'\\''");
            filters.push(format!(
                "sofalizer=sofa='{file}':gain={:.2}:rotation={:.2}:elevation={:.2}",
                s.gain_db, s.rotation, s.elevation
            ));
        }
        Ok((!filters.is_empty())
            .then(|| format!("@apricot_processing:lavfi=[{}]", filters.join(","))))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub struct MarkedRange {
    pub start: f64,
    pub end: f64,
}
impl MarkedRange {
    pub fn new(start: f64, end: f64) -> Result<Self, String> {
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
            return Err("The end of an excerpt must be after its beginning.".to_owned());
        }
        Ok(Self {
            start: (start * 1000.0).round() / 1000.0,
            end: (end * 1000.0).round() / 1000.0,
        })
    }
    pub fn bound(self, position: f64) -> f64 {
        position.clamp(self.start, self.end)
    }
}

#[derive(Clone, Debug, Default)]
pub struct SelectionRanges {
    pub media_key: String,
    pub ranges: Vec<MarkedRange>,
    pub selected: Option<usize>,
    pub active: Option<MarkedRange>,
    pub repeat: bool,
}
impl SelectionRanges {
    pub fn for_media(&mut self, key: &str) {
        if self.media_key != key {
            *self = Self {
                media_key: key.to_owned(),
                ..Self::default()
            };
        }
    }
    pub fn remember(&mut self, range: MarkedRange) -> usize {
        let index = self
            .ranges
            .iter()
            .position(|item| *item == range)
            .unwrap_or_else(|| {
                self.ranges.push(range);
                self.ranges.len() - 1
            });
        self.selected = Some(index);
        index
    }
    pub fn constrain(&self, position: f64) -> f64 {
        self.active
            .filter(|_| self.repeat)
            .map_or(position, |range| range.bound(position))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubtitleCue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// SRT and `WebVTT` share timing lines. Ignore headers, cue numbers and VTT
/// positioning; strip markup so the speech engine never reads formatting.
pub fn parse_subtitles(source: &str) -> Result<Vec<SubtitleCue>, String> {
    if source.len() > 32 * 1024 * 1024 {
        return Err("The subtitle file is too large.".to_owned());
    }
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in source.split("\n\n") {
        let lines: Vec<_> = block.lines().collect();
        let Some((index, timing)) = lines
            .iter()
            .enumerate()
            .find(|(_, line)| line.contains("-->"))
        else {
            continue;
        };
        let Some((start, end)) = timing.split_once("-->") else {
            continue;
        };
        let Some(start) = parse_time(start.trim()) else {
            continue;
        };
        let Some(end) = end.split_whitespace().next().and_then(parse_time) else {
            continue;
        };
        if end <= start {
            continue;
        }
        let markup = lines[index + 1..].join(" ");
        let mut inside = false;
        let text: String = markup
            .chars()
            .filter(|c| match c {
                '<' => {
                    inside = true;
                    false
                }
                '>' => {
                    inside = false;
                    false
                }
                _ => !inside,
            })
            .collect();
        let text = html_escape::decode_html_entities(&text).trim().to_owned();
        if !text.is_empty() {
            cues.push(SubtitleCue { start, end, text });
        }
        if cues.len() > 100_000 {
            return Err("The subtitle file contains too many cues.".to_owned());
        }
    }
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(cues)
}

pub fn parse_time(value: &str) -> Option<f64> {
    let normalized = value.trim().replace(',', ".");
    let parts: Vec<_> = normalized.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut seconds = 0.0;
    for part in parts {
        let part = part.parse::<f64>().ok()?;
        if !part.is_finite() || part < 0.0 {
            return None;
        }
        seconds = seconds * 60.0 + part;
    }
    Some(seconds)
}

/// Absolute source positions remain authoritative for bookmarks and resume.
/// The audiobook presentation projects the current chapter over that timeline.
pub fn chapter_span(app: &crate::Application) -> Option<(f64, f64)> {
    if !app.playback_features().audiobook {
        return None;
    }
    let session = app.player_session();
    let chapters = crate::chapters::session_chapters(session);
    let index = chapters
        .iter()
        .rposition(|chapter| chapter.start_seconds <= session.position_seconds())?;
    let chapter = &chapters[index];
    let end = chapter
        .end_seconds
        .or_else(|| chapters.get(index + 1).map(|next| next.start_seconds))
        .or(session.duration_seconds())?;
    (end > chapter.start_seconds).then_some((chapter.start_seconds, end))
}

pub fn displayed_times(app: &crate::Application) -> (f64, Option<f64>) {
    let session = app.player_session();
    chapter_span(app).map_or(
        (session.position_seconds(), session.duration_seconds()),
        |(start, end)| {
            (
                (session.position_seconds() - start).max(0.0),
                Some(end - start),
            )
        },
    )
}

pub fn audiobook_navigation(app: &crate::Application, direction: i32) -> Option<f64> {
    if !app.playback_features().audiobook {
        return None;
    }
    let chapters = crate::chapters::session_chapters(app.player_session());
    crate::chapters::relative_chapter(
        &chapters,
        app.player_session().position_seconds(),
        direction > 0,
    )
    .map(|chapter| chapter.start_seconds)
}

pub fn arm_saved_range(app: &mut crate::Application, bookmark: &apricot_storage::Bookmark) {
    let key = bookmark.media.copy_location().unwrap_or_default();
    let ranges = &mut app.playback_features_mut().ranges;
    ranges.for_media(&key);
    ranges.active = bookmark
        .end_position()
        .and_then(|end| MarkedRange::new(bookmark.position, end).ok());
    ranges.repeat = false;
    app.playback_features_mut().note_manual_seek();
}

/// How the next player start was requested, which decides a crossfade.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionKind {
    /// The next item starts before the current one ends.
    Automatic,
    /// Previous or Next pressed by the user.
    Manual,
}

#[derive(Debug)]
pub struct PlaybackFeatures {
    pub timer: Option<SleepTimer>,
    pub ranges: SelectionRanges,
    pub subtitles_enabled: bool,
    pub subtitle_language: Option<String>,
    pub subtitle_interrupt: bool,
    pub subtitle_remember_language: bool,
    pub subtitle_cues: Vec<SubtitleCue>,
    pub spoken_cue: Option<usize>,
    pub audiobook: bool,
    pub chapter_stop: bool,
    pub chapter_index: Option<usize>,
    pub applied_generation: Option<u64>,
    /// The equalizer profile shown in an open equalizer dialog.
    pub equalizer_preview_preset: Option<String>,
    /// Consumed by the next player start.
    pub pending_transition: Option<TransitionKind>,
    /// The player generation whose automatic crossfade already started.
    pub crossfade_started_generation: Option<u64>,
    pub last_position: Option<(Instant, f64)>,
    pub manual_seek_at: Option<Instant>,
    pub extra: BTreeMap<String, String>,
}

impl Default for PlaybackFeatures {
    fn default() -> Self {
        Self {
            timer: None,
            ranges: SelectionRanges::default(),
            subtitles_enabled: false,
            subtitle_language: None,
            subtitle_interrupt: false,
            subtitle_remember_language: true,
            subtitle_cues: Vec::new(),
            spoken_cue: None,
            audiobook: false,
            chapter_stop: false,
            chapter_index: None,
            applied_generation: None,
            equalizer_preview_preset: None,
            pending_transition: None,
            crossfade_started_generation: None,
            last_position: None,
            manual_seek_at: None,
            extra: BTreeMap::new(),
        }
    }
}

impl PlaybackFeatures {
    pub fn take_timer_effects(&mut self, now: Instant, playing: bool) -> Vec<TimerEffect> {
        let effects = self
            .timer
            .as_mut()
            .map_or_else(Vec::new, |timer| timer.tick(now, playing));
        if effects
            .iter()
            .any(|effect| matches!(effect, TimerEffect::Expire(_)))
        {
            self.timer = None;
        }
        effects
    }
    pub fn cancel_timer(&mut self) -> Option<f64> {
        self.timer.take().and_then(|timer| timer.restore_volume())
    }
    pub fn reset_media(&mut self, key: &str, is_m4b: bool) {
        self.ranges.for_media(key);
        self.audiobook = is_m4b;
        self.chapter_index = None;
        self.subtitle_cues.clear();
        self.spoken_cue = None;
        self.applied_generation = None;
        self.last_position = None;
        self.manual_seek_at = None;
        self.extra.remove("spoken_subtitle");
    }
    pub fn note_manual_seek(&mut self) {
        self.manual_seek_at = Some(Instant::now());
    }
    pub fn close_player(&mut self) {
        let timer = self.timer.take();
        *self = Self::default();
        self.timer = timer;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn wall_timer_counts_pause_and_restores_volume_after_fade() {
        let now = Instant::now();
        let mut timer = SleepTimer::new(
            SleepTimerOptions {
                seconds: 90.0,
                fade_seconds: 30.0,
                ..SleepTimerOptions::default()
            },
            now,
            80.0,
            true,
        )
        .unwrap();
        assert_eq!(
            timer.tick(now + Duration::from_secs(75), false),
            vec![TimerEffect::Warning, TimerEffect::Volume(40.0)]
        );
        assert_eq!(
            timer.tick(now + Duration::from_secs(90), false),
            vec![
                TimerEffect::Expire(TimerAction::Pause),
                TimerEffect::Volume(80.0)
            ]
        );
    }
    #[test]
    fn playing_timer_does_not_count_time_spent_paused() {
        let now = Instant::now();
        let mut timer = SleepTimer::new(
            SleepTimerOptions {
                clock: TimerClock::Playing,
                seconds: 120.0,
                ..SleepTimerOptions::default()
            },
            now,
            50.0,
            false,
        )
        .unwrap();
        assert!(timer.tick(now + Duration::from_secs(100), true).is_empty());
        assert_eq!(timer.remaining(), Some(120.0));
        timer.tick(now + Duration::from_secs(110), false);
        assert_eq!(timer.remaining(), Some(110.0));
        timer.tick(now + Duration::from_secs(210), false);
        assert_eq!(timer.remaining(), Some(110.0));
    }
    #[test]
    fn track_and_chapter_timers_are_distinct() {
        let timer = SleepTimer::new(
            SleepTimerOptions {
                mode: TimerMode::ChapterEnd,
                ..SleepTimerOptions::default()
            },
            Instant::now(),
            50.0,
            true,
        )
        .unwrap();
        assert_eq!(timer.natural_end(false), None);
        assert_eq!(timer.natural_end(true), Some(TimerAction::Pause));
    }
    #[test]
    fn range_precision_and_seek_constraints_are_explicit() {
        let range = MarkedRange::new(1.2344, 2.3456).unwrap();
        assert_eq!(
            range,
            MarkedRange {
                start: 1.234,
                end: 2.346
            }
        );
        let ranges = SelectionRanges {
            active: Some(range),
            repeat: true,
            ..SelectionRanges::default()
        };
        assert_eq!(ranges.constrain(9.0), 2.346);
        assert!(MarkedRange::new(2.0, 1.0).is_err());
    }
    #[test]
    fn webvtt_and_srt_are_read_without_markup() {
        let cues=parse_subtitles("WEBVTT\n\n00:01.250 --> 00:02.500 position:50%\n<i>Hello &amp; goodbye</i>\n\n2\n00:00:03,000 --> 00:00:04,000\nNext line").unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Hello & goodbye");
        assert_eq!(cues[0].start, 1.25);
        assert_eq!(cues[1].start, 3.0);
    }
    #[test]
    fn processing_defaults_leave_the_audio_chain_unchanged() {
        assert_eq!(AudioProcessingSettings::default().filter().unwrap(), None);
        let mut settings = AudioProcessingSettings {
            pan: -100.0,
            ..AudioProcessingSettings::default()
        };
        assert!(settings.filter().unwrap().unwrap().contains("c1=0.0000*c1"));
        settings.pan = f64::NAN;
        assert!(settings.filter().is_err());
    }
}
