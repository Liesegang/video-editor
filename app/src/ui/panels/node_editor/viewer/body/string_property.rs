use eframe::egui;
use library::model::Node;
use library::model::project::PortOwner;
use library::model::property::PropertyValue;
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    INLINE_CONTROL_WIDTH, NodeEdit, PORT_ROW_HEIGHT, continuous_response_finished,
    evaluate_node_property, node_property_time, property_label, render_node_property_issue,
};

use super::ProjectNodeViewer;

impl ProjectNodeViewer<'_> {
    pub(in crate::ui::panels::node_editor::viewer) fn edit_string_property(
        &mut self,
        ui: &mut egui::Ui,
        node_id: Uuid,
        node: &Node,
        key: &str,
        label: &str,
        fallback: &str,
    ) {
        let property_time = node_property_time(
            self.project,
            self.plugin_manager,
            node_id,
            self.current_time,
        );
        let evaluated = node.properties().get(key).map(|property| {
            evaluate_node_property(
                self.project,
                self.plugin_manager,
                node_id,
                property,
                property_time,
            )
        });
        let mut value = evaluated
            .as_ref()
            .and_then(|evaluated| evaluated.value())
            .and_then(|value| value.get_as::<String>())
            .unwrap_or_else(|| fallback.to_string());
        ui.horizontal(|ui| {
            property_label(ui, label);
            if let Some(issue) = evaluated.as_ref().and_then(|evaluated| evaluated.issue()) {
                render_node_property_issue(ui, node_id, key, issue);
            }
            let response = ui.add_sized(
                [INLINE_CONTROL_WIDTH, PORT_ROW_HEIGHT],
                egui::TextEdit::singleline(&mut value),
            );
            self.record_body_response(&response);
            let finished = continuous_response_finished(ui, &response);
            let edit = response.changed().then(|| NodeEdit::SetProperty {
                owner: PortOwner::Node(node_id),
                key: key.to_string(),
                time: property_time,
                value: PropertyValue::String(value),
            });
            self.queue_continuous_edit(PortOwner::Node(node_id), key.to_string(), edit, finished);
        });
    }
}
