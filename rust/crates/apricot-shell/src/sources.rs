//! Shared source browsers and recorder ownership. Native windows render the
//! same model and report actions; network/capture work stays off the UI thread.

use crate::{FeatureForm, FormField, FormValue, FormValues};
use apricot_core::{MediaItem, TranslationCatalog};
use apricot_platform::{
    iptv::{ChannelKind, EpgGuide, IptvChannel, IptvClient, IptvError, IptvSource, SourceKind},
    radio::{
        DirectoryEntry, DirectoryKind, RadioCategory, RadioClient, RadioError, RadioFilters,
        RadioLibrary, RadioPage, RadioRequest, Station,
    },
    recording::{
        self, CaptureKind, CaptureSource, RecordingError, RecordingEvent, RecordingFeedback,
        RecordingFormat, RecordingSession, RecordingSettings,
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
    /// A label for each row when it depends on the row, such as "Add to
    /// radio favorites" or "Remove from radio favorites"; empty otherwise.
    pub row_labels: Vec<String>,
}

impl SourceAction {
    /// The label the action menu shows while `row` is selected.
    #[must_use]
    pub fn label_for(&self, row: Option<usize>) -> &str {
        row.and_then(|row| self.row_labels.get(row))
            .map_or(self.label.as_str(), String::as_str)
    }
}
#[derive(Clone, Debug)]
pub struct SourceBrowser {
    pub title: String,
    /// The list's accessible name; a different one from the title, so a
    /// screen reader does not say the same words twice (review 2.1).
    pub list_name: String,
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
    /// A kept channel opened again from its source: it plays with the
    /// options its start asked for (review 5, B15).
    fn play_kept(&mut self, item: MediaItem) {
        self.play(item);
    }
    fn choose_file(&mut self, title: &str) -> Option<PathBuf>;
    fn choose_folder(&mut self, title: &str) -> Option<PathBuf>;
    fn copy(&mut self, text: &str) -> bool;
    fn cue(&mut self, started: bool);
    /// A warning with Yes and No, No being the default.
    fn ask_yes_no(&mut self, message: &str) -> bool;
    /// A number that changes whenever the main window shows another screen,
    /// so work that finishes after the user moved on is dropped instead of
    /// opening a window over the new screen (review 2.1).
    fn navigation_mark(&self) -> u64 {
        0
    }
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
        /// A channel kept in favorites, a playlist or the queue, not
        /// chosen in the IPTV lists (review 4, A1): the kept entry.
        saved: Option<Box<MediaItem>>,
    },
    Sources(Vec<CaptureSource>),
    Message(String),
}

pub struct SourcesController {
    loaded: Option<PathBuf>,
    blocked: BTreeSet<String>,
    radio: RadioLibrary,
    station_rows: Vec<Station>,
    /// Pages in a row that the filters left empty (review 2.1).
    radio_empty_pages: u8,
    /// `Some(true)` while the list shows the radio favorites, `Some(false)`
    /// the recently played; Refresh then reads that list again and catalog
    /// filters do not apply (review 2.1).
    radio_personal: Option<bool>,
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
    /// The main window's screen when `pending` was started.
    pending_mark: u64,
    /// The localized "Loading" announcement of the current language.
    loading: String,
    hub_selected: usize,
    iptv_selected: usize,
    /// The episodes of `episode_series`, kept apart from the channel list.
    episodes: Vec<IptvChannel>,
    episode_series: Option<IptvChannel>,
    episode_selected: usize,
    /// The IPTV channel that plays, for its programme on request.
    playing_iptv: Option<String>,
    /// The playing channel and its source's guide, kept apart from what is
    /// browsed, so another source's list never answers for it (review 2.1).
    playing_iptv_context: Option<(IptvChannel, EpgGuide)>,
    /// The guide of a kept channel's own source, read in the background
    /// after the channel started: (source id, channel id, answer)
    /// (review 5, B16).
    kept_guide: Option<(String, String, Receiver<EpgGuide>)>,
    /// "Now: programme", spoken after the player names the channel.
    pending_programme: Option<String>,
    /// The recording shortcut asked for a source first; record after it.
    start_after_sources: bool,
    /// The clock time the current recording started.
    recording_since: String,
    metadata_identity: String,
    metadata_title: String,
    /// The list a station or channel was played from, with its id: leaving
    /// the player goes back to it, one step, as everywhere else.
    player_return: Option<(SourceReturn, String)>,
    /// The IPTV list whose channel is being resolved.
    opening_from: IptvScreen,
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
            radio_empty_pages: 0,
            radio_personal: None,
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
            pending_mark: 0,
            loading: String::new(),
            hub_selected: 0,
            iptv_selected: 0,
            episodes: Vec::new(),
            episode_series: None,
            episode_selected: 0,
            playing_iptv: None,
            playing_iptv_context: None,
            kept_guide: None,
            pending_programme: None,
            start_after_sources: false,
            recording_since: String::new(),
            metadata_identity: String::new(),
            metadata_title: String::new(),
            player_return: None,
            opening_from: IptvScreen::Channels,
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
        // Playlists saved by earlier betas with a login in the address get
        // it protected now.
        let mut protected = false;
        for source in &mut self.iptv_sources {
            if source.kind == SourceKind::Playlist
                && source.protected_credentials.is_empty()
                && apricot_platform::iptv::address_is_private(&source.address)
            {
                let address = source.address.clone();
                protected |= source.set_playlist_address(&address).is_ok();
            }
            // Guide addresses with a login are protected too (review 2.1).
            if source.protected_epg.is_empty()
                && apricot_platform::iptv::address_is_private(&source.epg_address)
            {
                let address = source.epg_address.clone();
                protected |= source.set_epg_address(&address).is_ok();
            }
        }
        if protected {
            let _ = self.save_iptv(environment);
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
            "open_radio" => self.radio_navigate(ui, environment, RadioScreen::Menu),
            "open_iptv" => self.iptv_navigate(ui, environment, IptvScreen::Sources),
            "open_recording" => self.recording_panel(ui, environment),
            "recording_start" => self.start_recording(ui, environment),
            "recording_pause" => self.pause_recording(ui, &catalog),
            "recording_stop" => self.stop_recording(ui, &catalog),
            "open_recordings_folder" => self.open_recordings_folder(ui, &catalog),
            "radio_now_playing" => {
                if let Some(programme) = self.playing_programme(&catalog) {
                    ui.status(&programme, true);
                } else {
                    ui.status(
                        if self.metadata_title.is_empty() {
                            catalog.text("radio_no_song")
                        } else {
                            &self.metadata_title
                        },
                        true,
                    );
                }
            }
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
        self.pending_mark = ui.navigation_mark();
        std::thread::spawn(move || {
            let _ = send.send(work());
        });
        ui.status(&self.loading, true);
    }

    /// A private IPTV channel kept in favorites, a playlist or the queue
    /// plays: its source opens it again, because only a reference to it is
    /// kept on disk (review 4, A1). The host starts the opened channel with
    /// the options it was asked to play with.
    pub fn play_saved_iptv(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        item: &MediaItem,
    ) {
        self.load(ui, environment);
        let catalog = environment.catalog();
        let source = item.iptv_ids().and_then(|(source_id, _)| {
            self.iptv_sources
                .iter()
                .find(|source| source.id == source_id)
                .cloned()
        });
        let Some(source) = source else {
            ui.error(catalog.text("iptv_error_source_gone"));
            return;
        };
        catalog
            .text("iptv_opening_channel")
            .clone_into(&mut self.loading);
        let item = item.clone();
        let proxy = environment.proxy.clone();
        self.spawn(ui, move || {
            let client =
                IptvClient::new(Some(&proxy)).map_err(|error| iptv_error(&catalog, error))?;
            Ok(Work::PlayIptv {
                channel: client
                    .reopen(&source, &item)
                    .map_err(|error| iptv_error(&catalog, error))?,
                source,
                saved: Some(Box::new(item)),
            })
        });
    }

