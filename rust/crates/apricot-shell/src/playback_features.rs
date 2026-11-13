//! Shared player-tool dialogs and transport decisions.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.

use std::{path::PathBuf, time::Instant};

use crate::{FeatureForm, FormField, FormValue, FormValues};
use apricot_app::{
    Application, PlaybackPhase,
    playback_features::{
        AudioProcessingSettings, MarkedRange, SleepTimer, SleepTimerOptions, TimerAction,
        TimerClock, TimerEffect, TimerMode, parse_time,
    },
};
use apricot_playback::PlaybackCommand;

pub trait PlaybackFeatureUi {
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues>;
    fn announce(&mut self, text: &str);
    fn open_file(&mut self, title: &str, extensions: &[&str]) -> Option<PathBuf>;
    fn form_preview(
        &mut self,
        form: &FeatureForm,
        _preview: &mut dyn FnMut(&FormValues) -> Vec<PlaybackCommand>,
    ) -> Option<FormValues> {
        self.form(form)
    }
    fn speak_subtitle(&mut self, text: &str, _interrupt: bool) {
        self.announce(text);
    }
}

fn number(values: &FormValues, key: &str) -> f64 {
    values
        .get(key)
        .and_then(FormValue::number)
        .unwrap_or_default()
}
fn checked(values: &FormValues, key: &str) -> bool {
    values
        .get(key)
        .and_then(FormValue::checked)
        .unwrap_or(false)
}
fn choice(values: &FormValues, key: &str) -> usize {
    values
        .get(key)
        .and_then(FormValue::choice)
        .unwrap_or_default()
}
fn text<'a>(values: &'a FormValues, key: &str) -> &'a str {
    values
        .get(key)
        .and_then(FormValue::text)
        .unwrap_or_default()
}
fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}
fn set_property(name: &'static str, value: impl ToString) -> PlaybackCommand {
    PlaybackCommand::SetProperty {
        name,
        value: value.to_string(),
    }
}

fn persist(
    app: &mut Application,
    key: &str,
    value: serde_json::Value,
    ui: &mut dyn PlaybackFeatureUi,
) -> bool {
    app.set_feature_settings(key, value);
    match app.save_settings() {
        Ok(()) => true,
        Err(error) => {
            ui.announce(&format!("Settings were not saved: {error}"));
            false
        }
    }
}

fn audio_settings(
    app: &Application,
    ui: &mut dyn PlaybackFeatureUi,
) -> Option<AudioProcessingSettings> {
    match AudioProcessingSettings::from_settings(app.settings()) {
        Ok(settings) => Some(settings),
        Err(error) => {
            ui.announce(&error);
            None
        }
    }
}

fn apply_processing(
    app: &mut Application,
    settings: &AudioProcessingSettings,
    ui: &mut dyn PlaybackFeatureUi,
) -> Vec<PlaybackCommand> {
    let filter = match settings.filter() {
        Ok(filter) => filter,
        Err(error) => {
            ui.announce(&error);
            return Vec::new();
        }
    };
    if !persist(app, "audio_processing_v1", serde_json::json!(settings), ui) {
        return Vec::new();
    }
    configure_pcm_crossfade(app, settings);
    let mut commands = vec![PlaybackCommand::RemoveAudioFilter(
        "@apricot_processing".to_owned(),
    )];
    if let Some(filter) = filter {
        commands.push(PlaybackCommand::AddAudioFilter(filter));
    }
    commands.push(set_property(
        "gapless-audio",
        if settings.transitions.gapless {
            "yes"
        } else {
            "no"
        },
    ));
    ui.announce("Audio settings saved.");
    commands
}

