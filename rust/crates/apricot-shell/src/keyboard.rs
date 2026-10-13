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
        ) && focus != FocusKind::List;
        let entry_focus = focus == FocusKind::TextEntry
            || self.view == View::DirectLink
            || (search_field_focus && !(chord.control || chord.alt));
        let (scope, accepts_text) = if entry_focus {
            (ActionScope::Dialog, true)
        } else if search_field_focus {
            (ActionScope::List, false)
        } else {
            view_shortcut_scope(self.view)
        };
        let context = ShortcutContext::new(scope, accepts_text);
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
