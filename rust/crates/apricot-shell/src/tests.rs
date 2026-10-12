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
        /// The list's selected row.
        pub(super) selected: Option<usize>,
        /// The list screens shown, last last.
        pub(super) screens: Vec<crate::ListScreen>,
        /// The RSS timer interval, `None` when stopped.
        pub(super) rss_timer: Option<f64>,
        /// The yes or no questions asked, and the answer given.
        pub(super) questions: Vec<String>,
        pub(super) answer_yes: bool,
        pub(super) progress: Option<crate::DownloadProgress>,
    }

    impl ShellUi for Ui {
        fn set_status(&mut self, message: &str, announce: bool) {
            if announce {
                self.announcements.push(message.to_owned());
            }
        }
        fn show_error(&mut self, _message: &str) {}
        fn show_list_screen(&mut self, screen: &ListScreen) {
            self.screens.push(screen.clone());
            self.selected = screen.selected;
        }
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
            self.selected
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
        fn choose_open_file(&mut self, _title: &str, _extensions: &[&str]) -> Option<PathBuf> {
            None
        }
        fn choose_save_file(
            &mut self,
            _title: &str,
            _directory: Option<&std::path::Path>,
            _default_name: &str,
            _extensions: &[&str],
        ) -> Option<PathBuf> {
            None
        }
        fn choose_download_folder(
            &mut self,
            _title: &str,
            _initial: &std::path::Path,
        ) -> Option<PathBuf> {
            None
        }
        fn ask_yes_no(&mut self, message: &str, _yes: &str, _no: &str) -> bool {
            self.questions.push(message.to_owned());
            self.answer_yes
        }
        fn show_download_progress(&mut self, progress: &crate::DownloadProgress) {
            self.progress = Some(progress.clone());
        }
        fn update_download_progress(&mut self, percent: u32, message: &str) {
            if let Some(progress) = self.progress.as_mut() {
                progress.percent = percent;
                message.clone_into(&mut progress.message);
            }
        }
        fn close_download_progress(&mut self) {
            self.progress = None;
        }
        fn set_list_row(&mut self, index: usize, label: &str) {
            if let Some(row) = self
                .screens
                .last_mut()
                .and_then(|screen| screen.rows.get_mut(index))
            {
                label.clone_into(row);
            }
        }
        fn app_is_active(&self) -> bool {
            true
        }
        fn open_converter(&mut self, _dialog: crate::ConverterDialog) {}
        fn open_settings(&mut self) {}
        fn audiovault_login(
            &mut self,
            _login: &crate::AudiovaultLogin,
        ) -> Option<(String, String)> {
            None
        }
        fn focus_list(&mut self) {}
        fn open_spotify_login(&mut self, _texts: &crate::SpotifyLoginTexts) {}
        fn spotify_login_update(&mut self, _update: &crate::SpotifyLoginUpdate) {}
        fn spotify_settings(
            &mut self,
            _texts: &crate::SpotifySettingsTexts,
            _current: apricot_spotify::settings::SpotifySettings,
        ) -> Option<apricot_spotify::settings::SpotifySettings> {
            None
        }
        fn spotify_search(
            &mut self,
            _texts: &crate::SpotifySearchTexts,
            _query: &str,
            _kind: usize,
        ) -> Option<(String, usize)> {
            None
        }
        fn open_spotify_dialog(&mut self, _dialog: crate::SpotifyListDialog) {}
        fn spotify_dialog_update(
            &mut self,
            _rows: Vec<crate::SpotifyDialogRow>,
            _selected: Option<usize>,
        ) {
        }
        fn settings_cookie_source(&mut self, _path: &str, _browser_index: usize) {}
        fn open_action_finder(&mut self, _model: apricot_app::ActionFinderModel) {}
        fn show_conversion_progress(&mut self, _title: &str, _message: &str, _total: usize) {}
        fn update_conversion_progress(&mut self, _converted: usize, _message: &str) {}
        fn close_conversion_progress(&mut self) {}
        fn set_subscription_timer(&mut self, _interval_seconds: Option<f64>) {}
        fn set_app_update_timer(&mut self, _interval_seconds: Option<f64>) {}
        fn choose_import_source(&mut self, _title: &str) -> Option<std::path::PathBuf> {
            None
        }
        fn schedule_startup_update_checks(&mut self, _ytdlp: Option<f64>, _app: Option<f64>) {}
        fn update_prompt(&mut self, _prompt: &crate::UpdatePrompt) -> bool {
            false
        }
        fn show_system_notification(&mut self, _title: &str, _message: &str) {}
        fn set_rss_timer(&mut self, interval_seconds: Option<f64>) {
            self.rss_timer = interval_seconds;
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

mod library {
    use super::equalizer::Ui;
    use crate::ScreenControl;

    #[test]
    fn favorites_add_play_order_and_remove_with_delete() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.activate_action(&mut ui, "open_favorites");
        let screen = ui.screens.last().expect("favorites screen").clone();
        assert_eq!(screen.rows, [shell.text("favorites_empty")]);
        assert_eq!(screen.list_name, shell.text("favorites"));
        let ids: Vec<_> = screen
            .controls
            .iter()
            .map(|control| match control {
                ScreenControl::List => "list",
                ScreenControl::Button { id, .. } => id,
                _ => "field",
            })
            .collect();
        assert_eq!(ids, ["list", "open", "back", "collection_remove"]);

        let item = crate::local_file_item(std::path::Path::new("/tmp/Song.mp3"));
        shell.application.add_favorite(item).expect("added");
        shell.activate_action(&mut ui, "open_favorites");
        assert_eq!(ui.screens.last().expect("screen").rows, ["Song"]);
        ui.selected = Some(0);
        shell.activate_action(&mut ui, "remove_selected");
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("favorite_removed"))
        );
        assert_eq!(
            ui.screens.last().expect("screen").rows,
            [shell.text("favorites_empty")]
        );
    }

    #[test]
    fn history_has_clear_and_says_cleared() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.activate_action(&mut ui, "open_history");
        let screen = ui.screens.last().expect("history screen");
        assert!(screen.controls.iter().any(|control| matches!(
            control,
            ScreenControl::Button {
                id: "history_clear",
                ..
            }
        )));
        shell.activate_screen_button(&mut ui, "history_clear");
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("history_cleared"))
        );
    }
}

