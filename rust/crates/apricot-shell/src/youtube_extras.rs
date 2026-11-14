//! Forms and shell handling of the 2.1 `YouTube` additions: one download's own
//! options, the choice for a link with a video and a playlist, and search
//! suggestions.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use apricot_app::{
    DownloadChoice,
    youtube_extras::{AUDIO_QUALITY_VALUES, ItemDownloadOptions, audio_quality_label},
};
use apricot_core::{MediaItem, TranslationCatalog};
use apricot_storage::SettingsDocument;

use crate::{FeatureForm, FormField, FormValue, FormValues, Shell, ShellUi};

const AUDIO_FORMATS: [&str; 5] = ["mp3", "m4a", "opus", "wav", "flac"];
const VIDEO_FORMATS: [(&str, &str); 4] = [
    ("mp4", "video_format_mp4_recommended"),
    ("best-any", "video_format_best_available"),
    ("mp4-single", "video_format_mp4_single"),
    ("smallest", "video_format_smallest"),
];
const HEIGHTS: [i64; 7] = [0, 360, 480, 720, 1080, 1440, 2160];

fn position<T: PartialEq>(values: &[T], value: &T) -> usize {
    values.iter().position(|item| item == value).unwrap_or(0)
}

/// Download with options: the formats and folder for this one download.
#[must_use]
pub fn download_options_form(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    choice: DownloadChoice,
) -> FeatureForm {
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("download_with_options"),
        vec![
            FormField::choice(
                "kind",
                catalog.text("download_options_kind"),
                vec![
                    catalog.text("download_audio").to_owned(),
                    catalog.text("download_video").to_owned(),
                ],
                usize::from(choice == DownloadChoice::Video),
            ),
            FormField::choice(
                "audio_format",
                catalog.text("audio_format"),
                AUDIO_FORMATS
                    .iter()
                    .map(|format| format.to_uppercase())
                    .collect(),
                position(&AUDIO_FORMATS, &settings.audio_format.as_str()),
            ),
            FormField::choice(
                "audio_quality",
                catalog.text("audio_quality"),
                AUDIO_QUALITY_VALUES
                    .iter()
                    .map(|value| audio_quality_label(value))
                    .collect(),
                position(&AUDIO_QUALITY_VALUES, &settings.audio_quality.as_str()),
            ),
            FormField::choice(
                "video_format",
                catalog.text("video_format"),
                VIDEO_FORMATS
                    .iter()
                    .map(|(_, key)| catalog.text(key).to_owned())
                    .collect(),
                VIDEO_FORMATS
                    .iter()
                    .position(|(value, _)| *value == settings.video_format)
                    .unwrap_or(0),
            ),
            FormField::choice(
                "height",
                catalog.text("max_height"),
                HEIGHTS
                    .iter()
                    .map(|height| {
                        if *height == 0 {
                            catalog.text("download_no_height_limit").to_owned()
                        } else {
                            format!("{height}p")
                        }
                    })
                    .collect(),
                position(&HEIGHTS, &settings.max_video_height),
            ),
            FormField::text(
                "folder",
                catalog.text("download_options_folder"),
                settings.download_folder.trim(),
            ),
        ],
    );
    catalog.text("download_start").clone_into(&mut form.submit);
    form
}

/// The download kind and options chosen in [`download_options_form`].
/// Only what differs from the settings is kept, so later changes in
/// Settings still apply to the rest.
#[must_use]
pub fn download_options_from_values(
    values: &FormValues,
    settings: &SettingsDocument,
) -> (DownloadChoice, ItemDownloadOptions) {
    let choice = |id: &str| values.get(id).and_then(FormValue::choice).unwrap_or(0);
    let kind = if choice("kind") == 1 {
        DownloadChoice::Video
    } else {
        DownloadChoice::Audio
    };
    let differ = |chosen: &str, current: &str| {
        if chosen == current {
            String::new()
        } else {
            chosen.to_owned()
        }
    };
    let audio_format = AUDIO_FORMATS[choice("audio_format").min(AUDIO_FORMATS.len() - 1)];
    let audio_quality =
        AUDIO_QUALITY_VALUES[choice("audio_quality").min(AUDIO_QUALITY_VALUES.len() - 1)];
    let video_format = VIDEO_FORMATS[choice("video_format").min(VIDEO_FORMATS.len() - 1)].0;
    let height = HEIGHTS[choice("height").min(HEIGHTS.len() - 1)];
    let folder = values
        .get("folder")
        .and_then(FormValue::text)
        .unwrap_or_default()
        .trim();
    let options = ItemDownloadOptions {
        audio_format: differ(audio_format, &settings.audio_format),
        audio_quality: differ(audio_quality, &settings.audio_quality),
        video_format: differ(video_format, &settings.video_format),
        max_video_height: (height != settings.max_video_height).then_some(height),
        folder: differ(folder, settings.download_folder.trim()),
    };
    (kind, options)
}

/// The question for a link that names a video in a playlist.
#[must_use]
pub fn link_choice_form(catalog: &TranslationCatalog) -> FeatureForm {
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("link_choice_title"),
        vec![FormField::choice(
            "open",
            catalog.text("link_choice_open"),
            vec![
                catalog.text("link_choice_video").to_owned(),
                catalog.text("link_choice_playlist").to_owned(),
            ],
            0,
        )],
    );
    catalog.text("ok").clone_into(&mut form.submit);
    form
}

/// `true` when the link choice picked the whole playlist.
#[must_use]
pub fn wants_playlist(values: &FormValues) -> bool {
    values.get("open").and_then(FormValue::choice) == Some(1)
}

