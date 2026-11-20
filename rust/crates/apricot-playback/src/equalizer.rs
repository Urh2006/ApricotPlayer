use std::collections::BTreeMap;

use apricot_core::audio::EQUALIZER_BANDS;

/// Python `EQ_FILTER_LABEL`.
pub const EQUALIZER_FILTER_LABEL: &str = "apricot_eq";
/// Python `EQ_FILTER_ALT_LABEL`, used while replacing a running equalizer.
pub const EQUALIZER_FILTER_ALT_LABEL: &str = "apricot_eq_next";
/// Python `EQ_LIMITER_FILTER`.
pub const LIMITER_FILTER: &str = "alimiter=limit=0.95:attack=5:release=80";
/// Python `EQ_CLIPPING_HEADROOM_LIMIT_DB`.
pub const CLIPPING_HEADROOM_LIMIT_DB: f64 = 12.0;

/// Python `equalizer_filter` without its `@label:` prefix: the `lavfi` graph
/// for the given gains, or `None` when no band is audible.
pub fn equalizer_filter_graph(
    gains: &BTreeMap<String, f64>,
    clipping_protection: bool,
) -> Option<String> {
    let filters = equalizer_filters(gains, clipping_protection);
    (!filters.is_empty()).then(|| format!("lavfi=[{}]", filters.join(",")))
}

/// Python `ffmpeg_equalizer_filters`: the separate filters of the graph, empty
/// when no band is audible. As in Python `equalizer_clipping_protection_active`,
/// protection only applies when a band boosts: it adds headroom for the largest
/// positive gain and a limiter.
pub fn equalizer_filters(gains: &BTreeMap<String, f64>, clipping_protection: bool) -> Vec<String> {
    let maximum = EQUALIZER_BANDS
        .iter()
        .filter_map(|band| gains.get(band.id).copied())
        .fold(0.0_f64, f64::max);
    let clipping_protection = clipping_protection && maximum > 0.05;
    let mut filters = Vec::new();
    if clipping_protection {
        let headroom = -maximum.clamp(0.0, CLIPPING_HEADROOM_LIMIT_DB);
        if headroom <= -0.05 {
            filters.push(format!("volume={headroom:.1}dB"));
        }
    }
    let mut audible = false;
    for band in EQUALIZER_BANDS {
        let gain = gains
            .get(band.id)
            .copied()
            .unwrap_or_default()
            .clamp(-24.0, 24.0);
        if gain.abs() < 0.05 {
            continue;
        }
        audible = true;
        filters.push(format!(
            "equalizer=f={}:t=q:w={}:g={gain:.1}",
            band.frequency_hz,
            equalizer_band_width(band.id)
        ));
    }
    if !audible {
        return Vec::new();
    }
    if clipping_protection {
        filters.push(LIMITER_FILTER.to_owned());
    }
    filters
}

/// The headroom in dB that clipping protection takes in the equalizer of a
/// tagged filter or a whole `af` chain, `None` when that equalizer is
/// unprotected or missing. A protected graph starts with its `volume=-XdB`
/// headroom and ends with [`LIMITER_FILTER`], as both graph builders write it.
#[must_use]
pub fn clipping_headroom_db(filters: &str, label: &str) -> Option<f64> {
    let start = filters.find(&format!("@{label}:lavfi=["))?;
    if start > 0 && !filters[..start].ends_with(',') {
        return None;
    }
    let graph = &filters[start + label.len() + 2..];
    let graph = &graph[..=graph.find(']')?];
    let headroom = graph.strip_prefix("lavfi=[volume=-")?;
    let headroom: f64 = headroom[..headroom.find("dB,")?].parse().ok()?;
    (graph.ends_with(&format!(",{LIMITER_FILTER}]")) && headroom.is_finite() && headroom > 0.0)
        .then(|| headroom.min(CLIPPING_HEADROOM_LIMIT_DB))
}

/// The mpv `volume` that plays `volume` through an equalizer whose clipping
/// protection took `headroom_db`. mpv applies its volume after the filters
/// and as a cube, (volume / 100)^3, so at a lower volume the boosted bands
/// cannot reach full scale and the headroom only makes everything quieter.
/// Below 100 the volume therefore gets back as much of the headroom as still
/// keeps the output at or below full scale: the result is as loud as without
/// protection until that limit, and never louder than it. From 100 up, with
/// volume boost, the volume is unchanged, as before 2.1.2.
#[must_use]
pub fn protected_volume(volume: f64, headroom_db: f64) -> f64 {
    if !volume.is_finite() || volume >= 100.0 || headroom_db <= 0.0 {
        return volume;
    }
    let headroom_db = headroom_db.min(CLIPPING_HEADROOM_LIMIT_DB);
    (volume.max(0.0) * 10_f64.powf(headroom_db / 60.0)).min(100.0)
}

/// Tagged filter string for `af add` and the initial `af` chain.
pub fn tagged_equalizer_filter(label: &str, graph: &str) -> String {
    format!("@{label}:{graph}")
}

