//! Platform-neutral projection of Settings sections and controls.

use std::path::Path;

use apricot_core::{
    CUSTOMIZABLE_MAIN_MENU, SETTINGS_SECTIONS, SettingId, SettingsSection, TranslationCatalog,
    action::ACTIONS,
    audio::{CUSTOM_EQUALIZER_PRESET_IDS, EQUALIZER_BANDS, FACTORY_EQUALIZER_PRESETS},
    locale::LANGUAGES,
};
use apricot_storage::SettingsDocument;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsChoiceOption {
    pub value: String,
    pub label: String,
}

impl SettingsChoiceOption {
    fn raw(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            label: value.clone(),
            value,
        }
    }

    fn labeled(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsCommand {
    BrowseDownloadFolder,
    BrowseCacheFolder,
    SetDefaultPlayer,
    CheckYoutubeComponentUpdates,
    CheckAppUpdates,
    CheckSubscriptions,
    ChooseCookiesFile,
    OpenYoutubeLoginProfile,
    ExportBrowserCookies,
    ObtainYoutubeApiKey,
    AudiovaultLogin,
    AudiovaultLogout,
    AudiovaultRegister,
    ResetEqualizerPreset,
    AddEqualizerProfile,
    ImportEqualizerProfile,
    ExportEqualizerProfile,
    DeleteEqualizerProfile,
    ResetSection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsValueType {
    String,
    Integer,
    Float,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsControl {
    ReadOnlyText {
        id: &'static str,
        label: String,
        value: String,
    },
    Text {
        setting: SettingId,
        label: String,
        value: String,
        secret: bool,
    },
    Choice {
        setting: SettingId,
        label: String,
        value: String,
        value_type: SettingsValueType,
        options: Vec<SettingsChoiceOption>,
    },
    Checkbox {
        setting: SettingId,
        label: String,
        checked: bool,
    },
    Integer {
        setting: SettingId,
        label: String,
        value: i64,
        minimum: i64,
        maximum: i64,
    },
    IntegerSlider {
        setting: SettingId,
        label: String,
        value: i64,
        minimum: i64,
        maximum: i64,
        unit: &'static str,
    },
    EqualizerDevicePresetChoice {
        device_id: String,
        label: String,
        value: String,
        options: Vec<SettingsChoiceOption>,
    },
    EqualizerPresetName {
        preset_id: String,
        label: String,
        value: String,
    },
    EqualizerBandSlider {
        preset_id: String,
        band_id: &'static str,
        label: String,
        value_db: f64,
        minimum_db: i64,
        maximum_db: i64,
    },
    ShortcutActionList {
        label: String,
        actions: Vec<ShortcutActionItem>,
    },
    ShortcutCapture {
        label: String,
        action_id: String,
        value: String,
    },
    MenuItemCheckbox {
        action_id: &'static str,
        label: String,
        checked: bool,
    },
    Command {
        command: SettingsCommand,
        label: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSectionItem {
    pub section: SettingsSection,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutActionItem {
    pub action_id: String,
    pub label: String,
    pub shortcut: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsScreenModel {
    pub title: String,
    pub section_list_name: String,
    pub sections: Vec<SettingsSectionItem>,
    pub selected_section: SettingsSection,
    pub controls: Vec<SettingsControl>,
}

impl SettingsScreenModel {
    pub fn build(
        catalog: &TranslationCatalog,
        settings: &SettingsDocument,
        settings_file: &Path,
        selected_section: SettingsSection,
    ) -> Self {
        let sections = SETTINGS_SECTIONS
            .iter()
            .map(|definition| SettingsSectionItem {
                section: definition.section,
                label: catalog.text(definition.label_key).to_owned(),
            })
            .collect();
        let mut controls = match selected_section {
            SettingsSection::General => general_controls(catalog, settings, settings_file),
            SettingsSection::MainMenu => main_menu_controls(catalog, settings),
            SettingsSection::Playback => playback_controls(catalog, settings),
            SettingsSection::Equalizer => equalizer_controls(catalog, settings),
            SettingsSection::Downloads => download_controls(catalog, settings),
            SettingsSection::Library => library_controls(catalog, settings),
            SettingsSection::Podcasts => podcast_controls(catalog, settings),
            SettingsSection::Notifications => notification_controls(catalog, settings),
            SettingsSection::Cookies => cookie_controls(catalog, settings),
            SettingsSection::Audiovault => audiovault_controls(catalog, settings),
            SettingsSection::Shortcuts => shortcut_controls(catalog, settings),
        };
        let section_label = SETTINGS_SECTIONS
            .iter()
            .find(|definition| definition.section == selected_section)
            .map_or(selected_section.id(), |definition| {
                catalog.text(definition.label_key)
            });
        controls.push(SettingsControl::Command {
            command: SettingsCommand::ResetSection,
            label: catalog
                .text("reset_settings_for_section")
                .replace("{section}", section_label),
        });
        Self {
            title: catalog.text("settings").to_owned(),
            section_list_name: catalog.text("settings_sections").to_owned(),
            sections,
            selected_section,
            controls,
        }
    }
}

#[allow(clippy::too_many_lines)]
fn general_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    settings_file: &Path,
) -> Vec<SettingsControl> {
    let result_limits = ["0", "10", "20", "50", "100", "150", "200", "250"]
        .into_iter()
        .map(|value| {
            if value == "0" {
                SettingsChoiceOption::labeled(value, catalog.text("dynamic_results"))
            } else {
                SettingsChoiceOption::raw(value)
            }
        })
        .collect();
    let direct_link_options = [
        ("play", "direct_link_enter_play"),
        ("download_audio", "direct_link_enter_audio"),
        ("download_video", "direct_link_enter_video"),
        ("copy_stream_url", "direct_link_enter_stream"),
    ]
    .into_iter()
    .map(|(value, key)| SettingsChoiceOption::labeled(value, catalog.text(key)))
    .collect();
    let update_intervals = ["0.5", "1", "2", "3", "6", "12", "24"]
        .into_iter()
        .map(|value| {
            let label = match value {
                "0.5" => catalog.text("interval_30_minutes").to_owned(),
                "1" => catalog.text("interval_1_hour").to_owned(),
                hours => catalog.text("interval_hours").replace("{hours}", hours),
            };
            SettingsChoiceOption::labeled(value, label)
        })
        .collect();

    vec![
        SettingsControl::Choice {
            setting: SettingId::Language,
            label: catalog.text("language").to_owned(),
            value: settings.language.clone(),
            value_type: SettingsValueType::String,
            options: LANGUAGES
                .iter()
                .map(|language| SettingsChoiceOption::labeled(language.code, language.name))
                .collect(),
        },
        SettingsControl::ReadOnlyText {
            id: "settings_file",
            label: catalog.text("settings_file").to_owned(),
            value: settings_file.display().to_string(),
        },
        SettingsControl::Text {
            setting: SettingId::DownloadFolder,
            label: catalog.text("download_folder").to_owned(),
            value: settings.download_folder.clone(),
            secret: false,
        },
        SettingsControl::Command {
            command: SettingsCommand::BrowseDownloadFolder,
            label: catalog.text("browse").to_owned(),
        },
        SettingsControl::Command {
            command: SettingsCommand::SetDefaultPlayer,
            label: catalog.text("set_default_player").to_owned(),
        },
        SettingsControl::Choice {
            setting: SettingId::ResultsLimit,
            label: catalog.text("results_limit").to_owned(),
            value: settings.results_limit.min(250).to_string(),
            value_type: SettingsValueType::Integer,
            options: result_limits,
        },
        SettingsControl::Choice {
            setting: SettingId::DirectLinkEnterAction,
            label: catalog.text("direct_link_enter_action").to_owned(),
            value: settings.direct_link_enter_action.clone(),
            value_type: SettingsValueType::String,
            options: direct_link_options,
        },
        checkbox(
            SettingId::ShowShortcutsInLabels,
            "show_shortcuts_in_labels",
            settings.show_shortcuts_in_labels,
            catalog,
        ),
        SettingsControl::Choice {
            setting: SettingId::YoutubeBackend,
            label: catalog.text("youtube_backend").to_owned(),
            value: settings.youtube_backend.clone(),
            value_type: SettingsValueType::String,
            options: [
                SettingsChoiceOption::labeled("yt-dlp", catalog.text("youtube_backend_ytdlp")),
                SettingsChoiceOption::labeled(
                    "rusty_ytdl",
                    catalog.text("youtube_backend_rusty_ytdl"),
                ),
            ]
            .into(),
        },
        checkbox(
            SettingId::AutoUpdateYtdlp,
            "auto_update_youtube_components",
            settings.auto_update_ytdlp,
            catalog,
        ),
        checkbox(
            SettingId::AutoUpdateApp,
            "auto_update_app",
            settings.auto_update_app,
            catalog,
        ),
        SettingsControl::Choice {
            setting: SettingId::UpdateChannel,
            label: catalog.text("update_channel").to_owned(),
            value: settings.update_channel.clone(),
            value_type: SettingsValueType::String,
            options: [
                SettingsChoiceOption::labeled("stable", catalog.text("update_channel_stable")),
                SettingsChoiceOption::labeled("beta", catalog.text("update_channel_beta")),
            ]
            .into(),
        },
        SettingsControl::Choice {
            setting: SettingId::AppUpdateIntervalHours,
            label: catalog.text("app_update_interval").to_owned(),
            value: compact_number(settings.app_update_interval_hours),
            value_type: SettingsValueType::Float,
            options: update_intervals,
        },
        SettingsControl::Command {
            command: SettingsCommand::CheckYoutubeComponentUpdates,
            label: catalog
                .text("check_youtube_component_updates_now")
                .to_owned(),
        },
        SettingsControl::Command {
            command: SettingsCommand::CheckAppUpdates,
            label: catalog.text("check_app_updates_now").to_owned(),
        },
        checkbox(
            SettingId::CloseToTray,
            "close_to_tray",
            settings.close_to_tray,
            catalog,
        ),
        checkbox(
            SettingId::StartWithWindows,
            "start_with_windows",
            settings.start_with_windows,
            catalog,
        ),
        checkbox(
            SettingId::TrayNotification,
            "tray_notification",
            settings.tray_notification,
            catalog,
        ),
    ]
}

fn main_menu_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    CUSTOMIZABLE_MAIN_MENU
        .iter()
        .map(|definition| SettingsControl::MenuItemCheckbox {
            action_id: definition.action_id,
            label: catalog.text(definition.label_key).to_owned(),
            checked: !settings
                .main_menu_hidden_actions
                .iter()
                .any(|hidden| hidden == definition.action_id),
        })
        .collect()
}

#[allow(clippy::too_many_lines)]
fn playback_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    let playback_rates = [
        "0.25", "0.5", "0.6", "0.7", "0.75", "0.8", "0.9", "1.0", "1.1", "1.2", "1.25", "1.3",
        "1.4", "1.5", "1.75", "2.0",
    ];
    let audio_modes = [
        ("Rubberband high quality", "speed_audio_mode_rubberband"),
        ("High quality scaletempo2", "speed_audio_mode_scaletempo2"),
        ("mpv default scaletempo2", "speed_audio_mode_mpv"),
        ("Classic scaletempo", "speed_audio_mode_scaletempo"),
    ];
    let pitch_modes = [
        (
            "Independent pitch - highest quality (mpv built-in)",
            "pitch_mode_mpv",
        ),
        (
            "Independent pitch - advanced (Rubberband)",
            "pitch_mode_rubberband",
        ),
        (
            "Linked pitch and speed - pitch keys change both",
            "pitch_mode_linked_speed",
        ),
    ];
    let stream_cache_options = ["5", "10", "20", "30", "60", "240", "1440", "10080", "0"]
        .into_iter()
        .map(|value| {
            let minutes = value.parse::<i64>().expect("constant minutes");
            let label = if minutes == 0 {
                catalog.text("stream_cache_permanent").to_owned()
            } else if minutes < 60 {
                catalog
                    .text("stream_cache_minutes_label")
                    .replace("{minutes}", value)
            } else if minutes % 1_440 == 0 {
                catalog
                    .text("stream_cache_days_label")
                    .replace("{days}", &(minutes / 1_440).to_string())
            } else {
                catalog
                    .text("stream_cache_hours_label")
                    .replace("{hours}", &(minutes / 60).to_string())
            };
            SettingsChoiceOption::labeled(value, label)
        })
        .collect();
    let mut controls = vec![
        choice_raw(
            SettingId::PlayerSpeed,
            "player_speed",
            &settings.player_speed,
            &playback_rates,
            SettingsValueType::String,
            catalog,
        ),
        choice_labeled(
            SettingId::SpeedAudioMode,
            "speed_audio_mode",
            &settings.speed_audio_mode,
            &audio_modes,
            catalog,
        ),
        choice_labeled(
            SettingId::PitchMode,
            "pitch_mode",
            &settings.pitch_mode,
            &pitch_modes,
            catalog,
        ),
        choice_raw(
            SettingId::SpeedStep,
            "speed_step",
            &compact_number(settings.speed_step),
            &["0.01", "0.02", "0.05", "0.10", "0.25"],
            SettingsValueType::Float,
            catalog,
        ),
        choice_raw(
            SettingId::PitchStep,
            "pitch_step",
            &compact_number(settings.pitch_step),
            &["0.01", "0.02", "0.05", "0.10", "0.25"],
            SettingsValueType::Float,
            catalog,
        ),
        integer(
            SettingId::SpeedPitchHoldDelayMs,
            "speed_pitch_hold_delay_ms",
            settings.speed_pitch_hold_delay_ms,
            50,
            1_000,
            catalog,
        ),
        integer(
            SettingId::SpeedPitchHoldIntervalMs,
            "speed_pitch_hold_interval_ms",
            settings.speed_pitch_hold_interval_ms,
            20,
            500,
            catalog,
        ),
        checkbox(
            SettingId::ShowVideoDetailsByDefault,
            "show_video_details_by_default",
            settings.show_video_details_by_default,
            catalog,
        ),
        checkbox(
            SettingId::EnableAgeRestrictedVideos,
            "enable_age_restricted_videos",
            settings.enable_age_restricted_videos,
            catalog,
        ),
        checkbox(
            SettingId::EnableStreamCache,
            "enable_stream_cache",
            settings.enable_stream_cache,
            catalog,
        ),
        checkbox(
            SettingId::EnableStreamUrlCache,
            "enable_stream_url_cache",
            settings.enable_stream_url_cache,
            catalog,
        ),
        SettingsControl::Choice {
            setting: SettingId::StreamUrlCacheMinutes,
            label: catalog.text("stream_url_cache_minutes").to_owned(),
            value: settings.stream_url_cache_minutes.to_string(),
            value_type: SettingsValueType::Integer,
            options: stream_cache_options,
        },
        choice_labeled(
            SettingId::StreamFormatPreference,
            "stream_format_preference",
            &settings.stream_format_preference,
            &[
                ("auto", "stream_format_preference_auto"),
                ("video", "stream_format_preference_video"),
                ("audio", "stream_format_preference_audio"),
            ],
            catalog,
        ),
        checkbox(
            SettingId::PrefetchNextStreamUrl,
            "prefetch_next_stream_url",
            settings.prefetch_next_stream_url,
            catalog,
        ),
        checkbox(
            SettingId::GaplessPlayback,
            "gapless_playback",
            settings.gapless_playback,
            catalog,
        ),
        choice_labeled(
            SettingId::ReplaygainMode,
            "replaygain_mode",
            &settings.replaygain_mode,
            &[
                ("no", "replaygain_off"),
                ("track", "replaygain_track"),
                ("album", "replaygain_album"),
            ],
            catalog,
        ),
        checkbox(
            SettingId::EnableOnlineLyrics,
            "enable_online_lyrics",
            settings.enable_online_lyrics,
            catalog,
        ),
        text(
            SettingId::CacheFolder,
            "cache_folder",
            &settings.cache_folder,
            false,
            catalog,
        ),
        choice_raw(
            SettingId::CacheSizeMb,
            "cache_size_mb",
            &settings.cache_size_mb.to_string(),
            &["128", "256", "512", "1024", "2048", "4096"],
            SettingsValueType::Integer,
            catalog,
        ),
        checkbox(
            SettingId::ResumePlayback,
            "resume_playback",
            settings.resume_playback,
            catalog,
        ),
        checkbox(
            SettingId::ShowResumeInMenu,
            "show_resume_in_menu",
            settings.show_resume_in_menu,
            catalog,
        ),
        choice_raw(
            SettingId::AudioOutputDevice,
            "default_audio_device",
            &settings.audio_output_device,
            &["auto"],
            SettingsValueType::String,
            catalog,
        ),
        choice_raw(
            SettingId::SeekSeconds,
            "seek_seconds",
            &compact_number(settings.seek_seconds),
            &[
                "0.1", "0.25", "0.5", "0.75", "1", "1.5", "2", "2.5", "3", "4", "5", "7.5", "10",
                "15", "20", "30", "45", "60",
            ],
            SettingsValueType::Float,
            catalog,
        ),
        choice_raw(
            SettingId::VolumeStep,
            "volume_step",
            &settings.volume_step.to_string(),
            &["1", "2", "5", "10"],
            SettingsValueType::Integer,
            catalog,
        ),
        SettingsControl::IntegerSlider {
            setting: SettingId::DefaultVolume,
            label: catalog.text("default_volume").to_owned(),
            value: settings.default_volume,
            minimum: 0,
            maximum: if settings.volume_boost_by_default {
                300
            } else {
                100
            },
            unit: "percent",
        },
        checkbox(
            SettingId::VolumeBoostByDefault,
            "volume_boost_by_default",
            settings.volume_boost_by_default,
            catalog,
        ),
        checkbox(
            SettingId::AutoplayNext,
            "autoplay_next",
            settings.autoplay_next,
            catalog,
        ),
        checkbox(
            SettingId::AutoplayRelated,
            "autoplay_related",
            settings.autoplay_related,
            catalog,
        ),
        checkbox(
            SettingId::PreferBrowserPlayback,
            "browser_playback",
            settings.prefer_browser_playback,
            catalog,
        ),
        checkbox(
            SettingId::PlayerFullscreen,
            "fullscreen",
            settings.player_fullscreen,
            catalog,
        ),
        checkbox(
            SettingId::PlayerStartPaused,
            "start_paused",
            settings.player_start_paused,
            catalog,
        ),
        checkbox(
            SettingId::AnnouncePlayPause,
            "announce_play_pause",
            settings.announce_play_pause,
            catalog,
        ),
        checkbox(
            SettingId::AnnouncePlaybackFinished,
            "announce_playback_finished",
            settings.announce_playback_finished,
            catalog,
        ),
        checkbox(
            SettingId::EnableBackgroundPlayback,
            "enable_background_playback",
            settings.enable_background_playback,
            catalog,
        ),
    ];
    controls.insert(
        18,
        SettingsControl::Command {
            command: SettingsCommand::BrowseCacheFolder,
            label: catalog.text("browse").to_owned(),
        },
    );
    controls
}

fn equalizer_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    let mut controls = vec![
        checkbox(
            SettingId::GlobalEqualizerEnabled,
            "global_equalizer",
            settings.global_equalizer_enabled,
            catalog,
        ),
        checkbox(
            SettingId::EqualizerClippingProtection,
            "equalizer_clipping_protection",
            settings.equalizer_clipping_protection,
            catalog,
        ),
    ];
    if !settings.global_equalizer_enabled {
        return controls;
    }

    controls.extend(enabled_equalizer_controls(catalog, settings));
    controls
}

fn enabled_equalizer_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    let mut controls = Vec::new();
    let preset = settings.global_equalizer_preset.clone();
    let preset_options = equalizer_preset_options(catalog, settings);
    controls.push(SettingsControl::Choice {
        setting: SettingId::GlobalEqualizerPreset,
        label: catalog.text("equalizer_preset").to_owned(),
        value: preset.clone(),
        value_type: SettingsValueType::String,
        options: preset_options.clone(),
    });
    let device_id = if settings.audio_output_device.trim().is_empty() {
        "auto".to_owned()
    } else {
        settings.audio_output_device.clone()
    };
    let device_value = settings
        .equalizer_device_presets
        .get(&device_id)
        .cloned()
        .unwrap_or_default();
    let mut device_options = vec![SettingsChoiceOption::labeled(
        "",
        catalog.text("equalizer_use_global_preset"),
    )];
    device_options.extend(preset_options);
    controls.push(SettingsControl::EqualizerDevicePresetChoice {
        device_id,
        label: catalog.text("equalizer_device_preset").to_owned(),
        value: device_value,
        options: device_options,
    });
    if !FACTORY_EQUALIZER_PRESETS
        .iter()
        .any(|factory| factory.id == preset)
    {
        controls.push(SettingsControl::EqualizerPresetName {
            preset_id: preset.clone(),
            label: catalog.text("equalizer_preset_name").to_owned(),
            value: settings
                .equalizer_custom_names
                .get(&preset)
                .cloned()
                .unwrap_or_else(|| preset.clone()),
        });
    }
    controls.push(choice_raw(
        SettingId::EqualizerDbRange,
        "equalizer_db_range",
        &settings.equalizer_db_range.to_string(),
        &["6", "12", "18", "24"],
        SettingsValueType::Integer,
        catalog,
    ));
    let gains = settings
        .equalizer_preset_gains
        .get(&preset)
        .unwrap_or(&settings.global_equalizer_gains);
    for band in EQUALIZER_BANDS {
        let frequency = band.frequency_hz.to_string();
        let label = catalog
            .text("equalizer_band_gain")
            .replace("{band}", &format!("{frequency} Hz"));
        controls.push(SettingsControl::EqualizerBandSlider {
            preset_id: preset.clone(),
            band_id: band.id,
            label,
            value_db: gains.get(band.id).copied().unwrap_or(0.0),
            minimum_db: -settings.equalizer_db_range,
            maximum_db: settings.equalizer_db_range,
        });
    }
    controls.extend([
        SettingsControl::Command {
            command: SettingsCommand::ResetEqualizerPreset,
            label: catalog.text("reset_equalizer").to_owned(),
        },
        SettingsControl::Command {
            command: SettingsCommand::AddEqualizerProfile,
            label: catalog.text("add_equalizer_profile").to_owned(),
        },
        SettingsControl::Command {
            command: SettingsCommand::ImportEqualizerProfile,
            label: catalog.text("import_equalizer_profile").to_owned(),
        },
        SettingsControl::Command {
            command: SettingsCommand::ExportEqualizerProfile,
            label: catalog.text("export_equalizer_profile").to_owned(),
        },
    ]);
    if !FACTORY_EQUALIZER_PRESETS
        .iter()
        .any(|factory| factory.id == preset)
    {
        controls.push(SettingsControl::Command {
            command: SettingsCommand::DeleteEqualizerProfile,
            label: catalog.text("delete_equalizer_profile").to_owned(),
        });
    }
    controls
}

fn equalizer_preset_options(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsChoiceOption> {
    let mut options: Vec<_> = FACTORY_EQUALIZER_PRESETS
        .iter()
        .map(|preset| {
            SettingsChoiceOption::labeled(
                preset.id,
                catalog.text(&format!("eq_preset_{}", preset.id)),
            )
        })
        .collect();
    let mut custom_ids: Vec<_> = settings
        .equalizer_preset_gains
        .keys()
        .filter(|id| {
            !FACTORY_EQUALIZER_PRESETS
                .iter()
                .any(|factory| factory.id == id.as_str())
        })
        .cloned()
        .collect();
    for id in CUSTOM_EQUALIZER_PRESET_IDS {
        if !custom_ids.iter().any(|existing| existing == id) {
            custom_ids.push((*id).to_owned());
        }
    }
    custom_ids.sort_by_key(|id| {
        CUSTOM_EQUALIZER_PRESET_IDS
            .iter()
            .position(|default| *default == id)
            .map_or((1, usize::MAX, id.clone()), |index| {
                (0, index, String::new())
            })
    });
    options.extend(custom_ids.into_iter().map(|id| {
        let label = settings
            .equalizer_custom_names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| id.clone());
        SettingsChoiceOption::labeled(id, label)
    }));
    options
}

#[allow(clippy::too_many_lines)]
fn download_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    let audio_quality_options = [
        "0", "320", "256", "192", "160", "128", "96", "64", "1", "2", "3", "4", "5", "6", "7", "8",
        "9", "10",
    ]
    .into_iter()
    .map(|value| {
        let label = if value == "0" {
            "Best variable quality (VBR 0)".to_owned()
        } else if value.parse::<i64>().is_ok_and(|number| number <= 10) {
            format!("Variable quality (VBR {value})")
        } else {
            format!("{value} kbps")
        };
        SettingsChoiceOption::labeled(value, label)
    })
    .collect();
    vec![
        checkbox(
            SettingId::ConfirmBeforeDownload,
            "confirm_download",
            settings.confirm_before_download,
            catalog,
        ),
        checkbox(
            SettingId::OpenFolderAfterDownload,
            "open_after_download",
            settings.open_folder_after_download,
            catalog,
        ),
        checkbox(
            SettingId::PopupWhenDownloadComplete,
            "download_complete_popup",
            settings.popup_when_download_complete,
            catalog,
        ),
        checkbox(
            SettingId::PopupWhenConversionComplete,
            "conversion_complete_popup",
            settings.popup_when_conversion_complete,
            catalog,
        ),
        checkbox(
            SettingId::AskDownloadLocationEachTime,
            "ask_download_location_each_time",
            settings.ask_download_location_each_time,
            catalog,
        ),
        choice_raw(
            SettingId::AudioFormat,
            "audio_format",
            &settings.audio_format,
            &["mp3", "m4a", "opus", "wav", "flac"],
            SettingsValueType::String,
            catalog,
        ),
        SettingsControl::Choice {
            setting: SettingId::AudioQuality,
            label: catalog.text("audio_quality").to_owned(),
            value: settings.audio_quality.clone(),
            value_type: SettingsValueType::String,
            options: audio_quality_options,
        },
        choice_labeled(
            SettingId::VideoFormat,
            "video_format",
            &settings.video_format,
            &[
                ("mp4", "video_format_mp4_recommended"),
                ("best-any", "video_format_best_available"),
                ("mp4-single", "video_format_mp4_single"),
                ("smallest", "video_format_smallest"),
            ],
            catalog,
        ),
        choice_raw(
            SettingId::MaxVideoHeight,
            "max_height",
            &settings.max_video_height.to_string(),
            &["0", "360", "480", "720", "1080", "1440", "2160"],
            SettingsValueType::Integer,
            catalog,
        ),
        text(
            SettingId::FilenameTemplate,
            "filename_template",
            &settings.filename_template,
            false,
            catalog,
        ),
        text(
            SettingId::SubtitleLanguages,
            "subtitle_langs",
            &settings.subtitle_languages,
            false,
            catalog,
        ),
        checkbox(
            SettingId::QuietDownloads,
            "quiet_downloads",
            settings.quiet_downloads,
            catalog,
        ),
        checkbox(
            SettingId::KeepPlaylistOrder,
            "playlist_order",
            settings.keep_playlist_order,
            catalog,
        ),
        checkbox(
            SettingId::WriteThumbnail,
            "write_thumbnail",
            settings.write_thumbnail,
            catalog,
        ),
        checkbox(
            SettingId::WriteDescription,
            "write_description",
            settings.write_description,
            catalog,
        ),
        checkbox(
            SettingId::WriteInfoJson,
            "write_info_json",
            settings.write_info_json,
            catalog,
        ),
        checkbox(
            SettingId::WriteSubtitles,
            "write_subtitles",
            settings.write_subtitles,
            catalog,
        ),
        checkbox(
            SettingId::AutoSubtitles,
            "auto_subtitles",
            settings.auto_subtitles,
            catalog,
        ),
        checkbox(
            SettingId::EmbedMetadata,
            "embed_metadata",
            settings.embed_metadata,
            catalog,
        ),
        checkbox(
            SettingId::EmbedThumbnail,
            "embed_thumbnail",
            settings.embed_thumbnail,
            catalog,
        ),
        checkbox(
            SettingId::RestrictFilenames,
            "restrict_filenames",
            settings.restrict_filenames,
            catalog,
        ),
        checkbox(
            SettingId::DownloadArchive,
            "download_archive",
            settings.download_archive,
            catalog,
        ),
    ]
}

fn library_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    vec![
        checkbox(
            SettingId::EnableTrending,
            "enable_trending",
            settings.enable_trending,
            catalog,
        ),
        checkbox(
            SettingId::EnableHistory,
            "enable_history",
            settings.enable_history,
            catalog,
        ),
        choice_raw(
            SettingId::HistoryLimit,
            "history_limit",
            &settings.history_limit.to_string(),
            &["100", "250", "500", "1000", "2000"],
            SettingsValueType::Integer,
            catalog,
        ),
        checkbox(
            SettingId::SubscriptionCheckEnabled,
            "subscription_check_enabled",
            settings.subscription_check_enabled,
            catalog,
        ),
        interval_choice(
            SettingId::SubscriptionCheckIntervalHours,
            "subscription_check_interval",
            settings.subscription_check_interval_hours,
            catalog,
        ),
        SettingsControl::Command {
            command: SettingsCommand::CheckSubscriptions,
            label: catalog.text("subscription_check_now").to_owned(),
        },
    ]
}

