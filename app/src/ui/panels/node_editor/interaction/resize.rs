use crate::state::context_types::{ContainerResizeEdge, ContainerResizeState, NodeEditorState};
use eframe::egui;
use library::model::project::PortOwner;
use library::model::Project;

#[cfg(test)]
use crate::ui::panels::node_editor::capture_test_rect;
use crate::ui::panels::node_editor::{
    clipped_qa_rect, container_rect, estimated_node_rect, node_editor_resize_interactions_enabled,
    qa_container_key, qa_rect_metadata, ContainerVisual, LayoutEdit, AUTO_LAYOUT_NODE_PADDING,
    CONTAINER_HEADER_HEIGHT, MIN_CONTAINER_SIZE, RESIZE_CORNER_SIZE, RESIZE_HIT_WIDTH,
};

pub(in crate::ui::panels::node_editor) fn container_resize_interactions(
    ui: &mut egui::Ui,
    project: &Project,
    containers: &[ContainerVisual],
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    state: &mut NodeEditorState,
) -> Vec<LayoutEdit> {
    let resize_interactions = node_editor_resize_interactions_enabled(to_global.scaling);
    let pointer = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.pointer.primary_released(),
            input.pointer.interact_pos(),
        )
    });

    let mut hit = None;
    for container in containers.iter().rev() {
        if container.collapsed {
            continue;
        }
        let global = to_global * container.rect();
        for (edge, label, unclipped_rect, cursor) in resize_regions(global) {
            let rect = clipped_qa_rect(unclipped_rect, canvas_clip);
            let id = format!(
                "node_editor.resize_edge.{}:{label}",
                qa_container_key(container.owner)
            );
            #[cfg(test)]
            capture_test_rect(&id, rect);
            crate::qa::register_component_with_metadata(
                id.clone(),
                "node_container_resize_edge",
                rect,
                resize_interactions,
                Some(serde_json::json!({
                    "owner": qa_container_key(container.owner),
                    "edge": label,
                    "visual": false,
                    "overview_disabled": !resize_interactions,
                    "unclipped_rect": qa_rect_metadata(unclipped_rect),
                    "visible_in_canvas": rect.is_positive(),
                })),
            );
            if resize_interactions && rect.is_positive() {
                ui.interact(rect, egui::Id::new(id), egui::Sense::hover())
                    .on_hover_cursor(cursor);
            }
            if resize_interactions
                && rect.is_positive()
                && hit.is_none()
                && pointer.3.is_some_and(|position| rect.contains(position))
            {
                hit = Some((container, edge));
            }
        }
    }

    if !resize_interactions {
        state.container_resize = None;
        return Vec::new();
    }

    if state.container_resize.is_none() && pointer.0 {
        if let (Some((container, edge)), Some(position)) = (hit, pointer.3) {
            state.container_resize = Some(ContainerResizeState {
                owner: container.owner,
                edge,
                start_pointer: position,
                start_position: container.position,
                start_size: container.size,
            });
        }
    }

    let mut edits = Vec::new();
    if pointer.1 {
        if let (Some(resize), Some(position)) = (&state.container_resize, pointer.3) {
            let from_global = to_global.inverse();
            let start = from_global * resize.start_pointer;
            let current = from_global * position;
            let delta = current - start;
            let child_bounds = container_child_bounds(project, resize.owner);
            let (position, size) = resized_container_geometry(resize, delta, child_bounds);
            edits.push(LayoutEdit::ResizeContainer {
                owner: resize.owner,
                position,
                size,
            });
        }
    }
    if pointer.2 {
        state.container_resize = None;
    }
    edits
}

pub(in crate::ui::panels::node_editor) fn resize_regions(
    rect: egui::Rect,
) -> [(
    ContainerResizeEdge,
    &'static str,
    egui::Rect,
    egui::CursorIcon,
); 8] {
    let corner = RESIZE_CORNER_SIZE
        .min(rect.width() * 0.25)
        .min(rect.height() * 0.25);
    let h = RESIZE_HIT_WIDTH;
    [
        (
            ContainerResizeEdge::TopLeft,
            "tl",
            egui::Rect::from_min_size(rect.min, egui::vec2(corner, corner)),
            egui::CursorIcon::ResizeNwSe,
        ),
        (
            ContainerResizeEdge::TopRight,
            "tr",
            egui::Rect::from_min_size(
                egui::pos2(rect.right() - corner, rect.top()),
                egui::vec2(corner, corner),
            ),
            egui::CursorIcon::ResizeNeSw,
        ),
        (
            ContainerResizeEdge::BottomLeft,
            "bl",
            egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.bottom() - corner),
                egui::vec2(corner, corner),
            ),
            egui::CursorIcon::ResizeNeSw,
        ),
        (
            ContainerResizeEdge::BottomRight,
            "br",
            egui::Rect::from_min_size(
                egui::pos2(rect.right() - corner, rect.bottom() - corner),
                egui::vec2(corner, corner),
            ),
            egui::CursorIcon::ResizeNwSe,
        ),
        (
            ContainerResizeEdge::Top,
            "top",
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + corner, rect.top() - h * 0.5),
                egui::pos2(rect.right() - corner, rect.top() + h * 0.5),
            ),
            egui::CursorIcon::ResizeVertical,
        ),
        (
            ContainerResizeEdge::Bottom,
            "bottom",
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + corner, rect.bottom() - h * 0.5),
                egui::pos2(rect.right() - corner, rect.bottom() + h * 0.5),
            ),
            egui::CursorIcon::ResizeVertical,
        ),
        (
            ContainerResizeEdge::Left,
            "left",
            egui::Rect::from_min_max(
                egui::pos2(rect.left() - h * 0.5, rect.top() + corner),
                egui::pos2(rect.left() + h * 0.5, rect.bottom() - corner),
            ),
            egui::CursorIcon::ResizeHorizontal,
        ),
        (
            ContainerResizeEdge::Right,
            "right",
            egui::Rect::from_min_max(
                egui::pos2(rect.right() - h * 0.5, rect.top() + corner),
                egui::pos2(rect.right() + h * 0.5, rect.bottom() - corner),
            ),
            egui::CursorIcon::ResizeHorizontal,
        ),
    ]
}