mod playlists {
    use super::equalizer::Ui;
    use crate::{ListScreen, ScreenControl};

    fn ids(screen: &ListScreen) -> Vec<&'static str> {
        screen
            .controls
            .iter()
            .map(|control| match control {
                ScreenControl::List => "list",
                ScreenControl::Button { id, .. } => id,
                _ => "field",
            })
            .collect()
    }

    #[test]
    fn playlists_are_created_opened_and_emptied() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.activate_action(&mut ui, "open_playlists");
        let screen = ui.screens.last().expect("playlists").clone();
        assert_eq!(screen.rows, [shell.text("no_playlists")]);
        assert_eq!(
            ids(&screen),
            [
                "back",
                "playlist_create",
                "open",
                "playlist_download",
                "collection_remove",
                "list"
            ]
        );
        ui.prompt_answer = Some("Mix".to_owned());
        shell.activate_screen_button(&mut ui, "playlist_create");
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("playlist_created").replace("{title}", "Mix"))
        );
        let item = crate::local_file_item(std::path::Path::new("/tmp/Song.mp3"));
        shell
            .application
            .add_item_to_user_playlist(0, item, 1.0)
            .expect("added");
        shell.activate_action(&mut ui, "open_playlists");
        ui.selected = Some(0);
        shell.activate_action(&mut ui, "open_selected");
        let screen = ui.screens.last().expect("items").clone();
        assert_eq!(screen.rows, ["Song"]);
        assert_eq!(
            ids(&screen),
            [
                "back",
                "open",
                "playlist_download",
                "collection_remove",
                "list"
            ]
        );
        shell.activate_action(&mut ui, "remove_selected");
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("removed_from_playlist"))
        );
        let screen = ui.screens.last().expect("empty items").clone();
        assert_eq!(screen.rows, [shell.text("playlist_empty")]);
        assert_eq!(ids(&screen), ["back", "list"]);
    }
}

