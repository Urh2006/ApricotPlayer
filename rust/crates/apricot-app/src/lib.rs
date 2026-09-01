//! Application coordinator state. UI controls are projections of this state.

use apricot_core::{MediaItem, NavigationStack};

#[derive(Debug, Default)]
pub struct AppState {
    pub navigation: NavigationStack,
    pub player: PlayerSession,
}

#[derive(Debug, Default)]
pub struct PlayerSession {
    pub open: bool,
    pub current_item: Option<MediaItem>,
    pub volume: Option<f64>,
    pub output_device: Option<String>,
    pub autoplay_next: bool,
}

impl PlayerSession {
    pub fn close(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::PlayerSession;

    #[test]
    fn closing_player_resets_session_values() {
        let mut session = PlayerSession {
            open: true,
            current_item: None,
            volume: Some(80.0),
            output_device: Some("speakers".to_owned()),
            autoplay_next: true,
        };
        session.close();
        assert_eq!(session.volume, None);
        assert_eq!(session.output_device, None);
        assert!(!session.autoplay_next);
    }
}
