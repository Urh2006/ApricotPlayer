use std::time::{Duration, Instant};

use apricot_app::MainMenuItem;

use super::{TypeAhead, last_activated_index, preserved_index};

mod keyboard {
    use apricot_core::shortcut::ShortcutChord;

    use crate::{FocusKind, KeyRoute, Shell};

    pub(super) fn shell() -> Shell {
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
            shell.route_key(chord("Enter"), false, FocusKind::List),
            KeyRoute::Run("open_selected")
        );
        assert_eq!(
            shell.route_key(chord("Ctrl+Alt+Y"), false, FocusKind::List),
            KeyRoute::Run("open_search")
        );
        assert_eq!(
            shell.route_key(chord("Ctrl+Alt+Y"), true, FocusKind::List),
            KeyRoute::Consumed
        );
        assert_eq!(
            shell.route_key(chord("Escape"), false, FocusKind::List),
            KeyRoute::Unhandled
        );
        // A list action is not a main menu action.
        assert_eq!(
            shell.route_key(chord("Ctrl+Shift+A"), false, FocusKind::List),
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

mod equalizer {
    use std::path::PathBuf;

    use apricot_app::PlayerScreenModel;
    use apricot_playback::MpvVideoMode;

    use crate::{
        ChoiceDialog, DetailsLabels, EqualizerButton, EqualizerView, ListScreen, PlayerFocus,
        ShellUi,
    };

    /// A main window that records announcements.
    #[derive(Default)]
    pub(super) struct Ui {
        pub(super) announcements: Vec<String>,
        pub(super) bookmarks: Option<crate::BookmarkDialog>,
        /// What a text prompt returns; `None` cancels.
        pub(super) prompt_answer: Option<String>,
    }

    impl ShellUi for Ui {
        fn set_status(&mut self, message: &str, announce: bool) {
            if announce {
                self.announcements.push(message.to_owned());
            }
        }
        fn show_error(&mut self, _message: &str) {}
        fn show_list_screen(&mut self, _screen: &ListScreen) {}
        fn show_player(&mut self, _model: &PlayerScreenModel, _focus: PlayerFocus) {}
        fn show_player_details(&mut self, _labels: &DetailsLabels, _text: &str) {}
        fn hide_player_details(&mut self) {}
        fn set_player_details_text(&mut self, _text: &str) {}
        fn focus_player_control(&mut self, _id: &str) {}
        fn set_window_fullscreen(&mut self, _fullscreen: bool) {}
        fn choose(&mut self, _dialog: &ChoiceDialog) -> Option<usize> {
            None
        }
        fn screen_text(&self, _id: &str) -> String {
            String::new()
        }
        fn screen_choice(&self, _id: &str) -> Option<usize> {
            None
        }
        fn set_screen_choices(&mut self, _id: &str, _options: &[String], _selected: usize) {}
        fn focus_screen_control(&mut self, _id: &str) {}
        fn set_list_rows(&mut self, _rows: Vec<String>, _selected: usize) {}
        fn open_equalizer(&mut self) {}
        fn open_lyrics(&mut self, _dialog: crate::LyricsDialog) {}
        fn open_transcript(&mut self, _dialog: crate::TranscriptDialog) {}
        fn open_comments(&mut self, _dialog: crate::CommentsDialog) {}
        fn open_playback_queue(&mut self, _dialog: crate::QueueDialog) {}
        fn show_context_menu(
            &mut self,
            _entries: &[apricot_app::context_menu::ContextMenuEntry],
        ) -> Option<apricot_app::context_menu::ContextCommand> {
            None
        }
        fn open_bookmarks(&mut self, dialog: crate::BookmarkDialog) {
            self.bookmarks = Some(dialog);
        }
        fn prompt_text(
            &mut self,
            _title: &str,
            _prompt: &str,
            _initial: &str,
            _ok: &str,
            _cancel: &str,
        ) -> Option<String> {
            self.prompt_answer.clone()
        }
        fn copy_text(&mut self, _text: &str) -> bool {
            true
        }
        fn set_window_title(&mut self, _title: &str) {}
        fn selected_row(&self) -> Option<usize> {
            None
        }
        fn focused_player_control(&self) -> Option<&'static str> {
            None
        }
        fn focus_primary_control(&mut self) {}
        fn video_mode(&self) -> MpvVideoMode {
            MpvVideoMode::AudioOnly
        }
        fn set_playback_polling(&mut self, _active: bool) {}
        fn schedule_key_repeat(&mut self, _delay_ms: u32) {}
        fn cancel_key_repeat(&mut self) {}
        fn play_default_reached_sound(&mut self) {}
        fn choose_media_file(&mut self, _title: &str) -> Result<Option<PathBuf>, String> {
            Ok(None)
        }
        fn choose_folder(&mut self, _title: &str) -> Option<PathBuf> {
            None
        }
        fn set_background_polling(&mut self, _active: bool) {}
        fn restore(&mut self) {}
        fn exit(&mut self) {}
    }

    /// The dialog controls as the shell left them.
    #[derive(Default)]
    struct View {
        presets: Vec<String>,
        preset: usize,
        range: usize,
        name: String,
        name_visible: bool,
        /// Per band: limit, position, label, value text, spoken.
        sliders: Vec<(i32, i32, String, String, bool)>,
        scheduled: Option<u32>,
        closed: bool,
        prompt_answer: Option<String>,
        message_answer: usize,
        messages: Vec<String>,
    }

    impl EqualizerView for View {
        fn set_presets(&mut self, labels: &[String], selected: usize) {
            self.presets = labels.to_vec();
            self.preset = selected;
        }
        fn selected_preset(&self) -> Option<usize> {
            Some(self.preset)
        }
        fn set_range(&mut self, index: usize) {
            self.range = index;
        }
        fn selected_range(&self) -> Option<usize> {
            Some(self.range)
        }
        fn set_name(&mut self, text: &str) {
            text.clone_into(&mut self.name);
        }
        fn name(&self) -> String {
            self.name.clone()
        }
        fn set_name_visible(&mut self, visible: bool) {
            self.name_visible = visible;
        }
        fn set_slider(
            &mut self,
            index: usize,
            limit: i32,
            tenths: i32,
            label: &str,
            value: &str,
            notify: bool,
        ) {
            if self.sliders.len() <= index {
                self.sliders.resize(index + 1, Default::default());
            }
            self.sliders[index] = (limit, tenths, label.to_owned(), value.to_owned(), notify);
        }
        fn schedule_apply(&mut self, delay_ms: u32) {
            self.scheduled = Some(delay_ms);
        }
        fn cancel_apply(&mut self) {
            self.scheduled = None;
        }
        fn close(&mut self) {
            self.closed = true;
        }
        fn message(&mut self, _title: &str, message: &str, _buttons: &[&str]) -> usize {
            self.messages.push(message.to_owned());
            self.message_answer
        }
        fn prompt_name(
            &mut self,
            _title: &str,
            _prompt: &str,
            _ok: &str,
            _cancel: &str,
        ) -> Option<String> {
            self.prompt_answer.clone()
        }
        fn choose(&mut self, _dialog: &ChoiceDialog) -> Option<usize> {
            None
        }
        fn choose_json_file(&mut self, _title: &str, _type_label: &str) -> Option<PathBuf> {
            None
        }
        fn save_json_file(
            &mut self,
            _title: &str,
            _file_name: &str,
            _type_label: &str,
        ) -> Option<PathBuf> {
            None
        }
    }

    #[test]
    fn the_dialog_opens_with_the_saved_preset_range_and_flat_sliders() {
        let mut shell = super::keyboard::shell();
        let mut view = View::default();
        shell.equalizer_begin(&mut view);
        assert!(!view.presets.is_empty());
        assert_eq!(view.range, 1, "12 dB is the second range option");
        assert!(!view.name_visible, "a factory preset has no name field");
        assert_eq!(
            view.sliders.len(),
            apricot_core::audio::EQUALIZER_BANDS.len()
        );
        for (limit, position, label, value, spoken) in &view.sliders {
            assert_eq!((*limit, *position, *spoken), (120, 0, false));
            assert!(!label.is_empty() && !value.is_empty());
        }
        assert_eq!(view.scheduled, None, "opening applies nothing");
    }

    #[test]
    fn a_slider_change_is_spoken_and_applied_after_the_delay() {
        let mut shell = super::keyboard::shell();
        let mut view = View::default();
        shell.equalizer_begin(&mut view);
        shell.equalizer_slider_changed(&mut view, 2, 30);
        let (_, position, _, value, spoken) = &view.sliders[2];
        assert_eq!((*position, *spoken), (30, true));
        assert_eq!(value, &apricot_app::equalizer::slider_value_text(30));
        assert_eq!(view.scheduled, Some(apricot_app::equalizer::APPLY_DELAY_MS));
    }

    #[test]
    fn a_smaller_range_limits_the_sliders() {
        let mut shell = super::keyboard::shell();
        let mut view = View::default();
        shell.equalizer_begin(&mut view);
        shell.equalizer_slider_changed(&mut view, 0, 110);
        view.range = 0;
        shell.equalizer_range_changed(&mut view);
        assert_eq!(view.sliders[0].0, 60);
        assert_eq!(view.sliders[0].1, 60);
    }

    #[test]
    fn ok_keeps_the_range_and_cancel_restores_it() {
        let mut ui = Ui::default();
        let mut shell = super::keyboard::shell();
        let mut view = View::default();
        shell.equalizer_begin(&mut view);
        view.range = 3;
        shell.equalizer_range_changed(&mut view);
        shell.equalizer_button(&mut ui, &mut view, EqualizerButton::Ok);
        assert!(view.closed);
        shell.equalizer_finish(&mut ui);
        assert_eq!(shell.application.equalizer_settings().db_range(), 24);
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("equalizer_saved"))
        );