mod podcasts {
    use super::equalizer::Ui;
    use crate::{ListScreen, ScreenControl};

    fn ids(screen: &ListScreen) -> Vec<&'static str> {
        screen
            .controls
            .iter()
            .map(|control| match control {
                ScreenControl::List => "list",
                ScreenControl::Button { id, .. } => id,
                _ => "field",
            })
            .collect()
    }

    fn episode(title: &str) -> apricot_core::MediaItem {
        let mut item = crate::local_file_item(std::path::Path::new(&format!("/tmp/{title}.mp3")));
        item.kind = apricot_core::MediaKind::PodcastEpisode;
        item
    }

    #[test]
    fn the_feed_list_has_the_windows_tab_order_and_says_it_is_empty() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.activate_action(&mut ui, "open_podcasts_rss");
        let screen = ui.screens.last().expect("feeds screen");
        assert_eq!(screen.list_name, shell.text("rss_feeds"));
        assert_eq!(screen.rows, [shell.text("rss_feeds_empty")]);
        assert_eq!(
            ids(screen),
            [
                "back",
                "rss_search",
                "rss_categories",
                "rss_add",
                "rss_refresh",
                "open",
                "collection_remove",
                "rss_filter",
                "rss_set_category",
                "rss_import",
                "rss_export",
                "list"
            ]
        );
        shell.activate_screen_button(&mut ui, "rss_refresh");
        assert_eq!(
            ui.announcements.last(),
            Some(&shell.text("rss_feeds_empty"))
        );
    }

    #[test]
    fn a_feed_opens_its_episodes_marks_them_played_and_goes_back() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        let mut feed = apricot_app::RssFeed::new(
            "Show",
            "https://example.com/feed.xml",
            "",
            vec![episode("One"), episode("Two")],
            1.0,
        );
        feed.items_complete = Some(true);
        let _ = shell.application.add_rss_feed(feed).expect("added");
        shell.activate_action(&mut ui, "open_podcasts_rss");
        let row = ui.screens.last().expect("feeds").rows[0].clone();
        assert!(row.starts_with("Show | "), "{row}");
        ui.selected = Some(0);
        shell.activate_screen_button(&mut ui, "open");
        let screen = ui.screens.last().expect("episodes").clone();
        assert_eq!(screen.list_name, shell.text("rss_feed_items"));
        assert_eq!(
            ids(&screen),
            [
                "back",
                "rss_refresh",
                "open",
                "rss_download_episode",
                "rss_download_feed",
                "list"
            ]
        );
        assert!(screen.rows[1].starts_with("Two | "), "{:?}", screen.rows);
        ui.selected = Some(1);
        shell.activate_action(&mut ui, "toggle_podcast_played");
        let screen = ui.screens.last().expect("episodes");
        assert!(
            screen.rows[1].contains(&shell.text("played_marker")),
            "{:?}",
            screen.rows
        );
        assert_eq!(screen.selected, Some(1));
        shell.activate_screen_button(&mut ui, "back");
        let screen = ui.screens.last().expect("feeds");
        assert_eq!(screen.list_name, shell.text("rss_feeds"));
        assert!(screen.rows[0].contains("Show"));
    }

    #[test]
    fn the_refresh_timer_follows_the_settings() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.start(&mut ui);
        let settings = shell.application.settings();
        let expected = (settings.enable_podcasts_rss && settings.rss_auto_refresh_enabled)
            .then(|| settings.rss_refresh_interval_hours.clamp(0.5, 168.0) * 3600.0);
        assert_eq!(ui.rss_timer, expected);
    }
}

mod downloads {
    use super::equalizer::Ui;
    use crate::{ListScreen, ScreenControl};

