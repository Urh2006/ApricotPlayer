//! Which action a key press runs on the current screen (Windows
//! `handle_shortcut_message` and `run_shortcut_action`).

use apricot_core::{
    ActionScope, RepeatPolicy,
    shortcut::{ShortcutChord, ShortcutContext, ShortcutKey, action_for_shortcut},
};

use crate::{Shell, View};

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
}

impl Shell {
    /// The route of a key press with the screen's list focused.
    #[must_use]
    pub fn route_key(&self, chord: ShortcutChord, repeat: bool) -> KeyRoute {
        let plain = !chord.control && !chord.shift && !chord.alt;
        if plain && chord.key == ShortcutKey::Escape && self.view != View::MainMenu {
            return KeyRoute::Run("player_back");
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
        if repeat && action.repeat == RepeatPolicy::None {
            return KeyRoute::Consumed;
        }
        KeyRoute::Run(action.id.as_str())
    }
}

/// Windows `view_shortcut_scope`.
const fn view_shortcut_scope(view: View) -> (ActionScope, bool) {
    match view {
        View::MainMenu => (ActionScope::List, false),
    }
}
