use egui::{Key, RichText, ScrollArea, TextEdit, Ui};
use std::collections::BTreeMap;

use super::types::SearchableItem;

#[derive(Clone, Default)]
struct MenuState {
    query: String,
    selected_index: usize,
}

/// Render a searchable, categorized context menu.
/// Returns `Some(action)` if the user selected an item.
pub fn show_searchable_context_menu<A: Clone + 'static>(
    ui: &mut Ui,
    id_source: &str,
    items: &[SearchableItem<A>],
) -> Option<A> {
    let id = ui.make_persistent_id(id_source);
    let mut state = ui.data_mut(|d| d.get_temp::<MenuState>(id).unwrap_or_default());
    let mut selected_action: Option<A> = None;

    // Search Input
    let text_res = ui.add(TextEdit::singleline(&mut state.query).hint_text("Search..."));
    if state.query.is_empty() && !ui.memory(|m| m.has_focus(text_res.id)) {
        text_res.request_focus();
    }

    // Handle Escape
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        state = MenuState::default();
        ui.close();
        ui.data_mut(|d| d.insert_temp(id, state));
        return None;
    }

    ui.separator();

    if state.query.is_empty() {
        render_categorized_view(ui, items, &mut state, &mut selected_action);
    } else {
        render_flat_view(ui, items, &mut state, &text_res, &mut selected_action);
    }

    // Clean up state on selection
    if selected_action.is_some() {
        state.query.clear();
        state.selected_index = 0;
        ui.close();
    }

    ui.data_mut(|d| d.insert_temp(id, state));
    selected_action
}

fn render_categorized_view<A: Clone>(
    ui: &mut Ui,
    items: &[SearchableItem<A>],
    _state: &mut MenuState,
    selected: &mut Option<A>,
) {
    let mut categorized: BTreeMap<String, Vec<&SearchableItem<A>>> = BTreeMap::new();
    let mut uncategorized: Vec<&SearchableItem<A>> = Vec::new();

    for item in items {
        if let Some(cat) = &item.category {
            categorized.entry(cat.clone()).or_default().push(item);
        } else {
            uncategorized.push(item);
        }
    }

    ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        for (category, cat_items) in &categorized {
            ui.menu_button(category, |ui| {
                for item in cat_items {
                    let text = match &item.icon {
                        Some(icon) => format!("{} {}", icon, item.label),
                        None => item.label.clone(),
                    };
                    let response = ui.add_enabled(item.enabled, egui::Button::new(&text));
                    if response.clicked() {
                        *selected = Some(item.action.clone());
                    }
                }
            });
        }

        if !categorized.is_empty() && !uncategorized.is_empty() {
            ui.separator();
        }

        for item in uncategorized {
            let text = match &item.icon {
                Some(icon) => format!("{} {}", icon, item.label),
                None => item.label.clone(),
            };
            let response = ui.add_enabled(item.enabled, egui::Button::new(&text));
            if response.clicked() {
                *selected = Some(item.action.clone());
            }
        }
    });
}