/// Python `EQ_BANDS` filter width (Q) of a band.
#[must_use]
pub fn equalizer_band_width(id: &str) -> f64 {
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

    use super::{
        EQUALIZER_FILTER_ALT_LABEL, EQUALIZER_FILTER_LABEL, clipping_headroom_db,
        equalizer_filter_graph, equalizer_filters, protected_volume, tagged_equalizer_filter,
    };

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

        let graph = equalizer_filter_graph(&gains, false).expect("audible equalizer");
        let filter = tagged_equalizer_filter(EQUALIZER_FILTER_LABEL, &graph);

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
        assert!(filter.starts_with("@apricot_eq:lavfi=[equalizer=f=31:"));
        assert_eq!(filter.matches("equalizer=").count(), 10);
    }

    #[test]
    fn clipping_uses_largest_positive_gain_and_limiter() {
        let gains = BTreeMap::from([
            ("31".to_owned(), 18.0),
            ("1000".to_owned(), 4.0),
            ("16000".to_owned(), -3.0),
        ]);
        let graph = equalizer_filter_graph(&gains, true).expect("protected equalizer");

        assert!(graph.starts_with("lavfi=[volume=-12.0dB,"));
        assert!(graph.ends_with("alimiter=limit=0.95:attack=5:release=80]"));
    }

    #[test]
    fn clipping_protection_is_inactive_without_a_boosted_band() {
        let gains = BTreeMap::from([("250".to_owned(), -4.0)]);
        assert_eq!(
            equalizer_filter_graph(&gains, true).as_deref(),
            Some("lavfi=[equalizer=f=250:t=q:w=2.3:g=-4.0]")
        );
    }

    #[test]
    fn flat_gains_have_no_filter() {
        assert_eq!(equalizer_filter_graph(&BTreeMap::new(), true), None);
        let tiny = BTreeMap::from([("31".to_owned(), 0.04)]);
        assert_eq!(equalizer_filter_graph(&tiny, false), None);
        assert!(equalizer_filters(&tiny, true).is_empty());
    }

    #[test]
    fn separate_filters_match_python_ffmpeg_equalizer_filters() {
        let gains = BTreeMap::from([("62".to_owned(), 6.0), ("8000".to_owned(), -2.0)]);
        assert_eq!(
            equalizer_filters(&gains, true),
            [
                "volume=-6.0dB",
                "equalizer=f=62:t=q:w=2:g=6.0",
                "equalizer=f=8000:t=q:w=1.7:g=-2.0",
                "alimiter=limit=0.95:attack=5:release=80",
            ]
        );
    }

    #[test]
    fn headroom_is_read_back_from_a_protected_graph_or_chain() {
        let gains = BTreeMap::from([("62".to_owned(), 6.0), ("8000".to_owned(), -2.0)]);
        let protected = equalizer_filter_graph(&gains, true).expect("protected");
        let plain = equalizer_filter_graph(&gains, false).expect("plain");
        let tagged = tagged_equalizer_filter(EQUALIZER_FILTER_LABEL, &protected);
        assert_eq!(
            clipping_headroom_db(&tagged, EQUALIZER_FILTER_LABEL),
            Some(6.0)
        );
        assert_eq!(
            clipping_headroom_db(&tagged, EQUALIZER_FILTER_ALT_LABEL),
            None
        );
        let next = tagged_equalizer_filter(EQUALIZER_FILTER_ALT_LABEL, &protected);
        assert_eq!(
            clipping_headroom_db(&next, EQUALIZER_FILTER_ALT_LABEL),
            Some(6.0)
        );
        assert_eq!(clipping_headroom_db(&next, EQUALIZER_FILTER_LABEL), None);
        let chain =
            format!("@apricot_speed:scaletempo2,{tagged},@apricot_processing:lavfi=[pan=mono]");
        assert_eq!(
            clipping_headroom_db(&chain, EQUALIZER_FILTER_LABEL),
            Some(6.0)
        );
        let unprotected = tagged_equalizer_filter(EQUALIZER_FILTER_LABEL, &plain);
        assert_eq!(
            clipping_headroom_db(&unprotected, EQUALIZER_FILTER_LABEL),
            None
        );
        // A user filter that only looks similar is not the equalizer.
        assert_eq!(
            clipping_headroom_db(
                "@x@apricot_eq:lavfi=[volume=-6dB,alimiter=limit=0.95:attack=5:release=80]",
                EQUALIZER_FILTER_LABEL
            ),
            None
        );
    }

    #[test]
    fn protected_volume_keeps_the_sound_until_full_scale() {
        // +6 dB boost: 50 % gets the whole headroom back, the output is as
        // loud as without protection: (63 / 100)^3 * 10^(-6 / 20) = (50 / 100)^3.
        let restored = protected_volume(50.0, 6.0);
        assert!((restored - 50.0 * 10_f64.powf(0.1)).abs() < 1e-9);
        let without = (0.5_f64).powi(3);
        let with = (restored / 100.0).powi(3) * 10_f64.powf(-6.0 / 20.0);
        assert!((without - with).abs() < 1e-12);
        // From about 79 % up the output stays at full scale.
        assert!((protected_volume(80.0, 6.0) - 100.0).abs() < 1e-9);
        assert!((protected_volume(99.0, 6.0) - 100.0).abs() < 1e-9);
        // 100 % and volume boost keep their old behaviour.
        assert!((protected_volume(100.0, 6.0) - 100.0).abs() < 1e-9);
        assert!((protected_volume(200.0, 6.0) - 200.0).abs() < 1e-9);
        assert!(protected_volume(0.0, 6.0).abs() < 1e-9);
        assert!((protected_volume(40.0, 0.0) - 40.0).abs() < 1e-9);
        // The headroom never exceeds the 12 dB limit.
        assert!((protected_volume(10.0, 30.0) - protected_volume(10.0, 12.0)).abs() < 1e-9);
    }
}
