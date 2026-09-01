//! Equalizer identities and factory values preserved from Python 1.0.21.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqualizerBand {
    pub id: &'static str,
    pub frequency_hz: u32,
}

pub const EQUALIZER_BANDS: &[EqualizerBand] = &[
    EqualizerBand {
        id: "31",
        frequency_hz: 31,
    },
    EqualizerBand {
        id: "62",
        frequency_hz: 62,
    },
    EqualizerBand {
        id: "125",
        frequency_hz: 125,
    },
    EqualizerBand {
        id: "250",
        frequency_hz: 250,
    },
    EqualizerBand {
        id: "500",
        frequency_hz: 500,
    },
    EqualizerBand {
        id: "1000",
        frequency_hz: 1_000,
    },
    EqualizerBand {
        id: "2000",
        frequency_hz: 2_000,
    },
    EqualizerBand {
        id: "4000",
        frequency_hz: 4_000,
    },
    EqualizerBand {
        id: "8000",
        frequency_hz: 8_000,
    },
    EqualizerBand {
        id: "16000",
        frequency_hz: 16_000,
    },
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FactoryEqualizerPreset {
    pub id: &'static str,
    pub gains_db: [f32; 10],
}

pub const FACTORY_EQUALIZER_PRESETS: &[FactoryEqualizerPreset] = &[
    FactoryEqualizerPreset {
        id: "flat",
        gains_db: [0.0; 10],
    },
    FactoryEqualizerPreset {
        id: "bass_boost",
        gains_db: [5.0, 4.0, 3.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    FactoryEqualizerPreset {
        id: "full_bass_treble",
        gains_db: [4.0, 3.0, 0.0, -4.0, -2.0, 1.0, 4.0, 5.0, 6.0, 6.0],
    },
    FactoryEqualizerPreset {
        id: "dance",
        gains_db: [5.0, 4.0, 2.0, 0.0, -1.0, -1.0, 0.0, 2.0, 3.0, 3.0],
    },
    FactoryEqualizerPreset {
        id: "hip_hop",
        gains_db: [4.0, 5.0, 3.0, 1.0, -1.0, -1.0, 1.0, 2.0, 2.0, 1.0],
    },
    FactoryEqualizerPreset {
        id: "electronic",
        gains_db: [4.0, 3.0, 0.0, -3.0, -2.0, 0.0, 4.0, 5.0, 5.0, 4.0],
    },
    FactoryEqualizerPreset {
        id: "rock",
        gains_db: [4.0, 3.0, 1.0, -2.0, -3.0, -1.0, 2.0, 4.0, 5.0, 5.0],
    },
    FactoryEqualizerPreset {
        id: "pop",
        gains_db: [-1.0, 2.0, 3.0, 4.0, 2.0, 0.0, -1.0, -1.0, 0.0, 1.0],
    },
    FactoryEqualizerPreset {
        id: "classical",
        gains_db: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -2.0, -2.0, -2.0, -3.0],
    },
    FactoryEqualizerPreset {
        id: "jazz",
        gains_db: [2.0, 1.0, 0.0, 1.0, 2.0, 2.0, 1.0, 2.0, 3.0, 2.0],
    },
    FactoryEqualizerPreset {
        id: "acoustic",
        gains_db: [2.0, 3.0, 2.0, 1.0, 0.0, 1.0, 2.0, 3.0, 2.0, 1.0],
    },
    FactoryEqualizerPreset {
        id: "vocal",
        gains_db: [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 3.0, 1.0, -1.0],
    },
    FactoryEqualizerPreset {
        id: "podcast",
        gains_db: [-3.0, -2.0, -1.0, 0.0, 2.0, 3.0, 4.0, 3.0, 0.0, -2.0],
    },
    FactoryEqualizerPreset {
        id: "bright",
        gains_db: [-1.0, -1.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 5.0],
    },
    FactoryEqualizerPreset {
        id: "mellow",
        gains_db: [1.0, 2.0, 1.0, 0.0, -1.0, -1.0, -1.0, -2.0, -3.0, -3.0],
    },
    FactoryEqualizerPreset {
        id: "treble_boost",
        gains_db: [-5.0, -4.0, -3.0, -1.0, 1.0, 3.0, 5.0, 6.0, 6.0, 6.0],
    },
    FactoryEqualizerPreset {
        id: "laptop_headphones",
        gains_db: [3.0, 5.0, 3.0, -2.0, 0.0, -3.0, -4.0, -4.0, 0.0, 0.0],
    },
    FactoryEqualizerPreset {
        id: "late_night",
        gains_db: [2.0, 2.0, 1.0, 0.0, -1.0, -2.0, -1.0, 0.0, 1.0, 1.0],
    },
];

pub const CUSTOM_EQUALIZER_PRESET_IDS: &[&str] = &["custom1", "custom2", "custom3"];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{CUSTOM_EQUALIZER_PRESET_IDS, EQUALIZER_BANDS, FACTORY_EQUALIZER_PRESETS};

    #[test]
    fn equalizer_contract_has_independent_bands_and_all_presets() {
        let band_ids: HashSet<_> = EQUALIZER_BANDS.iter().map(|band| band.id).collect();
        let preset_ids: HashSet<_> = FACTORY_EQUALIZER_PRESETS
            .iter()
            .map(|preset| preset.id)
            .collect();
        assert_eq!(EQUALIZER_BANDS.len(), 10);
        assert_eq!(band_ids.len(), 10);
        assert_eq!(FACTORY_EQUALIZER_PRESETS.len(), 18);
        assert_eq!(preset_ids.len(), 18);
        assert_eq!(CUSTOM_EQUALIZER_PRESET_IDS.len(), 3);
    }
}
