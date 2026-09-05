use std::collections::BTreeMap;

use apricot_core::audio::{EQUALIZER_BANDS, FACTORY_EQUALIZER_PRESETS};

#[derive(Clone, Copy, Debug)]
pub struct EqualizerFilterConfig<'a> {
    pub gains: &'a BTreeMap<String, f64>,
    pub equalizer_enabled: bool,
    pub bass_boost: bool,
    pub clipping_protection: bool,
}

/// Builds Apricot's tagged mpv audio filter without mutating the stored gains.
pub fn build_equalizer_filter(config: EqualizerFilterConfig<'_>) -> Option<String> {
    let mut gains = config.gains.clone();
    if config.bass_boost {
        let preset = FACTORY_EQUALIZER_PRESETS
            .iter()
            .find(|preset| preset.id == "bass_boost")?;
        for (band, boost) in EQUALIZER_BANDS.iter().zip(preset.gains_db) {
            let gain = gains.entry(band.id.to_owned()).or_default();
            *gain = (*gain + f64::from(boost)).clamp(-24.0, 24.0);
        }
    }
    if !config.equalizer_enabled && !config.bass_boost {
        return None;
    }

    let has_positive = gains.values().any(|gain| *gain > 0.05);
    let protect = config.clipping_protection && has_positive;
    let mut filters = Vec::new();
    if protect {
        let maximum = gains.values().copied().fold(0.0_f64, f64::max);
        if maximum > 0.05 {
            filters.push(format!("volume={:.1}dB", -maximum.min(12.0)));
        }
    }
    for band in EQUALIZER_BANDS {
        let gain = gains
            .get(band.id)
            .copied()
            .unwrap_or_default()
            .clamp(-24.0, 24.0);
        if gain.abs() < 0.05 {
            continue;
        }
        filters.push(format!(
            "equalizer=f={}:t=q:w={}:g={gain:.1}",
            band.frequency_hz,
            band_width(band.id)
        ));
    }
    if protect && !filters.is_empty() {
        filters.push("alimiter=limit=0.95:attack=5:release=80".to_owned());
    }
    (!filters.is_empty()).then(|| format!("@apricot_eq:lavfi=[{}]", filters.join(",")))
}

fn band_width(id: &str) -> f64 {
    match id {
        "31" | "8000" => 1.7,
        "62" | "1000" => 2.0,
        "125" => 2.6,
        "250" => 2.3,
        "500" => 2.1,
        "2000" | "4000" => 1.9,
        "16000" => 1.5,
        _ => 1.8,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{EqualizerFilterConfig, build_equalizer_filter};

    #[test]
    fn all_ten_bands_keep_independent_values_and_widths() {
        let gains = BTreeMap::from([
            ("31".to_owned(), 1.0),
            ("62".to_owned(), 2.0),
            ("125".to_owned(), 3.0),
            ("250".to_owned(), 4.0),
            ("500".to_owned(), 5.0),
            ("1000".to_owned(), 6.0),
            ("2000".to_owned(), 7.0),
            ("4000".to_owned(), 8.0),
            ("8000".to_owned(), 9.0),
            ("16000".to_owned(), 10.0),
        ]);

        let filter = build_equalizer_filter(EqualizerFilterConfig {
            gains: &gains,
            equalizer_enabled: true,
            bass_boost: false,
            clipping_protection: false,
        })
        .expect("enabled equalizer filter");

        let expected = [
            "f=31:t=q:w=1.7:g=1.0",
            "f=62:t=q:w=2:g=2.0",
            "f=125:t=q:w=2.6:g=3.0",
            "f=250:t=q:w=2.3:g=4.0",
            "f=500:t=q:w=2.1:g=5.0",
            "f=1000:t=q:w=2:g=6.0",
            "f=2000:t=q:w=1.9:g=7.0",
            "f=4000:t=q:w=1.9:g=8.0",
            "f=8000:t=q:w=1.7:g=9.0",
            "f=16000:t=q:w=1.5:g=10.0",
        ];
        for fragment in expected {
            assert!(filter.contains(fragment), "missing {fragment}: {filter}");
        }
        assert_eq!(filter.matches("equalizer=").count(), 10);
        assert_eq!(gains.get("31"), Some(&1.0));
        assert_eq!(gains.get("16000"), Some(&10.0));
    }

    #[test]
    fn clipping_uses_largest_positive_gain_and_limiter() {
        let gains = BTreeMap::from([
            ("31".to_owned(), 18.0),
            ("1000".to_owned(), 4.0),
            ("16000".to_owned(), -3.0),
        ]);
        let filter = build_equalizer_filter(EqualizerFilterConfig {
            gains: &gains,
            equalizer_enabled: true,
            bass_boost: false,
            clipping_protection: true,
        })
        .expect("protected equalizer filter");

        assert!(filter.starts_with("@apricot_eq:lavfi=[volume=-12.0dB,"));
        assert!(filter.ends_with("alimiter=limit=0.95:attack=5:release=80]"));
    }

    #[test]
    fn bass_boost_is_added_once_without_changing_input_gains() {
        let gains = BTreeMap::from([("31".to_owned(), 2.0), ("62".to_owned(), -1.0)]);
        let filter = build_equalizer_filter(EqualizerFilterConfig {
            gains: &gains,
            equalizer_enabled: false,
            bass_boost: true,
            clipping_protection: false,
        })
        .expect("bass boost filter");

        assert!(filter.contains("f=31:t=q:w=1.7:g=7.0"));
        assert!(filter.contains("f=62:t=q:w=2:g=3.0"));
        assert_eq!(gains.get("31"), Some(&2.0));
        assert_eq!(gains.get("62"), Some(&-1.0));
    }

    #[test]
    fn disabled_flat_equalizer_has_no_filter() {
        assert_eq!(
            build_equalizer_filter(EqualizerFilterConfig {
                gains: &BTreeMap::new(),
                equalizer_enabled: false,
                bass_boost: false,
                clipping_protection: true,
            }),
            None
        );
    }
}
