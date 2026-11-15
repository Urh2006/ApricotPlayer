//! Playlist editor shared by both native frontends: the edits are made on a
//! copy and written in one step when the user saves.

use std::{
    collections::BTreeSet,
    fmt::Write as _,
    path::{Path, PathBuf},
};

use apricot_app::Application;
use apricot_core::{MediaItem, TranslationCatalog};
use apricot_storage::UserPlaylist;

use crate::{FeatureForm, FormField, FormValue, FormValues};

/// File types the editor imports; HLS streams stay one live entry.
pub const PLAYLIST_IMPORT_EXTENSIONS: [&str; 7] =
    ["m3u", "m3u8", "pls", "xspf", "asx", "cue", "json"];

/// What the editor needs from the native window.
pub trait PlaylistEditorUi {
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues>;
    fn open_file(&mut self, title: &str, extensions: &[&str]) -> Option<PathBuf>;
    fn open_folder(&mut self, title: &str) -> Option<PathBuf>;
    fn save_file(
        &mut self,
        title: &str,
        default_name: &str,
        extensions: &[&str],
    ) -> Option<PathBuf>;
}

const ACTIONS: [&str; 11] = [
    "playlist_editor_save",
    "playlist_editor_add_file",
    "playlist_editor_add_link",
    "playlist_editor_add_folder",
    "playlist_editor_import",
    "playlist_editor_export",
    "playlist_editor_mark",
    "playlist_editor_mark_all",
    "playlist_editor_clear_marks",
    "playlist_editor_move",
    "playlist_editor_remove",
];

fn text<'a>(values: &'a FormValues, key: &str) -> &'a str {
    values.get(key).and_then(FormValue::text).unwrap_or("")
}

fn entry_label(catalog: &TranslationCatalog, item: &MediaItem) -> String {
    if item
        .metadata
        .get("apricot_folder_reference")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return catalog
            .text("playlist_folder_entry")
            .replace("{path}", item.local_path.as_deref().unwrap_or(""));
    }
    item.title.clone()
}

/// Shows a failed action's message in a read-only form so the editor stays
/// open and the draft is kept.
fn show_error(ui: &mut dyn PlaylistEditorUi, catalog: &TranslationCatalog, message: &str) {
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("playlist_edit"),
        vec![FormField::read_only("error", message, "")],
    );
    catalog.text("ok").clone_into(&mut form.submit);
    let _ = ui.form(&form);
}