/// `None` means this action belongs to another feature. A handled, cancelled
/// dialog returns an empty command list and has no side effects.
pub fn dispatch_playback_feature(
    app: &mut Application,
    ui: &mut dyn PlaybackFeatureUi,
    action: &str,
) -> Option<Vec<PlaybackCommand>> {
    let mut commands = Vec::new();
    match action {
        "player_equalizer_advanced" => return Some(edit_advanced_equalizer(app, ui)),
        "open_sleep_timer" => {
            let options = app
                .settings()
                .preserved
                .get("sleep_timer_v1")
                .and_then(|value| serde_json::from_value::<SleepTimerOptions>(value.clone()).ok())
                .unwrap_or_default();
            let active = app.playback_features().timer.is_some();
            let form = FeatureForm::new(
                "Sleep timer",
                vec![
                    FormField::checkbox("enabled", "Timer enabled", true),
                    FormField::choice(
                        "mode",
                        "When to finish",
                        strings(&[
                            "After a duration",
                            "At the end of the track",
                            "At the end of the chapter",
                        ]),
                        match options.mode {
                            TimerMode::Duration => 0,
                            TimerMode::TrackEnd => 1,
                            TimerMode::ChapterEnd => 2,
                        },
                    ),
                    FormField::choice(
                        "preset",
                        "Duration preset",
                        strings(&[
                            "Custom duration",
                            "15 minutes",
                            "30 minutes",
                            "45 minutes",
                            "60 minutes",
                            "90 minutes",
                            "120 minutes",
                        ]),
                        0,
                    ),
                    FormField::number(
                        "minutes",
                        "Custom duration in minutes",
                        options.seconds / 60.0,
                        0.016_666_7,
                        10_080.0,
                    ),
                    FormField::choice(
                        "clock",
                        "Count down",
                        strings(&["Real time, including pauses", "Only while playing"]),
                        usize::from(options.clock == TimerClock::Playing),
                    ),
                    FormField::choice(
                        "action",
                        "When the timer finishes",
                        strings(&["Pause and keep position", "Stop playback"]),
                        usize::from(options.action == TimerAction::Stop),
                    ),
                    FormField::checkbox(
                        "fade",
                        "Fade out before finishing",
                        options.fade_seconds > 0.0,
                    ),
                    FormField::number(
                        "fade_seconds",
                        "Fade duration in seconds",
                        if options.fade_seconds > 0.0 {
                            options.fade_seconds
                        } else {
                            30.0
                        },
                        1.0,
                        120.0,
                    ),
                    FormField::read_only(
                        "current",
                        "Current timer",
                        if active {
                            timer_description(app)
                        } else {
                            "No timer is running.".to_owned()
                        },
                    ),
                ],
            );
            let Some(values) = ui.form(&form) else {
                return Some(commands);
            };
            if !checked(&values, "enabled") {
                if let Some(volume) = app.playback_features_mut().cancel_timer() {
                    commands.push(PlaybackCommand::SetVolume(volume));
                }
                ui.announce("Sleep timer cancelled.");
                return Some(commands);
            }
            let mode = match choice(&values, "mode") {
                1 => TimerMode::TrackEnd,
                2 => TimerMode::ChapterEnd,
                _ => TimerMode::Duration,
            };
            if mode != TimerMode::Duration && !app.player_session().is_open() {
                ui.announce("Open a track before setting an end-of-track or chapter timer.");
                return Some(commands);
            }
            let seconds = match choice(&values, "preset") {
                1 => 900.0,
                2 => 1800.0,
                3 => 2700.0,
                4 => 3600.0,
                5 => 5400.0,
                6 => 7200.0,
                _ => number(&values, "minutes") * 60.0,
            };
            let options = SleepTimerOptions {
                mode,
                seconds,
                clock: if choice(&values, "clock") == 1 {
                    TimerClock::Playing
                } else {
                    TimerClock::Wall
                },
                action: if choice(&values, "action") == 1 {
                    TimerAction::Stop
                } else {
                    TimerAction::Pause
                },
                fade_seconds: if checked(&values, "fade") {
                    number(&values, "fade_seconds")
                } else {
                    0.0
                },
            };
            let volume = app
                .player_session()
                .audio()
                .map_or(100.0, |audio| audio.volume);
            match SleepTimer::new(
                options.clone(),
                Instant::now(),
                volume,
                app.player_session().phase() == PlaybackPhase::Playing,
            ) {
                Ok(timer) => {
                    if persist(app, "sleep_timer_v1", serde_json::json!(options), ui) {
                        if let Some(volume) = app.playback_features_mut().cancel_timer() {
                            commands.push(PlaybackCommand::SetVolume(volume));
                        }
                        app.playback_features_mut().timer = Some(timer);
                        ui.announce("Sleep timer started.");
                    }
                }
                Err(error) => ui.announce(&error),
            }
        }
        "cancel_sleep_timer" => {
            if let Some(volume) = app.playback_features_mut().cancel_timer() {
                commands.push(PlaybackCommand::SetVolume(volume));
            }
            ui.announce("Sleep timer cancelled.");
        }
        "sleep_timer_remaining" => ui.announce(&timer_description(app)),
        "player_audio_processing" => {
            let Some(mut settings) = audio_settings(app, ui) else {
                return Some(commands);
            };
            let Some(values) = ui.form(&FeatureForm::new(
                "Audio processing",
                vec![
                    FormField::number(
                        "pan",
                        "Stereo pan: -100 left, 0 centre, 100 right",
                        settings.pan,
                        -100.0,
                        100.0,
                    ),
                    FormField::number("pan_step", "Pan step", settings.pan_step, 1.0, 100.0),
                    FormField::checkbox("mono", "Play audio as mono", settings.mono),
                    FormField::checkbox(
                        "normalize",
                        "Dynamic normalization and limiter",
                        settings.normalize,
                    ),
                ],
            )) else {
                return Some(commands);
            };
            settings.pan = number(&values, "pan");
            settings.pan_step = number(&values, "pan_step");
            settings.mono = checked(&values, "mono");
            settings.normalize = checked(&values, "normalize");
            commands = apply_processing(app, &settings, ui);
        }
        "player_pan_left" | "player_pan_right" => {
            let Some(mut settings) = audio_settings(app, ui) else {
                return Some(commands);
            };
            settings.pan = (settings.pan
                + if action == "player_pan_left" {
                    -settings.pan_step
                } else {
                    settings.pan_step
                })
            .clamp(-100.0, 100.0);
            commands = apply_processing(app, &settings, ui);
            ui.announce(&format!("Pan: {:.0}", settings.pan));
        }
        "player_pan_status" => {
            if let Some(settings) = audio_settings(app, ui) {
                ui.announce(&format!("Pan: {:.0}", settings.pan));
            }
        }
        "player_silence_settings" => {
            let Some(mut settings) = audio_settings(app, ui) else {
                return Some(commands);
            };
            let s = &settings.silence;
            let Some(values) = ui.form(&FeatureForm::new(
                "Silence removal",
                vec![
                    FormField::checkbox("enabled", "Enable silence removal", s.enabled),
                    FormField::number(
                        "threshold",
                        "Silence threshold in dB",
                        s.threshold_db,
                        -100.0,
                        0.0,
                    ),
                    FormField::number(
                        "minimum",
                        "Minimum silence duration in seconds",
                        s.minimum_seconds,
                        0.01,
                        60.0,
                    ),
                    FormField::number(
                        "leading",
                        "Leading silent parts to trim",
                        f64::from(s.leading_parts),
                        0.0,
                        100.0,
                    ),
                    FormField::number(
                        "sound",
                        "Sound required before leading trim stops",
                        s.leading_sound_seconds,
                        0.0,
                        60.0,
                    ),
                    FormField::number(
                        "later",
                        "Later silent parts: -1 for all, 0 to preserve",
                        f64::from(s.later_parts),
                        -100.0,
                        100.0,
                    ),
                    FormField::number(
                        "keep",
                        "Pause to keep after trimmed silence",
                        s.keep_seconds,
                        0.0,
                        60.0,
                    ),
                    FormField::number(
                        "window",
                        "Detection window in seconds",
                        s.window_seconds,
                        0.0,
                        10.0,
                    ),
                    FormField::choice(
                        "detection",
                        "Detection mode",
                        strings(&["Peak", "RMS"]),
                        usize::from(s.rms),
                    ),
                ],
            )) else {
                return Some(commands);
            };
            let s = &mut settings.silence;
            s.enabled = checked(&values, "enabled");
            s.threshold_db = number(&values, "threshold");
            s.minimum_seconds = number(&values, "minimum");
            #[allow(clippy::cast_possible_truncation)]
            {
                s.leading_parts = number(&values, "leading").round() as i32;
                s.later_parts = number(&values, "later").round() as i32;
            }
            s.leading_sound_seconds = number(&values, "sound");
            s.keep_seconds = number(&values, "keep");
            s.window_seconds = number(&values, "window");
            s.rms = choice(&values, "detection") == 1;
            commands = apply_processing(app, &settings, ui);
        }
        "player_spatial_settings" => {
            let Some(mut settings) = audio_settings(app, ui) else {
                return Some(commands);
            };
            let s = &settings.spatial;
            let Some(values) = ui.form(&FeatureForm::new(
                "Headphone spatialization",
                vec![
                    FormField::checkbox(
                        "enabled",
                        "Enable SOFA headphone spatialization",
                        s.enabled,
                    ),
                    FormField::text("file", "SOFA file", &s.file),
                    FormField::number("gain", "Gain in dB", s.gain_db, -24.0, 24.0),
                    FormField::number("rotation", "Rotation in degrees", s.rotation, -360.0, 360.0),
                    FormField::number(
                        "elevation",
                        "Elevation in degrees",
                        s.elevation,
                        -90.0,
                        90.0,
                    ),
                    FormField::number("step", "Movement step in degrees", s.step, 0.1, 90.0),
                ],
            )) else {
                return Some(commands);
            };
            let s = &mut settings.spatial;
            s.enabled = checked(&values, "enabled");
            s.file = text(&values, "file").to_owned();
            s.gain_db = number(&values, "gain");
            s.rotation = number(&values, "rotation");
            s.elevation = number(&values, "elevation");
            s.step = number(&values, "step");
            commands = apply_processing(app, &settings, ui);
        }
        "player_transition_settings" => {
            let Some(mut settings) = audio_settings(app, ui) else {
                return Some(commands);
            };
            let t = &settings.transitions;
            let Some(values) = ui.form(&FeatureForm::new(
                "Playback transitions",
                vec![
                    FormField::checkbox("gapless", "Gapless playback", t.gapless),
                    FormField::checkbox(
                        "crossfade",
                        "Crossfade automatic transitions",
                        t.crossfade,
                    ),
                    FormField::number(
                        "crossfade_seconds",
                        "Crossfade duration in seconds",
                        t.crossfade_seconds,
                        0.001,
                        30.0,
                    ),
                    FormField::checkbox(
                        "manual",
                        "Also crossfade manual previous and next",
                        t.manual_crossfade,
                    ),
                    FormField::choice(
                        "curve",
                        "Crossfade curve",
                        strings(&["Equal power", "Linear"]),
                        usize::from(!t.equal_power),
                    ),
                    FormField::checkbox("seek", "Fade during seeking", t.seek_fade),
                    FormField::checkbox("pause", "Fade during pause and resume", t.pause_fade),
                    FormField::checkbox("start", "Fade at the start", t.start_fade),
                    FormField::checkbox("end", "Fade at the end", t.end_fade),
                    FormField::number(
                        "fade_seconds",
                        "Other fade duration in seconds",
                        t.fade_seconds,
                        0.001,
                        10.0,
                    ),
                ],
            )) else {
                return Some(commands);
            };
            let t = &mut settings.transitions;
            t.gapless = checked(&values, "gapless");
            t.crossfade = checked(&values, "crossfade");
            t.crossfade_seconds = number(&values, "crossfade_seconds");
            t.manual_crossfade = checked(&values, "manual");
            t.equal_power = choice(&values, "curve") == 0;
            t.seek_fade = checked(&values, "seek");
            t.pause_fade = checked(&values, "pause");
            t.start_fade = checked(&values, "start");
            t.end_fade = checked(&values, "end");
            t.fade_seconds = number(&values, "fade_seconds");
            commands = apply_processing(app, &settings, ui);
        }
        "player_go_to_time" | "player_go_to_percent" => {
            let percent = action == "player_go_to_percent";
            let Some(values) = ui.form(&FeatureForm::new(
                if percent {
                    "Go to percentage"
                } else {
                    "Go to time"
                },
                if percent {
                    vec![FormField::number("percent", "Percentage", 0.0, 0.0, 100.0)]
                } else {
                    vec![FormField::text(
                        "time",
                        "Time: hours:minutes:seconds or seconds",
                        crate::format_duration(app.player_session().position_seconds()),
                    )]
                },
            )) else {
                return Some(commands);
            };
            let span = apricot_app::playback_features::chapter_span(app);
            let target = if percent {
                span.map_or_else(
                    || {
                        app.player_session()
                            .duration_seconds()
                            .map(|duration| duration * number(&values, "percent") / 100.0)
                    },
                    |(start, end)| Some(start + (end - start) * number(&values, "percent") / 100.0),
                )
            } else {
                parse_time(text(&values, "time"))
                    .map(|time| span.map_or(time, |(start, end)| (start + time).min(end)))
            };
            if let Some(target) = target {
                let target = app.playback_features().ranges.constrain(target);
                commands.push(PlaybackCommand::SeekAbsolute {
                    seconds: target,
                    exact: true,
                });
            } else {
                ui.announce("The time or duration is not available.");
            }
        }
        "player_elapsed" | "player_remaining" | "player_duration" | "player_percent"
        | "player_title" | "player_artist" => {
            let session = app.player_session();
            let (elapsed, duration) = apricot_app::playback_features::displayed_times(app);
            let message = match action {
                "player_title" => session.current_item().map(|item| item.title.clone()),
                "player_artist" => session.current_item().map(|item| item.channel.clone()),
                "player_elapsed" => Some(crate::format_duration(elapsed)),
                "player_remaining" => {
                    duration.map(|d| crate::format_duration((d - elapsed).max(0.0)))
                }
                "player_duration" => duration.map(crate::format_duration),
                _ => duration
                    .filter(|d| *d > 0.0)
                    .map(|d| format!("{:.0} percent", elapsed / d * 100.0)),
            };
            ui.announce(message.as_deref().unwrap_or("Timing is not available yet."));
        }
        "player_ab_loop" => {
            let active = app.playback_features().ranges.repeat;
            if active {
                app.playback_features_mut().ranges.repeat = false;
                app.playback_features_mut().ranges.active = None;
                ui.announce("A-B repeat off.");
            } else if let Some((start, end)) = app.player_session().clip_range() {
                if let Ok(range) = MarkedRange::new(start, end) {
                    let ranges = &mut app.playback_features_mut().ranges;
                    ranges.remember(range);
                    ranges.active = Some(range);
                    ranges.repeat = true;
                    commands.push(PlaybackCommand::SeekAbsolute {
                        seconds: start,
                        exact: true,
                    });
                    commands.push(PlaybackCommand::SetPaused(false));
                    ui.announce("A-B repeat on.");
                }
            } else {
                ui.announce("Set the start and end markers first.");
            }
        }
        "player_ranges" => {
            let range = app
                .player_session()
                .clip_range()
                .and_then(|(start, end)| MarkedRange::new(start, end).ok());
            if let Some(range) = range {
                app.playback_features_mut().ranges.remember(range);
            }
            let ranges = app.playback_features().ranges.ranges.clone();
            if ranges.is_empty() {
                ui.announce("Set the start and end markers first.");
                return Some(commands);
            }
            let labels = ranges
                .iter()
                .enumerate()
                .map(|(i, r)| format!("{}: {:.3} to {:.3}", i + 1, r.start, r.end))
                .collect();
            let Some(values) = ui.form(&FeatureForm::new(
                "Marked excerpts",
                vec![
                    FormField::choice(
                        "range",
                        "Excerpt",
                        labels,
                        app.playback_features()
                            .ranges
                            .selected
                            .unwrap_or(0)
                            .min(ranges.len() - 1),
                    ),
                    FormField::choice(
                        "action",
                        "Action",
                        strings(&[
                            "Play once",
                            "Repeat",
                            "Save in Bookmarks",
                            "Delete excerpt",
                            "Clear all excerpts",
                        ]),
                        0,
                    ),
                    FormField::text("name", "Bookmark name", ""),
                ],
            )) else {
                return Some(commands);
            };
            let index = choice(&values, "range");
            let Some(range) = ranges.get(index).copied() else {
                return Some(commands);
            };
            match choice(&values, "action") {
                0 | 1 => {
                    let repeat = choice(&values, "action") == 1;
                    let state = &mut app.playback_features_mut().ranges;
                    state.selected = Some(index);
                    state.active = Some(range);
                    state.repeat = repeat;
                    commands.push(PlaybackCommand::SeekAbsolute {
                        seconds: range.start,
                        exact: true,
                    });
                    commands.push(PlaybackCommand::SetPaused(false));
                }
                2 => {
                    if let Some(item) = app.player_session().current_item().cloned() {
                        let name = if text(&values, "name").trim().is_empty() {
                            item.title.clone()
                        } else {
                            text(&values, "name").trim().to_owned()
                        };
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0.0, |d| d.as_secs_f64());
                        match app.add_range_bookmark(&name, range.start, range.end, item, timestamp)
                        {
                            Ok(_) => ui.announce("Excerpt saved in Bookmarks."),
                            Err(error) => ui.announce(&error.to_string()),
                        }
                    }
                }
                3 => {
                    let state = &mut app.playback_features_mut().ranges;
                    state.ranges.remove(index);
                    state.active = None;
                    state.repeat = false;
                    state.selected = None;
                }
                _ => {
                    app.playback_features_mut().ranges = Default::default();
                }
            }
        }
        "player_audio_tracks" | "player_audio_track_next" | "player_audio_track_previous" => {
            let tracks: Vec<_> = app
                .player_session()
                .media_info()
                .tracks
                .iter()
                .filter(|track| track["type"] == "audio")
                .cloned()
                .collect();
            if tracks.is_empty() {
                ui.announce("No alternative audio tracks are available.");
                return Some(commands);
            }
            let selected = tracks
                .iter()
                .position(|track| track["selected"].as_bool() == Some(true))
                .unwrap_or_default();
            let index = if action == "player_audio_track_next" {
                Some((selected + 1) % tracks.len())
            } else if action == "player_audio_track_previous" {
                Some((selected + tracks.len() - 1) % tracks.len())
            } else {
                ui.form(&FeatureForm::new(
                    "Audio tracks",
                    vec![FormField::choice(
                        "track",
                        "Audio track",
                        tracks.iter().map(track_label).collect(),
                        selected,
                    )],
                ))
                .map(|v| choice(&v, "track"))
            };
            if let Some(index) = index.and_then(|i| tracks.get(i))
                && let Some(id) = index["id"].as_i64()
            {
                commands.push(set_property("aid", id));
                ui.announce(&track_label(index));
            }
        }
        "player_subtitle_file" => {
            if let Some(path) = ui.open_file("Load subtitles", &["srt", "vtt", "ass", "ssa", "sub"])
            {
                commands.push(PlaybackCommand::AddSubtitle(
                    path.to_string_lossy().into_owned(),
                ));
                app.playback_features_mut().subtitles_enabled = true;
            }
        }
        "player_subtitle_url" => {
            if let Some(values) = ui.form(&FeatureForm::new(
                "Load subtitle URL",
                vec![FormField::text("url", "Subtitle URL", "")],
            )) {
                match url::Url::parse(text(&values, "url")) {
                    Ok(url) if matches!(url.scheme(), "http" | "https") => {
                        commands.push(PlaybackCommand::AddSubtitle(url.to_string()));
                        app.playback_features_mut().subtitles_enabled = true;
                    }
                    _ => ui.announce("Enter a valid HTTP or HTTPS subtitle URL."),
                }
            }
        }
        "player_subtitles" => {
            let tracks: Vec<_> = app
                .player_session()
                .media_info()
                .tracks
                .iter()
                .filter(|track| track["type"] == "sub")
                .cloned()
                .collect();
            let state = app.playback_features();
            let mut options = vec!["Off".to_owned()];
            options.extend(tracks.iter().map(track_label));
            let selected = tracks
                .iter()
                .position(|track| track["selected"].as_bool() == Some(true))
                .map_or(0, |i| i + 1);
            let Some(values) = ui.form(&FeatureForm::new(
                "Subtitles",
                vec![
                    FormField::choice("track", "Subtitle track", options, selected),
                    FormField::checkbox("speak", "Read subtitles aloud", state.subtitles_enabled),
                    FormField::checkbox(
                        "interrupt",
                        "New lines interrupt current speech",
                        state.subtitle_interrupt,
                    ),
                    FormField::checkbox(
                        "remember",
                        "Remember language until the player is closed",
                        state.subtitle_remember_language,
                    ),
                ],
            )) else {
                return Some(commands);
            };
            let selected = choice(&values, "track");
            let state = app.playback_features_mut();
            state.subtitles_enabled = checked(&values, "speak");
            state.subtitle_interrupt = checked(&values, "interrupt");
            state.subtitle_remember_language = checked(&values, "remember");
            if selected == 0 {
                commands.push(set_property("sid", "no"));
            } else if let Some(track) = tracks.get(selected - 1)
                && let Some(id) = track["id"].as_i64()
            {
                commands.push(set_property("sid", id));
                state.subtitle_language = track["lang"].as_str().map(ToOwned::to_owned);
            }
        }
        "player_audiobook_mode" => {
            let state = app.playback_features_mut();
            state.audiobook = !state.audiobook;
            ui.announce(if state.audiobook {
                "Audiobook mode on."
            } else {
                "Audiobook mode off."
            });
        }
        "player_chapter_stop" => {
            let state = app.playback_features_mut();
            state.chapter_stop = !state.chapter_stop;
            ui.announce(if state.chapter_stop {
                "Stop at the end of the chapter on."
            } else {
                "Stop at the end of the chapter off."
            });
        }
        _ => return None,
    }
    if commands.iter().any(|command| {
        matches!(
            command,
            PlaybackCommand::SeekAbsolute { .. } | PlaybackCommand::SeekRelative { .. }
        )
    }) {
        app.playback_features_mut().note_manual_seek();
    }
    Some(commands)
}

