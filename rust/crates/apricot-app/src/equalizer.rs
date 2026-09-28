//! Equalizer presets, profiles and the effective player equalizer, following
//! Python `apricot/ui/equalizer.py` (`EqualizerUI`).

use std::collections::BTreeMap;

use apricot_core::{
    SettingId, TranslationCatalog,
    audio::{CUSTOM_EQUALIZER_PRESET_IDS, EQUALIZER_BANDS, FACTORY_EQUALIZER_PRESETS},
};
use apricot_storage::SettingsDocument;
use serde_json::{Value, json};

use crate::player_session::EqualizerSession;

pub type EqualizerGains = BTreeMap<String, f64>;

/// Python `EQ_PRESET_FLAT`.
pub const FLAT_PRESET: &str = "flat";
/// Python `EQ_RANGE_OPTIONS`.
pub const RANGE_OPTIONS: [&str; 4] = ["6", "12", "18", "24"];
/// Python `EQ_APPLY_DELAY_MS`.
pub const APPLY_DELAY_MS: u32 = 160;
/// Python `configure_equalizer_slider_steps`: arrows move 1 dB, Page keys 3 dB.
pub const SLIDER_LINE_STEP: i32 = 10;
pub const SLIDER_PAGE_STEP: i32 = 30;

/// Python `EQ_BANDS` labels, used in `equalizer_band_gain`.
pub fn band_label(band_id: &str) -> &'static str {
    match band_id {
        "31" => "31 Hz sub bass rumble",
        "62" => "62 Hz bass thump",
        "125" => "125 Hz upper bass warmth",
        "250" => "250 Hz low mids",
        "500" => "500 Hz mids body",
        "1000" => "1 kHz midrange presence",
        "2000" => "2 kHz vocal clarity",
        "4000" => "4 kHz attack and detail",
        "8000" => "8 kHz brightness",
        "16000" => "16 kHz air and sparkle",
        _ => "",
    }
}

/// Python `t("equalizer_band_gain", band=band_label)`.
pub fn band_gain_label(catalog: &TranslationCatalog, band_id: &str) -> String {
    catalog
        .text("equalizer_band_gain")
        .replace("{band}", band_label(band_id))
}

/// Python `set_equalizer_slider_accessibility` value text, from slider tenths.
pub fn slider_value_text(tenths: i32) -> String {
    format!("{:.1} dB", f64::from(tenths) / 10.0)
}

/// Python `equalizer_gain_from_slider_value`.
pub fn gain_from_slider(tenths: i32) -> f64 {
    round_gain(f64::from(tenths) / 10.0)
}

/// Slider position for a gain limited to the dialog range.
#[allow(clippy::cast_possible_truncation)]
pub fn slider_from_gain(gain: f64, db_range: i64) -> i32 {
    let range = f64::from(i32::try_from(db_range.clamp(6, 24)).unwrap_or(12));
    (gain.clamp(-range, range) * 10.0).round() as i32
}

pub fn is_factory_preset(preset_id: &str) -> bool {
    FACTORY_EQUALIZER_PRESETS
        .iter()
        .any(|preset| preset.id == preset_id)
}

/// Python `is_custom_equalizer_preset`.
pub fn is_custom_preset(preset_id: &str) -> bool {
    !is_factory_preset(preset_id)
}

/// Python `default_equalizer_gains`.
pub fn default_gains() -> EqualizerGains {
    EQUALIZER_BANDS
        .iter()
        .map(|band| (band.id.to_owned(), 0.0))
        .collect()
}

fn round_gain(gain: f64) -> f64 {
    let gain = if gain.is_finite() { gain } else { 0.0 };
    (gain.clamp(-24.0, 24.0) * 10.0).round() / 10.0
}

/// Python `normalized_equalizer_gains`.
pub fn normalized_gains(gains: &EqualizerGains) -> EqualizerGains {
    EQUALIZER_BANDS
        .iter()
        .map(|band| {
            (
                band.id.to_owned(),
                round_gain(gains.get(band.id).copied().unwrap_or_default()),
            )
        })
        .collect()
}