/// Edits playlist `index`; `Ok(true)` when it was saved. A failed action shows
/// its message and leaves the editor open with the draft intact.
///
/// # Errors
///
/// Returns a message when the playlist does not exist.
#[allow(clippy::too_many_lines)] // One dialog loop with its actions.
pub fn edit(
    application: &mut Application,
    index: usize,
    ui: &mut dyn PlaylistEditorUi,
) -> Result<bool, String> {
    let catalog = apricot_app::embedded_catalog(&application.settings().language);
    let Some(mut draft) = application.user_playlists().get(index).cloned() else {
        return Err(catalog.text("playlist_editor_no_playlist").to_owned());
    };
    let mut selected = 0;
    let mut marked = BTreeSet::new();
    // After an action the form comes back on the entries with that action
    // still chosen, so Enter repeats it on the next entry.
    let mut last_action = None;
    loop {
        let rows = if draft.items.is_empty() {
            vec![catalog.text("playlist_empty").to_owned()]
        } else {
            draft
                .items
                .iter()
                .enumerate()
                .map(|(at, item)| {
                    let label = entry_label(&catalog, item);
                    if marked.contains(&at) {
                        catalog
                            .text("playlist_editor_marked_row")
                            .replace("{number}", &(at + 1).to_string())
                            .replace("{title}", &label)
                    } else {
                        format!("{}. {label}", at + 1)
                    }
                })
                .collect()
        };
        let mut form = FeatureForm::localized(
            &catalog,
            catalog.text("playlist_edit"),
            vec![
                FormField::text("name", catalog.text("playlist_editor_name"), &draft.title),
                FormField::text(
                    "description",
                    catalog.text("playlist_editor_description"),
                    draft
                        .metadata
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                ),
                FormField::choice(
                    "entry",
                    catalog.text("playlist_editor_entries"),
                    rows,
                    selected.min(draft.items.len().saturating_sub(1)),
                ),
                FormField::choice(
                    "action",
                    catalog.text("playlist_editor_action"),
                    ACTIONS
                        .iter()
                        .map(|key| catalog.text(key).to_owned())
                        .collect(),
                    last_action.unwrap_or(0),
                ),
            ],
        );
        catalog
            .text("playlist_editor_do")
            .clone_into(&mut form.submit);
        if last_action.is_some() {
            form.initial_focus = 2;
        }
        let Some(values) = ui.form(&form) else {
            return Ok(false);
        };
        let title = text(&values, "name").trim();
        if !title.is_empty() {
            title.clone_into(&mut draft.title);
        }
        draft.metadata.insert(
            "description".into(),
            serde_json::json!(text(&values, "description").trim()),
        );
        selected = values.get("entry").and_then(FormValue::choice).unwrap_or(0);
        let action = values
            .get("action")
            .and_then(FormValue::choice)
            .unwrap_or(0);
        last_action = Some(action);
        match action {
            0 => match application.replace_user_playlist(index, draft.clone(), now()) {
                Ok(saved) => return Ok(saved),
                Err(error) => show_error(ui, &catalog, &error.to_string()),
            },
            1 => {
                if let Some(path) = ui.open_file(
                    catalog.text("playlist_editor_add_file"),
                    apricot_platform::local_media::MEDIA_EXTENSIONS,
                ) {
                    draft.items.push(crate::local_file_item(&path));
                    selected = draft.items.len() - 1;
                }
            }
            2 => {
                let Some(values) = ui.form(&FeatureForm::localized(
                    &catalog,
                    catalog.text("playlist_editor_add_link"),
                    vec![FormField::text(
                        "url",
                        catalog.text("playlist_editor_link_field"),
                        "",
                    )],
                )) else {
                    continue;
                };
                let link = text(&values, "url").trim();
                match url::Url::parse(link)
                    .ok()
                    .filter(|url| matches!(url.scheme(), "http" | "https"))
                    .and_then(|url| {
                        apricot_platform::playlist_file::parse(&format!("#EXTM3U\n{url}"), &url)
                            .ok()
                    })
                    .and_then(|parsed| parsed.items.into_iter().next())
                {
                    Some(item) => {
                        draft.items.push(item);
                        selected = draft.items.len() - 1;
                    }
                    None => show_error(ui, &catalog, catalog.text("subtitles_url_invalid")),
                }
            }
            3 => {
                let Some(folder) = ui.open_folder(catalog.text("playlist_editor_add_folder"))
                else {
                    continue;
                };
                let Some(values) = ui.form(&FeatureForm::localized(
                    &catalog,
                    catalog.text("playlist_editor_add_folder"),
                    vec![FormField::choice(
                        "kind",
                        catalog.text("playlist_editor_folder_kind"),
                        vec![
                            catalog.text("playlist_editor_folder_live").to_owned(),
                            catalog.text("playlist_editor_folder_snapshot").to_owned(),
                        ],
                        0,
                    )],
                )) else {
                    continue;
                };
                let added = if values.get("kind").and_then(FormValue::choice) == Some(1) {
                    apricot_platform::local_media::scan_local_media_folder(&folder)
                        .map_err(|error| error.to_string())
                } else {
                    apricot_platform::playlist_file::folder_reference(&folder)
                        .map(|item| vec![item])
                };
                match added {
                    Ok(items) => {
                        draft.items.extend(items);
                        selected = draft.items.len().saturating_sub(1);
                    }
                    Err(error) => show_error(ui, &catalog, &error),
                }
            }
            4 => {
                let Some(path) = ui.open_file(
                    catalog.text("playlist_editor_import"),
                    &PLAYLIST_IMPORT_EXTENSIONS,
                ) else {
                    continue;
                };
                match apricot_platform::playlist_file::read_local(&path) {
                    Ok(parsed) => draft.items.extend(parsed.items),
                    Err(error) => show_error(
                        ui,
                        &catalog,
                        &catalog
                            .text("playlist_import_failed")
                            .replace("{error}", &error),
                    ),
                }
            }
            5 => {
                let name = format!("{}.m3u8", crate::safe_path_component(&draft.title));
                let Some(path) = ui.save_file(
                    catalog.text("playlist_editor_export"),
                    &name,
                    &["m3u8", "m3u", "json"],
                ) else {
                    continue;
                };
                if let Err(error) = export(&catalog, &draft, &path) {
                    show_error(ui, &catalog, &error);
                }
            }
            6 => {
                if selected < draft.items.len() && !marked.remove(&selected) {
                    marked.insert(selected);
                }
            }
            7 => marked = (0..draft.items.len()).collect(),
            8 => marked.clear(),
            9 => {
                if draft.items.is_empty() {
                    continue;
                }
                let indices: Vec<_> = if marked.is_empty() {
                    vec![selected]
                } else {
                    marked.iter().copied().collect()
                };
                #[allow(clippy::cast_precision_loss)]
                let length = draft.items.len() as f64;
                let Some(values) = ui.form(&FeatureForm::localized(
                    &catalog,
                    catalog.text("playlist_editor_move"),
                    vec![FormField::number(
                        "before",
                        catalog.text("playlist_editor_move_before"),
                        1.0,
                        1.0,
                        length + 1.0,
                    )],
                )) else {
                    continue;
                };
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let destination = values
                    .get("before")
                    .and_then(FormValue::number)
                    .unwrap_or(1.0)
                    .round() as usize
                    - 1;
                match move_entries(&mut draft.items, &indices, destination) {
                    Ok(()) => marked.clear(),
                    Err(error) => show_error(ui, &catalog, &error),
                }
            }
            _ => {
                if marked.is_empty() {
                    if selected < draft.items.len() {
                        draft.items.remove(selected);
                    }
                } else {
                    draft.items = draft
                        .items
                        .into_iter()
                        .enumerate()
                        .filter(|(index, _)| !marked.contains(index))
                        .map(|(_, item)| item)
                        .collect();
                }
                marked.clear();
                selected = selected.min(draft.items.len().saturating_sub(1));
            }
        }
    }
}

