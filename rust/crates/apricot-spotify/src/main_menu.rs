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
    #[serde(default)]
    pub metadata: Option<CatalogItem>,
    pub label: String,
    pub target: MenuTarget,
}

impl MenuPin {
    pub fn new(label: impl Into<String>, target: MenuTarget) -> Self {
        Self {
            label: label.into(),
            target,
            metadata: None,
        }
    }
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
            metadata: Some(item.clone()),
        })
    }

    pub fn item(&self) -> CatalogItem {
        let (kind, uri) = match &self.target {
            MenuTarget::Playlist(uri) => (ItemKind::Playlist, uri),
            MenuTarget::Artist(uri) => (ItemKind::Artist, uri),
            MenuTarget::Category(uri) => (ItemKind::Page, uri),
            MenuTarget::Section { uri, .. } => (ItemKind::Section, uri),
        };
        if let Some(mut item) = self
            .metadata
            .clone()
            .filter(|item| item.kind == kind && item.uri == *uri)
        {
            item.kind = kind;
            item.uri.clone_from(uri);
            item.name.clone_from(&self.label);
            item.saved = None;
            return item;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{AccountStore, account_key};
    #[test]
    fn pins_survive_restart_are_account_scoped_and_do_not_change_library() {
        let temp = tempfile::tempdir().unwrap();
        let store = AccountStore::new(temp.path());
        let a = account_key("a");
        let b = account_key("b");
        let pin = MenuPin {
            metadata: None,
            label: "Music".into(),
            target: MenuTarget::Category("spotify:page:music".into()),
        };
        assert!(store.toggle_menu_pin(&a, pin.clone()).unwrap());
        assert_eq!(
            AccountStore::new(temp.path()).menu_pins(&a).unwrap(),
            vec![pin.clone()]
        );
        assert!(store.menu_pins(&b).unwrap().is_empty());
        assert!(!store.toggle_menu_pin(&a, pin).unwrap());
        assert!(store.menu_pins(&a).unwrap().is_empty());
        assert!(
            !store.file().exists(),
            "pinning must not rewrite accounts/library"
        );
    }
    #[test]
    fn malformed_pins_are_preserved_and_cannot_be_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let store = AccountStore::new(temp.path());
        let key = account_key("a");
        let path = store.account_dir(&key).join("main_menu.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"broken").unwrap();
        let pin = MenuPin {
            metadata: None,
            label: "Music".into(),
            target: MenuTarget::Category("spotify:page:music".into()),
        };
        assert!(store.toggle_menu_pin(&key, pin).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
    }
    #[test]
    fn only_artists_playlists_and_browse_sections_can_be_pinned() {
        for kind in [
            ItemKind::Track,
            ItemKind::Album,
            ItemKind::Show,
            ItemKind::User,
            ItemKind::Folder,
            ItemKind::LikedSongs,
        ] {
            let mut item = MenuPin {
                metadata: None,
                label: "Example".into(),
                target: MenuTarget::Artist("spotify:artist:a".into()),
            }
            .item();
            item.kind = kind;
            assert!(MenuPin::from_item(&item, Some(MenuOrigin::Home)).is_none());
        }
        let section = MenuPin {
            metadata: None,
            label: "Uniquely yours".into(),
            target: MenuTarget::Section {
                origin: MenuOrigin::Home,
                uri: "spotify:section:yours".into(),
            },
        }
        .item();
        assert!(MenuPin::from_item(&section, None).is_none());
        assert!(MenuPin::from_item(&section, Some(MenuOrigin::Home)).is_some());
    }
}