/// Python `factory_equalizer_gains_for_preset`.
pub fn factory_gains(preset_id: &str) -> EqualizerGains {
    FACTORY_EQUALIZER_PRESETS
        .iter()
        .find(|preset| preset.id == preset_id)
        .map_or_else(default_gains, |preset| {
            EQUALIZER_BANDS
                .iter()
                .zip(preset.gains_db)
                .map(|(band, gain)| (band.id.to_owned(), f64::from(gain)))
                .collect()
        })
}

/// Python `equalizer_gains_with_bass_boost`.
pub fn gains_with_bass_boost(gains: &EqualizerGains) -> EqualizerGains {
    let boost = factory_gains("bass_boost");
    let mut combined = normalized_gains(gains);
    for band in EQUALIZER_BANDS {
        let gain = combined.entry(band.id.to_owned()).or_default();
        *gain = round_gain(*gain + boost.get(band.id).copied().unwrap_or_default());
    }
    combined
}

/// Python `equalizer_gains_match`.
pub fn gains_match(left: &EqualizerGains, right: &EqualizerGains) -> bool {
    EQUALIZER_BANDS.iter().all(|band| {
        (left.get(band.id).copied().unwrap_or_default()
            - right.get(band.id).copied().unwrap_or_default())
        .abs()
            < 0.05
    })
}

/// Python `apply_equalizer_to_player`: a filter is only added for an audible
/// band.
pub fn has_audible_gain(gains: &EqualizerGains) -> bool {
    gains.values().any(|gain| gain.abs() >= 0.05)
}

/// Python `equalizer_device_key`.
pub fn device_key(device: &str) -> String {
    let device = device.trim();
    if device.is_empty() {
        "auto".to_owned()
    } else {
        device.to_owned()
    }
}

/// Python `equalizer_device_display_name`.
pub fn device_display_name(device: &str) -> String {
    let key = device_key(device);
    if key.eq_ignore_ascii_case("auto") {
        "auto".to_owned()
    } else {
        key
    }
}

/// Python `export_equalizer_profile_dialog` default file name.
pub fn export_file_name(name: &str) -> String {
    // `re.sub(r"[^A-Za-z0-9._ -]+", "_", name)`: one underscore per run.
    let mut replaced = String::new();
    let mut in_run = false;
    for character in name.trim().chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ' ' | '-') {
            replaced.push(character);
            in_run = false;
        } else if !in_run {
            replaced.push('_');
            in_run = true;
        }
    }
    let limited: String = replaced.chars().take(80).collect();
    let trimmed = limited.trim_matches(|character| matches!(character, ' ' | '.' | '_'));
    if trimmed.is_empty() {
        "equalizer-profile.json".to_owned()
    } else {
        format!("{trimmed}.json")
    }
}

/// Python `path.with_suffix(".json")` for exported profiles.
pub fn export_path(path: &std::path::Path) -> std::path::PathBuf {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        path.to_path_buf()
    } else {
        path.with_extension("json")
    }
}

fn truncated_name(name: &str) -> String {
    name.trim().chars().take(80).collect::<String>()
}

/// The equalizer part of `settings.json` that Python `EqualizerUI` edits.
#[derive(Clone, Debug, PartialEq)]
pub struct EqualizerSettings {
    pub enabled: bool,
    pub preset: String,
    pub global_gains: EqualizerGains,
    pub preset_gains: BTreeMap<String, EqualizerGains>,
    pub custom_names: BTreeMap<String, String>,
    pub device_presets: BTreeMap<String, String>,
    pub db_range: i64,
    pub clipping_protection: bool,
}