        let mut view = View::default();
        shell.equalizer_begin(&mut view);
        assert_eq!(view.range, 3, "the saved range is shown");
        view.range = 0;
        shell.equalizer_range_changed(&mut view);
        shell.equalizer_button(&mut ui, &mut view, EqualizerButton::Cancel);
        assert!(view.closed);
        shell.equalizer_finish(&mut ui);
        assert_eq!(shell.application.equalizer_settings().db_range(), 24);
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("equalizer_closed"))
        );
    }

    #[test]
    fn a_new_profile_is_selected_named_and_deleted_only_after_yes() {
        let mut ui = Ui::default();
        let mut shell = super::keyboard::shell();
        let mut view = View::default();
        shell.equalizer_begin(&mut view);
        view.prompt_answer = Some("Mine".to_owned());
        shell.equalizer_button(&mut ui, &mut view, EqualizerButton::AddProfile);
        assert_eq!(view.presets[view.preset], "Mine");
        assert_eq!(view.name, "Mine");
        assert!(view.name_visible);
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("equalizer_profile_saved"))
        );

        view.message_answer = 1;
        shell.equalizer_button(&mut ui, &mut view, EqualizerButton::DeleteProfile);
        assert_eq!(view.presets[view.preset], "Mine", "No keeps the profile");
        view.message_answer = 0;
        shell.equalizer_button(&mut ui, &mut view, EqualizerButton::DeleteProfile);
        assert!(!view.presets.contains(&"Mine".to_owned()));
        assert!(!view.name_visible);
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("equalizer_profile_deleted"))
        );
    }
}

