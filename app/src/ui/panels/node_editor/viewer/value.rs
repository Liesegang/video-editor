use super::ProjectNodeViewer;
use crate::ui::panels::node_editor::{
    INLINE_CONTROL_WIDTH, VALUE_NODE_CATEGORY_LABEL, bounded_non_selectable_label, property_label,
    value_operation_label,
};
use eframe::egui;
use library::model::ValueContent;

impl ProjectNodeViewer<'_> {
    pub(super) fn show_value_body(&self, ui: &mut egui::Ui, operation: ValueContent) {
        ui.horizontal(|ui| {
            property_label(ui, "Category");
            bounded_non_selectable_label(
                ui,
                VALUE_NODE_CATEGORY_LABEL,
                INLINE_CONTROL_WIDTH,
                egui::Align::LEFT,
            );
        });
        ui.horizontal(|ui| {
            property_label(ui, "Operation");
            bounded_non_selectable_label(
                ui,
                value_operation_label(operation),
                INLINE_CONTROL_WIDTH,
                egui::Align::LEFT,
            );
        });
    }
}
