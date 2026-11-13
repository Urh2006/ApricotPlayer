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
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            ui.announce(
                &catalog
                    .text("settings_save_failed")
                    .replace("{error}", &error.to_string()),
            );
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
        Err(key) => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            ui.announce(catalog.text(&key));
            None
        }
    }
}

fn pan_status(app: &Application, pan: f64) -> String {
    let catalog = apricot_app::embedded_catalog(&app.settings().language);
    let key = if pan.abs() < 0.5 {
        "audio_balance_centre"
    } else if pan < 0.0 {
        "audio_balance_left"
    } else {
        "audio_balance_right"
    };
    catalog
        .text(key)
        .replace("{value}", &format!("{:.0}", pan.abs()))
}

/// The complete audio processing form, shared by the player (F5) and
/// Settings > Playback.
#[must_use]
pub fn audio_processing_form(
    catalog: &apricot_core::TranslationCatalog,
    settings: &AudioProcessingSettings,
) -> FeatureForm {
    let silence = &settings.silence;
    let spatial = &settings.spatial;
    FeatureForm::localized(
        catalog,
        catalog.text("audio_processing"),
        vec![
            FormField::number(
                "pan",
                catalog.text("audio_balance_field"),
                settings.pan,
                -100.0,
                100.0,
            ),
            FormField::number(
                "pan_step",
                catalog.text("audio_balance_step"),
                settings.pan_step,
                1.0,
                100.0,
            ),
            FormField::checkbox("mono", catalog.text("audio_mono"), settings.mono),
            FormField::checkbox(
                "normalize",
                catalog.text("audio_dynamic_normalization"),
                settings.normalize,
            ),
            FormField::checkbox(
                "silence",
                catalog.text("silence_removal_enabled"),
                silence.enabled,
            ),
            FormField::number(
                "threshold",
                catalog.text("silence_threshold"),
                silence.threshold_db,
                -100.0,
                0.0,
            ),
            FormField::number(
                "minimum",
                catalog.text("silence_minimum"),
                silence.minimum_seconds,
                0.01,
                60.0,
            ),
            FormField::number(
                "leading",
                catalog.text("silence_leading_parts"),
                f64::from(silence.leading_parts),
                0.0,
                100.0,
            ),
            FormField::number(
                "sound",
                catalog.text("silence_leading_sound"),
                silence.leading_sound_seconds,
                0.0,
                60.0,
            ),
            FormField::number(
                "later",
                catalog.text("silence_later_parts"),
                f64::from(silence.later_parts),
                -100.0,
                100.0,
            ),
            FormField::number(
                "keep",
                catalog.text("silence_keep"),
                silence.keep_seconds,
                0.0,
                60.0,
            ),
            FormField::number(
                "window",
                catalog.text("silence_window"),
                silence.window_seconds,
                0.0,
                10.0,
            ),
            FormField::choice(
                "detection",
                catalog.text("silence_detection"),
                vec![
                    catalog.text("silence_detection_peak").to_owned(),
                    catalog.text("silence_detection_rms").to_owned(),
                ],
                usize::from(silence.rms),
            ),
            FormField::checkbox(
                "spatial",
                catalog.text("spatial_audio_enabled"),
                spatial.enabled,
            ),
            FormField::file(
                "sofa",
                catalog.text("spatial_sofa_file"),
                &spatial.file,
                &["sofa"],
                catalog.text("browse"),
            ),
            FormField::number(
                "spatial_gain",
                catalog.text("spatial_gain"),
                spatial.gain_db,
                -24.0,
                24.0,
            ),
            FormField::number(
                "rotation",
                catalog.text("spatial_rotation"),
                spatial.rotation,
                -360.0,
                360.0,
            ),
            FormField::number(
                "elevation",
                catalog.text("spatial_elevation"),
                spatial.elevation,
                -90.0,
                90.0,
            ),
            FormField::number(
                "spatial_step",
                catalog.text("spatial_step"),
                spatial.step,
                0.1,
                90.0,
            ),
        ],
    )
}

/// The settings a confirmed audio processing form describes; transitions
/// and other values not in the form are kept.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // Bounded by the form ranges.
pub fn audio_processing_from_values(
    previous: &AudioProcessingSettings,
    values: &FormValues,
) -> AudioProcessingSettings {
    let mut settings = previous.clone();
    settings.pan = number(values, "pan");
    settings.pan_step = number(values, "pan_step");
    settings.mono = checked(values, "mono");
    settings.normalize = checked(values, "normalize");
    let silence = &mut settings.silence;
    silence.enabled = checked(values, "silence");
    silence.threshold_db = number(values, "threshold");
    silence.minimum_seconds = number(values, "minimum");
    silence.leading_parts = number(values, "leading").round() as i32;
    silence.leading_sound_seconds = number(values, "sound");
    silence.later_parts = number(values, "later").round() as i32;
    silence.keep_seconds = number(values, "keep");
    silence.window_seconds = number(values, "window");
    silence.rms = choice(values, "detection") == 1;
    let spatial = &mut settings.spatial;
    spatial.enabled = checked(values, "spatial");
    text(values, "sofa").trim().clone_into(&mut spatial.file);
    spatial.gain_db = number(values, "spatial_gain");
    spatial.rotation = number(values, "rotation");
    spatial.elevation = number(values, "elevation");
    spatial.step = number(values, "spatial_step");
    settings
}

/// The tagged processing filter for a starting player's `af` chain.
#[must_use]
pub fn processing_chain_filter(app: &Application) -> Option<String> {
    AudioProcessingSettings::from_settings(app.settings())
        .ok()
        .and_then(|settings| settings.filter().ok().flatten())
}