fn track_label(track: &serde_json::Value) -> String {
    let id = track["id"].as_i64().unwrap_or_default();
    let language = track["lang"].as_str().unwrap_or("");
    let title = track["title"].as_str().unwrap_or("");
    let codec = track["codec"].as_str().unwrap_or("");
    format!("{id}: {language} {title} {codec}")
        .trim()
        .to_owned()
}

fn edit_advanced_equalizer(
    app: &mut Application,
    ui: &mut dyn PlaybackFeatureUi,
) -> Vec<PlaybackCommand> {
    use apricot_app::playback_features::{ParametricBand, ParametricProfile, ShelfBand};
    let settings = app.equalizer_settings();
    let preset = app
        .playback_features()
        .extra
        .get("equalizer_edit_profile")
        .cloned()
        .unwrap_or_else(|| settings.effective_preset(&app.equalizer_device_key()));
    let gains = app
        .player_equalizer_state(None)
        .map_or_else(|| settings.gains_for_preset(&preset), |(_, gains)| gains);
    let original_graph = app.player_equalizer_graph(None);
    let mut profiles = app
        .settings()
        .preserved
        .get("equalizer_advanced_v1")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let profile = match profiles.get(&preset) {
        Some(value) => match serde_json::from_value::<ParametricProfile>(value.clone()) {
            Ok(profile) => profile,
            Err(error) => {
                ui.announce(&format!("The equalizer profile could not be read: {error}"));
                return Vec::new();
            }
        },
        None => ParametricProfile::default(),
    };
    let mut fields = vec![FormField::checkbox(
        "reset",
        "Reset this profile to factory defaults",
        false,
    )];
    for band in apricot_core::audio::EQUALIZER_BANDS {
        let params = profile.bands.get(band.id);
        let q = match band.id {
            "31" => 1.7,
            "62" => 2.0,
            "125" => 2.6,
            "250" => 2.3,
            "500" => 2.1,
            "2000" | "4000" => 1.9,
            "8000" => 1.7,
            "16000" => 1.5,
            _ => 2.0,
        };
        fields.push(FormField::number(
            &format!("{}_frequency", band.id),
            format!("{} Hz band: frequency", band.frequency_hz),
            params.map_or(f64::from(band.frequency_hz), |p| p.frequency),
            10.0,
            24_000.0,
        ));
        fields.push(FormField::number(
            &format!("{}_q", band.id),
            format!("{} Hz band: width Q", band.frequency_hz),
            params.map_or(q, |p| p.q),
            0.1,
            10.0,
        ));
        fields.push(FormField::number(
            &format!("{}_gain", band.id),
            format!("{} Hz band: gain dB", band.frequency_hz),
            gains.get(band.id).copied().unwrap_or_default(),
            -24.0,
            24.0,
        ));
    }
    for (id, label, shelf) in [
        ("low", "Low shelf", &profile.low),
        ("high", "High shelf", &profile.high),
    ] {
        fields.push(FormField::number(
            &format!("{id}_frequency"),
            format!("{label}: frequency"),
            shelf.frequency,
            10.0,
            24_000.0,
        ));
        fields.push(FormField::number(
            &format!("{id}_q"),
            format!("{label}: width Q"),
            shelf.q,
            0.1,
            10.0,
        ));
        fields.push(FormField::number(
            &format!("{id}_gain"),
            format!("{label}: gain dB"),
            shelf.gain,
            -24.0,
            24.0,
        ));
    }
    let decode = |values: &FormValues| {
        let mut profile = ParametricProfile::default();
        let mut gains = std::collections::BTreeMap::new();
        if checked(values, "reset") {
            return (profile, apricot_app::equalizer::factory_gains(&preset));
        }
        for band in apricot_core::audio::EQUALIZER_BANDS {
            profile.bands.insert(
                band.id.to_owned(),
                ParametricBand {
                    frequency: number(values, &format!("{}_frequency", band.id)),
                    q: number(values, &format!("{}_q", band.id)),
                },
            );
            gains.insert(
                band.id.to_owned(),
                number(values, &format!("{}_gain", band.id)),
            );
        }
        profile.low = ShelfBand {
            frequency: number(values, "low_frequency"),
            q: number(values, "low_q"),
            gain: number(values, "low_gain"),
        };
        profile.high = ShelfBand {
            frequency: number(values, "high_frequency"),
            q: number(values, "high_q"),
            gain: number(values, "high_gain"),
        };
        (profile, gains)
    };
    let mut preview = |values: &FormValues| {
        let (profile, gains) = decode(values);
        match profile.graph(&gains, settings.clipping_protection) {
            Ok(graph) => vec![PlaybackCommand::SetEqualizerFilter(graph)],
            Err(_) => Vec::new(),
        }
    };
    let Some(values) = ui.form_preview(
        &FeatureForm::new(
            format!(
                "Advanced equalizer: {}",
                settings.preset_label(
                    &apricot_app::embedded_catalog(&app.settings().language),
                    &preset
                )
            ),
            fields,
        ),
        &mut preview,
    ) else {
        return vec![PlaybackCommand::SetEqualizerFilter(original_graph)];
    };
    let (profile, gains) = decode(&values);
    let graph = match profile.graph(&gains, settings.clipping_protection) {
        Ok(graph) => graph,
        Err(error) => {
            ui.announce(&error);
            return vec![PlaybackCommand::SetEqualizerFilter(original_graph)];
        }
    };
    let mut next = settings.clone();
    next.set_preset_gains(&preset, &gains);
    if let Err(error) = app.set_equalizer_settings(&next) {
        ui.announce(&error.to_string());
        return vec![PlaybackCommand::SetEqualizerFilter(original_graph)];
    }
    profiles.insert(preset.clone(), serde_json::json!(profile));
    if !persist(
        app,
        "equalizer_advanced_v1",
        serde_json::Value::Object(profiles),
        ui,
    ) {
        let _ = app.set_equalizer_settings(&settings);
        return vec![PlaybackCommand::SetEqualizerFilter(original_graph)];
    }
    app.set_player_equalizer(Some(apricot_app::EqualizerSession {
        enabled: true,
        gains,
    }));
    app.playback_features_mut()
        .extra
        .insert("equalizer_edit_profile".to_owned(), preset);
    ui.announce("Equalizer profile saved.");
    vec![PlaybackCommand::SetEqualizerFilter(graph)]
}

