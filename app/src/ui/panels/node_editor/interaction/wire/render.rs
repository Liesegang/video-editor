use eframe::egui::{self, Color32};
use library::model::project::{PortAddress, PortDataType, PortDirection, PortOwner, TIME_PORT};
use library::model::Project;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::container_outputs::register_container_output_edges;
use super::hit::cubic_bezier_point;
#[cfg(test)]
use crate::ui::panels::node_editor::capture_test_rect;
use crate::ui::panels::node_editor::{
    blend_mode_qa_key, clipped_qa_rect, connection_supports_authored_blend,
    container_highlight_metadata, container_inactive, container_output_node_id,
    container_output_type_key, container_visual_style, edge_endpoint_qa_metadata,
    native_variadic_merge_target, overview_wire_graph_points, pin_color, qa_container_key,
    qa_rect_metadata, reconnect_handle_position, screen_stroke_in_graph_units,
    wire_order_menu_states, ContainerKind, ContainerVisual, EdgeComponent, OverviewWirePainter,
    RenderedEdge, RenderedEdgeKind, RenderedPortKey, WIRE_RECONNECT_HANDLE_RADIUS,
};
use crate::ui::panels::time_context::{time_source_state, TimeSourceState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) struct TimeContextNode {
    pub(in crate::ui::panels::node_editor) node_id: Uuid,
    pub(in crate::ui::panels::node_editor) selected: bool,
    pub(in crate::ui::panels::node_editor) hovered: bool,
}