    fn ids(screen: &ListScreen) -> Vec<&'static str> {
        screen
            .controls
            .iter()
            .map(|control| match control {
                ScreenControl::List => "list",
                ScreenControl::Button { id, .. } => id,
                _ => "field",
            })
            .collect()
    }

    fn remote(title: &str) -> apricot_core::MediaItem {
        let mut item =
            apricot_core::MediaItem::from_direct_link(&format!("https://example.com/{title}.mp3"))
                .expect("item");
        item.title = title.to_owned();
        item.kind = apricot_core::MediaKind::PodcastEpisode;
        item
    }

    #[test]
    fn the_download_screen_shows_the_queue_and_its_buttons() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.activate_action(&mut ui, "open_current_downloads");
        let screen = ui.screens.last().expect("downloads");
        assert_eq!(screen.list_name, shell.text("current_downloads"));
        assert_eq!(screen.rows, [shell.text("no_queued_downloads")]);
        assert_eq!(ids(screen), ["back", "list"]);

        let _ = shell
            .application
            .downloads_mut()
            .queue_item(remote("Episode"), apricot_app::DownloadChoice::Audio);
        shell.activate_action(&mut ui, "open_current_downloads");
        let screen = ui.screens.last().expect("downloads");
        assert_eq!(
            ids(screen),
            [
                "back",
                "open",
                "download_all_audio",
                "download_all_video",
                "list"
            ]
        );
        assert!(
            screen.rows[0].starts_with("Episode | "),
            "{:?}",
            screen.rows
        );
        ui.selected = Some(0);
        shell.activate_action(&mut ui, "context_menu");
        shell.activate_screen_button(&mut ui, "download_remove_queued");
        assert!(
            ui.announcements
                .last()
                .is_some_and(|message| message.contains("Episode")),
            "{:?}",
            ui.announcements
        );
        assert_eq!(
            ui.screens.last().expect("downloads").rows,
            [shell.text("no_queued_downloads")]
        );
    }

    #[test]
    fn a_declined_confirmation_cancels_the_download() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell
            .application
            .set_boolean_setting(apricot_core::SettingId::ConfirmBeforeDownload, true)
            .expect("set");
        shell.start_download_item(
            &mut ui,
            &remote("Episode"),
            apricot_app::DownloadChoice::Audio,
            false,
        );
        assert_eq!(ui.questions.len(), 1);
        assert!(ui.questions[0].contains("Episode"));
        assert!(shell.application.downloads().active().is_empty());
    }
}

mod settings {
    use apricot_core::SettingId;
    use apricot_core::shortcut::ShortcutChord;

    use crate::{SettingsBinding, SettingsInput, ShortcutCapture};

    #[test]
    fn a_checkbox_applies_and_a_used_shortcut_is_refused() {
        let mut shell = super::keyboard::shell();
        let binding = SettingsBinding::Checkbox(SettingId::ConfirmBeforeDownload);
        shell
            .settings_apply(
                &binding,
                &SettingsInput::Checked(true),
                &std::collections::BTreeMap::default(),
            )
            .expect("applied");
        assert!(shell.application.settings().confirm_before_download);

        let chord = ShortcutChord::parse("Ctrl+Shift+J").expect("chord");
        let outcome =
            shell.settings_capture_shortcut("open_search", chord, &|_| "Action finder".to_owned());
        assert!(
            matches!(outcome, ShortcutCapture::Conflict { ref message, .. } if message.contains("Action finder")),
            "{outcome:?}"
        );
        let chord = ShortcutChord::parse("Ctrl+Alt+F12").expect("chord");
        let outcome = shell.settings_capture_shortcut("open_search", chord, &|id| id.to_owned());
        assert!(
            matches!(outcome, ShortcutCapture::Captured { ref shortcut, .. } if shortcut == "Ctrl+Alt+F12"),
            "{outcome:?}"
        );
    }
}

mod cookies {
    use crate::CookieCommandResult;

