//! Platform-neutral player screen projection with canonical tab order.

use std::collections::BTreeSet;

use apricot_core::{MediaItem, MediaKind, MediaSource, TranslationCatalog, action::action_by_id};
use apricot_storage::SettingsDocument;

use crate::{PlaybackPhase, PlayerSession, SessionToggle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerControlRole {
    VideoHost,
    Button,
    Checkbox,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerControlModel {
    pub id: &'static str,
    pub action_id: Option<&'static str>,
    pub label: String,
    pub role: PlayerControlRole,
    pub checked: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TransportState {
    #[default]
    Playing,
    Paused,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PlayerToggle {
    Fullscreen,
    Repeat,
    SessionAutoplayNext,
    BassBoost,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlayerViewState {
    pub transport: TransportState,
    pub enabled_toggles: BTreeSet<PlayerToggle>,
}

impl From<&PlayerSession> for PlayerViewState {
    fn from(session: &PlayerSession) -> Self {
        let mut enabled_toggles = BTreeSet::new();
        for (source, target) in [
            (SessionToggle::Fullscreen, PlayerToggle::Fullscreen),
            (SessionToggle::Repeat, PlayerToggle::Repeat),
            (
                SessionToggle::AutoplayNext,
                PlayerToggle::SessionAutoplayNext,
            ),
            (SessionToggle::BassBoost, PlayerToggle::BassBoost),
        ] {
            if session.enabled_toggles().contains(&source) {
                enabled_toggles.insert(target);
            }
        }
        Self {
            transport: if session.phase() == PlaybackPhase::Paused {
                TransportState::Paused
            } else {
                TransportState::Playing
            },
            enabled_toggles,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerScreenModel {
    pub window_title: String,
    pub heading: String,
    pub controls: Vec<PlayerControlModel>,
    pub initial_focus_id: &'static str,
}

impl PlayerScreenModel {
    pub fn build(
        catalog: &TranslationCatalog,
        settings: &SettingsDocument,
        item: &MediaItem,
        state: &PlayerViewState,
    ) -> Self {
        let mut controls = navigation_controls(catalog, settings);
        controls.push(PlayerControlModel {
            id: "video_host",
            action_id: None,
            label: catalog.text("player").to_owned(),
            role: PlayerControlRole::VideoHost,
            checked: None,
        });
        append_primary_controls(&mut controls, catalog, settings, state.transport);
        append_media_controls(&mut controls, catalog, settings, item);
        controls.push(button(
            catalog,
            settings,
            "details",
            "show_video_details",
            "player_details",
        ));
        if settings.enable_background_playback {
            controls.push(button(
                catalog,
                settings,
                "close_player",
                "close_player",
                "player_back",
            ));
        }
        append_toggle_controls(&mut controls, catalog, settings, state);
        Self {
            window_title: item.title.clone(),
            heading: format!("{}: {}", catalog.text("internal_player"), item.title),
            controls,
            // With `show_video_details_by_default` the page opens its details
            // field instead, like Python `show_player_page`.
            initial_focus_id: "video_host",
        }
    }
}

fn append_primary_controls(
    controls: &mut Vec<PlayerControlModel>,
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    transport: TransportState,
) {
    let play_pause_label = match transport {
        TransportState::Playing => "pause",
        TransportState::Paused => "play",
    };
    for (id, label_key, action_id) in [
        ("previous", "previous", "player_previous"),
        ("play_pause", play_pause_label, "player_play_pause"),
        ("next", "next", "player_next"),
        ("queue", "playback_queue", "open_playback_queue"),
        ("add_to_playlist", "add_to_playlist", "add_to_playlist"),
        ("add_bookmark", "add_bookmark", "player_add_bookmark"),
        ("bookmarks", "bookmarks", "player_bookmarks"),
        ("output_devices", "output_devices", "player_output_devices"),
        ("equalizer", "equalizer", "player_equalizer"),
        (
            "audio_normalization",
            "audio_normalization",
            "player_replaygain",
        ),
        ("chapters", "chapters", "player_chapters"),
        ("transcript", "transcript", "player_transcript"),
        ("lyrics", "lyrics", "player_lyrics"),
        ("comments", "comments", "player_comments"),
        ("edit_mode", "edit_mode", "player_edit_mode"),
    ] {
        controls.push(button(catalog, settings, id, label_key, action_id));
    }
}

fn append_media_controls(
    controls: &mut Vec<PlayerControlModel>,
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    item: &MediaItem,
) {
    let local_media = item.source == MediaSource::Local || item.local_path.is_some();
    controls.push(button(
        catalog,
        settings,
        "copy_location",
        if local_media {
            "copy_path"
        } else {
            "copy_link"
        },
        "player_copy_link",
    ));
    if !local_media {
        controls.push(button(
            catalog,
            settings,
            "copy_stream_url",
            "copy_stream_url",
            "copy_stream_url",
        ));
    }
    if item.source == MediaSource::Podcast || item.kind == MediaKind::PodcastEpisode {
        controls.push(button(
            catalog,
            settings,
            "save_podcast_speed",
            "save_podcast_speed_preset",
            "save_podcast_speed_preset",
        ));
    }
}

fn append_toggle_controls(
    controls: &mut Vec<PlayerControlModel>,
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    state: &PlayerViewState,
) {
    for (id, label_key, action_id, toggle) in [
        (
            "fullscreen",
            "fullscreen",
            "player_fullscreen",
            PlayerToggle::Fullscreen,
        ),
        ("repeat", "repeat", "player_repeat", PlayerToggle::Repeat),
    ] {
        controls.push(checkbox(
            catalog,
            settings,
            id,
            label_key,
            action_id,
            state.enabled_toggles.contains(&toggle),
        ));
    }
    if !settings.autoplay_next {
        controls.push(checkbox_without_shortcut(
            catalog,
            "session_autoplay_next",
            "autoplay_next_session",
            state
                .enabled_toggles
                .contains(&PlayerToggle::SessionAutoplayNext),
        ));
    }
    controls.push(checkbox(
        catalog,
        settings,
        "bass_boost",
        "bass_boost",
        "player_bass_boost",
        state.enabled_toggles.contains(&PlayerToggle::BassBoost),
    ));
}

fn navigation_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<PlayerControlModel> {
    if settings.enable_background_playback {
        return vec![button(catalog, settings, "back", "back", "player_back")];
    }
    vec![
        button(
            catalog,
            settings,
            "back_results",
            "back_results",
            "player_back",
        ),
        button(catalog, settings, "back_main", "back", "player_back"),
    ]
}

fn button(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    id: &'static str,
    label_key: &str,
    action_id: &'static str,
) -> PlayerControlModel {
    PlayerControlModel {
        id,
        action_id: Some(action_id),
        label: label_with_shortcut(catalog.text(label_key), action_id, settings),
        role: PlayerControlRole::Button,
        checked: None,
    }
}

fn checkbox(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    id: &'static str,
    label_key: &str,
    action_id: &'static str,
    checked: bool,
) -> PlayerControlModel {
    PlayerControlModel {
        id,
        action_id: Some(action_id),
        label: label_with_shortcut(catalog.text(label_key), action_id, settings),
        role: PlayerControlRole::Checkbox,
        checked: Some(checked),
    }
}

fn checkbox_without_shortcut(
    catalog: &TranslationCatalog,
    id: &'static str,
    label_key: &str,
    checked: bool,
) -> PlayerControlModel {
    PlayerControlModel {
        id,
        action_id: None,
        label: catalog.text(label_key).to_owned(),
        role: PlayerControlRole::Checkbox,
        checked: Some(checked),
    }
}

fn label_with_shortcut(label: &str, action_id: &str, settings: &SettingsDocument) -> String {
    if !settings.show_shortcuts_in_labels {
        return label.to_owned();
    }
    let shortcut = settings
        .keyboard_shortcuts
        .get(action_id)
        .map(String::as_str)
        .or_else(|| action_by_id(action_id).map(|action| action.default_windows_shortcut))
        .unwrap_or_default();
    if shortcut.is_empty() {
        label.to_owned()
    } else {
        format!("{label} {shortcut}")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaKind, MediaSource};
    use apricot_storage::SettingsDocument;

    use super::{PlayerControlRole, PlayerScreenModel, PlayerViewState};
    use crate::english_catalog;

    fn local_item() -> apricot_core::MediaItem {
        apricot_core::MediaItem {
            id: MediaId("local".to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: "Track".to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: Some(r"C:\Music\Track.mp3".to_owned()),
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn local_player_has_canonical_controls_without_remote_stream_copy() {
        let model = PlayerScreenModel::build(
            &english_catalog(),
            &SettingsDocument::default(),
            &local_item(),
            &PlayerViewState::default(),
        );
        let ids: Vec<_> = model.controls.iter().map(|control| control.id).collect();
        assert_eq!(
            &ids[..5],
            &[
                "back_results",
                "back_main",
                "video_host",
                "previous",
                "play_pause"
            ]
        );
        assert!(ids.contains(&"copy_location"));
        assert!(!ids.contains(&"copy_stream_url"));
        assert_eq!(model.initial_focus_id, "video_host");
        assert!(
            model
                .controls
                .iter()
                .find(|control| control.id == "copy_location")
                .is_some_and(|control| control.label.starts_with("Copy path"))
        );
    }

    #[test]
    fn shortcut_labels_and_session_controls_follow_settings() {
        let mut settings = SettingsDocument {
            enable_background_playback: true,
            autoplay_next: true,
            ..SettingsDocument::default()
        };
        settings
            .keyboard_shortcuts
            .insert("player_previous".to_owned(), "F9".to_owned());
        let model = PlayerScreenModel::build(
            &english_catalog(),
            &settings,
            &local_item(),
            &PlayerViewState::default(),
        );
        assert_eq!(model.controls[0].id, "back");
        assert!(
            model
                .controls
                .iter()
                .find(|control| control.id == "previous")
                .is_some_and(|control| control.label.ends_with("F9"))
        );
        assert!(
            model
                .controls
                .iter()
                .all(|control| control.id != "session_autoplay_next")
        );
        assert_eq!(
            model
                .controls
                .iter()
                .filter(|control| control.role == PlayerControlRole::Checkbox)
                .count(),
            3
        );
    }
}
