//! The transcript window (Windows `show_player_transcript` with
//! `transcript_win32.rs`): a search field, the cue list, Play, Copy line,
//! Copy transcript, Copy timestamp link and Back. The transcript loads on a
//! worker (or comes from the session cache); typing filters the cues.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use apricot_app::transcript::{CachedTranscript, TranscriptView};
use apricot_core::MediaItem;

use crate::transcript_loader::{LoadedTranscript, TranscriptFailure};
use crate::{Shell, ShellUi, format_duration};

/// The label keys of the window (Windows `show_player_transcript`).
const LABEL_KEYS: [&str; 18] = [
    "transcript",
    "transcript_search",
    "transcript_loading",
    "play",
    "copy_transcript_line",
    "copy_transcript",
    "copy_timestamp_link",
    "back",
    "transcript_no_search_results",
    "no_transcript_available",
    "transcript_failed",
    "transcript_rate_limited",
    "transcript_copied",
    "transcript_line_copied",
    "transcript_loaded_from_source",
    "transcript_source_local",
    "transcript_source_subtitles",
    "transcript_source_auto_captions",
];

/// A button of the window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscriptButton {
    Play,
    CopyLine,
    CopyAll,
    CopyTimestamp,
}

/// What the list shows and which buttons are enabled
/// (Windows `refresh`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranscriptRows {
    pub rows: Vec<String>,
    /// Play and Copy line.
    pub line_buttons: bool,
    /// Copy transcript.
    pub copy_all: bool,
    /// Copy timestamp link.
    pub copy_timestamp: bool,
}

/// The transcript window's state.
pub struct TranscriptDialog {
    labels: BTreeMap<String, String>,
    can_copy_timestamp: bool,
    receiver: Option<Receiver<Result<LoadedTranscript, TranscriptFailure>>>,
    view: TranscriptView,
    loaded: Option<LoadedTranscript>,
    query: String,
    /// Shown instead of rows while loading or after a failure.
    placeholder: Option<String>,
    item: MediaItem,
    generation: u64,
}

impl TranscriptDialog {
    /// A label of the window (title, search field and buttons).
    #[must_use]
    pub fn label(&self, key: &str) -> String {
        self.labels
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_owned())
    }

    /// Windows `refresh`.
    #[must_use]
    pub fn rows(&self) -> TranscriptRows {
        let rows: Vec<String> = self.view.visible_labels().map(str::to_owned).collect();
        let has_visible = !rows.is_empty();
        let rows = if has_visible {
            rows
        } else {
            vec![self.placeholder.clone().unwrap_or_else(|| {
                self.label(if self.view.has_entries() {
                    "transcript_no_search_results"
                } else {
                    "no_transcript_available"
                })
            })]
        };
        TranscriptRows {
            rows,
            line_buttons: has_visible,
            copy_all: self.view.has_entries(),
            copy_timestamp: has_visible && self.can_copy_timestamp,
        }
    }

    /// Windows `EN_CHANGE` of the search field.
    pub fn set_query(&mut self, query: &str) {
        query.clone_into(&mut self.query);
        self.view.filter(query);
    }

    /// Windows `WM_TIMER`: `Some(announcement)` once the worker answered.
    pub fn poll(&mut self) -> Option<String> {
        let receiver = self.receiver.as_ref()?;
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(TranscriptFailure::Failed(String::new())),
        };
        self.receiver = None;
        self.placeholder = None;
        match result {
            Ok(loaded) => {
                let ready = if loaded.entries.is_empty() {
                    self.label("no_transcript_available")
                } else {
                    self.label("transcript_loaded_from_source")
                        .replace("{count}", &loaded.entries.len().to_string())
                        .replace("{source}", &self.label(loaded.source_key))
                };
                self.view = TranscriptView::new(loaded.entries.clone());
                self.view.filter(&self.query);
                self.loaded = Some(loaded);
                Some(ready)
            }
            Err(failure) => {
                // Python caches a rate-limited video as checked, so reopening
                // shows no transcript instead of retrying.
                if failure == TranscriptFailure::RateLimited {
                    self.loaded = Some(LoadedTranscript {
                        entries: Vec::new(),
                        source_key: "",
                    });
                }
                let error = match failure {
                    TranscriptFailure::RateLimited => self.label("transcript_rate_limited"),
                    TranscriptFailure::Failed(message) => message,
                };
                let message = self.label("transcript_failed").replace("{error}", &error);
                self.placeholder = Some(message.clone());
                Some(message)
            }
        }
    }

    /// Whether the worker has not answered yet.
    #[must_use]
    pub const fn is_loading(&self) -> bool {
        self.receiver.is_some()
    }
}