fn apply_processing(
    app: &mut Application,
    settings: &AudioProcessingSettings,
    ui: &mut dyn PlaybackFeatureUi,
) -> Vec<PlaybackCommand> {
    let filter = match settings.filter() {
        Ok(filter) => filter,
        Err(key) => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            ui.announce(catalog.text(&key));
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
    commands
}

/// Playback transitions: crossfade and fades, shared by the player menu and
/// Settings > Playback. Gapless playback stays its own Python setting.
#[must_use]
pub fn transitions_form(
    catalog: &apricot_core::TranslationCatalog,
    settings: &AudioProcessingSettings,
) -> FeatureForm {
    let t = &settings.transitions;
    FeatureForm::localized(
        catalog,
        catalog.text("playback_transitions"),
        vec![
            FormField::checkbox(
                "crossfade",
                catalog.text("crossfade_automatic"),
                t.crossfade,
            ),
            FormField::number(
                "crossfade_seconds",
                catalog.text("crossfade_seconds"),
                t.crossfade_seconds,
                0.5,
                30.0,
            ),
            FormField::checkbox(
                "manual",
                catalog.text("crossfade_manual"),
                t.manual_crossfade,
            ),
            FormField::choice(
                "curve",
                catalog.text("crossfade_curve"),
                vec![
                    catalog.text("crossfade_curve_equal_power").to_owned(),
                    catalog.text("crossfade_curve_linear").to_owned(),
                ],
                usize::from(!t.equal_power),
            ),
            FormField::checkbox("seek", catalog.text("fade_on_seek"), t.seek_fade),
            FormField::checkbox("pause", catalog.text("fade_on_pause"), t.pause_fade),
            FormField::checkbox("start", catalog.text("fade_at_start"), t.start_fade),
            FormField::checkbox("end", catalog.text("fade_at_end"), t.end_fade),
            FormField::number(
                "fade_seconds",
                catalog.text("fade_seconds"),
                t.fade_seconds,
                0.05,
                10.0,
            ),
            FormField::checkbox("wrap", catalog.text("list_wrap"), t.wrap_list),
        ],
    )
}

#[must_use]
pub fn transitions_from_values(
    previous: &AudioProcessingSettings,
    values: &FormValues,
) -> AudioProcessingSettings {
    let mut settings = previous.clone();
    let t = &mut settings.transitions;
    t.crossfade = checked(values, "crossfade");
    t.crossfade_seconds = number(values, "crossfade_seconds");
    t.manual_crossfade = checked(values, "manual");
    t.equal_power = choice(values, "curve") == 0;
    t.seek_fade = checked(values, "seek");
    t.pause_fade = checked(values, "pause");
    t.start_fade = checked(values, "start");
    t.end_fade = checked(values, "end");
    t.fade_seconds = number(values, "fade_seconds");
    t.wrap_list = checked(values, "wrap");
    settings
}

/// Saved fades for the running player (Settings Save replaces them).
#[must_use]
pub fn running_transition_options(
    app: &Application,
) -> apricot_playback::PlaybackTransitionOptions {
    running_transitions(
        &AudioProcessingSettings::from_settings(app.settings())
            .unwrap_or_default()
            .transitions,
    )
}

/// Fades for the running player; a crossfade only comes with a start.
fn running_transitions(
    t: &apricot_app::playback_features::TransitionSettings,
) -> apricot_playback::PlaybackTransitionOptions {
    let fade = |enabled: bool| if enabled { t.fade_seconds } else { 0.0 };
    apricot_playback::PlaybackTransitionOptions {
        crossfade_seconds: 0.0,
        equal_power: t.equal_power,
        seek_fade_seconds: fade(t.seek_fade),
        pause_fade_seconds: fade(t.pause_fade),
        start_fade_seconds: fade(t.start_fade),
        end_fade_seconds: fade(t.end_fade),
    }
}

/// The transitions of the player that starts now. The pending start kind is
/// consumed: an automatic advance crossfades when crossfade is on, a manual
/// Previous or Next only when that option is on too, every other start never.
pub fn take_transition_options(
    app: &mut Application,
) -> apricot_playback::PlaybackTransitionOptions {
    let kind = app.playback_features_mut().pending_transition.take();
    let settings = AudioProcessingSettings::from_settings(app.settings()).unwrap_or_default();
    let t = &settings.transitions;
    let mut options = running_transitions(t);
    let crossfade = t.crossfade
        && match kind {
            Some(apricot_app::playback_features::TransitionKind::Automatic) => true,
            Some(apricot_app::playback_features::TransitionKind::Manual) => t.manual_crossfade,
            None => false,
        };
    if crossfade {
        options.crossfade_seconds = t.crossfade_seconds.clamp(0.5, 30.0);
        // The incoming item fades in through the crossfade, not twice.
        options.start_fade_seconds = 0.0;
    }
    options
}

fn excerpt_label(
    catalog: &apricot_core::TranslationCatalog,
    index: usize,
    range: MarkedRange,
) -> String {
    catalog
        .text("excerpt_label")
        .replace("{number}", &(index + 1).to_string())
        .replace(
            "{start}",
            &apricot_app::playback_features::format_precise(range.start),
        )
        .replace(
            "{end}",
            &apricot_app::playback_features::format_precise(range.end),
        )
}

/// Plays one excerpt from its start; once it ends playback pauses, or with
/// `repeat` it starts again until A-B repeat is turned off.
fn play_excerpt(
    app: &mut Application,
    index: usize,
    range: MarkedRange,
    repeat: bool,
) -> Vec<PlaybackCommand> {
    let state = &mut app.playback_features_mut().ranges;
    state.selected = Some(index);
    state.active = Some(range);
    state.repeat = repeat;
    app.playback_features_mut().note_manual_seek();
    vec![
        PlaybackCommand::SeekAbsolute {
            seconds: range.start,
            exact: true,
        },
        PlaybackCommand::SetPaused(false),
    ]
}

/// The marked excerpts dialog: which excerpt, what to do, and its exact times.
#[must_use]
pub fn excerpts_form(
    catalog: &apricot_core::TranslationCatalog,
    ranges: &[MarkedRange],
    selected: usize,
) -> FeatureForm {
    let shown = ranges[selected];
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("excerpts_title"),
        vec![
            FormField::choice(
                "excerpt",
                catalog.text("excerpt_field"),
                ranges
                    .iter()
                    .enumerate()
                    .map(|(index, range)| excerpt_label(catalog, index, *range))
                    .collect(),
                selected,
            ),
            FormField::choice(
                "action",
                catalog.text("excerpt_action"),
                [
                    "excerpt_play_once",
                    "excerpt_repeat",
                    "excerpt_save_bookmark",
                    "excerpt_export",
                    "excerpt_export_all",
                    "excerpt_delete",
                    "excerpts_clear",
                ]
                .iter()
                .map(|key| catalog.text(key).to_owned())
                .collect(),
                0,
            ),
            FormField::text(
                "start",
                catalog.text("excerpt_start"),
                apricot_app::playback_features::format_precise(shown.start),
            ),
            FormField::text(
                "end",
                catalog.text("excerpt_end"),
                apricot_app::playback_features::format_precise(shown.end),
            ),
            FormField::text("name", catalog.text("excerpt_bookmark_name"), ""),
            FormField::checkbox("effects", catalog.text("excerpt_apply_effects"), false),
        ],
    );
    catalog.text("ok").clone_into(&mut form.submit);
    form
}

/// The player's current speed, pitch, equalizer and processing as one
/// `FFmpeg` filter for an exported excerpt; `None` when nothing changes.
#[must_use]
pub fn export_effect_filter(app: &Application) -> Option<String> {
    let mut filters = Vec::new();
    if let Some(audio) = app.player_session().audio() {
        if (audio.pitch - 1.0).abs() > 0.001 && audio.pitch.is_finite() && audio.pitch > 0.0 {
            // Pitch without tempo change: resample, then restore the tempo.
            filters.push(format!(
                "aresample=48000,asetrate={:.0},aresample=48000",
                48_000.0 * audio.pitch
            ));
            filters.extend(atempo_chain(1.0 / audio.pitch));
        }
        if (audio.speed - 1.0).abs() > 0.001 && audio.speed.is_finite() && audio.speed > 0.0 {
            filters.extend(atempo_chain(audio.speed));
        }
    }
    let inner = |graph: &str| -> Option<String> {
        let start = graph.find("lavfi=[")? + "lavfi=[".len();
        Some(graph[start..graph.rfind(']')?].to_owned())
    };
    if let Some(graph) = app
        .player_equalizer_graph(None)
        .and_then(|graph| inner(&graph))
    {
        filters.push(graph);
    }
    if let Some(graph) = processing_chain_filter(app).and_then(|graph| inner(&graph)) {
        filters.push(graph);
    }
    (!filters.is_empty()).then(|| filters.join(","))
}

