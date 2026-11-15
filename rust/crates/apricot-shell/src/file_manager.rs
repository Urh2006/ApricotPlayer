//! Local file operations from the folder list, shared by both native
//! frontends: the Recycle Bin or Trash by default, permanent deletion only
//! after a confirmation, and copying or moving with a choice for every name
//! that already exists.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use apricot_core::{MediaItem, TranslationCatalog};
use apricot_platform::file_operations::{self, Collision, TransferMode, TransferOutcome};

use crate::{FeatureForm, FormField, FormValue, FormValues};

/// What the file operations need from the native window.
pub trait FileManagerUi {
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues>;
    fn open_folder(&mut self, title: &str) -> Option<PathBuf>;
    /// A warning with Yes and No, No being the default.
    fn ask_yes_no(&mut self, message: &str) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Trash,
    Delete,
    Copy,
    Move,
}

/// The result of one operation on one or more files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Files that left their folder.
    pub removed: Vec<PathBuf>,
    /// Spoken in the status bar.
    pub message: String,
    /// Shown in a message box when some files failed.
    pub failures: Option<String>,
}

/// The Recycle Bin on Windows, the Trash on macOS.
fn trash_key(windows: &'static str, macos: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        macos
    } else {
        windows
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The local files among `items`, in order.
#[must_use]
pub fn local_paths(items: &[MediaItem]) -> Vec<PathBuf> {
    items
        .iter()
        .filter(|item| item.is_local_media())
        .filter_map(|item| item.local_path.as_deref())
        .map(PathBuf::from)
        .collect()
}

/// Runs `operation` on `paths`; `None` when the user cancelled before any
/// file changed.
pub fn run(
    catalog: &TranslationCatalog,
    operation: Operation,
    paths: &[PathBuf],
    ui: &mut dyn FileManagerUi,
) -> Option<Report> {
    if paths.is_empty() {
        return None;
    }
    match operation {
        Operation::Trash => Some(remove(catalog, paths, true)),
        Operation::Delete => {
            let question = if let [path] = paths {
                catalog
                    .text("file_delete_confirm_one")
                    .replace("{name}", &file_name(path))
            } else {
                catalog
                    .text("file_delete_confirm")
                    .replace("{count}", &paths.len().to_string())
            };
            ui.ask_yes_no(&question)
                .then(|| remove(catalog, paths, false))
        }
        Operation::Copy | Operation::Move => {
            let mode = if operation == Operation::Copy {
                TransferMode::Copy
            } else {
                TransferMode::Move
            };
            let title = catalog.text(if mode == TransferMode::Copy {
                "file_copy_to"
            } else {
                "file_move_to"
            });
            let directory = ui.open_folder(title)?;
            Some(transfer(catalog, paths, mode, &directory, ui))
        }
    }
}

fn failures(catalog: &TranslationCatalog, failed: &[(PathBuf, String)]) -> Option<String> {
    (!failed.is_empty()).then(|| {
        let lines: Vec<_> = failed
            .iter()
            .map(|(path, error)| format!("{}: {error}", file_name(path)))
            .collect();
        catalog
            .text("file_operation_failed")
            .replace("{files}", &lines.join("\n"))
    })
}

fn summary(
    catalog: &TranslationCatalog,
    done: &[PathBuf],
    one_key: &str,
    many_key: &str,
) -> String {
    match done {
        [] => String::new(),
        [path] => catalog.text(one_key).replace("{name}", &file_name(path)),
        _ => catalog
            .text(many_key)
            .replace("{count}", &done.len().to_string()),
    }
}

fn remove(catalog: &TranslationCatalog, paths: &[PathBuf], trash: bool) -> Report {
    let mut done = Vec::new();
    let mut failed = Vec::new();
    for path in paths {
        let result = if trash {
            file_operations::move_to_trash(path)
        } else {
            file_operations::delete_permanently(path)
        };
        match result {
            Ok(()) => done.push(path.clone()),
            Err(error) => failed.push((path.clone(), error)),
        }
    }
    let message = if trash {
        summary(
            catalog,
            &done,
            trash_key("file_recycled_one", "file_trashed_one"),
            trash_key("file_recycled", "file_trashed"),
        )
    } else {
        summary(catalog, &done, "file_deleted_one", "file_deleted")
    };
    Report {
        removed: done,
        message,
        failures: failures(catalog, &failed),
    }
}

fn transfer(
    catalog: &TranslationCatalog,
    paths: &[PathBuf],
    mode: TransferMode,
    directory: &Path,
    ui: &mut dyn FileManagerUi,
) -> Report {
    let mut done = Vec::new();
    let mut skipped = 0;
    let mut failed = Vec::new();
    let mut for_all = None;
    for path in paths {
        let taken =
            file_operations::target_in(path, directory).is_some_and(|target| target.exists());
        let collision = if !taken {
            None
        } else if for_all.is_some() {
            for_all
        } else {
            let Some((choice, apply_all)) = ask_collision(catalog, path, ui) else {
                // Cancel stops here; the files already handled stay where
                // they went.
                skipped = paths.len() - done.len() - failed.len();
                break;
            };
            if apply_all {
                for_all = Some(choice);
            }
            Some(choice)
        };
        match file_operations::transfer(path, directory, mode, collision) {
            Ok(TransferOutcome::Done(_)) => done.push(path.clone()),
            Ok(TransferOutcome::Skipped) => skipped += 1,
            Err(error) => failed.push((path.clone(), error)),
        }
    }
    let mut message = if mode == TransferMode::Copy {
        summary(catalog, &done, "file_copied_one", "file_copied")
    } else {
        summary(catalog, &done, "file_moved_one", "file_moved")
    };
    if skipped > 0 {
        let skipped = catalog
            .text("file_skipped")
            .replace("{count}", &skipped.to_string());
        message = if message.is_empty() {
            skipped
        } else {
            format!("{message} {skipped}")
        };
    }
    Report {
        removed: if mode == TransferMode::Move {
            done
        } else {
            Vec::new()
        },
        message,
        failures: failures(catalog, &failed),
    }
}

fn ask_collision(
    catalog: &TranslationCatalog,
    path: &Path,
    ui: &mut dyn FileManagerUi,
) -> Option<(Collision, bool)> {
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("file_exists_title"),
        vec![
            FormField::choice(
                "choice",
                catalog
                    .text("file_exists")
                    .replace("{name}", &file_name(path)),
                vec![
                    catalog.text("file_exists_skip").to_owned(),
                    catalog.text("file_exists_keep_both").to_owned(),
                    catalog.text("file_exists_replace").to_owned(),
                ],
                0,
            ),
            FormField::checkbox("all", catalog.text("file_exists_apply_all"), false),
        ],
    );
    catalog.text("ok").clone_into(&mut form.submit);
    let values = ui.form(&form)?;
    let choice = match values.get("choice").and_then(FormValue::choice) {
        Some(1) => Collision::KeepBoth,
        Some(2) => Collision::Replace,
        _ => Collision::Skip,
    };
    let all = values
        .get("all")
        .and_then(FormValue::checked)
        .unwrap_or(false);
    Some((choice, all))
}

