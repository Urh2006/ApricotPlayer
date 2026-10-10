//! Held player keys (Windows `start_controlled_repeat`,
//! `tick_controlled_repeat` and `controlled_repeat_timing`): one immediate
//! step, a delay, then steps at an interval until the key is released, the
//! player closes or the screen changes. Never a backlog of key events.

use apricot_core::shortcut::ShortcutChord;

use crate::{Shell, ShellUi, View};

/// Windows `SEEK_HOLD_DELAY_MS` and `SEEK_HOLD_INTERVAL_MS`.
const SEEK_HOLD_DELAY_MS: u32 = 180;
const SEEK_HOLD_INTERVAL_MS: u32 = 110;

/// Which keys went up in one key event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // One flag per key.
pub struct KeyRelease {
    /// The held key itself.
    pub main_key: bool,
    /// Ctrl on Windows, Command on macOS.
    pub primary: bool,
    pub shift: bool,
    /// Alt on Windows, Option on macOS.
    pub alt: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct KeyRepeat {
    action_id: &'static str,
    chord: ShortcutChord,
}

/// Windows `controlled_repeat_timing`: (first delay, interval) in ms.
pub(crate) fn controlled_repeat_timing(
    action_id: &str,
    configured_delay: i64,
    configured_interval: i64,
) -> (u32, u32) {
    if matches!(
        action_id,
        "player_speed_up" | "player_speed_down" | "player_pitch_up" | "player_pitch_down"
    ) {
        return (
            u32::try_from(configured_delay.clamp(50, 1_000)).unwrap_or(180),
            u32::try_from(configured_interval.clamp(20, 500)).unwrap_or(110),
        );
    }
    (SEEK_HOLD_DELAY_MS, SEEK_HOLD_INTERVAL_MS)
}

impl Shell {
    fn repeat_timing(&self, action_id: &str) -> (u32, u32) {
        let settings = self.application.settings();
        controlled_repeat_timing(
            action_id,
            settings.speed_pitch_hold_delay_ms,
            settings.speed_pitch_hold_interval_ms,
        )
    }

    /// A held-key action was pressed: it runs once now and repeats while
    /// the key stays down.
    pub fn start_key_repeat(
        &mut self,
        ui: &mut dyn ShellUi,
        action_id: &'static str,
        chord: ShortcutChord,
    ) {
        self.stop_key_repeat(ui);
        self.key_repeat = Some(KeyRepeat { action_id, chord });
        let delay = self.repeat_timing(action_id).0;
        self.activate_action(ui, action_id);
        if self.key_repeat.is_some() {
            ui.schedule_key_repeat(delay.max(1));
        }
    }

    /// The repeat timer fired.
    pub fn key_repeat_tick(&mut self, ui: &mut dyn ShellUi) {
        let Some(repeat) = self.key_repeat else {
            return;
        };
        if self.view != View::Player || !self.application.player_session().is_open() {
            self.stop_key_repeat(ui);
            return;
        }
        let interval = self.repeat_timing(repeat.action_id).1;
        ui.schedule_key_repeat(interval.max(1));
        self.activate_action(ui, repeat.action_id);
    }

    /// A key or modifier went up: the repeat stops when it was the held
    /// key (`main_key`) or one of the chord's modifiers.
    pub fn key_released(&mut self, ui: &mut dyn ShellUi, released: KeyRelease) {
        let Some(repeat) = self.key_repeat else {
            return;
        };
        if released.main_key
            || (repeat.chord.control && released.primary)
            || (repeat.chord.shift && released.shift)
            || (repeat.chord.alt && released.alt)
        {
            self.stop_key_repeat(ui);
        }
    }

    /// Windows `stop_controlled_repeat`.
    pub fn stop_key_repeat(&mut self, ui: &mut dyn ShellUi) {
        if self.key_repeat.take().is_some() {
            ui.cancel_key_repeat();
        }
    }

    /// Whether a held key is repeating, so its own auto-repeat key events
    /// are consumed.
    #[must_use]
    pub const fn is_repeating(&self) -> bool {
        self.key_repeat.is_some()
    }
}
