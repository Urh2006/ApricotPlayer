//! Advanced equalizer settings of one profile: the frequency and width of
//! every band and a low and a high shelf. A profile without changes plays
//! through exactly the same filter graph as before 2.1.

use std::collections::BTreeMap;

use apricot_core::audio::EQUALIZER_BANDS;
use serde::{Deserialize, Serialize};

/// Key in `settings.json` (a map from profile id to its advanced settings).
pub const ADVANCED_SETTINGS_KEY: &str = "equalizer_advanced_v1";

pub const FREQUENCY_RANGE: (f64, f64) = (10.0, 24_000.0);
pub const WIDTH_RANGE: (f64, f64) = (0.1, 10.0);
pub const SHELF_GAIN_RANGE: (f64, f64) = (-24.0, 24.0);
const LOW_SHELF_FREQUENCY: f64 = 80.0;
const HIGH_SHELF_FREQUENCY: f64 = 8_000.0;
const SHELF_WIDTH: f64 = 0.7;
// Same headroom rule and limiter as the plain equalizer, so the player can
// read the headroom back (`apricot_playback::clipping_headroom_db`).
use apricot_playback::{CLIPPING_HEADROOM_LIMIT_DB, LIMITER_FILTER};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub struct BandShape {
    pub frequency: f64,
    pub q: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub struct Shelf {
    pub frequency: f64,
    pub q: f64,
    pub gain: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct AdvancedEqualizer {
    /// Only changed bands are stored; the others use their defaults.
    pub bands: BTreeMap<String, BandShape>,
    pub low_shelf: Shelf,
    pub high_shelf: Shelf,
}

impl Default for AdvancedEqualizer {
    fn default() -> Self {
        Self {
            bands: BTreeMap::new(),
            low_shelf: Shelf {
                frequency: LOW_SHELF_FREQUENCY,
                q: SHELF_WIDTH,
                gain: 0.0,
            },
            high_shelf: Shelf {
                frequency: HIGH_SHELF_FREQUENCY,
                q: SHELF_WIDTH,
                gain: 0.0,
            },
        }
    }
}

/// The frequency and width a band has without advanced changes.
#[must_use]
pub fn default_band(band_id: &str) -> Option<BandShape> {
    EQUALIZER_BANDS
        .iter()
        .find(|band| band.id == band_id)
        .map(|band| BandShape {
            frequency: f64::from(band.frequency_hz),
            q: apricot_playback::equalizer_band_width(band.id),
        })
}

fn in_range(value: f64, (minimum, maximum): (f64, f64)) -> bool {
    value.is_finite() && (minimum..=maximum).contains(&value)
}

fn same(first: f64, second: f64) -> bool {
    (first - second).abs() < 1e-6
}

/// Short number text for filter arguments: 31, 1.7, 0.707.
fn number(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl AdvancedEqualizer {
    /// The shape of one band, its default when it was never changed.
    #[must_use]
    pub fn band(&self, band_id: &str) -> Option<BandShape> {
        self.bands
            .get(band_id)
            .copied()
            .or_else(|| default_band(band_id))
    }

    /// Stores a band shape; a shape equal to the default is not stored.
    pub fn set_band(&mut self, band_id: &str, shape: BandShape) {
        if default_band(band_id).is_some_and(|default| {
            same(default.frequency, shape.frequency) && same(default.q, shape.q)
        }) {
            self.bands.remove(band_id);
        } else {
            self.bands.insert(band_id.to_owned(), shape);
        }
    }

    /// True when the profile plays exactly like the plain equalizer.
    #[must_use]
    pub fn is_default(&self) -> bool {
        let bands = self.bands.iter().all(|(id, shape)| {
            default_band(id).is_none_or(|default| {
                same(default.frequency, shape.frequency) && same(default.q, shape.q)
            })
        });
        bands && self.low_shelf.gain.abs() < 0.05 && self.high_shelf.gain.abs() < 0.05
    }

    /// Every value is finite and inside the editor's range.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.bands.iter().all(|(id, shape)| {
            default_band(id).is_some()
                && in_range(shape.frequency, FREQUENCY_RANGE)
                && in_range(shape.q, WIDTH_RANGE)
        }) && [self.low_shelf, self.high_shelf].iter().all(|shelf| {
            in_range(shelf.frequency, FREQUENCY_RANGE)
                && in_range(shelf.q, WIDTH_RANGE)
                && in_range(shelf.gain, SHELF_GAIN_RANGE)
        })
    }

    /// The `lavfi` graph for these settings and the slider gains, or `None`
    /// when nothing is audible. An unchanged profile returns the plain
    /// equalizer graph unchanged.
    #[must_use]
    pub fn graph(
        &self,
        gains: &BTreeMap<String, f64>,
        clipping_protection: bool,
    ) -> Option<String> {
        if self.is_default() || !self.is_valid() {
            return apricot_playback::equalizer_filter_graph(gains, clipping_protection);
        }
        let mut filters = Vec::new();
        let mut maximum = 0.0_f64;
        for band in EQUALIZER_BANDS {
            let gain = gains
                .get(band.id)
                .copied()
                .unwrap_or_default()
                .clamp(-24.0, 24.0);
            if gain.abs() < 0.05 {
                continue;
            }
            let Some(shape) = self.band(band.id) else {
                continue;
            };
            maximum = maximum.max(gain);
            filters.push(format!(
                "equalizer=f={}:t=q:w={}:g={gain:.1}",
                number(shape.frequency),
                number(shape.q)
            ));
        }
        for (filter, shelf) in [("lowshelf", self.low_shelf), ("highshelf", self.high_shelf)] {
            if shelf.gain.abs() < 0.05 {
                continue;
            }
            maximum = maximum.max(shelf.gain);
            filters.push(format!(
                "{filter}=f={}:t=q:w={}:g={:.1}",
                number(shelf.frequency),
                number(shelf.q),
                shelf.gain
            ));
        }
        if filters.is_empty() {
            return None;
        }
        if clipping_protection && maximum > 0.05 {
            let headroom = -maximum.clamp(0.0, CLIPPING_HEADROOM_LIMIT_DB);
            filters.insert(0, format!("volume={headroom:.1}dB"));
            filters.push(LIMITER_FILTER.to_owned());
        }
        Some(format!("lavfi=[{}]", filters.join(",")))
    }
}