mod player_tools {
    use super::equalizer::Ui;

    #[test]
    fn replaygain_cycles_saves_and_announces_without_a_player() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        let before = shell.application.settings().replaygain_mode.clone();
        shell.activate_action(&mut ui, "player_replaygain");
        let after = shell.application.settings().replaygain_mode.clone();
        assert_ne!(before, after);
        let label = shell.text(apricot_app::player_model::replaygain_mode_key(&after));
        assert_eq!(
            ui.announcements,
            [shell
                .text("audio_normalization_changed")
                .replace("{mode}", &label)]
        );
    }

    #[test]
    fn player_tools_without_a_player_say_why() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.activate_action(&mut ui, "player_bpm");
        shell.activate_action(&mut ui, "player_chapters");
        shell.activate_action(&mut ui, "player_copy_timestamp_link");
        shell.copy_active_location(&mut ui);
        shell.copy_active_stream_url(&mut ui);
        assert_eq!(
            ui.announcements,
            [
                shell.text("bpm_not_available"),
                shell.text("no_player"),
                shell.text("timestamp_url_unavailable"),
                shell.text("no_selection"),
                shell.text("no_selection"),
            ]
        );
    }
}

mod bookmarks {
    use super::equalizer::Ui;
    use crate::{BookmarkRequest, BookmarkResponse};

    #[test]
    fn the_bookmarks_window_opens_from_the_main_menu_and_is_empty() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.activate_action(&mut ui, "open_bookmarks");
        let dialog = ui.bookmarks.take().expect("bookmarks window");
        assert!(dialog.entries.is_empty());
        assert!(!dialog.can_add, "nothing plays");
        assert!(!dialog.current_only && !dialog.return_on_close);
        assert_eq!(dialog.empty, shell.text("bookmarks_empty"));
        shell.activate_action(&mut ui, "player_add_bookmark");
        assert_eq!(ui.announcements, [shell.text("no_player")]);
    }

    #[test]
    fn rename_and_delete_refresh_the_list() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        let bookmark = shell
            .application
            .add_bookmark(
                "Start",
                65.0,
                crate::local_file_item(std::path::Path::new("/tmp/a.mp3")),
                1.0,
            )
            .expect("saved")
            .expect("added");
        ui.prompt_answer = Some("Chorus".to_owned());
        let response =
            shell.bookmark_request(&mut ui, false, BookmarkRequest::Rename(bookmark.id.clone()));
        let BookmarkResponse::Refresh {
            entries,
            selected_id,
        } = response
        else {
            panic!("rename refreshes the list");
        };
        assert_eq!(selected_id.as_deref(), Some(bookmark.id.as_str()));
        assert!(
            entries[0].label.starts_with("1. 1:05 | Chorus"),
            "{entries:?}"
        );
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("bookmark_renamed").replace("{name}", "Chorus"))
        );

        let response = shell.bookmark_request(&mut ui, false, BookmarkRequest::Delete(bookmark.id));
        assert_eq!(
            response,
            BookmarkResponse::Refresh {
                entries: Vec::new(),
                selected_id: None
            }
        );
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("bookmark_deleted"))
        );
    }
}
