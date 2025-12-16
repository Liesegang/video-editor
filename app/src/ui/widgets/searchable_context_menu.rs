use egui::{Id, Key, RichText, ScrollArea, TextEdit, Ui};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct MenuState {
    query: String,
    selected_index: usize,
}

/// A reusable component for a searchable context menu.
pub fn show_searchable_menu<T: Clone + 'static>(
    ui: &mut Ui,
    id_source: &str,
    items: &[(String, Option<String>, T)],
    mut on_select: impl FnMut(T),
) {
    let id = ui.make_persistent_id(id_source);
    let mut state = ui.data_mut(|d| d.get_temp::<MenuState>(id).unwrap_or_default());

    // Helper closure to handle selection finalization
    // We pass this closure to the view renderers
    let mut finalize_selection = |value: T, state: &mut MenuState, ui: &mut Ui| {
        on_select(value);
        state.query.clear();
        state.selected_index = 0;
        ui.close_menu();
    };

    // Search Input
    let text_res = ui.add(TextEdit::singleline(&mut state.query).hint_text("Search..."));
    if state.query.is_empty() && !ui.memory(|m| m.has_focus(text_res.id)) {
        text_res.request_focus();
    }

    // Handle Escape
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        state = MenuState::default();
        ui.close_menu();
        ui.data_mut(|d| d.insert_temp(id, state));
        return;
    }

    ui.separator();

    let is_query_empty = state.query.is_empty();

    if is_query_empty {
        render_categorized_view(ui, items, &mut state, &mut finalize_selection);
    } else {
        render_flat_view(ui, items, &mut state, &text_res, &mut finalize_selection);
    }

    ui.data_mut(|d| d.insert_temp(id, state));
}

fn render_categorized_view<T: Clone>(
    ui: &mut Ui,
    items: &[(String, Option<String>, T)],
    state: &mut MenuState,
    finalize: &mut impl FnMut(T, &mut MenuState, &mut Ui),
) {
    let mut categorized: BTreeMap<String, Vec<&(String, Option<String>, T)>> = BTreeMap::new();
    let mut uncategorized: Vec<&(String, Option<String>, T)> = Vec::new();

    for item in items {
        if let Some(cat) = &item.1 {
            categorized.entry(cat.clone()).or_default().push(item);
        } else {
            uncategorized.push(item);
        }
    }

    ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        for (category, cat_items) in &categorized {
            ui.menu_button(category, |ui| {
                for item in cat_items {
                    if ui.button(&item.0).clicked() {
                        finalize(item.2.clone(), state, ui);
                    }
                }
            });
        }

        if !categorized.is_empty() && !uncategorized.is_empty() {
            ui.separator();
        }

        for item in uncategorized {
            if ui.button(&item.0).clicked() {
                finalize(item.2.clone(), state, ui);
            }
        }
    });
}

fn render_flat_view<T: Clone>(
    ui: &mut Ui,
    items: &[(String, Option<String>, T)],
    state: &mut MenuState,
    text_res: &egui::Response,
    finalize: &mut impl FnMut(T, &mut MenuState, &mut Ui),
) {
    let query_lower = state.query.to_lowercase();
    let filtered: Vec<&(String, Option<String>, T)> = items
        .iter()
        .filter(|(label, _, _)| label.to_lowercase().contains(&query_lower))
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
                finalize(item.2.clone(), state, ui);
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

        for (i, (label, _, value)) in filtered.iter().enumerate() {
            let selected = i == state.selected_index;
            let rich_label = if selected {
                RichText::new(label)
                    .strong()
                    .background_color(ui.visuals().selection.bg_fill)
                    .color(ui.visuals().selection.stroke.color)
            } else {
                RichText::new(label)
            };

            let response = ui.selectable_label(selected, rich_label);
            if response.clicked() {
                finalize(value.clone(), state, ui);
            }
            if selected {
                response.scroll_to_me(Some(egui::Align::Center));
            }
        }
    });
}
