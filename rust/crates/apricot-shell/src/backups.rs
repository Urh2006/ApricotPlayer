//! Import and export of settings and bookmarks, shared by both native
//! frontends. A settings import replaces every setting after a warning; a
//! bookmark import merges by default and replaces all only when the user picks
//! it and confirms.

use std::path::PathBuf;

use apricot_app::{Application, BookmarkControllerError, SettingsControllerError};
use apricot_core::TranslationCatalog;
use apricot_storage::BookmarkFileError;

use crate::{FeatureForm, FormField, FormValue, FormValues};

const EXTENSIONS: [&str; 1] = ["json"];

/// What the import and export need from the native window.
pub trait BackupUi {
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues>;
    fn open_file(&mut self, title: &str, extensions: &[&str]) -> Option<PathBuf>;
    fn save_file(
        &mut self,
        title: &str,
        default_name: &str,
        extensions: &[&str],
    ) -> Option<PathBuf>;
    /// A warning with Yes and No, No being the default.
    fn ask_yes_no(&mut self, message: &str) -> bool;
}

/// The outcome of one import or export.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Spoken in the status bar.
    pub message: String,
    /// Shown in a message box.
    pub error: Option<String>,
    /// The settings were replaced, so the window applies them again.
    pub settings_imported: bool,
}

impl Report {
    fn done(message: String) -> Self {
        Self {
            message,
            ..Self::default()
        }
    }

    fn failed(error: String) -> Self {
        Self {
            error: Some(error),
            ..Self::default()
        }
    }
}

fn unix_timestamp() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

fn settings_error(
    catalog: &TranslationCatalog,
    key: &str,
    error: &SettingsControllerError,
) -> String {
    match error {
        SettingsControllerError::ImportInvalid(_) => {
            catalog.text("settings_backup_invalid").to_owned()
        }
        SettingsControllerError::ImportTooLarge => catalog.text("backup_too_large").to_owned(),
        error => catalog.text(key).replace("{error}", &error.to_string()),
    }
}

fn bookmarks_error(
    catalog: &TranslationCatalog,
    key: &str,
    error: &BookmarkControllerError,
) -> String {
    match error {
        BookmarkControllerError::Storage(
            BookmarkFileError::InvalidRoot
            | BookmarkFileError::Json(apricot_storage::JsonFileError::Parse { .. }),
        ) => catalog.text("bookmarks_backup_invalid").to_owned(),
        BookmarkControllerError::Storage(BookmarkFileError::InvalidEntry(number)) => catalog
            .text("bookmarks_backup_damaged")
            .replace("{number}", &number.to_string()),
        BookmarkControllerError::Storage(BookmarkFileError::ImportTooLarge) => {
            catalog.text("backup_too_large").to_owned()
        }
        error => catalog.text(key).replace("{error}", &error.to_string()),
    }
}

/// Saves the current settings to a file the user picks.
pub fn export_settings(
    app: &Application,
    catalog: &TranslationCatalog,
    ui: &mut dyn BackupUi,
) -> Option<Report> {
    let path = ui.save_file(
        catalog.text("settings_export"),
        catalog.text("settings_backup_file_name"),
        &EXTENSIONS,
    )?;
    Some(match app.export_settings(&path) {
        Ok(()) => Report::done(catalog.text("settings_exported").to_owned()),
        Err(error) => Report::failed(settings_error(catalog, "settings_export_failed", &error)),
    })
}

/// Replaces every setting with a backup the user picks, after a warning.
pub fn import_settings(
    app: &mut Application,
    catalog: &TranslationCatalog,
    ui: &mut dyn BackupUi,
) -> Option<Report> {
    let path = ui.open_file(catalog.text("settings_import"), &EXTENSIONS)?;
    if !ui.ask_yes_no(catalog.text("settings_import_confirm")) {
        return None;
    }
    Some(match app.import_settings(&path) {
        Ok(()) => Report {
            message: catalog.text("settings_imported").to_owned(),
            error: None,
            settings_imported: true,
        },
        Err(error) => Report::failed(settings_error(catalog, "settings_import_failed", &error)),
    })
}

