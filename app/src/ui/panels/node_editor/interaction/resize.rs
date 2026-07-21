use crate::state::context_types::{ContainerResizeEdge, ContainerResizeState, NodeEditorState};
use eframe::egui;
use library::model::project::{PortDirection, PortOwner, PortSide};
use library::model::Project;

#[cfg(test)]
use crate::ui::panels::node_editor::capture_test_rect;
use crate::ui::panels::node_editor::{
    clipped_qa_rect, container_rect, estimated_node_rect, node_editor_port_interactions_enabled,
    node_editor_resize_interactions_enabled, qa_container_key, qa_rect_metadata,
    wire_port_drop_rect, ContainerVisual, LayoutEdit, PortAnchorKind, AUTO_LAYOUT_NODE_PADDING,
    CONTAINER_HEADER_HEIGHT, MIN_CONTAINER_SIZE, PORT_SOCKET_SIZE, RESIZE_CORNER_SIZE,
    RESIZE_HIT_WIDTH,
};

/// Capture a resize press before Snarl gets a chance to treat the same
/// primary gesture as background pan. Geometry comes from the previous
/// completed frame, which is also the source of the HTTP QA component rects.
pub(in crate::ui::panels::node_editor) fn capture_container_resize_before_canvas(
    ui: &egui::Ui,
    project: &Project,
    containers: &[ContainerVisual],
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    state: &mut NodeEditorState,
) -> bool {
    if state.container_resize.is_some()
        || !node_editor_resize_interactions_enabled(to_global.scaling)
    {
        return state.container_resize.is_some();
    }
    let Some(pointer) = ui.input(|input| {
        input
            .pointer
            .primary_pressed()
            .then(|| input.pointer.interact_pos())
            .flatten()
    }) else {
        return false;
    };
    let Some((container, edge)) = resize_hit(project, containers, to_global, canvas_clip, pointer)
    else {
        return false;
    };
    state.container_resize = Some(ContainerResizeState {
        owner: container.owner,
        edge,
        start_pointer: pointer,
        start_position: container.position,
        start_size: container.size,
        canvas_transform: to_global,
    });
    true
}

fn resize_hit<'a>(
    project: &Project,
    containers: &'a [ContainerVisual],
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    pointer: egui::Pos2,
) -> Option<(&'a ContainerVisual, ContainerResizeEdge)> {
    containers.iter().rev().find_map(|container| {
        if container.collapsed
            || pointer_hits_container_output(project, container, to_global, canvas_clip, pointer)
        {
            return None;
        }
        let global = to_global * container.rect();
        resize_regions(global)
            .into_iter()
            .find(|(_, _, rect, _)| rect.intersect(canvas_clip).contains(pointer))
            .map(|(edge, _, _, _)| (container, edge))
    })
}