pub(in crate::ui::panels::node_editor) fn register_container_chrome(
    container: &ContainerVisual,
    to_global: egui::emath::TSTransform,
    canvas_clip: egui::Rect,
    project: &Project,
    current_time: f64,
    selected: bool,
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
    let highlight_metadata = container_highlight_metadata(container_visual_style(
        container.kind,
        container_inactive(project, container.owner, current_time),
        selected,
        to_global.scaling,
    ));
    #[cfg(test)]
    {
        capture_test_rect(&main_id, main);
        crate::ui::panels::node_editor::capture_test_metadata(
            &main_id,
            &serde_json::json!({
                "owner": owner,
                "selected": selected,
                "highlight_style": highlight_metadata,
            }),
        );
    }
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
            "selected": selected,
            "highlight_style": highlight_metadata,
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
        let physical_merge_target = native_variadic_merge_target(project, &connection.to).is_some();
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
                physical_merge_target,
                authored_blend_mode: authored_blend_available
                    .then(|| blend_mode_qa_key(connection.blend_mode)),
                authored_blend_available,
                runtime_first_produced_may_be_normal: authored_blend_available
                    && connection
                        .blend_mode
                        .can_optimize_empty_backdrop_to_normal(),
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

/// Paint transient runtime context without adding an editable/rendered edge.
/// The returned count is diagnostic only; callers must keep `rendered_edges`
/// restricted to physical authored/binding/derived wires.
pub(in crate::ui::panels::node_editor) fn register_implicit_time_context_wires(
    project: &Project,
    rendered_ports: &Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
    nodes: &[TimeContextNode],
    canvas_clip: egui::Rect,
    painter: &egui::Painter,
) -> usize {
    let Ok(ports) = rendered_ports.lock() else {
        return 0;
    };
    let mut rendered_count = 0;
    for node in nodes {
        let Some(state) = time_source_state(project, PortOwner::Node(node.node_id)) else {
            continue;
        };
        let TimeSourceState::Inherited { from } = &state else {
            continue;
        };
        let target = PortAddress::new(PortOwner::Node(node.node_id), TIME_PORT);
        let Some(from_rect) = ports.get(&RenderedPortKey {
            address: from.clone(),
            direction: PortDirection::Output,
            connection_id: None,
        }) else {
            continue;
        };
        let Some(to_rect) = ports.get(&RenderedPortKey {
            address: target.clone(),
            direction: PortDirection::Input,
            connection_id: None,
        }) else {
            continue;
        };
        let start = from_rect.center();
        let end = to_rect.center();
        if ![start, end]
            .iter()
            .all(|position| position.x.is_finite() && position.y.is_finite())
        {
            continue;
        }
        let frame = ((end.x - start.x).abs() * 0.45).clamp(36.0, 110.0);
        let control_a = start + egui::vec2(frame, 0.0);
        let control_b = end - egui::vec2(frame, 0.0);
        let points = (0..=24)
            .map(|sample| {
                cubic_bezier_point(start, control_a, control_b, end, sample as f32 / 24.0)
            })
            .collect::<Vec<_>>();
        let unclipped_bbox =
            egui::Rect::from_points(&[start, control_a, control_b, end]).expand(6.0);
        let bbox = clipped_qa_rect(unclipped_bbox, canvas_clip);
        if bbox.is_positive() {
            painter.extend(egui::Shape::dashed_line(
                &points,
                egui::Stroke::new(1.35, pin_color(PortDataType::Number).gamma_multiply(0.62)),
                6.0,
                4.0,
            ));
        }

        let component_id = format!("node_editor.time_context_wire.node:{}", node.node_id);
        let mut metadata = state.qa_metadata(PortOwner::Node(node.node_id));
        if let Some(metadata) = metadata.as_object_mut() {
            metadata.insert("kind".to_string(), "implicit_time".into());
            metadata.insert("selected".to_string(), node.selected.into());
            metadata.insert("hovered".to_string(), node.hovered.into());
            metadata.insert("dashed".to_string(), true.into());
            metadata.insert("trigger".to_string(), "hold_key".into());
            metadata.insert("held".to_string(), true.into());
            metadata.insert(
                "key".to_string(),
                crate::ui::panels::node_editor::IMPLICIT_TIME_OVERLAY_KEY_LABEL.into(),
            );
            metadata.insert("reveal_gesture".to_string(), "hold".into());
            metadata.insert(
                "reveal_key".to_string(),
                crate::ui::panels::node_editor::IMPLICIT_TIME_OVERLAY_KEY_LABEL.into(),
            );
            metadata.insert("hit_testable".to_string(), false.into());
            metadata.insert("wire_collection".to_string(), "context_only".into());
            metadata.insert("visible".to_string(), bbox.is_positive().into());
            metadata.insert(
                "from".to_string(),
                serde_json::json!({
                    "owner": qa_container_key(from.owner),
                    "port": from.port,
                    "x": start.x,
                    "y": start.y,
                }),
            );
            metadata.insert(
                "to".to_string(),
                serde_json::json!({
                    "owner": qa_container_key(target.owner),
                    "port": target.port,
                    "x": end.x,
                    "y": end.y,
                }),
            );
            metadata.insert(
                "unclipped_rect".to_string(),
                qa_rect_metadata(unclipped_bbox),
            );
        }
        #[cfg(test)]
        {
            capture_test_rect(&component_id, bbox);
            crate::ui::panels::node_editor::capture_test_metadata(&component_id, &metadata);
        }
        crate::qa::register_component_with_metadata(
            component_id,
            "node_time_context_wire",
            bbox,
            false,
            Some(metadata),
        );
        rendered_count += 1;
    }
    rendered_count
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
        connection_id: None,
    })?;
    let exact_connection_id = edge
        .kind
        .connection_id()
        .filter(|_| edge.physical_merge_target);
    let to_rect = ports
        .get(&RenderedPortKey {
            address: edge.to.clone(),
            direction: PortDirection::Input,
            connection_id: exact_connection_id,
        })
        .or_else(|| {
            ports.get(&RenderedPortKey {
                address: edge.to.clone(),
                direction: PortDirection::Input,
                connection_id: None,
            })
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
            overview
                .painter
                .add(egui::epaint::CubicBezierShape::from_points_stroke(
                    graph_points,
                    false,
                    Color32::TRANSPARENT,
                    egui::Stroke::new(
                        screen_stroke_in_graph_units(1.65, overview.to_global.scaling),
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
    let action = match edge.kind {
        RenderedEdgeKind::ProjectConnection { .. } => Some("select_or_edit"),
        RenderedEdgeKind::OutputBinding { .. } => Some("delete_output_binding"),
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
            "from": {
                "owner": qa_container_key(edge.from.owner),
                "port": edge.from.port,
                "x": start.x,
                "y": start.y,
            },
            "control_a": {"x": control_a.x, "y": control_a.y},
            "control_b": {"x": control_b.x, "y": control_b.y},
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
            "runtime_first_produced_may_be_normal": edge
                .runtime_first_produced_may_be_normal,
            "physical_variadic_endpoint": exact_connection_id.is_some(),
            "unclipped_rect": qa_rect_metadata(unclipped_bbox),
            "hit_point": {"x": midpoint.x, "y": midpoint.y},
        })),
    );
    if let Some(connection_id) = connection_id {
        let rendered_edge = RenderedEdge {
            kind: edge.kind,
            start,
            control_a,
            control_b,
            end,
        };
        for (suffix, role, kind) in [
            (
                "from_handle",
                "source",
                crate::state::context_types::NodeEditorWireDragKind::ReconnectSource,
            ),
            (
                "to_handle",
                "target",
                crate::state::context_types::NodeEditorWireDragKind::ReconnectTarget,
            ),
        ] {
            let Some(position) = reconnect_handle_position(&rendered_edge, kind) else {
                continue;
            };
            let unclipped_rect = egui::Rect::from_center_size(
                position,
                egui::Vec2::splat(WIRE_RECONNECT_HANDLE_RADIUS * 2.0),
            );
            let rect = clipped_qa_rect(unclipped_rect, canvas_clip);
            let component_id = format!("node_editor.edge:{connection_id}.{suffix}");
            #[cfg(test)]
            capture_test_rect(&component_id, rect);
            crate::qa::register_component_with_metadata(
                component_id,
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