fn render_flat_view<A: Clone>(
    ui: &mut Ui,
    items: &[SearchableItem<A>],
    state: &mut MenuState,
    text_res: &egui::Response,
    selected: &mut Option<A>,
) {
    let query_lower = state.query.to_lowercase();
    let filtered: Vec<&SearchableItem<A>> = items
        .iter()
        .filter(|item| {
            item.label.to_lowercase().contains(&query_lower)
                || item
                    .keywords
                    .iter()
                    .any(|kw| kw.to_lowercase().contains(&query_lower))
        })
        .collect();

    // Keyboard Navigation
    if !filtered.is_empty() {
        if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
            state.selected_index = (state.selected_index + 1).min(filtered.len().saturating_sub(1));
        }
        if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
            state.selected_index = state.selected_index.saturating_sub(1);
        }
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            if let Some(item) = filtered.get(state.selected_index) {
                if item.enabled {
                    *selected = Some(item.action.clone());
                }
            }
        }
    }

    if text_res.changed() {
        state.selected_index = 0;
    }

    ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        if filtered.is_empty() {
            ui.label("No results");
        }

        for (i, item) in filtered.iter().enumerate() {
            let is_selected = i == state.selected_index;
            let rich_label = if is_selected {
                RichText::new(&item.label)
                    .strong()
                    .background_color(ui.visuals().selection.bg_fill)
                    .color(ui.visuals().selection.stroke.color)
            } else {
                RichText::new(&item.label)
            };

            let response = ui.selectable_label(is_selected, rich_label);
            if response.clicked() && item.enabled {
                *selected = Some(item.action.clone());
            }
            if is_selected {
                response.scroll_to_me(Some(egui::Align::Center));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    fn sample_items() -> Vec<SearchableItem<i32>> {
        vec![
            SearchableItem {
                label: "Blur".into(),
                category: Some("Effects".into()),
                icon: None,
                action: 1,
                enabled: true,
                keywords: vec![],
            },
            SearchableItem {
                label: "Glow".into(),
                category: Some("Effects".into()),
                icon: None,
                action: 2,
                enabled: true,
                keywords: vec![],
            },
            SearchableItem {
                label: "Fill".into(),
                category: Some("Styles".into()),
                icon: None,
                action: 3,
                enabled: true,
                keywords: vec![],
            },
            SearchableItem {
                label: "Stroke".into(),
                category: Some("Styles".into()),
                icon: None,
                action: 4,
                enabled: true,
                keywords: vec![],
            },
            SearchableItem {
                label: "Transform".into(),
                category: None,
                icon: None,
                action: 5,
                enabled: true,
                keywords: vec![],
            },
        ]
    }

    #[test]
    fn categorized_view_shows_categories() {
        let items = sample_items();
        let harness = Harness::builder()
            .with_size(egui::vec2(300.0, 400.0))
            .build_ui(|ui| {
                show_searchable_context_menu(ui, "test_cat", &items);
            });
        assert!(harness.query_by_label("Effects").is_some());
        assert!(harness.query_by_label("Styles").is_some());
        assert!(harness.query_by_label("Transform").is_some());
    }

    #[test]
    fn search_filters_by_label() {
        let items = sample_items();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(300.0, 400.0))
            .build_ui(|ui| {
                let id = ui.make_persistent_id("test_filter");
                ui.data_mut(|d| {
                    d.insert_temp(
                        id,
                        MenuState {
                            query: "blur".to_string(),
                            selected_index: 0,
                        },
                    );
                });
                show_searchable_context_menu(ui, "test_filter", &items);
            });
        harness.run_steps(2);
        assert!(harness.query_by_label("Blur").is_some());
        assert!(harness.query_by_label("Fill").is_none());
    }

    #[test]
    fn search_matches_keywords() {
        let items = vec![SearchableItem {
            label: "Gaussian Blur".into(),
            category: None,
            icon: None,
            action: 1,
            enabled: true,
            keywords: vec!["smooth".into(), "soften".into()],
        }];
        let mut harness = Harness::builder()
            .with_size(egui::vec2(300.0, 400.0))
            .build_ui(|ui| {
                let id = ui.make_persistent_id("test_kw");
                ui.data_mut(|d| {
                    d.insert_temp(
                        id,
                        MenuState {
                            query: "soften".to_string(),
                            selected_index: 0,
                        },
                    );
                });
                show_searchable_context_menu(ui, "test_kw", &items);
            });
        harness.run_steps(2);
        assert!(harness.query_by_label("Gaussian Blur").is_some());
    }

    #[test]
    fn search_is_case_insensitive() {
        let items = sample_items();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(300.0, 400.0))
            .build_ui(|ui| {
                let id = ui.make_persistent_id("test_case");
                ui.data_mut(|d| {
                    d.insert_temp(
                        id,
                        MenuState {
                            query: "BLUR".to_string(),
                            selected_index: 0,
                        },
                    );
                });
                show_searchable_context_menu(ui, "test_case", &items);
            });
        harness.run_steps(2);
        assert!(harness.query_by_label("Blur").is_some());
    }

    #[test]
    fn no_results_shows_label() {
        let items = sample_items();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(300.0, 400.0))
            .build_ui(|ui| {
                let id = ui.make_persistent_id("test_noresults");
                ui.data_mut(|d| {
                    d.insert_temp(
                        id,
                        MenuState {
                            query: "zzzzz".to_string(),
                            selected_index: 0,
                        },
                    );
                });
                show_searchable_context_menu(ui, "test_noresults", &items);
            });
        harness.run_steps(2);
        assert!(harness.query_by_label("No results").is_some());
    }

    #[test]
    fn clicking_uncategorized_item_returns_action() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let items = sample_items();
        let results: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let r = results.clone();

        let mut harness = Harness::builder()
            .with_size(egui::vec2(300.0, 400.0))
            .build_ui(move |ui| {
                let r2 = r.clone();
                if let Some(action) = show_searchable_context_menu(ui, "test_click", &items) {
                    r2.borrow_mut().push(action);
                }
            });

        harness.get_by_label("Transform").click();
        harness.run_steps(2);
        assert_eq!(*results.borrow(), vec![5]);
    }

    #[test]
    fn empty_items_shows_no_content() {
        let items: Vec<SearchableItem<i32>> = vec![];
        let harness = Harness::builder()
            .with_size(egui::vec2(300.0, 400.0))
            .build_ui(|ui| {
                show_searchable_context_menu(ui, "test_empty", &items);
            });
        assert!(harness.query_by_label("Effects").is_none());
        assert!(harness.query_by_label("Blur").is_none());
    }
}
