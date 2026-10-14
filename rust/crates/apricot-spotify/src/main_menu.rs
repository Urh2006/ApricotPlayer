//! Account-local shortcuts into Spotify's normal catalog. No source item is moved.
use crate::{CatalogItem, ItemKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MenuOrigin {
    Home,
    BrowseAll,
    BrowsePage(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MenuTarget {
    Playlist(String),
    Artist(String),
    Category(String),
    Section { origin: MenuOrigin, uri: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MenuPin {
    pub label: String,
    pub target: MenuTarget,
}

impl MenuPin {
    pub fn from_item(item: &CatalogItem, section_origin: Option<MenuOrigin>) -> Option<Self> {
        let target = match item.kind {
            ItemKind::Playlist if item.uri.starts_with("spotify:playlist:") => {
                MenuTarget::Playlist(item.uri.clone())
            }
            ItemKind::Artist if item.uri.starts_with("spotify:artist:") => {
                MenuTarget::Artist(item.uri.clone())
            }
            ItemKind::Page if item.uri.starts_with("spotify:page:") => {
                MenuTarget::Category(item.uri.clone())
            }
            ItemKind::Section if !item.uri.is_empty() => MenuTarget::Section {
                origin: section_origin?,
                uri: item.uri.clone(),
            },
            _ => return None,
        };
        (!item.name.trim().is_empty()).then(|| Self {
            label: item.name.clone(),
            target,
        })
    }

    pub fn item(&self) -> CatalogItem {
        let (kind, uri) = match &self.target {
            MenuTarget::Playlist(uri) => (ItemKind::Playlist, uri),
            MenuTarget::Artist(uri) => (ItemKind::Artist, uri),
            MenuTarget::Category(uri) => (ItemKind::Page, uri),
            MenuTarget::Section { uri, .. } => (ItemKind::Section, uri),
        };
        CatalogItem {
            kind,
            uri: uri.clone(),
            name: self.label.clone(),
            subtitle: String::new(),
            album: String::new(),
            album_uri: String::new(),
            artist_uri: String::new(),
            duration_ms: None,
            playable: matches!(kind, ItemKind::Playlist | ItemKind::Artist),
            explicit: false,
            uid: None,
            saved: None,
            count: None,
            editable: false,
            format: String::new(),
        }
    }
}
