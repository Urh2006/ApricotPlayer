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
    /// Python `show_player_page`: with background playback the result list
    /// the player came from sits between Back and the player, except in full
    /// screen.
    pub embedded_results: bool,
}

impl PlayerScreenModel {
    pub fn build(
        catalog: &TranslationCatalog,
        settings: &SettingsDocument,
        item: &MediaItem,
        state: &PlayerViewState,
    ) -> Self {
        let fullscreen = state.enabled_toggles.contains(&PlayerToggle::Fullscreen);
        let mut controls = navigation_controls(catalog, settings, fullscreen);
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
            // Python `close_current_player`, labelled with the Back shortcut.
            controls.push(button_for(
                catalog,
                settings,
                "close_player",
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
            embedded_results: settings.enable_background_playback && !fullscreen,
        }
    }
}

fn append_primary_controls(
    controls: &mut Vec<PlayerControlModel>,
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    transport: TransportState,
) {
    controls.push(button(
        catalog,
        settings,
        "previous",
        "previous",
        "player_previous",
    ));
    // Python `current_play_pause_label`: the button never shows a shortcut.
    controls.push(PlayerControlModel {
        id: "play_pause",
        action_id: Some("player_play_pause"),
        label: catalog.text(play_pause_key(transport)).to_owned(),
        role: PlayerControlRole::Button,
        checked: None,
    });
    for (id, label_key, action_id) in [
        ("next", "next", "player_next"),
        ("queue", "playback_queue", "open_playback_queue"),
        ("add_to_playlist", "add_to_playlist", "add_to_playlist"),
        ("add_bookmark", "add_bookmark", "player_add_bookmark"),
        ("bookmarks", "bookmarks", "player_bookmarks"),
        ("output_devices", "output_devices", "player_output_devices"),
        ("equalizer", "equalizer", "player_equalizer"),
    ] {
        controls.push(button(catalog, settings, id, label_key, action_id));
    }
    // Python `audio_normalization_status_label`: the button names the mode.
    let status = catalog.text("audio_normalization_status").replace(
        "{mode}",
        catalog.text(replaygain_mode_key(&settings.replaygain_mode)),
    );
    controls.push(PlayerControlModel {
        id: "audio_normalization",
        action_id: Some("player_replaygain"),
        label: label_with_shortcut(&status, "player_replaygain", settings),
        role: PlayerControlRole::Button,
        checked: None,
    });
    for (id, label_key, action_id) in [
        ("chapters", "chapters", "player_chapters"),
        ("transcript", "transcript", "player_transcript"),
        ("lyrics", "lyrics", "player_lyrics"),
        ("comments", "comments", "player_comments"),
        ("edit_mode", "edit_mode", "player_edit_mode"),
    ] {
        controls.push(button(catalog, settings, id, label_key, action_id));
    }
}

/// Python `current_play_pause_label`.
pub const fn play_pause_key(transport: TransportState) -> &'static str {
    match transport {
        TransportState::Playing => "pause",
        TransportState::Paused => "play",
    }
}

/// Python `replaygain_mode_label`.
pub fn replaygain_mode_key(mode: &str) -> &'static str {
    match mode {
        "track" => "replaygain_track",
        "album" => "replaygain_album",
        _ => "replaygain_off",
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

/// Python `show_player_page` navigation row. Every button is labelled with
/// the Back shortcut; the activation ids say where each one goes.
fn navigation_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    fullscreen: bool,
) -> Vec<PlayerControlModel> {
    if settings.enable_background_playback && fullscreen {
        // Python `exit_fullscreen_to_results`.
        return vec![button_for(
            catalog,
            settings,
            "back_fullscreen_results",
            "back_results",
            "player_fullscreen_back_to_results",
            "player_back",
        )];
    }
    if settings.enable_background_playback {
        // Python `leave_player_to_main_menu(force_keep_playing=True)`.
        return vec![button_for(
            catalog,
            settings,
            "back",
            "back",
            "player_back_keep_playing",
            "player_back",
        )];
    }
    vec![
        // Python `leave_player_to_previous_screen`.
        button_for(
            catalog,
            settings,
            "back_results",
            "back_results",
            "player_back_to_results",
            "player_back",
        ),
        // Python `leave_player_to_main_menu(force_keep_playing=False)`.
        button_for(
            catalog,
            settings,
            "back_main",
            "back",
            "player_back_to_main_menu",
            "player_back",
        ),
    ]
}

