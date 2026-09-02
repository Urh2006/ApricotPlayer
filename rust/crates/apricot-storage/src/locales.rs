use std::{collections::BTreeMap, fs, io, path::Path};

use apricot_core::{TranslationCatalog, locale::LANGUAGES};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LocaleLoadError {
    #[error("locale file could not be read: {path}: {source}")]
    Read { path: String, source: io::Error },
    #[error("locale file is not a string map: {path}: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },
    #[error("English locale is missing")]
    MissingEnglish,
}

/// Loads the requested locale plus the mandatory English fallback.
///
/// Unknown language codes deliberately select English, matching the Python
/// settings migration behavior.
///
/// # Errors
///
/// Returns [`LocaleLoadError`] when the English or selected locale cannot be
/// read and parsed as a JSON object containing string values.
pub fn load_translation_catalog(
    locales_directory: &Path,
    requested_code: &str,
) -> Result<TranslationCatalog, LocaleLoadError> {
    let english = read_locale(locales_directory, "en")?;
    if english.is_empty() {
        return Err(LocaleLoadError::MissingEnglish);
    }
    let selected_code = if LANGUAGES
        .iter()
        .any(|language| language.code == requested_code)
    {
        requested_code
    } else {
        "en"
    };
    let selected = if selected_code == "en" {
        english.clone()
    } else {
        read_locale(locales_directory, selected_code)?
    };
    Ok(TranslationCatalog::new(selected_code, english, selected))
}

fn read_locale(
    locales_directory: &Path,
    language_code: &str,
) -> Result<BTreeMap<String, String>, LocaleLoadError> {
    let path = locales_directory.join(format!("{language_code}.json"));
    let bytes = fs::read(&path).map_err(|source| LocaleLoadError::Read {
        path: path.display().to_string(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| LocaleLoadError::Parse {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use apricot_core::{
        CUSTOMIZABLE_MAIN_MENU, SCREENS, SETTINGS_SECTIONS, action::ACTIONS, locale::LANGUAGES,
    };

    use super::{load_translation_catalog, read_locale};

    fn python_locales_directory() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("apricot")
            .join("locales")
    }

    #[test]
    fn every_python_locale_loads_with_english_fallback() {
        let locales = python_locales_directory();
        for language in LANGUAGES {
            let catalog = load_translation_catalog(&locales, language.code)
                .unwrap_or_else(|error| panic!("{}: {error}", language.code));
            assert_eq!(catalog.selected_code(), language.code);
            assert!(catalog.english_key_count() > 500);
            assert!(catalog.selected_key_count() > 0);
            assert_ne!(catalog.text("play"), "play");
        }
    }

    #[test]
    fn unknown_language_falls_back_to_english() {
        let locales = python_locales_directory();
        let catalog = load_translation_catalog(&locales, "not-a-language").expect("English");
        assert_eq!(catalog.selected_code(), "en");
    }

    #[test]
    fn all_registry_labels_exist_in_the_english_locale() {
        let locales = python_locales_directory();
        let english = read_locale(&locales, "en").expect("English locale");
        let mut missing = Vec::new();

        for action in ACTIONS {
            if !english.contains_key(action.label_key) {
                missing.push(format!(
                    "action {} label {}",
                    action.id.as_str(),
                    action.label_key
                ));
            }
        }
        for item in CUSTOMIZABLE_MAIN_MENU {
            if !english.contains_key(item.label_key) {
                missing.push(format!(
                    "main menu {} label {}",
                    item.action_id, item.label_key
                ));
            }
        }
        for screen in SCREENS {
            if !english.contains_key(screen.label_key) {
                missing.push(format!("screen {} label {}", screen.id, screen.label_key));
            }
        }
        for section in SETTINGS_SECTIONS {
            if !english.contains_key(section.label_key) {
                missing.push(format!(
                    "settings section {} label {}",
                    section.section.id(),
                    section.label_key
                ));
            }
        }
        assert!(missing.is_empty(), "{}", missing.join("\n"));
    }
}
