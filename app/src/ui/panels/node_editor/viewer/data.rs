use eframe::egui;
use library::model::project::connection::DATA_VALUE_PROPERTY;
use library::model::property::{Property, PropertyValue};
use uuid::Uuid;

use super::{ProjectNodeViewer, property_value_summary};
use crate::ui::panels::node_editor::property_label;

impl ProjectNodeViewer<'_> {
    pub(super) fn show_data_body(&self, ui: &mut egui::Ui, node_id: Uuid) {
        let Some(node) = self.project.get_node(node_id) else {
            return;
        };
        ui.horizontal(|ui| {
            property_label(ui, "Value");
            let response = match node
                .properties()
                .get(DATA_VALUE_PROPERTY)
                .and_then(Property::value)
            {
                Some(PropertyValue::ColorValue(value)) => {
                    property_value_summary::render_color(ui, value)
                }
                Some(PropertyValue::Path(value)) => property_value_summary::render_path(ui, value),
                _ => ui.colored_label(ui.visuals().error_fg_color, "Invalid canonical value"),
            };
            response.on_hover_text("Edit losslessly in the Inspector");
        });
    }
}