/// `atempo` accepts 0.5 to 2 per stage.
fn atempo_chain(mut factor: f64) -> Vec<String> {
    let mut stages = Vec::new();
    while factor > 2.0 {
        stages.push("atempo=2".to_owned());
        factor /= 2.0;
    }
    while factor < 0.5 {
        stages.push("atempo=0.5".to_owned());
        factor /= 0.5;
    }
    stages.push(format!("atempo={factor:.6}"));
    stages
}

fn list_position(
    catalog: &apricot_core::TranslationCatalog,
    current: usize,
    length: usize,
) -> String {
    catalog
        .text("list_position_status")
        .replace("{number}", &(current + 1).to_string())
        .replace("{count}", &length.to_string())
}

/// "Item 3 of 12. Total 48:10, 1.2 GB": the list the player plays from.
/// Durations and sizes count only items that report them.
fn list_statistics(app: &Application, catalog: &apricot_core::TranslationCatalog) -> String {
    let Some((current, length)) = app.player_sequence_position() else {
        return catalog.text("list_position_unavailable").to_owned();
    };
    let items = app.player_sequence_items();
    let total: f64 = items.iter().filter_map(|item| item.duration_seconds).sum();
    let before: f64 = items[..current]
        .iter()
        .filter_map(|item| item.duration_seconds)
        .sum::<f64>()
        + app.player_session().position_seconds();
    let bytes: u64 = items
        .iter()
        .filter_map(|item| item.local_path.as_deref())
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .sum();
    let mut parts = vec![list_position(catalog, current, length)];
    if total > 0.0 {
        parts.push(
            catalog
                .text("list_duration_status")
                .replace("{total}", &crate::format_duration(total))
                .replace("{played}", &crate::format_duration(before.min(total)))
                .replace(
                    "{remaining}",
                    &crate::format_duration((total - before).max(0.0)),
                ),
        );
    }
    if bytes > 0 {
        #[allow(clippy::cast_precision_loss)]
        let megabytes = bytes as f64 / 1_048_576.0;
        parts.push(catalog.text("list_size_status").replace(
            "{size}",
            &if megabytes >= 1024.0 {
                format!("{:.1} GB", megabytes / 1024.0)
            } else {
                format!("{megabytes:.0} MB")
            },
        ));
    }
    parts.join(". ")
}

const SLEEP_TIMER_PRESETS: [f64; 6] = [900.0, 1_800.0, 2_700.0, 3_600.0, 5_400.0, 7_200.0];

/// The Sleep timer dialog: the running timer first, then the choices, which
/// start from the last ones used.
#[must_use]
pub fn sleep_timer_form(
    catalog: &apricot_core::TranslationCatalog,
    app: &Application,
    options: &SleepTimerOptions,
) -> FeatureForm {
    let minutes = |seconds: f64| {
        catalog
            .text("sleep_timer_minutes")
            .replace("{minutes}", &format!("{:.0}", seconds / 60.0))
    };
    let mut presets = vec![catalog.text("sleep_timer_custom").to_owned()];
    presets.extend(SLEEP_TIMER_PRESETS.iter().map(|seconds| minutes(*seconds)));
    let preset = SLEEP_TIMER_PRESETS
        .iter()
        .position(|seconds| (seconds - options.seconds).abs() < 0.5)
        .map_or(0, |index| index + 1);
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("sleep_timer"),
        vec![
            FormField::read_only(
                "current",
                catalog.text("sleep_timer_current"),
                timer_description(app),
            ),
            FormField::checkbox("enabled", catalog.text("sleep_timer_run"), true),
            FormField::choice(
                "mode",
                catalog.text("sleep_timer_mode"),
                vec![
                    catalog.text("sleep_timer_mode_duration").to_owned(),
                    catalog.text("sleep_timer_mode_track").to_owned(),
                    catalog.text("sleep_timer_mode_chapter").to_owned(),
                ],
                match options.mode {
                    TimerMode::Duration => 0,
                    TimerMode::TrackEnd => 1,
                    TimerMode::ChapterEnd => 2,
                },
            ),
            FormField::choice(
                "preset",
                catalog.text("sleep_timer_duration"),
                presets,
                preset,
            ),
            FormField::number(
                "minutes",
                catalog.text("sleep_timer_custom_minutes"),
                (options.seconds / 60.0).max(1.0).round(),
                1.0,
                10_080.0,
            ),
            FormField::choice(
                "clock",
                catalog.text("sleep_timer_clock"),
                vec![
                    catalog.text("sleep_timer_clock_wall").to_owned(),
                    catalog.text("sleep_timer_clock_playing").to_owned(),
                ],
                usize::from(options.clock == TimerClock::Playing),
            ),
            FormField::choice(
                "action",
                catalog.text("sleep_timer_action"),
                vec![
                    catalog.text("sleep_timer_action_pause").to_owned(),
                    catalog.text("sleep_timer_action_stop").to_owned(),
                ],
                usize::from(options.action == TimerAction::Stop),
            ),
            FormField::checkbox(
                "fade",
                catalog.text("sleep_timer_fade"),
                options.fade_seconds > 0.0,
            ),
            FormField::number(
                "fade_seconds",
                catalog.text("sleep_timer_fade_seconds"),
                if options.fade_seconds > 0.0 {
                    options.fade_seconds
                } else {
                    30.0
                },
                1.0,
                120.0,
            ),
        ],
    );
    catalog
        .text("sleep_timer_start")
        .clone_into(&mut form.submit);
    form
}

