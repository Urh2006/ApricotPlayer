//! The file and folder converter (Windows `converter_win32.rs`,
//! `show_converter`, `start_conversion`, `poll_conversions` and
//! `handle_conversion_message`) and the clip export of the player (Windows
//! `start_clip_export` and `poll_clip_exports`).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use apricot_app::DownloadChoice;
use apricot_app::converter::{
    self, FileConversionJob, FolderConversionEvent, FolderConversionJob, FolderConversionOutcome,
    converted_folder_path, default_output_path, dialog_format_values, folder_done_message,
    format_label, input_kind, path_from_text, progress_message, replaced_output_path,
    run_file_conversion, run_folder_conversion, shows_audio_video_options, with_default_extension,
};
use apricot_core::{MediaItem, MediaKind, TranslationCatalog};

use crate::player_tools::application_directory;
use crate::{Shell, ShellUi, format_duration};

/// What the converter hands to [`Shell::converter_closed`] (Windows
/// `ConverterStart`).
#[derive(Clone, Debug, PartialEq)]
pub enum ConverterStart {
    File {
        source: PathBuf,
        output: PathBuf,
        target: String,
        image: Option<PathBuf>,
        replace_original: bool,
        /// The save dialog confirmed overwriting an existing `output`.
        overwrite_confirmed: bool,
    },
    Folder {
        source: PathBuf,
        output_folder: PathBuf,
        target: String,
        image: Option<PathBuf>,
        replace_originals: bool,
    },
}

/// The dialogs the converter opens over itself (Windows `warn`,
/// `choose_download_folder` and `save_file_with_filters`).
pub trait ConverterView {
    /// A warning; focus returns to the control that had it.
    fn warn(&mut self, text: &str);
    /// A folder dialog starting in `initial`.
    fn choose_folder(&mut self, title: &str, initial: &Path) -> Option<PathBuf>;
    /// A save dialog in `directory` starting with `name`.
    fn choose_save_file(
        &mut self,
        title: &str,
        directory: &Path,
        name: &str,
        extensions: &[String],
    ) -> Option<PathBuf>;
    /// Windows `announce`: the status line of the main window.
    fn announce(&mut self, text: &str);
}

/// The extensions of a file filter's `*.mp3;*.wav` patterns.
fn filter_extensions(patterns: &str) -> Vec<String> {
    patterns
        .split(';')
        .filter_map(|pattern| pattern.trim().strip_prefix("*."))
        .filter(|extension| *extension != "*")
        .map(str::to_owned)
        .collect()
}

/// The converter dialog's state (Windows `DialogState`).
pub struct ConverterDialog {
    catalog: TranslationCatalog,
    pub folder_mode: bool,
    targets: Vec<&'static str>,
}

/// Which audio-to-video controls show (Windows
/// `update_audio_video_controls`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConverterOptions {
    /// The options label with Add image and Dark background.
    pub audio_video: bool,
    /// The image label, field and Choose image.
    pub image: bool,
    /// Dark background becomes checked.
    pub check_dark: bool,
}

impl ConverterDialog {
    #[must_use]
    pub fn text(&self, key: &str) -> String {
        self.catalog.text(key).to_owned()
    }

    /// The window title.
    #[must_use]
    pub fn title(&self) -> String {
        self.text(if self.folder_mode {
            "folder_converter"
        } else {
            "file_converter"
        })
    }

    /// The label and accessible name of the path field.
    #[must_use]
    pub fn path_label(&self) -> String {
        self.text(if self.folder_mode {
            "folder_to_convert"
        } else {
            "file_to_convert"
        })
    }

    /// The labels of Browse, Create new and Replace.
    #[must_use]
    pub fn mode_labels(&self) -> (String, String, String) {
        if self.folder_mode {
            (
                self.text("browse_folder"),
                self.text("converter_create_new_folder"),
                self.text("converter_replace_originals"),
            )
        } else {
            (
                self.text("browse_file"),
                self.text("converter_create_new_file"),
                self.text("converter_replace_original_file"),
            )
        }
    }