/// Reads the advanced settings of every profile; damaged entries are skipped
/// so one bad profile cannot disable the equalizer.
#[must_use]
pub fn profiles_from_settings(
    settings: &apricot_storage::SettingsDocument,
) -> BTreeMap<String, AdvancedEqualizer> {
    settings
        .preserved
        .get(ADVANCED_SETTINGS_KEY)
        .and_then(serde_json::Value::as_object)
        .map(|profiles| {
            profiles
                .iter()
                .filter_map(|(id, value)| {
                    serde_json::from_value::<AdvancedEqualizer>(value.clone())
                        .ok()
                        .filter(AdvancedEqualizer::is_valid)
                        .map(|profile| (id.clone(), profile))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gains(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs
            .iter()
            .map(|(id, gain)| ((*id).to_owned(), *gain))
            .collect()
    }

    #[test]
    fn unchanged_profiles_keep_the_plain_equalizer_graph() {
        let gains = gains(&[("31", 4.0), ("1000", -3.0)]);
        let mut profile = AdvancedEqualizer::default();
        assert_eq!(
            profile.graph(&gains, true),
            apricot_playback::equalizer_filter_graph(&gains, true)
        );
        profile.set_band("31", default_band("31").unwrap());
        assert!(profile.bands.is_empty(), "a default shape is not stored");
        assert!(profile.is_default());
    }

    #[test]
    fn changed_bands_and_shelves_shape_the_graph_with_headroom() {
        let mut profile = AdvancedEqualizer::default();
        profile.set_band(
            "1000",
            BandShape {
                frequency: 1_200.0,
                q: 0.707,
            },
        );
        profile.high_shelf.gain = 6.0;
        let graph = profile
            .graph(&gains(&[("1000", 3.0)]), true)
            .expect("audible");
        assert_eq!(
            graph,
            "lavfi=[volume=-6.0dB,equalizer=f=1200:t=q:w=0.707:g=3.0,highshelf=f=8000:t=q:w=0.7:g=6.0,alimiter=limit=0.95:attack=5:release=80]"
        );
        assert!(profile.graph(&gains(&[]), false).is_some(), "shelf alone");
        profile.high_shelf.gain = 0.0;
        assert_eq!(profile.graph(&gains(&[]), false), None);
    }

    #[test]
    fn invalid_values_fall_back_to_the_plain_equalizer() {
        let mut profile = AdvancedEqualizer::default();
        profile.bands.insert(
            "62".to_owned(),
            BandShape {
                frequency: f64::NAN,
                q: 1.0,
            },
        );
        assert!(!profile.is_valid());
        let gains = gains(&[("62", 2.0)]);
        assert_eq!(
            profile.graph(&gains, false),
            apricot_playback::equalizer_filter_graph(&gains, false)
        );
    }

    #[test]
    fn damaged_saved_profiles_are_skipped() {
        let mut settings = apricot_storage::SettingsDocument::default();
        settings.preserved.insert(
            ADVANCED_SETTINGS_KEY.to_owned(),
            serde_json::json!({
                "rock": {"high_shelf": {"frequency": 9000.0, "q": 0.7, "gain": 2.0}},
                "custom1": {"bands": {"31": {"frequency": 5.0, "q": 1.0}}},
                "custom2": "broken"
            }),
        );
        let profiles = profiles_from_settings(&settings);
        assert_eq!(profiles.keys().collect::<Vec<_>>(), ["rock"]);
        assert!((profiles["rock"].high_shelf.frequency - 9_000.0).abs() < f64::EPSILON);
    }
}
