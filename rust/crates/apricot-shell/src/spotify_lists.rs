//! Spotify lists, queue and devices (Windows `spotify_browse_win32.rs`,
//! `spotify_queue_win32.rs` and `spotify_devices_win32.rs`). The browse
//! screens arrive with unit M13b, the queue and the device list with M13c;
//! until then their entries say so.

use apricot_app::spotify::SpotifyHubEntry;
use apricot_core::SpotifyStamp;

use crate::{Shell, ShellUi};

#[allow(clippy::unused_self)] // Until the lists arrive (M13b, M13c).
impl Shell {
    /// A list entry of the hub (Search, Library, Liked songs, Playlists,
    /// Home, Daily Mixes, Recently played, Top, Browse all).
    pub(crate) fn spotify_open_hub_list(
        &mut self,
        ui: &mut dyn ShellUi,
        _entry: SpotifyHubEntry,
        label: &str,
    ) {
        if !self.spotify_ready_for_lists(ui) {
            return;
        }
        let label = label.split('\t').next().unwrap_or(label);
        let message = apricot_app::unavailable_feature_message(&self.catalog(), label);
        ui.set_status(&message, true);
    }

    pub(crate) fn spotify_browse_loaded(
        &mut self,
        _ui: &mut dyn ShellUi,
        _stamp: SpotifyStamp,
        _result: Result<apricot_spotify::CatalogResult, apricot_spotify::SpotifyError>,
    ) {
    }

    pub(crate) fn spotify_browse_edited(
        &mut self,
        _ui: &mut dyn ShellUi,
        _stamp: SpotifyStamp,
        _result: Result<apricot_spotify::EditOutcome, apricot_spotify::SpotifyError>,
    ) {
    }

    pub(crate) fn spotify_browse_back(&mut self, ui: &mut dyn ShellUi) {
        self.show_spotify_hub(ui);
    }

    pub(crate) fn show_spotify_queue(&mut self, ui: &mut dyn ShellUi) {
        let message =
            apricot_app::unavailable_feature_message(&self.catalog(), &self.text("spotify_queue"));
        ui.set_status(&message, true);
    }

    pub(crate) fn show_spotify_devices(&mut self, ui: &mut dyn ShellUi) {
        let message = apricot_app::unavailable_feature_message(
            &self.catalog(),
            &self.text("spotify_devices"),
        );
        ui.set_status(&message, true);
    }

    pub(crate) fn spotify_device_rows(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}