const ACTIONS: [&str; 7] = [
    "file_manager_trash",
    "delete_file_permanently",
    "file_copy_to",
    "file_move_to",
    "file_manager_mark",
    "file_manager_mark_all",
    "playlist_editor_clear_marks",
];

/// The Manage files dialog: marks files in the folder list and runs one
/// operation on the marked files, or on the chosen one when none is marked.
/// `None` when it was closed without changing anything.
pub fn manage(
    catalog: &TranslationCatalog,
    paths: &[PathBuf],
    selected: usize,
    ui: &mut dyn FileManagerUi,
) -> Option<Report> {
    manage_with_marks(catalog, paths, selected, false, ui)
}

/// [`manage`] with every file already marked, for files marked while
/// listening.
pub fn manage_marked(
    catalog: &TranslationCatalog,
    paths: &[PathBuf],
    ui: &mut dyn FileManagerUi,
) -> Option<Report> {
    manage_with_marks(catalog, paths, 0, true, ui)
}

fn manage_with_marks(
    catalog: &TranslationCatalog,
    paths: &[PathBuf],
    selected: usize,
    all_marked: bool,
    ui: &mut dyn FileManagerUi,
) -> Option<Report> {
    if paths.is_empty() {
        return None;
    }
    let mut selected = selected.min(paths.len() - 1);
    let mut marked: BTreeSet<usize> = if all_marked {
        (0..paths.len()).collect()
    } else {
        BTreeSet::new()
    };
    let mut last_action = None;
    loop {
        let rows = paths
            .iter()
            .enumerate()
            .map(|(at, path)| {
                if marked.contains(&at) {
                    catalog
                        .text("playlist_editor_marked_row")
                        .replace("{number}", &(at + 1).to_string())
                        .replace("{title}", &file_name(path))
                } else {
                    format!("{}. {}", at + 1, file_name(path))
                }
            })
            .collect();
        let mut form = FeatureForm::localized(
            catalog,
            catalog.text("file_manager_title"),
            vec![
                FormField::choice("file", catalog.text("file_manager_files"), rows, selected),
                FormField::choice(
                    "action",
                    catalog.text("playlist_editor_action"),
                    ACTIONS
                        .iter()
                        .map(|key| {
                            if *key == "file_manager_trash" {
                                catalog.text(trash_key("file_manager_recycle", key))
                            } else {
                                catalog.text(key)
                            }
                            .to_owned()
                        })
                        .collect(),
                    last_action.unwrap_or(0),
                ),
            ],
        );
        catalog
            .text("playlist_editor_do")
            .clone_into(&mut form.submit);
        catalog
            .text("file_manager_close")
            .clone_into(&mut form.cancel);
        // The files come first, so after marking the dialog comes back on
        // them with the same action chosen and Enter marks the next file.
        let values = ui.form(&form)?;
        selected = values
            .get("file")
            .and_then(FormValue::choice)
            .unwrap_or(0)
            .min(paths.len() - 1);
        let action = values
            .get("action")
            .and_then(FormValue::choice)
            .unwrap_or(0);
        last_action = Some(action);
        let operation = match action {
            0 => Operation::Trash,
            1 => Operation::Delete,
            2 => Operation::Copy,
            3 => Operation::Move,
            4 => {
                if !marked.remove(&selected) {
                    marked.insert(selected);
                }
                continue;
            }
            5 => {
                marked = (0..paths.len()).collect();
                continue;
            }
            _ => {
                marked.clear();
                continue;
            }
        };
        let chosen: Vec<_> = if marked.is_empty() {
            vec![paths[selected].clone()]
        } else {
            marked.iter().map(|at| paths[*at].clone()).collect()
        };
        // Cancelling a confirmation or a folder choice returns to the dialog.
        if let Some(report) = run(catalog, operation, &chosen, ui) {
            return Some(report);
        }
    }
}

