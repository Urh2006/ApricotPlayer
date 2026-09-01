//! Shared media identities. Source adapters enrich these values without changing
//! list ordering or selection identity.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MediaId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSource {
    Youtube,
    Soundcloud,
    Direct,
    Local,
    Podcast,
    Audiovault,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Audio,
    Video,
    LiveStream,
    Playlist,
    Channel,
    PodcastFeed,
    PodcastEpisode,
    Movie,
    TvShow,
    TvEpisode,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct MediaItem {
    pub id: MediaId,
    pub source: MediaSource,
    pub kind: MediaKind,
    pub title: String,
    #[serde(default)]
    pub url: Option<Url>,
    #[serde(default)]
    pub local_path: Option<String>,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}
