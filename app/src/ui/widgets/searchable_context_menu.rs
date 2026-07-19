use egui::{Key, RichText, ScrollArea, TextEdit, Ui};
use std::collections::BTreeMap;

type MenuItem<T> = (String, Option<String>, T);

/// One entry in a searchable menu.
///
/// `keywords` contain aliases that should find the item without being shown in
/// the menu. Disabled items remain visible, but mouse and keyboard selection
/// both skip them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchableItem<T> {
    pub label: String,
    pub category: Option<String>,
    pub keywords: Vec<String>,
    pub enabled: bool,
    /// Stable QA bridge identifier for the real egui button, when the menu is
    /// used by a coordinate-driven integration test.
    pub qa_id: Option<String>,
    /// Caller-defined action data exposed alongside `qa_id`.
    pub qa_metadata: Option<serde_json::Value>,
    pub value: T,
}

impl<T> SearchableItem<T> {
    pub fn new(label: impl Into<String>, value: T) -> Self {
        Self {
            label: label.into(),
            category: None,
            keywords: Vec::new(),
            enabled: true,
            qa_id: None,
            qa_metadata: None,
            value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchNavigation {
    Previous,
    Next,
    First,
    Last,
}

#[derive(Clone, Default)]
struct MenuState {
    query: String,
    /// Original item index, not an index into a transient filtered list.
    selected_item: Option<usize>,
}

/// Return the indices whose label or keyword matches every whitespace-separated
/// query term. Matching is case-insensitive and preserves input order.
pub fn filter_searchable_items<T>(items: &[SearchableItem<T>], query: &str) -> Vec<usize> {
    let terms = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();

    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            if terms.is_empty() {
                return true;
            }
            let label = item.label.to_lowercase();
            let keywords = item
                .keywords
                .iter()
                .map(|keyword| keyword.to_lowercase())
                .collect::<Vec<_>>();
            terms.iter().all(|term| {
                label.contains(term) || keywords.iter().any(|keyword| keyword.contains(term))
            })
        })
        .map(|(index, _)| index)
        .collect()
}

/// Move a selection through visible enabled items without wrapping.
///
/// The returned value is an original item index. Keeping that identity avoids
/// selecting a different entry when a query changes the filtered ordering.
pub fn navigate_searchable_items<T>(
    items: &[SearchableItem<T>],
    visible_indices: &[usize],
    selected_item: Option<usize>,
    navigation: SearchNavigation,
) -> Option<usize> {
    let selectable = visible_indices
        .iter()
        .copied()
        .filter(|index| items.get(*index).is_some_and(|item| item.enabled))
        .collect::<Vec<_>>();
    let first = selectable.first().copied()?;
    let last = selectable.last().copied()?;
    let current = selected_item.and_then(|selected| {
        selectable
            .iter()
            .position(|candidate| *candidate == selected)
    });

    match navigation {
        SearchNavigation::First => Some(first),
        SearchNavigation::Last => Some(last),
        SearchNavigation::Next => current.map_or(Some(first), |index| {
            selectable
                .get(index.saturating_add(1))
                .copied()
                .or(Some(last))
        }),
        SearchNavigation::Previous => current.map_or(Some(last), |index| {
            index
                .checked_sub(1)
                .and_then(|index| selectable.get(index).copied())
                .or(Some(first))
        }),
    }
}

/// Render a categorized searchable menu and return the selected value.
///
/// Callers can use the returned value directly, while the legacy callback API
/// below remains available for existing menus.
pub fn show_searchable_items<T: Clone>(
    ui: &mut Ui,
    id_source: &str,
    items: &[SearchableItem<T>],
) -> Option<T> {
    show_searchable_items_with_qa(ui, id_source, None, items)
}

/// Render a categorized searchable menu while exposing its actual search box
/// and item buttons to the loopback QA bridge. Input is still delivered via
/// egui; these identifiers do not provide a model-mutation shortcut.
pub fn show_searchable_items_with_qa<T: Clone>(
    ui: &mut Ui,
    id_source: &str,
    qa_search_id: Option<&str>,
    items: &[SearchableItem<T>],
) -> Option<T> {
    let id = ui.make_persistent_id(id_source);
    let mut state = ui.data_mut(|data| data.get_temp::<MenuState>(id).unwrap_or_default());

    let text_response = ui.add(TextEdit::singleline(&mut state.query).hint_text("Search..."));
    if let Some(qa_search_id) = qa_search_id {
        crate::qa::register_component_with_metadata(
            qa_search_id,
            "searchable_menu_query",
            text_response.rect,
            text_response.enabled(),
            Some(serde_json::json!({"action": "filter"})),
        );
    }
    if state.query.is_empty() && !ui.memory(|memory| memory.has_focus(text_response.id)) {
        text_response.request_focus();
    }

    if ui.input(|input| input.key_pressed(Key::Escape)) {
        ui.data_mut(|data| data.insert_temp(id, MenuState::default()));
        ui.close();
        return None;
    }

    ui.separator();

    let filtered = filter_searchable_items(items, &state.query);
    let displayed = categorized_indices(items, &filtered);
    if text_response.changed()
        || state.selected_item.is_none_or(|selected| {
            !displayed.contains(&selected) || items.get(selected).is_none_or(|item| !item.enabled)
        })
    {
        state.selected_item =
            navigate_searchable_items(items, &displayed, None, SearchNavigation::First);
    }

    for (key, navigation) in [
        (Key::ArrowDown, SearchNavigation::Next),
        (Key::ArrowUp, SearchNavigation::Previous),
        (Key::Home, SearchNavigation::First),
        (Key::End, SearchNavigation::Last),
    ] {
        if ui.input(|input| input.key_pressed(key)) {
            state.selected_item =
                navigate_searchable_items(items, &displayed, state.selected_item, navigation);
        }
    }

    let keyboard_selection = ui
        .input(|input| input.key_pressed(Key::Enter))
        .then_some(state.selected_item)
        .flatten();
    let mut clicked_selection = None;

    ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        if displayed.is_empty() {
            ui.label("No results");
            return;
        }

        let mut previous_category: Option<&str> = None;
        for index in displayed.iter().copied() {
            let Some(item) = items.get(index) else {
                continue;
            };
            let category = item.category.as_deref();
            if category != previous_category {
                if previous_category.is_some() {
                    ui.add_space(4.0);
                }
                if let Some(category) = category {
                    ui.label(RichText::new(category).small().strong().weak());
                } else if displayed
                    .iter()
                    .any(|item_index| items[*item_index].category.is_some())
                {
                    ui.label(RichText::new("Other").small().strong().weak());
                }
                previous_category = category;
            }

            let selected = state.selected_item == Some(index);
            let response = ui.add_enabled(
                item.enabled,
                egui::Button::selectable(selected, &item.label).frame(false),
            );
            if let Some(qa_id) = &item.qa_id {
                let mut metadata = item.qa_metadata.clone().unwrap_or_else(|| {
                    serde_json::json!({
                        "label": item.label,
                        "category": item.category,
                    })
                });
                if let Some(object) = metadata.as_object_mut() {
                    object
                        .entry("label")
                        .or_insert_with(|| serde_json::json!(item.label));
                    object
                        .entry("category")
                        .or_insert_with(|| serde_json::json!(item.category));
                }
                crate::qa::register_component_with_metadata(
                    qa_id,
                    "searchable_menu_item",
                    response.rect,
                    response.enabled(),
                    Some(metadata),
                );
            }
            if response.clicked() {
                clicked_selection = Some(index);
            }
            if selected {
                response.scroll_to_me(Some(egui::Align::Center));
            }
        }
    });

    let selection = clicked_selection.or(keyboard_selection).and_then(|index| {
        items
            .get(index)
            .filter(|item| item.enabled)
            .map(|item| item.value.clone())
    });

    if selection.is_some() {
        state = MenuState::default();
        ui.close();
    }
    ui.data_mut(|data| data.insert_temp(id, state));
    selection
}