impl EqualizerSettings {
    pub fn from_document(settings: &SettingsDocument) -> Self {
        Self {
            enabled: settings.global_equalizer_enabled,
            preset: settings.global_equalizer_preset.clone(),
            global_gains: settings.global_equalizer_gains.clone(),
            preset_gains: settings.equalizer_preset_gains.clone(),
            custom_names: settings.equalizer_custom_names.clone(),
            device_presets: settings.equalizer_device_presets.clone(),
            db_range: settings.equalizer_db_range,
            clipping_protection: settings.equalizer_clipping_protection,
        }
    }

    pub fn setting_values(&self) -> Vec<(SettingId, Value)> {
        vec![
            (SettingId::GlobalEqualizerEnabled, json!(self.enabled)),
            (SettingId::GlobalEqualizerPreset, json!(self.preset)),
            (SettingId::GlobalEqualizerGains, json!(self.global_gains)),
            (SettingId::EqualizerPresetGains, json!(self.preset_gains)),
            (SettingId::EqualizerCustomNames, json!(self.custom_names)),
            (
                SettingId::EqualizerDevicePresets,
                json!(self.device_presets),
            ),
            (SettingId::EqualizerDbRange, json!(self.db_range)),
            (
                SettingId::EqualizerClippingProtection,
                json!(self.clipping_protection),
            ),
        ]
    }

    /// Python `equalizer_db_range_value`.
    pub fn db_range(&self) -> i64 {
        self.db_range.clamp(6, 24)
    }

