//! Which action a key press runs on the current screen (Windows
//! `handle_shortcut_message` and `run_shortcut_action`).

use apricot_core::{
    ActionScope, RepeatPolicy,
    shortcut::{ShortcutChord, ShortcutContext, ShortcutKey, action_for_shortcut},
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
        let shortcuts = &self.application.settings().keyboard_shortcuts;
        let (scope, accepts_text) = view_shortcut_scope(self.view);
        let Some(action) =
            action_for_shortcut(shortcuts, chord, ShortcutContext::new(scope, accepts_text))
        else {
            return KeyRoute::Unhandled;
        };
        let is_global = action.scopes.contains(&ActionScope::Global);
        if !is_global && self.view == View::MainMenu && action.id.as_str() != "open_selected" {
            return KeyRoute::Unhandled;
        }
        match action.repeat {
            RepeatPolicy::None | RepeatPolicy::Controlled if repeat => KeyRoute::Consumed,
            RepeatPolicy::Controlled => KeyRoute::Hold(action.id.as_str(), chord),
            _ => KeyRoute::Run(action.id.as_str()),
        }
    }
}

/// Windows `view_shortcut_scope`.
const fn view_shortcut_scope(view: View) -> (ActionScope, bool) {
    match view {
        View::MainMenu | View::LocalFolder => (ActionScope::List, false),
        View::Player => (ActionScope::Player, false),
    }
}
