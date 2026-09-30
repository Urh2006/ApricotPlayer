//! Platform-neutral Spotify screens: the hub opened from the main menu and
//! the account list (`docs/SPOTIFY_PLAN.md` 4.1, 9.1).
//!
//! The hub lists only entries that already work. Later phases add their
//! entries here; there are no inert rows.

use std::collections::BTreeMap;

use apricot_core::{TranslationCatalog, action::action_by_id};
use apricot_spotify::{SpotifyAccount, SpotifyAccounts, SpotifyError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpotifyHubEntry {
    /// Browser login; the only way in without a saved account.
    LogIn,
    Accounts,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyHubItem {
    pub entry: SpotifyHubEntry,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyHubModel {
    pub accessible_name: String,
    pub items: Vec<SpotifyHubItem>,
}

fn with_shortcut(
    mut label: String,
    action_id: &str,
    show_shortcuts: bool,
    shortcuts: &BTreeMap<String, String>,
) -> String {
    if show_shortcuts && let Some(action) = action_by_id(action_id) {
        label.push('\t');
        label.push_str(
            shortcuts
                .get(action_id)
                .map_or(action.default_windows_shortcut, String::as_str),
        );
    }
    label
}

/// The active account when it is still logged in.
pub fn active_account(accounts: &SpotifyAccounts) -> Option<&SpotifyAccount> {
    let key = accounts.active.as_deref()?;
    accounts
        .accounts
        .iter()
        .find(|account| account.key == key && account.is_logged_in())
}

impl SpotifyHubModel {
    pub fn build(
        catalog: &TranslationCatalog,
        accounts: &SpotifyAccounts,
        show_shortcuts: bool,
        shortcuts: &BTreeMap<String, String>,
    ) -> Self {
        let mut items = Vec::new();
        if active_account(accounts).is_none() {
            items.push(SpotifyHubItem {
                entry: SpotifyHubEntry::LogIn,
                label: catalog.text("spotify_log_in").to_owned(),
            });
        }
        if !accounts.accounts.is_empty() {
            items.push(SpotifyHubItem {
                entry: SpotifyHubEntry::Accounts,
                label: with_shortcut(
                    catalog.text("spotify_accounts").to_owned(),
                    "spotify_accounts",
                    show_shortcuts,
                    shortcuts,
                ),
            });
        }
        Self {
            accessible_name: catalog.text("spotify").to_owned(),
            items,
        }
    }
}

/// One row of the account list: an account or the Add account row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpotifyAccountRow {
    Account { key: String, label: String },
    AddAccount { label: String },
}

impl SpotifyAccountRow {
    pub fn label(&self) -> &str {
        match self {
            Self::Account { label, .. } | Self::AddAccount { label } => label,
        }
    }

    pub fn key(&self) -> Option<&str> {
        match self {
            Self::Account { key, .. } => Some(key),
            Self::AddAccount { .. } => None,
        }
    }
}

/// "Name, Premium, active" style row text.
pub fn account_label(
    catalog: &TranslationCatalog,
    account: &SpotifyAccount,
    active: bool,
) -> String {
    let product = match account.product.as_str() {
        "premium" => catalog.text("spotify_premium").to_owned(),
        "" => String::new(),
        _ => catalog.text("spotify_not_premium").to_owned(),
    };
    let state = if !account.is_logged_in() {
        catalog.text("spotify_logged_out")
    } else if active {
        catalog.text("spotify_active_account")
    } else {
        ""
    };
    [account.display_name.as_str(), product.as_str(), state]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn account_rows(
    catalog: &TranslationCatalog,
    accounts: &SpotifyAccounts,
) -> Vec<SpotifyAccountRow> {
    let mut rows: Vec<SpotifyAccountRow> = accounts
        .accounts
        .iter()
        .map(|account| SpotifyAccountRow::Account {
            key: account.key.clone(),
            label: account_label(
                catalog,
                account,
                accounts.active.as_deref() == Some(account.key.as_str()),
            ),
        })
        .collect();
    rows.push(SpotifyAccountRow::AddAccount {
        label: catalog.text("spotify_add_account").to_owned(),
    });
    rows
}

/// Localized error text with `{error}` filled in when there is a detail.
pub fn error_text(catalog: &TranslationCatalog, error: &SpotifyError) -> String {
    catalog
        .text(error.text_key())
        .replace("{error}", error.detail())
        .trim_end_matches([' ', ':'])
        .to_owned()
}

pub fn named(catalog: &TranslationCatalog, key: &str, name: &str) -> String {
    catalog.text(key).replace("{name}", name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::main_menu::english_catalog;

    fn account(key: &str, product: &str, logged_in: bool) -> SpotifyAccount {
        SpotifyAccount {
            key: key.into(),
            display_name: format!("User {key}"),
            product: product.into(),
            country: "SI".into(),
            credentials: if logged_in {
                "blob".into()
            } else {
                String::new()
            },
        }
    }

    #[test]
    fn hub_without_accounts_offers_only_login() {
        let model = SpotifyHubModel::build(
            &english_catalog(),
            &SpotifyAccounts::default(),
            true,
            &BTreeMap::new(),
        );
        assert_eq!(model.accessible_name, "Spotify");
        assert_eq!(model.items.len(), 1);
        assert_eq!(model.items[0].entry, SpotifyHubEntry::LogIn);
    }

    #[test]
    fn hub_with_active_login_shows_accounts_with_shortcut() {
        let accounts = SpotifyAccounts {
            active: Some("a".into()),
            accounts: vec![account("a", "premium", true)],
            ..SpotifyAccounts::default()
        };
        let model = SpotifyHubModel::build(&english_catalog(), &accounts, true, &BTreeMap::new());
        assert_eq!(model.items.len(), 1);
        assert_eq!(model.items[0].entry, SpotifyHubEntry::Accounts);
        assert!(model.items[0].label.ends_with("\tCtrl+Alt+Shift+C"));
    }

    #[test]
    fn logged_out_active_account_offers_login_and_accounts() {
        let accounts = SpotifyAccounts {
            active: Some("a".into()),
            accounts: vec![account("a", "premium", false)],
            ..SpotifyAccounts::default()
        };
        let model = SpotifyHubModel::build(&english_catalog(), &accounts, false, &BTreeMap::new());
        let entries: Vec<_> = model.items.iter().map(|item| item.entry).collect();
        assert_eq!(entries, [SpotifyHubEntry::LogIn, SpotifyHubEntry::Accounts]);
        assert!(!model.items[1].label.contains('\t'));
    }

    #[test]
    fn account_rows_describe_product_and_state_and_end_with_add() {
        let catalog = english_catalog();
        let accounts = SpotifyAccounts {
            active: Some("a".into()),
            accounts: vec![
                account("a", "premium", true),
                account("b", "free", true),
                account("c", "premium", false),
            ],
            ..SpotifyAccounts::default()
        };
        let rows = account_rows(&catalog, &accounts);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].label(), "User a, Premium, active");
        assert_eq!(rows[1].label(), "User b, Free, playback needs Premium");
        assert_eq!(rows[2].label(), "User c, Premium, logged out");
        assert_eq!(rows[3].key(), None);
        assert_eq!(rows[3].label(), "Add account");
    }

    #[test]
    fn errors_fill_or_drop_the_detail() {
        let catalog = english_catalog();
        assert_eq!(
            error_text(&catalog, &SpotifyError::Network("timeout".into())),
            "Could not reach Spotify: timeout"
        );
        assert_eq!(
            error_text(&catalog, &SpotifyError::Network(String::new())),
            "Could not reach Spotify"
        );
        assert_eq!(
            error_text(&catalog, &SpotifyError::Cancelled),
            "Spotify login cancelled."
        );
    }
}
