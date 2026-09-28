//! Spoken feedback for Python actions that the Rust beta cannot perform yet.
//!
//! Python announces unavailable actions through the player status and speech
//! and never opens a window for them, so an action without a Rust route is
//! announced as not available in this beta.

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
    _current_item: Option<&MediaItem>,
) -> Option<String> {
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
        "open_audiovault" => "audiovault",
        "open_channel" => "open_channel",
        _ => action_by_id(action_id).map_or(action_id, |action| action.label_key),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};

    use super::unavailable_action_message;
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
}
