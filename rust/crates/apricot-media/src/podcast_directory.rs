//! Normalized podcast directory search results.

use url::Url;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PodcastDirectoryItem {
    pub title: String,
    pub author: String,
    pub genre: String,
    pub episode_count: u64,
    pub feed_url: Url,
    pub webpage_url: Url,
}
