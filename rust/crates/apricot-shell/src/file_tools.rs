//! 2.1 file tools shared by both native frontends: recently opened files and
//! folders, a file opened with its folder, and renaming a file.

use std::path::{Path, PathBuf};

use apricot_app::{Application, recents::Recents};
use apricot_core::{MediaItem, TranslationCatalog};
use apricot_platform::file_operations::{RenameError, rename_file};

use crate::{FeatureForm, FormField, FormValue, FormValues};

/// A file opened from the system (Explorer, Finder, Play file) with the
/// files of its folder, as Settings, Playback chooses: the item to play with
/// the folder as the playing list, or `None` to play it alone.
pub fn opened_file_playback(app: &mut Application, path: &Path) -> Option<MediaItem> {
    let mode = app.settings().open_file_mode.clone();
    if mode != "folder" && mode != "folder_and_subfolders" {
        return None;
    }
    let folder = std::fs::canonicalize(path.parent()?).ok()?;
    let wanted = std::fs::canonicalize(path).ok()?;
    let canonical = |item: &MediaItem| {
        item.local_path
            .as_deref()
            .and_then(|local| std::fs::canonicalize(local).ok())
    };
    let items: Vec<_> = apricot_platform::local_media::scan_local_media_folder(&folder)
        .ok()?
        .into_iter()
        .filter(|item| {
            mode == "folder_and_subfolders"
                || canonical(item).and_then(|local| local.parent().map(Path::to_path_buf))
                    == Some(folder.clone())
        })
        .collect();
    let item = items
        .iter()
        .find(|item| canonical(item).as_ref() == Some(&wanted))?
        .clone();
    app.set_player_sequence_items(&items, &item);
    Some(item)
}

/// What the Recent form chose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecentChoice {
    File(PathBuf),
    Folder(PathBuf),
    Clear,
}

/// The recent files and folders, or `None` when there are none.
#[must_use]
pub fn recents_form(catalog: &TranslationCatalog, recents: &Recents) -> Option<FeatureForm> {
    if recents.is_empty() {
        return None;
    }
    let entries = recents
        .entries()
        .into_iter()
        .map(|(folder, path)| {
            catalog
                .text(if folder {
                    "recent_folder_entry"
                } else {
                    "recent_file_entry"
                })
                .replace("{path}", &path.display().to_string())
        })
        .collect();
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("shortcut_open_recents"),
        vec![
            FormField::choice("entry", catalog.text("recent_entries"), entries, 0),
            FormField::checkbox("clear", catalog.text("recent_clear"), false),
        ],
    );
    catalog.text("recent_open").clone_into(&mut form.submit);
    Some(form)
}

/// The choice made in [`recents_form`].
#[must_use]
pub fn recent_choice(values: &FormValues, recents: &Recents) -> Option<RecentChoice> {
    if values.get("clear").and_then(FormValue::checked) == Some(true) {
        return Some(RecentChoice::Clear);
    }
    let index = values.get("entry").and_then(FormValue::choice)?;
    let (folder, path) = recents.entries().into_iter().nth(index)?;
    Some(if folder {
        RecentChoice::Folder(path)
    } else {
        RecentChoice::File(path)
    })
}

/// The new name of `path`, starting from its current name.
#[must_use]
pub fn rename_form(catalog: &TranslationCatalog, path: &Path) -> FeatureForm {
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("shortcut_rename_file"),
        vec![FormField::text(
            "name",
            catalog.text("rename_file_name"),
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )],
    );
    catalog
        .text("rename_file_submit")
        .clone_into(&mut form.submit);
    form
}