/// Moves `indices` before position `destination` in their old order.
///
/// # Errors
///
/// Returns an error when a position is outside the list.
pub fn move_entries(
    items: &mut Vec<MediaItem>,
    indices: &[usize],
    destination: usize,
) -> Result<(), String> {
    let marked: BTreeSet<_> = indices.iter().copied().collect();
    if destination > items.len() || marked.iter().any(|index| *index >= items.len()) {
        return Err("A playlist position is no longer valid".into());
    }
    let mut moved = Vec::new();
    let mut retained = Vec::new();
    for (at, item) in std::mem::take(items).into_iter().enumerate() {
        if marked.contains(&at) {
            moved.push(item);
        } else {
            retained.push(item);
        }
    }
    let at =
        destination.saturating_sub(marked.iter().filter(|index| **index < destination).count());
    retained.splice(at..at, moved);
    *items = retained;
    Ok(())
}

/// Writes the playlist as M3U8 (folders expanded to their current files) or
/// as Apricot JSON (folders kept as folders).
///
/// # Errors
///
/// Returns a translated message when the file cannot be written.
pub fn export(
    catalog: &TranslationCatalog,
    playlist: &UserPlaylist,
    path: &Path,
) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = if extension == "json" {
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "title": playlist.title,
            "description": playlist.metadata.get("description"),
            "items": playlist.items,
        }))
        .map_err(|error| error.to_string())?
    } else {
        let mut text = format!(
            "#EXTM3U\n#PLAYLIST:{}\n",
            playlist.title.replace(['\r', '\n'], " ")
        );
        let items = apricot_platform::playlist_file::resolve_folders(
            &playlist.items,
            &std::sync::atomic::AtomicBool::new(false),
        )?;
        for item in items {
            let Some(location) = item.copy_location() else {
                continue;
            };
            if location.contains(['\r', '\n']) {
                continue;
            }
            #[allow(clippy::cast_possible_truncation)]
            let seconds = item
                .duration_seconds
                .map_or(-1, |value| value.round() as i64);
            let _ = writeln!(
                text,
                "#EXTINF:{seconds},{}\n{location}",
                item.title.replace(['\r', '\n'], " ")
            );
        }
        text.into_bytes()
    };
    apricot_storage::write_bytes_atomic(path, &bytes).map_err(|error| {
        catalog
            .text("playlist_editor_export_failed")
            .replace("{error}", &error.to_string())
    })
}

