//! Explicit one-step navigation and stable focus restoration.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct FocusId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    MainMenu,
    Search,
    Results,
    Trending,
    ChannelResults,
    PlaylistResults,
    SoundcloudArtistTracks,
    DirectLink,
    LocalFolder,
    Favorites,
    Bookmarks,
    History,
    UserPlaylists,
    UserPlaylistItems,
    Subscriptions,
    NotificationCenter,
    RssFeeds,
    RssItems,
    PodcastCategories,
    PodcastSearchResults,
    AudiovaultMenu,
    AudiovaultSearch,
    AudiovaultResults,
    AudiovaultEpisodes,
    DownloadQueue,
    PlaybackQueue,
    Settings,
    Player,
}

impl Route {
    pub const ALL: &[Self] = &[
        Self::MainMenu,
        Self::Search,
        Self::Results,
        Self::Trending,
        Self::ChannelResults,
        Self::PlaylistResults,
        Self::SoundcloudArtistTracks,
        Self::DirectLink,
        Self::LocalFolder,
        Self::Favorites,
        Self::Bookmarks,
        Self::History,
        Self::UserPlaylists,
        Self::UserPlaylistItems,
        Self::Subscriptions,
        Self::NotificationCenter,
        Self::RssFeeds,
        Self::RssItems,
        Self::PodcastCategories,
        Self::PodcastSearchResults,
        Self::AudiovaultMenu,
        Self::AudiovaultSearch,
        Self::AudiovaultResults,
        Self::AudiovaultEpisodes,
        Self::DownloadQueue,
        Self::PlaybackQueue,
        Self::Settings,
        Self::Player,
    ];
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RouteFrame {
    pub route: Route,
    #[serde(default)]
    pub focus: Option<FocusId>,
    #[serde(default)]
    pub selected_item_id: Option<String>,
    #[serde(default)]
    pub selected_index: usize,
    #[serde(default)]
    pub parameters: serde_json::Map<String, serde_json::Value>,
}

impl RouteFrame {
    pub fn new(route: Route) -> Self {
        Self {
            route,
            focus: None,
            selected_item_id: None,
            selected_index: 0,
            parameters: serde_json::Map::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationStack {
    root: RouteFrame,
    frames: Vec<RouteFrame>,
}

impl Default for NavigationStack {
    fn default() -> Self {
        Self {
            root: RouteFrame::new(Route::MainMenu),
            frames: Vec::new(),
        }
    }
}

impl NavigationStack {
    pub fn current(&self) -> &RouteFrame {
        self.frames.last().unwrap_or(&self.root)
    }

    pub fn push(&mut self, frame: RouteFrame) {
        self.frames.push(frame);
    }

    pub fn replace(&mut self, frame: RouteFrame) {
        if let Some(current) = self.frames.last_mut() {
            *current = frame;
        } else {
            self.root = frame;
        }
    }

    pub fn back(&mut self) -> Option<RouteFrame> {
        if self.frames.is_empty() {
            return None;
        }
        self.frames.pop();
        Some(self.current().clone())
    }

    pub fn depth(&self) -> usize {
        self.frames.len() + 1
    }
}

#[cfg(test)]
mod tests {
    use super::{NavigationStack, Route, RouteFrame};

    #[test]
    fn escape_model_unwinds_exactly_one_route() {
        let mut stack = NavigationStack::default();
        stack.push(RouteFrame::new(Route::Results));
        stack.push(RouteFrame::new(Route::ChannelResults));
        stack.push(RouteFrame::new(Route::Player));

        assert_eq!(stack.back().expect("channel").route, Route::ChannelResults);
        assert_eq!(stack.back().expect("results").route, Route::Results);
        assert_eq!(stack.back().expect("main menu").route, Route::MainMenu);
        assert_eq!(stack.back(), None);
    }
}
