use egui::{Ui, Key, Color32, RichText, ScrollArea, TextEdit, Id};
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct MenuState {
    query: String,
    selected_index: usize,
    last_update_time: f64,
}

/// A reusable component for a searchable context menu.
/// * `id_source`: Unique identifier for state persistence (useful within the same parent memory).
/// * `items`: List of (Label, Category, Value) pairs. Category is optional.
/// * `on_select`: Callback when an item is selected.
pub fn show_searchable_menu<T: Clone + 'static>(
    ui: &mut Ui,
    id_source: &str,
    items: &[(String, Option<String>, T)],
    mut on_select: impl FnMut(T),
) {
    let id = ui.make_persistent_id(id_source);
    let current_time = ui.input(|i| i.time);
    
    let mut state = ui.data_mut(|d| d.get_temp::<MenuState>(id).unwrap_or_default());

    // Check if the menu has been re-opened (gap in time)
    if current_time - state.last_update_time > 0.2 {
        state = MenuState::default();
    }
    state.last_update_time = current_time;

    // Search Input
    let text_res = ui.add(TextEdit::singleline(&mut state.query).hint_text("Search..."));
    
    if state.query.is_empty() && !ui.memory(|m| m.has_focus(text_res.id)) {
        text_res.request_focus();
    }

    ui.separator();

    let is_query_empty = state.query.is_empty();
    
    // CASE 1: Empty Query -> Render Categories as Submenus
    if is_query_empty {
        // Group items by category
        // Use BTreeMap to keep categories sorted
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
             // Render Categories
             for (category, cat_items) in categorized {
                 ui.menu_button(category, |ui| {
                     for item in cat_items {
                         if ui.button(&item.0).clicked() {
                             on_select(item.2.clone());
                             ui.close_menu();
                         }
                     }
                 });
             }

             if !categorized.is_empty() && !uncategorized.is_empty() {
                 ui.separator();
             }

             // Render Uncategorized
             for item in uncategorized {
                 if ui.button(&item.0).clicked() {
                     on_select(item.2.clone());
                     ui.close_menu();
                 }
             }
        });
    } 
    // CASE 2: Search Query Active -> Flat List with Keyboard Navigation
    else {
        let query_lower = state.query.to_lowercase();
        let filtered: Vec<&(String, Option<String>, T)> = items.iter()
            .filter(|(label, _, _)| label.to_lowercase().contains(&query_lower))
            .collect();

        // Keyboard Navigation (Only valid for flat list mode)
        if !filtered.is_empty() {
             if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                state.selected_index = (state.selected_index + 1).min(filtered.len().saturating_sub(1));
            }
            if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                state.selected_index = state.selected_index.saturating_sub(1);
            }
            if ui.input(|i| i.key_pressed(Key::Enter)) {
                if let Some(item) = filtered.get(state.selected_index) {
                    on_select(item.2.clone());
                    ui.close_menu();
                }
            }
        }
        
        // Reset selection if query changed
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
                    RichText::new(label).strong().background_color(ui.visuals().selection.bg_fill).color(ui.visuals().selection.stroke.color)
                } else {
                    RichText::new(label)
                };
                
                let response = ui.selectable_label(selected, rich_label);
                if response.clicked() {
                    on_select(value.clone());
                    ui.close_menu();
                }
                 if selected {
                    response.scroll_to_me(Some(egui::Align::Center));
                }
            }
        });
    }

    ui.data_mut(|d| d.insert_temp(id, state));
}
