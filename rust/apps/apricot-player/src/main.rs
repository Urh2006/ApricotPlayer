#![cfg_attr(windows, windows_subsystem = "windows")]

use apricot_app::{MainMenuAvailability, MainMenuModel, MenuVisibility, english_catalog};
use apricot_core::{
    action::ACTIONS, locale::LANGUAGES, menu::CUSTOMIZABLE_MAIN_MENU, setting::SettingId,
};
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

    let availability = MainMenuAvailability {
        history: MenuVisibility::Visible,
        podcasts: MenuVisibility::Visible,
        ..Default::default()
    };
    let model = MainMenuModel::build(&english_catalog(), availability, &[], true);
    apricot_ui_windows::run_main_menu(model, env!("CARGO_PKG_VERSION"))
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
}
