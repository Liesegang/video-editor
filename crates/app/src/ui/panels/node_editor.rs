//! The production Node Editor surface.
//!
//! A document supplies processing nodes and applies edit intents; this module
//! owns the shared visual language, navigation policy, and Snarl interaction
//! surface. Timeline items are deliberately outside this module: only an
//! explicitly opened Node Clip supplies a `ModuleDefinition` document.

mod canvas;
mod components;
mod module_document;

pub use module_document::node_editor_panel;

use library::editor::TimelineEditorService;
use library::model::authoring::{AuthoringProject, TransitionId};

use crate::state::authoring::AuthoringUiState;
use crate::state::node_editor::{ModuleEditorHost, NodeEditorDocument};

use canvas::{
    node_editor_details_visible, node_editor_navigation_config,
    node_editor_port_interactions_enabled, node_editor_snarl_style_for,
    paint_node_editor_canvas_grid, NODE_EDITOR_MAX_SCALE, NODE_EDITOR_MIN_SCALE,
};
use components::{
    measured_label_width, node_icon_for_node, node_palette_for_node, pin_info, property_label,
};

// Physical dimensions from the original production Node Editor. Keeping them
// at the surface boundary makes Snarl layout, property controls, and hit boxes
// derive from one contract.
const PORT_SOCKET_SIZE: f32 = 13.0;
const NODE_HEADER_WIDTH: f32 = 190.0;
const PORT_LABEL_WIDTH: f32 = 96.0;
const PORT_ROW_HEIGHT: f32 = 22.0;
const PROPERTY_LABEL_WIDTH: f32 = 58.0;

/// Opens the production Node Editor for one Transition processor. Built-in
/// processors are promoted once to a private Module; Timeline placement and
/// clip ownership remain outside the graph.
pub(crate) fn open_transition_document(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    transition_id: TransitionId,
) -> Result<(), library::LibraryError> {
    let transition = project.transitions.get(&transition_id).ok_or_else(|| {
        library::LibraryError::Validation(format!("Missing Transition {transition_id}"))
    })?;
    let (definition_id, instance_id, promoted) =
        if let Some(module) = transition.processor.module_processor() {
            let instance = project
                .module_instances
                .get(&module.instance_id)
                .ok_or_else(|| {
                    library::LibraryError::Validation(format!(
                        "Transition {transition_id} has no Module instance {}",
                        module.instance_id
                    ))
                })?;
            (instance.definition_id, instance.id, false)
        } else {
            let from_name = project
                .items
                .get(&transition.from_item_id)
                .map_or("A", |item| item.name.as_str());
            let to_name = project
                .items
                .get(&transition.to_item_id)
                .map_or("B", |item| item.name.as_str());
            let (definition_id, instance_id, _) = service.promote_transition_to_module(
                transition_id,
                format!("{from_name} → {to_name} Transition"),
            )?;
            (definition_id, instance_id, true)
        };

    state
        .node_editor
        .request_document(NodeEditorDocument::ModuleDefinition {
            definition_id,
            host: ModuleEditorHost::Transition {
                transition_id,
                instance_path: state.active_instance_path.clone(),
                module_instance_id: instance_id,
            },
        });
    state.status = if promoted {
        "Promoted Transition logic to a private Node Module".to_string()
    } else {
        "Opened Transition logic in Node Editor".to_string()
    };
    Ok(())
}
