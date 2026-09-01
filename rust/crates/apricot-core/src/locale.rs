//! Shipped language identities.

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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::LANGUAGES;

    #[test]
    fn baseline_contains_27_unique_languages() {
        let codes: HashSet<_> = LANGUAGES.iter().map(|language| language.code).collect();
        assert_eq!(LANGUAGES.len(), 27);
        assert_eq!(codes.len(), LANGUAGES.len());
    }
}
