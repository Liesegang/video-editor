use crate::state::context_types::{NodeEditorEditableWire, NodeEditorWireDragKind};
use eframe::egui;
use library::model::project::{PortAddress, PortDirection, PortOwner};
use library::model::Project;
use std::collections::HashMap;
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    container_output_node_id, qa_container_key, wire_port_drop_rect, RenderedEdge, RenderedPortKey,
    WireSecondaryClickHit, WIRE_ENDPOINT_RADIUS, WIRE_HIT_RADIUS,
};

pub(in crate::ui::panels::node_editor) fn cubic_bezier_point(
    start: egui::Pos2,
    control_a: egui::Pos2,
    control_b: egui::Pos2,
    end: egui::Pos2,
    t: f32,
) -> egui::Pos2 {
    let one_minus_t = 1.0 - t;
    let weights = [
        one_minus_t.powi(3),
        3.0 * one_minus_t.powi(2) * t,
        3.0 * one_minus_t * t.powi(2),
        t.powi(3),
    ];
    egui::pos2(
        start.x * weights[0]
            + control_a.x * weights[1]
            + control_b.x * weights[2]
            + end.x * weights[3],
        start.y * weights[0]
            + control_a.y * weights[1]
            + control_b.y * weights[2]
            + end.y * weights[3],
    )
}

pub(in crate::ui::panels::node_editor) fn distance_to_segment(
    point: egui::Pos2,
    start: egui::Pos2,
    end: egui::Pos2,
) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * t)
}

pub(in crate::ui::panels::node_editor) fn distance_to_rendered_edge(
    point: egui::Pos2,
    edge: &RenderedEdge,
) -> f32 {
    let mut previous = edge.start;
    let mut distance = f32::INFINITY;
    for sample in 1..=32 {
        let current = cubic_bezier_point(
            edge.start,
            edge.control_a,
            edge.control_b,
            edge.end,
            sample as f32 / 32.0,
        );
        distance = distance.min(distance_to_segment(point, previous, current));
        previous = current;
    }
    distance
}

pub(in crate::ui::panels::node_editor) fn segment_orientation(
    start: egui::Pos2,
    end: egui::Pos2,
    point: egui::Pos2,
) -> f32 {
    let segment = end - start;
    let offset = point - start;
    segment.x * offset.y - segment.y * offset.x
}

pub(in crate::ui::panels::node_editor) fn point_on_segment(
    point: egui::Pos2,
    start: egui::Pos2,
    end: egui::Pos2,
) -> bool {
    const EPSILON: f32 = 1.0e-4;
    segment_orientation(start, end, point).abs() <= EPSILON
        && point.x >= start.x.min(end.x) - EPSILON
        && point.x <= start.x.max(end.x) + EPSILON
        && point.y >= start.y.min(end.y) - EPSILON
        && point.y <= start.y.max(end.y) + EPSILON
}

pub(in crate::ui::panels::node_editor) fn segments_intersect(
    left_start: egui::Pos2,
    left_end: egui::Pos2,
    right_start: egui::Pos2,
    right_end: egui::Pos2,
) -> bool {
    let left_to_right_start = segment_orientation(left_start, left_end, right_start);
    let left_to_right_end = segment_orientation(left_start, left_end, right_end);
    let right_to_left_start = segment_orientation(right_start, right_end, left_start);
    let right_to_left_end = segment_orientation(right_start, right_end, left_end);
    let crosses_both_lines = left_to_right_start * left_to_right_end < 0.0
        && right_to_left_start * right_to_left_end < 0.0;
    if crosses_both_lines {
        return true;
    }

    const EPSILON: f32 = 1.0e-4;
    (left_to_right_start.abs() <= EPSILON && point_on_segment(right_start, left_start, left_end))
        || (left_to_right_end.abs() <= EPSILON && point_on_segment(right_end, left_start, left_end))
        || (right_to_left_start.abs() <= EPSILON
            && point_on_segment(left_start, right_start, right_end))
        || (right_to_left_end.abs() <= EPSILON
            && point_on_segment(left_end, right_start, right_end))
}

pub(in crate::ui::panels::node_editor) fn knife_segment_hits_edge(
    start: egui::Pos2,
    end: egui::Pos2,
    edge: &RenderedEdge,
) -> bool {
    let mut previous = edge.start;
    for sample in 1..=48 {
        let current = cubic_bezier_point(
            edge.start,
            edge.control_a,
            edge.control_b,
            edge.end,
            sample as f32 / 48.0,
        );
        let within_tolerance = segments_intersect(start, end, previous, current)
            || distance_to_segment(start, previous, current) <= 3.0
            || distance_to_segment(end, previous, current) <= 3.0
            || distance_to_segment(previous, start, end) <= 3.0
            || distance_to_segment(current, start, end) <= 3.0;
        if within_tolerance {
            return true;
        }
        previous = current;
    }
    false
}