    #[test]
    fn an_imported_cookies_file_fills_the_field_and_says_the_login_was_found() {
        let mut shell = super::keyboard::shell();
        let folder = std::env::temp_dir().join(format!("apricot-cookies-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("folder");
        let file = folder.join("cookies.txt");
        std::fs::write(
            &file,
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t4102444800\tSAPISID\tabc\n.youtube.com\tTRUE\t/\tTRUE\t4102444800\tLOGIN_INFO\txyz\n",
        )
        .expect("written");
        let result = shell.settings_import_cookies_file(&file);
        let CookieCommandResult::Source {
            path,
            browser_index,
            announce,
            warning,
        } = result
        else {
            panic!("{result:?}");
        };
        assert_eq!(path, file.to_string_lossy());
        assert_eq!(browser_index, 0);
        assert_eq!(announce.len(), 2, "{announce:?}");
        assert!(warning.is_none());
        let _ = std::fs::remove_dir_all(&folder);
    }
}

mod audiovault {
    use super::equalizer::Ui;

    #[test]
    fn audiovault_without_a_login_asks_for_one_and_stays_on_the_menu() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.activate_action(&mut ui, "open_audiovault");
        assert_eq!(shell.view(), crate::View::MainMenu);
        assert!(ui.screens.is_empty());
    }
}

mod spotify_lists {
    use apricot_app::context_menu::{ContextCommand, ContextMenuContext, ContextMenuEntry};
    use apricot_spotify::{CatalogItem, ItemKind};

    use super::equalizer::Ui;
    use crate::ScreenControl;
    use crate::spotify_lists::Source;

    fn track(name: &str, uid: &str) -> CatalogItem {
        CatalogItem {
            kind: ItemKind::Track,
            uri: format!("spotify:track:{name:0>22}"),
            name: name.to_owned(),
            subtitle: "Artist".to_owned(),
            album: "Album".to_owned(),
            album_uri: "spotify:album:a".to_owned(),
            artist_uri: "spotify:artist:b".to_owned(),
            duration_ms: Some(180_000),
            playable: true,
            explicit: false,
            uid: Some(uid.to_owned()),
            saved: Some(false),
            count: None,
            editable: false,
            format: String::new(),
        }
    }

    fn commands(entries: &[ContextMenuEntry]) -> Vec<ContextCommand> {
        entries
            .iter()
            .flat_map(|entry| match entry {
                ContextMenuEntry::Command { command, .. } => vec![*command],
                ContextMenuEntry::Submenu { entries, .. } => commands(entries),
                ContextMenuEntry::Disabled { .. } => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn a_list_is_named_by_its_title_with_open_and_back() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.spotify_browse_test_open(
            &mut ui,
            Source::Playlist("spotify:playlist:p".into()),
            "Mix",
            vec![track("one", "u1"), track("two", "u2")],
            true,
        );
        let screen = ui.screens.last().unwrap();
        assert_eq!(screen.list_name, "Mix");
        assert_eq!(screen.rows.len(), 2);
        assert!(screen.focus_list);
        assert!(matches!(screen.controls[0], ScreenControl::List));
        assert_eq!(screen.controls.len(), 3);
    }

    #[test]
    fn back_returns_to_the_same_row_of_the_previous_list() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.spotify_browse_test_open(
            &mut ui,
            Source::Album("spotify:album:a".into()),
            "Album",
            vec![track("one", "u1"), track("two", "u2"), track("three", "u3")],
            false,
        );
        ui.selected = Some(2);
        shell.list_selection_changed(&mut ui);
        shell.spotify_browse_test_open(
            &mut ui,
            Source::Album("spotify:album:c".into()),
            "Other",
            vec![track("four", "u4")],
            false,
        );
        shell.spotify_browse_back(&mut ui);
        let screen = ui.screens.last().unwrap();
        assert_eq!(screen.list_name, "Album");
        assert_eq!(screen.selected, Some(2));
        assert!(screen.focus_list);
    }

    #[test]
    fn an_editable_playlist_offers_remove_and_only_possible_moves() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.spotify_browse_test_open(
            &mut ui,
            Source::Playlist("spotify:playlist:p".into()),
            "Mine",
            vec![track("one", "u1"), track("two", "u2")],
            true,
        );
        ui.selected = Some(0);
        let catalog = shell.catalog();
        let titles = Vec::new();
        let context = ContextMenuContext {
            catalog: &catalog,
            settings: shell.application.settings(),
            user_playlist_titles: &titles,
            queued_download_count: 0,
        };
        let entries = commands(&shell.spotify_browse_context_entries(&ui, &context).unwrap());
        assert!(entries.contains(&ContextCommand::SpotifyRemoveFromPlaylist));
        assert!(entries.contains(&ContextCommand::SpotifyMoveDown));
        assert!(!entries.contains(&ContextCommand::SpotifyMoveUp));
    }

