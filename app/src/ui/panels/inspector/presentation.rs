use egui::Ui;
use library::model::project::{PortOwner, Project};
use uuid::Uuid;

use crate::state::context::EditorContext;
use crate::ui::panels::time_context::{time_source_state, TimeSourcePresentation, TimeSourceState};

#[derive(Clone, Debug)]
pub(super) struct NodeTimeSource {
    state: TimeSourceState,
    presentation: TimeSourcePresentation,
}

pub(super) fn resolve_node_time_source(project: &Project, node_id: Uuid) -> Option<NodeTimeSource> {
    let state = time_source_state(project, PortOwner::Node(node_id))?;
    let presentation = state.presentation(project);
    Some(NodeTimeSource {
        state,
        presentation,
    })
}

pub(super) fn render_node_time_source(ui: &mut Ui, node_id: Uuid, source: &NodeTimeSource) {
    let row = ui.horizontal(|ui| {
        ui.label("Time source:");
        ui.add(
            egui::Label::new(
                egui::RichText::new(&source.presentation.label)
                    .small()
                    .weak(),
            )
            .selectable(false),
        )
        .on_hover_text(&source.presentation.tooltip)
    });
    let mut metadata = source.state.qa_metadata(PortOwner::Node(node_id));
    if let Some(metadata) = metadata.as_object_mut() {
        metadata.insert(
            "label".to_string(),
            source.presentation.label.clone().into(),
        );
        metadata.insert(
            "tooltip".to_string(),
            source.presentation.tooltip.clone().into(),
        );
        metadata.insert("surface".to_string(), "inspector".into());
    }
    crate::qa::register_component_with_metadata(
        format!("inspector.time_source.node:{node_id}"),
        "inspector_time_source",
        row.inner.rect,
        true,
        Some(metadata),
    );
}

pub(super) fn render_multi_selection_notice(ui: &mut Ui, editor_context: &EditorContext) {
    let selected_count = editor_context.selection.len();
    if selected_count <= 1 {
        return;
    }
    ui.heading(format!("{selected_count} Items Selected"));
    ui.label(
        egui::RichText::new("(Editing Primary Item)")
            .italics()
            .small(),
    );
    ui.separator();
}