/// The file operations on the shared shell UI.
struct ShellFileUi<'a> {
    ui: &'a mut dyn crate::ShellUi,
    yes: String,
    no: String,
}

impl FileManagerUi for ShellFileUi<'_> {
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues> {
        self.ui.feature_form(form)
    }

    fn open_folder(&mut self, title: &str) -> Option<PathBuf> {
        self.ui.choose_folder(title)
    }

    fn ask_yes_no(&mut self, message: &str) -> bool {
        self.ui.ask_yes_no(message, &self.yes, &self.no)
    }
}

impl crate::Shell {
    /// Delete, Shift+Delete and Manage files in the folder list; `None`
    /// opens Manage files.
    pub(crate) fn local_file_operation(
        &mut self,
        ui: &mut dyn crate::ShellUi,
        operation: Option<Operation>,
    ) {
        if self.view != crate::View::LocalFolder {
            return;
        }
        let Some(row) = ui.selected_row() else {
            return;
        };
        let catalog = self.catalog();
        let paths = local_paths(self.application.local_folder_session().items());
        let mut adapter = ShellFileUi {
            ui,
            yes: catalog.text("yes").to_owned(),
            no: catalog.text("no").to_owned(),
        };
        let report = match operation {
            Some(operation) => paths
                .get(row)
                .cloned()
                .and_then(|path| run(&catalog, operation, &[path], &mut adapter)),
            None => manage(&catalog, &paths, row, &mut adapter),
        };
        let ui = adapter.ui;
        let Some(report) = report else {
            return;
        };
        self.application.remove_local_folder_files(&report.removed);
        self.refresh_local_folder(ui, true, false);
        if !report.message.is_empty() {
            ui.set_status(&report.message, true);
        }
        if let Some(failures) = &report.failures {
            ui.show_error(failures);
        }
    }
}