    #[test]
    fn a_playlist_track_plays_in_its_playlist_by_occurrence() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.spotify_browse_test_open(
            &mut ui,
            Source::Playlist("spotify:playlist:p".into()),
            "Mine",
            vec![track("one", "u1"), track("two", "u2")],
            true,
        );
        ui.selected = Some(1);
        let media = shell.active_media_item(&ui).unwrap();
        assert_eq!(media.id.0, format!("spotify:track:{:0>22}", "two"));
        assert_eq!(
            media
                .metadata
                .get("spotify_context")
                .and_then(|v| v.as_str()),
            Some("spotify:playlist:p")
        );
        assert_eq!(
            media.metadata.get("spotify_uid").and_then(|v| v.as_str()),
            Some("u2")
        );
    }
}

mod subscriptions {
    use super::equalizer::Ui;
    use crate::{ScreenControl, View};

    fn channel(title: &str) -> apricot_core::MediaItem {
        let url = format!("https://www.youtube.com/@{title}");
        apricot_core::MediaItem {
            id: apricot_core::MediaId(url.clone()),
            source: apricot_core::MediaSource::Youtube,
            kind: apricot_core::MediaKind::Channel,
            title: title.to_owned(),
            url: Some(url.parse().unwrap()),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: title.to_owned(),
            duration_seconds: None,
            metadata: std::collections::BTreeMap::new(),
        }
    }

    fn button_ids(screen: &crate::ListScreen) -> Vec<&str> {
        screen
            .controls
            .iter()
            .map(|control| match control {
                ScreenControl::Button { id, .. } => *id,
                ScreenControl::List => "list",
                _ => "other",
            })
            .collect()
    }

    #[test]
    fn subscriptions_follow_the_windows_tab_order_and_keep_the_channel() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.activate_action(&mut ui, "open_subscriptions");
        assert_eq!(shell.view(), View::Subscriptions);
        let screen = ui.screens.last().unwrap();
        assert_eq!(screen.rows.len(), 1);
        assert_eq!(
            button_ids(screen),
            [
                "back",
                "subscription_check",
                "open",
                "subscription_new",
                "collection_remove",
                "subscription_filter",
                "subscription_set_category",
                "list"
            ]
        );
        for title in ["Beta", "Alpha"] {
            shell
                .application
                .subscribe_to_item(&channel(title), 1.0)
                .unwrap();
        }
        shell.refresh_subscriptions(&mut ui, false, None);
        let rows = ui.screens.last().unwrap().rows.clone();
        assert_eq!(rows.len(), 2);
        ui.selected = Some(1);
        let selected_title = rows[1].split(" | ").next().unwrap().to_owned();
        shell.refresh_subscriptions(&mut ui, false, None);
        let screen = ui.screens.last().unwrap();
        assert!(screen.rows[screen.selected.unwrap()].starts_with(&selected_title));
        assert!(!screen.focus_list);
    }

    #[test]
    fn delete_removes_the_selected_subscription() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell
            .application
            .subscribe_to_item(&channel("Alpha"), 1.0)
            .unwrap();
        shell.activate_action(&mut ui, "open_subscriptions");
        ui.selected = Some(0);
        shell.activate_action(&mut ui, "remove_selected");
        assert!(shell.application.subscriptions().is_empty());
        assert!(ui.announcements.last().unwrap().contains("Alpha"));
    }

    #[test]
    fn the_notification_center_has_back_play_and_clear() {
        let mut shell = super::keyboard::shell();
        let mut ui = Ui::default();
        shell.activate_action(&mut ui, "new_subscription_videos");
        assert_eq!(shell.view(), View::NotificationCenter);
        let screen = ui.screens.last().unwrap();
        assert_eq!(
            button_ids(screen),
            ["back", "open", "notification_clear", "list"]
        );
        assert!(screen.focus_list);
    }
}