fn button(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    id: &'static str,
    label_key: &str,
    action_id: &'static str,
) -> PlayerControlModel {
    button_for(catalog, settings, id, label_key, action_id, action_id)
}

/// A button whose activation differs from the action that names its
/// shortcut in the label.
fn button_for(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    id: &'static str,
    label_key: &str,
    action_id: &'static str,
    shortcut_action_id: &str,
) -> PlayerControlModel {
    PlayerControlModel {
        id,
        action_id: Some(action_id),
        label: label_with_shortcut(catalog.text(label_key), shortcut_action_id, settings),
        role: PlayerControlRole::Button,
        checked: None,
    }
}

/// Python `add_background_player_section`: the player and a row of buttons
/// appended to every other screen while background playback continues.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundPlayerModel {
    /// The static label "Player: title".
    pub label: String,
    pub buttons: Vec<BackgroundPlayerButton>,
}

/// A button of the background player. Python also calls `SetName("Player:
/// label")`, but wx keeps that name private: screen readers get the label.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackgroundPlayerButton {
    pub id: &'static str,
    pub action_id: &'static str,
    pub label: String,
}

impl BackgroundPlayerModel {
    pub fn build(
        catalog: &TranslationCatalog,
        settings: &SettingsDocument,
        item: &MediaItem,
        transport: TransportState,
    ) -> Self {
        let label = catalog
            .text("background_player_now_playing")
            .replace("{title}", &item.title);
        let button = |id, label, action_id| BackgroundPlayerButton {
            id,
            action_id,
            label,
        };
        let mut buttons = vec![
            button(
                "previous",
                label_with_shortcut(catalog.text("previous"), "player_previous", settings),
                "player_previous",
            ),
            // Python `current_play_pause_label` has no shortcut.
            button(
                "play_pause",
                catalog.text(play_pause_key(transport)).to_owned(),
                "player_play_pause",
            ),
        ];
        for (id, label_key, action_id) in [
            ("next", "next", "player_next"),
            ("queue", "playback_queue", "open_playback_queue"),
            ("add_to_playlist", "add_to_playlist", "add_to_playlist"),
            ("output_devices", "output_devices", "player_output_devices"),
            ("equalizer", "equalizer", "player_equalizer"),
            ("fullscreen", "fullscreen", "player_fullscreen"),
            ("bass_boost", "bass_boost", "player_bass_boost"),
            ("repeat", "repeat", "player_repeat"),
            ("shuffle", "shuffle", "player_shuffle"),
            ("copy_link", "copy_link", "player_copy_link"),
        ] {
            buttons.push(button(
                id,
                label_with_shortcut(catalog.text(label_key), action_id, settings),
                action_id,
            ));
        }
        buttons.push(button(
            "close",
            label_with_shortcut(catalog.text("close_player"), "player_back", settings),
            "close_player",
        ));
        Self { label, buttons }
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

    use super::{
        BackgroundPlayerModel, PlayerControlRole, PlayerScreenModel, PlayerToggle, PlayerViewState,
        TransportState,
    };
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
    fn audio_normalization_button_names_the_current_mode() {
        let settings = SettingsDocument {
            replaygain_mode: "album".to_owned(),
            ..SettingsDocument::default()
        };
        let model = PlayerScreenModel::build(
            &english_catalog(),
            &settings,
            &local_item(),
            &PlayerViewState::default(),
        );
        let button = model
            .controls
            .iter()
            .find(|control| control.id == "audio_normalization")
            .expect("audio normalization button");
        assert_eq!(button.label, "Audio normalization: Album Ctrl+Shift+G");
        assert_eq!(button.action_id, Some("player_replaygain"));
        let ids: Vec<_> = model.controls.iter().map(|control| control.id).collect();
        let position = |id| ids.iter().position(|candidate| *candidate == id);
        assert_eq!(
            position("audio_normalization"),
            position("equalizer").map(|i| i + 1)
        );
        assert_eq!(
            position("chapters"),
            position("audio_normalization").map(|i| i + 1)
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

    fn background_settings() -> SettingsDocument {
        SettingsDocument {
            enable_background_playback: true,
            ..SettingsDocument::default()
        }
    }

    fn control<'a>(model: &'a PlayerScreenModel, id: &str) -> &'a super::PlayerControlModel {
        model
            .controls
            .iter()
            .find(|control| control.id == id)
            .expect("control")
    }

    #[test]
    fn navigation_buttons_leave_the_player_like_python() {
        let model = PlayerScreenModel::build(
            &english_catalog(),
            &SettingsDocument::default(),
            &local_item(),
            &PlayerViewState::default(),
        );
        assert!(!model.embedded_results);
        let back_results = control(&model, "back_results");
        assert_eq!(back_results.label, "Back to results Escape");
        assert_eq!(back_results.action_id, Some("player_back_to_results"));
        let back_main = control(&model, "back_main");
        assert_eq!(back_main.label, "Back to main menu Escape");
        assert_eq!(back_main.action_id, Some("player_back_to_main_menu"));
        assert!(
            model
                .controls
                .iter()
                .all(|control| control.id != "close_player")
        );
    }

    #[test]
    fn background_playback_keeps_playing_and_embeds_results() {
        let model = PlayerScreenModel::build(
            &english_catalog(),
            &background_settings(),
            &local_item(),
            &PlayerViewState::default(),
        );
        assert!(model.embedded_results);
        assert_eq!(model.controls[0].id, "back");
        assert_eq!(model.controls[0].label, "Back to main menu Escape");
        assert_eq!(
            model.controls[0].action_id,
            Some("player_back_keep_playing")
        );
        let close = control(&model, "close_player");
        assert_eq!(close.label, "Close Escape");
        assert_eq!(close.action_id, Some("close_player"));
        let ids: Vec<_> = model.controls.iter().map(|control| control.id).collect();
        let details = ids.iter().position(|id| *id == "details");
        assert_eq!(
            ids.iter().position(|id| *id == "close_player"),
            details.map(|index| index + 1)
        );
    }

    #[test]
    fn full_screen_background_player_goes_back_to_results() {
        let mut state = PlayerViewState::default();
        state.enabled_toggles.insert(PlayerToggle::Fullscreen);
        let model = PlayerScreenModel::build(
            &english_catalog(),
            &background_settings(),
            &local_item(),
            &state,
        );
        assert!(!model.embedded_results);
        assert_eq!(model.controls[0].id, "back_fullscreen_results");
        assert_eq!(model.controls[0].label, "Back to results Escape");
        assert_eq!(
            model.controls[0].action_id,
            Some("player_fullscreen_back_to_results")
        );
        assert_eq!(model.controls[1].id, "video_host");
    }

    #[test]
    fn play_pause_button_has_no_shortcut_in_its_label() {
        let model = PlayerScreenModel::build(
            &english_catalog(),
            &SettingsDocument::default(),
            &local_item(),
            &PlayerViewState::default(),
        );
        assert_eq!(control(&model, "play_pause").label, "Pause");
        assert_eq!(control(&model, "previous").label, "Previous Ctrl+PageUp");
    }

    #[test]
    fn background_player_section_matches_python_buttons() {
        let model = BackgroundPlayerModel::build(
            &english_catalog(),
            &background_settings(),
            &local_item(),
            TransportState::Paused,
        );
        assert_eq!(model.label, "Player: Track");
        let labels: Vec<_> = model
            .buttons
            .iter()
            .map(|button| button.label.as_str())
            .collect();
        assert_eq!(labels.len(), 13);
        assert_eq!(labels[0], "Previous Ctrl+PageUp");
        assert_eq!(labels[1], "Play");
        assert_eq!(labels[12], "Close Escape");
        let actions: Vec<_> = model
            .buttons
            .iter()
            .map(|button| button.action_id)
            .collect();
        assert_eq!(
            actions,
            [
                "player_previous",
                "player_play_pause",
                "player_next",
                "open_playback_queue",
                "add_to_playlist",
                "player_output_devices",
                "player_equalizer",
                "player_fullscreen",
                "player_bass_boost",
                "player_repeat",
                "player_shuffle",
                "player_copy_link",
                "close_player",
            ]
        );
    }
}
