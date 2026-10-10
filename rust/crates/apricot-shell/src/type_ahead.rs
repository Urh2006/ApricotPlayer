//! Letter navigation in lists, as the Windows list box does it.
//!
//! Typing letters quickly searches for rows that start with all of them
//! ("pl" finds "Playlists"). Typing the same letter again moves to the next
//! row that starts with it and wraps around, so P, P, P cycles through every
//! row on P. AppKit's own type select only does the first, which made a
//! second P land nowhere (Urh's VoiceOver test of the spike, 11. 10. 2026).

use std::time::{Duration, Instant};

/// How long typed letters form one search, like the Windows list box.
pub const TYPE_AHEAD_TIMEOUT: Duration = Duration::from_millis(1_000);

#[derive(Debug, Default)]
pub struct TypeAhead {
    typed: String,
    last: Option<Instant>,
}

impl TypeAhead {
    /// The row to select after typing `character` while `current` is
    /// selected, or `None` when no row matches (the selection stays).
    pub fn type_character<S: AsRef<str>>(
        &mut self,
        labels: &[S],
        current: Option<usize>,
        character: char,
        now: Instant,
    ) -> Option<usize> {
        if self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) > TYPE_AHEAD_TIMEOUT)
        {
            self.typed.clear();
        }
        self.last = Some(now);
        self.typed.extend(character.to_lowercase());
        if labels.is_empty() {
            return None;
        }
        let mut letters = self.typed.chars();
        let first = letters.next()?;
        if letters.all(|letter| letter == first) {
            // One letter, or the same letter again: the next row on it.
            let start = current.map_or(0, |index| index + 1);
            return find_from(labels, start, |label| {
                starts_with(label, &first.to_string())
            });
        }
        let typed = self.typed.clone();
        find_from(labels, current.unwrap_or(0), |label| {
            starts_with(label, &typed)
        })
    }

    /// Forgets the typed letters, for example after the list changed.
    pub fn reset(&mut self) {
        self.typed.clear();
        self.last = None;
    }
}

fn starts_with(label: &str, typed: &str) -> bool {
    label.trim_start().to_lowercase().starts_with(typed)
}

/// The first matching row from `start`, wrapping around once.
fn find_from<S: AsRef<str>>(
    labels: &[S],
    start: usize,
    matches: impl Fn(&str) -> bool,
) -> Option<usize> {
    let count = labels.len();
    (0..count)
        .map(|offset| (start + offset) % count)
        .find(|index| matches(labels[*index].as_ref()))
}