pub fn timer_description(app: &Application) -> String {
    app.playback_features().timer.as_ref().map_or_else(
        || "No sleep timer is running.".to_owned(),
        |timer| {
            timer.remaining().map_or_else(
                || match timer.options().mode {
                    TimerMode::ChapterEnd => "Waiting for the end of the chapter.".to_owned(),
                    _ => "Waiting for the end of the track.".to_owned(),
                },
                |seconds| {
                    format!(
                        "Sleep timer: {} remaining.",
                        crate::format_duration(seconds)
                    )
                },
            )
        },
    )
}

/// Run before normal automatic advance. Returning true consumes the natural end.
pub fn natural_playback_end(app: &mut Application, chapter: bool) -> Option<Vec<PlaybackCommand>> {
    let action = app
        .playback_features()
        .timer
        .as_ref()
        .and_then(|timer| timer.natural_end(chapter))?;
    let restore = app.playback_features_mut().cancel_timer();
    let mut commands = vec![match action {
        TimerAction::Pause => PlaybackCommand::SetPaused(true),
        TimerAction::Stop => PlaybackCommand::Stop,
    }];
    if let Some(volume) = restore {
        commands.push(PlaybackCommand::SetVolume(volume));
    }
    Some(commands)
}

/// Called on the native playback poll even while menus have focus. State is
/// generation-bound, so a late subtitle or range cannot control the next item.
pub fn tick_playback_features(
    app: &mut Application,
    ui: &mut dyn PlaybackFeatureUi,
) -> Vec<PlaybackCommand> {
    if let Ok(settings) = AudioProcessingSettings::from_settings(app.settings()) {
        configure_pcm_crossfade(app, &settings);
    }
    let session = app.player_session();
    let playing = session.phase() == PlaybackPhase::Playing;
    let generation = session.generation();
    let position = session.position_seconds();
    let duration = session.duration_seconds();
    let key = session
        .current_item()
        .and_then(apricot_core::MediaItem::copy_location)
        .unwrap_or_default();
    let m4b = session
        .current_item()
        .and_then(|item| item.local_path.as_deref())
        .is_some_and(|path| path.to_ascii_lowercase().ends_with(".m4b"));
    let chapters = apricot_app::chapters::session_chapters(session);
    let media_info = session.media_info().clone();
    let mut commands = Vec::new();
    let state = app.playback_features_mut();
    if state.ranges.media_key != key {
        state.reset_media(&key, m4b);
    }
    if state.applied_generation != Some(generation) && playing {
        state.applied_generation = Some(generation);
        match AudioProcessingSettings::from_settings(app.settings())
            .and_then(|settings| settings.filter())
        {
            Ok(Some(filter)) => commands.push(PlaybackCommand::AddAudioFilter(filter)),
            Ok(None) => {}
            Err(error) => ui.announce(&error),
        }
    }
    {
        let state = app.playback_features_mut();
        if state.subtitle_remember_language
            && let Some(language) = &state.subtitle_language
            && let Some(track) = media_info.tracks.iter().find(|track| {
                track["type"] == "sub" && track["lang"].as_str() == Some(language.as_str())
            })
            && let Some(id) = track["id"].as_i64()
        {
            let key = format!("{generation}:{id}");
            if state.extra.get("subtitle_language_applied") != Some(&key) {
                commands.push(set_property("sid", id));
                state
                    .extra
                    .insert("subtitle_language_applied".to_owned(), key);
            }
        }
    }
    for effect in app
        .playback_features_mut()
        .take_timer_effects(Instant::now(), playing)
    {
        match effect {
            TimerEffect::Warning => ui.announce("Sleep timer: one minute remaining."),
            TimerEffect::Volume(volume) => commands.push(PlaybackCommand::SetVolume(volume)),
            TimerEffect::Expire(action) => {
                commands.push(match action {
                    TimerAction::Pause => PlaybackCommand::SetPaused(true),
                    TimerAction::Stop => PlaybackCommand::Stop,
                });
                ui.announce("Sleep timer finished.");
            }
        }
    }
    if playing
        && let Some(range) = app.playback_features().ranges.active
        && position >= range.end - 0.015
    {
        if app.playback_features().ranges.repeat {
            commands.push(PlaybackCommand::SeekAbsolute {
                seconds: range.start,
                exact: true,
            });
        } else {
            commands.push(PlaybackCommand::SetPaused(true));
            commands.push(PlaybackCommand::SeekAbsolute {
                seconds: range.end,
                exact: true,
            });
            app.playback_features_mut().ranges.active = None;
        }
    }
    let chapter = chapters
        .iter()
        .rposition(|chapter| chapter.start_seconds <= position);
    let prior = app.playback_features().chapter_index;
    let now = Instant::now();
    let natural_progress = app
        .playback_features()
        .manual_seek_at
        .is_none_or(|at| now.saturating_duration_since(at).as_secs_f64() > 1.0)
        && app
            .playback_features()
            .last_position
            .is_some_and(|(at, previous)| {
                let speed = app
                    .player_session()
                    .audio()
                    .map_or(1.0, |audio| audio.speed);
                position >= previous
                    && ((position - previous)
                        - now.saturating_duration_since(at).as_secs_f64() * speed)
                        .abs()
                        < 0.75
            });
    if playing && natural_progress && prior.is_some() && chapter != prior {
        let stop = app.playback_features().chapter_stop;
        if let Some(mut timer_commands) = natural_playback_end(app, true) {
            commands.append(&mut timer_commands);
        } else if stop {
            commands.push(PlaybackCommand::SetPaused(true));
        }
    }
    app.playback_features_mut().chapter_index = chapter;
    app.playback_features_mut().last_position = Some((now, position));
    let state = app.playback_features_mut();
    if state.subtitles_enabled && playing {
        if let Some(text) = media_info
            .subtitle_text
            .filter(|text| !text.trim().is_empty())
        {
            if state.extra.get("spoken_subtitle") != Some(&text) {
                ui.speak_subtitle(&text, state.subtitle_interrupt);
                state.extra.insert("spoken_subtitle".to_owned(), text);
            }
        } else {
            state.extra.remove("spoken_subtitle");
        }
    }
    let _ = duration;
    commands
}

