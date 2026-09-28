//! Player BPM announcement rules from Python `announce_bpm_async` and its
//! static helpers. Decoding and tempo estimation live in the platform and
//! media crates.

/// Python `bpm_analysis_state_key`: one analysis per item, speed and pitch.
pub fn bpm_analysis_state_key(item_key: &str, speed: f64, pitch: f64) -> String {
    format!("{item_key}\u{1f}{speed:.4}\u{1f}{pitch:.4}")
}

/// Python `effective_playback_bpm`: the source tempo at the playback speed.
#[allow(clippy::cast_possible_truncation)]
pub fn effective_playback_bpm(source_bpm: f64, playback_speed: f64) -> i64 {
    let speed = if playback_speed == 0.0 || playback_speed.is_nan() {
        1.0
    } else {
        playback_speed
    };
    (source_bpm * speed.clamp(0.25, 4.0)).round_ties_even() as i64
}

/// Python `bpm_analysis_window`: up to 72 seconds starting 18 seconds before
/// the playback position, moved back to fit inside a known duration.
pub fn bpm_analysis_window(position: f64, duration: Option<f64>) -> (f64, f64) {
    const MAXIMUM_SECONDS: f64 = 72.0;
    let position = position.max(0.0);
    let available_duration = duration.unwrap_or(0.0).max(0.0);
    let start = (position - 18.0).max(0.0);
    if available_duration > 0.0 {
        let length = MAXIMUM_SECONDS.min(available_duration);
        return (start.min((available_duration - length).max(0.0)), length);
    }
    (0.0, MAXIMUM_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::{bpm_analysis_state_key, bpm_analysis_window, effective_playback_bpm};

    #[test]
    fn state_key_matches_python_format() {
        assert_eq!(
            bpm_analysis_state_key("https://youtu.be/x", 1.25, 1.0),
            "https://youtu.be/x\u{1f}1.2500\u{1f}1.0000"
        );
    }

    #[test]
    fn effective_bpm_follows_speed_like_python() {
        assert_eq!(effective_playback_bpm(120.0, 1.0), 120);
        assert_eq!(effective_playback_bpm(120.0, 1.5), 180);
        assert_eq!(effective_playback_bpm(120.0, 0.0), 120);
        assert_eq!(effective_playback_bpm(120.0, 10.0), 480);
        assert_eq!(effective_playback_bpm(100.0, 0.1), 25);
        // Python `round` sends halves to the even neighbour.
        assert_eq!(effective_playback_bpm(122.5, 1.0), 122);
        assert_eq!(effective_playback_bpm(123.5, 1.0), 124);
    }

    #[test]
    fn analysis_window_matches_python() {
        assert_eq!(bpm_analysis_window(100.0, Some(300.0)), (82.0, 72.0));
        assert_eq!(bpm_analysis_window(5.0, Some(300.0)), (0.0, 72.0));
        assert_eq!(bpm_analysis_window(290.0, Some(300.0)), (228.0, 72.0));
        assert_eq!(bpm_analysis_window(20.0, Some(40.0)), (0.0, 40.0));
        assert_eq!(bpm_analysis_window(500.0, None), (0.0, 72.0));
        assert_eq!(bpm_analysis_window(-3.0, Some(0.0)), (0.0, 72.0));
    }
}
