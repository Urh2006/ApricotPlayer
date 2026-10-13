//! App and yt-dlp updates (Windows `start_ytdlp_update_check`,
//! `start_app_update_check`, `handle_update_message`, `offer_app_update`,
//! `begin_app_update_install` and `finish_app_update_install`). The checks
//! and downloads run on worker threads; their messages arrive through
//! `poll_background`. On macOS the downloaded zip replaces the application
//! bundle through a shell script after the application quits.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use apricot_updater::{AppUpdateCheck, DownloadedUpdate, Release, ReleaseAsset, YtdlpUpdate};

use crate::{MainMenuSelection, Shell, ShellUi, View};

/// Python's startup delays of the yt-dlp and app checks, in seconds.
const YTDLP_STARTUP_DELAY: f64 = 3.5;
const APP_UPDATE_STARTUP_DELAY: f64 = 5.5;

/// Windows `UpdateMessage`.
enum UpdateMessage {
    YtdlpMissing,
    YtdlpUpdating,
    YtdlpFinished {
        manual: bool,
        result: Result<YtdlpUpdate, String>,
    },
    AppChecked {
        manual: bool,
        prompt: bool,
        notify: bool,
        result: AppUpdateCheck,
    },
    AppDownloadProgress {
        version: String,
        percent: Option<u8>,
    },
    AppDownloaded {
        version: String,
        result: Result<DownloadedUpdate, String>,
    },
}

#[derive(Default)]
pub(crate) struct UpdateState {
    workers: Vec<Receiver<UpdateMessage>>,
    ytdlp_check_running: bool,
    app_check_running: bool,
    pending_app_update: Option<(Release, ReleaseAsset)>,
    progress_open: bool,
}

/// The "Update available" dialog (Windows `show_update_prompt`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdatePrompt {
    pub title: String,
    pub version: String,
    pub whats_new: String,
    pub changelog: String,
    pub question: String,
    pub update_now: String,
    pub skip: String,
}

/// Windows `update_log_path`.
fn update_log_path() -> Option<PathBuf> {
    apricot_platform::discover_app_paths()
        .ok()
        .map(|paths| paths.app_data.join("updater.log"))
}

/// Python `log_update_event`.
fn log_update_event(message: &str) {
    if let Some(path) = update_log_path() {
        apricot_updater::script::log_update_event(&path, message);
    }
}

impl Shell {
    /// Windows `configure_app_update_timer`: every 0.5 to 24 hours, six by
    /// default.
    pub(crate) fn configure_app_update_timer(&self, ui: &mut dyn ShellUi) {
        let settings = self.application.settings();
        if !settings.auto_update_app {
            ui.set_app_update_timer(None);
            return;
        }
        let hours = settings.app_update_interval_hours;
        let hours = if hours.is_finite() {
            hours.clamp(0.5, 24.0)
        } else {
            6.0
        };
        ui.set_app_update_timer(Some((hours * 3600.0).max(1800.0)));
    }

    /// Windows `schedule_startup_update_checks`.
    pub(crate) fn schedule_startup_update_checks(&self, ui: &mut dyn ShellUi) {
        let settings = self.application.settings();
        ui.schedule_startup_update_checks(
            settings.auto_update_ytdlp.then_some(YTDLP_STARTUP_DELAY),
            settings.auto_update_app.then_some(APP_UPDATE_STARTUP_DELAY),
        );
    }

    /// The startup yt-dlp check.
    pub fn startup_ytdlp_update_check(&mut self, ui: &mut dyn ShellUi) {
        self.start_ytdlp_update_check(ui, false);
    }

    /// The startup app check, which asks at once.
    pub fn startup_app_update_check(&mut self, ui: &mut dyn ShellUi) {
        self.start_app_update_check(ui, false, true, false);
    }

    /// The periodic app check, which only marks the update in the menu.
    pub fn app_update_timer_fired(&mut self, ui: &mut dyn ShellUi) {
        self.start_app_update_check(ui, false, false, true);
    }

    fn watch_update_worker(&mut self, ui: &mut dyn ShellUi, receiver: Receiver<UpdateMessage>) {
        self.updates.workers.push(receiver);
        ui.set_background_polling(true);
    }

