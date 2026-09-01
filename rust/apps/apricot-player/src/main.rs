use apricot_core::{
    action::ACTIONS, locale::LANGUAGES, menu::CUSTOMIZABLE_MAIN_MENU, setting::SettingId,
};
use apricot_updater::UpdateChannel;

fn main() {
    let channel = UpdateChannel::LocalOnly;
    println!(
        "ApricotPlayer {} foundation: {} settings, {} actions, {} menu items, {} languages, remote updates: {}",
        env!("CARGO_PKG_VERSION"),
        SettingId::ALL.len(),
        ACTIONS.len(),
        CUSTOMIZABLE_MAIN_MENU.len(),
        LANGUAGES.len(),
        channel.allows_remote_install()
    );
}