fn podcast_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    vec![
        checkbox(
            SettingId::EnablePodcastsRss,
            "enable_podcasts_rss",
            settings.enable_podcasts_rss,
            catalog,
        ),
        SettingsControl::ReadOnlyText {
            id: "podcast_source",
            label: catalog.text("podcast_source").to_owned(),
            value: catalog.text("podcast_source_info").to_owned(),
        },
        choice_labeled(
            SettingId::PodcastSearchProvider,
            "podcast_search_provider",
            &settings.podcast_search_provider,
            &[("apple", "podcast_search_provider_apple")],
            catalog,
        ),
        choice_raw(
            SettingId::PodcastSearchCountry,
            "podcast_search_country",
            &settings.podcast_search_country,
            PODCAST_COUNTRIES,
            SettingsValueType::String,
            catalog,
        ),
        choice_raw(
            SettingId::PodcastSearchLimit,
            "podcast_search_limit",
            &settings.podcast_search_limit.to_string(),
            &["10", "20", "50", "100", "150", "200"],
            SettingsValueType::Integer,
            catalog,
        ),
        choice_raw(
            SettingId::RssMaxItems,
            "rss_episode_batch_size",
            &settings.rss_max_items.to_string(),
            &["25", "50", "100", "200", "500"],
            SettingsValueType::Integer,
            catalog,
        ),
        checkbox(
            SettingId::RssRefreshOnStartup,
            "rss_refresh_on_startup",
            settings.rss_refresh_on_startup,
            catalog,
        ),
        checkbox(
            SettingId::RssAutoRefreshEnabled,
            "rss_auto_refresh_enabled",
            settings.rss_auto_refresh_enabled,
            catalog,
        ),
        interval_choice(
            SettingId::RssRefreshIntervalHours,
            "rss_refresh_interval",
            settings.rss_refresh_interval_hours,
            catalog,
        ),
    ]
}

