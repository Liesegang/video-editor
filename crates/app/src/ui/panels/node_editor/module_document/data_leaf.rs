//! Canonical authored-value rows for typed Data leaf Nodes.

use egui_snarl::{InPin, OutPin, Snarl};
use library::model::property::Property;
use library::model::{Node, NodeContent};
use uuid::Uuid;

use super::property;
use super::viewer::ModuleNodeViewer;
use crate::ui::panels::node_editor::{
    node_editor_details_visible, PORT_LABEL_WIDTH, PORT_ROW_HEIGHT,
};
use crate::ui::property_metadata::node_property_definition;

pub(super) fn value_property(node: &Node) -> Option<(&str, &Property)> {
    if !matches!(node.content(), NodeContent::Data(_)) {
        return None;
    }
    node.properties()
        .get(library::model::project::connection::DATA_VALUE_PROPERTY)
        .map(|property| {
            (
                library::model::project::connection::DATA_VALUE_PROPERTY,
                property,
            )
        })
}

pub(super) const TIMELINE_AUTHORING: Result<(), &str> =
    Err("Data leaf values do not expose a Timeline input port");

pub(super) fn show_value(
    viewer: &mut ModuleNodeViewer<'_, '_>,
    node_id: egui_snarl::NodeId,
    _inputs: &[InPin],
    _outputs: &[OutPin],
    ui: &mut egui::Ui,
    snarl: &mut Snarl<Uuid>,
) {
    let Some(node) = viewer.node(snarl, node_id).cloned() else {
        return;
    };
    let Some((key, property)) = value_property(&node) else {
        return;
    };
    if !node_editor_details_visible(viewer.to_global.scaling) {
        ui.allocate_space(egui::vec2(PORT_LABEL_WIDTH + 80.0, PORT_ROW_HEIGHT));
        return;
    }
    let definition = node_property_definition(viewer.plugins, &node, key);
    let (response, action) = property::show_property_input(
        ui,
        viewer.plugins,
        &node,
        key,
        property,
        definition.as_ref(),
        false,
        viewer.property_context,
        viewer.canvas_transform,
        viewer.palette,
        key,
        TIMELINE_AUTHORING,
    );
    viewer.capture_response(&response);
    if let Some(action) = action {
        viewer.actions.push(action);
    }
}