/// Renames `path` as the form says: the new path, or the sentence to show.
///
/// # Errors
///
/// Returns the localized reason the file kept its name.
pub fn rename_from_values(
    catalog: &TranslationCatalog,
    path: &Path,
    values: &FormValues,
) -> Result<PathBuf, String> {
    let name = values.get("name").and_then(FormValue::text).unwrap_or("");
    rename_file(path, name).map_err(|error| match error {
        RenameError::Empty => catalog.text("rename_file_empty").to_owned(),
        RenameError::Invalid => catalog.text("rename_file_invalid").to_owned(),
        RenameError::Exists => catalog.text("rename_file_exists").to_owned(),
        RenameError::Failed(reason) => catalog
            .text("rename_file_failed")
            .replace("{error}", &reason),
    })
}

/// The web page of an item, or of its channel: only http and https.
#[must_use]
pub fn browser_address(item: Option<&MediaItem>, channel: bool) -> Option<String> {
    let item = item?;
    let address = if channel {
        item.metadata
            .get("channel_url")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)?
    } else {
        item.url.as_ref()?.to_string()
    };
    (address.starts_with("https://") || address.starts_with("http://")).then_some(address)
}

/// The name form of a favorite.
#[must_use]
pub fn favorite_name_form(catalog: &TranslationCatalog, title: &str) -> FeatureForm {
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("rename_favorite"),
        vec![FormField::text(
            "name",
            catalog.text("rename_file_name"),
            title,
        )],
    );
    catalog
        .text("rename_file_submit")
        .clone_into(&mut form.submit);
    form
}