    /// Windows `update_formats`: the new format labels and selection when
    /// the list changed, keeping the chosen format.
    pub fn update_formats(
        &mut self,
        path: &str,
        selected: Option<usize>,
    ) -> Option<(Vec<String>, usize)> {
        let values = dialog_format_values(self.folder_mode, path);
        if values == self.targets {
            return None;
        }
        let current = (!self.targets.is_empty()).then(|| self.target(selected));
        let selection = current
            .and_then(|current| values.iter().position(|value| *value == current))
            .unwrap_or(0);
        let labels = values.iter().map(|value| format_label(value)).collect();
        self.targets = values;
        Some((labels, selection))
    }

    /// Windows `selected_target`.
    #[must_use]
    pub fn target(&self, selected: Option<usize>) -> &'static str {
        selected
            .and_then(|index| self.targets.get(index).copied())
            .or_else(|| self.targets.first().copied())
            .unwrap_or("mp3")
    }

    /// Windows `update_audio_video_controls`.
    #[must_use]
    pub fn options(
        &self,
        path: &str,
        selected: Option<usize>,
        add_image: bool,
        dark: bool,
    ) -> ConverterOptions {
        let audio_video = shows_audio_video_options(self.folder_mode, path, self.target(selected));
        let check_dark = audio_video && !add_image && !dark;
        ConverterOptions {
            audio_video,
            image: audio_video && add_image,
            check_dark,
        }
    }

    /// The extensions the file browser accepts (the first Windows filter).
    #[must_use]
    pub fn input_extensions(&self) -> Vec<String> {
        converter::input_filters(&self.catalog)
            .first()
            .map(|(_, patterns)| filter_extensions(patterns))
            .unwrap_or_default()
    }

    /// The extensions of the image browser.
    #[must_use]
    pub fn image_extensions(&self) -> Vec<String> {
        converter::image_filters(&self.catalog)
            .first()
            .map(|(_, patterns)| filter_extensions(patterns))
            .unwrap_or_default()
    }

    /// Windows `convert`: `Some` with the conversion to start, after which
    /// the dialog closes.
    pub fn convert(
        &self,
        view: &mut dyn ConverterView,
        path: &str,
        selected: Option<usize>,
        image_text: Option<&str>,
        replace: bool,
    ) -> Option<ConverterStart> {
        let source = path_from_text(path);
        if path.trim().trim_matches('"').is_empty() || !source.exists() {
            view.warn(&self.text("no_selection"));
            return None;
        }
        let target = self.target(selected);
        let image = image_text.map(path_from_text);
        if image.as_ref().is_some_and(|image| !image.exists()) {
            view.warn(&self.text("select_image_file"));
            return None;
        }
        if self.folder_mode {
            let output_folder = if replace {
                source.clone()
            } else {
                let Some(chosen) = view.choose_folder(&self.text("choose_output_folder"), &source)
                else {
                    view.announce(&self.text("conversion_cancelled"));
                    return None;
                };
                converted_folder_path(&chosen, &source)
            };
            return Some(ConverterStart::Folder {
                source,
                output_folder,
                target: target.to_owned(),
                image,
                replace_originals: replace,
            });
        }
        if input_kind(&source).is_none() {
            view.warn(&self.text("unsupported_input_format"));
            return None;
        }
        let (output, overwrite_confirmed) = if replace {
            (replaced_output_path(&source, target), false)
        } else {
            let default_output = default_output_path(&source, target);
            let extensions = converter::target_filters(&self.catalog, target)
                .first()
                .map(|(_, patterns)| filter_extensions(patterns))
                .unwrap_or_default();
            let Some(chosen) = view.choose_save_file(
                &self.text("choose_output_file"),
                default_output.parent().unwrap_or_else(|| Path::new("")),
                &default_output
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                &extensions,
            ) else {
                view.announce(&self.text("conversion_cancelled"));
                return None;
            };
            // The dialog asked about exactly this name only when no
            // extension had to be added afterwards.
            let output = with_default_extension(chosen.clone(), target);
            let confirmed = output == chosen;
            (output, confirmed)
        };
        Some(ConverterStart::File {
            source,
            output,
            target: target.to_owned(),
            image,
            replace_original: replace,
            overwrite_confirmed,
        })
    }
}

