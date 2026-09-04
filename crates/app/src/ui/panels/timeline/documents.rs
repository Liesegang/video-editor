//! Opens the explicit editor document owned by a Timeline placement.
//!
//! Ordinary nested compositions open their Timeline. Only an explicitly
//! authored Module source opens the production Node Editor; Timeline
//! structure is never derived into a graph here.

use library::model::authoring::{AuthoringProject, SourceRef, TimelineItemId};

use crate::state::authoring::{AuthoringSelection, AuthoringUiState};
use crate::state::node_editor::{ModuleEditorHost, NodeEditorDocument};

pub(super) fn open_item(
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    item_id: TimelineItemId,
) {
    let Some(item) = project.items.get(&item_id) else {
        return;
    };
    match &item.source {
        SourceRef::Composition(instance) => {
            state.active_instance_path = state
                .active_instance_path
                .as_ref()
                .map(|path| path.nested(item.id));
            state.active_timeline_id = instance.timeline_id;
            if let Some(timeline) = project.timelines.get(&instance.timeline_id) {
                state
                    .timeline
                    .expanded_tracks
                    .extend(timeline.track_order.iter().copied());
            }
            state
                .selection
                .replace(AuthoringSelection::Timeline(instance.timeline_id));
            state.timeline.current_frame = 0;
            state.timeline.set_playing(false);
            state.preview.auto_fit = true;
        }
        SourceRef::Module(invocation) => {
            let Some(instance) = project.module_instances.get(&invocation.instance_id) else {
                state.error = Some("Node Clip instance is missing".to_string());
                return;
            };
            state
                .node_editor
                .request_document(NodeEditorDocument::ModuleDefinition {
                    definition_id: instance.definition_id,
                    host: ModuleEditorHost::NodeClip {
                        timeline_item_id: item.id,
                        instance_path: state.active_instance_path.clone(),
                        module_instance_id: instance.id,
                    },
                });
        }
        _ => {}
    }
}
