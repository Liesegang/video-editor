use super::ProjectNodeViewer;
use crate::ui::panels::node_editor::*;
use eframe::egui::{self, Color32};
use library::model::NodeContent;
use uuid::Uuid;

impl ProjectNodeViewer<'_> {
    pub(super) fn show_native_body(&self, ui: &mut egui::Ui, node_id: Uuid) {
        let Some(NodeContent::NativeOperation(operation)) = self
            .project
            .get_node(node_id)
            .map(library::model::Node::content)
        else {
            return;
        };
        let descriptor = library::model::native_node_descriptor(&operation.catalog_id);
        let category = descriptor.map_or("Unknown", |item| item.category());
        ui.horizontal(|ui| {
            property_label(ui, "Category");
            bounded_non_selectable_label(ui, category, INLINE_CONTROL_WIDTH, egui::Align::LEFT);
        });
        let diagnostic = descriptor
            .and_then(|item| item.runtime_diagnostic())
            .unwrap_or_else(|| {
                format!(
                    "Unknown native catalog id '{}'; evaluation produces No Output",
                    operation.catalog_id
                )
            });
        let response = non_selectable_label(
            ui,
            egui::RichText::new(&diagnostic)
                .small()
                .color(Color32::from_rgb(238, 170, 92)),
        );
        crate::qa::register_component_with_metadata(
            format!("node_editor.native_diagnostic:{node_id}"),
            "node_editor_native_runtime_diagnostic",
            clipped_qa_rect(*self.to_global * response.rect, *self.canvas_clip),
            false,
            Some(serde_json::json!({
                "node_id": node_id,
                "catalog_id": operation.catalog_id,
                "runtime_status": descriptor.map(|item| item.runtime_status().key()),
                "output": "no_output",
                "message": diagnostic,
            })),
        );
    }
}