/// The suggestions to pick from.
#[must_use]
pub fn suggestions_form(catalog: &TranslationCatalog, suggestions: &[String]) -> FeatureForm {
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("search_suggestions"),
        vec![FormField::choice(
            "suggestion",
            catalog.text("search_suggestions"),
            suggestions.to_vec(),
            0,
        )],
    );
    catalog.text("search").clone_into(&mut form.submit);
    form
}

/// The suggestion chosen in [`suggestions_form`].
#[must_use]
pub fn chosen_suggestion(values: &FormValues, suggestions: &[String]) -> Option<String> {
    values
        .get("suggestion")
        .and_then(FormValue::choice)
        .and_then(|index| suggestions.get(index))
        .cloned()
}

/// A suggestion request that runs off the UI thread.
pub struct PendingSuggestions {
    receiver: Receiver<Result<String, String>>,
}

impl PendingSuggestions {
    #[must_use]
    pub fn start(query: &str, language: &str, proxy: &str) -> Self {
        let (sender, receiver) = mpsc::channel();
        let (query, language, proxy) = (query.to_owned(), language.to_owned(), proxy.to_owned());
        std::thread::spawn(move || {
            let _ = sender.send(apricot_platform::search_suggestions::youtube_suggestions(
                &query, &language, &proxy,
            ));
        });
        Self { receiver }
    }

    /// `None` while it runs; then the suggestions, empty when none came.
    pub fn poll(&self) -> Option<Result<Vec<String>, String>> {
        match self.receiver.try_recv() {
            Ok(result) => {
                Some(result.map(|body| apricot_app::youtube_extras::parse_suggestions(&body)))
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(String::new())),
        }
    }
}

impl Shell {
    /// Download with options for the selected or playing item.
    pub(crate) fn download_with_options(&mut self, ui: &mut dyn ShellUi) {
        let Some(mut item) = self.active_media_item(ui) else {
            ui.set_status(&self.text("no_selection"), true);
            return;
        };
        let catalog = self.catalog();
        let settings = self.application.settings().clone();
        let Some(values) = ui.feature_form(&download_options_form(
            &catalog,
            &settings,
            DownloadChoice::Audio,
        )) else {
            return;
        };
        let (choice, options) = download_options_from_values(&values, &settings);
        options.store(&mut item);
        self.start_download_item(ui, &item, choice, false);
    }

    /// The link field's link, or its video or playlist when it names both;
    /// `None` when the question was cancelled.
    pub(crate) fn choose_link_item(
        &self,
        ui: &mut dyn ShellUi,
        link: &str,
        item: MediaItem,
    ) -> Option<MediaItem> {
        let Some((video, playlist)) = apricot_app::youtube_extras::video_and_playlist(link) else {
            return Some(item);
        };
        let values = ui.feature_form(&link_choice_form(&self.catalog()))?;
        Some(if wants_playlist(&values) {
            playlist
        } else {
            video
        })
    }

    /// Search suggestions for the typed query.
    pub(crate) fn start_search_suggestions(&mut self, ui: &mut dyn ShellUi) {
        let query = ui.screen_text("search_query");
        if query.trim().is_empty() {
            ui.set_status(&self.text("enter_query"), true);
            return;
        }
        let settings = self.application.settings();
        self.pending_suggestions = Some(PendingSuggestions::start(
            &query,
            &settings.language,
            &settings.proxy,
        ));
        ui.set_status(&self.text("search_suggestions_loading"), true);
        ui.set_background_polling(true);
    }

    /// `true` while suggestions are on their way.
    pub(crate) fn poll_search_suggestions(&mut self, ui: &mut dyn ShellUi) -> bool {
        let Some(result) = self
            .pending_suggestions
            .as_ref()
            .and_then(PendingSuggestions::poll)
        else {
            return self.pending_suggestions.is_some();
        };
        self.pending_suggestions = None;
        let catalog = self.catalog();
        let suggestions = match result {
            Ok(suggestions) if !suggestions.is_empty() => suggestions,
            _ => {
                ui.set_status(catalog.text("search_suggestions_none"), true);
                return false;
            }
        };
        let Some(values) = ui.feature_form(&suggestions_form(&catalog, &suggestions)) else {
            return false;
        };
        if let Some(query) = chosen_suggestion(&values, &suggestions) {
            ui.set_screen_text("search_query", &query);
            self.submit_search(ui);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_options_keep_only_what_differs_from_settings() {
        let catalog = apricot_app::embedded_catalog("en");
        let settings = SettingsDocument::default();
        let form = download_options_form(&catalog, &settings, DownloadChoice::Video);
        let mut values: FormValues = form
            .fields
            .iter()
            .filter_map(|field| match &field.kind {
                crate::FormFieldKind::Choice { selected, .. } => {
                    Some((field.id.clone(), FormValue::Choice(*selected)))
                }
                crate::FormFieldKind::Text { value, .. } => {
                    Some((field.id.clone(), FormValue::Text(value.clone())))
                }
                _ => None,
            })
            .collect();
        let (choice, options) = download_options_from_values(&values, &settings);
        assert_eq!(choice, DownloadChoice::Video);
        assert_eq!(options, ItemDownloadOptions::default());
        values.insert("audio_format".into(), FormValue::Choice(4));
        values.insert("height".into(), FormValue::Choice(3));
        let (_, options) = download_options_from_values(&values, &settings);
        assert_eq!(options.audio_format, "flac");
        assert_eq!(options.max_video_height, Some(720));
    }
}