fn notification_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    vec![
        checkbox(
            SettingId::WindowsNotifications,
            "windows_notifications",
            settings.windows_notifications,
            catalog,
        ),
        checkbox(
            SettingId::DownloadNotifications,
            "download_notifications",
            settings.download_notifications,
            catalog,
        ),
        checkbox(
            SettingId::SubscriptionNotifications,
            "subscription_notifications",
            settings.subscription_notifications,
            catalog,
        ),
        checkbox(
            SettingId::AppUpdateNotifications,
            "app_update_notifications",
            settings.app_update_notifications,
            catalog,
        ),
    ]
}

#[allow(clippy::too_many_lines)]
fn cookie_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    let mut controls = vec![
        text(
            SettingId::CookiesFile,
            "cookies",
            &settings.cookies_file,
            false,
            catalog,
        ),
        SettingsControl::Command {
            command: SettingsCommand::ChooseCookiesFile,
            label: catalog.text("choose_cookies_file").to_owned(),
        },
        choice_raw(
            SettingId::CookiesFromBrowser,
            "cookies_from_browser",
            &settings.cookies_from_browser,
            &[
                "none", "chrome", "edge", "firefox", "brave", "chromium", "opera", "vivaldi",
            ],
            SettingsValueType::String,
            catalog,
        ),
        choice_raw(
            SettingId::CookiesBrowserProfile,
            "cookies_browser_profile",
            &settings.cookies_browser_profile,
            &["auto"],
            SettingsValueType::String,
            catalog,
        ),
        SettingsControl::Command {
            command: SettingsCommand::OpenYoutubeLoginProfile,
            label: catalog.text("open_youtube_login_profile").to_owned(),
        },
        SettingsControl::Command {
            command: SettingsCommand::ExportBrowserCookies,
            label: catalog.text("export_browser_cookies").to_owned(),
        },
        text(SettingId::Proxy, "proxy", &settings.proxy, false, catalog),
        text(
            SettingId::YoutubeDataApiKey,
            "youtube_data_api_key",
            &settings.youtube_data_api_key,
            true,
            catalog,
        ),
        SettingsControl::Command {
            command: SettingsCommand::ObtainYoutubeApiKey,
            label: catalog.text("obtain_youtube_api_key").to_owned(),
        },
        checkbox(
            SettingId::ShowAdvancedNetworkSettings,
            "show_advanced_network_settings",
            settings.show_advanced_network_settings,
            catalog,
        ),
    ];
    if settings.show_advanced_network_settings {
        controls.extend([
            text(
                SettingId::CookieUserAgent,
                "cookie_user_agent",
                &settings.cookie_user_agent,
                false,
                catalog,
            ),
            text(
                SettingId::RateLimit,
                "rate_limit",
                &settings.rate_limit,
                false,
                catalog,
            ),
            text(
                SettingId::FfmpegLocation,
                "ffmpeg",
                &settings.ffmpeg_location,
                false,
                catalog,
            ),
            choice_raw(
                SettingId::ConcurrentFragments,
                "fragments",
                &settings.concurrent_fragments.to_string(),
                &["1", "2", "4", "8", "16"],
                SettingsValueType::Integer,
                catalog,
            ),
        ]);
    }
    controls.extend([
        choice_raw(
            SettingId::Retries,
            "retries",
            &settings.retries.to_string(),
            &["0", "3", "5", "10", "20"],
            SettingsValueType::Integer,
            catalog,
        ),
        choice_raw(
            SettingId::SocketTimeout,
            "timeout",
            &settings.socket_timeout.to_string(),
            &["5", "10", "20", "30", "60"],
            SettingsValueType::Integer,
            catalog,
        ),
    ]);
    controls
}

