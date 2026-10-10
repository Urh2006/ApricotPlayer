use std::time::{Duration, Instant};

use apricot_app::MainMenuItem;

use super::{TypeAhead, last_activated_index, preserved_index};

mod keyboard {
    use apricot_core::shortcut::ShortcutChord;

    use crate::{KeyRoute, Shell};

    fn shell() -> Shell {
        let directory = std::env::temp_dir().join(format!(
            "apricot-shell-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |time| time.as_nanos())
        ));
        let paths =
            apricot_storage::SettingsPaths::for_app_data(&directory, &directory.join("legacy"));
        let defaults = apricot_storage::SettingsDocument::with_platform_defaults(
            directory.join("Downloads").to_string_lossy().into_owned(),
            directory.join("cache").to_string_lossy().into_owned(),
            "beta",
        );
        let settings = apricot_app::SettingsController::load(paths, defaults);
        Shell::new(apricot_app::Application::new(
            settings,
            apricot_app::MainMenuAvailability::default(),
        ))
    }

    #[test]
    fn the_main_menu_runs_global_shortcuts_and_enter_only() {
        let shell = shell();
        let chord = |text: &str| ShortcutChord::parse(text).expect("chord");
        assert_eq!(
            shell.route_key(chord("Enter"), false),
            KeyRoute::Run("open_selected")
        );
        assert_eq!(
            shell.route_key(chord("Ctrl+Alt+Y"), false),
            KeyRoute::Run("open_search")
        );
        assert_eq!(
            shell.route_key(chord("Ctrl+Alt+Y"), true),
            KeyRoute::Consumed
        );
        assert_eq!(shell.route_key(chord("Escape"), false), KeyRoute::Unhandled);
        // A list action is not a main menu action.
        assert_eq!(
            shell.route_key(chord("Ctrl+Shift+A"), false),
            KeyRoute::Unhandled
        );
    }
}

const ROWS: [&str; 9] = [
    "Search YouTube / SoundCloud",
    "Play file",
    "Favorites",
    "History",
    "Playlists",
    "Podcasts and RSS",
    "Bookmarks",
    "Settings",
    "Exit",
];

#[test]
fn the_same_letter_again_cycles_through_its_rows() {
    let mut typing = TypeAhead::default();
    let start = Instant::now();
    let first = typing.type_character(&ROWS, Some(0), 'p', start);
    assert_eq!(first, Some(1));
    let second = typing.type_character(&ROWS, first, 'p', start + Duration::from_millis(200));
    assert_eq!(second, Some(4));
    let third = typing.type_character(&ROWS, second, 'P', start + Duration::from_millis(400));
    assert_eq!(third, Some(5));
    let wrapped = typing.type_character(&ROWS, third, 'p', start + Duration::from_millis(600));
    assert_eq!(wrapped, Some(1));
}

#[test]
fn different_letters_search_by_the_start_of_the_row() {
    let mut typing = TypeAhead::default();
    let start = Instant::now();
    assert_eq!(typing.type_character(&ROWS, Some(0), 'p', start), Some(1));
    assert_eq!(
        typing.type_character(&ROWS, Some(1), 'o', start + Duration::from_millis(150)),
        Some(5)
    );
    // "pl" from "Play file" stays on it, as the search includes the current row.
    let mut typing = TypeAhead::default();
    assert_eq!(typing.type_character(&ROWS, Some(0), 'p', start), Some(1));
    assert_eq!(
        typing.type_character(&ROWS, Some(1), 'l', start + Duration::from_millis(150)),
        Some(1)
    );
    assert_eq!(
        typing.type_character(&ROWS, Some(1), 'a', start + Duration::from_millis(300)),
        Some(1)
    );
    assert_eq!(
        typing.type_character(&ROWS, Some(1), 'y', start + Duration::from_millis(450)),
        Some(1)
    );
    assert_eq!(
        typing.type_character(&ROWS, Some(1), 'l', start + Duration::from_millis(600)),
        Some(4)
    );
}

#[test]
fn a_pause_starts_a_new_search_and_no_match_keeps_the_selection() {
    let mut typing = TypeAhead::default();
    let start = Instant::now();
    assert_eq!(typing.type_character(&ROWS, Some(0), 's', start), Some(7));
    assert_eq!(
        typing.type_character(&ROWS, Some(7), 'h', start + Duration::from_secs(2)),
        Some(3)
    );
    assert_eq!(
        typing.type_character(&ROWS, Some(3), 'z', start + Duration::from_secs(4)),
        None
    );
}

#[test]
fn main_menu_returns_to_the_last_opened_row_or_keeps_the_label() {
    let items = vec![
        MainMenuItem {
            id: "search",
            label: "Search".to_owned(),
        },
        MainMenuItem {
            id: "history",
            label: "History".to_owned(),
        },
    ];
    assert_eq!(last_activated_index(&items, Some("history")), 1);
    assert_eq!(last_activated_index(&items, Some("gone")), 0);
    assert_eq!(last_activated_index(&items, None), 0);
    let labels = ["Current downloads (1)", "Search", "History"];
    assert_eq!(preserved_index(&labels, Some("History"), Some(1)), 2);
    assert_eq!(
        preserved_index(&labels, Some("Current downloads (2)"), Some(0)),
        0
    );
    assert_eq!(preserved_index(&labels, None, Some(9)), 2);
}