fn configure_pcm_crossfade(app: &Application, settings: &AudioProcessingSettings) {
    if let Some(source) = apricot_playback::pcm_source::pcm_source() {
        let end_timer = app
            .playback_features()
            .timer
            .as_ref()
            .is_some_and(|timer| timer.options().mode != TimerMode::Duration);
        let looping = app.playback_features().ranges.active.is_some()
            || app
                .player_session()
                .enabled_toggles()
                .contains(&apricot_app::SessionToggle::Repeat);
        source.configure_crossfade(
            if settings.transitions.crossfade && !end_timer && !looping {
                settings.transitions.crossfade_seconds
            } else {
                0.0
            },
            settings.transitions.manual_crossfade,
            settings.transitions.equal_power,
        );
    }
}

/// Native coordinators call this before EOF, then request the ordinary next
/// item. The existing resolver/queue still decides which item actually follows.
pub fn automatic_crossfade_due(app: &mut Application) -> bool {
    let Ok(settings) = AudioProcessingSettings::from_settings(app.settings()) else {
        return false;
    };
    if !settings.transitions.crossfade {
        return false;
    }
    let session = app.player_session();
    if session.phase() != PlaybackPhase::Playing
        || !session
            .enabled_toggles()
            .contains(&apricot_app::SessionToggle::AutoplayNext)
        || session
            .enabled_toggles()
            .contains(&apricot_app::SessionToggle::Repeat)
        || session.current_item().is_none_or(|item| {
            item.source == apricot_core::MediaSource::Spotify
                || item.kind == apricot_core::MediaKind::Video
        })
        || app.playback_features().ranges.active.is_some()
        || app
            .playback_features()
            .timer
            .as_ref()
            .is_some_and(|timer| timer.options().mode != TimerMode::Duration)
    {
        return false;
    }
    let Some(duration) = session
        .duration_seconds()
        .filter(|duration| *duration > settings.transitions.crossfade_seconds)
    else {
        return false;
    };
    let remaining = duration - session.position_seconds();
    if remaining <= 0.0 || remaining > settings.transitions.crossfade_seconds {
        return false;
    }
    if app.prefetch_candidate().is_none_or(|item| {
        item.kind == apricot_core::MediaKind::Video
            || item.source == apricot_core::MediaSource::Spotify
    }) {
        return false;
    }
    let generation = session.generation().to_string();
    let state = app.playback_features_mut();
    if state.extra.get("automatic_crossfade_generation") == Some(&generation) {
        return false;
    }
    state
        .extra
        .insert("automatic_crossfade_generation".to_owned(), generation);
    state
        .extra
        .insert("automatic_crossfade_launch".to_owned(), "true".to_owned());
    true
}

