//! Shared source browsers and recorder ownership. Native windows render the
//! same model and report actions; network/capture work stays off the UI thread.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.

use crate::{FeatureForm, FormField, FormValue, FormValues};
use apricot_core::{MediaItem, TranslationCatalog};
use apricot_platform::{
    iptv::{ChannelKind, EpgGuide, IptvChannel, IptvClient, IptvSource, SourceKind},
    radio::{
        DirectoryEntry, DirectoryKind, RadioCategory, RadioClient, RadioError, RadioFilters,
        RadioLibrary, RadioPage, RadioRequest, Station,
    },
    recording::{
        self, CaptureKind, CaptureSource, RecordingEvent, RecordingFeedback, RecordingFormat,
        RecordingSession, RecordingSettings,
    },
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub struct SourceAction {
    pub id: &'static str,
    pub label: String,
    pub shortcut: String,
}
#[derive(Clone, Debug)]
pub struct SourceBrowser {
    pub title: String,
    pub prompt: String,
    pub rows: Vec<String>,
    pub selected: usize,
    pub actions: Vec<SourceAction>,
    /// Labels of the button that lists the actions and of the Back button.
    pub actions_button: String,
    pub back_button: String,
}
#[derive(Clone, Debug)]
pub struct SourceSelection {
    pub index: usize,
    pub action: String,
}

pub trait SourcesUi {
    fn browse(&mut self, browser: &SourceBrowser) -> Option<SourceSelection>;
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues>;
    fn status(&mut self, message: &str, announce: bool);
    fn error(&mut self, message: &str);
    fn play(&mut self, item: MediaItem);
    fn choose_file(&mut self, title: &str) -> Option<PathBuf>;
    fn choose_folder(&mut self, title: &str) -> Option<PathBuf>;
    fn copy(&mut self, text: &str) -> bool;
    fn cue(&mut self, started: bool);
    /// A warning with Yes and No, No being the default.
    fn ask_yes_no(&mut self, message: &str) -> bool;
}

#[derive(Clone, Debug)]
pub struct SourcesEnvironment {
    pub directory: PathBuf,
    pub download_directory: PathBuf,
    pub ffmpeg: Option<PathBuf>,
    pub proxy: String,
    pub shortcuts: BTreeMap<String, String>,
    /// The interface language.
    pub language: String,
}

impl SourcesEnvironment {
    #[must_use]
    pub fn catalog(&self) -> TranslationCatalog {
        apricot_app::embedded_catalog(&self.language)
    }
}

enum Work {
    RadioPage(RadioPage),
    RadioDirectory {
        kind: DirectoryKind,
        entries: Vec<DirectoryEntry>,
        offset: usize,
    },
    Iptv {
        source: IptvSource,
        channels: Vec<IptvChannel>,
        guide: EpgGuide,
        warning: Option<String>,
    },
    IptvSeries {
        source: IptvSource,
        channels: Vec<IptvChannel>,
    },
    PlayIptv {
        source: IptvSource,
        channel: IptvChannel,
    },
    Sources(Vec<CaptureSource>),
    Message(String),
}

pub struct SourcesController {
    loaded: Option<PathBuf>,
    blocked: BTreeSet<String>,
    radio: RadioLibrary,
    station_rows: Vec<Station>,
    station_selected: usize,
    request: RadioRequest,
    radio_has_more: bool,
    iptv_sources: Vec<IptvSource>,
    iptv_active: Option<IptvSource>,
    channels: Vec<IptvChannel>,
    channel_selected: usize,
    channel_query: String,
    channel_group: String,
    channel_kind: Option<ChannelKind>,
    guide: EpgGuide,
    recording: RecordingSettings,
    recorder: Option<RecordingSession>,
    available_sources: Vec<CaptureSource>,
    pending: Option<Receiver<Result<Work, String>>>,
    /// The localized "Loading" announcement of the current language.
    loading: String,
    hub_selected: usize,
    metadata_identity: String,
    metadata_title: String,
}

impl Default for SourcesController {
    fn default() -> Self {
        let radio = RadioLibrary::default();
        Self {
            loaded: None,
            blocked: BTreeSet::new(),
            request: RadioRequest {
                category: RadioCategory::All,
                search: String::new(),
                global_filters: radio.global_filters.clone(),
                local_filters: RadioFilters::default(),
                offset: 0,
            },
            radio,
            station_rows: Vec::new(),
            station_selected: 0,
            radio_has_more: false,
            iptv_sources: Vec::new(),
            iptv_active: None,
            channels: Vec::new(),
            channel_selected: 0,
            channel_query: String::new(),
            channel_group: String::new(),
            channel_kind: None,
            guide: EpgGuide::default(),
            recording: RecordingSettings::default(),
            recorder: None,
            available_sources: Vec::new(),
            pending: None,
            loading: String::new(),
            hub_selected: 0,
            metadata_identity: String::new(),
            metadata_title: String::new(),
        }
    }
}

impl SourcesController {
    fn load(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        if self.loaded.as_ref() == Some(&environment.directory) {
            return;
        }
        self.loaded = Some(environment.directory.clone());
        let catalog = environment.catalog();
        for name in ["radio.json", "iptv-sources.json", "recording.json"] {
            let path = environment.directory.join(name);
            if !path.exists() {
                continue;
            }
            let result = std::fs::read(&path).ok().and_then(|bytes| {
                if bytes.len() > 8 * 1024 * 1024 {
                    return None;
                }
                match name {
                    "radio.json" => serde_json::from_slice(&bytes)
                        .map(|value| self.radio = value)
                        .ok(),
                    "iptv-sources.json" => serde_json::from_slice(&bytes)
                        .map(|value| self.iptv_sources = value)
                        .ok(),
                    _ => serde_json::from_slice::<RecordingSettings>(&bytes)
                        .map(|value| {
                            if value.remember {
                                self.recording = value;
                            }
                        })
                        .ok(),
                }
            });
            if result.is_none() {
                self.blocked.insert(name.into());
                ui.error(
                    &catalog
                        .text("sources_data_unreadable")
                        .replace("{file}", name),
                );
            }
        }
        self.request.global_filters = self.radio.global_filters.clone();
        if self.recording.directory.as_os_str().is_empty() {
            self.recording.directory = environment.download_directory.join("Recordings");
        }
    }

    fn save(
        &self,
        environment: &SourcesEnvironment,
        name: &str,
        value: &Value,
    ) -> Result<(), String> {
        if self.blocked.contains(name) {
            return Err(environment
                .catalog()
                .text("sources_save_blocked")
                .replace("{file}", name));
        }
        apricot_storage::write_json_atomic(&environment.directory.join(name), value)
            .map_err(|error| error.to_string())
    }

    fn save_radio(&self, environment: &SourcesEnvironment) -> Result<(), String> {
        self.save(
            environment,
            "radio.json",
            &serde_json::to_value(&self.radio).map_err(|error| error.to_string())?,
        )
    }
    fn save_iptv(&self, environment: &SourcesEnvironment) -> Result<(), String> {
        self.save(
            environment,
            "iptv-sources.json",
            &serde_json::to_value(&self.iptv_sources).map_err(|error| error.to_string())?,
        )
    }
    fn save_recording(&self, environment: &SourcesEnvironment) -> Result<(), String> {
        let mut saved = self.recording.clone();
        if !saved.remember {
            saved.sources.clear();
        }
        self.save(
            environment,
            "recording.json",
            &serde_json::to_value(saved).map_err(|error| error.to_string())?,
        )
    }

    /// Returns true for a source/recording command even when it was cancelled.
    pub fn action(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        action: &str,
    ) -> bool {
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
        self.load(ui, environment);
        let catalog = environment.catalog();
        catalog
            .text("sources_loading")
            .clone_into(&mut self.loading);
        match action {
            "open_radio" => self.radio_hub(ui, environment),
            "open_iptv" => self.iptv_hub(ui, environment),
            "open_recording" => self.recording_panel(ui, environment),
            "recording_start" => self.start_recording(ui, environment),
            "recording_pause" => {
                if let Some(recorder) = &self.recorder {
                    let paused = recorder.toggle_pause();
                    ui.status(
                        if paused {
                            "Recording paused."
                        } else {
                            "Recording resumed."
                        },
                        true,
                    );
                }
            }
            "recording_stop" => {
                if let Some(recorder) = &self.recorder {
                    recorder.stop();
                    ui.status("Finishing recording...", true);
                }
            }
            "open_recordings_folder" => {
                if let Err(error) = apricot_platform::shell::open_path(&self.recording.directory) {
                    ui.error(&error.to_string());
                }
            }
            "radio_now_playing" => ui.status(
                if self.metadata_title.is_empty() {
                    catalog.text("radio_no_song")
                } else {
                    &self.metadata_title
                },
                true,
            ),
            "recording_settings" => self.recording_settings(ui, environment),
            _ => {}
        }
        true
    }

    fn spawn(
        &mut self,
        ui: &mut dyn SourcesUi,
        work: impl FnOnce() -> Result<Work, String> + Send + 'static,
    ) {
        let (send, receive) = mpsc::channel();
        self.pending = Some(receive);
        std::thread::spawn(move || {
            let _ = send.send(work());
        });
        ui.status(&self.loading, true);
    }

    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
            || self
                .recorder
                .as_ref()
                .is_some_and(RecordingSession::is_active)
    }
    #[must_use]
    pub fn is_recording(&self) -> bool {
        self.recorder
            .as_ref()
            .is_some_and(RecordingSession::is_active)
    }

    pub fn poll(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) -> bool {
        if let Some(recorder) = &mut self.recorder {
            for event in recorder.poll() {
                match event {
                    RecordingEvent::Started { output, sources } => {
                        if matches!(
                            self.recording.feedback,
                            RecordingFeedback::Sounds | RecordingFeedback::Both
                        ) {
                            ui.cue(true);
                        }
                        ui.status(
                            &format!(
                                "Recording started: {}. {}",
                                output.display(),
                                sources.join(", ")
                            ),
                            matches!(
                                self.recording.feedback,
                                RecordingFeedback::Speech | RecordingFeedback::Both
                            ),
                        );
                    }
                    RecordingEvent::SourceFailed { name, error } => {
                        ui.error(&format!("Recording source {name}: {error}"));
                    }
                    RecordingEvent::Paused(paused) => ui.status(
                        if paused {
                            "Recording paused."
                        } else {
                            "Recording resumed."
                        },
                        true,
                    ),
                    RecordingEvent::Finished(result) => {
                        if matches!(
                            self.recording.feedback,
                            RecordingFeedback::Sounds | RecordingFeedback::Both
                        ) {
                            ui.cue(false);
                        }
                        match result {
                            Ok(path) => ui.status(
                                &format!("Recording saved: {}", path.display()),
                                matches!(
                                    self.recording.feedback,
                                    RecordingFeedback::Speech | RecordingFeedback::Both
                                ),
                            ),
                            Err(error) => ui.error(&error),
                        }
                    }
                }
            }
        }
        let Some(pending) = self.pending.as_ref() else {
            return self.is_busy();
        };
        let result = match pending.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => Err(environment
                .catalog()
                .text("sources_task_stopped")
                .to_owned()),
        };
        self.pending = None;
        match result {
            Err(error) => ui.error(&error),
            Ok(Work::RadioPage(page)) => {
                let ids: BTreeSet<_> = self
                    .station_rows
                    .iter()
                    .map(|station| station.stationuuid.clone())
                    .collect();
                let first_new = self.station_rows.len();
                self.station_rows.extend(
                    page.stations
                        .into_iter()
                        .filter(|station| !ids.contains(&station.stationuuid)),
                );
                // More stations continue from the first new one.
                if first_new > 0 && self.station_rows.len() > first_new {
                    self.station_selected = first_new;
                }
                self.request.offset = page.next_offset;
                self.radio_has_more = page.has_more;
                if self.station_rows.is_empty() {
                    ui.status(environment.catalog().text("radio_no_stations"), true);
                    self.radio_hub(ui, environment);
                } else {
                    self.radio_list(ui, environment);
                }
            }
            Ok(Work::RadioDirectory {
                kind,
                entries,
                offset,
            }) => self.radio_directory_list(ui, environment, kind, entries, offset),
            Ok(Work::Iptv {
                source,
                channels,
                guide,
                warning,
            }) => {
                self.iptv_active = Some(source);
                self.channels = channels;
                self.guide = guide;
                self.channel_selected = 0;
                if let Some(warning) = warning {
                    ui.status(&warning, true);
                }
                self.channel_list(ui, environment);
            }
            Ok(Work::IptvSeries { source, channels }) => {
                self.iptv_active = Some(source);
                self.channels = channels;
                self.channel_selected = 0;
                self.channel_list(ui, environment);
            }
            Ok(Work::PlayIptv { source, channel }) => {
                if let Some(item) = channel.media(&source) {
                    ui.play(item);
                } else {
                    ui.error("This IPTV channel has no playable address.");
                }
            }
            Ok(Work::Sources(sources)) => {
                self.available_sources = sources;
                self.add_recording_source(ui, environment);
            }
            Ok(Work::Message(message)) => ui.status(&message, true),
        }
        self.is_busy()
    }

    pub fn stream_metadata(
        &mut self,
        ui: &mut dyn SourcesUi,
        station_id: Option<&str>,
        title: Option<&str>,
        stream_url: Option<&str>,
    ) {
        let Some(station_id) = station_id else {
            self.metadata_identity.clear();
            self.metadata_title.clear();
            return;
        };
        if self.metadata_identity != station_id {
            self.metadata_identity = station_id.into();
            self.metadata_title.clear();
        }
        // Until the station sends a song, mpv names the stream after the end
        // of its address, which is no song title.
        let address_name = stream_url
            .and_then(|url| url.trim_end_matches('/').rsplit('/').next())
            .unwrap_or_default();
        let Some(title) = title.map(str::trim).filter(|title| {
            !title.is_empty()
                && !title.starts_with("http://")
                && !title.starts_with("https://")
                && !title.eq_ignore_ascii_case(address_name)
        }) else {
            return;
        };
        if self.metadata_title != title {
            self.metadata_title = title.into();
            if self.radio.announce_metadata {
                ui.status(title, true);
            }
        }
    }

    fn browser(
        environment: &SourcesEnvironment,
        title: &str,
        rows: Vec<String>,
        selected: usize,
        actions: &[(&'static str, &str)],
    ) -> SourceBrowser {
        let catalog = environment.catalog();
        SourceBrowser {
            title: title.into(),
            prompt: catalog.text("sources_browser_prompt").to_owned(),
            rows,
            selected,
            actions: actions
                .iter()
                .map(|(id, label)| SourceAction {
                    id,
                    label: (*label).into(),
                    shortcut: environment
                        .shortcuts
                        .get(shortcut_action(id))
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect(),
            actions_button: catalog.text("sources_actions").to_owned(),
            back_button: catalog.text("sources_back").to_owned(),
        }
    }

    fn radio_hub(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        let catalog = environment.catalog();
        let rows = RADIO_HUB
            .iter()
            .map(|key| catalog.text(key).to_owned())
            .collect();
        let Some(choice) = ui.browse(&Self::browser(
            environment,
            catalog.text("radio_title"),
            rows,
            self.hub_selected,
            &[("open", catalog.text("open"))],
        )) else {
            return;
        };
        self.hub_selected = choice.index;
        match choice.index {
            0 => self.search_radio(ui, environment),
            1 => {
                self.request.category = RadioCategory::All;
                self.request.search.clear();
                self.load_radio(ui, environment, true);
            }
            2 | 3 => {
                self.station_rows = if choice.index == 2 {
                    self.radio.favorites.clone()
                } else {
                    self.radio.recent.clone()
                };
                self.station_selected = 0;
                self.radio_has_more = false;
                if self.station_rows.is_empty() {
                    ui.status(catalog.text("radio_no_stations"), true);
                    self.radio_hub(ui, environment);
                } else {
                    self.radio_list(ui, environment);
                }
            }
            index @ 4..=8 => {
                self.request.category = [
                    RadioCategory::MostPlayed,
                    RadioCategory::Trending,
                    RadioCategory::MostVoted,
                    RadioCategory::RecentClicks,
                    RadioCategory::Updated,
                ][index - 4]
                    .clone();
                self.request.search.clear();
                self.load_radio(ui, environment, true);
            }
            index @ 9..=13 => self.load_radio_directory(
                ui,
                environment,
                [
                    DirectoryKind::Countries,
                    DirectoryKind::Regions,
                    DirectoryKind::Languages,
                    DirectoryKind::Tags,
                    DirectoryKind::Codecs,
                ][index - 9],
                0,
            ),
            14 => {
                self.radio_settings(ui, environment);
                self.radio_hub(ui, environment);
            }
            _ => {}
        }
    }

    fn search_radio(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        let catalog = environment.catalog();
        let mut form = FeatureForm::localized(
            &catalog,
            catalog.text("radio_search_title"),
            vec![FormField::text(
                "query",
                catalog.text("radio_station_name"),
                &self.request.search,
            )],
        );
        catalog.text("search").clone_into(&mut form.submit);
        let Some(values) = ui.form(&form) else {
            self.radio_hub(ui, environment);
            return;
        };
        self.request.search = text(&values, "query");
        self.request.category = RadioCategory::All;
        self.load_radio(ui, environment, true);
    }

    fn load_radio(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        reset: bool,
    ) {
        if reset {
            self.station_rows.clear();
            self.station_selected = 0;
            self.request.offset = 0;
        }
        self.request.global_filters = self.radio.global_filters.clone();
        self.request.local_filters = self
            .radio
            .local_filters
            .get(&format!("{:?}", self.request.category))
            .cloned()
            .unwrap_or_default();
        let request = self.request.clone();
        let proxy = environment.proxy.clone();
        let catalog = environment.catalog();
        self.spawn(ui, move || {
            RadioClient::new(Some(&proxy))
                .and_then(|client| client.stations(&request))
                .map(Work::RadioPage)
                .map_err(|error| radio_error(&catalog, &error))
        });
    }

    #[allow(clippy::too_many_lines)] // One list with its actions.
    fn radio_list(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        let catalog = environment.catalog();
        loop {
            let mut actions = vec![
                ("play", catalog.text("play")),
                ("radio_details", catalog.text("radio_details")),
                ("radio_favorite", catalog.text("radio_favorite")),
                ("radio_copy_url", catalog.text("radio_copy_url")),
                ("radio_copy_name", catalog.text("radio_copy_name")),
                ("radio_website", catalog.text("radio_website")),
                ("radio_vote", catalog.text("radio_vote")),
                ("radio_filters", catalog.text("radio_filters")),
                ("radio_refresh", catalog.text("radio_refresh")),
                ("radio_search", catalog.text("radio_search_stations")),
                ("radio_clear_history", catalog.text("radio_clear_history")),
            ];
            if self.radio_has_more {
                actions.push(("radio_more", catalog.text("radio_more")));
            }
            let rows = self.station_rows.iter().map(Station::label).collect();
            let Some(choice) = ui.browse(&Self::browser(
                environment,
                catalog.text("radio_stations"),
                rows,
                self.station_selected,
                &actions,
            )) else {
                // Back returns to the radio menu.
                self.radio_hub(ui, environment);
                return;
            };
            self.station_selected = choice.index;
            match choice.action.as_str() {
                "radio_more" => {
                    self.load_radio(ui, environment, false);
                    return;
                }
                "radio_refresh" => {
                    self.load_radio(ui, environment, true);
                    return;
                }
                "radio_filters" => {
                    if self.radio_filters(ui, environment) {
                        self.load_radio(ui, environment, true);
                        return;
                    }
                    continue;
                }
                "radio_search" => {
                    self.search_radio(ui, environment);
                    return;
                }
                "radio_clear_history" => {
                    if ui.ask_yes_no(catalog.text("radio_clear_history_confirm")) {
                        let mut next = self.radio.clone();
                        next.recent.clear();
                        let previous = std::mem::replace(&mut self.radio, next);
                        match self.save_radio(environment) {
                            Ok(()) => ui.status(catalog.text("radio_history_cleared"), true),
                            Err(error) => {
                                self.radio = previous;
                                ui.error(&error);
                            }
                        }
                    }
                    continue;
                }
                _ => {}
            }
            let Some(station) = self.station_rows.get(choice.index).cloned() else {
                continue;
            };
            match choice.action.as_str() {
                "play" | "open" => {
                    let Some(item) = station.media() else {
                        ui.error(catalog.text("radio_no_stream"));
                        continue;
                    };
                    let previous = self.radio.clone();
                    self.radio.played(&station);
                    if let Err(error) = self.save_radio(environment) {
                        self.radio = previous;
                        ui.error(&error);
                    }
                    ui.play(item);
                    // The catalog counts plays; nothing waits for the answer.
                    let proxy = environment.proxy.clone();
                    let id = station.stationuuid;
                    std::thread::spawn(move || {
                        let _ =
                            RadioClient::new(Some(&proxy)).and_then(|client| client.clicked(&id));
                    });
                    return;
                }
                "radio_details" => {
                    let mut form = FeatureForm::localized(
                        &catalog,
                        catalog.text("radio_details"),
                        vec![FormField::read_only(
                            "details",
                            &station.name,
                            station_details(&catalog, &station),
                        )],
                    );
                    catalog.text("ok").clone_into(&mut form.submit);
                    let _ = ui.form(&form);
                }
                "radio_favorite" => {
                    let previous = self.radio.clone();
                    let added = self.radio.toggle_favorite(&station);
                    match self.save_radio(environment) {
                        Ok(()) => ui.status(
                            catalog.text(if added {
                                "radio_favorite_added"
                            } else {
                                "radio_favorite_removed"
                            }),
                            true,
                        ),
                        Err(error) => {
                            self.radio = previous;
                            ui.error(&error);
                        }
                    }
                }
                "radio_copy_url" => {
                    if let Some(url) = station.stream_url()
                        && ui.copy(url.as_str())
                    {
                        ui.status(catalog.text("url_copied"), true);
                    }
                }
                "radio_copy_name" => {
                    if ui.copy(&station.name) {
                        ui.status(catalog.text("radio_name_copied"), true);
                    }
                }
                "radio_website" => match apricot_platform::radio::http_url(&station.homepage) {
                    Some(url) => {
                        if let Err(error) = apricot_platform::shell::open_url(url.as_str()) {
                            ui.error(&error.to_string());
                        }
                    }
                    None => ui.status(catalog.text("radio_no_website"), true),
                },
                "radio_vote" => {
                    let proxy = environment.proxy.clone();
                    let id = station.stationuuid;
                    let catalog = catalog.clone();
                    self.spawn(ui, move || {
                        RadioClient::new(Some(&proxy))
                            .and_then(|client| client.vote(&id))
                            .map(|()| Work::Message(catalog.text("radio_vote_done").to_owned()))
                            .map_err(|error| radio_error(&catalog, &error))
                    });
                    return;
                }
                _ => {}
            }
        }
    }

    fn load_radio_directory(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        kind: DirectoryKind,
        offset: usize,
    ) {
        let proxy = environment.proxy.clone();
        let catalog = environment.catalog();
        self.spawn(ui, move || {
            RadioClient::new(Some(&proxy))
                .and_then(|client| client.directory(kind, offset, "", ""))
                .map(|entries| Work::RadioDirectory {
                    kind,
                    entries,
                    offset,
                })
                .map_err(|error| radio_error(&catalog, &error))
        });
    }

    fn radio_directory_list(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        kind: DirectoryKind,
        entries: Vec<DirectoryEntry>,
        offset: usize,
    ) {
        let catalog = environment.catalog();
        let more = entries.len() >= apricot_platform::radio::PAGE_SIZE;
        let mut actions = vec![("open", catalog.text("open"))];
        if more {
            actions.push(("more", catalog.text("radio_next_page")));
        }
        let rows = entries
            .iter()
            .map(|entry| {
                let count = entry.stationcount.to_string();
                if kind == DirectoryKind::Regions && !entry.country.is_empty() {
                    catalog
                        .text("radio_region_row")
                        .replace("{name}", &entry.name)
                        .replace("{country}", &entry.country)
                        .replace("{count}", &count)
                } else {
                    catalog
                        .text("radio_directory_row")
                        .replace("{name}", &entry.name)
                        .replace("{count}", &count)
                }
            })
            .collect();
        let title = catalog.text(RADIO_HUB[9 + directory_index(kind)]);
        let Some(choice) = ui.browse(&Self::browser(environment, title, rows, 0, &actions)) else {
            self.radio_hub(ui, environment);
            return;
        };
        if choice.action == "more" {
            self.load_radio_directory(ui, environment, kind, offset + entries.len());
            return;
        }
        let Some(entry) = entries.get(choice.index) else {
            return;
        };
        self.request.category = match kind {
            DirectoryKind::Countries => RadioCategory::Country(entry.iso_3166_1.clone()),
            DirectoryKind::Regions => RadioCategory::Region {
                country: String::new(),
                name: entry.name.clone(),
            },
            DirectoryKind::Languages => RadioCategory::Language(entry.name.clone()),
            DirectoryKind::Tags => RadioCategory::Tag(entry.name.clone()),
            DirectoryKind::Codecs => RadioCategory::Codec(entry.name.clone()),
        };
        self.request.search.clear();
        self.load_radio(ui, environment, true);
    }

    fn radio_settings(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        let catalog = environment.catalog();
        let Some(values) = ui.form(&FeatureForm::localized(
            &catalog,
            catalog.text("radio_settings"),
            vec![FormField::checkbox(
                "announce",
                catalog.text("radio_announce_metadata"),
                self.radio.announce_metadata,
            )],
        )) else {
            return;
        };
        let old = self.radio.announce_metadata;
        self.radio.announce_metadata = checked(&values, "announce");
        if let Err(error) = self.save_radio(environment) {
            self.radio.announce_metadata = old;
            ui.error(&error);
        }
    }

    fn radio_filters(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) -> bool {
        let catalog = environment.catalog();
        let key = format!("{:?}", self.request.category);
        let local = self
            .radio
            .local_filters
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let form = FeatureForm::localized(
            &catalog,
            catalog.text("radio_filters"),
            vec![
                FormField::choice(
                    "scope",
                    catalog.text("radio_filter_scope"),
                    vec![
                        catalog.text("radio_filter_this").to_owned(),
                        catalog.text("radio_filter_all").to_owned(),
                    ],
                    0,
                ),
                FormField::text(
                    "country",
                    catalog.text("radio_filter_country"),
                    &local.country,
                ),
                FormField::text("region", catalog.text("radio_filter_region"), &local.region),
                FormField::text(
                    "language",
                    catalog.text("radio_filter_language"),
                    &local.language,
                ),
                FormField::text("tags", catalog.text("radio_filter_tags"), &local.tags),
                FormField::text("codec", catalog.text("radio_filter_codec"), &local.codec),
                FormField::number(
                    "min",
                    catalog.text("radio_filter_min_bitrate"),
                    f64::from(local.minimum_bitrate),
                    0.0,
                    10000.0,
                ),
                FormField::number(
                    "max",
                    catalog.text("radio_filter_max_bitrate"),
                    f64::from(local.maximum_bitrate),
                    0.0,
                    10000.0,
                ),
                FormField::checkbox(
                    "working",
                    catalog.text("radio_filter_working"),
                    local.only_working,
                ),
                FormField::checkbox(
                    "https",
                    catalog.text("radio_filter_https"),
                    local.https_only,
                ),
            ],
        );
        let Some(values) = ui.form(&form) else {
            return false;
        };
        let bitrate = |key: &str| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let value = number(&values, key).clamp(0.0, 10000.0).round() as u32;
            value
        };
        let filters = RadioFilters {
            country: text(&values, "country"),
            region: text(&values, "region"),
            language: text(&values, "language"),
            tags: text(&values, "tags"),
            codec: text(&values, "codec"),
            minimum_bitrate: bitrate("min"),
            maximum_bitrate: bitrate("max"),
            only_working: checked(&values, "working"),
            https_only: checked(&values, "https"),
        };
        if filters.maximum_bitrate > 0 && filters.maximum_bitrate < filters.minimum_bitrate {
            ui.error(catalog.text("radio_filter_bitrate_order"));
            return false;
        }
        let previous = self.radio.clone();
        if choice(&values, "scope") == 1 {
            self.radio.global_filters = filters;
        } else {
            self.radio.local_filters.insert(key, filters);
        }
        if let Err(error) = self.save_radio(environment) {
            self.radio = previous;
            ui.error(&error);
            return false;
        }
        true
    }

    fn iptv_hub(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        loop {
            let rows = self
                .iptv_sources
                .iter()
                .map(|source| source.name.clone())
                .collect();
            let Some(selection) = ui.browse(&Self::browser(
                environment,
                "IPTV sources",
                rows,
                0,
                &[
                    ("open", "Open source"),
                    ("add", "Add source"),
                    ("edit", "Edit source"),
                    ("remove", "Remove source"),
                ],
            )) else {
                return;
            };
            match selection.action.as_str() {
                "add" => self.edit_iptv_source(ui, environment, None),
                "edit" => self.edit_iptv_source(ui, environment, Some(selection.index)),
                "remove" => {
                    if let Some(source) = self.iptv_sources.get(selection.index)
                        && confirm(ui, "Remove IPTV source?", &source.name)
                    {
                        let previous = self.iptv_sources.clone();
                        self.iptv_sources.remove(selection.index);
                        if let Err(error) = self.save_iptv(environment) {
                            self.iptv_sources = previous;
                            ui.error(&error);
                        }
                    }
                }
                "open" => {
                    if let Some(source) = self.iptv_sources.get(selection.index).cloned() {
                        self.load_iptv(ui, environment, source);
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn edit_iptv_source(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        index: Option<usize>,
    ) {
        let old = index
            .and_then(|index| self.iptv_sources.get(index))
            .cloned();
        let credentials = old
            .as_ref()
            .and_then(|source| source.credentials().ok())
            .unwrap_or(Value::Null);
        let form = FeatureForm::new(
            "IPTV source",
            vec![
                FormField::text(
                    "name",
                    "Source name",
                    old.as_ref().map_or("", |source| &source.name),
                ),
                FormField::choice(
                    "kind",
                    "Source type",
                    vec![
                        "Playlist file or URL".into(),
                        "Xtream".into(),
                        "Stalker portal".into(),
                    ],
                    old.as_ref().map_or(0, |source| match source.kind {
                        SourceKind::Playlist => 0,
                        SourceKind::Xtream => 1,
                        SourceKind::Stalker => 2,
                    }),
                ),
                FormField::text(
                    "address",
                    "Playlist file path or server URL",
                    old.as_ref().map_or("", |source| &source.address),
                ),
                FormField::text(
                    "username",
                    "Username",
                    credentials["username"].as_str().unwrap_or_default(),
                ),
                FormField::secret(
                    "password",
                    "Password",
                    credentials["password"].as_str().unwrap_or_default(),
                ),
                FormField::text(
                    "mac",
                    "Portal MAC address",
                    credentials["mac"].as_str().unwrap_or_default(),
                ),
                FormField::text(
                    "epg",
                    "Programme guide file or URL (optional)",
                    old.as_ref().map_or("", |source| &source.epg_address),
                ),
                FormField::checkbox(
                    "hls",
                    "Prefer HLS live streams",
                    old.as_ref().is_some_and(|source| source.prefer_hls),
                ),
            ],
        );
        let Some(values) = ui.form(&form) else { return };
        let name = text(&values, "name");
        let address = text(&values, "address");
        if name.trim().is_empty() || address.trim().is_empty() {
            ui.error("Enter a source name and address.");
            return;
        }
        let mut source = IptvSource {
            id: old
                .as_ref()
                .map_or_else(unique_id, |source| source.id.clone()),
            name,
            kind: match choice(&values, "kind") {
                1 => SourceKind::Xtream,
                2 => SourceKind::Stalker,
                _ => SourceKind::Playlist,
            },
            address,
            protected_credentials: String::new(),
            epg_address: text(&values, "epg"),
            prefer_hls: checked(&values, "hls"),
        };
        if source.kind != SourceKind::Playlist
            && let Err(error) = source.set_credentials(
                &text(&values, "username"),
                &text(&values, "password"),
                &text(&values, "mac"),
            )
        {
            ui.error(&error);
            return;
        }
        let previous = self.iptv_sources.clone();
        if let Some(index) = index {
            if let Some(current) = self.iptv_sources.get_mut(index) {
                *current = source;
            }
        } else {
            self.iptv_sources.push(source);
        }
        if let Err(error) = self.save_iptv(environment) {
            self.iptv_sources = previous;
            ui.error(&error);
        }
    }

    fn load_iptv(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        source: IptvSource,
    ) {
        let proxy = environment.proxy.clone();
        self.spawn(ui, move || {
            let client = IptvClient::new(Some(&proxy))?;
            let channels = client.load(&source)?;
            let mut warning = None;
            let guide = if source.epg_address.trim().is_empty() {
                EpgGuide::default()
            } else {
                match client.epg(&source.epg_address) {
                    Ok(guide) => guide,
                    Err(error) => {
                        warning = Some(error);
                        EpgGuide::default()
                    }
                }
            };
            Ok(Work::Iptv {
                source,
                channels,
                guide,
                warning,
            })
        });
    }

    fn channel_list(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        loop {
            let now = chrono::Utc::now().timestamp();
            let query = self.channel_query.to_lowercase();
            let indices: Vec<_> = self
                .channels
                .iter()
                .enumerate()
                .filter(|(_, channel)| {
                    (self.channel_kind.is_none() || self.channel_kind == Some(channel.kind))
                        && (self.channel_group.is_empty() || self.channel_group == channel.group)
                        && (query.is_empty() || channel.name.to_lowercase().contains(&query))
                })
                .map(|(index, _)| index)
                .collect();
            let rows = indices
                .iter()
                .map(|&index| {
                    let channel = &self.channels[index];
                    let current = self.guide.now_next(&channel.epg_id, now).0;
                    current.map_or_else(
                        || channel.name.clone(),
                        |programme| format!("{}; {}", channel.name, programme.title),
                    )
                })
                .collect();
            let Some(selected) = ui.browse(&Self::browser(
                environment,
                "IPTV channels",
                rows,
                self.channel_selected,
                &[
                    ("play", "Play"),
                    ("details", "Programme details"),
                    ("filters", "Search and filters"),
                    ("refresh", "Refresh source"),
                    ("copy", "Copy stream URL"),
                ],
            )) else {
                return;
            };
            self.channel_selected = selected.index;
            if selected.action == "filters" {
                self.channel_filters(ui);
                continue;
            }
            let Some(source) = self.iptv_active.clone() else {
                return;
            };
            if selected.action == "refresh" {
                self.load_iptv(ui, environment, source);
                return;
            }
            let Some(channel) = indices
                .get(selected.index)
                .and_then(|&index| self.channels.get(index))
                .cloned()
            else {
                continue;
            };
            match selected.action.as_str() {
                "open" | "play" => {
                    let proxy = environment.proxy.clone();
                    self.spawn(ui, move || {
                        let client = IptvClient::new(Some(&proxy))?;
                        if channel.kind == ChannelKind::Series {
                            Ok(Work::IptvSeries {
                                channels: client.series(&source, &channel.id)?,
                                source,
                            })
                        } else {
                            Ok(Work::PlayIptv {
                                channel: client.resolve_channel(&channel)?,
                                source,
                            })
                        }
                    });
                    return;
                }
                "details" => {
                    let (current, next) = self.guide.now_next(&channel.epg_id, now);
                    let mut details = format!(
                        "{}\nCategory: {}\nChannel number: {}",
                        channel.name, channel.group, channel.number
                    );
                    if let Some(current) = current {
                        details.push_str(&format!(
                            "\nNow: {}\n{}",
                            current.title, current.description
                        ));
                    }
                    if let Some(next) = next {
                        details.push_str(&format!("\nNext: {}\n{}", next.title, next.description));
                    }
                    let _ = ui.form(&FeatureForm::new(
                        "Programme details",
                        vec![FormField::read_only("details", "Programme", details)],
                    ));
                }
                "copy" => {
                    let _ = ui.copy(&channel.url);
                }
                _ => {}
            }
        }
    }

    fn channel_filters(&mut self, ui: &mut dyn SourcesUi) {
        let mut groups = vec!["All categories".into()];
        groups.extend(
            self.channels
                .iter()
                .map(|channel| channel.group.clone())
                .filter(|group| !group.is_empty())
                .collect::<BTreeSet<_>>(),
        );
        let selected = groups
            .iter()
            .position(|group| *group == self.channel_group)
            .unwrap_or_default();
        let Some(values) = ui.form(&FeatureForm::new(
            "IPTV filters",
            vec![
                FormField::text("query", "Channel name", &self.channel_query),
                FormField::choice(
                    "kind",
                    "Show",
                    vec![
                        "All".into(),
                        "Live TV".into(),
                        "Movies".into(),
                        "Series".into(),
                        "Episodes".into(),
                    ],
                    match self.channel_kind {
                        None => 0,
                        Some(ChannelKind::Live) => 1,
                        Some(ChannelKind::Movie) => 2,
                        Some(ChannelKind::Series) => 3,
                        Some(ChannelKind::Episode) => 4,
                    },
                ),
                FormField::choice("group", "Category", groups.clone(), selected),
            ],
        )) else {
            return;
        };
        self.channel_query = text(&values, "query");
        self.channel_kind = match choice(&values, "kind") {
            1 => Some(ChannelKind::Live),
            2 => Some(ChannelKind::Movie),
            3 => Some(ChannelKind::Series),
            4 => Some(ChannelKind::Episode),
            _ => None,
        };
        self.channel_group = groups
            .get(choice(&values, "group"))
            .filter(|_| choice(&values, "group") > 0)
            .cloned()
            .unwrap_or_default();
        self.channel_selected = 0;
    }

    fn recording_panel(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        loop {
            let rows = self
                .recording
                .sources
                .iter()
                .map(|source| format!("{}; {:.0}%", source.name, source.gain * 100.0))
                .collect();
            let Some(selected) = ui.browse(&Self::browser(
                environment,
                "Recording",
                rows,
                0,
                &[
                    ("recording_start", "Start recording"),
                    ("recording_pause", "Pause or resume"),
                    ("recording_stop", "Stop recording"),
                    ("add", "Add source"),
                    ("edit", "Edit source level"),
                    ("remove", "Remove source"),
                    ("settings", "Recording settings"),
                    ("folder", "Open recordings folder"),
                ],
            )) else {
                return;
            };
            match selected.action.as_str() {
                "recording_start" | "open" => {
                    self.start_recording(ui, environment);
                    return;
                }
                "recording_pause" => {
                    if let Some(recorder) = &self.recorder {
                        recorder.toggle_pause();
                    }
                }
                "recording_stop" => {
                    if let Some(recorder) = &self.recorder {
                        recorder.stop();
                    }
                    return;
                }
                "add" => {
                    let proxy = environment.proxy.clone();
                    self.spawn(ui, move || {
                        let _ = proxy;
                        recording::sources().map(Work::Sources)
                    });
                    return;
                }
                "edit" => {
                    if let Some(source) = self.recording.sources.get_mut(selected.index) {
                        let Some(values) = ui.form(&FeatureForm::new(
                            "Recording source",
                            vec![
                                FormField::text("name", "Source name", &source.name),
                                FormField::number(
                                    "level",
                                    "Level, percent",
                                    f64::from(source.gain) * 100.0,
                                    0.0,
                                    400.0,
                                ),
                                FormField::checkbox(
                                    "exclude",
                                    "Record everything except this application",
                                    matches!(
                                        source.kind,
                                        CaptureKind::Application { exclude: true, .. }
                                    ),
                                ),
                            ],
                        )) else {
                            continue;
                        };
                        source.name = text(&values, "name");
                        source.gain = (number(&values, "level") / 100.0) as f32;
                        if let CaptureKind::Application { exclude, .. } = &mut source.kind {
                            *exclude = checked(&values, "exclude");
                        }
                        if let Err(error) = self.save_recording(environment) {
                            ui.error(&error);
                        }
                    }
                }
                "remove" => {
                    if selected.index < self.recording.sources.len() {
                        self.recording.sources.remove(selected.index);
                        if let Err(error) = self.save_recording(environment) {
                            ui.error(&error);
                        }
                    }
                }
                "settings" => self.recording_settings(ui, environment),
                "folder" => {
                    if let Err(error) =
                        apricot_platform::shell::open_path(&self.recording.directory)
                    {
                        ui.error(&error.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    fn add_recording_source(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        let rows = self
            .available_sources
            .iter()
            .map(|source| source.name.clone())
            .collect();
        let Some(selected) = ui.browse(&Self::browser(
            environment,
            "Add recording source",
            rows,
            0,
            &[("add", "Add source")],
        )) else {
            return;
        };
        if let Some(source) = self.available_sources.get(selected.index).cloned() {
            self.recording.sources.push(source);
            if let Err(error) = self.save_recording(environment) {
                ui.error(&error);
            }
        }
        self.recording_panel(ui, environment);
    }

    fn recording_settings(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        let rates = [
            8000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 88200, 96000, 192000,
        ];
        let form = FeatureForm::new(
            "Recording settings",
            vec![
                FormField::choice(
                    "format",
                    "Format",
                    vec![
                        "FLAC".into(),
                        "WAV".into(),
                        "MP3".into(),
                        "AAC (M4A)".into(),
                    ],
                    match self.recording.format {
                        RecordingFormat::Flac => 0,
                        RecordingFormat::Wav => 1,
                        RecordingFormat::Mp3 => 2,
                        RecordingFormat::Aac => 3,
                    },
                ),
                FormField::choice(
                    "rate",
                    "Sample rate",
                    rates.iter().map(ToString::to_string).collect(),
                    rates
                        .iter()
                        .position(|rate| *rate == self.recording.sample_rate)
                        .unwrap_or(7),
                ),
                FormField::choice(
                    "channels",
                    "Channels",
                    vec!["Mono".into(), "Stereo".into()],
                    usize::from(self.recording.channels.saturating_sub(1)),
                ),
                FormField::number(
                    "bitrate",
                    "Lossy bitrate, kbps",
                    f64::from(self.recording.bitrate),
                    32.0,
                    512.0,
                ),
                FormField::text(
                    "folder",
                    "Destination folder",
                    self.recording.directory.to_string_lossy(),
                ),
                FormField::checkbox(
                    "remember",
                    "Remember this choice, including sources and levels",
                    self.recording.remember,
                ),
                FormField::choice(
                    "feedback",
                    "Start and stop confirmation",
                    vec![
                        "Sounds".into(),
                        "Speech".into(),
                        "Sounds and speech".into(),
                        "None".into(),
                    ],
                    match self.recording.feedback {
                        RecordingFeedback::Sounds => 0,
                        RecordingFeedback::Speech => 1,
                        RecordingFeedback::Both => 2,
                        RecordingFeedback::None => 3,
                    },
                ),
            ],
        );
        let Some(values) = ui.form(&form) else { return };
        let previous = self.recording.clone();
        self.recording.format = match choice(&values, "format") {
            1 => RecordingFormat::Wav,
            2 => RecordingFormat::Mp3,
            3 => RecordingFormat::Aac,
            _ => RecordingFormat::Flac,
        };
        self.recording.sample_rate = rates[choice(&values, "rate")];
        self.recording.channels = (choice(&values, "channels") + 1) as u8;
        self.recording.bitrate = number(&values, "bitrate") as u32;
        self.recording.directory = PathBuf::from(text(&values, "folder"));
        self.recording.remember = checked(&values, "remember");
        self.recording.feedback = match choice(&values, "feedback") {
            1 => RecordingFeedback::Speech,
            2 => RecordingFeedback::Both,
            3 => RecordingFeedback::None,
            _ => RecordingFeedback::Sounds,
        };
        if !self.recording.directory.is_absolute() {
            self.recording = previous;
            ui.error("Choose an absolute recording destination folder.");
            return;
        }
        if let Err(error) = self.save_recording(environment) {
            self.recording = previous;
            ui.error(&error);
        }
    }

    fn start_recording(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        if self.is_recording() {
            ui.status("Recording is already active.", true);
            return;
        }
        if self.recording.sources.is_empty() {
            self.spawn(ui, || recording::sources().map(Work::Sources));
            return;
        }
        let Some(ffmpeg) = environment.ffmpeg.clone() else {
            ui.error("FFmpeg is required for recording.");
            return;
        };
        let name = format!(
            "Recording {}.{}",
            chrono::Local::now().format("%Y-%m-%d %H-%M-%S"),
            self.recording.format.extension()
        );
        let output = self.recording.directory.join(name);
        match RecordingSession::start(self.recording.clone(), ffmpeg, output) {
            Ok(recorder) => {
                self.recorder = Some(recorder);
                ui.status("Preparing recording sources...", true);
            }
            Err(error) => ui.error(&error),
        }
    }
}

fn text(values: &FormValues, key: &str) -> String {
    values
        .get(key)
        .and_then(FormValue::text)
        .unwrap_or_default()
        .to_owned()
}
fn number(values: &FormValues, key: &str) -> f64 {
    values
        .get(key)
        .and_then(FormValue::number)
        .unwrap_or_default()
}
fn checked(values: &FormValues, key: &str) -> bool {
    values
        .get(key)
        .and_then(FormValue::checked)
        .unwrap_or_default()
}
fn choice(values: &FormValues, key: &str) -> usize {
    values
        .get(key)
        .and_then(FormValue::choice)
        .unwrap_or_default()
}
fn unique_id() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or_else(
        |_| "iptv".into(),
        |duration| format!("iptv-{}", duration.as_nanos()),
    )
}
fn confirm(ui: &mut dyn SourcesUi, title: &str, message: &str) -> bool {
    ui.ask_yes_no(&format!("{title} {message}"))
}

/// The rows of the Radio menu, in order.
const RADIO_HUB: [&str; 15] = [
    "radio_search_stations",
    "radio_all",
    "favorites",
    "radio_recent",
    "radio_most_played",
    "radio_trending",
    "radio_most_voted",
    "radio_recent_clicks",
    "radio_updated",
    "radio_countries",
    "radio_regions",
    "radio_languages",
    "radio_tags",
    "radio_codecs",
    "radio_settings",
];

const fn directory_index(kind: DirectoryKind) -> usize {
    match kind {
        DirectoryKind::Countries => 0,
        DirectoryKind::Regions => 1,
        DirectoryKind::Languages => 2,
        DirectoryKind::Tags => 3,
        DirectoryKind::Codecs => 4,
    }
}

/// Station actions answer to the shortcuts of the same command elsewhere in
/// Apricot, so Ctrl+F, Ctrl+L and F7 keep their meaning.
fn shortcut_action(id: &str) -> &str {
    match id {
        "radio_favorite" => "add_favorite",
        "radio_copy_url" => "copy_link",
        "radio_details" => "player_details",
        _ => id,
    }
}

fn radio_error(catalog: &TranslationCatalog, error: &RadioError) -> String {
    match error {
        RadioError::Proxy => catalog.text("radio_error_proxy").to_owned(),
        RadioError::Connect => catalog.text("radio_error_connect").to_owned(),
        RadioError::Busy => catalog.text("radio_error_busy").to_owned(),
        RadioError::Http(code) => catalog
            .text("radio_error_http")
            .replace("{code}", &code.to_string()),
        RadioError::InvalidData => catalog.text("radio_error_data").to_owned(),
        RadioError::InvalidStation => catalog.text("radio_error_station").to_owned(),
        RadioError::VoteRejected(reason) if reason.trim().is_empty() => {
            catalog.text("radio_vote_rejected").to_owned()
        }
        RadioError::VoteRejected(reason) => catalog
            .text("radio_vote_rejected_reason")
            .replace("{reason}", reason.trim()),
    }
}

/// One line for each known property of the station.
fn station_details(catalog: &TranslationCatalog, station: &Station) -> String {
    let yes_no = |value: bool| catalog.text(if value { "yes" } else { "no" }).to_owned();
    let mut lines = vec![station.name.clone()];
    for (key, value) in [
        ("radio_detail_country", station.country.clone()),
        ("radio_detail_region", station.state.clone()),
        ("radio_detail_language", station.language.clone()),
        ("radio_detail_tags", station.tags.clone()),
        ("radio_detail_codec", station.codec.clone()),
        (
            "radio_detail_bitrate",
            if station.bitrate > 0 {
                format!("{} kbps", station.bitrate)
            } else {
                String::new()
            },
        ),
        ("radio_detail_votes", station.votes.to_string()),
        ("radio_detail_listens", station.clickcount.to_string()),
        ("radio_detail_working", yes_no(station.lastcheckok == 1)),
        ("radio_detail_website", station.homepage.clone()),
        (
            "radio_detail_stream",
            station
                .stream_url()
                .map_or_else(String::new, |url| url.to_string()),
        ),
    ] {
        if !value.trim().is_empty() {
            lines.push(format!("{}: {}", catalog.text(key), value.trim()));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn station_details_skip_unknown_properties() {
        let catalog = apricot_app::embedded_catalog("en");
        let station = Station {
            name: "Radio Test".into(),
            country: "Slovenia".into(),
            bitrate: 128,
            votes: 3,
            lastcheckok: 1,
            url: "https://example.test/live".into(),
            ..Station::default()
        };
        assert_eq!(
            station_details(&catalog, &station),
            "Radio Test\nCountry: Slovenia\nBitrate: 128 kbps\nVotes: 3\nListens: 0\n\
             Working at the last check: Yes\nStream: https://example.test/live"
        );
    }

    #[test]
    fn catalog_errors_are_worded_for_people() {
        let catalog = apricot_app::embedded_catalog("en");
        assert_eq!(
            radio_error(&catalog, &RadioError::Http(503)),
            "The radio catalog answered with an error (HTTP 503)."
        );
        assert_eq!(
            radio_error(&catalog, &RadioError::VoteRejected("too often".into())),
            "The radio catalog did not accept the vote: too often"
        );
    }

    #[derive(Default)]
    struct Spoken(Vec<String>);

    impl SourcesUi for Spoken {
        fn browse(&mut self, _browser: &SourceBrowser) -> Option<SourceSelection> {
            None
        }
        fn form(&mut self, _form: &FeatureForm) -> Option<FormValues> {
            None
        }
        fn status(&mut self, message: &str, announce: bool) {
            if announce {
                self.0.push(message.to_owned());
            }
        }
        fn error(&mut self, message: &str) {
            self.0.push(message.to_owned());
        }
        fn play(&mut self, _item: MediaItem) {}
        fn choose_file(&mut self, _title: &str) -> Option<PathBuf> {
            None
        }
        fn choose_folder(&mut self, _title: &str) -> Option<PathBuf> {
            None
        }
        fn copy(&mut self, _text: &str) -> bool {
            false
        }
        fn cue(&mut self, _started: bool) {}
        fn ask_yes_no(&mut self, _message: &str) -> bool {
            false
        }
    }

    #[test]
    fn only_new_song_titles_are_announced() {
        let mut controller = SourcesController::default();
        let mut ui = Spoken::default();
        let url = Some("https://stream.example.test/aac-320");
        for title in ["aac-320", "Artist - Song", "Artist - Song", "Artist - Next"] {
            controller.stream_metadata(&mut ui, Some("station"), Some(title), url);
        }
        assert_eq!(ui.0, ["Artist - Song", "Artist - Next"]);
        controller.radio.announce_metadata = false;
        controller.stream_metadata(&mut ui, Some("station"), Some("Quiet - Song"), url);
        assert_eq!(ui.0.len(), 2);
        assert_eq!(controller.metadata_title, "Quiet - Song");
    }

    #[test]
    fn station_actions_reuse_the_familiar_shortcuts() {
        assert_eq!(shortcut_action("radio_favorite"), "add_favorite");
        assert_eq!(shortcut_action("radio_copy_url"), "copy_link");
        assert_eq!(shortcut_action("radio_vote"), "radio_vote");
    }
}