    /// Python `equalizer_custom_ids`: the three built-in custom slots first,
    /// then every other custom profile ordered case-insensitively.
    pub fn custom_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = CUSTOM_EQUALIZER_PRESET_IDS
            .iter()
            .map(|id| (*id).to_owned())
            .collect();
        let mut others: Vec<String> = self
            .custom_names
            .keys()
            .chain(self.preset_gains.keys())
            .filter(|id| {
                !id.trim().is_empty()
                    && is_custom_preset(id)
                    && !CUSTOM_EQUALIZER_PRESET_IDS.contains(&id.as_str())
            })
            .cloned()
            .collect();
        others.sort_by_key(|id| id.to_lowercase());
        others.dedup();
        ids.extend(others);
        ids
    }

    /// Python `equalizer_preset_options`.
    pub fn preset_options(&self) -> Vec<String> {
        FACTORY_EQUALIZER_PRESETS
            .iter()
            .map(|preset| preset.id.to_owned())
            .chain(self.custom_ids())
            .collect()
    }

    /// Python `normalized_equalizer_preset`.
    pub fn normalized_preset(&self, preset_id: &str) -> String {
        let value = preset_id.trim();
        let value = if value.is_empty() { FLAT_PRESET } else { value };
        if is_factory_preset(value)
            || CUSTOM_EQUALIZER_PRESET_IDS.contains(&value)
            || value.starts_with("custom_")
            || value.starts_with("user_")
            || self.preset_options().iter().any(|option| option == value)
        {
            value.to_owned()
        } else {
            FLAT_PRESET.to_owned()
        }
    }

    /// Python `equalizer_custom_name`.
    pub fn custom_name(&self, preset_id: &str) -> String {
        self.custom_names
            .get(preset_id)
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .or_else(|| {
                CUSTOM_EQUALIZER_PRESET_IDS
                    .iter()
                    .position(|id| *id == preset_id)
                    .map(|index| format!("Custom {}", index + 1))
            })
            .unwrap_or_else(|| preset_id.to_owned())
    }

    /// Python `equalizer_preset_label`.
    pub fn preset_label(&self, catalog: &TranslationCatalog, preset_id: &str) -> String {
        if is_custom_preset(preset_id) {
            self.custom_name(preset_id)
        } else {
            catalog.text(&format!("eq_preset_{preset_id}")).to_owned()
        }
    }

    /// Python `equalizer_preset_labels`.
    pub fn preset_labels(&self, catalog: &TranslationCatalog) -> Vec<String> {
        self.preset_options()
            .iter()
            .map(|id| self.preset_label(catalog, id))
            .collect()
    }

    /// Python `equalizer_gains_for_preset`: factory presets always use their
    /// factory values, custom profiles their saved gains.
    pub fn gains_for_preset(&self, preset_id: &str) -> EqualizerGains {
        let preset_id = self.normalized_preset(preset_id);
        if is_factory_preset(&preset_id) {
            return factory_gains(&preset_id);
        }
        self.preset_gains
            .get(&preset_id)
            .map_or_else(default_gains, normalized_gains)
    }

    /// Python `effective_equalizer_preset`: the output device's own preset,
    /// otherwise the global one.
    pub fn effective_preset(&self, device: &str) -> String {
        let key = device_key(device);
        let preset = self
            .device_presets
            .get(&key)
            .filter(|preset| !preset.trim().is_empty())
            .unwrap_or(&self.preset);
        self.normalized_preset(preset)
    }

    /// Python `base_equalizer_state`.
    pub fn base_state(
        &self,
        session_override: Option<&EqualizerSession>,
        device: &str,
    ) -> (bool, EqualizerGains) {
        if let Some(session) = session_override {
            return (session.enabled, normalized_gains(&session.gains));
        }
        (
            self.enabled,
            self.gains_for_preset(&self.effective_preset(device)),
        )
    }

    /// Python `effective_equalizer_state`: bass boost always adds its curve,
    /// to the flat response when the equalizer itself is off.
    pub fn effective_state(
        &self,
        session_override: Option<&EqualizerSession>,
        device: &str,
        bass_boost: bool,
    ) -> (bool, EqualizerGains) {
        let (enabled, gains) = self.base_state(session_override, device);
        if bass_boost {
            let base = if enabled { gains } else { default_gains() };
            return (true, gains_with_bass_boost(&base));
        }
        (enabled, gains)
    }

    /// Python `equalizer_default_profile_name`.
    pub fn default_profile_name(&self) -> String {
        format!("Custom {}", self.custom_ids().len() + 1)
    }

    /// Python `next_equalizer_profile_id`.
    pub fn next_profile_id(&self) -> String {
        let existing = self.preset_options();
        // One more candidate than there are presets always finds a free id.
        (1..=existing.len() + 1)
            .map(|counter| format!("custom_{counter}"))
            .find(|candidate| !existing.contains(candidate))
            .unwrap_or_else(|| "custom_1".to_owned())
    }

    /// Python `create_equalizer_profile`.
    pub fn create_profile(&mut self, name: &str, gains: &EqualizerGains) -> String {
        let preset_id = self.next_profile_id();
        let name = truncated_name(name);
        let name = if name.is_empty() {
            self.default_profile_name()
        } else {
            name
        };
        self.custom_names.insert(preset_id.clone(), name);
        self.preset_gains
            .insert(preset_id.clone(), normalized_gains(gains));
        preset_id
    }

    /// Python `delete_equalizer_profile` after confirmation: the built-in
    /// custom slots are reset, other profiles removed, and the global preset
    /// returns to flat. Returns `None` for factory presets.
    pub fn delete_profile(&mut self, preset_id: &str) -> Option<String> {
        let preset_id = self.normalized_preset(preset_id);
        if !is_custom_preset(&preset_id) {
            return None;
        }
        self.custom_names.remove(&preset_id);
        if let Some(index) = CUSTOM_EQUALIZER_PRESET_IDS
            .iter()
            .position(|id| *id == preset_id)
        {
            self.preset_gains.insert(preset_id.clone(), default_gains());
            self.custom_names
                .insert(preset_id.clone(), format!("Custom {}", index + 1));
        } else {
            self.preset_gains.remove(&preset_id);
        }
        self.device_presets.retain(|_, preset| *preset != preset_id);
        FLAT_PRESET.clone_into(&mut self.preset);
        self.global_gains = self.gains_for_preset(FLAT_PRESET);
        Some(FLAT_PRESET.to_owned())
    }

    /// Python `save_current_dialog_name`: only custom profiles have a name.
    pub fn set_custom_name(&mut self, preset_id: &str, name: &str) {
        if !is_custom_preset(preset_id) {
            return;
        }
        let name = truncated_name(name);
        let name = if name.is_empty() {
            self.custom_name(preset_id)
        } else {
            name
        };
        self.custom_names.insert(preset_id.to_owned(), name);
    }

    /// Stores gains for a custom profile; factory presets are read-only.
    pub fn set_preset_gains(&mut self, preset_id: &str, gains: &EqualizerGains) {
        if is_custom_preset(preset_id) {
            self.preset_gains
                .insert(preset_id.to_owned(), normalized_gains(gains));
        }
    }

    /// Python `set_equalizer_device_preset`: an empty preset clears the entry.
    pub fn set_device_preset(&mut self, device: &str, preset_id: &str) {
        let key = device_key(device);
        if preset_id.trim().is_empty() {
            self.device_presets.remove(&key);
        } else {
            let preset = self.normalized_preset(preset_id);
            self.device_presets.insert(key, preset);
        }
    }

    /// Python `equalizer_profile_export_payload`.
    pub fn export_payload(&self, name: &str, gains: &EqualizerGains, preset_id: &str) -> Value {
        let name = truncated_name(name);
        json!({
            "type": "apricot_equalizer_profile",
            "version": 1,
            "name": if name.is_empty() { self.default_profile_name() } else { name },
            "preset_id": preset_id,
            "bands": EQUALIZER_BANDS.iter().map(|band| band.id).collect::<Vec<_>>(),
            "gains": normalized_gains(gains),
        })
    }

    /// Python `equalizer_profile_from_payload`.
    ///
    /// # Errors
    ///
    /// Returns `invalid_message` when the payload has no gains object.
    pub fn profile_from_payload(
        &self,
        payload: &Value,
        invalid_message: &str,
    ) -> Result<(String, EqualizerGains), String> {
        let gains = payload
            .as_object()
            .and_then(|payload| payload.get("gains"))
            .and_then(Value::as_object)
            .ok_or_else(|| invalid_message.to_owned())?;
        let gains: EqualizerGains = EQUALIZER_BANDS
            .iter()
            .map(|band| {
                let gain = gains.get(band.id).map_or(0.0, |value| {
                    value
                        .as_f64()
                        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
                        .unwrap_or_default()
                });
                (band.id.to_owned(), round_gain(gain))
            })
            .collect();
        let name = payload
            .get("name")
            .and_then(Value::as_str)
            .map(truncated_name)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.default_profile_name());
        Ok((name, gains))
    }
}