/// Backward-compatible tuple + callback API.
pub fn show_searchable_menu<T: Clone + 'static>(
    ui: &mut Ui,
    id_source: &str,
    items: &[MenuItem<T>],
    mut on_select: impl FnMut(T),
) {
    let searchable_items = items
        .iter()
        .map(|(label, category, value)| {
            let mut item = SearchableItem::new(label.clone(), value.clone());
            item.category = category.clone();
            item
        })
        .collect::<Vec<_>>();

    if let Some(value) = show_searchable_items(ui, id_source, &searchable_items) {
        on_select(value);
    }
}

fn categorized_indices<T>(items: &[SearchableItem<T>], filtered: &[usize]) -> Vec<usize> {
    let mut categorized = BTreeMap::<&str, Vec<usize>>::new();
    let mut uncategorized = Vec::new();
    for index in filtered.iter().copied() {
        let Some(item) = items.get(index) else {
            continue;
        };
        if let Some(category) = item.category.as_deref() {
            categorized.entry(category).or_default().push(index);
        } else {
            uncategorized.push(index);
        }
    }

    categorized
        .into_values()
        .flatten()
        .chain(uncategorized)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<SearchableItem<u8>> {
        fn item(
            label: &str,
            category: Option<&str>,
            keywords: &[&str],
            enabled: bool,
            value: u8,
        ) -> SearchableItem<u8> {
            SearchableItem {
                label: label.to_string(),
                category: category.map(str::to_string),
                keywords: keywords
                    .iter()
                    .map(|keyword| (*keyword).to_string())
                    .collect(),
                enabled,
                qa_id: None,
                qa_metadata: None,
                value,
            }
        }

        vec![
            item(
                "Gaussian Blur",
                Some("Image Effect"),
                &["soften", "defocus"],
                true,
                1,
            ),
            item("Merge", Some("Compositing"), &["blend", "layers"], true, 2),
            item(
                "Unavailable Effect",
                Some("Image Effect"),
                &["offline"],
                false,
                3,
            ),
            item("Text", None, &["title", "caption"], true, 4),
        ]
    }

    #[test]
    fn filter_matches_case_insensitive_labels_and_keywords() {
        let items = fixture();
        assert_eq!(filter_searchable_items(&items, "gAuSs"), vec![0]);
        assert_eq!(filter_searchable_items(&items, "DEFOCUS"), vec![0]);
        assert_eq!(filter_searchable_items(&items, "blend LAYERS"), vec![1]);
        assert_eq!(filter_searchable_items(&items, "title"), vec![3]);
        assert!(filter_searchable_items(&items, "missing").is_empty());
    }

    #[test]
    fn categorized_order_is_stable_and_uncategorized_is_last() {
        let items = fixture();
        let filtered = filter_searchable_items(&items, "");
        assert_eq!(categorized_indices(&items, &filtered), vec![1, 0, 2, 3]);
    }

    #[test]
    fn keyboard_navigation_skips_disabled_items_and_clamps_at_ends() {
        let items = fixture();
        let visible = vec![0, 2, 3];

        assert_eq!(
            navigate_searchable_items(&items, &visible, None, SearchNavigation::First),
            Some(0)
        );
        assert_eq!(
            navigate_searchable_items(&items, &visible, Some(0), SearchNavigation::Next),
            Some(3)
        );
        assert_eq!(
            navigate_searchable_items(&items, &visible, Some(3), SearchNavigation::Next),
            Some(3)
        );
        assert_eq!(
            navigate_searchable_items(&items, &visible, Some(3), SearchNavigation::Previous),
            Some(0)
        );
        assert_eq!(
            navigate_searchable_items(&items, &visible, None, SearchNavigation::Last),
            Some(3)
        );
    }

    #[test]
    fn no_enabled_result_has_no_keyboard_selection() {
        let items = fixture();
        assert_eq!(
            navigate_searchable_items(&items, &[2], None, SearchNavigation::First),
            None
        );
    }
}
