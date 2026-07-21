use crate::state::context_types::{NodeEditorEditableWire, NodeEditorWireDragKind};
use eframe::egui;
use library::model::project::{PortAddress, PortDirection, PortOwner, PortSide};
use library::model::Project;
use node_editor_ui::wire::{CubicBezier, HitRegion};
use std::collections::HashMap;
use uuid::Uuid;

use crate::ui::panels::node_editor::{
    container_output_node_id, container_output_type_key, qa_container_key, wire_port_drop_rect,
    RenderedEdge, RenderedPortKey, WireSecondaryClickHit, WIRE_ENDPOINT_RADIUS, WIRE_HIT_RADIUS,
};

pub(in crate::ui::panels::node_editor) fn cubic_bezier_point(
    start: egui::Pos2,
    control_a: egui::Pos2,
    control_b: egui::Pos2,
    end: egui::Pos2,
    t: f32,
) -> egui::Pos2 {
    CubicBezier::new(start, control_a, control_b, end).point(t)
}

pub(in crate::ui::panels::node_editor) fn distance_to_rendered_edge(
    point: egui::Pos2,
    edge: &RenderedEdge,
) -> f32 {
    rendered_edge_curve(edge).distance_to(point)
}

pub(in crate::ui::panels::node_editor) fn knife_segment_hits_edge(
    start: egui::Pos2,
    end: egui::Pos2,
    edge: &RenderedEdge,
) -> bool {
    rendered_edge_curve(edge).intersects_segment(start, end, 3.0)
}

fn rendered_edge_curve(edge: &RenderedEdge) -> CubicBezier {
    CubicBezier::new(edge.start, edge.control_a, edge.control_b, edge.end)
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
) -> (u8, u8, u8, Uuid, Uuid) {
    match target {
        NodeEditorEditableWire::ProjectConnection { connection_id } => {
            (0, 0, 0, connection_id, Uuid::nil())
        }
        NodeEditorEditableWire::OutputBinding {
            owner,
            node_id,
            data_type,
        } => {
            let owner_kind = match owner {
                PortOwner::Composition(_) => 0,
                PortOwner::Track(_) => 1,
                PortOwner::Clip(_) => 2,
                PortOwner::Node(_) => 3,
            };
            let output_kind = match container_output_type_key(data_type) {
                Some("image") => 0,
                Some("audio") => 1,
                Some(_) | None => 2,
            };
            (1, owner_kind, output_kind, owner.id(), node_id)
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
        NodeEditorEditableWire::OutputBinding {
            owner,
            node_id,
            data_type,
        } => serde_json::json!({
            "kind": "output_binding",
            "owner": qa_container_key(owner),
            "node_id": node_id,
            "output_type": container_output_type_key(data_type),
        }),
    }
}

pub(in crate::ui::panels::node_editor) fn editable_wire_stable_key(
    target: NodeEditorEditableWire,
) -> String {
    match target {
        NodeEditorEditableWire::ProjectConnection { connection_id } => connection_id.to_string(),
        NodeEditorEditableWire::OutputBinding {
            owner,
            node_id,
            data_type,
        } => {
            let output_type = container_output_type_key(data_type).unwrap_or("unsupported");
            format!(
                "output_binding:{}:{output_type}:{node_id}",
                qa_container_key(owner)
            )
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
        NodeEditorEditableWire::OutputBinding {
            owner,
            node_id,
            data_type,
        } => container_output_node_id(project, owner, data_type) == Some(node_id),
    }
}

pub(in crate::ui::panels::node_editor) fn rendered_wire_drag_kind(
    edge: &RenderedEdge,
    position: egui::Pos2,
) -> NodeEditorWireDragKind {
    match rendered_edge_curve(edge).hit_region(position, WIRE_ENDPOINT_RADIUS) {
        HitRegion::Start => NodeEditorWireDragKind::ReconnectSource,
        HitRegion::Body => NodeEditorWireDragKind::Disconnect,
        HitRegion::End => NodeEditorWireDragKind::ReconnectTarget,
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

/// Container outputs sit against the integrated header, while their public QA
/// rectangle includes the fixed reconnect/drop padding around the socket. A
/// press in that padding must start the wire gesture instead of leaking
/// through to Snarl's larger container-frame drag surface.
pub(in crate::ui::panels::node_editor) fn rendered_container_output_at_position(
    project: &Project,
    ports: &HashMap<RenderedPortKey, egui::Rect>,
    position: egui::Pos2,
    canvas_clip: egui::Rect,
) -> Option<RenderedPortKey> {
    canvas_clip.contains(position).then_some(())?;
    ports
        .iter()
        .filter(|(key, rect)| {
            key.direction == PortDirection::Output
                && !matches!(key.address.owner, PortOwner::Node(_))
                && rect.is_positive()
                && wire_port_drop_rect(**rect).contains(position)
                && project
                    .port_definition(&key.address, PortDirection::Output)
                    .is_some_and(|definition| definition.side == PortSide::Right)
        })
        .min_by(|left, right| {
            left.1
                .center()
                .distance(position)
                .total_cmp(&right.1.center().distance(position))
        })
        .map(|(key, _)| key.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::project::PortDataType;

    #[test]
    fn derived_wire_secondary_hit_is_display_only_instead_of_blank_canvas() {
        let derived = RenderedEdge {
            kind: crate::ui::panels::node_editor::RenderedEdgeKind::DerivedOutput {
                owner: PortOwner::Track(Uuid::from_u128(0xD001)),
                source: PortOwner::Clip(Uuid::from_u128(0xD002)),
                data_type: PortDataType::Image,
            },
            start: egui::pos2(100.0, 180.0),
            control_a: egui::pos2(180.0, 180.0),
            control_b: egui::pos2(320.0, 180.0),
            end: egui::pos2(400.0, 180.0),
        };
        let hit_point = egui::pos2(250.0, 180.0);

        assert_eq!(
            wire_secondary_click_hit(&[derived], hit_point),
            Some(WireSecondaryClickHit::DisplayOnly)
        );
        assert_eq!(wire_secondary_click_hit(&[], hit_point), None);
    }

    #[test]
    fn wire_knife_detects_midspan_intersection_of_long_segments() {
        let knife_start = egui::pos2(10.0, -1_000.0);
        let knife_end = egui::pos2(10.0, 1_000.0);
        let edge = RenderedEdge {
            kind: crate::ui::panels::node_editor::RenderedEdgeKind::ProjectConnection {
                connection_id: Uuid::new_v4(),
            },
            start: egui::pos2(-1_000.0, 0.0),
            control_a: egui::pos2(-333.333_34, 0.0),
            control_b: egui::pos2(333.333_34, 0.0),
            end: egui::pos2(1_000.0, 0.0),
        };

        assert!(knife_segment_hits_edge(knife_start, knife_end, &edge));
    }
}