fn audiovault_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    vec![
        text(
            SettingId::AudiovaultEmail,
            "audiovault_email",
            &settings.audiovault_email,
            false,
            catalog,
        ),
        SettingsControl::Command {
            command: SettingsCommand::AudiovaultLogin,
            label: catalog.text("audiovault_login").to_owned(),
        },
        SettingsControl::Command {
            command: SettingsCommand::AudiovaultLogout,
            label: catalog.text("audiovault_logout").to_owned(),
        },
        SettingsControl::Command {
            command: SettingsCommand::AudiovaultRegister,
            label: catalog.text("register").to_owned(),
        },
    ]
}

fn shortcut_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    let actions: Vec<_> = ACTIONS
        .iter()
        .map(|action| {
            let action_id = action.id.as_str();
            let shortcut = settings
                .keyboard_shortcuts
                .get(action_id)
                .cloned()
                .unwrap_or_else(|| action.default_windows_shortcut.to_owned());
            ShortcutActionItem {
                action_id: action_id.to_owned(),
                label: catalog.text(action.label_key).to_owned(),
                shortcut,
            }
        })
        .collect();
    let first = actions.first().cloned();
    let mut controls = vec![SettingsControl::ShortcutActionList {
        label: catalog.text("shortcut_actions").to_owned(),
        actions,
    }];
    if let Some(first) = first {
        controls.push(SettingsControl::ShortcutCapture {
            label: format!(
                "{}. {}",
                catalog.text("shortcut_value"),
                catalog.text("shortcut_capture_hint")
            ),
            action_id: first.action_id,
            value: first.shortcut,
        });
    }
    controls
}

