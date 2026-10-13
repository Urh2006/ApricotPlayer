//! Actions shared with the Windows shell: resume, row metadata and diagnostics.
use crate::{Shell, ShellUi, View};
use apricot_app::diagnostic_report::{DiagnosticEnvironment, DiagnosticRedactor};
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Default)]
pub(crate) struct ActionState {
    columns: apricot_app::result_columns::ResultColumnCursor,
    report: Option<Receiver<String>>,
}

impl Shell {
    pub(crate) fn announce_active_media_column(&mut self, ui: &mut dyn ShellUi, forward: bool) {
        if matches!(self.view, View::MainMenu | View::Player) {
            return;
        }
        let catalog = self.catalog();
        let message = match self.active_media_item(ui) {
            Some(item) => apricot_app::result_columns::result_column_announcement(
                &mut self.actions.columns,
                &item,
                &catalog,
                forward,
            ),
            None => catalog.text("no_selection").to_owned(),
        };
        ui.set_status(&message, true);
    }

    pub(crate) fn resume_last_player_session(&mut self, ui: &mut dyn ShellUi) {
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        let Some(resume) = self.application.prepare_last_player_session_resume() else {
            ui.set_status(&self.text("resume_last_session_unavailable"), true);
            return;
        };
        let index = |key: &str| {
            resume
                .return_data
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default()
        };
        match resume.return_screen.as_str() {
            "user_playlist_items" => {
                self.playlists.playlist = index("playlist_index");
                self.playlists.item = index("item_index");
            }
            "rss_items" => {
                self.restore_resumed_podcast_selection(index("feed_index"), index("item_index"));
            }
            _ => {}
        }
        if resume.sequence_active {
            self.start_sequence_media_item(ui, resume.item, None);
        } else {
            self.start_media_item(ui, resume.item, None);
        }
    }

    pub(crate) fn copy_diagnostic_report(&mut self, ui: &mut dyn ShellUi) {
        if self.actions.report.is_some() {
            return;
        }
        let directory = crate::player_tools::application_directory();
        let environment = DiagnosticEnvironment {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            app_label: format!("2 Beta {}", env!("CARGO_PKG_VERSION")),
            frozen_build: !cfg!(debug_assertions),
            executable: std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            working_directory: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            mpv_path: apricot_platform::mpv_locations()
                .map_or_else(|| "not found".to_owned(), |(_, p)| p.display().to_string()),
            ffmpeg_path: apricot_platform::ffmpeg_executable(
                &self.application.settings().ffmpeg_location,
                directory.as_deref(),
            )
            .map_or_else(|| "not found".to_owned(), |p| p.display().to_string()),
            player_active: self.application.player_session().is_open() && self.has_playback(),
            resolve_pending: self.youtube.resolving(),
            in_player_screen: self.view == View::Player,
            current_index: ui.selected_row(),
            visible_results: match self.view {
                View::Results | View::Trending => self.application.search_session().items().len(),
                View::Collection => self
                    .application
                    .youtube_collection()
                    .map_or(0, |c| c.items().len()),
                _ => 0,
            },
            process_id: std::process::id(),
            ..DiagnosticEnvironment::default()
        };
        let mut sections =
            apricot_app::diagnostic_report::diagnostic_sections(&self.application, &environment);
        let service = self.spotify_service_ref();
        let accounts = service
            .map(apricot_spotify::SpotifyService::accounts)
            .unwrap_or_default();
        let active = apricot_app::spotify::active_account(&accounts);
        let spotify_settings = apricot_spotify::settings::load();
        sections.push(
            apricot_app::diagnostic_report::DiagnosticSection::new("## Spotify")
                .line("Saved accounts", accounts.accounts.len())
                .line("Active account logged in", active.is_some())
                .line(
                    "Premium",
                    active.is_some_and(apricot_spotify::SpotifyAccount::is_premium),
                )
                .line(
                    "Connected",
                    service
                        .and_then(apricot_spotify::SpotifyService::connected_account)
                        .is_some(),
                )
                .line("Quality kbps", spotify_settings.quality.kbps().to_string())
                .line("Normalisation", spotify_settings.normalisation)
                .line("Autoplay", format!("{:?}", spotify_settings.autoplay)),
        );
        let data_folder = self
            .application
            .settings_file()
            .parent()
            .unwrap_or(std::path::Path::new(""))
            .to_path_buf();
        let ytdlp = directory.map(|d| {
            apricot_platform::app_update::preferred_ytdlp_executable(&d.join("components"))
        });
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let version = ytdlp
                .filter(|p| p.is_file())
                .and_then(|p| apricot_platform::YtDlpYoutubeEngine::new(&p).ok())
                .and_then(|e| e.version().ok())
                .unwrap_or_else(|| "unknown".to_owned());
            if let Some(section) = sections.first_mut() {
                section.set(apricot_app::diagnostic_report::YTDLP_LINE, version);
            }
            let mpv = data_folder.join("mpv.log");
            let updater = data_folder.join("updater.log");
            let spotify = data_folder.join("spotify/spotify.log");
            let report = DiagnosticRedactor::from_environment().report(
                &sections,
                &[
                    ("mpv.log", &mpv),
                    ("updater.log", &updater),
                    ("spotify.log", &spotify),
                ],
            );
            let _ = sender.send(report);
        });
        self.actions.report = Some(receiver);
        ui.set_background_polling(true);
    }

    pub(crate) fn poll_diagnostic_report(&mut self, ui: &mut dyn ShellUi) -> bool {
        let Some(receiver) = self.actions.report.as_ref() else {
            return false;
        };
        let result = match receiver.try_recv() {
            Err(TryRecvError::Empty) => return true,
            result => result,
        };
        self.actions.report = None;
        if let Ok(report) = result {
            self.copy_text_and_announce(ui, &report, "diagnostic_report_copied");
        }
        false
    }
}
