use super::ProjectNodeViewer;
use crate::ui::panels::node_editor::{
    PORT_ROW_HEIGHT, PinDefinition, bounded_non_selectable_label, clipped_qa_rect, qa_rect_metadata,
};
#[cfg(test)]
use crate::ui::panels::node_editor::{capture_test_metadata, capture_test_rect};
use crate::ui::panels::time_context::TimeSourceState;
use eframe::egui::{self, Color32};
use library::model::project::PortOwner;
use uuid::Uuid;

impl ProjectNodeViewer<'_> {
    /// Render the derived Time source separately from authored property
    /// controls. Time is graph context, so this row is informative and never
    /// becomes another editable model.
    pub(super) fn show_node_time_source_row(
        &self,
        ui: &mut egui::Ui,
        node_id: Uuid,
        definition: &PinDefinition,
        connected: bool,
        state: &TimeSourceState,
    ) {
        let presentation = state.presentation(self.project);
        let row = ui.horizontal(|ui| {
            bounded_non_selectable_label(ui, definition.name.clone(), 72.0, egui::Align::LEFT);
            ui.add_sized(
                [164.0, PORT_ROW_HEIGHT - 2.0],
                egui::Label::new(
                    egui::RichText::new(&presentation.label)
                        .small()
                        .color(Color32::from_gray(145)),
                )
                .selectable(false)
                .truncate(),
            )
            .on_hover_text(&presentation.tooltip)
        });

        let component_id = format!("node_editor.time_source.node:{node_id}");
        let unclipped_rect = *self.to_global * row.inner.rect;
        let rect = clipped_qa_rect(unclipped_rect, *self.canvas_clip);
        let mut metadata = state.qa_metadata(PortOwner::Node(node_id));
        if let Some(metadata) = metadata.as_object_mut() {
            metadata.insert("label".to_string(), presentation.label.into());
            metadata.insert("tooltip".to_string(), presentation.tooltip.into());
            metadata.insert("connected".to_string(), connected.into());
            metadata.insert(
                "unclipped_rect".to_string(),
                qa_rect_metadata(unclipped_rect),
            );
            metadata.insert("visible_in_canvas".to_string(), rect.is_positive().into());
        }
        #[cfg(test)]
        {
            capture_test_rect(&component_id, rect);
            capture_test_metadata(&component_id, &metadata);
        }
        crate::qa::register_component_with_metadata(
            component_id,
            "node_time_source",
            rect,
            true,
            Some(metadata),
        );
    }
}