fn text(
    setting: SettingId,
    label_key: &str,
    value: &str,
    secret: bool,
    catalog: &TranslationCatalog,
) -> SettingsControl {
    SettingsControl::Text {
        setting,
        label: catalog.text(label_key).to_owned(),
        value: value.to_owned(),
        secret,
    }
}

fn integer(
    setting: SettingId,
    label_key: &str,
    value: i64,
    minimum: i64,
    maximum: i64,
    catalog: &TranslationCatalog,
) -> SettingsControl {
    SettingsControl::Integer {
        setting,
        label: catalog.text(label_key).to_owned(),
        value,
        minimum,
        maximum,
    }
}

fn choice_raw(
    setting: SettingId,
    label_key: &str,
    value: &str,
    options: &[&str],
    value_type: SettingsValueType,
    catalog: &TranslationCatalog,
) -> SettingsControl {
    SettingsControl::Choice {
        setting,
        label: catalog.text(label_key).to_owned(),
        value: value.to_owned(),
        value_type,
        options: options
            .iter()
            .map(|option| SettingsChoiceOption::raw(*option))
            .collect(),
    }
}

fn choice_labeled(
    setting: SettingId,
    label_key: &str,
    value: &str,
    options: &[(&str, &str)],
    catalog: &TranslationCatalog,
) -> SettingsControl {
    SettingsControl::Choice {
        setting,
        label: catalog.text(label_key).to_owned(),
        value: value.to_owned(),
        value_type: SettingsValueType::String,
        options: options
            .iter()
            .map(|(option, key)| SettingsChoiceOption::labeled(*option, catalog.text(key)))
            .collect(),
    }
}

