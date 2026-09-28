//! Spoken feedback for Python actions that the Rust beta cannot perform yet.
//!
//! Python announces unavailable actions through the player status and speech
//! and never opens a window for them. Where Python has a message for the
//! current situation (for example comments on a local file), that message is
//! used. Otherwise the action is announced as not available in this beta.

use apricot_core::{MediaItem, TranslationCatalog, action::action_by_id};

/// Spoken text for a Python feature that the Rust beta cannot perform yet.
pub fn unavailable_feature_message(catalog: &TranslationCatalog, feature: &str) -> String {
    catalog
        .text("rust_feature_unavailable")
        .replace("{feature}", feature)
}

/// Announcement for an action without a Rust route, or `None` where Python
/// stays silent in the same situation.
pub fn unavailable_action_message(
    catalog: &TranslationCatalog,
    action_id: &str,
    current_item: Option<&MediaItem>,
) -> Option<String> {
    let python_key = match action_id {
        // Python `save_edited_local_file` returns silently while edit mode is
        // off, and the Rust beta cannot turn edit mode on yet.
        "player_save_edit_copy" | "player_replace_edit_original" => return None,
        // Python `toggle_edit_mode`.
        "player_edit_mode" => match current_item {
            None => return None,
            Some(item) if !has_local_path(item) => Some("edit_mode_local_only"),
            Some(_) => None,
        },
        // Python `show_comments`.
        "player_comments" => match current_item {
            None => Some("no_player"),
            Some(item) if item.youtube_url_at_timestamp(0.0).is_none() => Some("comments_disabled"),
            Some(_) => None,
        },
        // Python `announce_bpm_async`.
        "player_bpm" if current_item.is_none() => Some("bpm_not_available"),
        _ => None,
    };
    if let Some(key) = python_key {
        return Some(catalog.text(key).to_owned());
    }
    Some(unavailable_feature_message(
        catalog,
        catalog.text(feature_label_key(action_id)),
    ))
}

/// Prefers the short feature name Python shows on the matching button or menu
/// item over the longer shortcut description.
fn feature_label_key(action_id: &str) -> &str {
    match action_id {
        "player_equalizer" => "equalizer",
        "player_comments" => "comments",
        "player_edit_mode" => "edit_mode",
        "open_audiovault" => "audiovault",
        "open_channel" => "open_channel",
        _ => action_by_id(action_id).map_or(action_id, |action| action.label_key),
    }
}

fn has_local_path(item: &MediaItem) -> bool {
    item.is_local_media()
        && item
            .local_path
            .as_deref()
            .is_some_and(|path| !path.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};

    use super::{unavailable_action_message, unavailable_feature_message};
    use crate::embedded_catalog;

    fn local_item() -> MediaItem {
        MediaItem {
            id: MediaId("song".to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: "Song".to_owned(),
            local_path: Some(r"C:\Music\song.mp3".to_owned()),
            url: None,
            stream_url: None,
            external_audio_url: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn youtube_item() -> MediaItem {
        MediaItem {
            id: MediaId("dQw4w9WgXcQ".to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: "Video".to_owned(),
            local_path: None,
            url: Some(
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
                    .parse()
                    .expect("url"),
            ),
            stream_url: None,
            external_audio_url: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn unported_actions_are_announced_with_their_localized_label() {
        let english = embedded_catalog("en");
        assert_eq!(
            unavailable_action_message(&english, "player_equalizer", Some(&local_item())),
            Some("Equalizer is not available in this beta yet.".to_owned())
        );
        assert_eq!(
            unavailable_action_message(&english, "copy_diagnostic_report", None),
            Some("Copy diagnostic report is not available in this beta yet.".to_owned())
        );
        let slovenian = embedded_catalog("sl");
        let message =
            unavailable_action_message(&slovenian, "player_equalizer", Some(&local_item()))
                .expect("message");
        assert!(message.ends_with(" v tej beta različici še ni na voljo."));
        assert!(message.starts_with(slovenian.text("equalizer")));
    }

    #[test]
    fn python_messages_are_used_where_python_has_one() {
        let catalog = embedded_catalog("en");
        let local = local_item();
        let youtube = youtube_item();
        let text = |key: &str| Some(catalog.text(key).to_owned());
        assert_eq!(
            unavailable_action_message(&catalog, "player_comments", Some(&local)),
            text("comments_disabled")
        );
        assert_eq!(
            unavailable_action_message(&catalog, "player_edit_mode", Some(&youtube)),
            text("edit_mode_local_only")
        );
        assert_eq!(
            unavailable_action_message(&catalog, "player_bpm", None),
            text("bpm_not_available")
        );
        assert_eq!(
            unavailable_action_message(&catalog, "player_comments", Some(&youtube)),
            Some(unavailable_feature_message(
                &catalog,
                catalog.text("comments")
            ))
        );
        assert_eq!(
            unavailable_action_message(&catalog, "player_edit_mode", Some(&local)),
            Some(unavailable_feature_message(
                &catalog,
                catalog.text("edit_mode")
            ))
        );
    }

    #[test]
    fn python_silent_cases_stay_silent() {
        let catalog = embedded_catalog("en");
        for action in ["player_save_edit_copy", "player_replace_edit_original"] {
            assert_eq!(
                unavailable_action_message(&catalog, action, Some(&local_item())),
                None
            );
        }
        assert_eq!(
            unavailable_action_message(&catalog, "player_edit_mode", None),
            None
        );
    }
}
