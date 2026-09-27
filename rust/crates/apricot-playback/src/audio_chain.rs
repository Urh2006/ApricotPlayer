//! Speed and pitch audio policy matching Python `speed_audio_filter_args`,
//! `apply_pitch_value` and `rubberband_pitch_filter`.

/// Label of the time-stretch filter selected by `speed_audio_mode`.
pub const SPEED_FILTER_LABEL: &str = "apricot_speed";
/// Label of the Rubberband pitch filter used by the Rubberband and linked pitch modes.
pub const PITCH_FILTER_LABEL: &str = "apricot_pitch";

/// Python `SPEED_AUDIO_MODE_*` values stored in `settings.json`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeedAudioMode {
    Rubberband,
    Scaletempo2,
    Mpv,
    Scaletempo,
}

impl SpeedAudioMode {
    /// Maps a normalized settings value; unknown values use Python's
    /// scaletempo2 fallback branch.
    pub fn from_setting(value: &str) -> Self {
        match value.trim() {
            "Rubberband high quality" => Self::Rubberband,
            "mpv default scaletempo2" => Self::Mpv,
            "Classic scaletempo" => Self::Scaletempo,
            _ => Self::Scaletempo2,
        }
    }

    /// Value of mpv `audio-pitch-correction` for this mode.
    pub const fn audio_pitch_correction(self) -> bool {
        matches!(self, Self::Mpv | Self::Rubberband)
    }

    /// Tagged time-stretch filter, or `None` when mpv inserts its own.
    pub const fn filter(self) -> Option<&'static str> {
        match self {
            Self::Mpv => None,
            Self::Scaletempo => Some("@apricot_speed:scaletempo=stride=30:overlap=.50:search=10"),
            Self::Rubberband => Some(
                "@apricot_speed:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer",
            ),
            Self::Scaletempo2 => {
                Some("@apricot_speed:scaletempo2=search-interval=50:window-size=20:max-speed=8.0")
            }
        }
    }
}

/// Python `PITCH_MODE_*` values stored in `settings.json`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PitchMode {
    Mpv,
    Rubberband,
    LinkedSpeed,
}

impl PitchMode {
    pub fn from_setting(value: &str) -> Self {
        match value.trim() {
            "Independent pitch - advanced (Rubberband)" => Self::Rubberband,
            "Linked pitch and speed - pitch keys change both" => Self::LinkedSpeed,
            _ => Self::Mpv,
        }
    }

    /// Whether pitch is applied through mpv's own `pitch` property.
    pub const fn uses_mpv_pitch(self) -> bool {
        matches!(self, Self::Mpv)
    }
}

/// Python `is_default_rate`.
pub fn is_default_rate(value: f64) -> bool {
    (value - 1.0).abs() < 0.001
}

/// Python `rubberband_pitch_filter`.
pub fn rubberband_pitch_filter(pitch: f64) -> String {
    format!(
        "@{PITCH_FILTER_LABEL}:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer:pitch-scale={pitch:.4}"
    )
}

/// Value sent to mpv's `pitch` property for the given mode.
pub fn mpv_pitch_property(mode: PitchMode, pitch: f64) -> f64 {
    if mode.uses_mpv_pitch() { pitch } else { 1.0 }
}

/// Whether the Rubberband pitch filter is part of the chain.
pub fn pitch_filter_active(mode: PitchMode, pitch: f64) -> bool {
    !mode.uses_mpv_pitch() && !is_default_rate(pitch)
}

/// Complete tagged audio filter chain in the order Python ends up with:
/// time-stretch filter, equalizer, then the Rubberband pitch filter.
pub fn audio_filter_chain(
    speed_mode: SpeedAudioMode,
    equalizer: Option<&str>,
    pitch_mode: PitchMode,
    pitch: f64,
) -> Option<String> {
    let mut filters: Vec<String> = Vec::new();
    if let Some(filter) = speed_mode.filter() {
        filters.push(filter.to_owned());
    }
    if let Some(filter) = equalizer.filter(|filter| !filter.is_empty()) {
        filters.push(filter.to_owned());
    }
    if pitch_filter_active(pitch_mode, pitch) {
        filters.push(rubberband_pitch_filter(pitch));
    }
    (!filters.is_empty()).then(|| filters.join(","))
}

#[cfg(test)]
mod tests {
    use super::{
        PitchMode, SpeedAudioMode, audio_filter_chain, mpv_pitch_property, rubberband_pitch_filter,
    };

    #[test]
    fn speed_modes_match_python_arguments() {
        let rubberband = SpeedAudioMode::from_setting("Rubberband high quality");
        assert!(rubberband.audio_pitch_correction());
        assert_eq!(
            rubberband.filter(),
            Some(
                "@apricot_speed:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer"
            )
        );
        let mpv = SpeedAudioMode::from_setting("mpv default scaletempo2");
        assert!(mpv.audio_pitch_correction());
        assert_eq!(mpv.filter(), None);
        let classic = SpeedAudioMode::from_setting("Classic scaletempo");
        assert!(!classic.audio_pitch_correction());
        assert_eq!(
            classic.filter(),
            Some("@apricot_speed:scaletempo=stride=30:overlap=.50:search=10")
        );
        let scaletempo2 = SpeedAudioMode::from_setting("High quality scaletempo2");
        assert!(!scaletempo2.audio_pitch_correction());
        assert_eq!(
            scaletempo2.filter(),
            Some("@apricot_speed:scaletempo2=search-interval=50:window-size=20:max-speed=8.0")
        );
    }

    #[test]
    fn rubberband_pitch_filter_matches_python_format() {
        assert_eq!(
            rubberband_pitch_filter(1.05),
            "@apricot_pitch:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer:pitch-scale=1.0500"
        );
    }

    #[test]
    fn chain_orders_speed_equalizer_and_pitch_filters() {
        let rubberband_pitch = PitchMode::from_setting("Independent pitch - advanced (Rubberband)");
        let chain = audio_filter_chain(
            SpeedAudioMode::Scaletempo,
            Some("@apricot_eq:lavfi=[equalizer=f=31:t=q:w=1.7:g=3.0]"),
            rubberband_pitch,
            0.9,
        )
        .expect("chain");
        assert_eq!(
            chain,
            "@apricot_speed:scaletempo=stride=30:overlap=.50:search=10,@apricot_eq:lavfi=[equalizer=f=31:t=q:w=1.7:g=3.0],@apricot_pitch:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer:pitch-scale=0.9000"
        );
        assert!((mpv_pitch_property(rubberband_pitch, 0.9) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn default_pitch_and_mpv_mode_add_no_pitch_filter() {
        let mpv_pitch =
            PitchMode::from_setting("Independent pitch - highest quality (mpv built-in)");
        assert_eq!(
            audio_filter_chain(SpeedAudioMode::Mpv, None, mpv_pitch, 1.2),
            None
        );
        assert!((mpv_pitch_property(mpv_pitch, 1.2) - 1.2).abs() < f64::EPSILON);
        let linked = PitchMode::from_setting("Linked pitch and speed - pitch keys change both");
        assert_eq!(
            audio_filter_chain(SpeedAudioMode::Mpv, None, linked, 1.0),
            None
        );
    }
}