fn interval_choice(
    setting: SettingId,
    label_key: &str,
    value: f64,
    catalog: &TranslationCatalog,
) -> SettingsControl {
    let options = ["0.5", "1", "2", "3", "6", "12", "24"]
        .into_iter()
        .map(|option| {
            let label = match option {
                "0.5" => catalog.text("interval_30_minutes").to_owned(),
                "1" => catalog.text("interval_1_hour").to_owned(),
                hours => catalog.text("interval_hours").replace("{hours}", hours),
            };
            SettingsChoiceOption::labeled(option, label)
        })
        .collect();
    SettingsControl::Choice {
        setting,
        label: catalog.text(label_key).to_owned(),
        value: compact_number(value),
        value_type: SettingsValueType::Float,
        options,
    }
}

const PODCAST_COUNTRIES: &[&str] = &[
    "US", "SI", "GB", "DE", "FR", "ES", "IT", "AT", "HR", "RS", "CA", "AU", "NL", "SE", "PL", "AR",
    "BE", "BR", "CH", "CL", "CO", "CZ", "DK", "EG", "FI", "GR", "HK", "HU", "ID", "IE", "IL", "IN",
    "JP", "KR", "MX", "NO", "NZ", "PH", "PT", "RO", "RU", "SG", "SK", "TH", "TR", "TW", "UA", "VN",
    "ZA",
];