/// Saves every bookmark to a file the user picks.
pub fn export_bookmarks(
    app: &Application,
    catalog: &TranslationCatalog,
    ui: &mut dyn BackupUi,
) -> Option<Report> {
    let path = ui.save_file(
        catalog.text("bookmarks_export"),
        catalog.text("bookmarks_backup_file_name"),
        &EXTENSIONS,
    )?;
    let count = app.bookmarks().len();
    Some(match app.export_bookmarks(&path) {
        Ok(()) => Report::done(
            catalog
                .text("bookmarks_exported")
                .replace("{count}", &count.to_string()),
        ),
        Err(error) => Report::failed(bookmarks_error(catalog, "bookmarks_export_failed", &error)),
    })
}

/// How a bookmark import treats the current bookmarks; merging comes first.
#[must_use]
pub fn bookmarks_import_form(catalog: &TranslationCatalog) -> FeatureForm {
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("bookmarks_import"),
        vec![FormField::choice(
            "mode",
            catalog.text("bookmarks_import_mode"),
            vec![
                catalog.text("bookmarks_import_merge").to_owned(),
                catalog.text("bookmarks_import_replace").to_owned(),
            ],
            0,
        )],
    );
    catalog.text("import").clone_into(&mut form.submit);
    form
}

/// Adds the bookmarks of a backup the user picks to the current ones, or
/// replaces them all when the user chooses that and confirms it.
pub fn import_bookmarks(
    app: &mut Application,
    catalog: &TranslationCatalog,
    ui: &mut dyn BackupUi,
) -> Option<Report> {
    let path = ui.open_file(catalog.text("bookmarks_import"), &EXTENSIONS)?;
    let values = ui.form(&bookmarks_import_form(catalog))?;
    let replace = values.get("mode").and_then(FormValue::choice) == Some(1);
    if replace {
        let question = catalog
            .text("bookmarks_replace_confirm")
            .replace("{count}", &app.bookmarks().len().to_string());
        if !ui.ask_yes_no(&question) {
            return None;
        }
    }
    Some(
        match app.import_bookmarks(&path, replace, unix_timestamp()) {
            Ok(count) if replace => Report::done(
                catalog
                    .text("bookmarks_replaced")
                    .replace("{count}", &count.to_string()),
            ),
            Ok(0) => Report::done(catalog.text("bookmarks_import_nothing_new").to_owned()),
            Ok(count) => Report::done(
                catalog
                    .text("bookmarks_imported")
                    .replace("{count}", &count.to_string()),
            ),
            Err(error) => {
                Report::failed(bookmarks_error(catalog, "bookmarks_import_failed", &error))
            }
        },
    )
}

/// The import and export on the shared shell UI.
struct ShellBackupUi<'a> {
    ui: &'a mut dyn crate::ShellUi,
    yes: String,
    no: String,
}

impl BackupUi for ShellBackupUi<'_> {
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues> {
        self.ui.feature_form(form)
    }

    fn open_file(&mut self, title: &str, extensions: &[&str]) -> Option<PathBuf> {
        self.ui.choose_open_file(title, extensions)
    }

    fn save_file(
        &mut self,
        title: &str,
        default_name: &str,
        extensions: &[&str],
    ) -> Option<PathBuf> {
        self.ui
            .choose_save_file(title, None, default_name, extensions)
    }

    fn ask_yes_no(&mut self, message: &str) -> bool {
        self.ui.ask_yes_no(message, &self.yes, &self.no)
    }
}

/// Which import or export an action runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Backup {
    ExportSettings,
    ImportSettings,
    ExportBookmarks,
    ImportBookmarks,
}

impl Backup {
    /// The action that runs this import or export.
    #[must_use]
    pub fn from_action(action: &str) -> Option<Self> {
        Some(match action {
            "settings_export" => Self::ExportSettings,
            "settings_import" => Self::ImportSettings,
            "bookmarks_export" => Self::ExportBookmarks,
            "bookmarks_import" => Self::ImportBookmarks,
            _ => return None,
        })
    }
}