/// A message from a conversion worker (Windows `ConversionMessage`).
enum ConversionMessage {
    Folder(FolderConversionEvent),
    FileFinished(Result<PathBuf, String>),
    FolderFinished(Result<FolderConversionOutcome, String>),
}

/// The conversions and clip exports that run (Windows
/// `pending_conversions`, `conversion_progress_window` and
/// `clip_exports`).
#[derive(Default)]
pub(crate) struct ConversionState {
    pending: Vec<Receiver<ConversionMessage>>,
    progress_open: bool,
    clip_exports: Vec<Receiver<Result<PathBuf, String>>>,
}

/// Windows `clip_video_extension`.
fn clip_video_extension(item: &MediaItem) -> String {
    let local = item
        .local_path
        .as_deref()
        .and_then(|path| Path::new(path).extension())
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    let reported = || {
        item.metadata
            .get("ext")
            .and_then(serde_json::Value::as_str)
            .map(|extension| {
                extension
                    .trim()
                    .trim_start_matches('.')
                    .to_ascii_lowercase()
            })
            .filter(|extension| {
                (1..=10).contains(&extension.len())
                    && extension
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            })
    };
    local.or_else(reported).unwrap_or_else(|| {
        if item.kind == MediaKind::PodcastEpisode {
            "m4a".to_owned()
        } else {
            "mp4".to_owned()
        }
    })
}

/// Windows `clip_http_headers`: one `-headers` block for `FFmpeg`.
fn clip_http_headers(item: &MediaItem) -> String {
    item.metadata
        .get("http_headers")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| {
            let value = value.as_str()?.trim();
            (!value.is_empty() && !name.contains(['\r', '\n']) && !value.contains(['\r', '\n']))
                .then(|| format!("{name}: {value}\r\n"))
        })
        .collect()
}

/// Windows `start_clip_export`: the `clips` folder, the first free
/// `title - start-end.ext` in it and the extension.
fn clip_output_path(
    settings: &apricot_storage::SettingsDocument,
    item: &MediaItem,
    start: f64,
    end: f64,
    choice: DownloadChoice,
) -> Result<(PathBuf, PathBuf, String), String> {
    let mut folder_item = item.clone();
    if folder_item.kind == MediaKind::PodcastEpisode {
        folder_item.kind = MediaKind::PodcastFeed;
    }
    let folder =
        crate::downloads::download_folder_for_item(settings, &folder_item, false)?.join("clips");
    let extension = if choice == DownloadChoice::Audio {
        crate::downloads::normalized_audio_format(&settings.audio_format)
    } else {
        clip_video_extension(item)
    };
    let stem = format!(
        "{} - {}-{}",
        crate::safe_path_component(&item.title),
        format_duration(start).replace(':', "-"),
        format_duration(end).replace(':', "-")
    );
    let mut output = folder.join(format!("{stem}.{extension}"));
    let mut counter = 2;
    while output.exists() {
        output = folder.join(format!("{stem} ({counter}).{extension}"));
        counter += 1;
    }
    Ok((folder, output, extension))
}

impl Shell {
    /// Windows `show_converter`: Python `show_file_converter` and
    /// `show_folder_converter`.
    pub(crate) fn show_converter(&mut self, ui: &mut dyn ShellUi, folder_mode: bool) {
        self.stop_key_repeat(ui);
        ui.open_converter(ConverterDialog {
            catalog: self.catalog(),
            folder_mode,
            targets: Vec::new(),
        });
    }

