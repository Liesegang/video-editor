use eframe::egui::{self, Color32};
use library::model::project::{PortDataType, PortDirection, PortOwner};
use library::model::Project;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::container_outputs::register_container_output_edges;
use super::hit::cubic_bezier_point;
#[cfg(test)]
use crate::ui::panels::node_editor::capture_test_rect;
use crate::ui::panels::node_editor::{
    blend_mode_qa_key, clipped_qa_rect, connection_supports_authored_blend, container_inactive,
    container_output_node_id, container_output_type_key, edge_endpoint_qa_metadata,
    overview_wire_graph_points, pin_color, qa_container_key, qa_rect_metadata,
    screen_stroke_in_graph_units, wire_order_menu_states, ContainerKind, ContainerVisual,
    EdgeComponent, OverviewWirePainter, RenderedEdge, RenderedEdgeKind, RenderedPortKey,
};

pub(in crate::ui::panels::node_editor) fn register_container_chrome(
    container: &ContainerVisual,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    project: &Project,
    current_time: f64,
) {
    let owner = qa_container_key(container.owner);
    let graph_main = container.rect();
    let unclipped_main = to_global * graph_main;
    let main = clipped_qa_rect(unclipped_main, canvas_clip);
    let graph_content = container.content_rect();
    let content_ratio = graph_content.map(|content| {
        content.width() * content.height() / (graph_main.width() * graph_main.height())
    });
    let main_id = format!("node_editor.container.{owner}");
    #[cfg(test)]
    capture_test_rect(&main_id, main);
    crate::qa::register_component_with_metadata(
        main_id,
        match container.kind {
            ContainerKind::Composition => "composition_container",
            ContainerKind::Track => "track_container",
            ContainerKind::Clip => "clip_container",
        },
        main,
        true,
        Some(serde_json::json!({
            "owner": owner,
            "collapsed": container.collapsed,
            "inactive": container_inactive(project, container.owner, current_time),
            "output_node_id": container_output_node_id(
                project,
                container.owner,
                PortDataType::Image,
            ),
            "image_output_node_id": container_output_node_id(
                project,
                container.owner,
                PortDataType::Image,
            ),
            "audio_output_node_id": container_output_node_id(
                project,
                container.owner,
                PortDataType::Audio,
            ),
            "content_rect": graph_content.map(qa_rect_metadata),
            "content_area_ratio": content_ratio,
            "port_hit_policy": "localized_socket",
            "unclipped_rect": qa_rect_metadata(unclipped_main),
            "visible_in_canvas": main.is_positive(),
        })),
    );

    if let Some(graph_content) = graph_content {
        let unclipped_content = to_global * graph_content;
        let content = clipped_qa_rect(unclipped_content, canvas_clip);
        let content_id = format!("node_editor.container_content.{owner}");
        #[cfg(test)]
        capture_test_rect(&content_id, content);
        crate::qa::register_component_with_metadata(
            content_id,
            "node_container_content",
            content,
            true,
            Some(serde_json::json!({
                "owner": owner,
                "accepts_node_placement": true,
                "port_hit_policy": "localized_socket",
                "unclipped_rect": qa_rect_metadata(unclipped_content),
                "visible_in_canvas": content.is_positive(),
            })),
        );
    }
}

pub(in crate::ui::panels::node_editor) fn register_rendered_edges(
    project: &Project,
    rendered_ports: &Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
    canvas_clip: egui::Rect,
    overview: Option<OverviewWirePainter<'_>>,
) -> Vec<RenderedEdge> {
    let Ok(ports) = rendered_ports.lock() else {
        return Vec::new();
    };
    let mut rendered_edges = Vec::new();
    let order_states = wire_order_menu_states(project);
    for connection in &project.connections {
        let order = order_states.get(&connection.id).copied();
        let authored_blend_available = connection_supports_authored_blend(project, connection);
        let edge = register_edge_component(
            EdgeComponent {
                id: format!("node_editor.edge:{}", connection.id),
                kind: RenderedEdgeKind::ProjectConnection {
                    connection_id: connection.id,
                },
                from: &connection.from,
                to: &connection.to,
                wire_color: project
                    .port_definition(&connection.from, PortDirection::Output)
                    .map_or_else(
                        || pin_color(PortDataType::Any),
                        |definition| pin_color(definition.data_type),
                    ),
                authored_order: Some(connection.order),
                back_to_front_index: order.map(|order| order.back_to_front_index),
                layer_count: order.map(|order| order.layer_count),
                authored_blend_mode: authored_blend_available
                    .then(|| blend_mode_qa_key(connection.blend_mode)),
                authored_blend_available,
            },
            &ports,
            canvas_clip,
            overview,
        );
        if let Some(edge) = edge {
            rendered_edges.push(edge);
        }
    }
    for owner in project
        .compositions
        .iter()
        .map(|item| PortOwner::Composition(item.id))
        .chain(
            project
                .tracks
                .values()
                .map(|item| PortOwner::Track(item.id)),
        )
        .chain(project.clips.values().map(|item| PortOwner::Clip(item.id)))
    {
        rendered_edges.extend(register_container_output_edges(
            project,
            owner,
            &ports,
            canvas_clip,
            overview,
        ));
    }
    rendered_edges
}

