//! Shipped language identities and platform-neutral translation lookup.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// 2.1: short announcements, Settings, General, "Announcements". A key
/// with a `_short` variant is spoken in that shorter form while it is on.
static SHORT_ANNOUNCEMENTS: AtomicBool = AtomicBool::new(false);

/// Turns the short announcements on or off for every catalog.
pub fn set_short_announcements(short: bool) {
    SHORT_ANNOUNCEMENTS.store(short, Ordering::Relaxed);
}

/// Whether announcements use their short forms.
pub fn short_announcements() -> bool {
    SHORT_ANNOUNCEMENTS.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LanguageDefinition {
    pub code: &'static str,
    pub name: &'static str,
}

pub const LANGUAGES: &[LanguageDefinition] = &[
    LanguageDefinition {
        code: "en",
        name: "English",
    },
    LanguageDefinition {
        code: "sl",
        name: "Slovenščina",
    },
    LanguageDefinition {
        code: "de",
        name: "Deutsch",
    },
    LanguageDefinition {
        code: "fr",
        name: "Français",
    },
    LanguageDefinition {
        code: "es",
        name: "Español",
    },
    LanguageDefinition {
        code: "pt",
        name: "Português",
    },
    LanguageDefinition {
        code: "it",
        name: "Italiano",
    },
    LanguageDefinition {
        code: "pl",
        name: "Polski",
    },
    LanguageDefinition {
        code: "nl",
        name: "Nederlands",
    },
    LanguageDefinition {
        code: "sv",
        name: "Svenska",
    },
    LanguageDefinition {
        code: "hr",
        name: "Hrvatski",
    },
    LanguageDefinition {
        code: "sr",
        name: "Srpski",
    },
    LanguageDefinition {
        code: "cs",
        name: "Czech",
    },
    LanguageDefinition {
        code: "sk",
        name: "Slovak",
    },
    LanguageDefinition {
        code: "hu",
        name: "Hungarian",
    },
    LanguageDefinition {
        code: "ro",
        name: "Romanian",
    },
    LanguageDefinition {
        code: "tr",
        name: "Turkish",
    },
    LanguageDefinition {
        code: "uk",
        name: "Ukrainian",
    },
    LanguageDefinition {
        code: "ru",
        name: "Russian",
    },
    LanguageDefinition {
        code: "ja",
        name: "Japanese",
    },
    LanguageDefinition {
        code: "ko",
        name: "Korean",
    },
    LanguageDefinition {
        code: "zh",
        name: "Chinese Simplified",
    },
    LanguageDefinition {
        code: "ar",
        name: "Arabic",
    },
    LanguageDefinition {
        code: "hi",
        name: "Hindi",
    },
    LanguageDefinition {
        code: "id",
        name: "Indonesian",
    },
    LanguageDefinition {
        code: "fi",
        name: "Finnish",
    },
    LanguageDefinition {
        code: "el",
        name: "Greek",
    },
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranslationCatalog {
    selected_code: String,
    english: Arc<BTreeMap<String, String>>,
    selected: Arc<BTreeMap<String, String>>,
}

impl TranslationCatalog {
    pub fn new(
        selected_code: impl Into<String>,
        english: BTreeMap<String, String>,
        selected: BTreeMap<String, String>,
    ) -> Self {
        Self {
            selected_code: selected_code.into(),
            english: Arc::new(english),
            selected: Arc::new(selected),
        }
    }

    pub fn selected_code(&self) -> &str {
        &self.selected_code
    }

    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        if short_announcements() {
            let short = format!("{key}_short");
            if let Some(text) = self
                .selected
                .get(&short)
                .or_else(|| self.english.get(&short))
            {
                return text;
            }
        }
        self.selected
            .get(key)
            .or_else(|| self.english.get(key))
            .map_or(key, String::as_str)
    }

    /// The full form of `key`, whatever the announcement length.
    pub fn full_text<'a>(&'a self, key: &'a str) -> &'a str {
        self.selected
            .get(key)
            .or_else(|| self.english.get(key))
            .map_or(key, String::as_str)
    }

    pub fn english_key_count(&self) -> usize {
        self.english.len()
    }

    pub fn selected_key_count(&self) -> usize {
        self.selected.len()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn short_announcements_use_the_short_form_only_where_one_exists() {
        let english = std::collections::BTreeMap::from([
            ("volume".to_owned(), "Volume: {volume}".to_owned()),
            ("volume_short".to_owned(), "{volume}".to_owned()),
            ("label".to_owned(), "Label".to_owned()),
        ]);
        let catalog =
            super::TranslationCatalog::new("en", english, std::collections::BTreeMap::new());
        super::set_short_announcements(true);
        let short = (
            catalog.text("volume").to_owned(),
            catalog.text("label").to_owned(),
        );
        let full = catalog.full_text("volume").to_owned();
        super::set_short_announcements(false);
        assert_eq!(short, ("{volume}".to_owned(), "Label".to_owned()));
        assert_eq!(full, "Volume: {volume}");
        assert_eq!(catalog.text("volume"), "Volume: {volume}");
    }

    use std::collections::{BTreeMap, HashSet};

    use super::{LANGUAGES, TranslationCatalog};

    #[test]
    fn baseline_contains_27_unique_languages() {
        let codes: HashSet<_> = LANGUAGES.iter().map(|language| language.code).collect();
        assert_eq!(LANGUAGES.len(), 27);
        assert_eq!(codes.len(), LANGUAGES.len());
    }

    #[test]
    fn translation_lookup_uses_selected_then_english_then_key() {
        let catalog = TranslationCatalog::new(
            "sl",
            [("play".to_owned(), "Play".to_owned())].into(),
            [("play".to_owned(), "Predvajaj".to_owned())].into(),
        );
        assert_eq!(catalog.text("play"), "Predvajaj");
        assert_eq!(catalog.text("missing"), "missing");

        let fallback = TranslationCatalog::new(
            "sl",
            [("pause".to_owned(), "Pause".to_owned())].into(),
            BTreeMap::new(),
        );
        assert_eq!(fallback.text("pause"), "Pause");
    }
}