pub(in crate::ui::panels::node_editor) fn resized_container_geometry(
    resize: &ContainerResizeState,
    delta: egui::Vec2,
    child_bounds: Option<egui::Rect>,
) -> ([f32; 2], [f32; 2]) {
    let mut left = resize.start_position[0];
    let mut top = resize.start_position[1];
    let mut right = left + resize.start_size[0];
    let mut bottom = top + resize.start_size[1];
    let moves_left = matches!(
        resize.edge,
        ContainerResizeEdge::Left | ContainerResizeEdge::TopLeft | ContainerResizeEdge::BottomLeft
    );
    let moves_right = matches!(
        resize.edge,
        ContainerResizeEdge::Right
            | ContainerResizeEdge::TopRight
            | ContainerResizeEdge::BottomRight
    );
    let moves_top = matches!(
        resize.edge,
        ContainerResizeEdge::Top | ContainerResizeEdge::TopLeft | ContainerResizeEdge::TopRight
    );
    let moves_bottom = matches!(
        resize.edge,
        ContainerResizeEdge::Bottom
            | ContainerResizeEdge::BottomLeft
            | ContainerResizeEdge::BottomRight
    );
    if moves_left {
        left += delta.x;
    }
    if moves_right {
        right += delta.x;
    }
    if moves_top {
        top += delta.y;
    }
    if moves_bottom {
        bottom += delta.y;
    }
    if let Some(children) = child_bounds {
        left = left.min(children.left() - AUTO_LAYOUT_NODE_PADDING);
        top = top.min(children.top() - CONTAINER_HEADER_HEIGHT - AUTO_LAYOUT_NODE_PADDING);
        right = right.max(children.right() + AUTO_LAYOUT_NODE_PADDING);
        bottom = bottom.max(children.bottom() + AUTO_LAYOUT_NODE_PADDING);
    }
    if right - left < MIN_CONTAINER_SIZE.x {
        if moves_left && !moves_right {
            left = right - MIN_CONTAINER_SIZE.x;
        } else {
            right = left + MIN_CONTAINER_SIZE.x;
        }
    }
    if bottom - top < MIN_CONTAINER_SIZE.y {
        if moves_top && !moves_bottom {
            top = bottom - MIN_CONTAINER_SIZE.y;
        } else {
            bottom = top + MIN_CONTAINER_SIZE.y;
        }
    }
    ([left, top], [right - left, bottom - top])
}

pub(in crate::ui::panels::node_editor) fn container_child_bounds(
    project: &Project,
    owner: PortOwner,
) -> Option<egui::Rect> {
    let mut rect = egui::Rect::NOTHING;
    let mut include = |child: egui::Rect| rect = rect.union(child);
    match owner {
        PortOwner::Composition(id) => {
            let item = project.get_composition(id)?;
            for track_id in &item.track_ids {
                if let Some(track) = project.get_track(*track_id) {
                    include(container_rect(track.ui_position, track.ui_size));
                }
            }
            for node_id in &item.node_ids {
                if let Some(node) = estimated_node_rect(project, *node_id) {
                    include(node);
                }
            }
        }
        PortOwner::Track(id) => {
            let item = project.get_track(id)?;
            for clip_id in &item.clip_ids {
                if let Some(clip) = project.get_clip(*clip_id) {
                    include(container_rect(clip.ui_position, clip.ui_size));
                }
            }
            for node_id in &item.node_ids {
                if let Some(node) = estimated_node_rect(project, *node_id) {
                    include(node);
                }
            }
        }
        PortOwner::Clip(id) => {
            let item = project.get_clip(id)?;
            for node_id in &item.node_ids {
                if let Some(node) = estimated_node_rect(project, *node_id) {
                    include(node);
                }
            }
        }
        PortOwner::Node(_) => return None,
    }
    rect.is_positive().then_some(rect)
}