    /// The converter closed; focus is back where it was. A chosen
    /// conversion starts (Windows `start_conversion`).
    pub fn converter_closed(&mut self, ui: &mut dyn ShellUi, start: Option<ConverterStart>) {
        if let Some(start) = start {
            self.start_conversion(ui, start);
        }
    }

    /// The converter's announcements go to the status line.
    pub fn converter_announce(ui: &mut dyn ShellUi, text: &str) {
        ui.set_status(text, true);
    }

    /// Windows `start_conversion`.
    fn start_conversion(&mut self, ui: &mut dyn ShellUi, start: ConverterStart) {
        let ffmpeg = apricot_platform::ffmpeg_executable(
            &self.application.settings().ffmpeg_location,
            application_directory().as_deref(),
        );
        let (sender, receiver) = mpsc::channel();
        match start {
            ConverterStart::File {
                source,
                output,
                target,
                image,
                replace_original,
                overwrite_confirmed,
            } => {
                let job = FileConversionJob::new(
                    ffmpeg,
                    source,
                    output,
                    &target,
                    image,
                    replace_original,
                    overwrite_confirmed,
                    self.text("unsupported_input_format"),
                );
                std::thread::spawn(move || {
                    let result =
                        run_file_conversion(&job, &mut apricot_platform::run_ffmpeg_conversion);
                    let _ = sender.send(ConversionMessage::FileFinished(result));
                });
            }
            ConverterStart::Folder {
                source,
                output_folder,
                target,
                image,
                replace_originals,
            } => {
                let job = FolderConversionJob {
                    ffmpeg,
                    source_folder: source,
                    output_folder,
                    target,
                    image,
                    replace_originals,
                };
                std::thread::spawn(move || {
                    let events = sender.clone();
                    let result = run_folder_conversion(
                        &job,
                        &mut apricot_platform::run_ffmpeg_conversion,
                        &mut |event| {
                            let _ = events.send(ConversionMessage::Folder(event));
                        },
                    );
                    let _ = sender.send(ConversionMessage::FolderFinished(result));
                });
            }
        }
        ui.set_status(&self.text("conversion_started"), true);
        self.conversions.pending.push(receiver);
        ui.set_background_polling(true);
    }

    /// Windows `poll_conversions` and `poll_clip_exports`; `true` while
    /// work runs.
    pub(crate) fn poll_conversions(&mut self, ui: &mut dyn ShellUi) -> bool {
        let mut messages = Vec::new();
        self.conversions.pending.retain(|receiver| {
            loop {
                match receiver.try_recv() {
                    Ok(message) => {
                        let finished = !matches!(message, ConversionMessage::Folder(_));
                        messages.push(message);
                        if finished {
                            break false;
                        }
                    }
                    Err(TryRecvError::Empty) => break true,
                    Err(TryRecvError::Disconnected) => break false,
                }
            }
        });
        for message in messages {
            self.handle_conversion_message(ui, message);
        }
        self.poll_clip_exports(ui);
        !self.conversions.pending.is_empty() || !self.conversions.clip_exports.is_empty()
    }

    /// Windows `finish_conversion_message`.
    fn finish_conversion_message(&self, ui: &mut dyn ShellUi, text: &str) {
        if self.application.settings().popup_when_conversion_complete {
            ui.show_error(text);
        } else {
            ui.set_status(text, true);
        }
    }

    /// Windows `close_conversion_progress`.
    fn close_conversion_progress(&mut self, ui: &mut dyn ShellUi) {
        if std::mem::take(&mut self.conversions.progress_open) {
            ui.close_conversion_progress();
        }
    }