    /// A list or a channel is being loaded; a running recording alone is
    /// not loading (review 4, B3).
    #[must_use]
    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }

    /// Back while the next list loads: the load is dropped, its result is
    /// never shown (review 5, B12).
    pub fn cancel_loading(&mut self) {
        self.pending = None;
        self.loading.clear();
    }

    /// Reads the guide of a kept channel's own source in the background;
    /// the channel's programme waits for it (review 5, B16).
    fn read_kept_guide(
        &mut self,
        environment: &SourcesEnvironment,
        source: &IptvSource,
        channel: &str,
    ) {
        let address = source.epg_url();
        if address.trim().is_empty() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let proxy = environment.proxy.clone();
        std::thread::spawn(move || {
            if let Ok(client) = IptvClient::new(Some(&proxy))
                && let Ok(guide) = client.epg(&address)
            {
                let _ = sender.send(guide);
            }
        });
        self.kept_guide = Some((source.id.clone(), channel.to_owned(), receiver));
    }

    /// A kept channel's guide arrived: its programme comes with the next
    /// stream details, while the same channel still plays.
    fn poll_kept_guide(&mut self) {
        let Some((_, channel, receiver)) = self.kept_guide.as_ref() else {
            return;
        };
        let guide = match receiver.try_recv() {
            Ok(guide) => guide,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.kept_guide = None;
                return;
            }
        };
        if let Some((playing, kept)) = self.playing_iptv_context.as_mut()
            && playing.id == *channel
        {
            *kept = guide;
        }
        self.kept_guide = None;
    }

    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.kept_guide.is_some()
            || self.pending.is_some()
            || self
                .recorder
                .as_ref()
                .is_some_and(RecordingSession::is_active)
    }
    /// Quitting: the recording is stopped and saved before the app ends.
    /// The message for the user when it did not end as a normal recording.
    pub fn finish_recording(&mut self, catalog: &TranslationCatalog) -> Option<String> {
        let mut recorder = self.recorder.take()?;
        match recorder.finish()? {
            Ok(_) => None,
            Err(error) => Some(recording_error(catalog, &error)),
        }
    }

    /// Stop and Pause of the running recording, for a window that holds
    /// this controller out of reach (review 2.1).
    #[must_use]
    pub fn recording_controls(&self) -> Option<recording::RecordingControls> {
        self.recorder
            .as_ref()
            .filter(|recorder| recorder.is_active())
            .map(RecordingSession::controls)
    }

    #[must_use]
    pub fn is_recording(&self) -> bool {
        self.recorder
            .as_ref()
            .is_some_and(RecordingSession::is_active)
    }

    fn poll_recording(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        if let Some(recorder) = &mut self.recorder {
            let catalog = environment.catalog();
            let sounds = matches!(
                self.recording.feedback,
                RecordingFeedback::Sounds | RecordingFeedback::Both
            );
            let speech = matches!(
                self.recording.feedback,
                RecordingFeedback::Speech | RecordingFeedback::Both
            );
            for event in recorder.poll() {
                match event {
                    RecordingEvent::Started { output, .. } => {
                        self.recording_since = chrono::Local::now().format("%H:%M").to_string();
                        if sounds {
                            ui.cue(true);
                        }
                        ui.status(
                            &catalog
                                .text("recording_started")
                                .replace("{file}", &file_name(&output)),
                            speech,
                        );
                    }
                    RecordingEvent::SourceFailed { name, error } => {
                        ui.error(&match error {
                            Some(error) => catalog
                                .text("recording_source_failed")
                                .replace("{name}", &name)
                                .replace("{error}", &error),
                            None => catalog
                                .text("recording_source_not_ready")
                                .replace("{name}", &name),
                        });
                    }
                    // A pause said by the global key while a list window
                    // held this controller is not said twice.
                    RecordingEvent::Paused(_) if recorder.take_spoken_pause() => {}
                    RecordingEvent::Paused(paused) => ui.status(
                        catalog.text(if paused {
                            "recording_paused"
                        } else {
                            "recording_resumed"
                        }),
                        true,
                    ),
                    RecordingEvent::Finished(result) => {
                        if sounds {
                            ui.cue(false);
                        }
                        match result {
                            Ok(path) => ui.status(
                                &catalog
                                    .text("recording_saved")
                                    .replace("{file}", &file_name(&path)),
                                speech,
                            ),
                            Err(error) => ui.error(&recording_error(&catalog, &error)),
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)] // One arm for each kind of finished work.
    pub fn poll(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) -> bool {
        self.poll_recording(ui, environment);
        self.poll_kept_guide();
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
        // The user went to another screen meanwhile: the late answer would
        // take the focus from it.
        if ui.navigation_mark() != self.pending_mark {
            return self.is_busy();
        }
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
                // Filters can leave a whole page empty while more follow:
                // the next pages are read, up to ten, before "no stations"
                // (review 2.1).
                let nothing_new = self.station_rows.len() == first_new;
                if nothing_new && page.has_more && self.radio_empty_pages < 10 {
                    self.radio_empty_pages += 1;
                    self.load_radio(ui, environment, false);
                    return self.is_busy();
                }
                self.radio_empty_pages = 0;
                let screen = if self.station_rows.is_empty() {
                    ui.status(environment.catalog().text("radio_no_stations"), true);
                    RadioScreen::Menu
                } else {
                    RadioScreen::Stations
                };
                self.radio_navigate(ui, environment, screen);
            }
            Ok(Work::RadioDirectory {
                kind,
                entries,
                offset,
            }) => self.radio_directory_list(ui, environment, kind, &entries, offset),
            Ok(Work::Iptv {
                source,
                channels,
                guide,
                warning,
            }) => {
                self.iptv_active = Some(source);
                self.channels = channels;
                self.guide = guide;
                self.channel_kind = None;
                self.channel_group.clear();
                self.channel_query.clear();
                self.channel_selected = 0;
                self.episodes.clear();
                if let Some(warning) = warning {
                    ui.status(&warning, true);
                }
                self.iptv_navigate(ui, environment, IptvScreen::Kinds);
            }
            Ok(Work::IptvSeries { source, channels }) => {
                self.iptv_active = Some(source);
                self.episodes = channels;
                self.iptv_navigate(ui, environment, IptvScreen::Episodes);
            }
            Ok(Work::PlayIptv {
                source,
                channel,
                saved,
            }) => {
                let catalog = environment.catalog();
                if let Some(item) = channel.media(&source) {
                    if saved.is_some() {
                        ui.play_kept(with_kept_entry(item, saved.as_deref()));
                    } else {
                        ui.play(item);
                    }
                    // A kept channel goes back to the list it was kept in,
                    // not to the IPTV lists.
                    self.player_return = saved
                        .is_none()
                        .then(|| (SourceReturn::Iptv(self.opening_from), channel.id.clone()));
                    self.playing_iptv = Some(channel.id.clone());
                    // The guide read last belongs to the source browsed last;
                    // a kept channel of another source gets no programme from
                    // it, even with the same guide id (review 5, B16).
                    let guide = if self
                        .iptv_active
                        .as_ref()
                        .is_some_and(|active| active.id == source.id)
                    {
                        self.guide.clone()
                    } else {
                        self.read_kept_guide(environment, &source, &channel.id);
                        EpgGuide::default()
                    };
                    self.playing_iptv_context = Some((channel.clone(), guide.clone()));
                    // The player names the channel when it starts; the
                    // programme follows with its first stream details.
                    let now = chrono::Utc::now().timestamp();
                    self.pending_programme =
                        guide.now_next(&channel.epg_id, now).0.map(|programme| {
                            catalog
                                .text("iptv_now_short")
                                .replace("{title}", &programme.title)
                        });
                } else {
                    ui.error(catalog.text("iptv_no_stream"));
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

    /// After the player announced what started: the IPTV programme.
    /// Leaving the player: back to the station or channel list it was
    /// played from, when the player still plays that station or channel.
    /// `true` when a list opened. The return is used once either way.
    pub fn resume_after_player(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        station: Option<&str>,
        channel: Option<&str>,
    ) -> bool {
        let Some((target, id)) = self.player_return.take() else {
            return false;
        };
        match target {
            SourceReturn::Radio if station == Some(id.as_str()) => {
                self.radio_navigate(ui, environment, RadioScreen::Stations);
            }
            SourceReturn::Iptv(screen) if channel == Some(id.as_str()) => {
                self.iptv_navigate(ui, environment, screen);
            }
            _ => return false,
        }
        true
    }

    /// The player went to the main menu: nothing to go back to.
    pub fn forget_player_return(&mut self) {
        self.player_return = None;
    }

    pub fn playback_started(&mut self, ui: &mut dyn SourcesUi) {
        if let Some(programme) = self.pending_programme.take() {
            ui.status(&programme, true);
        }
    }

    pub fn stream_metadata(
        &mut self,
        ui: &mut dyn SourcesUi,
        station_id: Option<&str>,
        title: Option<&str>,
        stream_url: Option<&str>,
        iptv_channel: Option<&str>,
    ) {
        self.playing_iptv = iptv_channel.map(str::to_owned);
        if self.playing_iptv.is_none() {
            self.pending_programme = None;
        }
        if self
            .playing_iptv_context
            .as_ref()
            .is_some_and(|(channel, _)| Some(channel.id.as_str()) != iptv_channel)
        {
            self.playing_iptv_context = None;
        }
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
            list_name: title.into(),
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
                    row_labels: Vec::new(),
                })
                .collect(),
            actions_button: catalog.text("sources_actions").to_owned(),
            back_button: catalog.text("sources_back").to_owned(),
        }
    }

    /// The radio screens as one loop, so going back and forth never nests
    /// calls. A screen returns the next one, or `None` when it closed Radio
    /// or started loading.
    fn radio_navigate(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        mut screen: RadioScreen,
    ) {
        loop {
            let next = match screen {
                RadioScreen::Menu => self.radio_hub(ui, environment),
                RadioScreen::Search => self.search_radio(ui, environment),
                RadioScreen::Stations => self.radio_list(ui, environment),
                RadioScreen::Settings => {
                    self.radio_settings(ui, environment);
                    Some(RadioScreen::Menu)
                }
            };
            match next {
                Some(next) => screen = next,
                None => return,
            }
        }
    }

    fn radio_hub(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
    ) -> Option<RadioScreen> {
        let catalog = environment.catalog();
        let rows = RADIO_HUB
            .iter()
            .map(|key| catalog.text(key).to_owned())
            .collect();
        let choice = ui.browse(&Self::browser(
            environment,
            catalog.text("radio_title"),
            rows,
            self.hub_selected,
            &[("open", catalog.text("open"))],
        ))?;
        self.hub_selected = choice.index;
        match choice.index {
            0 => return Some(RadioScreen::Search),
            1 => {
                self.request.category = RadioCategory::All;
                self.request.search.clear();
                self.load_radio(ui, environment, true);
            }
            2 | 3 => {
                self.radio_personal = Some(choice.index == 2);
                self.station_rows = if choice.index == 2 {
                    self.radio.favorites.clone()
                } else {
                    self.radio.recent.clone()
                };
                self.station_selected = 0;
                self.radio_has_more = false;
                if self.station_rows.is_empty() {
                    ui.status(catalog.text("radio_no_stations"), true);
                    return Some(RadioScreen::Menu);
                }
                return Some(RadioScreen::Stations);
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
            14 => return Some(RadioScreen::Settings),
            _ => {}
        }
        None
    }

    fn search_radio(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
    ) -> Option<RadioScreen> {
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
            return Some(RadioScreen::Menu);
        };
        self.request.search = text(&values, "query");
        self.request.category = RadioCategory::All;
        self.load_radio(ui, environment, true);
        None
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
            self.radio_empty_pages = 0;
            self.radio_personal = None;
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
    fn radio_list(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
    ) -> Option<RadioScreen> {
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
            ];
            // Catalog filters do not apply to a personal list.
            // Each scope has its own form with its own values (review 5,
            // B17).
            if self.radio_personal.is_none() {
                actions.push(("radio_filters", catalog.text("radio_filters_this_list")));
                actions.push(("radio_filters_all", catalog.text("radio_filters_all_lists")));
            }
            actions.extend([
                ("radio_refresh", catalog.text("radio_refresh")),
                ("radio_search", catalog.text("radio_search_stations")),
                ("radio_clear_history", catalog.text("radio_clear_history")),
            ]);
            if self.radio_has_more {
                actions.push(("radio_more", catalog.text("radio_more")));
            }
            let rows = Station::list_rows(&self.station_rows);
            let mut browser = Self::browser(
                environment,
                catalog.text("radio_stations"),
                rows,
                self.station_selected,
                &actions,
            );
            catalog.text("radio_title").clone_into(&mut browser.title);
            // The menu says what the command does to the selected station.
            if let Some(favorite) = browser
                .actions
                .iter_mut()
                .find(|action| action.id == "radio_favorite")
            {
                favorite.row_labels = self
                    .station_rows
                    .iter()
                    .map(|station| {
                        catalog
                            .text(if self.radio.is_favorite(station) {
                                "radio_favorite_remove"
                            } else {
                                "radio_favorite_add"
                            })
                            .to_owned()
                    })
                    .collect();
            }
            let Some(choice) = ui.browse(&browser) else {
                // Back returns to the radio menu.
                return Some(RadioScreen::Menu);
            };
            self.station_selected = choice.index;
            match choice.action.as_str() {
                "radio_more" => {
                    self.load_radio(ui, environment, false);
                    return None;
                }
                "radio_refresh" => {
                    if let Some(favorites) = self.radio_personal {
                        self.station_rows = if favorites {
                            self.radio.favorites.clone()
                        } else {
                            self.radio.recent.clone()
                        };
                        if self.station_rows.is_empty() {
                            ui.status(catalog.text("radio_no_stations"), true);
                            return Some(RadioScreen::Menu);
                        }
                        self.station_selected =
                            self.station_selected.min(self.station_rows.len() - 1);
                        continue;
                    }
                    self.load_radio(ui, environment, true);
                    return None;
                }
                "radio_filters" | "radio_filters_all" => {
                    if self.radio_filters(ui, environment, choice.action == "radio_filters_all") {
                        self.load_radio(ui, environment, true);
                        return None;
                    }
                    continue;
                }
                "radio_search" => return Some(RadioScreen::Search),
                "radio_clear_history" => {
                    if ui.ask_yes_no(catalog.text("radio_clear_history_confirm")) {
                        let mut next = self.radio.clone();
                        next.recent.clear();
                        let previous = std::mem::replace(&mut self.radio, next);
                        match self.save_radio(environment) {
                            Ok(()) => {
                                ui.status(catalog.text("radio_history_cleared"), true);
                                // The shown history is gone too.
                                if self.radio_personal == Some(false) {
                                    self.station_rows.clear();
                                    return Some(RadioScreen::Menu);
                                }
                            }
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
                    self.player_return = Some((SourceReturn::Radio, station.stationuuid.clone()));
                    // The catalog counts plays; nothing waits for the answer.
                    let proxy = environment.proxy.clone();
                    let id = station.stationuuid;
                    std::thread::spawn(move || {
                        let _ =
                            RadioClient::new(Some(&proxy)).and_then(|client| client.clicked(&id));
                    });
                    return None;
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
                    return None;
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
        entries: &[DirectoryEntry],
        offset: usize,
    ) {
        let catalog = environment.catalog();
        // Countries come as one whole list; the others in pages.
        let more =
            kind != DirectoryKind::Countries && entries.len() >= apricot_platform::radio::PAGE_SIZE;
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
            self.radio_navigate(ui, environment, RadioScreen::Menu);
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
                country: entry.country.clone(),
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

    #[allow(clippy::too_many_lines)] // One form with all its fields.
    fn radio_filters(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        global: bool,
    ) -> bool {
        let catalog = environment.catalog();
        let key = format!("{:?}", self.request.category);
        let local_only = self
            .radio
            .local_filters
            .get(&key)
            .cloned()
            .unwrap_or_default();
        // This list's form shows what filters it: its own values, and the
        // filters for all lists where it has none (review 4, B15). The form
        // for all lists shows their own values, which this list's may
        // override (review 5, B17).
        let local = if global {
            self.radio.global_filters.clone()
        } else {
            shown_filters(&local_only, &self.radio.global_filters)
        };
        let form = FeatureForm::localized(
            &catalog,
            catalog.text(if global {
                "radio_filters_all_lists"
            } else {
                "radio_filters_this_list"
            }),
            vec![
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
        let typed = RadioFilters {
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
        // The form for all lists is theirs entirely; for this list only the
        // fields the user changed become its own (review 2.1).
        let filters = if global {
            typed
        } else {
            changed_filters(&local, &typed, local_only.clone())
        };
        if filters.maximum_bitrate > 0 && filters.maximum_bitrate < filters.minimum_bitrate {
            ui.error(catalog.text("radio_filter_bitrate_order"));
            return false;
        }
        let previous = self.radio.clone();
        if global {
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

    /// The IPTV screens as one loop, so going back and forth never nests
    /// calls. A screen returns the next one, or `None` to close IPTV.
    fn iptv_navigate(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        mut screen: IptvScreen,
    ) {
        loop {
            let next = match screen {
                IptvScreen::Sources => self.iptv_hub(ui, environment),
                IptvScreen::Kinds => Some(self.iptv_kinds(ui, environment)),
                IptvScreen::Groups => Some(self.iptv_groups(ui, environment)),
                IptvScreen::Channels => self.channel_list(ui, environment),
                IptvScreen::Episodes => self.episode_list(ui, environment),
            };
            match next {
                Some(next) => screen = next,
                None => return,
            }
        }
    }

    fn iptv_hub(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
    ) -> Option<IptvScreen> {
        let catalog = environment.catalog();
        if self.iptv_sources.is_empty() {
            ui.status(catalog.text("iptv_no_sources"), true);
            if !self.edit_iptv_source(ui, environment, None) {
                return None;
            }
        }
        loop {
            let rows = self
                .iptv_sources
                .iter()
                .map(|source| source.name.clone())
                .collect();
            let selection = ui.browse(&Self::browser(
                environment,
                catalog.text("iptv_sources"),
                rows,
                self.iptv_selected,
                &[
                    ("open", catalog.text("open")),
                    ("add", catalog.text("iptv_add_source")),
                    ("edit", catalog.text("iptv_edit_source")),
                    ("remove", catalog.text("iptv_remove_source")),
                ],
            ))?;
            self.iptv_selected = selection.index;
            match selection.action.as_str() {
                "add" => {
                    if self.edit_iptv_source(ui, environment, None) {
                        self.iptv_selected = self.iptv_sources.len().saturating_sub(1);
                    }
                }
                "edit" => {
                    self.edit_iptv_source(ui, environment, Some(selection.index));
                }
                "remove" => {
                    if let Some(source) = self.iptv_sources.get(selection.index)
                        && ui.ask_yes_no(
                            &catalog
                                .text("iptv_remove_confirm")
                                .replace("{name}", &source.name),
                        )
                    {
                        let previous = self.iptv_sources.clone();
                        self.iptv_sources.remove(selection.index);
                        match self.save_iptv(environment) {
                            Ok(()) => {
                                self.iptv_selected = self
                                    .iptv_selected
                                    .min(self.iptv_sources.len().saturating_sub(1));
                                ui.status(catalog.text("iptv_source_removed"), true);
                                if self.iptv_sources.is_empty() {
                                    return None;
                                }
                            }
                            Err(error) => {
                                self.iptv_sources = previous;
                                ui.error(&error);
                            }
                        }
                    }
                }
                _ => {
                    if let Some(source) = self.iptv_sources.get(selection.index).cloned() {
                        self.load_iptv(ui, environment, source);
                        return None;
                    }
                }
            }
        }
    }

    /// Adds a source or edits one; `true` when it was saved.
    #[allow(clippy::too_many_lines)] // One form with its checks.
    fn edit_iptv_source(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        index: Option<usize>,
    ) -> bool {
        let catalog = environment.catalog();
        let old = index
            .and_then(|index| self.iptv_sources.get(index))
            .cloned();
        let credentials = old
            .as_ref()
            .and_then(|source| source.credentials().ok())
            .unwrap_or(Value::Null);
        let mut form = FeatureForm::localized(
            &catalog,
            catalog.text("iptv_source_title"),
            vec![
                FormField::text(
                    "name",
                    catalog.text("iptv_source_name"),
                    old.as_ref().map_or("", |source| &source.name),
                ),
                FormField::choice(
                    "kind",
                    catalog.text("iptv_source_kind"),
                    vec![
                        catalog.text("iptv_kind_playlist").to_owned(),
                        catalog.text("iptv_kind_xtream").to_owned(),
                        catalog.text("iptv_kind_stalker").to_owned(),
                    ],
                    old.as_ref().map_or(0, |source| match source.kind {
                        SourceKind::Playlist => 0,
                        SourceKind::Xtream => 1,
                        SourceKind::Stalker => 2,
                    }),
                ),
                FormField::file(
                    "address",
                    catalog.text("iptv_source_address"),
                    old.as_ref()
                        .map_or_else(String::new, IptvSource::playlist_address),
                    &["m3u", "m3u8"],
                    catalog.text("browse"),
                ),
                FormField::text(
                    "username",
                    catalog.text("iptv_username"),
                    credentials["username"].as_str().unwrap_or_default(),
                ),
                FormField::secret(
                    "password",
                    catalog.text("iptv_password"),
                    credentials["password"].as_str().unwrap_or_default(),
                ),
                FormField::text(
                    "mac",
                    catalog.text("iptv_mac"),
                    credentials["mac"].as_str().unwrap_or_default(),
                ),
                FormField::file(
                    "epg",
                    catalog.text("iptv_guide_address"),
                    old.as_ref().map(IptvSource::epg_url).unwrap_or_default(),
                    &["xml", "gz"],
                    catalog.text("browse"),
                ),
                FormField::checkbox(
                    "hls",
                    catalog.text("iptv_prefer_hls"),
                    old.as_ref().is_some_and(|source| source.prefer_hls),
                ),
            ],
        );
        loop {
            let Some(values) = ui.form(&form) else {
                return false;
            };
            // A rejected form comes back with what was typed.
            for field in &mut form.fields {
                match (&mut field.kind, values.get(&field.id)) {
                    (
                        crate::FormFieldKind::Text { value, .. }
                        | crate::FormFieldKind::File { value, .. },
                        Some(FormValue::Text(text)),
                    ) => text.clone_into(value),
                    (
                        crate::FormFieldKind::Choice { selected, .. },
                        Some(FormValue::Choice(choice)),
                    ) => *selected = *choice,
                    (
                        crate::FormFieldKind::Checkbox { checked },
                        Some(FormValue::Checkbox(value)),
                    ) => *checked = *value,
                    _ => {}
                }
            }
            let name = text(&values, "name").trim().to_owned();
            let address = text(&values, "address").trim().to_owned();
            if name.is_empty() || address.is_empty() {
                ui.error(catalog.text("iptv_source_incomplete"));
                continue;
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
                epg_address: String::new(),
                protected_epg: String::new(),
                prefer_hls: checked(&values, "hls"),
            };
            if let Err(error) = source.set_epg_address(&text(&values, "epg")) {
                ui.error(&iptv_error(&catalog, error));
                continue;
            }
            let protected = if source.kind == SourceKind::Playlist {
                source.set_playlist_address(&text(&values, "address"))
            } else {
                Ok(())
            };
            if let Err(error) = protected {
                ui.error(&iptv_error(&catalog, error));
                continue;
            }
            if source.kind != SourceKind::Playlist
                && let Err(error) = source.set_credentials(
                    &text(&values, "username"),
                    &text(&values, "password"),
                    &text(&values, "mac"),
                )
            {
                ui.error(&iptv_error(&catalog, error));
                continue;
            }
            let previous = self.iptv_sources.clone();
            if let Some(index) = index {
                if let Some(current) = self.iptv_sources.get_mut(index) {
                    *current = source;
                }
            } else {
                self.iptv_sources.push(source);
            }
            return match self.save_iptv(environment) {
                Ok(()) => {
                    ui.status(catalog.text("iptv_source_saved"), true);
                    true
                }
                Err(error) => {
                    self.iptv_sources = previous;
                    ui.error(&error);
                    false
                }
            };
        }
    }

    fn load_iptv(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        source: IptvSource,
    ) {
        let proxy = environment.proxy.clone();
        let catalog = environment.catalog();
        self.spawn(ui, move || {
            let client =
                IptvClient::new(Some(&proxy)).map_err(|error| iptv_error(&catalog, error))?;
            let channels = client
                .load(&source)
                .map_err(|error| iptv_error(&catalog, error))?;
            let mut warning = None;
            let guide_address = source.epg_url();
            let guide = if guide_address.trim().is_empty() {
                EpgGuide::default()
            } else {
                client.epg(&guide_address).unwrap_or_else(|error| {
                    warning = Some(
                        catalog
                            .text("iptv_guide_failed")
                            .replace("{error}", &iptv_error(&catalog, error)),
                    );
                    EpgGuide::default()
                })
            };
            Ok(Work::Iptv {
                source,
                channels,
                guide,
                warning,
            })
        });
    }

    fn kinds_present(&self) -> Vec<ChannelKind> {
        IPTV_KINDS
            .iter()
            .copied()
            .filter(|kind| self.channels.iter().any(|channel| channel.kind == *kind))
            .collect()
    }

    fn groups_present(&self) -> Vec<(String, usize)> {
        let mut groups = BTreeMap::<String, usize>::new();
        for channel in self
            .channels
            .iter()
            .filter(|channel| self.channel_kind.is_none_or(|kind| channel.kind == kind))
        {
            *groups.entry(channel.group.clone()).or_default() += 1;
        }
        groups.into_iter().collect()
    }

    /// Where Back goes from the channel list: the screens that had a
    /// choice to make.
    fn channels_back(&self) -> IptvScreen {
        if self.groups_present().len() > 1 {
            IptvScreen::Groups
        } else if self.kinds_present().len() > 1 {
            IptvScreen::Kinds
        } else {
            IptvScreen::Sources
        }
    }

    fn iptv_kinds(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
    ) -> IptvScreen {
        let kinds = self.kinds_present();
        if kinds.len() <= 1 {
            self.channel_kind = kinds.first().copied();
            return IptvScreen::Groups;
        }
        let catalog = environment.catalog();
        let rows = kinds
            .iter()
            .map(|kind| {
                catalog
                    .text("iptv_kind_row")
                    .replace("{name}", catalog.text(iptv_kind_key(*kind)))
                    .replace(
                        "{count}",
                        &self
                            .channels
                            .iter()
                            .filter(|channel| channel.kind == *kind)
                            .count()
                            .to_string(),
                    )
            })
            .collect();
        let selected = self
            .channel_kind
            .and_then(|kind| kinds.iter().position(|present| *present == kind))
            .unwrap_or(0);
        let Some(choice) = ui.browse(&Self::browser(
            environment,
            self.iptv_active
                .as_ref()
                .map_or(catalog.text("iptv_title"), |source| &source.name),
            rows,
            selected,
            &[("open", catalog.text("open"))],
        )) else {
            return IptvScreen::Sources;
        };
        let kind = kinds.get(choice.index).copied();
        if kind != self.channel_kind {
            self.channel_kind = kind;
            self.channel_group.clear();
            self.channel_selected = 0;
        }
        IptvScreen::Groups
    }

    fn iptv_groups(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
    ) -> IptvScreen {
        let groups = self.groups_present();
        let back = if self.kinds_present().len() > 1 {
            IptvScreen::Kinds
        } else {
            IptvScreen::Sources
        };
        if groups.len() <= 1 {
            self.channel_group = groups
                .first()
                .map(|(name, _)| name.clone())
                .unwrap_or_default();
            return IptvScreen::Channels;
        }
        let catalog = environment.catalog();
        let total: usize = groups.iter().map(|(_, count)| count).sum();
        let mut rows = vec![
            catalog
                .text("iptv_kind_row")
                .replace("{name}", catalog.text("iptv_all_categories"))
                .replace("{count}", &total.to_string()),
        ];
        rows.extend(groups.iter().map(|(name, count)| {
            catalog
                .text("iptv_kind_row")
                .replace(
                    "{name}",
                    if name.is_empty() {
                        catalog.text("iptv_no_category")
                    } else {
                        name
                    },
                )
                .replace("{count}", &count.to_string())
        }));
        let selected = if self.channel_group.is_empty() {
            0
        } else {
            groups
                .iter()
                .position(|(name, _)| *name == self.channel_group)
                .map_or(0, |at| at + 1)
        };
        let title = self
            .channel_kind
            .map_or(catalog.text("iptv_categories"), |kind| {
                catalog.text(iptv_kind_key(kind))
            });
        let Some(choice) = ui.browse(&Self::browser(
            environment,
            title,
            rows,
            selected,
            &[("open", catalog.text("open"))],
        )) else {
            return back;
        };
        let group = choice
            .index
            .checked_sub(1)
            .and_then(|at| groups.get(at))
            .map(|(name, _)| name.clone())
            .unwrap_or_default();
        if group != self.channel_group || choice.index == 0 {
            self.channel_group = group;
            self.channel_selected = 0;
        }
        IptvScreen::Channels
    }

    fn visible_channels(&self) -> Vec<usize> {
        let query = self.channel_query.to_lowercase();
        self.channels
            .iter()
            .enumerate()
            .filter(|(_, channel)| {
                self.channel_kind.is_none_or(|kind| channel.kind == kind)
                    && (self.channel_group.is_empty() || self.channel_group == channel.group)
                    && (query.is_empty() || channel.name.to_lowercase().contains(&query))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn channel_list(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
    ) -> Option<IptvScreen> {
        let catalog = environment.catalog();
        loop {
            let now = chrono::Utc::now().timestamp();
            let indices = self.visible_channels();
            if indices.is_empty() {
                ui.status(catalog.text("iptv_no_channels"), true);
                if self.channel_query.is_empty() {
                    return Some(self.channels_back());
                }
                self.channel_query.clear();
                continue;
            }
            let rows = indices
                .iter()
                .map(|&index| {
                    let channel = &self.channels[index];
                    self.guide.now_next(&channel.epg_id, now).0.map_or_else(
                        || channel.name.clone(),
                        |programme| {
                            catalog
                                .text("iptv_channel_now")
                                .replace("{name}", &channel.name)
                                .replace("{title}", &programme.title)
                        },
                    )
                })
                .collect();
            let mut actions = vec![
                ("play", catalog.text("play")),
                ("iptv_details", catalog.text("iptv_details")),
                ("iptv_search", catalog.text("iptv_search")),
                ("iptv_refresh", catalog.text("iptv_refresh")),
            ];
            let playlist = self
                .iptv_active
                .as_ref()
                .is_some_and(|source| source.kind == SourceKind::Playlist);
            // Provider addresses carry the login, so only playlist
            // addresses are offered for copying.
            if playlist {
                actions.push(("iptv_copy_url", catalog.text("radio_copy_url")));
            }
            let title = if self.channel_group.is_empty() {
                self.channel_kind
                    .map_or(catalog.text("iptv_channels"), |kind| {
                        catalog.text(iptv_kind_key(kind))
                    })
                    .to_owned()
            } else {
                self.channel_group.clone()
            };
            let Some(selected) = ui.browse(&Self::browser(
                environment,
                &title,
                rows,
                self.channel_selected.min(indices.len() - 1),
                &actions,
            )) else {
                self.channel_query.clear();
                return Some(self.channels_back());
            };
            self.channel_selected = selected.index;
            let source = self.iptv_active.clone()?;
            match selected.action.as_str() {
                "iptv_search" => {
                    self.search_channels(ui, &catalog);
                    continue;
                }
                "iptv_refresh" => {
                    self.load_iptv(ui, environment, source);
                    return None;
                }
                _ => {}
            }
            let Some(channel) = indices
                .get(selected.index)
                .and_then(|&index| self.channels.get(index))
                .cloned()
            else {
                continue;
            };
            match selected.action.as_str() {
                "iptv_details" => {
                    self.show_programme_details(ui, &catalog, &channel);
                }
                "iptv_copy_url" => {
                    if ui.copy(&channel.url) {
                        ui.status(catalog.text("url_copied"), true);
                    }
                }
                _ => {
                    if channel.kind == ChannelKind::Series {
                        self.episode_series = Some(channel.clone());
                        self.episode_selected = 0;
                    }
                    self.opening_from = IptvScreen::Channels;
                    self.open_channel(ui, environment, source, channel);
                    return None;
                }
            }
        }
    }

    fn open_channel(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        source: IptvSource,
        channel: IptvChannel,
    ) {
        let proxy = environment.proxy.clone();
        let catalog = environment.catalog();
        self.spawn(ui, move || {
            let client =
                IptvClient::new(Some(&proxy)).map_err(|error| iptv_error(&catalog, error))?;
            if channel.kind == ChannelKind::Series {
                Ok(Work::IptvSeries {
                    channels: client
                        .series(&source, &channel.id)
                        .map_err(|error| iptv_error(&catalog, error))?,
                    source,
                })
            } else {
                Ok(Work::PlayIptv {
                    channel: client
                        .resolve_channel(&channel)
                        .map_err(|error| iptv_error(&catalog, error))?,
                    source,
                    saved: None,
                })
            }
        });
    }

    fn episode_list(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
    ) -> Option<IptvScreen> {
        let catalog = environment.catalog();
        if self.episodes.is_empty() {
            ui.status(catalog.text("iptv_no_episodes"), true);
            return Some(IptvScreen::Channels);
        }
        let rows = self
            .episodes
            .iter()
            .map(|episode| {
                catalog
                    .text("iptv_episode_row")
                    .replace("{season}", &episode.group)
                    .replace("{episode}", &episode.number)
                    .replace("{title}", &episode.name)
            })
            .collect();
        let title = self
            .episode_series
            .as_ref()
            .map_or(catalog.text("iptv_episodes"), |series| &series.name)
            .to_owned();
        let Some(selected) = ui.browse(&Self::browser(
            environment,
            &title,
            rows,
            self.episode_selected,
            &[("play", catalog.text("play"))],
        )) else {
            return Some(IptvScreen::Channels);
        };
        self.episode_selected = selected.index;
        let (Some(source), Some(episode)) = (
            self.iptv_active.clone(),
            self.episodes.get(selected.index).cloned(),
        ) else {
            return Some(IptvScreen::Channels);
        };
        self.opening_from = IptvScreen::Episodes;
        self.open_channel(ui, environment, source, episode);
        None
    }

    fn search_channels(&mut self, ui: &mut dyn SourcesUi, catalog: &TranslationCatalog) {
        let mut form = FeatureForm::localized(
            catalog,
            catalog.text("iptv_search"),
            vec![FormField::text(
                "query",
                catalog.text("iptv_channel_name"),
                &self.channel_query,
            )],
        );
        catalog.text("search").clone_into(&mut form.submit);
        if let Some(values) = ui.form(&form) {
            text(&values, "query")
                .trim()
                .clone_into(&mut self.channel_query);
            self.channel_selected = 0;
        }
    }

    fn show_programme_details(
        &self,
        ui: &mut dyn SourcesUi,
        catalog: &TranslationCatalog,
        channel: &IptvChannel,
    ) {
        let mut form = FeatureForm::localized(
            catalog,
            catalog.text("iptv_details"),
            vec![FormField::read_only(
                "details",
                &channel.name,
                self.programme_details(catalog, channel),
            )],
        );
        catalog.text("ok").clone_into(&mut form.submit);
        let _ = ui.form(&form);
    }

    fn programme_details(&self, catalog: &TranslationCatalog, channel: &IptvChannel) -> String {
        let now = chrono::Utc::now().timestamp();
        let (current, next) = self.guide.now_next(&channel.epg_id, now);
        let mut lines = vec![channel.name.clone()];
        if !channel.group.is_empty() {
            lines.push(format!(
                "{}: {}",
                catalog.text("iptv_category"),
                channel.group
            ));
        }
        if !channel.number.is_empty() {
            lines.push(format!(
                "{}: {}",
                catalog.text("iptv_number"),
                channel.number
            ));
        }
        for (key, programme) in [("iptv_now", current), ("iptv_next", next)] {
            if let Some(programme) = programme {
                lines.push(
                    catalog
                        .text(key)
                        .replace("{start}", &clock(programme.start))
                        .replace("{end}", &clock(programme.end))
                        .replace("{title}", &programme.title),
                );
                if !programme.description.is_empty() {
                    lines.push(programme.description.clone());
                }
            }
        }
        if current.is_none() && next.is_none() {
            lines.push(catalog.text("iptv_no_guide").to_owned());
        }
        lines.join("\n")
    }

    /// The programme now and next of the IPTV channel that plays.
    fn playing_programme(&self, catalog: &TranslationCatalog) -> Option<String> {
        let id = self.playing_iptv.as_deref()?;
        let (channel, guide) = match &self.playing_iptv_context {
            Some((channel, guide)) if channel.id == id => (channel, guide),
            _ => (
                self.channels
                    .iter()
                    .chain(&self.episodes)
                    .find(|channel| channel.id == id)?,
                &self.guide,
            ),
        };
        let now = chrono::Utc::now().timestamp();
        let (current, next) = guide.now_next(&channel.epg_id, now);
        let mut parts = Vec::new();
        for (key, programme) in [("iptv_now", current), ("iptv_next", next)] {
            if let Some(programme) = programme {
                parts.push(
                    catalog
                        .text(key)
                        .replace("{start}", &clock(programme.start))
                        .replace("{end}", &clock(programme.end))
                        .replace("{title}", &programme.title),
                );
            }
        }
        Some(if parts.is_empty() {
            catalog.text("iptv_no_guide").to_owned()
        } else {
            parts.join(" ")
        })
    }

    /// The Recording panel. Closing it leaves a recording running; Stop or
    /// its shortcut ends it.
    fn recording_panel(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        let catalog = environment.catalog();
        let mut selected = 0;
        loop {
            let status = match &self.recorder {
                Some(recorder) if recorder.is_active() && recorder.is_paused() => {
                    catalog.text("recording_status_paused").to_owned()
                }
                Some(recorder) if recorder.is_active() => catalog
                    .text("recording_status_active")
                    .replace("{time}", &self.recording_since),
                _ => catalog.text("recording_status_idle").to_owned(),
            };
            let mut rows = vec![status];
            rows.extend(self.recording.sources.iter().map(|source| {
                catalog
                    .text("recording_source_row")
                    .replace("{name}", &source_label(&catalog, source))
                    .replace("{level}", &format!("{:.0}", source.gain * 100.0))
            }));
            let mut actions = if self.is_recording() {
                vec![
                    ("recording_stop", catalog.text("recording_stop")),
                    ("recording_pause", catalog.text("recording_pause")),
                ]
            } else {
                vec![("recording_start", catalog.text("recording_start"))]
            };
            actions.extend([
                ("add", catalog.text("recording_add_source")),
                ("edit", catalog.text("recording_edit_source")),
                ("remove", catalog.text("recording_remove_source")),
                ("settings", catalog.text("recording_settings")),
                ("folder", catalog.text("recording_open_folder")),
            ]);
            let Some(choice) = ui.browse(&Self::browser(
                environment,
                catalog.text("recording_title"),
                rows,
                selected,
                &actions,
            )) else {
                return;
            };
            selected = choice.index;
            // Row 0 is the status; the sources follow.
            let source = choice.index.checked_sub(1);
            match choice.action.as_str() {
                "recording_start" | "open" if !self.is_recording() => {
                    self.start_recording(ui, environment);
                    return;
                }
                "recording_stop" | "open" => {
                    self.stop_recording(ui, &catalog);
                    return;
                }
                "recording_pause" => self.pause_recording(ui, &catalog),
                "add" => {
                    self.start_after_sources = false;
                    self.spawn(ui, move || {
                        recording::sources()
                            .map(Work::Sources)
                            .map_err(|error| recording_error(&catalog, &error))
                    });
                    return;
                }
                "edit" => {
                    if let Some(index) = source {
                        self.edit_recording_source(ui, environment, index);
                    }
                }
                "remove" => {
                    if let Some(index) =
                        source.filter(|index| *index < self.recording.sources.len())
                    {
                        let previous = self.recording.clone();
                        self.recording.sources.remove(index);
                        match self.save_recording(environment) {
                            Ok(()) => {
                                ui.status(catalog.text("recording_source_removed"), true);
                                selected = selected.min(self.recording.sources.len());
                            }
                            Err(error) => {
                                self.recording = previous;
                                ui.error(&error);
                            }
                        }
                    }
                }
                "settings" => self.recording_settings(ui, environment),
                "folder" => self.open_recordings_folder(ui, &catalog),
                _ => {}
            }
        }
    }

    fn edit_recording_source(
        &mut self,
        ui: &mut dyn SourcesUi,
        environment: &SourcesEnvironment,
        index: usize,
    ) {
        let catalog = environment.catalog();
        let Some(source) = self.recording.sources.get(index).cloned() else {
            return;
        };
        let mut fields = vec![FormField::number(
            "level",
            catalog.text("recording_level"),
            (f64::from(source.gain) * 100.0).round(),
            0.0,
            400.0,
        )];
        if let CaptureKind::Application { exclude, .. } = source.kind {
            fields.push(FormField::checkbox(
                "exclude",
                catalog.text("recording_exclude"),
                exclude,
            ));
        }
        let Some(values) = ui.form(&FeatureForm::localized(
            &catalog,
            source_label(&catalog, &source),
            fields,
        )) else {
            return;
        };
        let previous = self.recording.clone();
        if let Some(source) = self.recording.sources.get_mut(index) {
            #[allow(clippy::cast_possible_truncation)] // A level of 0 to 400 percent.
            let gain = (number(&values, "level") / 100.0) as f32;
            source.gain = gain;
            if let CaptureKind::Application { exclude, .. } = &mut source.kind {
                *exclude = checked(&values, "exclude");
            }
        }
        if let Err(error) = self.save_recording(environment) {
            self.recording = previous;
            ui.error(&error);
        }
    }

    fn add_recording_source(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        let catalog = environment.catalog();
        let start = std::mem::take(&mut self.start_after_sources);
        if self.available_sources.is_empty() {
            ui.error(catalog.text("recording_no_sources_found"));
            return;
        }
        let rows = self
            .available_sources
            .iter()
            .map(|source| source_label(&catalog, source))
            .collect();
        let Some(choice) = ui.browse(&Self::browser(
            environment,
            catalog.text("recording_add_source"),
            rows,
            0,
            &[("add", catalog.text("recording_add_source"))],
        )) else {
            if !start {
                self.recording_panel(ui, environment);
            }
            return;
        };
        if let Some(source) = self.available_sources.get(choice.index).cloned() {
            let previous = self.recording.clone();
            self.recording.sources.push(source);
            if let Err(error) = self.save_recording(environment) {
                self.recording = previous;
                ui.error(&error);
                return;
            }
        }
        // The recording shortcut with no source chosen yet records right
        // after the choice.
        if start {
            self.start_recording(ui, environment);
        } else {
            self.recording_panel(ui, environment);
        }
    }

    #[allow(clippy::too_many_lines)] // One form with its checks.
    /// The recording defaults: Settings, Recording shows these fields in
    /// place, and the recording panel opens them as a form.
    #[must_use]
    pub fn recording_form(&self, catalog: &TranslationCatalog) -> FeatureForm {
        FeatureForm::localized(
            catalog,
            catalog.text("recording_settings"),
            vec![
                FormField::choice(
                    "format",
                    catalog.text("recording_format"),
                    vec![
                        catalog.text("recording_format_flac").to_owned(),
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
                    catalog.text("recording_sample_rate"),
                    RECORDING_RATES
                        .iter()
                        .map(|rate| format!("{rate} Hz"))
                        .collect(),
                    RECORDING_RATES
                        .iter()
                        .position(|rate| *rate == self.recording.sample_rate)
                        .unwrap_or(7),
                ),
                FormField::choice(
                    "channels",
                    catalog.text("recording_channels"),
                    vec![
                        catalog.text("recording_mono").to_owned(),
                        catalog.text("recording_stereo").to_owned(),
                    ],
                    usize::from(self.recording.channels.saturating_sub(1)),
                ),
                FormField::number(
                    "bitrate",
                    catalog.text("recording_bitrate"),
                    f64::from(self.recording.bitrate),
                    32.0,
                    512.0,
                ),
                FormField::text(
                    "folder",
                    catalog.text("recording_folder"),
                    self.recording.directory.to_string_lossy(),
                ),
                FormField::checkbox(
                    "remember",
                    catalog.text("recording_remember"),
                    self.recording.remember,
                ),
                FormField::choice(
                    "feedback",
                    catalog.text("recording_feedback"),
                    vec![
                        catalog.text("recording_feedback_sounds").to_owned(),
                        catalog.text("recording_feedback_speech").to_owned(),
                        catalog.text("recording_feedback_both").to_owned(),
                        catalog.text("recording_feedback_none").to_owned(),
                    ],
                    match self.recording.feedback {
                        RecordingFeedback::Sounds => 0,
                        RecordingFeedback::Speech => 1,
                        RecordingFeedback::Both => 2,
                        RecordingFeedback::None => 3,
                    },
                ),
            ],
        )
    }

    /// Applies and saves values of [`Self::recording_form`].
    ///
    /// # Errors
    ///
    /// Returns the message when the folder is not absolute or the settings
    /// could not be saved; the previous settings stay in effect.
    pub fn apply_recording_values(
        &mut self,
        environment: &SourcesEnvironment,
        values: &FormValues,
    ) -> Result<(), String> {
        let catalog = environment.catalog();
        let previous = self.recording.clone();
        self.recording.format = match choice(values, "format") {
            1 => RecordingFormat::Wav,
            2 => RecordingFormat::Mp3,
            3 => RecordingFormat::Aac,
            _ => RecordingFormat::Flac,
        };
        self.recording.sample_rate =
            RECORDING_RATES[choice(values, "rate").min(RECORDING_RATES.len() - 1)];
        self.recording.channels = if choice(values, "channels") == 0 {
            1
        } else {
            2
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 32 to 512.
        let bitrate = number(values, "bitrate").clamp(32.0, 512.0).round() as u32;
        self.recording.bitrate = bitrate;
        self.recording.directory = PathBuf::from(text(values, "folder").trim());
        self.recording.remember = checked(values, "remember");
        self.recording.feedback = match choice(values, "feedback") {
            1 => RecordingFeedback::Speech,
            2 => RecordingFeedback::Both,
            3 => RecordingFeedback::None,
            _ => RecordingFeedback::Sounds,
        };
        if !self.recording.directory.is_absolute() {
            self.recording = previous;
            return Err(catalog.text("recording_folder_absolute").to_owned());
        }
        self.save_recording(environment).inspect_err(|_| {
            self.recording = previous;
        })
    }

    /// Loads the saved sources, IPTV and recording files once.
    pub fn ensure_loaded(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        self.load(ui, environment);
    }

    fn recording_settings(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        let catalog = environment.catalog();
        let form = self.recording_form(&catalog);
        let Some(values) = ui.form(&form) else { return };
        if let Err(error) = self.apply_recording_values(environment, &values) {
            ui.error(&error);
        }
    }

    fn start_recording(&mut self, ui: &mut dyn SourcesUi, environment: &SourcesEnvironment) {
        let catalog = environment.catalog();
        if self.is_recording() {
            ui.status(catalog.text("recording_already_active"), true);
            return;
        }
        if self.recording.sources.is_empty() {
            self.start_after_sources = true;
            self.spawn(ui, move || {
                recording::sources()
                    .map(Work::Sources)
                    .map_err(|error| recording_error(&catalog, &error))
            });
            return;
        }
        let Some(ffmpeg) = environment.ffmpeg.clone() else {
            ui.error(catalog.text("recording_needs_ffmpeg"));
            return;
        };
        let name = format!(
            "{} {}.{}",
            catalog.text("recording_file_name"),
            chrono::Local::now().format("%Y-%m-%d %H-%M-%S"),
            self.recording.format.extension()
        );
        let output = self.recording.directory.join(name);
        match RecordingSession::start(self.recording.clone(), ffmpeg, output) {
            Ok(recorder) => {
                self.recorder = Some(recorder);
                ui.status(catalog.text("recording_preparing"), true);
            }
            Err(error) => ui.error(&recording_error(&catalog, &error)),
        }
    }

    fn stop_recording(&mut self, ui: &mut dyn SourcesUi, catalog: &TranslationCatalog) {
        match &self.recorder {
            Some(recorder) if recorder.is_active() => {
                recorder.stop();
                ui.status(catalog.text("recording_finishing"), true);
            }
            _ => ui.status(catalog.text("recording_not_active"), true),
        }
    }

    /// The worker's Paused event speaks the new state, once.
    fn pause_recording(&mut self, ui: &mut dyn SourcesUi, catalog: &TranslationCatalog) {
        match &self.recorder {
            Some(recorder) if recorder.is_active() => {
                recorder.toggle_pause();
            }
            _ => ui.status(catalog.text("recording_not_active"), true),
        }
    }

    fn open_recordings_folder(&self, ui: &mut dyn SourcesUi, catalog: &TranslationCatalog) {
        if let Err(error) = std::fs::create_dir_all(&self.recording.directory)
            .map_err(|error| error.to_string())
            .and_then(|()| {
                apricot_platform::shell::open_path(&self.recording.directory)
                    .map_err(|error| error.to_string())
            })
        {
            ui.error(
                &catalog
                    .text("recording_folder_failed")
                    .replace("{error}", &error),
            );
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RadioScreen {
    Menu,
    Search,
    Stations,
    Settings,
}

const RECORDING_RATES: [u32; 11] = [
    8000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 88200, 96000, 192_000,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceReturn {
    Radio,
    Iptv(IptvScreen),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IptvScreen {
    Sources,
    Kinds,
    Groups,
    Channels,
    Episodes,
}

const IPTV_KINDS: [ChannelKind; 4] = [
    ChannelKind::Live,
    ChannelKind::Movie,
    ChannelKind::Series,
    ChannelKind::Episode,
];

const fn iptv_kind_key(kind: ChannelKind) -> &'static str {
    match kind {
        ChannelKind::Live => "iptv_live",
        ChannelKind::Movie => "iptv_movies",
        ChannelKind::Series => "iptv_series",
        ChannelKind::Episode => "iptv_episodes",
    }
}

/// A programme time in the computer's time zone.
fn clock(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0).map_or_else(String::new, |time| {
        time.with_timezone(&chrono::Local)
            .format("%H:%M")
            .to_string()
    })
}

/// A kept channel opened again stays the same occurrence of its playlist,
/// so the next entry follows it (review 5, A5).
fn with_kept_entry(mut item: MediaItem, kept: Option<&MediaItem>) -> MediaItem {
    if let Some(kept) = kept {
        for (key, value) in &kept.metadata {
            if key.starts_with("apricot_playlist_") || key == "added_at" {
                item.metadata.insert(key.clone(), value.clone());
            }
        }
    }
    item
}

fn iptv_error(catalog: &TranslationCatalog, error: IptvError) -> String {
    let key = match error {
        IptvError::CredentialsSave => "iptv_error_credentials_save",
        IptvError::CredentialsLocked => "iptv_error_credentials_locked",
        IptvError::PlaylistUnreadable => "iptv_error_playlist_unreadable",
        IptvError::PlaylistTooLarge | IptvError::TooManyChannels => "iptv_error_playlist_large",
        IptvError::HlsStream => "iptv_error_hls",
        IptvError::NoChannels => "iptv_error_no_channels",
        IptvError::GuideUnreadable => "iptv_error_guide_unreadable",
        IptvError::GuideTooLarge => "iptv_error_guide_large",
        IptvError::GuideInvalid => "iptv_error_guide_invalid",
        IptvError::Proxy => "radio_error_proxy",
        IptvError::Connect => "iptv_error_connect",
        IptvError::Http(code) => {
            return catalog
                .text("iptv_error_http")
                .replace("{code}", &code.to_string());
        }
        IptvError::ResponseTooLarge => "iptv_error_response_large",
        IptvError::InvalidData => "iptv_error_data",
        IptvError::NoSeries => "iptv_no_episodes",
        IptvError::ChooseEpisode => "iptv_error_choose_episode",
        IptvError::InvalidMac => "iptv_error_mac",
        IptvError::PortalRefused => "iptv_error_portal",
        IptvError::NoPlaybackLink => "iptv_error_no_link",
        IptvError::NeedHttpAddress => "iptv_error_address",
        IptvError::ChannelGone => "iptv_error_channel_gone",
    };
    catalog.text(key).to_owned()
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A recording source as the lists name it.
fn source_label(catalog: &TranslationCatalog, source: &CaptureSource) -> String {
    let (key, name) = match &source.kind {
        CaptureKind::Input { .. } => ("recording_input", source.name.as_str()),
        CaptureKind::Output { .. } if source.name.is_empty() => {
            return catalog.text("recording_computer_audio").to_owned();
        }
        CaptureKind::Output { .. } => ("recording_output", source.name.as_str()),
        CaptureKind::Application { exclude: true, .. } => {
            ("recording_app_except", source.name.as_str())
        }
        CaptureKind::Application { .. } => ("recording_app", source.name.as_str()),
    };
    catalog.text(key).replace("{name}", name)
}

#[cfg(test)]
mod filter_tests {
    /// Review 4, B15: a filter saved for all lists shows in this list's
    /// form where the list has none of its own; its own value shows over it.
    #[test]
    fn filters_for_all_lists_show_in_this_lists_form() {
        let global = super::RadioFilters {
            country: "SI".into(),
            ..super::RadioFilters::default()
        };
        let shown = super::shown_filters(&super::RadioFilters::default(), &global);
        assert_eq!(shown.country, "SI");
        let local = super::RadioFilters {
            country: "DE".into(),
            ..super::RadioFilters::default()
        };
        assert_eq!(super::shown_filters(&local, &global).country, "DE");
    }

    /// Review 2.1: saving one changed field for all lists keeps the others.
    #[test]
    fn only_changed_filter_fields_reach_the_scope() {
        let shown = super::RadioFilters::default();
        let mut typed = shown.clone();
        typed.codec = "MP3".into();
        let global = super::RadioFilters {
            country: "SI".into(),
            ..super::RadioFilters::default()
        };
        let saved = super::changed_filters(&shown, &typed, global);
        assert_eq!(saved.country, "SI");
        assert_eq!(saved.codec, "MP3");
    }
}

/// This list's own filters, with the filters for all lists in the fields
/// it leaves empty.
fn shown_filters(local: &RadioFilters, global: &RadioFilters) -> RadioFilters {
    let empty = RadioFilters::default();
    let mut shown = local.clone();
    macro_rules! fill {
        ($($field:ident),*) => {
            $(if local.$field == empty.$field {
                shown.$field.clone_from(&global.$field);
            })*
        };
    }
    fill!(
        country,
        region,
        language,
        tags,
        codec,
        minimum_bitrate,
        maximum_bitrate,
        only_working,
        https_only
    );
    shown
}

/// `target` with the fields of `typed` that differ from what the form
/// showed (`shown`).
fn changed_filters(
    shown: &RadioFilters,
    typed: &RadioFilters,
    mut target: RadioFilters,
) -> RadioFilters {
    macro_rules! take {
        ($($field:ident),*) => {
            $(if typed.$field != shown.$field {
                target.$field.clone_from(&typed.$field);
            })*
        };
    }
    take!(
        country,
        region,
        language,
        tags,
        codec,
        minimum_bitrate,
        maximum_bitrate,
        only_working,
        https_only
    );
    target
}

fn recording_error(catalog: &TranslationCatalog, error: &RecordingError) -> String {
    let key = match error {
        RecordingError::NoSources => "recording_error_no_sources",
        RecordingError::TooManySources => "recording_error_too_many",
        RecordingError::InvalidFormat => "recording_error_format",
        RecordingError::InvalidLevel => "recording_error_level",
        RecordingError::DestinationExists | RecordingError::UnfinishedExists => {
            "recording_error_exists"
        }
        RecordingError::CreateFolder => "recording_error_folder",
        RecordingError::EncoderStart => "recording_error_encoder",
        RecordingError::NoneOpened => "recording_error_none_opened",
        RecordingError::NotCompleted(details) => {
            return catalog
                .text("recording_error_not_completed")
                .replace("{error}", details)
                .trim()
                .to_owned();
        }
        RecordingError::Incomplete(kept) => {
            return catalog
                .text("recording_error_incomplete")
                .replace("{file}", &kept.display().to_string());
        }
        RecordingError::SaveFailed(kept) => {
            return catalog
                .text("recording_error_save_kept")
                .replace("{file}", &kept.display().to_string());
        }
        RecordingError::Discovery(details) => {
            return catalog
                .text("recording_error_discovery")
                .replace("{error}", details)
                .trim()
                .to_owned();
        }
        RecordingError::Unsupported => "recording_error_unsupported",
    };
    catalog.text(key).to_owned()
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

    /// Review 5, A5: a kept channel of an edited playlist, opened again
    /// from its source, is the same playlist occurrence as the kept entry.
    #[test]
    fn a_reopened_playlist_channel_keeps_its_entry() {
        let source = IptvSource {
            id: "s".into(),
            name: "Home".into(),
            kind: apricot_platform::iptv::SourceKind::Xtream,
            address: "https://tv.example.test/player_api.php".into(),
            protected_credentials: String::new(),
            epg_address: String::new(),
            protected_epg: String::new(),
            prefer_hls: false,
        };
        let channel = IptvChannel {
            id: "7".into(),
            name: "News".into(),
            group: String::new(),
            number: String::new(),
            epg_id: String::new(),
            logo: String::new(),
            url: "http://tv.example.test/live/USER/FAKE/7.ts".into(),
            kind: ChannelKind::Live,
            metadata: std::collections::BTreeMap::new(),
        };
        let playing = channel.media(&source).expect("item");
        let mut kept = playing.without_private_address().expect("kept");
        kept.metadata
            .insert("apricot_playlist_entry_id".into(), "e1".into());
        let opened = with_kept_entry(playing, Some(&kept));
        assert_eq!(
            apricot_app::playback_sequence::playlist_entry_identity(&opened),
            apricot_app::playback_sequence::playlist_entry_identity(&kept)
        );
    }

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
            controller.stream_metadata(&mut ui, Some("station"), Some(title), url, None);
        }
        assert_eq!(ui.0, ["Artist - Song", "Artist - Next"]);
        controller.radio.announce_metadata = false;
        controller.stream_metadata(&mut ui, Some("station"), Some("Quiet - Song"), url, None);
        assert_eq!(ui.0.len(), 2);
        assert_eq!(controller.metadata_title, "Quiet - Song");
    }

    #[derive(Default)]
    struct Browsing {
        answers: std::collections::VecDeque<Option<(usize, &'static str)>>,
        titles: Vec<String>,
        rows: Vec<Vec<String>>,
    }

    impl SourcesUi for Browsing {
        fn browse(&mut self, browser: &SourceBrowser) -> Option<SourceSelection> {
            self.titles.push(browser.list_name.clone());
            self.rows.push(browser.rows.clone());
            self.answers
                .pop_front()
                .flatten()
                .map(|(index, action)| SourceSelection {
                    index,
                    action: action.to_owned(),
                })
        }
        fn form(&mut self, _form: &FeatureForm) -> Option<FormValues> {
            None
        }
        fn status(&mut self, _message: &str, _announce: bool) {}
        fn error(&mut self, _message: &str) {}
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
    fn leaving_the_player_returns_to_the_list_it_was_played_from() {
        let environment = SourcesEnvironment {
            directory: PathBuf::from("unused"),
            download_directory: PathBuf::from("unused"),
            ffmpeg: None,
            proxy: String::new(),
            shortcuts: BTreeMap::new(),
            language: "en".into(),
        };
        let mut controller = SourcesController {
            station_rows: vec![
                Station {
                    name: "First".into(),
                    stationuuid: "first".into(),
                    ..Station::default()
                },
                Station {
                    name: "Second".into(),
                    stationuuid: "second".into(),
                    ..Station::default()
                },
            ],
            station_selected: 1,
            player_return: Some((SourceReturn::Radio, "second".into())),
            ..SourcesController::default()
        };
        let mut ui = Browsing::default();
        // Back from the stations, then from the radio menu.
        ui.answers.extend([None, None]);
        assert!(controller.resume_after_player(&mut ui, &environment, Some("second"), None));
        let catalog = apricot_app::embedded_catalog("en");
        assert_eq!(
            ui.titles,
            [catalog.text("radio_stations"), catalog.text("radio_title")]
        );
        assert_eq!(controller.station_selected, 1);
        // The return is used once.
        assert!(!controller.resume_after_player(&mut ui, &environment, Some("second"), None));

        // The player went on to something else: nothing opens.
        controller.player_return = Some((SourceReturn::Radio, "second".into()));
        let mut ui = Browsing::default();
        assert!(!controller.resume_after_player(&mut ui, &environment, None, None));
        assert!(ui.titles.is_empty());
        assert!(controller.player_return.is_none());

        controller.player_return = Some((SourceReturn::Radio, "second".into()));
        controller.forget_player_return();
        assert!(!controller.resume_after_player(&mut ui, &environment, Some("second"), None));
    }

    /// Review 5, B16: a kept channel of one source gets no programme from
    /// the guide of another source read last, even with the same guide id;
    /// a channel of the browsed source still does.
    #[test]
    fn a_kept_channel_takes_no_programme_from_another_source() {
        let environment = SourcesEnvironment {
            directory: PathBuf::from("unused"),
            download_directory: PathBuf::from("unused"),
            ffmpeg: None,
            proxy: String::new(),
            shortcuts: BTreeMap::new(),
            language: "en".into(),
        };
        let source = |id: &str| IptvSource {
            id: id.into(),
            name: id.into(),
            kind: SourceKind::Playlist,
            address: "https://example.test/list.m3u".into(),
            protected_credentials: String::new(),
            epg_address: String::new(),
            protected_epg: String::new(),
            prefer_hls: false,
        };
        let now = chrono::Utc::now().timestamp();
        let mut guide = EpgGuide::default();
        guide.programmes.insert(
            "x".into(),
            vec![apricot_platform::iptv::Programme {
                channel: "x".into(),
                title: "News at nine".into(),
                description: String::new(),
                start: now - 60,
                end: now + 600,
            }],
        );
        for (playing, expected) in [("two", false), ("one", true)] {
            let mut controller = SourcesController {
                iptv_active: Some(source("one")),
                guide: guide.clone(),
                ..SourcesController::default()
            };
            let mut news = channel("News", "", ChannelKind::Live);
            news.epg_id = "x".into();
            let kept = news.media(&source(playing)).expect("item");
            let (sender, receiver) = std::sync::mpsc::channel();
            sender
                .send(Ok(Work::PlayIptv {
                    source: source(playing),
                    channel: news,
                    saved: Some(Box::new(kept)),
                }))
                .unwrap();
            controller.pending = Some(receiver);
            controller.poll(&mut Spoken::default(), &environment);
            assert_eq!(
                controller.pending_programme.is_some(),
                expected,
                "{playing}"
            );
        }
    }

    fn channel(name: &str, group: &str, kind: ChannelKind) -> IptvChannel {
        IptvChannel {
            id: name.into(),
            name: name.into(),
            group: group.into(),
            number: String::new(),
            epg_id: String::new(),
            logo: String::new(),
            url: format!("https://example.test/{name}"),
            kind,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn iptv_back_walks_up_through_the_screens_that_had_a_choice() {
        let environment = SourcesEnvironment {
            directory: PathBuf::from("unused"),
            download_directory: PathBuf::from("unused"),
            ffmpeg: None,
            proxy: String::new(),
            shortcuts: BTreeMap::new(),
            language: "en".into(),
        };
        let mut controller = SourcesController::default();
        controller.iptv_sources.push(IptvSource {
            id: "one".into(),
            name: "Home".into(),
            kind: SourceKind::Playlist,
            address: "https://example.test/list.m3u".into(),
            protected_credentials: String::new(),
            epg_address: String::new(),
            protected_epg: String::new(),
            prefer_hls: false,
        });
        controller.iptv_active = controller.iptv_sources.first().cloned();
        controller.channels = vec![
            channel("News One", "News", ChannelKind::Live),
            channel("Sport One", "Sport", ChannelKind::Live),
            channel("Film", "Drama", ChannelKind::Movie),
        ];
        let mut ui = Browsing::default();
        ui.answers
            .extend([Some((0, "open")), Some((2, "open")), None, None, None, None]);

        controller.iptv_navigate(&mut ui, &environment, IptvScreen::Kinds);

        assert_eq!(
            ui.titles,
            [
                "Home",
                "Live TV",
                "Sport",
                "Live TV",
                "Home",
                "IPTV sources"
            ]
        );
        assert_eq!(ui.rows[0], ["Live TV, 2", "Movies, 1"]);
        assert_eq!(ui.rows[1], ["All categories, 2", "News, 1", "Sport, 1"]);
        assert_eq!(ui.rows[2], ["Sport One"]);
        assert_eq!(ui.rows[5], ["Home"]);
    }

    #[test]
    fn iptv_errors_are_worded_for_people() {
        let catalog = apricot_app::embedded_catalog("en");
        assert_eq!(
            iptv_error(&catalog, IptvError::Http(401)),
            "The IPTV provider answered with an error (HTTP 401)."
        );
        assert!(iptv_error(&catalog, IptvError::HlsStream).contains("Play link"));
    }

    #[test]
    fn station_actions_reuse_the_familiar_shortcuts() {
        assert_eq!(shortcut_action("radio_favorite"), "add_favorite");
        assert_eq!(shortcut_action("radio_copy_url"), "copy_link");
        assert_eq!(shortcut_action("radio_vote"), "radio_vote");
    }
}
