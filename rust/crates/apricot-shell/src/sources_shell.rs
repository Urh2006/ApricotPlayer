//! Thin shell adapter for the shared Radio, IPTV and recording controller.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.
use crate::{
    FeatureForm, FormValues, Shell, ShellUi,
    sources::{SourceBrowser, SourceSelection, SourcesEnvironment, SourcesUi},
};
use apricot_core::MediaItem;

struct Ui<'a> {
    shell: &'a mut Shell,
    ui: &'a mut dyn ShellUi,
}
impl SourcesUi for Ui<'_> {
    fn browse(&mut self, browser: &SourceBrowser) -> Option<SourceSelection> {
        self.ui.sources_browser(browser)
    }
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues> {
        self.ui.feature_form(form)
    }
    fn status(&mut self, message: &str, announce: bool) {
        if !message.is_empty() {
            self.ui.set_status(message, announce);
        }
    }
    fn error(&mut self, message: &str) {
        self.ui.show_error(message);
    }
    fn play(&mut self, item: MediaItem) {
        self.shell.start_media_item(self.ui, item, None);
    }
    fn choose_file(&mut self, title: &str) -> Option<std::path::PathBuf> {
        self.ui.choose_open_file(title, &["m3u", "m3u8", "xml"])
    }
    fn choose_folder(&mut self, title: &str) -> Option<std::path::PathBuf> {
        self.ui.choose_folder(title)
    }
    fn copy(&mut self, text: &str) -> bool {
        self.ui.copy_text(text)
    }
    fn cue(&mut self, started: bool) {
        apricot_platform::recording_cue::play(started);
    }
    fn ask_yes_no(&mut self, message: &str) -> bool {
        let catalog = self.shell.catalog();
        self.ui
            .ask_yes_no(message, catalog.text("yes"), catalog.text("no"))
    }
}

impl Shell {
    fn sources_environment(&self) -> SourcesEnvironment {
        let settings = self.application.settings();
        let app_directory = crate::player_tools::application_directory();
        SourcesEnvironment {
            directory: self
                .application
                .settings_file()
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_owned(),
            download_directory: settings.download_folder.clone().into(),
            ffmpeg: apricot_platform::ffmpeg_executable(
                &settings.ffmpeg_location,
                app_directory.as_deref(),
            ),
            proxy: settings.proxy.clone(),
            shortcuts: settings.keyboard_shortcuts.clone(),
            language: settings.language.clone(),
        }
    }
    pub(crate) fn sources_action(&mut self, ui: &mut dyn ShellUi, action: &str) -> bool {
        if !matches!(
            action,
            "open_radio"
                | "open_iptv"
                | "open_recording"
                | "recording_start"
                | "recording_pause"
                | "recording_stop"
                | "open_recordings_folder"
                | "radio_now_playing"
                | "recording_settings"
        ) {
            return false;
        }
        let environment = self.sources_environment();
        let mut controller = std::mem::take(&mut self.sources);
        let handled = controller.action(&mut Ui { shell: self, ui }, &environment, action);
        let busy = controller.is_busy();
        self.sources = controller;
        if busy {
            ui.set_background_polling(true);
        }
        handled
    }
    pub(crate) fn poll_sources(&mut self, ui: &mut dyn ShellUi) -> bool {
        let environment = self.sources_environment();
        let mut controller = std::mem::take(&mut self.sources);
        let busy = controller.poll(&mut Ui { shell: self, ui }, &environment);
        self.sources = controller;
        busy
    }
    pub(crate) fn source_stream_metadata(&mut self, ui: &mut dyn ShellUi) {
        let session = self.application.player_session();
        let station = session
            .current_item()
            .and_then(|item| item.metadata.get("radio_station_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let title = session.media_info().stream_title.clone();
        let url = session
            .current_item()
            .and_then(|item| item.url.as_ref())
            .map(ToString::to_string);
        let mut controller = std::mem::take(&mut self.sources);
        controller.stream_metadata(
            &mut Ui { shell: self, ui },
            station.as_deref(),
            title.as_deref(),
            url.as_deref(),
        );
        self.sources = controller;
    }
    #[must_use]
    pub fn recording_is_active(&self) -> bool {
        self.sources.is_recording()
    }
}
