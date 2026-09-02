#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;

use apricot_app::{ActivationRequest, Application, MainMenuAvailability, SettingsController};
use apricot_core::{
    action::ACTIONS, locale::LANGUAGES, menu::CUSTOMIZABLE_MAIN_MENU, setting::SettingId,
};
use apricot_platform::{
    ApplicationIdentity, SingleInstanceOutcome, acquire_single_instance,
    discover_windows_beta_paths, sync_startup_registration,
};
use apricot_storage::{SettingsDocument, SettingsPaths};
use apricot_updater::UpdateChannel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = UpdateChannel::LocalOnly;
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let start_hidden = arguments
        .iter()
        .any(|argument| argument == "--start-in-tray");
    if arguments
        .iter()
        .any(|argument| argument == "--qualification-smoke")
    {
        println!(
            "ApricotPlayer {} foundation: {} settings, {} actions, {} menu items, {} languages, remote updates: {}",
            env!("CARGO_PKG_VERSION"),
            SettingId::ALL.len(),
            ACTIONS.len(),
            CUSTOMIZABLE_MAIN_MENU.len(),
            LANGUAGES.len(),
            channel.allows_remote_install()
        );
        return Ok(());
    }

    let paths = discover_windows_beta_paths()?;
    let defaults = SettingsDocument::with_platform_defaults(
        paths.downloads.to_string_lossy().into_owned(),
        paths.cache.to_string_lossy().into_owned(),
        "beta",
    );
    let settings_paths = SettingsPaths::for_app_data(&paths.app_data, &paths.legacy_app_data);
    let settings = SettingsController::load(settings_paths, defaults);
    let startup_file = startup_file_argument(&arguments);
    let instance = acquire_single_instance(ApplicationIdentity::RustBeta)?;
    let _instance_guard = match instance {
        SingleInstanceOutcome::Primary(guard) => guard,
        SingleInstanceOutcome::Secondary => {
            if let Some(path) = startup_file {
                apricot_ui_windows::forward_to_existing(&ActivationRequest::OpenFile(path))?;
            } else if settings.current().close_to_tray {
                apricot_ui_windows::forward_to_existing(&ActivationRequest::Show)?;
            } else {
                let catalog = apricot_app::embedded_catalog(&settings.current().language);
                let message = catalog.text("already_open");
                apricot_ui_windows::show_already_open(message);
            }
            return Ok(());
        }
    };
    if let Ok(executable) = std::env::current_exe() {
        let _ = sync_startup_registration(
            ApplicationIdentity::RustBeta,
            &executable,
            settings.current().start_with_windows,
        );
    }
    let mut application = Application::new(settings, MainMenuAvailability::default());
    if !application.settings().language_prompted {
        let selected =
            apricot_ui_windows::choose_initial_language(&application.settings().language)?;
        application.complete_initial_language(selected)?;
    }
    if let Some(path) = startup_file {
        application.enqueue_activation(ActivationRequest::OpenFile(path));
    }
    apricot_ui_windows::run_application(application, env!("CARGO_PKG_VERSION"), start_hidden)
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
}

fn startup_file_argument(arguments: &[std::ffi::OsString]) -> Option<PathBuf> {
    arguments
        .iter()
        .find(|argument| !argument.to_string_lossy().starts_with("--"))
        .map(PathBuf::from)
}