/// Saves the list the player plays from as a new playlist.
///
/// # Errors
///
/// Returns a translated message when nothing plays or saving fails.
pub fn save_current(
    application: &mut Application,
    mut show: impl FnMut(&FeatureForm) -> Option<FormValues>,
) -> Result<bool, String> {
    let catalog = apricot_app::embedded_catalog(&application.settings().language);
    let mut items = application.current_sequence_items().to_vec();
    if items.is_empty() {
        items.extend(application.player_session().current_item().cloned());
    }
    if items.is_empty() {
        return Err(catalog.text("playlist_save_current_nothing").to_owned());
    }
    let Some(values) = show(&FeatureForm::localized(
        &catalog,
        catalog.text("shortcut_playlist_save_current"),
        vec![FormField::text(
            "name",
            catalog.text("playlist_editor_name"),
            "",
        )],
    )) else {
        return Ok(false);
    };
    match application
        .create_user_playlist_with_items(text(&values, "name"), items, now())
        .map_err(|error| error.to_string())?
    {
        apricot_app::PlaylistCreateOutcome::Created(_) => Ok(true),
        apricot_app::PlaylistCreateOutcome::EmptyName => Ok(false),
        apricot_app::PlaylistCreateOutcome::AlreadyExists => {
            Err(catalog.text("playlist_exists").to_owned())
        }
        apricot_app::PlaylistCreateOutcome::UnsupportedItem => {
            Err(catalog.text("playlist_save_current_failed").to_owned())
        }
    }
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |time| time.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[derive(Default)]
    struct Script {
        forms: VecDeque<Vec<(&'static str, FormValue)>>,
        files: VecDeque<PathBuf>,
        folders: VecDeque<PathBuf>,
        saves: VecDeque<PathBuf>,
        seen: Vec<FeatureForm>,
    }

    impl PlaylistEditorUi for Script {
        fn form(&mut self, form: &FeatureForm) -> Option<FormValues> {
            self.seen.push(form.clone());
            let answers = self.forms.pop_front()?;
            let mut values = FormValues::new();
            for field in &form.fields {
                let value = match &field.kind {
                    crate::FormFieldKind::Text { value, .. } => FormValue::Text(value.clone()),
                    crate::FormFieldKind::Choice { selected, .. } => FormValue::Choice(*selected),
                    crate::FormFieldKind::Number { value, .. } => FormValue::Number(*value),
                    _ => continue,
                };
                values.insert(field.id.clone(), value);
            }
            for (id, value) in answers {
                values.insert(id.to_owned(), value);
            }
            Some(values)
        }

        fn open_file(&mut self, _title: &str, _extensions: &[&str]) -> Option<PathBuf> {
            self.files.pop_front()
        }

        fn open_folder(&mut self, _title: &str) -> Option<PathBuf> {
            self.folders.pop_front()
        }

        fn save_file(
            &mut self,
            _title: &str,
            _name: &str,
            _extensions: &[&str],
        ) -> Option<PathBuf> {
            self.saves.pop_front()
        }
    }

    fn application(directory: &Path) -> Application {
        let paths =
            apricot_storage::SettingsPaths::for_app_data(directory, &directory.join("legacy"));
        let defaults = apricot_storage::SettingsDocument::with_platform_defaults(
            directory.join("Downloads").to_string_lossy().into_owned(),
            directory.join("cache").to_string_lossy().into_owned(),
            "beta",
        );
        let mut application = Application::new(
            apricot_app::SettingsController::load(paths, defaults),
            apricot_app::MainMenuAvailability::default(),
        );
        application
            .create_user_playlist_with_items("Mix", Vec::new(), 1.0)
            .expect("playlist");
        application
    }

    fn action(index: usize) -> (&'static str, FormValue) {
        ("action", FormValue::Choice(index))
    }

    #[test]
    fn the_editor_adds_marks_moves_and_saves_on_a_copy() {
        let directory = tempfile::tempdir().expect("folder");
        let media = directory.path().join("media");
        std::fs::create_dir_all(&media).expect("media");
        std::fs::write(media.join("01-alpha.ogg"), b"x").expect("alpha");
        std::fs::write(media.join("02-beta.ogg"), b"x").expect("beta");
        let mut application = application(directory.path());
        let mut ui = Script::default();
        ui.files.push_back(media.join("01-alpha.ogg"));
        ui.folders.push_back(media.clone());
        ui.forms.extend([
            vec![action(1)],
            vec![action(3)],
            vec![("kind", FormValue::Choice(0))],
            vec![("entry", FormValue::Choice(0)), action(6)],
            vec![action(9)],
            vec![("before", FormValue::Number(3.0))],
            vec![("name", FormValue::Text("Evening".into())), action(0)],
        ]);

        assert_eq!(edit(&mut application, 0, &mut ui), Ok(true));

        let playlist = &application.user_playlists()[0];
        assert_eq!(playlist.title, "Evening");
        assert_eq!(playlist.items.len(), 2);
        assert_eq!(
            playlist.items[0].metadata.get("apricot_folder_reference"),
            Some(&serde_json::json!(true))
        );
        assert_eq!(playlist.items[1].title, "01-alpha");
        let rows = entries(&ui.seen[4]);
        assert!(rows[0].ends_with(". 01-alpha") && rows[0].contains("Marked"));
        assert!(rows[1].starts_with("2. Folder: "));
        assert_eq!(ui.seen[0].initial_focus, 0);
        assert_eq!(ui.seen[4].initial_focus, 2);
        assert!(matches!(
            ui.seen[4].fields[3].kind,
            crate::FormFieldKind::Choice { selected: 6, .. }
        ));
    }

    #[test]
    fn closing_the_editor_keeps_the_saved_playlist() {
        let directory = tempfile::tempdir().expect("folder");
        let mut application = application(directory.path());
        let mut ui = Script::default();
        ui.forms
            .push_back(vec![("name", FormValue::Text("Other".into())), action(2)]);

        assert_eq!(edit(&mut application, 0, &mut ui), Ok(false));
        assert_eq!(application.user_playlists()[0].title, "Mix");
        assert_eq!(ui.seen.len(), 3);
    }

    #[test]
    fn moving_keeps_the_marked_order() {
        let item = |title: &str| crate::local_file_item(Path::new(&format!("{title}.mp3")));
        let mut items = vec![item("a"), item("b"), item("c"), item("d")];
        move_entries(&mut items, &[3, 1], 0).expect("move");
        let titles: Vec<_> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["b", "d", "a", "c"]);
        move_entries(&mut items, &[0], 4).expect("move");
        let titles: Vec<_> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["d", "a", "c", "b"]);
        assert!(move_entries(&mut items, &[4], 0).is_err());
    }

    #[test]
    fn export_expands_folders_in_m3u8_and_keeps_them_in_json() {
        let directory = tempfile::tempdir().expect("folder");
        let media = directory.path().join("media");
        std::fs::create_dir_all(&media).expect("media");
        std::fs::write(media.join("song.ogg"), b"x").expect("song");
        let catalog = apricot_app::embedded_catalog("en");
        let mut playlist = UserPlaylist::new("Mix", 1.0);
        playlist
            .items
            .push(apricot_platform::playlist_file::folder_reference(&media).expect("ref"));
        let m3u = directory.path().join("mix.m3u8");
        export(&catalog, &playlist, &m3u).expect("m3u8");
        let text = std::fs::read_to_string(&m3u).expect("read");
        assert!(text.starts_with("#EXTM3U\n#PLAYLIST:Mix\n#EXTINF:-1,song\n"));
        assert!(text.contains("song.ogg"));

        let json = directory.path().join("mix.json");
        export(&catalog, &playlist, &json).expect("json");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&json).expect("read")).expect("parse");
        assert_eq!(
            value["items"][0]["metadata"]["apricot_folder_reference"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn a_failed_action_shows_its_error_and_keeps_the_draft() {
        let directory = tempfile::tempdir().expect("folder");
        let missing = directory.path().join("missing.m3u");
        let mut application = application(directory.path());
        application
            .create_user_playlist_with_items("Other", Vec::new(), 2.0)
            .expect("second playlist");
        let index = application
            .user_playlists()
            .iter()
            .position(|playlist| playlist.title == "Mix")
            .expect("mix");
        let mut ui = Script::default();
        ui.files.push_back(missing);
        ui.forms.extend([
            // Import fails: the message is shown and the editor stays open.
            vec![("name", FormValue::Text("Kept".into())), action(4)],
            vec![],
            // A link that is not a web address.
            vec![action(2)],
            vec![("url", FormValue::Text("nonsense".into()))],
            vec![],
            // Save collides with the other playlist's name, then is corrected.
            vec![("name", FormValue::Text("Other".into())), action(0)],
            vec![],
            vec![("name", FormValue::Text("Fixed".into())), action(0)],
        ]);

        assert_eq!(edit(&mut application, index, &mut ui), Ok(true));
        assert_eq!(application.user_playlists()[index].title, "Fixed");
        let error_form = &ui.seen[1];
        assert!(matches!(
            error_form.fields[0].kind,
            crate::FormFieldKind::ReadOnly { .. }
        ));
        assert!(!error_form.fields[0].label.is_empty());
        // The draft title typed before the failure survived into the next form.
        assert!(matches!(
            &ui.seen[2].fields[0].kind,
            crate::FormFieldKind::Text { value, .. } if value == "Kept"
        ));
    }

    #[test]
    fn json_export_imports_back_with_folder_references_and_duplicates() {
        let directory = tempfile::tempdir().expect("folder");
        let media = directory.path().join("media");
        std::fs::create_dir_all(&media).expect("media");
        let song = media.join("song.ogg");
        std::fs::write(&song, b"x").expect("song");
        let catalog = apricot_app::embedded_catalog("en");
        let mut playlist = UserPlaylist::new("Mix", 1.0);
        playlist.items.push(crate::local_file_item(&song));
        playlist.items.push(crate::local_file_item(&song));
        playlist
            .items
            .push(apricot_platform::playlist_file::folder_reference(&media).expect("ref"));
        let json = directory.path().join("mix.json");
        export(&catalog, &playlist, &json).expect("json");
        assert!(PLAYLIST_IMPORT_EXTENSIONS.contains(&"json"));

        let parsed = apricot_platform::playlist_file::read_local(&json).expect("import");
        assert_eq!(parsed.title, "Mix");
        assert_eq!(parsed.items.len(), 3);
        assert_eq!(parsed.items[0].local_path, parsed.items[1].local_path);
        assert_eq!(
            parsed.items[2].metadata.get("apricot_folder_reference"),
            Some(&serde_json::json!(true))
        );
    }

    fn entries(form: &FeatureForm) -> Vec<String> {
        form.fields
            .iter()
            .find_map(|field| match &field.kind {
                crate::FormFieldKind::Choice { options, .. } if field.id == "entry" => {
                    Some(options.clone())
                }
                _ => None,
            })
            .expect("entries")
    }
}