    /// Windows `handle_conversion_message`.
    fn handle_conversion_message(&mut self, ui: &mut dyn ShellUi, message: ConversionMessage) {
        let catalog = self.catalog();
        match message {
            ConversionMessage::Folder(FolderConversionEvent::Started { total }) => {
                self.close_conversion_progress(ui);
                ui.show_conversion_progress(
                    catalog.text("conversion_progress_title"),
                    &progress_message(&catalog, "", 0, total),
                    total,
                );
                self.conversions.progress_open = true;
            }
            ConversionMessage::Folder(FolderConversionEvent::FileStarted {
                index,
                total,
                name,
            }) => {
                let text = format!(
                    "{} {index}/{total}: {name}",
                    catalog.text("conversion_started")
                );
                ui.set_status(&text, false);
            }
            ConversionMessage::Folder(FolderConversionEvent::Progress {
                file,
                converted,
                total,
            }) => {
                if self.conversions.progress_open {
                    ui.update_conversion_progress(
                        converted,
                        &progress_message(&catalog, &file, converted, total),
                    );
                }
            }
            ConversionMessage::FileFinished(Ok(path)) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let text = catalog.text("conversion_done").replace("{title}", &name);
                ui.set_status(&text, false);
                self.finish_conversion_message(ui, &text);
            }
            ConversionMessage::FolderFinished(Ok(FolderConversionOutcome::NoMediaFiles)) => {
                ui.show_error(catalog.text("conversion_no_media_files"));
            }
            ConversionMessage::FolderFinished(Ok(FolderConversionOutcome::Done {
                converted,
                failed,
            })) => {
                let text = folder_done_message(&catalog, converted, failed);
                ui.set_status(&text, false);
                self.close_conversion_progress(ui);
                self.finish_conversion_message(ui, &text);
            }
            ConversionMessage::FileFinished(Err(error))
            | ConversionMessage::FolderFinished(Err(error)) => {
                self.close_conversion_progress(ui);
                let text = catalog.text("conversion_failed").replace(
                    "{error}",
                    &apricot_app::comments::friendly_error(&catalog, &error),
                );
                ui.show_error(&text);
            }
        }
    }

    /// Windows `start_clip_export`: the marked clip of the player as audio
    /// or video in the `clips` folder.
    pub(crate) fn start_clip_export(&mut self, ui: &mut dyn ShellUi, choice: DownloadChoice) {
        let session = self.application.player_session();
        let Some(item) = session.current_item().cloned() else {
            return;
        };
        let Some((start, end)) = session.clip_range() else {
            let key =
                if session.clip_start_seconds().is_some() && session.clip_end_seconds().is_some() {
                    "clip_marker_invalid"
                } else {
                    "clip_markers_missing"
                };
            ui.set_status(&self.text(key), true);
            return;
        };
        let primary = item.local_path.clone().or_else(|| {
            item.stream_url
                .as_ref()
                .or(item.url.as_ref())
                .map(ToString::to_string)
        });
        let Some(primary) = primary else {
            return;
        };
        let settings = self.application.settings().clone();
        // Windows takes the configured `FFmpeg` or the packaged one.
        let Some(ffmpeg) = apricot_platform::ffmpeg_executable(
            &settings.ffmpeg_location,
            application_directory().as_deref(),
        ) else {
            let message = self
                .text("clip_export_failed")
                .replace("{error}", "FFmpeg was not found");
            ui.set_status(&message, false);
            ui.show_error(&message);
            return;
        };
        let (folder, mut output, extension) =
            match clip_output_path(&settings, &item, start, end, choice) {
                Ok(paths) => paths,
                Err(error) => {
                    ui.show_error(&error);
                    return;
                }
            };
        if settings.ask_download_location_each_time {
            let default_name = output
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            match ui.choose_save_file(
                &self.text("choose_save_path"),
                Some(&folder),
                &default_name,
                &[extension.as_str()],
            ) {
                Some(path) => output = path,
                None => return,
            }
        }
        let request = apricot_platform::ClipExportRequest {
            ffmpeg,
            primary_input: primary,
            external_audio_input: item.external_audio_url.as_ref().map(ToString::to_string),
            start_seconds: start,
            end_seconds: end,
            output_path: output,
            mode: if choice == DownloadChoice::Audio {
                apricot_platform::ClipExportMode::Audio
            } else {
                apricot_platform::ClipExportMode::Video
            },
            audio_format: settings.audio_format,
            audio_quality: settings.audio_quality,
            headers: clip_http_headers(&item),
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result =
                apricot_platform::export_marked_clip(&request).map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.conversions.clip_exports.push(receiver);
        ui.set_status(&self.text("clip_export_started"), true);
        ui.set_background_polling(true);
    }

    /// Windows `poll_clip_exports`.
    fn poll_clip_exports(&mut self, ui: &mut dyn ShellUi) {
        let mut completed = Vec::new();
        self.conversions
            .clip_exports
            .retain(|receiver| match receiver.try_recv() {
                Ok(result) => {
                    completed.push(result);
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => {
                    completed.push(Err("Clip export worker stopped".to_owned()));
                    false
                }
            });
        for result in completed {
            match result {
                Ok(path) => {
                    let message = self.text("clip_export_done").replace(
                        "{title}",
                        &path.file_name().unwrap_or_default().to_string_lossy(),
                    );
                    ui.set_status(&message, true);
                }
                // Python `export_marked_clip_worker` shows the failure in a
                // message box.
                Err(error) => {
                    let message = self.text("clip_export_failed").replace("{error}", &error);
                    ui.set_status(&message, false);
                    ui.show_error(&message);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct View {
        warnings: Vec<String>,
        save: Option<PathBuf>,
        announced: Vec<String>,
    }

    impl ConverterView for View {
        fn warn(&mut self, text: &str) {
            self.warnings.push(text.to_owned());
        }
        fn choose_folder(&mut self, _title: &str, _initial: &Path) -> Option<PathBuf> {
            None
        }
        fn choose_save_file(
            &mut self,
            _title: &str,
            _directory: &Path,
            _name: &str,
            _extensions: &[String],
        ) -> Option<PathBuf> {
            self.save.clone()
        }
        fn announce(&mut self, text: &str) {
            self.announced.push(text.to_owned());
        }
    }

    fn dialog(folder_mode: bool) -> ConverterDialog {
        ConverterDialog {
            catalog: apricot_app::embedded_catalog("en"),
            folder_mode,
            targets: Vec::new(),
        }
    }

    #[test]
    fn formats_follow_the_path_and_keep_the_choice() {
        let mut dialog = dialog(false);
        let (labels, selected) = dialog.update_formats("", None).expect("formats");
        assert!(!labels.is_empty());
        assert_eq!(selected, 0);
        assert!(dialog.update_formats("", Some(0)).is_none());
        assert_eq!(filter_extensions("*.mp3;*.wav;*.*"), ["mp3", "wav"]);
        assert!(dialog.input_extensions().iter().any(|value| value == "mp3"));
    }

    #[test]
    fn a_missing_file_warns_and_a_cancelled_save_is_announced() {
        let mut dialog = dialog(false);
        let _ = dialog.update_formats("", None);
        let mut view = View {
            warnings: Vec::new(),
            save: None,
            announced: Vec::new(),
        };
        assert!(dialog.convert(&mut view, "", None, None, false).is_none());
        assert_eq!(view.warnings, [dialog.text("no_selection")]);
        let source =
            std::env::temp_dir().join(format!("apricot-converter-test-{}.mp3", std::process::id()));
        std::fs::write(&source, b"x").expect("written");
        let path = source.to_string_lossy().into_owned();
        let _ = dialog.update_formats(&path, None);
        assert!(
            dialog
                .convert(&mut view, &path, None, None, false)
                .is_none()
        );
        assert_eq!(view.announced, [dialog.text("conversion_cancelled")]);
        view.save = Some(source.with_extension("wav"));
        let start = dialog.convert(&mut view, &path, None, None, true);
        assert!(matches!(
            start,
            Some(ConverterStart::File {
                replace_original: true,
                ..
            })
        ));
        let _ = std::fs::remove_file(&source);
    }
}