fn pointer_hits_container_output(
    project: &Project,
    container: &ContainerVisual,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    pointer: egui::Pos2,
) -> bool {
    if !node_editor_port_interactions_enabled(to_global.scaling) {
        return false;
    }
    let output_count = project
        .port_definitions(container.owner)
        .into_iter()
        .filter(|definition| {
            definition.direction == PortDirection::Output && definition.side == PortSide::Right
        })
        .count();
    (0..output_count).any(|index| {
        let graph_rect = egui::Rect::from_center_size(
            container.embedded_port_center(PortAnchorKind::ExternalOutputs, index),
            egui::Vec2::splat(PORT_SOCKET_SIZE),
        );
        wire_port_drop_rect(to_global * graph_rect)
            .intersect(canvas_clip)
            .contains(pointer)
    })
}

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
        let pointer_on_output = pointer.3.is_some_and(|position| {
            pointer_hits_container_output(project, container, to_global, canvas_clip, position)
        });
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
            if resize_interactions && rect.is_positive() && !pointer_on_output {
                ui.interact(rect, egui::Id::new(id), egui::Sense::hover())
                    .on_hover_cursor(cursor);
            }
            if resize_interactions
                && rect.is_positive()
                && !pointer_on_output
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

    if state.normal_connect_gesture.is_some() || state.wire_gesture.is_some() {
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
                canvas_transform: to_global,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::panels::node_editor::{resolve_node_editor_transform, ContainerKind};
    use library::model::Composition;
    use uuid::Uuid;

    #[test]
    fn edge_and_corner_resize_geometry_is_absolute_and_preserves_children() {
        let base = ContainerResizeState {
            owner: PortOwner::Clip(Uuid::from_u128(0x99)),
            edge: ContainerResizeEdge::Right,
            start_pointer: egui::pos2(0.0, 0.0),
            start_position: [100.0, 120.0],
            start_size: [500.0, 300.0],
            canvas_transform: egui::emath::TSTransform::IDENTITY,
        };
        let (position, size) = resized_container_geometry(&base, egui::vec2(40.0, 70.0), None);
        assert_eq!(position, [100.0, 120.0]);
        assert_eq!(size, [540.0, 300.0]);

        let corner = ContainerResizeState {
            edge: ContainerResizeEdge::BottomRight,
            ..base
        };
        let (position, size) = resized_container_geometry(&corner, egui::vec2(40.0, 70.0), None);
        assert_eq!(position, [100.0, 120.0]);
        assert_eq!(size, [540.0, 370.0]);

        let children = egui::Rect::from_min_max(egui::pos2(180.0, 230.0), egui::pos2(650.0, 460.0));
        let shrinking = ContainerResizeState {
            edge: ContainerResizeEdge::BottomRight,
            ..base
        };
        let (position, size) =
            resized_container_geometry(&shrinking, egui::vec2(-400.0, -300.0), Some(children));
        let result = container_rect(position, size);
        assert!(result.right() >= children.right() + AUTO_LAYOUT_NODE_PADDING - 0.01);
        assert!(result.bottom() >= children.bottom() + AUTO_LAYOUT_NODE_PADDING - 0.01);

        let regions = resize_regions(egui::Rect::from_min_size(
            egui::pos2(20.0, 30.0),
            egui::vec2(500.0, 300.0),
        ));
        assert_eq!(regions.len(), 8);
        assert!(regions.iter().all(|(_, _, rect, _)| rect.is_positive()));
    }

    #[test]
    fn resize_press_is_captured_before_canvas_pan_and_freezes_transform() {
        let visual = ContainerVisual {
            owner: PortOwner::Track(Uuid::from_u128(0x5151)),
            kind: ContainerKind::Track,
            position: [100.0, 80.0],
            size: [600.0, 420.0],
            collapsed: false,
        };
        let transform = egui::emath::TSTransform::new(egui::vec2(35.0, 21.0), 0.8);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let edge = (transform * visual.rect()).right_center();
        let context = egui::Context::default();
        let mut state = NodeEditorState::default();
        let project = Project::new("resize capture");

        let output = context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![
                    egui::Event::PointerMoved(edge),
                    egui::Event::PointerButton {
                        pos: edge,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    assert!(capture_container_resize_before_canvas(
                        ui,
                        &project,
                        std::slice::from_ref(&visual),
                        transform,
                        screen,
                        &mut state,
                    ));
                });
            },
        );
        assert!(!output.shapes.is_empty());

        let resize = state.container_resize.expect("captured resize gesture");
        assert_eq!(resize.edge, ContainerResizeEdge::Right);
        assert_eq!(resize.canvas_transform, transform);
        let mut scene_pan = egui::emath::TSTransform::new(
            transform.translation + egui::vec2(180.0, 90.0),
            transform.scaling,
        );
        resolve_node_editor_transform(&mut scene_pan, Some(resize.canvas_transform));
        assert_eq!(scene_pan, transform);
    }

    #[test]
    fn container_output_socket_preempts_overlapping_top_right_resize_hit() {
        let mut project = Project::new("output before resize");
        let (composition, mut track) = Composition::new("Main", 640, 360, 24.0, 2.0);
        track.ui_position = [100.0, 80.0];
        track.ui_size = [600.0, 420.0];
        let visual = ContainerVisual {
            owner: PortOwner::Track(track.id),
            kind: ContainerKind::Track,
            position: track.ui_position,
            size: track.ui_size,
            collapsed: false,
        };
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        let transform = egui::emath::TSTransform::IDENTITY;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let output = visual.embedded_port_center(PortAnchorKind::ExternalOutputs, 0);
        let top_right = resize_regions(visual.rect())
            .into_iter()
            .find(|(edge, _, _, _)| *edge == ContainerResizeEdge::TopRight)
            .expect("top-right resize region")
            .2;

        assert!(top_right.contains(output), "fixture must reproduce overlap");
        assert!(pointer_hits_container_output(
            &project, &visual, transform, screen, output,
        ));
        assert!(resize_hit(
            &project,
            std::slice::from_ref(&visual),
            transform,
            screen,
            output,
        )
        .is_none());

        let ordinary_right_edge = visual.rect().right_center();
        assert!(!pointer_hits_container_output(
            &project,
            &visual,
            transform,
            screen,
            ordinary_right_edge,
        ));
        assert_eq!(
            resize_hit(
                &project,
                std::slice::from_ref(&visual),
                transform,
                screen,
                ordinary_right_edge,
            )
            .map(|(_, edge)| edge),
            Some(ContainerResizeEdge::Right),
        );
    }
}