/// The local file of an item, when it still exists.
#[must_use]
pub fn local_file(item: Option<&MediaItem>) -> Option<PathBuf> {
    item.and_then(|item| item.local_path.as_deref())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

impl crate::Shell {
    /// Recently opened files and folders.
    pub(crate) fn open_recents(&mut self, ui: &mut dyn crate::ShellUi) {
        let catalog = self.catalog();
        let recents = self.application.recents();
        let Some(form) = recents_form(&catalog, &recents) else {
            ui.set_status(catalog.text("recent_none"), true);
            return;
        };
        let Some(values) = ui.feature_form(&form) else {
            return;
        };
        match recent_choice(&values, &recents) {
            Some(RecentChoice::File(path)) => {
                self.application
                    .enqueue_activation(apricot_app::ActivationRequest::OpenFile(path));
                self.process_activations(ui);
            }
            Some(RecentChoice::Folder(path)) => self.start_local_folder_scan(ui, path),
            Some(RecentChoice::Clear) => {
                self.application.set_recents(&Recents::default());
                ui.set_status(catalog.text("recent_cleared"), true);
            }
            None => {}
        }
    }

    /// Opens the item's page or its channel's page in the browser.
    pub(crate) fn open_active_in_browser(&mut self, ui: &mut dyn crate::ShellUi, channel: bool) {
        let catalog = self.catalog();
        match browser_address(self.active_media_item(ui).as_ref(), channel) {
            Some(address) if apricot_platform::shell::open_url(&address).is_ok() => {}
            Some(_) => ui.show_error(catalog.text("file_action_unavailable")),
            None => ui.set_status(catalog.text("browser_address_missing"), true),
        }
    }

    /// Renames the selected or playing local file; in Favorites the entry.
    pub(crate) fn rename_active_file(&mut self, ui: &mut dyn crate::ShellUi) {
        let catalog = self.catalog();
        if self.view == crate::View::Favorites {
            let Some(index) = ui.selected_row() else {
                return;
            };
            let Some(title) = self
                .application
                .favorites()
                .get(index)
                .map(|item| item.title.clone())
            else {
                return;
            };
            let Some(values) = ui.feature_form(&favorite_name_form(&catalog, &title)) else {
                return;
            };
            let name = values.get("name").and_then(FormValue::text).unwrap_or("");
            match self.application.rename_favorite(index, name) {
                Ok(true) => self.refresh_media_collection(ui, true, Some(index)),
                Ok(false) => ui.show_error(catalog.text("rename_file_empty")),
                Err(error) => ui.show_error(&error.to_string()),
            }
            return;
        }
        let Some(path) = local_file(self.active_media_item(ui).as_ref()) else {
            ui.set_status(catalog.text("local_file_required"), true);
            return;
        };
        let Some(values) = ui.feature_form(&rename_form(&catalog, &path)) else {
            return;
        };
        match rename_from_values(&catalog, &path, &values) {
            Ok(renamed) => {
                ui.set_status(
                    &catalog.text("rename_file_done").replace(
                        "{name}",
                        &renamed
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    ),
                    true,
                );
                if self.view == crate::View::LocalFolder
                    && let Some(folder) = renamed.parent()
                {
                    self.start_local_folder_scan(ui, folder.to_path_buf());
                }
            }
            Err(error) => ui.show_error(&error),
        }
    }

    /// Copy as a file, show in the file manager, file properties and opening
    /// what is on the clipboard: the native window does the work.
    pub(crate) fn native_file_action(&mut self, ui: &mut dyn crate::ShellUi, action: &str) {
        let catalog = self.catalog();
        if action == "open_from_clipboard" {
            match ui.clipboard_paths().into_iter().find(|path| path.exists()) {
                Some(path) if path.is_dir() => self.start_local_folder_scan(ui, path),
                Some(path) => {
                    self.application
                        .enqueue_activation(apricot_app::ActivationRequest::OpenFile(path));
                    self.process_activations(ui);
                }
                None => ui.set_status(catalog.text("clipboard_no_path"), true),
            }
            return;
        }
        let Some(path) = local_file(self.active_media_item(ui).as_ref()) else {
            ui.set_status(catalog.text("local_file_required"), true);
            return;
        };
        let done = match action {
            "copy_file_to_clipboard" => ui.copy_files_to_clipboard(&[path]),
            "show_in_folder" => ui.reveal_file(&path),
            _ => ui.show_file_properties(&path),
        };
        if action == "copy_file_to_clipboard" && done {
            ui.set_status(catalog.text("file_copied_to_clipboard"), true);
        } else if !done {
            ui.set_status(catalog.text("file_action_unavailable"), true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recent_form_opens_files_folders_or_clears() {
        let catalog = apricot_app::embedded_catalog("en");
        assert!(recents_form(&catalog, &Recents::default()).is_none());
        let mut recents = Recents::default();
        recents.add_file(Path::new("C:/a.mp3"));
        recents.add_folder(Path::new("C:/Music"));
        let form = recents_form(&catalog, &recents).expect("form");
        assert_eq!(form.fields.len(), 2);
        let pick = |index| FormValues::from([("entry".to_owned(), FormValue::Choice(index))]);
        assert_eq!(
            recent_choice(&pick(1), &recents),
            Some(RecentChoice::Folder(PathBuf::from("C:/Music")))
        );
        let mut clear = pick(0);
        clear.insert("clear".into(), FormValue::Checkbox(true));
        assert_eq!(recent_choice(&clear, &recents), Some(RecentChoice::Clear));
    }

    #[test]
    fn a_file_opened_with_its_folder_brings_its_neighbours() {
        let root = tempfile::tempdir().expect("folder");
        for name in ["01.mp3", "02.mp3"] {
            std::fs::write(root.path().join(name), b"x").expect("file");
        }
        std::fs::create_dir(root.path().join("sub")).expect("sub");
        std::fs::write(root.path().join("sub/03.mp3"), b"x").expect("file");
        let mut app = crate::tests::keyboard::shell().application;
        assert!(opened_file_playback(&mut app, &root.path().join("02.mp3")).is_none());
        app.set_string_setting(apricot_core::SettingId::OpenFileMode, "folder")
            .expect("mode");
        let item = opened_file_playback(&mut app, &root.path().join("02.mp3")).expect("item");
        assert!(item.local_path.unwrap().ends_with("02.mp3"));
        assert_eq!(app.player_sequence_len(), 2);
        app.set_string_setting(
            apricot_core::SettingId::OpenFileMode,
            "folder_and_subfolders",
        )
        .expect("mode");
        opened_file_playback(&mut app, &root.path().join("02.mp3")).expect("item");
        assert_eq!(app.player_sequence_len(), 3);
    }
}
