//! Which action a key press runs on the current screen (Windows
//! `handle_shortcut_message` and `run_shortcut_action`).

use apricot_core::{
    ActionScope, RepeatPolicy,
    shortcut::{
        ShortcutChord, ShortcutContext, ShortcutKey, action_for_shortcut,
        background_player_action_for_shortcut, focused_action_for_shortcut,
    },
};

use crate::{Shell, View};

/// What has keyboard focus when a key is pressed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusKind {
    /// The screen's list.
    List,
    /// The player surface (Windows video host).
    PlayerSurface,
    /// A push button of the player page.
    PlayerButton,
    /// A check box of the player page.
    PlayerCheckbox,
    /// A push button of a list screen (Open, Back, ...).
    Button,
    /// The read-only text of the player details.
    DetailsText,
    /// An editable text field, including AppKit's shared field editor.
    TextEntry,
    /// Anything else.
    Other,
}

/// What the UI does with a key press.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyRoute {
    /// The focused control handles the key itself.
    Unhandled,
    /// The key is consumed without an action (a held key that must not
    /// repeat its action).
    Consumed,
    /// The key runs this action through [`Shell::activate_action`].
    Run(&'static str),
    /// The key starts a held-key repeat through [`Shell::start_key_repeat`].
    Hold(&'static str, ShortcutChord),
}

impl Shell {
    /// The route of a key press; `repeat` is the system's auto-repeat.
    #[must_use]
    pub fn route_key(&self, chord: ShortcutChord, repeat: bool, focus: FocusKind) -> KeyRoute {
        let plain = !chord.control && !chord.shift && !chord.alt;
        // Ctrl+A (Command+A) in a Spotify list marks the whole list.
        if self.view == View::SpotifyBrowse
            && focus == FocusKind::List
            && chord.control
            && !chord.shift
            && !chord.alt
            && matches!(chord.key, ShortcutKey::Character('a' | 'A'))
        {
            return if repeat {
                KeyRoute::Consumed
            } else {
                KeyRoute::Run("spotify_mark_all")
            };
        }
        if plain && chord.key == ShortcutKey::Escape && self.view != View::MainMenu {
            return if repeat {
                KeyRoute::Consumed
            } else {
                KeyRoute::Run("player_back")
            };
        }
        // Python `activate_focused_button_from_key` and the native Space of
        // buttons and check boxes: the focused control acts itself.
        if plain
            && ((chord.key == ShortcutKey::Enter
                && matches!(focus, FocusKind::PlayerButton | FocusKind::Button))
                || (chord.key == ShortcutKey::Space
                    && matches!(focus, FocusKind::PlayerButton | FocusKind::PlayerCheckbox)))
        {
            return KeyRoute::Unhandled;
        }
        // Python `details_text_navigation_key`: reading keys stay in the
        // details text instead of seeking or changing volume.
        if focus == FocusKind::DetailsText && details_text_navigation_key(chord) {
            return KeyRoute::Unhandled;
        }
        let shortcuts = &self.application.settings().keyboard_shortcuts;
        // Windows `focus_in_background_player`: the player and its buttons
        // below another screen take the player's keys (beta 18).
        let background_focus = self.view != View::Player
            && self.application.player_session().is_open()
            && matches!(
                focus,
                FocusKind::PlayerSurface | FocusKind::PlayerButton | FocusKind::PlayerCheckbox
            );
        // Python `on_char_hook`: on the search screen's field, choices and
        // buttons Ctrl and Alt shortcuts act on the selected result, while
        // plain keys stay with the control. Its result list takes every list
        // shortcut.
        let search_field_focus = matches!(
            self.view,
            View::Search
                | View::Results
                | View::Collection
                | View::AudiovaultSearch
                | View::SpotifyBrowse
        ) && focus != FocusKind::List
            && !background_focus;
        let entry_focus = !background_focus
            && (focus == FocusKind::TextEntry
                || self.view == View::DirectLink
                || (search_field_focus && !(chord.control || chord.alt)));
        let (scope, accepts_text) = if background_focus {
            (ActionScope::Player, false)
        } else if entry_focus {
            (ActionScope::Dialog, true)
        } else if search_field_focus {
            (ActionScope::List, false)
        } else {
            view_shortcut_scope(self.view)
        };
        // Beta 17: the Spotify-only player keys apply while a Spotify song plays.
        let session = self.application.player_session();
        let spotify = session.is_open()
            && session
                .current_item()
                .is_some_and(|item| item.source == apricot_core::MediaSource::Spotify);
        let context = ShortcutContext::new(scope, accepts_text).with_spotify(spotify);
        // Beta 4: a focused Spotify row owns its keys before global
        // shortcuts, and the transport keys reach a player that keeps
        // playing behind another screen.
        let resolve = if self.view == View::SpotifyBrowse && focus == FocusKind::List {
            focused_action_for_shortcut
        } else {
            action_for_shortcut
        };
        let background_player =
            self.view != View::Player && self.application.player_session().is_open();
        let Some(action) = resolve(shortcuts, chord, context).or_else(|| {
            background_player
                .then(|| background_player_action_for_shortcut(shortcuts, chord, context))
                .flatten()
        }) else {
            return KeyRoute::Unhandled;
        };
        let is_global = action.scopes.contains(&ActionScope::Global);
        if !is_global && entry_focus {
            return KeyRoute::Unhandled;
        }
        if !is_global
            && !background_focus
            && self.view == View::MainMenu
            && action.id.as_str() != "open_selected"
            && !action.scopes.contains(&ActionScope::Player)
        {
            return KeyRoute::Unhandled;
        }
        match action.repeat {
            RepeatPolicy::None | RepeatPolicy::Controlled if repeat => KeyRoute::Consumed,
            RepeatPolicy::Controlled => KeyRoute::Hold(action.id.as_str(), chord),
            _ => KeyRoute::Run(action.id.as_str()),
        }
    }
}

/// Windows `details_text_navigation_key`.
const fn details_text_navigation_key(chord: ShortcutChord) -> bool {
    matches!(
        chord.key,
        ShortcutKey::Up
            | ShortcutKey::Down
            | ShortcutKey::Left
            | ShortcutKey::Right
            | ShortcutKey::Home
            | ShortcutKey::End
            | ShortcutKey::PageUp
            | ShortcutKey::PageDown
    ) || (chord.control && matches!(chord.key, ShortcutKey::Character('c' | 'C' | 'a' | 'A')))
}

/// Windows `view_shortcut_scope`.
const fn view_shortcut_scope(view: View) -> (ActionScope, bool) {
    match view {
        View::DirectLink => (ActionScope::Dialog, true),
        View::MainMenu
        | View::Search
        | View::Results
        | View::Collection
        | View::Trending
        | View::Favorites
        | View::History
        | View::UserPlaylists
        | View::UserPlaylistItems
        | View::LocalFolder
        | View::RssFeeds
        | View::RssItems
        | View::PodcastSearchResults
        | View::PodcastCategories
        | View::DownloadQueue
        | View::AudiovaultMenu
        | View::AudiovaultSearch
        | View::AudiovaultResults
        | View::SpotifyHub
        | View::SpotifyAccounts
        | View::SpotifyBrowse
        | View::Subscriptions
        | View::NotificationCenter => (ActionScope::List, false),
        View::Player => (ActionScope::Player, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell() -> Shell {
        let directory = tempfile::tempdir().unwrap();
        let paths = apricot_storage::SettingsPaths::for_app_data(
            directory.path(),
            &directory.path().join("legacy"),
        );
        let settings = apricot_app::SettingsController::load(
            paths,
            apricot_storage::SettingsDocument::default(),
        );
        Shell::new(apricot_app::Application::new(
            settings,
            apricot_app::MainMenuAvailability::default(),
        ))
    }

    /// Beta 16: every Spotify shortcut runs with this platform's default
    /// chord (Command for Ctrl on macOS, as in the main menu), from the
    /// main menu, the Spotify hub and a focused Spotify list.
    #[test]
    fn every_spotify_shortcut_runs_with_the_platform_default_chord() {
        let mut shell = shell();
        let spotify: Vec<_> = apricot_core::action::ACTIONS
            .iter()
            .filter(|action| {
                action.id.as_str().contains("spotify")
                    && !apricot_core::action::SPOTIFY_PLAYER_ACTIONS.contains(&action.id.as_str())
            })
            .collect();
        assert_eq!(spotify.len(), 12);
        for action in spotify {
            let chord = ShortcutChord::parse(action.default_shortcut()).unwrap();
            let global = action.scopes.contains(&ActionScope::Global);
            for view in [View::MainMenu, View::SpotifyHub, View::SpotifyBrowse] {
                if !global && view != View::SpotifyBrowse {
                    continue;
                }
                shell.view = view;
                assert_eq!(
                    shell.route_key(chord, false, FocusKind::List),
                    KeyRoute::Run(action.id.as_str()),
                    "{} in {view:?}",
                    action.id.as_str()
                );
            }
        }
    }

    fn media(id: &str, source: apricot_core::MediaSource) -> apricot_core::MediaItem {
        apricot_core::MediaItem {
            id: apricot_core::MediaId(id.into()),
            source,
            kind: apricot_core::MediaKind::Audio,
            title: "Title".into(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: std::collections::BTreeMap::new(),
        }
    }

    /// Beta 17: while a Spotify song plays, `Ctrl+Shift+PageUp` and
    /// `PageDown` (Command on macOS) go to the first and last song of its
    /// list, in the player and behind another screen; otherwise `PageDown`
    /// stays the next related video.
    #[test]
    fn spotify_list_edges_follow_the_playing_item() {
        let first = ShortcutChord::parse(
            apricot_core::action::action_by_id("spotify_first_track")
                .unwrap()
                .default_shortcut(),
        )
        .unwrap();
        let last = ShortcutChord::parse(
            apricot_core::action::action_by_id("spotify_last_track")
                .unwrap()
                .default_shortcut(),
        )
        .unwrap();
        let mut shell = shell();
        shell.view = View::Player;
        shell
            .application
            .start_player_item(media("dQw4w9WgXcQ", apricot_core::MediaSource::Youtube));
        assert_eq!(
            shell.route_key(last, false, FocusKind::PlayerSurface),
            KeyRoute::Run("player_next_related")
        );
        assert_eq!(
            shell.route_key(first, false, FocusKind::PlayerSurface),
            KeyRoute::Unhandled
        );
        shell.application.start_player_item(media(
            "spotify:track:4u7EnebtmKWzUH433cf5Qv",
            apricot_core::MediaSource::Spotify,
        ));
        for (view, focus) in [
            (View::Player, FocusKind::PlayerSurface),
            (View::SpotifyBrowse, FocusKind::List),
        ] {
            shell.view = view;
            assert_eq!(
                shell.route_key(first, false, focus),
                KeyRoute::Run("spotify_first_track"),
                "{view:?}"
            );
            assert_eq!(
                shell.route_key(last, false, focus),
                KeyRoute::Run("spotify_last_track"),
                "{view:?}"
            );
        }
    }

    /// Beta 18 (Windows `focus_in_background_player`): below another screen
    /// the player and its buttons take the player's keys, so the arrows seek
    /// there; Space and Return still press a focused button.
    #[test]
    fn the_background_player_takes_player_keys_on_any_screen() {
        let mut shell = shell();
        shell
            .application
            .start_player_item(media("dQw4w9WgXcQ", apricot_core::MediaSource::Youtube));
        let key = |name: &str| ShortcutChord::parse(name).unwrap();
        for view in [
            View::MainMenu,
            View::Search,
            View::SpotifyBrowse,
            View::DirectLink,
        ] {
            shell.view = view;
            for focus in [FocusKind::PlayerSurface, FocusKind::PlayerButton] {
                assert_eq!(
                    shell.route_key(key("Left"), false, focus),
                    KeyRoute::Hold("player_seek_back", key("Left")),
                    "{view:?} {focus:?}"
                );
                assert_eq!(
                    shell.route_key(key("Up"), false, focus),
                    KeyRoute::Run("player_volume_up"),
                    "{view:?} {focus:?}"
                );
            }
            assert_eq!(
                shell.route_key(key("Space"), false, FocusKind::PlayerSurface),
                KeyRoute::Run("player_play_pause"),
                "{view:?}"
            );
            assert_eq!(
                shell.route_key(key("Space"), false, FocusKind::PlayerButton),
                KeyRoute::Unhandled,
                "{view:?}"
            );
        }
        // The list itself keeps its own keys.
        shell.view = View::MainMenu;
        assert_eq!(
            shell.route_key(key("Left"), false, FocusKind::List),
            KeyRoute::Unhandled
        );
    }

    #[test]
    fn spotify_search_field_keeps_native_submit_delete_and_typing() {
        let mut shell = shell();
        shell.view = View::SpotifyBrowse;
        for key in ["Enter", "Delete", "Shift+A"] {
            assert_eq!(
                shell.route_key(ShortcutChord::parse(key).unwrap(), false, FocusKind::Other),
                KeyRoute::Unhandled,
                "{key} belongs to the search field"
            );
        }
    }

    #[test]
    fn spotify_search_field_types_every_letter() {
        // Beta 14 report: N in the Spotify search field opened the
        // notification center. No Apricot shortcut may take a typed letter.
        let mut shell = shell();
        shell.view = View::SpotifyBrowse;
        for letter in 'A'..='Z' {
            for key in [letter.to_string(), format!("Shift+{letter}")] {
                for focus in [FocusKind::TextEntry, FocusKind::Other] {
                    assert_eq!(
                        shell.route_key(ShortcutChord::parse(&key).unwrap(), false, focus),
                        KeyRoute::Unhandled,
                        "{key} with {focus:?} belongs to the search field"
                    );
                }
            }
        }
    }

    #[test]
    fn editable_fields_keep_native_keys_and_allow_global_navigation() {
        let mut shell = shell();
        shell.view = View::SpotifyBrowse;
        for key in [
            "Enter",
            "Delete",
            "Shift+A",
            "Ctrl+A",
            "Ctrl+Left",
            "Ctrl+Shift+I",
        ] {
            assert_eq!(
                shell.route_key(
                    ShortcutChord::parse(key).unwrap(),
                    false,
                    FocusKind::TextEntry
                ),
                KeyRoute::Unhandled,
                "{key} belongs to the editable field"
            );
        }
        assert_eq!(
            shell.route_key(
                ShortcutChord::parse("Ctrl+Alt+Y").unwrap(),
                false,
                FocusKind::TextEntry
            ),
            KeyRoute::Run("open_search")
        );
        assert_eq!(
            shell.route_key(
                ShortcutChord::parse("Escape").unwrap(),
                false,
                FocusKind::TextEntry
            ),
            KeyRoute::Run("player_back")
        );
        assert_eq!(
            shell.route_key(
                ShortcutChord::parse("Shift+A").unwrap(),
                false,
                FocusKind::List
            ),
            KeyRoute::Run("queue_audio")
        );
    }
}