    /// Python `manual_ytdlp_update_check`; Settings applied its controls
    /// first.
    pub fn manual_ytdlp_update_check(&mut self, ui: &mut dyn ShellUi) {
        ui.set_status(&self.text("checking_updates"), true);
        self.start_ytdlp_update_check(ui, true);
    }

    /// Python `start_ytdlp_update_check` and `update_ytdlp_worker`.
    fn start_ytdlp_update_check(&mut self, ui: &mut dyn ShellUi, manual: bool) {
        if self.updates.ytdlp_check_running {
            return;
        }
        let Some(bundled) = crate::youtube::components_directory() else {
            return;
        };
        self.updates.ytdlp_check_running = true;
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let executable = apricot_platform::app_update::preferred_ytdlp_executable(&bundled);
            if !executable.is_file() {
                let _ = sender.send(UpdateMessage::YtdlpMissing);
                return;
            }
            let current = apricot_platform::app_update::ytdlp_version(&executable);
            let result = apricot_platform::app_update::user_components_directory()
                .ok_or_else(|| "Application data path is unavailable".to_owned())
                .and_then(|components| {
                    let mut transport =
                        apricot_platform::app_update::update_transport(env!("CARGO_PKG_VERSION"));
                    let updating = sender.clone();
                    apricot_updater::update_ytdlp_component(
                        transport.as_mut(),
                        &current,
                        &components,
                        &mut || {
                            let _ = updating.send(UpdateMessage::YtdlpUpdating);
                        },
                    )
                });
            let _ = sender.send(UpdateMessage::YtdlpFinished { manual, result });
        });
        self.watch_update_worker(ui, receiver);
    }

    /// Python `start_app_update_check` and `app_update_worker`.
    pub fn start_app_update_check(
        &mut self,
        ui: &mut dyn ShellUi,
        manual: bool,
        prompt: bool,
        notify: bool,
    ) {
        let catalog = self.catalog();
        if !manual && !self.application.settings().auto_update_app {
            ui.set_status(catalog.text("app_update_disabled"), false);
            return;
        }
        if self.updates.app_check_running {
            if manual {
                ui.set_status(catalog.text("checking_app_updates"), true);
            }
            return;
        }
        self.updates.app_check_running = true;
        ui.set_status(catalog.text("checking_app_updates"), manual);
        let channel = self.application.settings().update_channel.clone();
        let skipped = self.application.settings().skipped_update_version.clone();
        let no_changelog = catalog.text("no_changelog").to_owned();
        let installed = std::env::current_exe()
            .is_ok_and(|executable| apricot_platform::app_update::is_installed_build(&executable));
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut transport =
                apricot_platform::app_update::update_transport(env!("CARGO_PKG_VERSION"));
            let mut feed = apricot_updater::GithubReleaseFeed {
                transport: transport.as_mut(),
            };
            let result = apricot_updater::check_app_update(
                &mut feed,
                &apricot_updater::APP_PACKAGE,
                &channel,
                env!("CARGO_PKG_VERSION"),
                &skipped,
                manual,
                installed,
                &no_changelog,
            );
            let _ = sender.send(UpdateMessage::AppChecked {
                manual,
                prompt,
                notify,
                result,
            });
        });
        self.watch_update_worker(ui, receiver);
    }

    /// The `wx.CallAfter` side of the update workers; `true` while one
    /// runs.
    pub(crate) fn poll_updates(&mut self, ui: &mut dyn ShellUi) -> bool {
        let mut messages = Vec::new();
        self.updates.workers.retain(|receiver| {
            loop {
                match receiver.try_recv() {
                    Ok(message) => {
                        let finished = !matches!(
                            message,
                            UpdateMessage::YtdlpUpdating
                                | UpdateMessage::AppDownloadProgress { .. }
                        );
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
            self.handle_update_message(ui, message);
        }
        !self.updates.workers.is_empty()
    }

    fn handle_update_message(&mut self, ui: &mut dyn ShellUi, message: UpdateMessage) {
        let catalog = self.catalog();
        match message {
            UpdateMessage::YtdlpMissing => {
                self.updates.ytdlp_check_running = false;
                ui.set_status(catalog.text("missing_ytdlp"), true);
            }
            UpdateMessage::YtdlpUpdating => {
                ui.set_status(catalog.text("components_updating"), true);
            }
            UpdateMessage::YtdlpFinished { manual, result } => {
                self.updates.ytdlp_check_running = false;
                match result {
                    Ok(YtdlpUpdate::Updated(_)) => {
                        ui.set_status(catalog.text("components_updated"), true);
                    }
                    Ok(YtdlpUpdate::Current) => {
                        if manual {
                            ui.set_status(catalog.text("updates_ok"), true);
                        }
                    }
                    Err(error) => {
                        let text = catalog.text("updates_failed").replace("{error}", &error);
                        ui.set_status(&text, true);
                    }
                }
            }
            UpdateMessage::AppChecked {
                manual,
                prompt,
                notify,
                result,
            } => self.app_update_checked(ui, manual, prompt, notify, result),
            UpdateMessage::AppDownloadProgress { version, percent } => {
                // Python `update_app_update_progress`.
                if !self.updates.progress_open {
                    return;
                }
                match percent {
                    Some(percent) => ui.update_conversion_progress(
                        usize::from(percent.min(100)),
                        &catalog
                            .text("update_download_percent")
                            .replace("{version}", &version)
                            .replace("{percent}", &percent.min(100).to_string()),
                    ),
                    None => ui.update_conversion_progress(
                        0,
                        &catalog
                            .text("update_download_unknown")
                            .replace("{version}", &version),
                    ),
                }
            }
            UpdateMessage::AppDownloaded { version, result } => match result {
                Ok(downloaded) => {
                    // Python `update_app_update_finished`.
                    if self.updates.progress_open {
                        ui.update_conversion_progress(
                            100,
                            catalog.text("update_download_complete"),
                        );
                    }
                    ui.set_status(catalog.text("update_download_complete"), true);
                    self.finish_app_update_install(ui, &downloaded, &version);
                }
                Err(error) => {
                    // Python `update_app_update_failed`.
                    log_update_event(&format!("Update failed before install: {error}"));
                    self.close_update_progress(ui);
                    ui.show_error(&catalog.text("app_update_failed").replace("{error}", &error));
                }
            },
        }
    }

    /// Python `report_app_update_status`: status, and speech when manual.
    fn app_update_checked(
        &mut self,
        ui: &mut dyn ShellUi,
        manual: bool,
        prompt: bool,
        notify: bool,
        result: AppUpdateCheck,
    ) {
        let catalog = self.catalog();
        self.updates.app_check_running = false;
        match result {
            AppUpdateCheck::UpToDate => ui.set_status(catalog.text("app_up_to_date"), manual),
            AppUpdateCheck::Skipped(version) => {
                let text = catalog
                    .text("update_skip_status")
                    .replace("{version}", &version);
                ui.set_status(&text, manual);
            }
            AppUpdateCheck::NoAsset => {
                let missing = if cfg!(target_os = "macos") {
                    "no macOS asset found in release"
                } else {
                    "no Windows asset found in release"
                };
                let text = catalog
                    .text("app_update_failed")
                    .replace("{error}", missing);
                ui.set_status(&text, manual);
            }
            AppUpdateCheck::Available { release, asset } => {
                if prompt {
                    log_update_event(&format!(
                        "Prompting for update {} with asset {}",
                        release.version(),
                        asset.name
                    ));
                    self.offer_app_update(ui, &release, asset, false);
                } else {
                    self.store_pending_app_update(ui, *release, asset, notify);
                }
            }
        }
    }

    /// Python `store_pending_app_update`: the main menu offers "Update
    /// available" and, in the background, a system notification says so.
    fn store_pending_app_update(
        &mut self,
        ui: &mut dyn ShellUi,
        release: Release,
        asset: ReleaseAsset,
        notify: bool,
    ) {
        let version = release.version();
        if !apricot_updater::is_newer_version(&version, env!("CARGO_PKG_VERSION")) {
            return;
        }
        let focused = ui.app_is_active();
        self.updates.pending_app_update = Some((release, asset));
        self.application
            .set_pending_app_update_version(Some(version.clone()));
        let catalog = self.catalog();
        let text = catalog
            .text("app_update_ready_status")
            .replace("{version}", &version);
        ui.set_status(&text, false);
        let settings = self.application.settings();
        if notify && settings.app_update_notifications && settings.windows_notifications && !focused
        {
            let message = catalog
                .text("app_update_notification_message")
                .replace("{version}", &version);
            ui.show_system_notification(catalog.text("update_available_title"), &message);
        }
        if self.view == View::MainMenu {
            self.refresh_main_menu(ui, MainMenuSelection::LastActivated, None, false);
        }
    }

    /// Python `open_pending_app_update`, the "Update available" menu row.
    pub(crate) fn open_pending_app_update(&mut self, ui: &mut dyn ShellUi) {
        match self.updates.pending_app_update.clone() {
            Some((release, asset)) => self.offer_app_update(ui, &release, asset, true),
            None => self.start_app_update_check(ui, true, true, false),
        }
    }

    /// The shared part of Python `prompt_for_app_update` and
    /// `open_pending_app_update`.
    fn offer_app_update(
        &mut self,
        ui: &mut dyn ShellUi,
        release: &Release,
        asset: ReleaseAsset,
        from_menu: bool,
    ) {
        let version = release.version();
        let catalog = self.catalog();
        // Python only installs in the frozen build; a local beta only from
        // its test feed (D-011).
        if !apricot_platform::app_update::app_update_install_allowed() {
            ui.show_error(
                &catalog
                    .text("update_source_only")
                    .replace("{version}", &version),
            );
            return;
        }
        let changelog =
            apricot_updater::release_changelog_text(release, catalog.text("no_changelog"));
        let accepted = ui.update_prompt(&UpdatePrompt {
            title: catalog.text("update_available_title").to_owned(),
            version: catalog
                .text("update_version_heading")
                .replace("{version}", &version),
            whats_new: catalog.text("whats_new").to_owned(),
            changelog,
            question: catalog.text("update_now").to_owned(),
            update_now: catalog.text("update_now_button").to_owned(),
            skip: catalog.text("skip_version_button").to_owned(),
        });
        let kind = if from_menu {
            "pending update"
        } else {
            "update"
        };
        if accepted {
            log_update_event(&format!("User selected {kind} now for {version}"));
            if !self
                .application
                .settings()
                .skipped_update_version
                .is_empty()
                && let Err(error) = self.application.set_skipped_update_version("")
            {
                ui.show_error(&error.to_string());
            }
            self.begin_app_update_install(ui, release, asset);
            return;
        }
        log_update_event(&format!("User skipped {kind} {version}"));
        self.updates.pending_app_update = None;
        self.application.set_pending_app_update_version(None);
        if let Err(error) = self.application.set_skipped_update_version(&version) {
            ui.show_error(&error.to_string());
        }
        ui.set_status(
            &catalog
                .text("update_skipped")
                .replace("{version}", &version),
            true,
        );
        if from_menu && self.view == View::MainMenu {
            self.refresh_main_menu(ui, MainMenuSelection::LastActivated, None, true);
        }
    }

    /// Python `begin_app_update_install` and `download_and_install_update`.
    fn begin_app_update_install(
        &mut self,
        ui: &mut dyn ShellUi,
        release: &Release,
        asset: ReleaseAsset,
    ) {
        let version = release.version();
        log_update_event(&format!("Beginning update {version}; asset={}", asset.name));
        self.close_update_progress(ui);
        let catalog = self.catalog();
        ui.show_conversion_progress(
            catalog.text("update_progress_title"),
            &catalog
                .text("update_download_unknown")
                .replace("{version}", &version),
            100,
        );
        self.updates.progress_open = true;
        ui.set_status(
            &catalog
                .text("downloading_update")
                .replace("{version}", &version),
            true,
        );
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut transport =
                apricot_platform::app_update::update_transport(env!("CARGO_PKG_VERSION"));
            let progress_sender = sender.clone();
            let progress_version = version.clone();
            let result = apricot_updater::download_app_update(
                transport.as_mut(),
                &apricot_updater::APP_PACKAGE,
                &asset,
                &mut |line| log_update_event(line),
                &mut |percent| {
                    let _ = progress_sender.send(UpdateMessage::AppDownloadProgress {
                        version: progress_version.clone(),
                        percent,
                    });
                },
            );
            let _ = sender.send(UpdateMessage::AppDownloaded { version, result });
        });
        self.watch_update_worker(ui, receiver);
    }

    /// Python `close_update_progress_dialog`.
    fn close_update_progress(&mut self, ui: &mut dyn ShellUi) {
        if std::mem::take(&mut self.updates.progress_open) {
            ui.close_conversion_progress();
        }
    }

    /// Python `finish_app_update_install`: start the install script, then
    /// leave so it can replace the application.
    fn finish_app_update_install(
        &mut self,
        ui: &mut dyn ShellUi,
        downloaded: &DownloadedUpdate,
        version: &str,
    ) {
        let catalog = self.catalog();
        if !apricot_platform::app_update::app_update_install_allowed() {
            self.close_update_progress(ui);
            ui.show_error(
                &catalog
                    .text("update_source_only")
                    .replace("{version}", version),
            );
            return;
        }
        let log_path = match launch_install(downloaded, version) {
            Ok(log_path) => log_path,
            Err(error) => {
                self.close_update_progress(ui);
                ui.show_error(&catalog.text("app_update_failed").replace("{error}", &error));
                return;
            }
        };
        ui.set_status(
            &catalog
                .text("installing_update")
                .replace("{version}", version),
            false,
        );
        self.close_update_progress(ui);
        ui.set_status(catalog.text("update_install_started"), true);
        ui.set_status(
            &catalog
                .text("update_install_log")
                .replace("{path}", &log_path.to_string_lossy()),
            false,
        );
        log_update_event("Exiting ApricotPlayer for update");
        // Python `exit_for_update`.
        ui.exit();
    }
}

/// Writes and starts the install script of this platform; the update log's
/// path.
fn launch_install(downloaded: &DownloadedUpdate, version: &str) -> Result<PathBuf, String> {
    use apricot_updater::{APP_PACKAGE, script};

    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let folder = executable
        .parent()
        .ok_or_else(|| "Application path is unavailable".to_owned())?;
    log_update_event(&format!(
        "Preparing install for {version}; package={}; current_exe={}",
        downloaded.path.display(),
        executable.display()
    ));
    let log_path =
        update_log_path().ok_or_else(|| "Application data path is unavailable".to_owned())?;
    let input = script::UpdateScriptInput {
        package: &APP_PACKAGE,
        downloaded_path: &downloaded.path,
        target_dir: folder,
        process_id: std::process::id(),
        log_path: &log_path,
        restart: true,
        expected_sha256: &downloaded.sha256,
    };
    let name = downloaded.path.to_string_lossy();
    if APP_PACKAGE.is_app_bundle() {
        if !APP_PACKAGE.is_portable_zip_asset(&name) {
            return Err(format!("unexpected update asset name: {name}"));
        }
        // The script replaces the folder two levels up, so it runs only
        // from the installed bundle, never from a development build.
        let in_bundle = executable.ends_with(APP_PACKAGE.executable)
            && folder
                .parent()
                .and_then(std::path::Path::parent)
                .and_then(std::path::Path::extension)
                .is_some_and(|extension| extension == "app");
        if !in_bundle {
            return Err("ApricotPlayer must run from its application bundle".to_owned());
        }
        let script_path = script::launch_macos_update_script(&script::macos_update_script(&input))?;
        log_update_event(&format!("Launched update script {}", script_path.display()));
        return Ok(log_path);
    }
    let (text, prefix) = if APP_PACKAGE.is_installer_asset(&name) {
        (
            script::installer_update_script(&input),
            "apricotplayer-installer-update-",
        )
    } else if APP_PACKAGE.is_portable_zip_asset(&name) {
        (
            script::portable_zip_update_script(&input),
            "apricotplayer-portable-update-",
        )
    } else {
        return Err(format!("unexpected update asset name: {name}"));
    };
    let script_path = script::write_update_script(&text, prefix)?;
    log_update_event(&format!(
        "Launching update script {}",
        script_path.display()
    ));
    script::launch_update_script(&script_path)?;
    Ok(log_path)
}

#[cfg(test)]
mod regressions {
    #[test]
    fn simultaneous_component_checks_share_one_worker() {
        let mut shell = crate::tests::keyboard::shell();
        let mut ui = crate::tests::equalizer::Ui::default();
        shell.start_ytdlp_update_check(&mut ui, false);
        shell.start_ytdlp_update_check(&mut ui, true);
        assert_eq!(shell.updates.workers.len(), 1);
    }
}
