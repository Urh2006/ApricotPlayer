//! Row selection of the main menu (Windows `main_menu_last_activated_index`
//! and `main_menu_preserved_index`).

use apricot_app::MainMenuItem;

/// Which row a refreshed main menu selects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MainMenuSelection {
    /// Python `show_main_menu`: the last opened screen's item, else the first.
    LastActivated,
    /// Python `refresh_main_menu_download_label`: the same label, else the
    /// same row, so background updates never move the selection.
    Preserve,
}

/// The row of the last opened item, else the first row.
#[must_use]
pub fn last_activated_index(items: &[MainMenuItem], last_activated: Option<&str>) -> usize {
    last_activated
        .and_then(|id| items.iter().position(|item| item.id == id))
        .unwrap_or_default()
}

/// The row with the same label, else the same row within the new list.
#[must_use]
pub fn preserved_index(
    labels: &[&str],
    old_label: Option<&str>,
    old_selection: Option<usize>,
) -> usize {
    if let Some(index) = old_label.and_then(|old| labels.iter().position(|label| *label == old)) {
        return index;
    }
    old_selection
        .unwrap_or_default()
        .min(labels.len().saturating_sub(1))
}