pub(in crate::ui::panels::node_editor) fn register_edge_component(
    edge: EdgeComponent<'_>,
    ports: &HashMap<RenderedPortKey, egui::Rect>,
    canvas_clip: egui::Rect,
    overview: Option<OverviewWirePainter<'_>>,
) -> Option<RenderedEdge> {
    let from_rect = ports.get(&RenderedPortKey {
        address: edge.from.clone(),
        direction: PortDirection::Output,
    })?;
    let to_rect = ports.get(&RenderedPortKey {
        address: edge.to.clone(),
        direction: PortDirection::Input,
    })?;
    let start = from_rect.center();
    let end = to_rect.center();
    if ![start, end]
        .iter()
        .all(|position| position.x.is_finite() && position.y.is_finite())
    {
        return None;
    }
    let min_frame = if overview.is_some() { 2.0 } else { 36.0 };
    let frame = ((end.x - start.x).abs() * 0.45).clamp(min_frame, 110.0);
    let control_a = start + egui::vec2(frame, 0.0);
    let control_b = end - egui::vec2(frame, 0.0);
    let screen_points = [start, control_a, control_b, end];
    if let Some(overview) = overview {
        if let Some(graph_points) = overview_wire_graph_points(screen_points, overview.to_global) {
            let width = if matches!(edge.kind, RenderedEdgeKind::DerivedOutput { .. }) {
                1.15
            } else {
                1.65
            };
            overview
                .painter
                .add(egui::epaint::CubicBezierShape::from_points_stroke(
                    graph_points,
                    false,
                    Color32::TRANSPARENT,
                    egui::Stroke::new(
                        screen_stroke_in_graph_units(width, overview.to_global.scaling),
                        edge.wire_color.gamma_multiply(0.9),
                    ),
                ));
        }
    }
    let unclipped_bbox = egui::Rect::from_points(&[start, control_a, control_b, end]).expand(7.0);
    let bbox = clipped_qa_rect(unclipped_bbox, canvas_clip);
    let midpoint = cubic_bezier_point(start, control_a, control_b, end, 0.5);
    let unclipped_hit_rect = egui::Rect::from_center_size(midpoint, egui::vec2(16.0, 16.0));
    let hit_rect = clipped_qa_rect(unclipped_hit_rect, canvas_clip);
    let editable_wire = edge.kind.editable_wire();
    let connection_id = edge.kind.connection_id();
    let qa_rect = if editable_wire.is_some() {
        hit_rect
    } else {
        bbox
    };
    let (binding_owner, binding_node_id, binding_output_type) = match edge.kind {
        RenderedEdgeKind::OutputBinding {
            owner,
            node_id,
            data_type,
        } => (
            Some(qa_container_key(owner)),
            Some(node_id),
            container_output_type_key(data_type),
        ),
        _ => (None, None, None),
    };
    let (derived_owner, derived_source, derived_output_type) = match edge.kind {
        RenderedEdgeKind::DerivedOutput {
            owner,
            source,
            data_type,
        } => (
            Some(qa_container_key(owner)),
            Some(qa_container_key(source)),
            container_output_type_key(data_type),
        ),
        _ => (None, None, None),
    };
    let action = match edge.kind {
        RenderedEdgeKind::ProjectConnection { .. } => Some("select_or_edit"),
        RenderedEdgeKind::OutputBinding { .. } => Some("delete_output_binding"),
        RenderedEdgeKind::DerivedOutput { .. } => None,
    };
    #[cfg(test)]
    capture_test_rect(&edge.id, qa_rect);
    crate::qa::register_component_with_metadata(
        edge.id,
        "node_edge",
        qa_rect,
        true,
        Some(serde_json::json!({
            "kind": edge.kind.metadata_kind(),
            "connection_id": connection_id,
            "action": action,
            "editable": editable_wire.is_some(),
            "edit_blocked_reason": edge.kind.blocked_reason(),
            "binding_owner": binding_owner,
            "binding_node_id": binding_node_id,
            "binding_output_type": binding_output_type,
            "derived_owner": derived_owner,
            "derived_source": derived_source,
            "derived_output_type": derived_output_type,
            "from": {
                "owner": qa_container_key(edge.from.owner),
                "port": edge.from.port,
                "x": start.x,
                "y": start.y,
            },
            "to": {
                "owner": qa_container_key(edge.to.owner),
                "port": edge.to.port,
                "x": end.x,
                "y": end.y,
            },
            "ltr": start.x <= end.x,
            "visible": qa_rect.is_positive(),
            "overview_painted": overview.is_some(),
            "authored_order": edge.authored_order,
            "back_to_front_index": edge.back_to_front_index,
            "layer_count": edge.layer_count,
            "authored_blend_mode": edge.authored_blend_mode,
            "authored_blend_available": edge.authored_blend_available,
            "runtime_first_produced_may_be_normal": edge.authored_blend_available,
            "unclipped_rect": qa_rect_metadata(unclipped_bbox),
            "hit_point": {"x": midpoint.x, "y": midpoint.y},
        })),
    );
    if let Some(connection_id) = connection_id {
        for (suffix, role, position) in [
            ("from_handle", "source", start),
            ("to_handle", "target", end),
        ] {
            let unclipped_rect = egui::Rect::from_center_size(position, egui::vec2(18.0, 18.0));
            let rect = clipped_qa_rect(unclipped_rect, canvas_clip);
            crate::qa::register_component_with_metadata(
                format!("node_editor.edge:{connection_id}.{suffix}"),
                "node_edge_endpoint",
                rect,
                true,
                Some(edge_endpoint_qa_metadata(
                    connection_id,
                    role,
                    position,
                    unclipped_rect,
                )),
            );
        }
    }
    Some(RenderedEdge {
        kind: edge.kind,
        start,
        control_a,
        control_b,
        end,
    })
}
