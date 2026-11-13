//! Transactional playlist editor shared by both native frontends.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.
use crate::{FeatureForm, FormField, FormValue, FormValues};
use apricot_app::Application;
use apricot_storage::UserPlaylist;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub fn edit(
    application: &mut Application,
    index: usize,
    mut show: impl FnMut(&FeatureForm) -> Option<FormValues>,
) -> Result<bool, String> {
    let Some(mut draft) = application.user_playlists().get(index).cloned() else {
        return Err("Select a playlist first".into());
    };
    let mut selected = 0;
    let mut marked = BTreeSet::new();
    loop {
        let rows = if draft.items.is_empty() {
            vec!["Empty playlist".into()]
        } else {
            draft
                .items
                .iter()
                .enumerate()
                .map(|(at, item)| {
                    format!(
                        "{}. {}{}",
                        at + 1,
                        if marked.contains(&at) { "Marked. " } else { "" },
                        item.title
                    )
                })
                .collect()
        };
        let mut form = FeatureForm::new(
            "Edit playlist",
            vec![
                FormField::text("name", "Name", &draft.title),
                FormField::text(
                    "description",
                    "Description",
                    draft
                        .metadata
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                ),
                FormField::choice(
                    "entry",
                    "Entries",
                    rows,
                    selected.min(draft.items.len().saturating_sub(1)),
                ),
                FormField::choice(
                    "action",
                    "Action",
                    vec![
                        "Save playlist".into(),
                        "Add file or link".into(),
                        "Add folder".into(),
                        "Import playlist".into(),
                        "Export playlist".into(),
                        "Mark or unmark selected".into(),
                        "Mark all".into(),
                        "Clear marks".into(),
                        "Move marked entries".into(),
                        "Remove marked or selected".into(),
                        "Duplicate selected entry".into(),
                    ],
                    0,
                ),
            ],
        );
        form.submit = "Continue".into();
        let Some(values) = show(&form) else {
            return Ok(false);
        };
        draft.title = values
            .get("name")
            .and_then(FormValue::text)
            .unwrap_or("")
            .trim()
            .to_owned();
        draft.metadata.insert(
            "description".into(),
            serde_json::json!(
                values
                    .get("description")
                    .and_then(FormValue::text)
                    .unwrap_or("")
            ),
        );
        selected = values.get("entry").and_then(FormValue::choice).unwrap_or(0);
        match values
            .get("action")
            .and_then(FormValue::choice)
            .unwrap_or(0)
        {
            0 => {
                return application
                    .replace_user_playlist(index, draft, now())
                    .map_err(|e| e.to_string());
            }
            1 => {
                let Some(values) = show(&FeatureForm::new(
                    "Add file or link",
                    vec![FormField::text("path", "File path or media link", "")],
                )) else {
                    continue;
                };
                let path = values
                    .get("path")
                    .and_then(FormValue::text)
                    .unwrap_or("")
                    .trim();
                if path.is_empty() {
                    continue;
                }
                let base = url::Url::from_directory_path(
                    std::env::current_dir().map_err(|e| e.to_string())?,
                )
                .map_err(|()| "Invalid current folder")?;
                let item =
                    apricot_platform::playlist_file::parse(&format!("#EXTM3U\n{path}"), &base)?
                        .items
                        .into_iter()
                        .next()
                        .ok_or("The media location is invalid")?;
                draft.items.push(item);
            }
            2 => {
                let Some(values) = show(&FeatureForm::new(
                    "Add folder",
                    vec![
                        FormField::text("path", "Folder path", ""),
                        FormField::checkbox(
                            "live",
                            "Read current folder contents whenever this playlist opens",
                            true,
                        ),
                    ],
                )) else {
                    continue;
                };
                let path = PathBuf::from(
                    values
                        .get("path")
                        .and_then(FormValue::text)
                        .unwrap_or("")
                        .trim()
                        .trim_matches('"'),
                );
                if values
                    .get("live")
                    .and_then(FormValue::checked)
                    .unwrap_or(true)
                {
                    draft
                        .items
                        .push(apricot_platform::playlist_file::folder_reference(&path)?);
                } else {
                    draft.items.extend(
                        apricot_platform::local_media::scan_local_media_folder(&path)
                            .map_err(|e| e.to_string())?,
                    );
                }
            }
            3 => {
                let Some(values) = show(&FeatureForm::new(
                    "Import playlist",
                    vec![FormField::text(
                        "path",
                        "Playlist file path or HTTP address",
                        "",
                    )],
                )) else {
                    continue;
                };
                let value = values
                    .get("path")
                    .and_then(FormValue::text)
                    .unwrap_or("")
                    .trim();
                let parsed = if let Some(url) = url::Url::parse(value)
                    .ok()
                    .filter(|url| matches!(url.scheme(), "http" | "https"))
                {
                    apricot_platform::playlist_file::read_url(&url)?
                } else {
                    apricot_platform::playlist_file::read_local(Path::new(value))?
                };
                draft.items.extend(parsed.items);
            }
            4 => {
                let Some(values) = show(&FeatureForm::new(
                    "Export playlist",
                    vec![
                        FormField::text("path", "Destination (.json, .m3u or .m3u8)", ""),
                        FormField::checkbox("replace", "Replace an existing file", false),
                    ],
                )) else {
                    continue;
                };
                let path = PathBuf::from(
                    values
                        .get("path")
                        .and_then(FormValue::text)
                        .unwrap_or("")
                        .trim(),
                );
                if path.exists()
                    && !values
                        .get("replace")
                        .and_then(FormValue::checked)
                        .unwrap_or(false)
                {
                    return Err(
                        "The destination exists; confirm replacement or choose another name".into(),
                    );
                }
                export(&draft, &path)?;
            }
            5 => {
                if selected < draft.items.len() && !marked.remove(&selected) {
                    marked.insert(selected);
                }
            }
            6 => marked = (0..draft.items.len()).collect(),
            7 => marked.clear(),
            8 => {
                let indices: Vec<_> = if marked.is_empty() {
                    vec![selected]
                } else {
                    marked.iter().copied().collect()
                };
                let Some(values) = show(&FeatureForm::new(
                    "Move playlist entries",
                    vec![FormField::number(
                        "before",
                        "Move before entry number (last + 1 means end)",
                        (selected + 1) as f64,
                        1.0,
                        (draft.items.len() + 1) as f64,
                    )],
                )) else {
                    continue;
                };
                let destination = values
                    .get("before")
                    .and_then(FormValue::number)
                    .unwrap_or(1.0)
                    .round() as usize
                    - 1;
                move_entries(&mut draft.items, &indices, destination)?;
                marked.clear();
            }
            9 => {
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
            }
            _ => {
                if let Some(mut item) = draft.items.get(selected).cloned() {
                    item.metadata.remove("apricot_playlist_entry_id");
                    draft.items.insert(selected + 1, item);
                    selected += 1;
                }
            }
        }
    }
}
pub fn move_entries(
    items: &mut Vec<apricot_core::MediaItem>,
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
pub fn export(playlist: &UserPlaylist, path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("Choose a destination file".into());
    }
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes=match ext.as_str(){
        "json"=>serde_json::to_vec_pretty(&serde_json::json!({"version":1,"title":playlist.title,"description":playlist.metadata.get("description"),"items":playlist.items})).map_err(|e|e.to_string())?,
        "m3u"|"m3u8"=>{
            let mut text=format!("#EXTM3U\n#PLAYLIST:{}\n",playlist.title.replace(['\r','\n']," "));
            for item in apricot_platform::playlist_file::resolve_folders(&playlist.items,&std::sync::atomic::AtomicBool::new(false))?{
                let path=item.copy_location().ok_or("A playlist entry has no exportable location")?;
                if path.contains(['\r','\n']){return Err("A playlist path contains a line break; use JSON export".into());}
                text.push_str(&format!("#EXTINF:{},{}\n{}\n",item.duration_seconds.unwrap_or(-1.0),item.title.replace(['\r','\n']," "),path));
            }text.into_bytes()
        },
        _=>return Err("Choose JSON, M3U or M3U8 for playlist export".into()),
    };
    apricot_storage::write_bytes_atomic(path, &bytes).map_err(|e| e.to_string())
}
pub fn save_current(
    application: &mut Application,
    mut show: impl FnMut(&FeatureForm) -> Option<FormValues>,
) -> Result<bool, String> {
    let mut items = application.current_sequence_items().to_vec();
    if items.is_empty() {
        items.extend(application.player_session().current_item().cloned());
    }
    if items.is_empty() {
        return Err("No playback list is open".into());
    }
    let Some(values) = show(&FeatureForm::new(
        "Save current playback list",
        vec![FormField::text("name", "Playlist name", "")],
    )) else {
        return Ok(false);
    };
    let name = values.get("name").and_then(FormValue::text).unwrap_or("");
    match application
        .create_user_playlist_with_items(name, items, now())
        .map_err(|e| e.to_string())?
    {
        apricot_app::PlaylistCreateOutcome::Created(_) => Ok(true),
        other => Err(format!("The playlist could not be saved: {other:?}")),
    }
}
fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |time| time.as_secs_f64())
}
