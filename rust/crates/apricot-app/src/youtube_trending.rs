//! Typed country and category choices for the Python-compatible Trending screen.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YoutubeTrendingChoice {
    pub code: &'static str,
    pub label_key: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YoutubeTrendingWork {
    pub generation: u64,
    pub country_code: &'static str,
    pub category_code: &'static str,
    pub limit: u32,
}

pub const YOUTUBE_TRENDING_COUNTRIES: &[YoutubeTrendingChoice] = &[
    choice("global", "Global"),
    choice("AR", "Argentina"),
    choice("AU", "Australia"),
    choice("AT", "Austria"),
    choice("BE", "Belgium"),
    choice("BR", "Brazil"),
    choice("CA", "Canada"),
    choice("CL", "Chile"),
    choice("CO", "Colombia"),
    choice("CZ", "Czechia"),
    choice("DK", "Denmark"),
    choice("EG", "Egypt"),
    choice("FI", "Finland"),
    choice("FR", "France"),
    choice("DE", "Germany"),
    choice("GR", "Greece"),
    choice("HK", "Hong Kong"),
    choice("HU", "Hungary"),
    choice("IN", "India"),
    choice("ID", "Indonesia"),
    choice("IE", "Ireland"),
    choice("IL", "Israel"),
    choice("IT", "Italy"),
    choice("JP", "Japan"),
    choice("KE", "Kenya"),
    choice("MY", "Malaysia"),
    choice("MX", "Mexico"),
    choice("NL", "Netherlands"),
    choice("NZ", "New Zealand"),
    choice("NG", "Nigeria"),
    choice("NO", "Norway"),
    choice("PK", "Pakistan"),
    choice("PE", "Peru"),
    choice("PH", "Philippines"),
    choice("PL", "Poland"),
    choice("PT", "Portugal"),
    choice("RO", "Romania"),
    choice("RU", "Russia"),
    choice("SA", "Saudi Arabia"),
    choice("RS", "Serbia"),
    choice("SG", "Singapore"),
    choice("SK", "Slovakia"),
    choice("SI", "Slovenia"),
    choice("ZA", "South Africa"),
    choice("KR", "South Korea"),
    choice("ES", "Spain"),
    choice("SE", "Sweden"),
    choice("CH", "Switzerland"),
    choice("TW", "Taiwan"),
    choice("TH", "Thailand"),
    choice("TR", "Turkey"),
    choice("UA", "Ukraine"),
    choice("AE", "United Arab Emirates"),
    choice("GB", "United Kingdom"),
    choice("US", "United States"),
    choice("VN", "Vietnam"),
];

pub const YOUTUBE_TRENDING_CATEGORIES: &[YoutubeTrendingChoice] = &[
    choice("all", "trending_all"),
    choice("music", "trending_music"),
    choice("movies", "trending_movies"),
    choice("gaming", "trending_gaming"),
    choice("sports", "trending_sports"),
    choice("news", "trending_news"),
    choice("entertainment", "trending_entertainment"),
    choice("comedy", "trending_comedy"),
    choice("technology", "trending_technology"),
];

const fn choice(code: &'static str, label_key: &'static str) -> YoutubeTrendingChoice {
    YoutubeTrendingChoice { code, label_key }
}

pub const fn category_id(code: &str) -> Option<&'static str> {
    match code.as_bytes() {
        b"music" => Some("10"),
        b"movies" => Some("1"),
        b"gaming" => Some("20"),
        b"sports" => Some("17"),
        b"news" => Some("25"),
        b"entertainment" => Some("24"),
        b"comedy" => Some("23"),
        b"technology" => Some("28"),
        _ => None,
    }
}

pub fn public_url(country_code: &str, category_code: &str) -> Option<String> {
    match category_code {
        "music" => Some(format!(
            "https://charts.youtube.com/charts/TrendingVideos/{}/right_now",
            country_code.to_ascii_lowercase()
        )),
        "gaming" => Some(format!(
            "https://www.youtube.com/gaming?gl={}",
            if country_code == "global" {
                "US"
            } else {
                country_code
            }
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{YOUTUBE_TRENDING_CATEGORIES, YOUTUBE_TRENDING_COUNTRIES, category_id, public_url};

    #[test]
    fn choices_match_the_python_catalog_without_duplicates() {
        assert_eq!(YOUTUBE_TRENDING_COUNTRIES.len(), 56);
        assert_eq!(YOUTUBE_TRENDING_CATEGORIES.len(), 9);
        assert_eq!(
            YOUTUBE_TRENDING_COUNTRIES
                .iter()
                .map(|choice| choice.code)
                .collect::<HashSet<_>>()
                .len(),
            YOUTUBE_TRENDING_COUNTRIES.len()
        );
        assert_eq!(category_id("music"), Some("10"));
        assert_eq!(category_id("all"), None);
    }

    #[test]
    fn public_fallbacks_are_real_feeds_and_never_fake_searches() {
        assert!(public_url("global", "all").is_none());
        assert!(public_url("SI", "music").is_some_and(|url| url.contains("/si/right_now")));
        assert!(public_url("SI", "sports").is_none());
    }
}