pub(in crate::ui::panels::node_editor) fn rendered_edge_at_position(
    edges: &[RenderedEdge],
    position: egui::Pos2,
) -> Option<&RenderedEdge> {
    edges
        .iter()
        .filter_map(|edge| {
            let distance = distance_to_rendered_edge(position, edge);
            (distance <= WIRE_HIT_RADIUS).then_some((
                edge,
                edge.kind.editable_wire().is_none(),
                distance,
            ))
        })
        .min_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| left.2.total_cmp(&right.2))
        })
        .map(|(edge, _, _)| edge)
}

pub(in crate::ui::panels::node_editor) fn wire_secondary_click_hit(
    edges: &[RenderedEdge],
    position: egui::Pos2,
) -> Option<WireSecondaryClickHit> {
    rendered_edge_at_position(edges, position).map(|edge| {
        edge.kind.editable_wire().map_or(
            WireSecondaryClickHit::DisplayOnly,
            WireSecondaryClickHit::Editable,
        )
    })
}

pub(in crate::ui::panels::node_editor) fn editable_wire_sort_key(
    target: NodeEditorEditableWire,
) -> (u8, u8, Uuid, Uuid) {
    match target {
        NodeEditorEditableWire::ProjectConnection { connection_id } => {
            (0, 0, connection_id, Uuid::nil())
        }
        NodeEditorEditableWire::OutputBinding { owner, node_id } => {
            let owner_kind = match owner {
                PortOwner::Composition(_) => 0,
                PortOwner::Track(_) => 1,
                PortOwner::Clip(_) => 2,
                PortOwner::Node(_) => 3,
            };
            (1, owner_kind, owner.id(), node_id)
        }
    }
}

pub(in crate::ui::panels::node_editor) fn editable_wire_qa_value(
    target: NodeEditorEditableWire,
) -> serde_json::Value {
    match target {
        NodeEditorEditableWire::ProjectConnection { connection_id } => serde_json::json!({
            "kind": "explicit",
            "connection_id": connection_id,
        }),
        NodeEditorEditableWire::OutputBinding { owner, node_id } => serde_json::json!({
            "kind": "output_binding",
            "owner": qa_container_key(owner),
            "node_id": node_id,
        }),
    }
}

pub(in crate::ui::panels::node_editor) fn editable_wire_stable_key(
    target: NodeEditorEditableWire,
) -> String {
    match target {
        NodeEditorEditableWire::ProjectConnection { connection_id } => connection_id.to_string(),
        NodeEditorEditableWire::OutputBinding { owner, node_id } => {
            format!("output_binding:{}:{node_id}", qa_container_key(owner))
        }
    }
}

pub(in crate::ui::panels::node_editor) fn editable_wire_is_current(
    project: &Project,
    target: NodeEditorEditableWire,
) -> bool {
    match target {
        NodeEditorEditableWire::ProjectConnection { connection_id } => project
            .connections
            .iter()
            .any(|connection| connection.id == connection_id),
        NodeEditorEditableWire::OutputBinding { owner, node_id } => {
            container_output_node_id(project, owner) == Some(node_id)
        }
    }
}

pub(in crate::ui::panels::node_editor) fn rendered_wire_drag_kind(
    edge: &RenderedEdge,
    position: egui::Pos2,
) -> NodeEditorWireDragKind {
    // At overview zoom a whole wire can be shorter than two fixed endpoint
    // radii. Reserve at most the outer quarter on either side so the rendered
    // curve midpoint always remains a genuine body/disconnect target.
    let endpoint_radius = WIRE_ENDPOINT_RADIUS.min(edge.start.distance(edge.end) * 0.25);
    if position.distance(edge.start) <= endpoint_radius {
        NodeEditorWireDragKind::ReconnectSource
    } else if position.distance(edge.end) <= endpoint_radius {
        NodeEditorWireDragKind::ReconnectTarget
    } else {
        NodeEditorWireDragKind::Disconnect
    }
}

pub(in crate::ui::panels::node_editor) fn rendered_port_at_position(
    ports: &HashMap<RenderedPortKey, egui::Rect>,
    direction: PortDirection,
    position: egui::Pos2,
    canvas_clip: egui::Rect,
) -> Option<PortAddress> {
    ports
        .iter()
        .filter(|(key, rect)| {
            key.direction == direction
                && canvas_clip.contains(position)
                && wire_port_drop_rect(**rect).contains(position)
        })
        .min_by(|left, right| {
            left.1
                .center()
                .distance(position)
                .total_cmp(&right.1.center().distance(position))
        })
        .map(|(key, _)| key.address.clone())
}

pub(in crate::ui::panels::node_editor) fn rendered_normal_port_at_position(
    ports: &HashMap<RenderedPortKey, egui::Rect>,
    position: egui::Pos2,
    canvas_clip: egui::Rect,
) -> Option<RenderedPortKey> {
    canvas_clip.contains(position).then_some(())?;
    ports
        .iter()
        .filter(|(_, rect)| rect.is_positive() && rect.contains(position))
        .min_by(|left, right| {
            left.1
                .center()
                .distance(position)
                .total_cmp(&right.1.center().distance(position))
        })
        .map(|(key, _)| key.clone())
}