pub fn configure_transition_launch(
    app: &mut Application,
    options: &mut apricot_playback::MpvLaunchOptions,
) {
    let automatic = app
        .playback_features_mut()
        .extra
        .remove("automatic_crossfade_launch")
        .is_some();
    let Ok(settings) = AudioProcessingSettings::from_settings(app.settings()) else {
        return;
    };
    let t = &settings.transitions;
    let can_overlap = app.player_session().current_item().is_some_and(|item| {
        item.kind != apricot_core::MediaKind::Video
            && item.source != apricot_core::MediaSource::Spotify
    }) && app.playback_features().ranges.active.is_none()
        && app
            .playback_features()
            .timer
            .as_ref()
            .is_none_or(|timer| timer.options().mode == TimerMode::Duration);
    options.transitions = apricot_playback::PlaybackTransitionOptions {
        crossfade_seconds: if can_overlap && t.crossfade && (automatic || t.manual_crossfade) {
            t.crossfade_seconds.clamp(0.001, 30.0)
        } else {
            0.0
        },
        equal_power: t.equal_power,
        seek_fade_seconds: if t.seek_fade {
            t.fade_seconds.clamp(0.001, 10.0)
        } else {
            0.0
        },
        pause_fade_seconds: if t.pause_fade {
            t.fade_seconds.clamp(0.001, 10.0)
        } else {
            0.0
        },
        start_fade_seconds: if t.start_fade {
            t.fade_seconds.clamp(0.001, 10.0)
        } else {
            0.0
        },
    };
    options.gapless = t.gapless;
    if options.transitions.crossfade_seconds > 0.0 {
        options.video_mode = apricot_playback::MpvVideoMode::AudioOnly;
    }
}

