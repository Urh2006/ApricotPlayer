#![cfg_attr(windows, windows_subsystem = "windows")]

use apricot_app::{MainMenuAvailability, MainMenuModel, MenuVisibility, embedded_catalog};
use apricot_core::{
    action::ACTIONS, locale::LANGUAGES, menu::CUSTOMIZABLE_MAIN_MENU, setting::SettingId,
};
use apricot_platform::discover_windows_beta_paths;
use apricot_storage::{SettingsDocument, SettingsPaths, load_settings};
use apricot_updater::UpdateChannel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = UpdateChannel::LocalOnly;
    if std::env::args().any(|argument| argument == "--qualification-smoke") {
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
    let loaded = load_settings(&settings_paths, defaults);
    let settings = loaded.settings;
    let catalog = embedded_catalog(&settings.language);
    let availability = MainMenuAvailability {
        trending: visibility(settings.enable_trending),
        history: visibility(settings.enable_history),
        podcasts: visibility(settings.enable_podcasts_rss),
        ..Default::default()
    };
    let model = MainMenuModel::build(
        &catalog,
        availability,
        &settings.main_menu_hidden_actions,
        settings.show_shortcuts_in_labels,
        &settings.keyboard_shortcuts,
    );
    apricot_ui_windows::run_main_menu(model, env!("CARGO_PKG_VERSION"))
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
}

const fn visibility(enabled: bool) -> MenuVisibility {
    if enabled {
        MenuVisibility::Visible
    } else {
        MenuVisibility::Hidden
    }
}
