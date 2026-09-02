//! Shipped language identities and platform-neutral translation lookup.

use std::collections::BTreeMap;

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
    english: BTreeMap<String, String>,
    selected: BTreeMap<String, String>,
}

impl TranslationCatalog {
    pub fn new(
        selected_code: impl Into<String>,
        english: BTreeMap<String, String>,
        selected: BTreeMap<String, String>,
    ) -> Self {
        Self {
            selected_code: selected_code.into(),
            english,
            selected,
        }
    }

    pub fn selected_code(&self) -> &str {
        &self.selected_code
    }

    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
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