struct ShellFeatureUi<'a> {
    ui: &'a mut dyn crate::ShellUi,
    runtime: Option<&'a apricot_playback::PlaybackRuntime>,
    generation: u64,
}
impl PlaybackFeatureUi for ShellFeatureUi<'_> {
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues> {
        self.ui.feature_form(form)
    }
    fn form_preview(
        &mut self,
        form: &FeatureForm,
        preview: &mut dyn FnMut(&FormValues) -> Vec<PlaybackCommand>,
    ) -> Option<FormValues> {
        let runtime = self.runtime;
        let generation = self.generation;
        self.ui.feature_form_preview(form, &mut |values| {
            for command in preview(values) {
                if let Some(runtime) = runtime {
                    let _ = runtime.execute(generation, command);
                }
            }
        })
    }
    fn speak_subtitle(&mut self, text: &str, interrupt: bool) {
        self.ui.speak_subtitle(text, interrupt);
    }
    fn announce(&mut self, text: &str) {
        self.ui.set_status(text, true);
    }
    fn open_file(&mut self, title: &str, extensions: &[&str]) -> Option<PathBuf> {
        self.ui.choose_open_file(title, extensions)
    }
}

impl crate::Shell {
    pub(crate) fn activate_playback_feature(
        &mut self,
        ui: &mut dyn crate::ShellUi,
        action: &str,
    ) -> bool {
        let mut adapter = ShellFeatureUi {
            ui,
            runtime: self.player.playback.as_ref(),
            generation: self.application.player_session().generation(),
        };
        let Some(commands) = dispatch_playback_feature(&mut self.application, &mut adapter, action)
        else {
            return false;
        };
        for command in commands {
            self.execute_player_command(ui, command);
        }
        ui.set_playback_polling(
            self.application.playback_features().timer.is_some() || self.has_playback(),
        );
        true
    }
    pub(crate) fn tick_playback_tools(&mut self, ui: &mut dyn crate::ShellUi) {
        let commands = tick_playback_features(
            &mut self.application,
            &mut ShellFeatureUi {
                ui,
                runtime: None,
                generation: 0,
            },
        );
        for command in commands {
            self.execute_player_command(ui, command);
        }
    }
}