impl crate::Shell {
    /// Manage files on the files marked while listening.
    pub(crate) fn manage_marked_files(&mut self, ui: &mut dyn crate::ShellUi) {
        let catalog = self.catalog();
        let paths: Vec<PathBuf> = self
            .application
            .playback_features()
            .marked_files
            .iter()
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .collect();
        if paths.is_empty() {
            ui.set_status(
                &catalog.text("files_marked_count").replace("{count}", "0"),
                true,
            );
            return;
        }
        let mut adapter = ShellFileUi {
            ui,
            yes: catalog.text("yes").to_owned(),
            no: catalog.text("no").to_owned(),
        };
        let report = manage_marked(&catalog, &paths, &mut adapter);
        let ui = adapter.ui;
        let Some(report) = report else {
            return;
        };
        let removed: Vec<String> = report
            .removed
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        self.application
            .playback_features_mut()
            .marked_files
            .retain(|path| !removed.contains(path));
        if !report.message.is_empty() {
            ui.set_status(&report.message, true);
        }
        if let Some(failures) = &report.failures {
            ui.show_error(failures);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, fs};

    use super::*;

    #[derive(Default)]
    struct Script {
        forms: VecDeque<Vec<(&'static str, FormValue)>>,
        folders: VecDeque<PathBuf>,
        answers: VecDeque<bool>,
        seen: Vec<FeatureForm>,
        questions: Vec<String>,
    }

    impl FileManagerUi for Script {
        fn form(&mut self, form: &FeatureForm) -> Option<FormValues> {
            self.seen.push(form.clone());
            let answers = self.forms.pop_front()?;
            let mut values = FormValues::new();
            for field in &form.fields {
                let value = match &field.kind {
                    crate::FormFieldKind::Choice { selected, .. } => FormValue::Choice(*selected),
                    crate::FormFieldKind::Checkbox { checked } => FormValue::Checkbox(*checked),
                    _ => continue,
                };
                values.insert(field.id.clone(), value);
            }
            for (id, value) in answers {
                values.insert(id.to_owned(), value);
            }
            Some(values)
        }

        fn open_folder(&mut self, _title: &str) -> Option<PathBuf> {
            self.folders.pop_front()
        }

        fn ask_yes_no(&mut self, message: &str) -> bool {
            self.questions.push(message.to_owned());
            self.answers.pop_front().unwrap_or(false)
        }
    }

    fn files(directory: &Path, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| {
                let path = directory.join(name);
                fs::write(&path, name).expect("write");
                path
            })
            .collect()
    }

    #[test]
    fn permanent_deletion_needs_a_yes() {
        let root = tempfile::tempdir().expect("folder");
        let paths = files(root.path(), &["a.mp3"]);
        let catalog = apricot_app::embedded_catalog("en");
        let mut ui = Script::default();
        ui.answers.extend([false, true]);

        assert_eq!(run(&catalog, Operation::Delete, &paths, &mut ui), None);
        assert!(paths[0].exists());
        let report = run(&catalog, Operation::Delete, &paths, &mut ui).expect("report");
        assert!(!paths[0].exists());
        assert_eq!(report.removed, paths);
        assert_eq!(report.message, "Deleted permanently: a.mp3");
        assert_eq!(
            ui.questions[0],
            "Delete a.mp3 permanently? This cannot be undone."
        );
    }

    #[test]
    fn a_collision_choice_can_apply_to_the_remaining_files() {
        let root = tempfile::tempdir().expect("folder");
        let from = root.path().join("from");
        let to = root.path().join("to");
        fs::create_dir_all(&from).expect("from");
        fs::create_dir_all(&to).expect("to");
        let paths = files(&from, &["a.mp3", "b.mp3", "c.mp3"]);
        files(&to, &["a.mp3", "b.mp3"]);
        let catalog = apricot_app::embedded_catalog("en");
        let mut ui = Script::default();
        ui.folders.push_back(to.clone());
        ui.forms.push_back(vec![
            ("choice", FormValue::Choice(1)),
            ("all", FormValue::Checkbox(true)),
        ]);

        let report = run(&catalog, Operation::Move, &paths, &mut ui).expect("report");

        assert_eq!(ui.seen.len(), 1);
        assert!(matches!(
            &ui.seen[0].fields[0].label,
            label if label == "a.mp3 already exists in the destination folder"
        ));
        assert_eq!(report.message, "3 files moved.");
        assert_eq!(report.removed, paths);
        for name in ["a.mp3", "a (2).mp3", "b.mp3", "b (2).mp3", "c.mp3"] {
            assert!(to.join(name).exists(), "{name}");
        }
    }

    #[test]
    fn cancelling_a_collision_skips_the_rest() {
        let root = tempfile::tempdir().expect("folder");
        let from = root.path().join("from");
        let to = root.path().join("to");
        fs::create_dir_all(&from).expect("from");
        fs::create_dir_all(&to).expect("to");
        let paths = files(&from, &["a.mp3", "b.mp3", "c.mp3"]);
        files(&to, &["b.mp3"]);
        let catalog = apricot_app::embedded_catalog("en");
        let mut ui = Script::default();
        ui.folders.push_back(to.clone());

        let report = run(&catalog, Operation::Copy, &paths, &mut ui).expect("report");

        assert_eq!(report.message, "Copied: a.mp3 2 skipped.");
        assert!(report.removed.is_empty());
        assert!(!to.join("c.mp3").exists());
    }

    #[test]
    fn manage_files_runs_on_the_marked_files() {
        let root = tempfile::tempdir().expect("folder");
        let paths = files(root.path(), &["a.mp3", "b.mp3", "c.mp3"]);
        let catalog = apricot_app::embedded_catalog("en");
        let mut ui = Script::default();
        ui.forms.extend([
            vec![
                ("file", FormValue::Choice(0)),
                ("action", FormValue::Choice(4)),
            ],
            vec![
                ("file", FormValue::Choice(2)),
                ("action", FormValue::Choice(4)),
            ],
            vec![
                ("file", FormValue::Choice(1)),
                ("action", FormValue::Choice(1)),
            ],
        ]);
        ui.answers.push_back(true);

        let report = manage(&catalog, &paths, 0, &mut ui).expect("report");

        assert_eq!(report.removed, [paths[0].clone(), paths[2].clone()]);
        assert_eq!(report.message, "2 files deleted permanently.");
        assert_eq!(
            ui.questions,
            ["Delete 2 files permanently? This cannot be undone."]
        );
        assert!(paths[1].exists());
        let crate::FormFieldKind::Choice { options, selected } = &ui.seen[2].fields[0].kind else {
            panic!("files");
        };
        assert_eq!(options[0], "1. Marked. a.mp3");
        assert_eq!(options[2], "3. Marked. c.mp3");
        assert_eq!(*selected, 2);
    }
}
