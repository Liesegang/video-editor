use super::ProjectNodeViewer;
use crate::ui::panels::node_editor::{
    bounded_non_selectable_label, property_label, INLINE_CONTROL_WIDTH,
};
use eframe::egui;
use library::model::ListContent;

impl ProjectNodeViewer<'_> {
    pub(super) fn show_list_body(&self, ui: &mut egui::Ui, operation: ListContent) {
        ui.horizontal(|ui| {
            property_label(ui, "Category");
            bounded_non_selectable_label(ui, "Logic", INLINE_CONTROL_WIDTH, egui::Align::LEFT);
        });
        ui.horizontal(|ui| {
            property_label(ui, "Operation");
            bounded_non_selectable_label(
                ui,
                operation.label(),
                INLINE_CONTROL_WIDTH,
                egui::Align::LEFT,
            );
        });
    }
}
