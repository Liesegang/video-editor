pub mod adapter;

use crate::state::context::PanelContext;

use adapter::{VideoEditorDataSource, VideoEditorMutator};
use egui_node_editor::{NodeEditorTheme, NodeEditorWidget};
use library::model::project::Node;

/// Main node editor panel function.
pub fn node_editor_panel(ui: &mut egui::Ui, ctx: &mut PanelContext) {
    let project = ctx.project.clone();
    let Ok(proj_read) = project.read() else {
        ui.label("Failed to read project");
        return;
    };

    let plugin_manager = ctx.project_service.get_plugin_manager();
    let state = &mut ctx.editor_context.node_editor_state;

    // Resolve current container from selected composition (use composition ID as container)
    if state.current_container.is_none() {
        if let Some(comp) = ctx
            .editor_context
            .selection
            .composition_id
            .and_then(|id| proj_read.compositions.iter().find(|c| c.id == id))
        {
            state.current_container = Some(comp.id);
        }
    }

    // Convert current_time (f32 seconds) to frame number
    let fps = proj_read
        .compositions
        .first()
        .map(|c| c.fps)
        .unwrap_or(30.0);
    let current_frame = (ctx.editor_context.timeline.current_time * fps as f32) as u64;

    let source = VideoEditorDataSource {
        project: &proj_read,
        plugin_manager: &plugin_manager,
        current_frame,
    };

    let theme = NodeEditorTheme::default();
    let mut widget = NodeEditorWidget::new(state, &theme);

    // Create a temporary mutator just for get_available_node_types (used by context menu)
    let temp_mutator = adapter::ReadOnlyMutator {
        project_service: &*ctx.project_service,
    };

    let pending = widget.show(ui, &source, &temp_mutator);

    // Sync selected node to inspector
    if let Some(selected_id) = pending.selected_node {
        // Find the parent track for this node (inspector needs track_id)
        let track_id = find_parent_track(&proj_read, selected_id);
        ctx.editor_context.selection.selected_entities.clear();
        ctx.editor_context
            .selection
            .selected_entities
            .insert(selected_id);
        ctx.editor_context.selection.last_selected_entity_id = Some(selected_id);
        ctx.editor_context.selection.last_selected_track_id = track_id;
    }

    // Drop read lock before applying mutations
    drop(proj_read);
    drop(plugin_manager);

    if !pending.is_empty() {
        let mut mutator = VideoEditorMutator {
            project_service: ctx.project_service,
        };
        pending.apply(&mut mutator);
    }
}

/// Find the parent track ID for a given node.
fn find_parent_track(
    project: &library::model::project::project::Project,
    node_id: uuid::Uuid,
) -> Option<uuid::Uuid> {
    for (id, node) in project.nodes.iter() {
        if let Node::Track(track) = node {
            if track.child_ids.contains(&node_id) {
                return Some(*id);
            }
        }
    }
    None
}