fn checkbox(
    setting: SettingId,
    label_key: &str,
    checked: bool,
    catalog: &TranslationCatalog,
) -> SettingsControl {
    SettingsControl::Checkbox {
        setting,
        label: catalog.text(label_key).to_owned(),
        checked,
    }
}

fn compact_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use apricot_core::{CUSTOMIZABLE_MAIN_MENU, SettingId, SettingsSection};
    use apricot_storage::SettingsDocument;

    use crate::english_catalog;

    use super::{SettingsControl, SettingsScreenModel};

    #[test]
    fn settings_shell_has_all_sections_in_canonical_order() {
        let model = SettingsScreenModel::build(
            &english_catalog(),
            &SettingsDocument::default(),
            Path::new(r"C:\Profile\settings.json"),
            SettingsSection::General,
        );
        assert_eq!(model.sections.len(), 11);
        assert_eq!(model.sections[0].section, SettingsSection::General);
        assert_eq!(model.sections[1].section, SettingsSection::MainMenu);
        assert_eq!(model.selected_section, SettingsSection::General);
        assert_eq!(model.section_list_name, "Settings sections");
    }

    #[test]
    fn general_controls_preserve_python_order_and_current_values() {
        let settings = SettingsDocument {
            language: "sl".to_owned(),
            results_limit: 50,
            ..SettingsDocument::default()
        };
        let model = SettingsScreenModel::build(
            &english_catalog(),
            &settings,
            Path::new(r"C:\Profile\settings.json"),
            SettingsSection::General,
        );
        assert_eq!(model.controls.len(), 19);
        assert!(matches!(
            &model.controls[0],
            SettingsControl::Choice {
                setting: SettingId::Language,
                value,
                options,
                ..
            } if value == "sl" && options.len() == 27
        ));
        assert!(matches!(
            &model.controls[5],
            SettingsControl::Choice {
                setting: SettingId::ResultsLimit,
                value,
                ..
            } if value == "50"
        ));
        assert!(matches!(
            &model.controls[8],
            SettingsControl::Choice {
                setting: SettingId::YoutubeBackend,
                value,
                options,
                ..
            } if value == "yt-dlp" && options.len() == 2
        ));
    }

    #[test]
    fn main_menu_customization_is_nineteen_real_checkboxes() {
        let settings = SettingsDocument {
            main_menu_hidden_actions: vec!["search".to_owned()],
            ..SettingsDocument::default()
        };
        let model = SettingsScreenModel::build(
            &english_catalog(),
            &settings,
            Path::new("settings.json"),
            SettingsSection::MainMenu,
        );
        assert_eq!(model.controls.len(), CUSTOMIZABLE_MAIN_MENU.len() + 1);
        assert!(matches!(
            &model.controls[0],
            SettingsControl::MenuItemCheckbox {
                action_id: "current_downloads",
                checked: true,
                ..
            }
        ));
        assert!(model.controls.iter().any(|control| matches!(
            control,
            SettingsControl::MenuItemCheckbox {
                action_id: "search",
                checked: false,
                ..
            }
        )));
        assert!(model.controls.iter().all(|control| !matches!(
            control,
            SettingsControl::MenuItemCheckbox {
                action_id: "settings" | "exit",
                ..
            }
        )));
    }

    #[test]
    fn implemented_sections_preserve_complete_control_counts() {
        let settings = SettingsDocument::default();
        let expected = [
            (SettingsSection::General, 19),
            (SettingsSection::MainMenu, 20),
            (SettingsSection::Playback, 36),
            (SettingsSection::Equalizer, 3),
            (SettingsSection::Downloads, 23),
            (SettingsSection::Library, 7),
            (SettingsSection::Podcasts, 10),
            (SettingsSection::Notifications, 5),
            (SettingsSection::Cookies, 13),
            (SettingsSection::Audiovault, 5),
            (SettingsSection::Shortcuts, 3),
        ];
        for (section, count) in expected {
            let model = SettingsScreenModel::build(
                &english_catalog(),
                &settings,
                Path::new("settings.json"),
                section,
            );
            assert_eq!(model.controls.len(), count, "{}", section.id());
        }
    }

    #[test]
    fn advanced_network_controls_are_conditional_without_losing_values() {
        let settings = SettingsDocument {
            show_advanced_network_settings: true,
            concurrent_fragments: 8,
            ..SettingsDocument::default()
        };
        let model = SettingsScreenModel::build(
            &english_catalog(),
            &settings,
            Path::new("settings.json"),
            SettingsSection::Cookies,
        );
        assert_eq!(model.controls.len(), 17);
        assert!(model.controls.iter().any(|control| matches!(
            control,
            SettingsControl::Choice {
                setting: SettingId::ConcurrentFragments,
                value,
                ..
            } if value == "8"
        )));
    }

    #[test]
    fn enabled_equalizer_exposes_ten_independent_band_sliders() {
        let settings = SettingsDocument {
            global_equalizer_enabled: true,
            ..SettingsDocument::default()
        };
        let model = SettingsScreenModel::build(
            &english_catalog(),
            &settings,
            Path::new("settings.json"),
            SettingsSection::Equalizer,
        );
        assert_eq!(model.controls.len(), 20);
        let bands: Vec<_> = model
            .controls
            .iter()
            .filter_map(|control| match control {
                SettingsControl::EqualizerBandSlider { band_id, .. } => Some(*band_id),
                _ => None,
            })
            .collect();
        assert_eq!(
            bands,
            [
                "31", "62", "125", "250", "500", "1000", "2000", "4000", "8000", "16000"
            ]
        );
    }

    #[test]
    fn shortcut_editor_projects_all_actions_into_one_accessible_list() {
        let model = SettingsScreenModel::build(
            &english_catalog(),
            &SettingsDocument::default(),
            Path::new("settings.json"),
            SettingsSection::Shortcuts,
        );
        assert!(matches!(
            &model.controls[0],
            SettingsControl::ShortcutActionList { actions, .. } if actions.len() == 91
        ));
        assert!(matches!(
            &model.controls[1],
            SettingsControl::ShortcutCapture { action_id, value, .. }
                if action_id == "open_main_menu" && value == "Ctrl+Alt+M"
        ));
    }
}
