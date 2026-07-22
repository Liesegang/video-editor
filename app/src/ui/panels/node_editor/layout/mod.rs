mod apply;
mod auto;
mod column_packing;
mod directional;
mod merge_alignment;
mod node_geometry;
mod ranking;
mod reflow;
mod structural_merge;
mod swipe;
mod types;

pub(in crate::ui::panels::node_editor) use apply::{
    apply_auto_layout, set_container_geometry, set_container_size, translate_container,
};
#[cfg(test)]
pub(in crate::ui::panels::node_editor) use auto::composition_graph_node_ids;
pub(in crate::ui::panels::node_editor) use auto::{
    compute_auto_layout, compute_full_composition_layout, immediate_child_rects,
};
pub(in crate::ui::panels::node_editor) use directional::{
    plan_directional_layout, BranchDirection, DirectionalLayoutMode, DirectionalLayoutRequest,
    LayoutAxis, NodeLayoutGeometry,
};
#[cfg(test)]
pub(in crate::ui::panels::node_editor) use node_geometry::estimated_merge_node_width;
pub(in crate::ui::panels::node_editor) use node_geometry::{
    estimated_node_size, estimated_node_width,
};
#[cfg(test)]
pub(in crate::ui::panels::node_editor) use ranking::canonical_edges;
pub(in crate::ui::panels::node_editor) use ranking::{rank_nodes_by_scc, rects_are_closer_than};
#[cfg(test)]
pub(in crate::ui::panels::node_editor) use reflow::collect_layout_edits;
#[cfg(test)]
pub(in crate::ui::panels::node_editor) use reflow::padded_intersection;
pub(in crate::ui::panels::node_editor) use reflow::{
    apply_layout_edit, collect_layout_edits_for_selection, composition_content_rect,
    container_hierarchy_needs_reflow, estimated_node_rect, layout_needs_reflow,
    nested_content_rect, rect_contains_rect,
};
pub(in crate::ui::panels::node_editor) use structural_merge::ensure_structural_merge_layout;
pub(in crate::ui::panels::node_editor) use swipe::{
    apply_directional_layout_commit, apply_directional_layout_preview,
    finish_directional_layout_release_guard, handle_directional_layout_outputs,
    recover_directional_layout_release_guard, DirectionalLayoutFrameOutcome,
};
pub(in crate::ui::panels::node_editor) use types::{
    AutoLayoutPlan, AutoLayoutScope, ContainerLayout, LayoutEdit,
};
