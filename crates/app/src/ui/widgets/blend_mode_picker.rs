//! Shared searchable picker for the authoritative BlendMode catalog.

use egui_phosphor::regular as icons;
use library::model::BlendMode;

use super::searchable_context_menu::{
    searchable_menu_button, show_searchable_items_with_qa, SearchableItem,
};

/// Shows the complete categorized Blend catalog and returns a newly selected
/// mode. Callers remain responsible for applying it through their owning
/// editor service; this widget never keeps a duplicate value model.
pub(crate) fn blend_mode_picker(
    ui: &mut egui::Ui,
    id_source: impl std::fmt::Display,
    selected: BlendMode,
) -> Option<BlendMode> {
    let id_source = format!("blend_mode_picker:{id_source}");
    let query_id = format!("{id_source}.query");
    let items = BlendMode::ALL
        .into_iter()
        .map(|mode| {
            let mut item = SearchableItem::new(mode.label(), mode);
            item.category = Some(mode.group().label().to_string());
            item.keywords = vec![mode.qa_key().to_string(), mode.group().qa_key().to_string()];
            item.enabled = mode != selected;
            item.qa_id = Some(format!("{id_source}.{}", mode.qa_key()));
            item.qa_metadata = Some(serde_json::json!({
                "blend_mode": mode.qa_key(),
                "group": mode.group().qa_key(),
            }));
            item
        })
        .collect::<Vec<_>>();

    let menu = searchable_menu_button(
        ui,
        format!("{}  {}", icons::STACK, selected.label()),
        |ui| {
            ui.set_min_width(280.0);
            ui.set_min_height(240.0_f32.min(ui.available_height().max(0.0)));
            show_searchable_items_with_qa(ui, &id_source, Some(&query_id), &items)
        },
    );
    crate::qa::register_component_with_metadata(
        id_source,
        "blend_mode_picker",
        menu.response.rect,
        menu.response.enabled(),
        Some(serde_json::json!({
            "selected": selected.qa_key(),
            "catalog_size": BlendMode::ALL.len(),
            "searchable": true,
        })),
    );
    menu.inner.flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_uses_the_complete_authoritative_catalog() {
        assert_eq!(BlendMode::ALL.len(), 29);
        assert_eq!(BlendMode::LinearDodge.group().label(), "Lighten");
    }
}