#[must_use]
pub fn sleep_timer_from_values(values: &FormValues) -> SleepTimerOptions {
    SleepTimerOptions {
        mode: match choice(values, "mode") {
            1 => TimerMode::TrackEnd,
            2 => TimerMode::ChapterEnd,
            _ => TimerMode::Duration,
        },
        seconds: match choice(values, "preset") {
            0 => number(values, "minutes") * 60.0,
            index => SLEEP_TIMER_PRESETS
                .get(index - 1)
                .copied()
                .unwrap_or(1_800.0),
        },
        clock: if choice(values, "clock") == 1 {
            TimerClock::Playing
        } else {
            TimerClock::Wall
        },
        action: if choice(values, "action") == 1 {
            TimerAction::Stop
        } else {
            TimerAction::Pause
        },
        fade_seconds: if checked(values, "fade") {
            number(values, "fade_seconds")
        } else {
            0.0
        },
    }
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
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let options = app
                .settings()
                .preserved
                .get("sleep_timer_v1")
                .and_then(|value| serde_json::from_value::<SleepTimerOptions>(value.clone()).ok())
                .unwrap_or_default();
            let Some(values) = ui.form(&sleep_timer_form(&catalog, app, &options)) else {
                return Some(commands);
            };
            if !checked(&values, "enabled") {
                if let Some(volume) = app.playback_features_mut().cancel_timer() {
                    commands.push(PlaybackCommand::SetVolume(volume));
                }
                ui.announce(catalog.text("sleep_timer_cancelled"));
                return Some(commands);
            }
            let options = sleep_timer_from_values(&values);
            if options.mode != TimerMode::Duration && !app.player_session().is_open() {
                ui.announce(catalog.text("sleep_timer_needs_player"));
                return Some(commands);
            }
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
                    // The last choices are remembered; a running countdown is not.
                    if persist(app, "sleep_timer_v1", serde_json::json!(options), ui) {
                        if let Some(volume) = app.playback_features_mut().cancel_timer() {
                            commands.push(PlaybackCommand::SetVolume(volume));
                        }
                        app.playback_features_mut().timer = Some(timer);
                        ui.announce(&timer_description(app));
                    }
                }
                Err(_) => ui.announce(catalog.text("audio_processing_invalid")),
            }
        }
        "cancel_sleep_timer" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let running = app.playback_features().timer.is_some();
            if let Some(volume) = app.playback_features_mut().cancel_timer() {
                commands.push(PlaybackCommand::SetVolume(volume));
            }
            ui.announce(catalog.text(if running {
                "sleep_timer_cancelled"
            } else {
                "sleep_timer_none"
            }));
        }
        "sleep_timer_remaining" => ui.announce(&timer_description(app)),
        "player_audio_processing" => {
            let Some(settings) = audio_settings(app, ui) else {
                return Some(commands);
            };
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let Some(values) = ui.form(&audio_processing_form(&catalog, &settings)) else {
                return Some(commands);
            };
            let settings = audio_processing_from_values(&settings, &values);
            commands = apply_processing(app, &settings, ui);
            if !commands.is_empty() {
                ui.announce(catalog.text("audio_processing_saved"));
            }
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
            ui.announce(&pan_status(app, settings.pan));
        }
        "player_pan_status" => {
            if let Some(settings) = audio_settings(app, ui) {
                ui.announce(&pan_status(app, settings.pan));
            }
        }
        "player_spatial_rotate_left"
        | "player_spatial_rotate_right"
        | "player_spatial_up"
        | "player_spatial_down" => {
            let Some(mut settings) = audio_settings(app, ui) else {
                return Some(commands);
            };
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            if !settings.spatial.enabled {
                ui.announce(catalog.text("spatial_audio_off"));
                return Some(commands);
            }
            let step = settings.spatial.step;
            let spatial = &mut settings.spatial;
            let message = match action {
                "player_spatial_rotate_left" | "player_spatial_rotate_right" => {
                    let delta = if action == "player_spatial_rotate_left" {
                        -step
                    } else {
                        step
                    };
                    // Keep the angle in -180..180 so it reads naturally.
                    let mut rotation = (spatial.rotation + delta) % 360.0;
                    if rotation > 180.0 {
                        rotation -= 360.0;
                    } else if rotation < -180.0 {
                        rotation += 360.0;
                    }
                    spatial.rotation = rotation;
                    catalog
                        .text("spatial_rotation_status")
                        .replace("{value}", &crate::feature_form::format_number(rotation))
                }
                _ => {
                    let delta = if action == "player_spatial_down" {
                        -step
                    } else {
                        step
                    };
                    spatial.elevation = (spatial.elevation + delta).clamp(-90.0, 90.0);
                    catalog.text("spatial_elevation_status").replace(
                        "{value}",
                        &crate::feature_form::format_number(spatial.elevation),
                    )
                }
            };
            commands = apply_processing(app, &settings, ui);
            ui.announce(&message);
        }
        "player_transition_settings" => {
            let Some(settings) = audio_settings(app, ui) else {
                return Some(commands);
            };
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let Some(values) = ui.form(&transitions_form(&catalog, &settings)) else {
                return Some(commands);
            };
            let settings = transitions_from_values(&settings, &values);
            if persist(app, "audio_processing_v1", serde_json::json!(settings), ui) {
                configure_pcm_crossfade(app, &settings);
                commands.push(PlaybackCommand::SetTransitions(running_transitions(
                    &settings.transitions,
                )));
                ui.announce(catalog.text("transitions_saved"));
            }
        }
        "player_go_to_time" | "player_go_to_percent" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let percent = action == "player_go_to_percent";
            let (elapsed, _) = apricot_app::playback_features::displayed_times(app);
            let field = if percent {
                FormField::number(
                    "percent",
                    catalog.text("go_to_percent_field"),
                    0.0,
                    0.0,
                    100.0,
                )
            } else {
                FormField::text(
                    "time",
                    catalog.text("go_to_time_field"),
                    crate::format_duration(elapsed),
                )
            };
            let Some(values) = ui.form(&FeatureForm::localized(
                &catalog,
                catalog.text(if percent {
                    "shortcut_player_go_to_percent"
                } else {
                    "shortcut_player_go_to_time"
                }),
                vec![field],
            )) else {
                return Some(commands);
            };
            // In audiobook mode times and percentages are within the chapter.
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
            match target {
                Some(target) => {
                    let target = app.playback_features().ranges.constrain(target);
                    commands.push(PlaybackCommand::SeekAbsolute {
                        seconds: target,
                        exact: true,
                    });
                }
                None => ui.announce(catalog.text("timing_unavailable")),
            }
        }
        "player_elapsed" | "player_remaining" | "player_duration" | "player_percent"
        | "player_title" | "player_artist" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let session = app.player_session();
            let (elapsed, duration) = apricot_app::playback_features::displayed_times(app);
            let message = match action {
                "player_title" => session
                    .current_item()
                    .map(|item| item.title.clone())
                    .filter(|title| !title.trim().is_empty()),
                "player_artist" => session
                    .current_item()
                    .map(|item| item.channel.clone())
                    .filter(|artist| !artist.trim().is_empty())
                    .or_else(|| Some(catalog.text("artist_unknown").to_owned())),
                "player_elapsed" => Some(
                    catalog
                        .text("time_elapsed_status")
                        .replace("{time}", &crate::format_duration(elapsed)),
                ),
                "player_remaining" => duration.map(|duration| {
                    catalog.text("time_remaining_status").replace(
                        "{time}",
                        &crate::format_duration((duration - elapsed).max(0.0)),
                    )
                }),
                "player_duration" => duration.map(|duration| {
                    catalog
                        .text("time_duration_status")
                        .replace("{time}", &crate::format_duration(duration))
                }),
                _ => duration.filter(|duration| *duration > 0.0).map(|duration| {
                    catalog
                        .text("time_percent_status")
                        .replace("{percent}", &format!("{:.0}", elapsed / duration * 100.0))
                }),
            };
            ui.announce(
                message
                    .as_deref()
                    .unwrap_or_else(|| catalog.text("timing_unavailable")),
            );
        }
        "player_go_to_item" | "player_first_item" | "player_last_item" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let Some((current, length)) = app.player_sequence_position() else {
                ui.announce(catalog.text("list_position_unavailable"));
                return Some(commands);
            };
            let index = match action {
                "player_first_item" => Some(0),
                "player_last_item" => Some(length - 1),
                _ => ui
                    .form(&FeatureForm::localized(
                        &catalog,
                        catalog.text("shortcut_player_go_to_item"),
                        vec![FormField::number(
                            "item",
                            catalog
                                .text("go_to_item_field")
                                .replace("{count}", &length.to_string()),
                            (current + 1) as f64,
                            1.0,
                            length as f64,
                        )],
                    ))
                    .map(|values| {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let number = number(&values, "item").round() as usize;
                        number.saturating_sub(1)
                    }),
            };
            if let Some(index) = index.filter(|index| *index != current) {
                app.playback_features_mut().pending_item_index = Some(index);
            } else if index.is_some() {
                ui.announce(&list_position(&catalog, current, length));
            }
        }
        "player_list_info" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            ui.announce(&list_statistics(app, &catalog));
        }
        slot if slot.starts_with("player_slot_set_") || slot.starts_with("player_slot_go_") => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let number: u8 = slot
                .rsplit('_')
                .next()
                .and_then(|digit| digit.parse().ok())
                .unwrap_or(1);
            let Some(media) = app
                .player_session()
                .current_item()
                .and_then(apricot_core::MediaItem::copy_location)
            else {
                return Some(commands);
            };
            let label = |key: &str, seconds: f64| {
                catalog
                    .text(key)
                    .replace("{slot}", &number.to_string())
                    .replace("{time}", &crate::format_duration(seconds))
            };
            if slot.starts_with("player_slot_set_") {
                let position = app.player_session().position_seconds();
                let value = apricot_app::playback_features::bookmark_slots_with(
                    app.settings(),
                    &media,
                    number,
                    position,
                );
                if persist(
                    app,
                    apricot_app::playback_features::BOOKMARK_SLOTS_KEY,
                    value,
                    ui,
                ) {
                    ui.announce(&label("bookmark_slot_set", position));
                }
            } else if let Some(seconds) =
                apricot_app::playback_features::bookmark_slot(app.settings(), &media, number)
            {
                commands.push(PlaybackCommand::SeekAbsolute {
                    seconds,
                    exact: true,
                });
                ui.announce(&label("bookmark_slot_go", seconds));
            } else {
                ui.announce(
                    &catalog
                        .text("bookmark_slot_empty")
                        .replace("{slot}", &number.to_string()),
                );
            }
        }
        "player_ab_loop" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            if app.playback_features().ranges.repeat {
                let ranges = &mut app.playback_features_mut().ranges;
                ranges.repeat = false;
                ranges.active = None;
                ui.announce(catalog.text("ab_repeat_off"));
            } else if let Some(range) = app
                .player_session()
                .clip_range()
                .and_then(|(start, end)| MarkedRange::new(start, end).ok())
                .or(app.playback_features().ranges.active)
            {
                let index = app.playback_features_mut().ranges.remember(range);
                commands.extend(play_excerpt(app, index, range, true));
                ui.announce(catalog.text("ab_repeat_on"));
            } else {
                ui.announce(catalog.text("excerpts_none"));
            }
        }
        "player_ranges" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            if let Some(range) = app
                .player_session()
                .clip_range()
                .and_then(|(start, end)| MarkedRange::new(start, end).ok())
            {
                app.playback_features_mut().ranges.remember(range);
            }
            let ranges = app.playback_features().ranges.ranges.clone();
            if ranges.is_empty() {
                ui.announce(catalog.text("excerpts_none"));
                return Some(commands);
            }
            let selected = app
                .playback_features()
                .ranges
                .selected
                .unwrap_or(ranges.len() - 1)
                .min(ranges.len() - 1);
            let Some(values) = ui.form(&excerpts_form(&catalog, &ranges, selected)) else {
                return Some(commands);
            };
            let index = choice(&values, "excerpt").min(ranges.len() - 1);
            // Edited times apply to the chosen excerpt; unchanged fields keep it.
            let mut range = ranges[index];
            let shown = ranges[selected];
            let start = parse_time(text(&values, "start"));
            let end = parse_time(text(&values, "end"));
            if let (Some(start), Some(end)) = (start, end)
                && ((start - shown.start).abs() > 0.000_5 || (end - shown.end).abs() > 0.000_5)
            {
                match MarkedRange::new(start, end) {
                    Ok(edited) => {
                        range = edited;
                        app.playback_features_mut().ranges.ranges[index] = edited;
                    }
                    Err(_) => {
                        ui.announce(catalog.text("excerpt_invalid_times"));
                        return Some(commands);
                    }
                }
            }
            let effects = checked(&values, "effects");
            match choice(&values, "action") {
                0 | 1 => {
                    commands.extend(play_excerpt(
                        app,
                        index,
                        range,
                        choice(&values, "action") == 1,
                    ));
                }
                2 => {
                    if let Some(item) = app.player_session().current_item().cloned() {
                        let name = if text(&values, "name").trim().is_empty() {
                            excerpt_label(&catalog, index, range)
                        } else {
                            text(&values, "name").trim().to_owned()
                        };
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0.0, |d| d.as_secs_f64());
                        match app.add_range_bookmark(&name, range.start, range.end, item, timestamp)
                        {
                            Ok(_) => ui.announce(catalog.text("excerpt_saved")),
                            Err(error) => ui.announce(&error.to_string()),
                        }
                    }
                }
                3 | 4 => {
                    let filter = effects.then(|| export_effect_filter(app)).flatten();
                    let chosen: Vec<MarkedRange> = if choice(&values, "action") == 3 {
                        vec![range]
                    } else {
                        app.playback_features().ranges.ranges.clone()
                    };
                    app.playback_features_mut()
                        .pending_exports
                        .extend(chosen.into_iter().map(|range| {
                            apricot_app::playback_features::ExcerptExport {
                                start: range.start,
                                end: range.end,
                                audio_filter: filter.clone(),
                            }
                        }));
                }
                5 => {
                    let state = &mut app.playback_features_mut().ranges;
                    state.ranges.remove(index);
                    state.active = None;
                    state.repeat = false;
                    state.selected = None;
                    ui.announce(catalog.text("excerpt_deleted"));
                }
                _ => {
                    let key = app.playback_features().ranges.media_key.clone();
                    let state = &mut app.playback_features_mut().ranges;
                    *state = apricot_app::playback_features::SelectionRanges::default();
                    state.media_key = key;
                    ui.announce(catalog.text("excerpts_cleared"));
                }
            }
        }
        "player_range_previous" | "player_range_next" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let ranges = app.playback_features().ranges.ranges.clone();
            if ranges.is_empty() {
                ui.announce(catalog.text("excerpts_none"));
                return Some(commands);
            }
            let current = app.playback_features().ranges.selected;
            let index = match (action, current) {
                ("player_range_next", Some(index)) => (index + 1).min(ranges.len() - 1),
                ("player_range_next", None) => 0,
                (_, Some(index)) => index.saturating_sub(1),
                (_, None) => ranges.len() - 1,
            };
            ui.announce(&excerpt_label(&catalog, index, ranges[index]));
            commands.extend(play_excerpt(app, index, ranges[index], false));
        }
        "player_audio_tracks" | "player_audio_track_next" | "player_audio_track_previous" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let tracks = media_tracks(app, "audio");
            if tracks.len() < 2 {
                ui.announce(catalog.text("audio_tracks_none"));
                return Some(commands);
            }
            let selected = tracks
                .iter()
                .position(|track| track["selected"].as_bool() == Some(true))
                .unwrap_or_default();
            let index = match action {
                "player_audio_track_next" => Some((selected + 1) % tracks.len()),
                "player_audio_track_previous" => Some((selected + tracks.len() - 1) % tracks.len()),
                _ => ui
                    .form(&FeatureForm::localized(
                        &catalog,
                        catalog.text("audio_tracks_title"),
                        vec![FormField::choice(
                            "track",
                            catalog.text("audio_track_field"),
                            tracks
                                .iter()
                                .map(|track| track_label(&catalog, track))
                                .collect(),
                            selected,
                        )],
                    ))
                    .map(|values| choice(&values, "track")),
            };
            if let Some(track) = index.and_then(|index| tracks.get(index))
                && let Some(id) = track["id"].as_i64()
            {
                commands.push(set_property("aid", id));
                ui.announce(&track_label(&catalog, track));
            }
        }
        "player_subtitle_file" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            if let Some(path) = ui.open_file(
                catalog.text("subtitles_load_file"),
                &["srt", "vtt", "ass", "ssa", "sub"],
            ) {
                commands.push(PlaybackCommand::AddSubtitle(
                    path.to_string_lossy().into_owned(),
                ));
                // A chosen subtitle file is meant to be read aloud.
                app.playback_features_mut().subtitles_enabled = true;
                ui.announce(catalog.text("subtitles_loaded"));
            }
        }
        "player_subtitle_url" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            if let Some(values) = ui.form(&FeatureForm::localized(
                &catalog,
                catalog.text("subtitles_load_url"),
                vec![FormField::text("url", catalog.text("subtitles_url"), "")],
            )) {
                match url::Url::parse(text(&values, "url").trim()) {
                    Ok(url) if matches!(url.scheme(), "http" | "https") => {
                        commands.push(PlaybackCommand::AddSubtitle(url.to_string()));
                        app.playback_features_mut().subtitles_enabled = true;
                        ui.announce(catalog.text("subtitles_loaded"));
                    }
                    _ => ui.announce(catalog.text("subtitles_url_invalid")),
                }
            }
        }
        "player_subtitles" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let tracks = media_tracks(app, "sub");
            let preferences = SubtitlePreferences::from_settings(app.settings());
            let mut options = vec![catalog.text("subtitles_off").to_owned()];
            options.extend(tracks.iter().map(|track| track_label(&catalog, track)));
            let selected = tracks
                .iter()
                .position(|track| track["selected"].as_bool() == Some(true))
                .map_or(0, |index| index + 1);
            let Some(values) = ui.form(&FeatureForm::localized(
                &catalog,
                catalog.text("subtitles_title"),
                vec![
                    FormField::choice("track", catalog.text("subtitles_track"), options, selected),
                    FormField::checkbox(
                        "speak",
                        catalog.text("subtitles_speak"),
                        app.playback_features().subtitles_enabled,
                    ),
                    FormField::checkbox(
                        "interrupt",
                        catalog.text("subtitles_interrupt"),
                        preferences.interrupt,
                    ),
                    FormField::checkbox(
                        "remember",
                        catalog.text("subtitles_remember_language"),
                        preferences.remember_language,
                    ),
                ],
            )) else {
                return Some(commands);
            };
            let preferences = SubtitlePreferences {
                interrupt: checked(&values, "interrupt"),
                remember_language: checked(&values, "remember"),
            };
            persist(app, "subtitles_v1", serde_json::json!(preferences), ui);
            let selected = choice(&values, "track");
            let state = app.playback_features_mut();
            state.subtitles_enabled = checked(&values, "speak");
            state.subtitle_interrupt = preferences.interrupt;
            state.subtitle_remember_language = preferences.remember_language;
            if selected == 0 {
                commands.push(set_property("sid", "no"));
                state.subtitle_language = None;
            } else if let Some(track) = tracks.get(selected - 1)
                && let Some(id) = track["id"].as_i64()
            {
                commands.push(set_property("sid", id));
                state.subtitle_language = track["lang"].as_str().map(ToOwned::to_owned);
            }
        }
        "player_audiobook_mode" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let state = app.playback_features_mut();
            state.audiobook = !state.audiobook;
            ui.announce(catalog.text(if state.audiobook {
                "audiobook_mode_on"
            } else {
                "audiobook_mode_off"
            }));
        }
        "player_chapter_stop" => {
            let catalog = apricot_app::embedded_catalog(&app.settings().language);
            let state = app.playback_features_mut();
            state.chapter_stop = !state.chapter_stop;
            ui.announce(catalog.text(if state.chapter_stop {
                "chapter_stop_on"
            } else {
                "chapter_stop_off"
            }));
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

/// Subtitle choices kept between sessions (the language itself is kept only
/// until the player is closed).
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct SubtitlePreferences {
    /// A new line interrupts the one being read; by default it waits.
    pub interrupt: bool,
    pub remember_language: bool,
}

impl Default for SubtitlePreferences {
    fn default() -> Self {
        Self {
            interrupt: false,
            remember_language: true,
        }
    }
}

impl SubtitlePreferences {
    #[must_use]
    pub fn from_settings(settings: &apricot_storage::SettingsDocument) -> Self {
        settings
            .preserved
            .get("subtitles_v1")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }
}

fn media_tracks(app: &Application, kind: &str) -> Vec<serde_json::Value> {
    app.player_session()
        .media_info()
        .tracks
        .iter()
        .filter(|track| track["type"] == kind)
        .cloned()
        .collect()
}

/// "English, Commentary, aac 2 channels": what a track is, without ids.
fn track_label(catalog: &apricot_core::TranslationCatalog, track: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(language) = track["lang"].as_str().filter(|value| !value.is_empty()) {
        parts.push(language.to_owned());
    }
    if let Some(title) = track["title"].as_str().filter(|value| !value.is_empty()) {
        parts.push(title.to_owned());
    }
    if let Some(codec) = track["codec"].as_str().filter(|value| !value.is_empty()) {
        parts.push(codec.to_owned());
    }
    if let Some(channels) = track["demux-channel-count"].as_i64() {
        parts.push(
            catalog
                .text("audio_track_channels")
                .replace("{count}", &channels.to_string()),
        );
    }
    if track["external"].as_bool() == Some(true) {
        parts.push(catalog.text("track_external").to_owned());
    }
    let number = track["id"].as_i64().unwrap_or_default().to_string();
    if parts.is_empty() {
        return catalog.text("track_number").replace("{number}", &number);
    }
    format!(
        "{}: {}",
        catalog.text("track_number").replace("{number}", &number),
        parts.join(", ")
    )
}

/// The advanced equalizer of the profile shown in the equalizer dialog: band
/// frequencies and widths and two shelves. Changes are heard at once; Save
/// keeps them, Cancel restores the previous sound.
fn edit_advanced_equalizer(
    app: &mut Application,
    ui: &mut dyn PlaybackFeatureUi,
) -> Vec<PlaybackCommand> {
    use apricot_app::equalizer_advanced::{
        AdvancedEqualizer, BandShape, FREQUENCY_RANGE, SHELF_GAIN_RANGE, Shelf, WIDTH_RANGE,
    };
    let catalog = apricot_app::embedded_catalog(&app.settings().language);
    let settings = app.equalizer_settings();
    let preset = app.equalizer_graph_preset();
    let current = app.equalizer_advanced(&preset);
    let gains = app
        .player_equalizer_state(None)
        .map_or_else(|| settings.gains_for_preset(&preset), |(_, gains)| gains);
    let original_graph = app.player_equalizer_graph(None);
    let mut fields = vec![FormField::checkbox(
        "restore",
        catalog.text("equalizer_advanced_restore"),
        false,
    )];
    for band in apricot_core::audio::EQUALIZER_BANDS {
        let Some(shape) = current.band(band.id) else {
            continue;
        };
        let name = apricot_app::equalizer::band_label(band.id);
        fields.push(FormField::number(
            &format!("{}_frequency", band.id),
            catalog
                .text("equalizer_band_frequency")
                .replace("{band}", name),
            shape.frequency,
            FREQUENCY_RANGE.0,
            FREQUENCY_RANGE.1,
        ));
        fields.push(FormField::number(
            &format!("{}_q", band.id),
            catalog.text("equalizer_band_width").replace("{band}", name),
            shape.q,
            WIDTH_RANGE.0,
            WIDTH_RANGE.1,
        ));
    }
    for (id, key, shelf) in [
        ("low", "equalizer_low_shelf", current.low_shelf),
        ("high", "equalizer_high_shelf", current.high_shelf),
    ] {
        let name = catalog.text(key);
        fields.push(FormField::number(
            &format!("{id}_frequency"),
            catalog
                .text("equalizer_band_frequency")
                .replace("{band}", name),
            shelf.frequency,
            FREQUENCY_RANGE.0,
            FREQUENCY_RANGE.1,
        ));
        fields.push(FormField::number(
            &format!("{id}_q"),
            catalog.text("equalizer_band_width").replace("{band}", name),
            shelf.q,
            WIDTH_RANGE.0,
            WIDTH_RANGE.1,
        ));
        fields.push(FormField::number(
            &format!("{id}_gain"),
            catalog.text("equalizer_shelf_gain").replace("{band}", name),
            shelf.gain,
            SHELF_GAIN_RANGE.0,
            SHELF_GAIN_RANGE.1,
        ));
    }
    let decode = |values: &FormValues| {
        let mut advanced = AdvancedEqualizer::default();
        if checked(values, "restore") {
            return advanced;
        }
        for band in apricot_core::audio::EQUALIZER_BANDS {
            advanced.set_band(
                band.id,
                BandShape {
                    frequency: number(values, &format!("{}_frequency", band.id)),
                    q: number(values, &format!("{}_q", band.id)),
                },
            );
        }
        advanced.low_shelf = Shelf {
            frequency: number(values, "low_frequency"),
            q: number(values, "low_q"),
            gain: number(values, "low_gain"),
        };
        advanced.high_shelf = Shelf {
            frequency: number(values, "high_frequency"),
            q: number(values, "high_q"),
            gain: number(values, "high_gain"),
        };
        advanced
    };
    let protection = settings.clipping_protection;
    let mut preview = |values: &FormValues| {
        vec![PlaybackCommand::SetEqualizerFilter(
            decode(values).graph(&gains, protection),
        )]
    };
    let title = catalog
        .text("equalizer_advanced_title")
        .replace("{profile}", &settings.preset_label(&catalog, &preset));
    let Some(values) = ui.form_preview(
        &FeatureForm::localized(&catalog, title, fields),
        &mut preview,
    ) else {
        return vec![PlaybackCommand::SetEqualizerFilter(original_graph)];
    };
    let advanced = decode(&values);
    app.set_equalizer_advanced(&preset, &advanced);
    if let Err(error) = app.save_settings() {
        ui.announce(
            &catalog
                .text("settings_save_failed")
                .replace("{error}", &error.to_string()),
        );
        return vec![PlaybackCommand::SetEqualizerFilter(original_graph)];
    }
    ui.announce(catalog.text("equalizer_advanced_saved"));
    vec![PlaybackCommand::SetEqualizerFilter(
        app.player_equalizer_graph(None),
    )]
}

pub fn timer_description(app: &Application) -> String {
    let catalog = apricot_app::embedded_catalog(&app.settings().language);
    let Some(timer) = app.playback_features().timer.as_ref() else {
        return catalog.text("sleep_timer_none").to_owned();
    };
    timer.remaining().map_or_else(
        || {
            catalog
                .text(match timer.options().mode {
                    TimerMode::ChapterEnd => "sleep_timer_waiting_chapter",
                    _ => "sleep_timer_waiting_track",
                })
                .to_owned()
        },
        |seconds| {
            catalog
                .text("sleep_timer_remaining_status")
                .replace("{time}", &crate::format_duration(seconds))
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
    let preferences = SubtitlePreferences::from_settings(app.settings());
    let state = app.playback_features_mut();
    state.subtitle_interrupt = preferences.interrupt;
    state.subtitle_remember_language = preferences.remember_language;
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
            TimerEffect::Warning => ui.announce(
                apricot_app::embedded_catalog(&app.settings().language).text("sleep_timer_warning"),
            ),
            TimerEffect::Volume(volume) => commands.push(PlaybackCommand::SetVolume(volume)),
            TimerEffect::Expire(action) => {
                commands.push(match action {
                    TimerAction::Pause => PlaybackCommand::SetPaused(true),
                    TimerAction::Stop => PlaybackCommand::Stop,
                });
                ui.announce(
                    apricot_app::embedded_catalog(&app.settings().language)
                        .text("sleep_timer_finished"),
                );
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
                // Python plays a related video at the end instead of the next item.
                || (app.settings().autoplay_related && item.youtube_video_id().is_some())
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
    let generation = session.generation();
    let state = app.playback_features_mut();
    if state.crossfade_started_generation == Some(generation) {
        return false;
    }
    state.crossfade_started_generation = Some(generation);
    state.pending_transition = Some(apricot_app::playback_features::TransitionKind::Automatic);
    true
}

pub fn configure_transition_launch(
    app: &mut Application,
    options: &mut apricot_playback::MpvLaunchOptions,
) {
    let mut transitions = take_transition_options(app);
    // Only audio that mpv plays can overlap; video and Spotify never do, and
    // neither does a marked range or an end-of-track sleep timer.
    let can_overlap = app.player_session().current_item().is_some_and(|item| {
        item.kind != apricot_core::MediaKind::Video
            && item.source != apricot_core::MediaSource::Spotify
    }) && app.playback_features().ranges.active.is_none()
        && app
            .playback_features()
            .timer
            .as_ref()
            .is_none_or(|timer| timer.options().mode == TimerMode::Duration);
    if !can_overlap {
        transitions.crossfade_seconds = 0.0;
    }
    options.transitions = transitions;
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
        if let Some(index) = self
            .application
            .playback_features_mut()
            .pending_item_index
            .take()
        {
            self.navigate_player_to_index(ui, index);
        }
        let exports = std::mem::take(&mut self.application.playback_features_mut().pending_exports);
        for export in exports {
            self.export_clip_range(
                ui,
                apricot_app::DownloadChoice::Audio,
                export.start,
                export.end,
                export.audio_filter,
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_processing_form_round_trips_and_keeps_transitions() {
        let catalog = apricot_app::english_catalog();
        let mut previous = AudioProcessingSettings::default();
        previous.transitions.crossfade = true;
        let form = audio_processing_form(&catalog, &previous);
        assert_eq!(form.title, "Audio processing");
        assert_eq!(form.submit, "Save");
        let mut values = form
            .validate(
                &form
                    .fields
                    .iter()
                    .filter_map(|field| {
                        let value = match &field.kind {
                            crate::FormFieldKind::Number { value, .. } => FormValue::Number(*value),
                            crate::FormFieldKind::Checkbox { checked } => {
                                FormValue::Checkbox(*checked)
                            }
                            crate::FormFieldKind::Choice { selected, .. } => {
                                FormValue::Choice(*selected)
                            }
                            crate::FormFieldKind::Text { value, .. }
                            | crate::FormFieldKind::File { value, .. } => {
                                FormValue::Text(value.clone())
                            }
                            crate::FormFieldKind::ReadOnly { .. } => return None,
                        };
                        Some((field.id.clone(), value))
                    })
                    .collect(),
            )
            .expect("defaults are valid");
        values.insert("mono".to_owned(), FormValue::Checkbox(true));
        values.insert("pan".to_owned(), FormValue::Number(-40.0));
        let settings = audio_processing_from_values(&previous, &values);
        assert!(settings.mono);
        assert!((settings.pan + 40.0).abs() < f64::EPSILON);
        assert!(settings.transitions.crossfade, "transitions are kept");
        let filter = settings.filter().expect("valid").expect("audible");
        assert!(
            filter.starts_with("@apricot_processing:lavfi=["),
            "{filter}"
        );
    }

    #[test]
    fn missing_sofa_file_is_a_translation_key() {
        let mut settings = AudioProcessingSettings::default();
        settings.spatial.enabled = true;
        settings.spatial.file = "Z:/missing/head.sofa".to_owned();
        let key = settings.filter().expect_err("missing file");
        assert_eq!(
            apricot_app::english_catalog().text(&key),
            "Choose an existing SOFA file for headphone spatialization."
        );
    }
}

#[cfg(test)]
mod sleep_timer_tests {
    use super::*;

    #[test]
    fn sleep_timer_form_remembers_the_last_preset_and_reports_no_timer() {
        let mut shell = crate::tests::keyboard::shell();
        let catalog = apricot_app::english_catalog();
        let options = SleepTimerOptions {
            seconds: 2_700.0,
            action: TimerAction::Stop,
            ..SleepTimerOptions::default()
        };
        let form = sleep_timer_form(&catalog, &shell.application, &options);
        assert_eq!(form.title, "Sleep timer");
        assert_eq!(form.submit, "Start");
        assert!(matches!(
            &form.fields[0].kind,
            crate::FormFieldKind::ReadOnly { value } if value == "No sleep timer is running."
        ));
        assert!(matches!(
            &form.fields[3].kind,
            crate::FormFieldKind::Choice { options, selected: 3 } if options[3] == "45 minutes"
        ));
        let values: FormValues = [
            ("enabled".to_owned(), FormValue::Checkbox(true)),
            ("mode".to_owned(), FormValue::Choice(0)),
            ("preset".to_owned(), FormValue::Choice(0)),
            ("minutes".to_owned(), FormValue::Number(10.0)),
            ("clock".to_owned(), FormValue::Choice(1)),
            ("action".to_owned(), FormValue::Choice(1)),
            ("fade".to_owned(), FormValue::Checkbox(false)),
            ("fade_seconds".to_owned(), FormValue::Number(30.0)),
        ]
        .into_iter()
        .collect();
        let chosen = sleep_timer_from_values(&values);
        assert!((chosen.seconds - 600.0).abs() < f64::EPSILON);
        assert_eq!(chosen.clock, TimerClock::Playing);
        assert_eq!(chosen.action, TimerAction::Stop);
        assert!(chosen.fade_seconds.abs() < f64::EPSILON);
        shell.application.playback_features_mut().timer =
            Some(SleepTimer::new(chosen, Instant::now(), 50.0, false).expect("timer"));
        assert_eq!(
            timer_description(&shell.application),
            "Sleep timer: 10:00 remaining."
        );
    }
}

#[cfg(test)]
mod excerpt_tests {
    use super::*;

    #[test]
    fn excerpt_labels_use_milliseconds_and_effects_build_one_filter() {
        let catalog = apricot_app::english_catalog();
        let range = MarkedRange::new(62.345, 75.5).expect("range");
        assert_eq!(
            excerpt_label(&catalog, 1, range),
            "Excerpt 2: 1:02.345 to 1:15.500"
        );
        let form = excerpts_form(&catalog, &[range], 0);
        assert_eq!(form.title, "Marked excerpts");
        assert_eq!(parse_time("1:02.345"), Some(62.345));
        assert_eq!(atempo_chain(4.0), ["atempo=2", "atempo=2.000000"]);
        let shell = crate::tests::keyboard::shell();
        assert_eq!(export_effect_filter(&shell.application), None);
    }
}
