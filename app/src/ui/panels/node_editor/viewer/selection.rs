use crate::ui::panels::node_editor::{
    graph_item_inactive, merge_images_target_node_id, node_palette, GraphItem,
};
use library::model::project::{PortAddress, PortOwner, MERGE_IMAGES_PORT};
use library::model::Project;
use node_editor_ui::{Editor, NodeVisualStyle};
use uuid::Uuid;

pub(super) fn is_physical_merge_node(project: &Project, node_id: Uuid) -> bool {
    let target = PortAddress::new(PortOwner::Node(node_id), MERGE_IMAGES_PORT);
    merge_images_target_node_id(project, &target).is_some()
}

pub(super) struct NodeSelectionPresentation {
    pub(super) selected: bool,
    pub(super) inactive: bool,
    pub(super) visual: NodeVisualStyle,
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