/// Runs one import or export with the given dialogs.
pub fn run(
    app: &mut Application,
    catalog: &TranslationCatalog,
    backup: Backup,
    ui: &mut dyn BackupUi,
) -> Option<Report> {
    match backup {
        Backup::ExportSettings => export_settings(app, catalog, ui),
        Backup::ImportSettings => import_settings(app, catalog, ui),
        Backup::ExportBookmarks => export_bookmarks(app, catalog, ui),
        Backup::ImportBookmarks => import_bookmarks(app, catalog, ui),
    }
}

impl crate::Shell {
    /// The settings and bookmark import and export actions.
    pub(crate) fn run_backup(&mut self, ui: &mut dyn crate::ShellUi, backup: Backup) {
        let catalog = self.catalog();
        let mut dialogs = ShellBackupUi {
            ui,
            yes: catalog.text("yes").to_owned(),
            no: catalog.text("no").to_owned(),
        };
        let Some(report) = run(&mut self.application, &catalog, backup, &mut dialogs) else {
            return;
        };
        if report.settings_imported {
            self.apply_integrations();
        }
        if !report.message.is_empty() {
            ui.set_status(&report.message, true);
        }
        if let Some(error) = &report.error {
            ui.show_error(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[derive(Default)]
    struct Script {
        files: VecDeque<PathBuf>,
        forms: VecDeque<Option<FormValues>>,
        answers: VecDeque<bool>,
        questions: Vec<String>,
    }

    impl BackupUi for Script {
        fn form(&mut self, _form: &FeatureForm) -> Option<FormValues> {
            self.forms.pop_front().flatten()
        }

        fn open_file(&mut self, _title: &str, _extensions: &[&str]) -> Option<PathBuf> {
            self.files.pop_front()
        }

        fn save_file(
            &mut self,
            _title: &str,
            _default_name: &str,
            _extensions: &[&str],
        ) -> Option<PathBuf> {
            self.files.pop_front()
        }

        fn ask_yes_no(&mut self, message: &str) -> bool {
            self.questions.push(message.to_owned());
            self.answers.pop_front().unwrap_or(false)
        }
    }

    fn mode(index: usize) -> FormValues {
        FormValues::from([("mode".to_owned(), FormValue::Choice(index))])
    }

    fn application(root: &std::path::Path) -> Application {
        let directory = root.join("data");
        std::fs::create_dir_all(&directory).unwrap();
        let paths =
            apricot_storage::SettingsPaths::for_app_data(&directory, &directory.join("legacy"));
        let defaults = apricot_storage::SettingsDocument::with_platform_defaults(
            directory.join("Downloads").to_string_lossy().into_owned(),
            directory.join("cache").to_string_lossy().into_owned(),
            "beta",
        );
        Application::new(
            apricot_app::SettingsController::load(paths, defaults),
            apricot_app::MainMenuAvailability::default(),
        )
    }

    fn song() -> apricot_core::MediaItem {
        apricot_core::MediaItem {
            id: apricot_core::MediaId("song".to_owned()),
            source: apricot_core::MediaSource::Local,
            kind: apricot_core::MediaKind::Audio,
            title: "Song".to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: Some("C:/Music/song.mp3".into()),
            channel: String::new(),
            duration_seconds: None,
            metadata: std::collections::BTreeMap::new(),
        }
    }

    fn catalog() -> TranslationCatalog {
        apricot_app::embedded_catalog("en")
    }

    #[test]
    fn settings_round_trip_replaces_every_setting_after_a_warning() {
        let root = tempfile::tempdir().unwrap();
        let mut app = application(root.path());
        let backup = root.path().join("settings backup.json");
        app.set_integer_setting(apricot_core::SettingId::DefaultVolume, 35)
            .unwrap();
        app.save_settings().unwrap();
        let mut ui = Script {
            files: VecDeque::from([backup.clone()]),
            ..Script::default()
        };
        let report = export_settings(&app, &catalog(), &mut ui).unwrap();
        assert_eq!(report.message, "Settings exported.");

        app.set_integer_setting(apricot_core::SettingId::DefaultVolume, 80)
            .unwrap();
        app.save_settings().unwrap();
        // No keeps the current settings.
        let mut ui = Script {
            files: VecDeque::from([backup.clone()]),
            answers: VecDeque::from([false]),
            ..Script::default()
        };
        assert!(import_settings(&mut app, &catalog(), &mut ui).is_none());
        assert_eq!(app.settings().default_volume, 80);

        let mut ui = Script {
            files: VecDeque::from([backup]),
            answers: VecDeque::from([true]),
            ..Script::default()
        };
        let report = import_settings(&mut app, &catalog(), &mut ui).unwrap();
        assert!(report.settings_imported);
        assert_eq!(app.settings().default_volume, 35);
        assert!(
            root.path()
                .join("data/settings.json.before-import")
                .is_file()
        );
    }

    #[test]
    fn a_file_that_is_not_a_settings_backup_changes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let mut app = application(root.path());
        let other = root.path().join("other.json");
        std::fs::write(&other, br#"{"something":"else"}"#).unwrap();
        let mut ui = Script {
            files: VecDeque::from([other]),
            answers: VecDeque::from([true]),
            ..Script::default()
        };
        let report = import_settings(&mut app, &catalog(), &mut ui).unwrap();
        assert_eq!(
            report.error.as_deref(),
            Some(catalog().text("settings_backup_invalid"))
        );
        assert!(!report.settings_imported);
    }

    #[test]
    fn bookmarks_merge_by_default_and_replace_only_after_confirmation() {
        let root = tempfile::tempdir().unwrap();
        let mut app = application(root.path());
        let item = song();
        app.add_bookmark("Intro", 1.0, item.clone(), 1.0).unwrap();
        let backup = root.path().join("bookmarks.json");
        let mut ui = Script {
            files: VecDeque::from([backup.clone()]),
            ..Script::default()
        };
        let report = export_bookmarks(&app, &catalog(), &mut ui).unwrap();
        assert_eq!(report.message, "1 bookmarks exported.");

        app.add_bookmark("Chorus", 30.0, item, 2.0).unwrap();
        let mut ui = Script {
            files: VecDeque::from([backup.clone()]),
            forms: VecDeque::from([Some(mode(0))]),
            ..Script::default()
        };
        let report = import_bookmarks(&mut app, &catalog(), &mut ui).unwrap();
        assert_eq!(
            report.message,
            catalog().text("bookmarks_import_nothing_new")
        );
        assert_eq!(app.bookmarks().len(), 2);

        // Replace all asks first; No keeps both bookmarks.
        let mut ui = Script {
            files: VecDeque::from([backup.clone()]),
            forms: VecDeque::from([Some(mode(1))]),
            answers: VecDeque::from([false]),
            ..Script::default()
        };
        assert!(import_bookmarks(&mut app, &catalog(), &mut ui).is_none());
        assert!(ui.questions[0].contains('2'));
        assert_eq!(app.bookmarks().len(), 2);

        let mut ui = Script {
            files: VecDeque::from([backup]),
            forms: VecDeque::from([Some(mode(1))]),
            answers: VecDeque::from([true]),
            ..Script::default()
        };
        import_bookmarks(&mut app, &catalog(), &mut ui).unwrap();
        assert_eq!(app.bookmarks().len(), 1);
        assert_eq!(app.bookmarks()[0].name, "Intro");
    }

    #[test]
    fn a_damaged_bookmark_backup_names_the_entry() {
        let root = tempfile::tempdir().unwrap();
        let mut app = application(root.path());
        let backup = root.path().join("bad.json");
        std::fs::write(&backup, b"[{}]").unwrap();
        let mut ui = Script {
            files: VecDeque::from([backup]),
            forms: VecDeque::from([Some(mode(0))]),
            ..Script::default()
        };
        let report = import_bookmarks(&mut app, &catalog(), &mut ui).unwrap();
        assert!(report.error.unwrap().contains('1'));
    }

    #[test]
    fn every_backup_action_has_a_runner() {
        for action in [
            "settings_export",
            "settings_import",
            "bookmarks_export",
            "bookmarks_import",
        ] {
            assert!(Backup::from_action(action).is_some(), "{action}");
        }
    }
}