impl Shell {
    /// Windows `show_player_transcript`.
    pub(crate) fn show_player_transcript(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        if !self.ensure_player_for_auxiliary_view(ui) {
            return;
        }
        let Some(item) = self.application.player_session().current_item().cloned() else {
            return;
        };
        let generation = self.application.player_session().generation();
        let catalog = self.catalog();
        let labels = LABEL_KEYS
            .into_iter()
            .map(|key| (key.to_owned(), catalog.text(key).to_owned()))
            .collect();
        let settings = self.application.settings();
        let languages = settings.subtitle_languages.clone();
        let user_agent = settings.cookie_user_agent.clone();
        let config = self.youtube_session_config();
        let cached = self.application.player_session().transcript().cloned();
        let (sender, receiver) = mpsc::channel();
        if let Some(cached) = cached {
            let source_key = match cached.source_key.as_str() {
                "transcript_source_local" => "transcript_source_local",
                "transcript_source_auto_captions" => "transcript_source_auto_captions",
                _ => "transcript_source_subtitles",
            };
            let _ = sender.send(Ok(LoadedTranscript {
                entries: cached.entries,
                source_key,
            }));
        } else {
            let worker_item = item.clone();
            let executable = crate::youtube::components_directory()
                .map(|path| apricot_platform::app_update::preferred_ytdlp_executable(&path));
            std::thread::spawn(move || {
                let result = crate::transcript_loader::load(
                    &worker_item,
                    executable.as_deref().unwrap_or(std::path::Path::new("")),
                    config,
                    &languages,
                    &user_agent,
                );
                let _ = sender.send(result);
            });
        }
        let can_copy_timestamp = item.youtube_url_at_timestamp(0.0).is_some();
        let loading = catalog.text("transcript_loading").to_owned();
        ui.open_transcript(TranscriptDialog {
            labels,
            can_copy_timestamp,
            receiver: Some(receiver),
            view: TranscriptView::default(),
            loaded: None,
            query: String::new(),
            placeholder: Some(loading),
            item,
            generation,
        });
    }

    /// A button, Enter or a double click in the transcript window: what to
    /// announce (Windows `command` and the window's `action`).
    pub fn transcript_button(
        &mut self,
        ui: &mut dyn ShellUi,
        dialog: &TranscriptDialog,
        button: TranscriptButton,
        row: Option<usize>,
    ) -> Option<String> {
        let selected = row.and_then(|row| dialog.view.selected(row));
        if selected.is_none() && button != TranscriptButton::CopyAll {
            return Some(dialog.label("no_transcript_available"));
        }
        if matches!(
            button,
            TranscriptButton::Play | TranscriptButton::CopyTimestamp
        ) && self.application.player_session().generation() != dialog.generation
        {
            return Some(self.text("no_player"));
        }
        match button {
            TranscriptButton::Play => {
                let (_, entry, _) = selected?;
                if self.seek_player_chapter(ui, entry.start) {
                    let text = if entry.text.chars().count() > 90 {
                        format!(
                            "{}...",
                            entry.text.chars().take(87).collect::<String>().trim_end()
                        )
                    } else {
                        entry.text.clone()
                    };
                    Some(
                        self.text("transcript_selected")
                            .replace("{time}", &format_duration(entry.start))
                            .replace("{text}", &text),
                    )
                } else {
                    Some(self.text("timing_unavailable"))
                }
            }
            TranscriptButton::CopyTimestamp => {
                let (_, entry, _) = selected?;
                Some(match dialog.item.youtube_url_at_timestamp(entry.start) {
                    Some(url) if ui.copy_text(url.as_str()) => self.text("timestamp_url_copied"),
                    Some(_) => self.text("copy_failed"),
                    None => self.text("timestamp_url_unavailable"),
                })
            }
            TranscriptButton::CopyLine | TranscriptButton::CopyAll => {
                let text = if button == TranscriptButton::CopyAll {
                    dialog.view.full_text()
                } else {
                    selected
                        .map(|(_, _, text)| text.to_owned())
                        .unwrap_or_default()
                };
                if text.is_empty() {
                    return Some(dialog.label("no_transcript_available"));
                }
                Some(if ui.copy_text(&text) {
                    dialog.label(if button == TranscriptButton::CopyAll {
                        "transcript_copied"
                    } else {
                        "transcript_line_copied"
                    })
                } else {
                    self.text("copy_failed")
                })
            }
        }
    }

    /// The transcript window closed: a loaded transcript is cached for the
    /// session and focus goes to the player.
    pub fn transcript_closed(&mut self, ui: &mut dyn ShellUi, dialog: TranscriptDialog) {
        if let Some(loaded) = dialog.loaded {
            self.application.cache_transcript(
                dialog.generation,
                CachedTranscript {
                    entries: loaded.entries,
                    source_key: loaded.source_key.to_owned(),
                },
            );
        }
        ui.focus_primary_control();
    }
}