#[cfg(test)]
mod tests {
    use apricot_storage::SettingsDocument;

    use super::*;

    fn settings() -> EqualizerSettings {
        EqualizerSettings::from_document(&SettingsDocument::default())
    }

    #[test]
    fn presets_list_factory_then_custom_profiles_like_python() {
        let mut equalizer = settings();
        equalizer
            .custom_names
            .insert("custom_2".to_owned(), "Zeta".to_owned());
        equalizer
            .preset_gains
            .insert("custom_1".to_owned(), default_gains());
        let options = equalizer.preset_options();
        assert_eq!(options.len(), 18 + 5);
        assert_eq!(options[0], "flat");
        assert_eq!(
            &options[18..],
            ["custom1", "custom2", "custom3", "custom_1", "custom_2"]
        );
        let catalog = crate::embedded_catalog("en");
        assert_eq!(equalizer.preset_label(&catalog, "flat"), "Default / flat");
        assert_eq!(equalizer.preset_label(&catalog, "custom2"), "Custom 2");
        assert_eq!(equalizer.preset_label(&catalog, "custom_2"), "Zeta");
        assert_eq!(equalizer.next_profile_id(), "custom_3");
    }

    #[test]
    fn band_labels_are_the_python_descriptions() {
        let catalog = crate::embedded_catalog("en");
        assert_eq!(
            band_gain_label(&catalog, "1000"),
            "Equalizer 1 kHz midrange presence"
        );
        assert_eq!(slider_value_text(-35), "-3.5 dB");
        assert_eq!(slider_from_gain(30.0, 12), 120);
        assert!((gain_from_slider(-35) + 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn effective_state_follows_device_preset_session_and_bass_boost() {
        let mut equalizer = settings();
        equalizer.enabled = true;
        "rock".clone_into(&mut equalizer.preset);
        let (enabled, gains) = equalizer.effective_state(None, "auto", false);
        assert!(enabled);
        assert!((gains["31"] - 4.0).abs() < f64::EPSILON);

        equalizer.set_device_preset("wasapi/headphones", "jazz");
        let (_, gains) = equalizer.effective_state(None, "wasapi/headphones", false);
        assert!((gains["31"] - 2.0).abs() < f64::EPSILON);

        let session = EqualizerSession {
            enabled: false,
            gains: factory_gains("pop"),
        };
        let (enabled, gains) = equalizer.effective_state(Some(&session), "auto", false);
        assert!(!enabled);
        assert!((gains["62"] - 2.0).abs() < f64::EPSILON);

        // A disabled equalizer contributes nothing to bass boost.
        let (enabled, gains) = equalizer.effective_state(Some(&session), "auto", true);
        assert!(enabled);
        assert!(gains_match(&gains, &factory_gains("bass_boost")));

        equalizer.enabled = true;
        let (_, gains) = equalizer.effective_state(None, "auto", true);
        assert!((gains["31"] - 9.0).abs() < f64::EPSILON);
    }

    #[test]
    fn profiles_are_created_named_and_deleted_like_python() {
        let mut equalizer = settings();
        let mut gains = default_gains();
        gains.insert("250".to_owned(), 3.25);
        let id = equalizer.create_profile("  Night  ", &gains);
        assert_eq!(id, "custom_1");
        assert_eq!(equalizer.custom_name(&id), "Night");
        assert!((equalizer.gains_for_preset(&id)["250"] - 3.3).abs() < 1e-9);

        let unnamed = equalizer.create_profile("", &gains);
        assert_eq!(equalizer.custom_name(&unnamed), "Custom 5");

        equalizer.set_device_preset("auto", &id);
        id.clone_into(&mut equalizer.preset);
        assert_eq!(equalizer.delete_profile(&id).as_deref(), Some("flat"));
        assert!(!equalizer.preset_options().contains(&id));
        assert!(equalizer.device_presets.is_empty());
        assert_eq!(equalizer.preset, "flat");

        equalizer.set_preset_gains("custom1", &gains);
        equalizer.set_custom_name("custom1", "Mine");
        assert_eq!(equalizer.delete_profile("custom1").as_deref(), Some("flat"));
        assert_eq!(equalizer.custom_name("custom1"), "Custom 1");
        assert!(gains_match(
            &equalizer.gains_for_preset("custom1"),
            &default_gains()
        ));
        assert_eq!(equalizer.delete_profile("rock"), None);
    }

    #[test]
    fn exported_profiles_round_trip_through_the_python_payload() {
        let equalizer = settings();
        let gains = factory_gains("vocal");
        let payload = equalizer.export_payload("Vocal clarity", &gains, "vocal");
        assert_eq!(payload["type"], "apricot_equalizer_profile");
        assert_eq!(payload["version"], 1);
        assert_eq!(payload["bands"].as_array().map(Vec::len), Some(10));
        let (name, imported) = equalizer
            .profile_from_payload(&payload, "invalid")
            .expect("valid payload");
        assert_eq!(name, "Vocal clarity");
        assert!(gains_match(&imported, &gains));
        assert_eq!(
            equalizer.profile_from_payload(&json!({"name": "x"}), "invalid"),
            Err("invalid".to_owned())
        );
        assert_eq!(export_file_name("Rock / Live: 2"), "Rock _ Live_ 2.json");
        assert_eq!(export_file_name("///"), "equalizer-profile.json");
        assert_eq!(
            export_path(std::path::Path::new(r"C:\x\eq.txt")),
            std::path::PathBuf::from(r"C:\x\eq.json")
        );
    }
}
