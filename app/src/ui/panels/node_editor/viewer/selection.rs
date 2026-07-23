use crate::ui::panels::node_editor::{
    container_inactive, container_visual_style, graph_item_inactive,
    native_variadic_merge_for_node, node_palette, ContainerVisual, ContainerVisualStyle, GraphItem,
};
use library::model::project::PortOwner;
use library::model::Project;
use node_editor_ui::{Editor, NodeVisualStyle};
use uuid::Uuid;

use super::ProjectNodeViewer;

pub(super) fn is_physical_merge_node(project: &Project, node_id: Uuid) -> bool {
    native_variadic_merge_for_node(project, node_id).is_some()
}

pub(super) struct NodeSelectionPresentation {
    pub(super) selected: bool,
    pub(super) inactive: bool,
    pub(super) visual: NodeVisualStyle,
}

pub(super) struct ContainerSelectionPresentation {
    pub(super) selected: bool,
    pub(super) visual: Option<ContainerVisualStyle>,
}

pub(super) fn container_selection_presentation(
    project: &Project,
    containers: &[ContainerVisual],
    selected_owners: &[PortOwner],
    owner: PortOwner,
    current_time: f64,
    scale: f32,
) -> ContainerSelectionPresentation {
    let selected = selected_owners.contains(&owner);
    let visual = containers
        .iter()
        .find(|container| container.owner == owner)
        .map(|container| {
            container_visual_style(
                container.kind,
                container_inactive(project, owner, current_time),
                selected,
                scale,
            )
        });
    ContainerSelectionPresentation { selected, visual }
}

pub(super) fn node_selection_presentation(
    project: &Project,
    selected_node_ids: &[Uuid],
    node_id: Uuid,
    current_time: f64,
    scale: f32,
) -> NodeSelectionPresentation {
    let selected = selected_node_ids.binary_search(&node_id).is_ok();
    let inactive = graph_item_inactive(project, GraphItem::Node(node_id), current_time);
    NodeSelectionPresentation {
        selected,
        inactive,
        visual: Editor::node_visual_style(
            node_palette(project, node_id),
            inactive,
            selected,
            scale,
        ),
    }
}

impl ProjectNodeViewer<'_> {
    pub(super) fn container_selection_presentation(
        &self,
        owner: PortOwner,
    ) -> ContainerSelectionPresentation {
        container_selection_presentation(
            self.project,
            self.containers,
            self.selected_container_owners,
            owner,
            self.current_time,
            self.to_global.scaling,
        )
    }

    pub(super) fn node_selection_presentation(&self, node_id: Uuid) -> NodeSelectionPresentation {
        node_selection_presentation(
            self.project,
            self.selected_node_ids,
            node_id,
            self.current_time,
            self.to_global.scaling,
        )
    }
}

pub(super) fn node_highlight_metadata(style: NodeVisualStyle) -> serde_json::Value {
    serde_json::json!({
        "state": style.highlight_state,
        "outer_stroke": {
            "color": [
                style.outer_stroke.color.r(),
                style.outer_stroke.color.g(),
                style.outer_stroke.color.b(),
                style.outer_stroke.color.a(),
            ],
            "width_graph": style.outer_stroke.width,
            "width_screen": style.highlight_screen_width,
        },
        "header_fill": [
            style.header_fill.r(),
            style.header_fill.g(),
            style.header_fill.b(),
            style.header_fill.a(),
        ],
    })
}
