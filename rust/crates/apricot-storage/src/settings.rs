use std::collections::{BTreeMap, BTreeSet};

use apricot_core::{
    CUSTOMIZABLE_MAIN_MENU,
    action::ACTIONS,
    audio::{CUSTOM_EQUALIZER_PRESET_IDS, EQUALIZER_BANDS, FACTORY_EQUALIZER_PRESETS},
    locale::LANGUAGES,
    setting::SettingId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

const DEFAULT_FILENAME_TEMPLATE: &str = "%(title)s.%(ext)s";
const OLD_FILENAME_TEMPLATE: &str = "%(title)s [%(id)s].%(ext)s";
const DEFAULT_PITCH_MODE: &str = "Independent pitch - highest quality (mpv built-in)";
const RUBBERBAND_SPEED_MODE: &str = "Rubberband high quality";
const AUDIO_QUALITY_OPTIONS: &[&str] = &[
    "0", "320", "256", "192", "160", "128", "96", "64", "1", "2", "3", "4", "5", "6", "7", "8",
    "9", "10",
];

#[derive(Debug, Error)]
pub enum SettingsLoadError {
    #[error("settings must contain a JSON object")]
    NotAnObject,
    #[error("settings contain no recognized ApricotPlayer fields")]
    NoRecognizedFields,
    #[error("settings could not be decoded: {0}")]
    Decode(#[from] serde_json::Error),
}

/// Complete Python 1.0.21 settings schema with forward-compatible key retention.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct SettingsDocument {
    pub language: String,
    pub download_folder: String,
    pub results_limit: i64,
    pub audio_format: String,
    pub video_format: String,
    pub max_video_height: i64,
    pub player_command: String,
    pub autoplay_next: bool,
    pub autoplay_related: bool,
    pub prefer_browser_playback: bool,
    pub player_fullscreen: bool,
    pub player_start_paused: bool,
    pub announce_play_pause: bool,
    pub announce_playback_finished: bool,
    pub enable_background_playback: bool,
    pub player_speed: String,
    pub speed_audio_mode: String,
    pub show_video_details_by_default: bool,
    pub direct_link_enter_action: String,
    pub enable_age_restricted_videos: bool,
    pub enable_stream_cache: bool,
    pub enable_stream_url_cache: bool,
    pub stream_url_cache_minutes: i64,
    pub stream_format_preference: String,
    pub prefetch_next_stream_url: bool,
    pub gapless_playback: bool,
    pub replaygain_mode: String,
    pub enable_online_lyrics: bool,
    pub cache_folder: String,
    pub cache_size_mb: i64,
    pub resume_playback: bool,
    pub show_resume_in_menu: bool,
    pub audio_output_device: String,
    pub speed_step: f64,
    pub pitch_step: f64,
    pub speed_pitch_hold_delay_ms: i64,
    pub speed_pitch_hold_interval_ms: i64,
    pub pitch_mode: String,
    pub global_equalizer_enabled: bool,
    pub global_equalizer_preset: String,
    pub global_equalizer_gains: BTreeMap<String, f64>,
    pub equalizer_preset_gains: BTreeMap<String, BTreeMap<String, f64>>,
    pub equalizer_custom_names: BTreeMap<String, String>,
    pub equalizer_device_presets: BTreeMap<String, String>,
    pub equalizer_db_range: i64,
    pub equalizer_clipping_protection: bool,
    pub ask_download_location_each_time: bool,
    pub quiet_downloads: bool,
    pub keep_playlist_order: bool,
    pub filename_template: String,
    pub audio_quality: String,
    pub seek_seconds: f64,
    pub volume_step: i64,
    pub default_volume: i64,
    pub volume_boost_by_default: bool,
    pub write_thumbnail: bool,
    pub write_description: bool,
    pub write_info_json: bool,
    pub write_subtitles: bool,
    pub auto_subtitles: bool,
    pub subtitle_languages: String,
    pub embed_metadata: bool,
    pub embed_thumbnail: bool,
    pub restrict_filenames: bool,
    pub open_folder_after_download: bool,
    pub popup_when_download_complete: bool,
    pub popup_when_conversion_complete: bool,
    pub auto_update_ytdlp: bool,
    pub auto_update_app: bool,
    pub app_update_interval_hours: f64,
    pub app_update_notifications: bool,
    pub skipped_update_version: String,
    pub update_channel: String,
    pub confirm_before_download: bool,
    pub download_archive: bool,
    pub rate_limit: String,
    pub proxy: String,
    pub youtube_data_api_key: String,
    pub audiovault_email: String,
    pub audiovault_password_protected: String,
    pub cookies_file: String,
    pub cookies_source_file: String,
    pub cookies_source_signature: String,
    pub cookies_from_browser: String,
    pub cookies_browser_profile: String,
    pub show_advanced_network_settings: bool,
    pub cookie_user_agent: String,
    pub ffmpeg_location: String,
    pub concurrent_fragments: i64,
    pub retries: i64,
    pub socket_timeout: i64,
    pub close_to_tray: bool,
    pub start_with_windows: bool,
    pub tray_notification: bool,
    pub subscription_check_enabled: bool,
    pub subscription_check_interval_hours: f64,
    pub windows_notifications: bool,
    pub download_notifications: bool,
    pub subscription_notifications: bool,
    pub last_subscription_check: f64,
    pub enable_trending: bool,
    pub enable_history: bool,
    pub enable_podcasts_rss: bool,
    pub show_shortcuts_in_labels: bool,
    pub main_menu_hidden_actions: Vec<String>,
    pub podcast_search_provider: String,
    pub podcast_search_country: String,
    pub podcast_search_limit: i64,
    pub rss_max_items: i64,
    pub rss_refresh_on_startup: bool,
    pub rss_auto_refresh_enabled: bool,
    pub rss_refresh_interval_hours: f64,
    pub history_limit: i64,
    pub keyboard_shortcuts: BTreeMap<String, String>,
    pub media_association_prompted_version: String,
    pub language_prompted: bool,
    #[serde(flatten)]
    pub preserved: BTreeMap<String, Value>,
}

impl Default for SettingsDocument {
    #[allow(clippy::too_many_lines)]
    fn default() -> Self {
        Self {
            language: "en".to_owned(),
            download_folder: String::new(),
            results_limit: 0,
            audio_format: "mp3".to_owned(),
            video_format: "mp4".to_owned(),
            max_video_height: 1_080,
            player_command: String::new(),
            autoplay_next: false,
            autoplay_related: false,
            prefer_browser_playback: false,
            player_fullscreen: false,
            player_start_paused: false,
            announce_play_pause: true,
            announce_playback_finished: true,
            enable_background_playback: false,
            player_speed: "1.0".to_owned(),
            speed_audio_mode: RUBBERBAND_SPEED_MODE.to_owned(),
            show_video_details_by_default: false,
            direct_link_enter_action: "play".to_owned(),
            enable_age_restricted_videos: false,
            enable_stream_cache: true,
            enable_stream_url_cache: true,
            stream_url_cache_minutes: 360,
            stream_format_preference: "auto".to_owned(),
            prefetch_next_stream_url: true,
            gapless_playback: true,
            replaygain_mode: "no".to_owned(),
            enable_online_lyrics: true,
            cache_folder: String::new(),
            cache_size_mb: 512,
            resume_playback: true,
            show_resume_in_menu: true,
            audio_output_device: "auto".to_owned(),
            speed_step: 0.01,
            pitch_step: 0.01,
            speed_pitch_hold_delay_ms: 180,
            speed_pitch_hold_interval_ms: 110,
            pitch_mode: DEFAULT_PITCH_MODE.to_owned(),
            global_equalizer_enabled: false,
            global_equalizer_preset: "flat".to_owned(),
            global_equalizer_gains: default_equalizer_gains(),
            equalizer_preset_gains: default_equalizer_preset_gains(),
            equalizer_custom_names: default_equalizer_custom_names(),
            equalizer_device_presets: BTreeMap::new(),
            equalizer_db_range: 12,
            equalizer_clipping_protection: false,
            ask_download_location_each_time: false,
            quiet_downloads: false,
            keep_playlist_order: true,
            filename_template: DEFAULT_FILENAME_TEMPLATE.to_owned(),
            audio_quality: "0".to_owned(),
            seek_seconds: 5.0,
            volume_step: 5,
            default_volume: 100,
            volume_boost_by_default: false,
            write_thumbnail: false,
            write_description: false,
            write_info_json: false,
            write_subtitles: false,
            auto_subtitles: false,
            subtitle_languages: "sl,en".to_owned(),
            embed_metadata: true,
            embed_thumbnail: false,
            restrict_filenames: false,
            open_folder_after_download: false,
            popup_when_download_complete: true,
            popup_when_conversion_complete: true,
            auto_update_ytdlp: true,
            auto_update_app: true,
            app_update_interval_hours: 6.0,
            app_update_notifications: true,
            skipped_update_version: String::new(),
            update_channel: "stable".to_owned(),
            confirm_before_download: false,
            download_archive: false,
            rate_limit: String::new(),
            proxy: String::new(),
            youtube_data_api_key: String::new(),
            audiovault_email: String::new(),
            audiovault_password_protected: String::new(),
            cookies_file: String::new(),
            cookies_source_file: String::new(),
            cookies_source_signature: String::new(),
            cookies_from_browser: "none".to_owned(),
            cookies_browser_profile: "auto".to_owned(),
            show_advanced_network_settings: false,
            cookie_user_agent: String::new(),
            ffmpeg_location: String::new(),
            concurrent_fragments: 4,
            retries: 10,
            socket_timeout: 20,
            close_to_tray: false,
            start_with_windows: false,
            tray_notification: true,
            subscription_check_enabled: true,
            subscription_check_interval_hours: 6.0,
            windows_notifications: true,
            download_notifications: true,
            subscription_notifications: true,
            last_subscription_check: 0.0,
            enable_trending: false,
            enable_history: true,
            enable_podcasts_rss: true,
            show_shortcuts_in_labels: true,
            main_menu_hidden_actions: Vec::new(),
            podcast_search_provider: "apple".to_owned(),
            podcast_search_country: "US".to_owned(),
            podcast_search_limit: 20,
            rss_max_items: 100,
            rss_refresh_on_startup: false,
            rss_auto_refresh_enabled: false,
            rss_refresh_interval_hours: 12.0,
            history_limit: 500,
            keyboard_shortcuts: default_keyboard_shortcuts(),
            media_association_prompted_version: String::new(),
            language_prompted: false,
            preserved: BTreeMap::new(),
        }
    }
}

impl SettingsDocument {
    pub fn with_platform_defaults(
        download_folder: impl Into<String>,
        cache_folder: impl Into<String>,
        update_channel: impl Into<String>,
    ) -> Self {
        Self {
            download_folder: download_folder.into(),
            cache_folder: cache_folder.into(),
            update_channel: update_channel.into(),
            ..Self::default()
        }
    }

    /// Merges a Python settings object over platform-specific defaults.
    ///
    /// # Errors
    ///
    /// Returns `SettingsLoadError` for a non-object, an object without any
    /// recognized settings, or values incompatible with the typed schema.
    pub fn from_value_with_defaults(
        raw: &Value,
        defaults: Self,
    ) -> Result<Self, SettingsLoadError> {
        let raw = raw.as_object().ok_or(SettingsLoadError::NotAnObject)?;
        let known: BTreeSet<_> = SettingId::ALL.iter().map(|id| id.key()).collect();
        if !raw.keys().any(|key| known.contains(key.as_str())) {
            return Err(SettingsLoadError::NoRecognizedFields);
        }
        let mut merged = serde_json::to_value(defaults)?
            .as_object()
            .cloned()
            .ok_or(SettingsLoadError::NotAnObject)?;
        merged.extend(raw.clone());
        let mut settings: Self = serde_json::from_value(Value::Object(merged))?;
        settings.normalize();
        Ok(settings)
    }

    pub fn normalize(&mut self) {
        if !LANGUAGES
            .iter()
            .any(|language| language.code == self.language)
        {
            "en".clone_into(&mut self.language);
        }
        if self.filename_template == OLD_FILENAME_TEMPLATE {
            DEFAULT_FILENAME_TEMPLATE.clone_into(&mut self.filename_template);
        }
        self.video_format = normalized_member(
            &self.video_format,
            &["mp4", "best-any", "mp4-single", "smallest"],
            "mp4",
        );
        self.direct_link_enter_action = normalized_member(
            &self.direct_link_enter_action,
            &[
                "play",
                "download_audio",
                "download_video",
                "copy_stream_url",
            ],
            "play",
        );
        self.stream_format_preference = normalized_member(
            &self.stream_format_preference.to_lowercase(),
            &["auto", "video", "audio"],
            "auto",
        );
        self.replaygain_mode = normalize_replaygain(&self.replaygain_mode);
        self.pitch_mode = normalize_pitch_mode(&self.pitch_mode);
        self.speed_audio_mode = normalize_speed_audio_mode(&self.speed_audio_mode);
        self.audio_quality = normalize_audio_quality(&self.audio_quality);
        self.stream_url_cache_minutes = if self.stream_url_cache_minutes <= 0 {
            0
        } else {
            self.stream_url_cache_minutes.clamp(5, 10_080)
        };
        self.seek_seconds = finite_or(self.seek_seconds, 5.0).clamp(0.1, 600.0);
        self.speed_step = finite_or(self.speed_step, 0.01).clamp(0.01, 0.25);
        self.pitch_step = finite_or(self.pitch_step, 0.01).clamp(0.01, 0.25);
        self.speed_pitch_hold_delay_ms = self.speed_pitch_hold_delay_ms.clamp(50, 1_000);
        self.speed_pitch_hold_interval_ms = self.speed_pitch_hold_interval_ms.clamp(20, 500);
        self.equalizer_db_range = self.equalizer_db_range.clamp(6, 24);
        self.global_equalizer_gains = normalize_equalizer_gains(&self.global_equalizer_gains);
        self.equalizer_preset_gains = normalize_equalizer_presets(&self.equalizer_preset_gains);
        self.equalizer_custom_names = normalize_custom_names(&self.equalizer_custom_names);
        let available_presets: BTreeSet<_> = self.equalizer_preset_gains.keys().cloned().collect();
        if !available_presets.contains(&self.global_equalizer_preset) {
            "flat".clone_into(&mut self.global_equalizer_preset);
        }
        self.equalizer_device_presets.retain(|device, preset| {
            !device.trim().is_empty() && available_presets.contains(preset)
        });
        let volume_max = if self.volume_boost_by_default {
            300
        } else {
            100
        };
        self.default_volume = self.default_volume.clamp(0, volume_max);
        self.main_menu_hidden_actions = normalize_hidden_menu(&self.main_menu_hidden_actions);
        "apple".clone_into(&mut self.podcast_search_provider);
        self.podcast_search_country = self.podcast_search_country.trim().to_uppercase();
        if self.podcast_search_country.len() != 2 {
            "US".clone_into(&mut self.podcast_search_country);
        }
        self.podcast_search_limit = self.podcast_search_limit.clamp(1, 100);
        self.rss_max_items = self.rss_max_items.clamp(25, 500);
        self.keyboard_shortcuts = normalize_shortcuts(&self.keyboard_shortcuts);
        if self.cookies_browser_profile.trim().is_empty() {
            "auto".clone_into(&mut self.cookies_browser_profile);
        }
        if !matches!(self.update_channel.as_str(), "stable" | "beta") {
            "stable".clone_into(&mut self.update_channel);
        }
    }
}

fn default_keyboard_shortcuts() -> BTreeMap<String, String> {
    ACTIONS
        .iter()
        .map(|action| {
            (
                action.id.as_str().to_owned(),
                action.default_windows_shortcut.to_owned(),
            )
        })
        .collect()
}

fn normalize_shortcuts(input: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    ACTIONS
        .iter()
        .map(|action| {
            let value = input
                .get(action.id.as_str())
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .unwrap_or(action.default_windows_shortcut);
            (action.id.as_str().to_owned(), value.to_owned())
        })
        .collect()
}

fn default_equalizer_gains() -> BTreeMap<String, f64> {
    EQUALIZER_BANDS
        .iter()
        .map(|band| (band.id.to_owned(), 0.0))
        .collect()
}

fn default_equalizer_preset_gains() -> BTreeMap<String, BTreeMap<String, f64>> {
    let mut presets: BTreeMap<_, _> = FACTORY_EQUALIZER_PRESETS
        .iter()
        .map(|preset| {
            let gains = EQUALIZER_BANDS
                .iter()
                .zip(preset.gains_db)
                .map(|(band, gain)| (band.id.to_owned(), f64::from(gain)))
                .collect();
            (preset.id.to_owned(), gains)
        })
        .collect();
    for id in CUSTOM_EQUALIZER_PRESET_IDS {
        presets.insert((*id).to_owned(), default_equalizer_gains());
    }
    presets
}

fn default_equalizer_custom_names() -> BTreeMap<String, String> {
    CUSTOM_EQUALIZER_PRESET_IDS
        .iter()
        .enumerate()
        .map(|(index, id)| ((*id).to_owned(), format!("Custom {}", index + 1)))
        .collect()
}

fn normalize_equalizer_gains(input: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    EQUALIZER_BANDS
        .iter()
        .map(|band| {
            let gain =
                finite_or(input.get(band.id).copied().unwrap_or(0.0), 0.0).clamp(-24.0, 24.0);
            (band.id.to_owned(), (gain * 10.0).round() / 10.0)
        })
        .collect()
}

fn normalize_equalizer_presets(
    input: &BTreeMap<String, BTreeMap<String, f64>>,
) -> BTreeMap<String, BTreeMap<String, f64>> {
    let mut output = default_equalizer_preset_gains();
    for (id, gains) in input {
        if !id.trim().is_empty()
            && !FACTORY_EQUALIZER_PRESETS
                .iter()
                .any(|preset| preset.id == id)
        {
            output.insert(id.clone(), normalize_equalizer_gains(gains));
        }
    }
    output
}

fn normalize_custom_names(input: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut output = default_equalizer_custom_names();
    for (id, name) in input {
        let trimmed = name.trim();
        if !id.trim().is_empty()
            && !trimmed.is_empty()
            && !FACTORY_EQUALIZER_PRESETS
                .iter()
                .any(|preset| preset.id == id)
        {
            output.insert(id.clone(), trimmed.chars().take(80).collect());
        }
    }
    output
}

fn normalize_hidden_menu(input: &[String]) -> Vec<String> {
    let hidden: BTreeSet<_> = input.iter().map(String::as_str).collect();
    CUSTOMIZABLE_MAIN_MENU
        .iter()
        .filter(|item| hidden.contains(item.action_id))
        .map(|item| item.action_id.to_owned())
        .collect()
}

fn normalize_replaygain(input: &str) -> String {
    match input.trim().to_lowercase().as_str() {
        "track" | "song" => "track".to_owned(),
        "album" => "album".to_owned(),
        _ => "no".to_owned(),
    }
}

fn normalize_pitch_mode(input: &str) -> String {
    let trimmed = input.trim();
    let lowered = trimmed.to_lowercase();
    match lowered.as_str() {
        "rubberband" | "independent pitch - best quality (rubberband)" => {
            "Independent pitch - advanced (Rubberband)".to_owned()
        }
        "linked speed" => "Linked pitch and speed - pitch keys change both".to_owned(),
        "mpv pitch" | "independent pitch - basic (mpv built-in)" => DEFAULT_PITCH_MODE.to_owned(),
        _ if matches!(
            trimmed,
            "Independent pitch - advanced (Rubberband)"
                | "Independent pitch - highest quality (mpv built-in)"
                | "Linked pitch and speed - pitch keys change both"
        ) =>
        {
            trimmed.to_owned()
        }
        _ => DEFAULT_PITCH_MODE.to_owned(),
    }
}

fn normalize_speed_audio_mode(input: &str) -> String {
    let trimmed = input.trim();
    if matches!(
        trimmed,
        "Rubberband high quality"
            | "High quality scaletempo2"
            | "mpv default scaletempo2"
            | "Classic scaletempo"
    ) {
        return trimmed.to_owned();
    }
    let lowered = trimmed.to_lowercase();
    if lowered.contains("classic") || lowered == "scaletempo" {
        "Classic scaletempo".to_owned()
    } else if lowered.contains("mpv") || lowered.contains("default") {
        "mpv default scaletempo2".to_owned()
    } else {
        RUBBERBAND_SPEED_MODE.to_owned()
    }
}

fn normalize_audio_quality(input: &str) -> String {
    let cleaned = input
        .trim()
        .to_lowercase()
        .replace("kbps", "")
        .replace('k', "")
        .trim()
        .to_owned();
    let Ok(number) = cleaned.parse::<f64>() else {
        return "0".to_owned();
    };
    let normalized = if number.fract().abs() < f64::EPSILON {
        format!("{number:.0}")
    } else {
        number.to_string()
    };
    normalized_member(&normalized, AUDIO_QUALITY_OPTIONS, "0")
}

fn normalized_member(input: &str, options: &[&str], fallback: &str) -> String {
    let trimmed = input.trim();
    options
        .iter()
        .find(|option| **option == trimmed)
        .copied()
        .unwrap_or(fallback)
        .to_owned()
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use apricot_core::setting::SettingId;
    use serde_json::json;

    use super::{SettingsDocument, SettingsLoadError};

    #[test]
    fn typed_schema_serializes_exactly_all_116_known_settings() {
        let object = serde_json::to_value(SettingsDocument::default())
            .expect("serialize")
            .as_object()
            .cloned()
            .expect("object");
        let actual: BTreeSet<_> = object.keys().map(String::as_str).collect();
        let expected: BTreeSet<_> = SettingId::ALL.iter().map(|id| id.key()).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn unknown_python_settings_survive_normalized_round_trip() {
        let input = json!({
            "language": "sl",
            "stream_format_preference": "audio",
            "future_python_key": {"enabled": true}
        });
        let document = SettingsDocument::from_value_with_defaults(
            &input,
            SettingsDocument::with_platform_defaults("downloads", "cache", "beta"),
        )
        .expect("deserialize");
        let output = serde_json::to_value(document).expect("serialize");
        assert_eq!(output["future_python_key"], json!({"enabled": true}));
        assert_eq!(output["download_folder"], "downloads");
        assert_eq!(output["cache_folder"], "cache");
        assert_eq!(output["update_channel"], "beta");
    }

    #[test]
    fn normalization_repairs_ranges_enums_and_shortcuts() {
        let input = json!({
            "language": "unknown",
            "seek_seconds": 900,
            "default_volume": 300,
            "volume_boost_by_default": false,
            "audio_quality": "320kbps",
            "replaygain_mode": "song",
            "global_equalizer_gains": {"31": 99.0},
            "keyboard_shortcuts": {"open_search": "Ctrl+F"},
            "main_menu_hidden_actions": ["search", "settings", "not-real"]
        });
        let document =
            SettingsDocument::from_value_with_defaults(&input, SettingsDocument::default())
                .expect("settings");
        assert_eq!(document.language, "en");
        assert!((document.seek_seconds - 600.0).abs() < f64::EPSILON);
        assert_eq!(document.default_volume, 100);
        assert_eq!(document.audio_quality, "320");
        assert_eq!(document.replaygain_mode, "track");
        assert!((document.global_equalizer_gains["31"] - 24.0).abs() < f64::EPSILON);
        assert_eq!(document.keyboard_shortcuts.len(), 91);
        assert_eq!(document.keyboard_shortcuts["open_search"], "Ctrl+F");
        assert_eq!(document.main_menu_hidden_actions, ["search"]);
    }

    #[test]
    fn objects_without_known_fields_are_rejected() {
        let error = SettingsDocument::from_value_with_defaults(
            &json!({"future_only": true}),
            SettingsDocument::default(),
        )
        .expect_err("must reject");
        assert!(matches!(error, SettingsLoadError::NoRecognizedFields));
    }
}
