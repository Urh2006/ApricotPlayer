//! Searchable projection of actions that are meaningful in the current context.

use apricot_core::{
    TranslationCatalog,
    action::{ACTIONS, ActionScope},
};
use apricot_storage::SettingsDocument;

use crate::MainMenuAvailability;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActionFinderContext {
    pub scope: Option<ActionScope>,
    pub selection_available: bool,
    pub player_active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionFinderItem {
    pub action_id: &'static str,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionFinderModel {
    pub title: String,
    pub query_label: String,
    pub results_name: String,
    pub no_results_label: String,
    pub items: Vec<ActionFinderItem>,
}

impl ActionFinderModel {
    pub fn build(
        catalog: &TranslationCatalog,
        settings: &SettingsDocument,
        availability: MainMenuAvailability,
        context: ActionFinderContext,
    ) -> Self {
        let items = ACTIONS
            .iter()
            .filter(|action| action.id.as_str() != "open_action_finder")
            .filter(|action| {
                action_is_meaningful(
                    action.id.as_str(),
                    action.scopes,
                    settings,
                    availability,
                    context,
                )
            })
            .map(|action| {
                let mut label = catalog.text(action.label_key).to_owned();
                if settings.show_shortcuts_in_labels {
                    let shortcut = settings
                        .keyboard_shortcuts
                        .get(action.id.as_str())
                        .map_or(action.default_windows_shortcut, String::as_str);
                    label.push_str(", ");
                    label.push_str(shortcut);
                }
                ActionFinderItem {
                    action_id: action.id.as_str(),
                    label,
                }
            })
            .collect();
        Self {
            title: catalog.text("action_finder").to_owned(),
            query_label: catalog.text("action_finder_search").to_owned(),
            results_name: catalog.text("action_finder_results").to_owned(),
            no_results_label: catalog.text("action_finder_no_results").to_owned(),
            items,
        }
    }

    pub fn filtered_items(&self, query: &str) -> Vec<&ActionFinderItem> {
        let words: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return self.items.iter().collect();
        }
        self.items
            .iter()
            .filter(|item| {
                let label = item.label.to_lowercase();
                words.iter().all(|word| label.contains(word))
            })
            .collect()
    }
}

fn action_is_meaningful(
    id: &str,
    scopes: &[ActionScope],
    settings: &SettingsDocument,
    availability: MainMenuAvailability,
    context: ActionFinderContext,
) -> bool {
    if scopes.contains(&ActionScope::Global) {
        return match id {
            "background_play_pause" => context.player_active,
            "open_current_downloads" => availability.download_count > 0,
            "open_playback_queue" => availability.playback_queue_count > 0,
            "open_history" => settings.enable_history,
            "open_podcasts_rss" => settings.enable_podcasts_rss,
            _ => true,
        };
    }
    if scopes.contains(&ActionScope::Player)
        && context.scope == Some(ActionScope::Player)
        && context.player_active
    {
        return true;
    }
    context.selection_available && context.scope.is_some_and(|scope| scopes.contains(&scope))
}

#[cfg(test)]
mod tests {
    use apricot_core::ActionScope;
    use apricot_storage::SettingsDocument;

    use super::{ActionFinderContext, ActionFinderModel};
    use crate::{MainMenuAvailability, english_catalog};

    #[test]
    fn hidden_main_menu_actions_remain_discoverable_by_shortcut() {
        let settings = SettingsDocument {
            main_menu_hidden_actions: vec!["search".to_owned()],
            ..SettingsDocument::default()
        };
        let model = ActionFinderModel::build(
            &english_catalog(),
            &settings,
            MainMenuAvailability::default(),
            ActionFinderContext::default(),
        );
        assert!(
            model
                .items
                .iter()
                .any(|item| item.action_id == "open_search")
        );
    }

    #[test]
    fn contextual_player_actions_never_appear_without_an_active_player() {
        let settings = SettingsDocument::default();
        let without_player = ActionFinderModel::build(
            &english_catalog(),
            &settings,
            MainMenuAvailability::default(),
            ActionFinderContext {
                scope: Some(ActionScope::Player),
                selection_available: false,
                player_active: false,
            },
        );
        assert!(
            without_player
                .items
                .iter()
                .all(|item| item.action_id != "player_volume_status")
        );

        let with_player = ActionFinderModel::build(
            &english_catalog(),
            &settings,
            MainMenuAvailability::default(),
            ActionFinderContext {
                scope: Some(ActionScope::Player),
                selection_available: false,
                player_active: true,
            },
        );
        assert!(
            with_player
                .items
                .iter()
                .any(|item| item.action_id == "player_volume_status")
        );
    }

    #[test]
    fn filtering_requires_every_query_word_without_reordering_items() {
        let settings = SettingsDocument::default();
        let model = ActionFinderModel::build(
            &english_catalog(),
            &settings,
            MainMenuAvailability::default(),
            ActionFinderContext::default(),
        );
        let filtered = model.filtered_items("search ctrl");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].action_id, "open_search");
    }
}
