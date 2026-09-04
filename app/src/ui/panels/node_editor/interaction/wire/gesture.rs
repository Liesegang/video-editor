use crate::state::context_types::{
    NodeEditorEditableWire, NodeEditorNormalConnectGesture, NodeEditorState,
    NodeEditorWireDragKind, NodeEditorWireGesture, NodeEditorWireKnifeGesture,
};
use eframe::egui::{self, Color32};
use library::model::Project;
use library::model::project::{PortAddress, PortDirection, PortOwner};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::model::edit_for_port_addresses;

use crate::ui::panels::node_editor::{
    NodeEdit, QueuedNodeEdit, RenderedEdge, RenderedEdgeKind, RenderedPortKey, WIRE_DRAG_THRESHOLD,
    WIRE_RECONNECT_HANDLE_RADIUS, container_output_binding_port, container_output_port,
    editable_wire_is_current, editable_wire_qa_value, editable_wire_sort_key,
    editable_wire_stable_key, knife_segment_hits_edge, node_editor_port_interactions_enabled,
    qa_container_key, reconnect_handle_at_position, reconnect_handle_position,
    rendered_container_output_at_position, rendered_edge_at_position,
    rendered_normal_port_at_position, rendered_port_at_position, rendered_wire_drag_kind,
};

pub(in crate::ui::panels::node_editor) struct WireInteractionFrame<'a> {
    pub(in crate::ui::panels::node_editor) project: &'a Project,
    pub(in crate::ui::panels::node_editor) edges: &'a [RenderedEdge],
    pub(in crate::ui::panels::node_editor) rendered_ports:
        &'a Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
    pub(in crate::ui::panels::node_editor) canvas_clip: egui::Rect,
    pub(in crate::ui::panels::node_editor) graph_item_rects: &'a [egui::Rect],
    pub(in crate::ui::panels::node_editor) to_global: egui::emath::TSTransform,
}

fn output_binding_edge_for_port<'a>(
    edges: &'a [RenderedEdge],
    port: &RenderedPortKey,
) -> Option<&'a RenderedEdge> {
    if port.direction != PortDirection::Output {
        return None;
    }
    let PortOwner::Node(port_node_id) = port.address.owner else {
        return None;
    };
    edges.iter().find(|edge| {
        matches!(
            edge.kind,
            RenderedEdgeKind::OutputBinding {
                node_id,
                data_type,
                ..
            } if node_id == port_node_id
                && container_output_port(data_type) == Some(port.address.port.as_str())
        )
    })
}

fn reconnect_edit(
    project: &Project,
    wire: NodeEditorEditableWire,
    endpoint: NodeEditorWireDragKind,
    port: PortAddress,
) -> Option<NodeEdit> {
    match wire {
        NodeEditorEditableWire::ProjectConnection { connection_id } => {
            let connection = project
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)?;
            let (from, to) = match endpoint {
                NodeEditorWireDragKind::ReconnectSource => (port, connection.to.clone()),
                NodeEditorWireDragKind::ReconnectTarget => (connection.from.clone(), port),
                NodeEditorWireDragKind::Disconnect => return None,
            };
            Some(NodeEdit::ReconnectConnection {
                connection_id,
                from,
                to,
            })
        }
        NodeEditorEditableWire::OutputBinding {
            owner, data_type, ..
        } => {
            // The container sink is structural and cannot move. Reconnecting
            // the source atomically replaces only this owner's typed binding.
            if endpoint != NodeEditorWireDragKind::ReconnectSource {
                return None;
            }
            let binding_port = container_output_binding_port(data_type)?;
            edit_for_port_addresses(project, port, PortAddress::new(owner, binding_port), true)
        }
    }
}

fn paint_wire_knife(ui: &egui::Ui, gesture: &NodeEditorWireKnifeGesture, canvas_clip: egui::Rect) {
    if gesture.points.len() < 2 {
        return;
    }
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("node_editor_wire_knife"),
        ))
        .with_clip_rect(canvas_clip);
    painter.add(egui::Shape::line(
        gesture.points.clone(),
        egui::Stroke::new(2.5, Color32::from_rgb(255, 88, 88)),
    ));
}

pub(in crate::ui::panels::node_editor) fn wire_knife_interaction(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
    frame: &WireInteractionFrame<'_>,
) -> Vec<QueuedNodeEdit> {
    crate::qa::register_component_with_metadata(
        "node_editor.knife_surface",
        "node_editor_knife_surface",
        frame.canvas_clip,
        true,
        Some(serde_json::json!({
            "action": "knife",
            "gesture": "alt_primary_drag_from_empty_canvas",
            "active": state.wire_knife.is_some(),
            "canvas_transform": {
                "scale": frame.to_global.scaling,
                "translation": {
                    "x": frame.to_global.translation.x,
                    "y": frame.to_global.translation.y,
                },
            },
        })),
    );
    let (primary_pressed, primary_down, primary_released, pointer, alt) = ui.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.pointer.primary_released(),
            input.pointer.interact_pos(),
            input.modifiers.alt,
        )
    });

    if state.wire_knife.is_none() && alt && primary_pressed {
        if let Some(position) = pointer.filter(|position| frame.canvas_clip.contains(*position)) {
            let graph_position = frame.to_global.inverse() * position;
            let over_item = frame
                .graph_item_rects
                .iter()
                .any(|rect| rect.contains(graph_position));
            let over_wire = rendered_edge_at_position(frame.edges, position).is_some();
            if !over_item && !over_wire {
                state.wire_gesture = None;
                state.wire_context_menu = None;
                state.wire_knife = Some(NodeEditorWireKnifeGesture {
                    points: vec![position],
                    crossed_wires: HashSet::new(),
                    canvas_transform: frame.to_global,
                });
            }
        }
    }

    if primary_down {
        if let (Some(position), Some(gesture)) = (pointer, state.wire_knife.as_mut()) {
            let previous = gesture.points.last().copied().unwrap_or(position);
            if previous.distance(position) > 0.5 {
                for edge in frame.edges {
                    if knife_segment_hits_edge(previous, position, edge) {
                        if let Some(target) = edge.kind.editable_wire() {
                            gesture.crossed_wires.insert(target);
                        }
                    }
                }
                gesture.points.push(position);
                ui.ctx().request_repaint();
            }
        }
    }

    if let Some(gesture) = state.wire_knife.as_ref() {
        paint_wire_knife(ui, gesture, frame.canvas_clip);
        let rect = egui::Rect::from_points(&gesture.points).expand(6.0);
        let mut crossed_wires = gesture.crossed_wires.iter().copied().collect::<Vec<_>>();
        crossed_wires.sort_by_key(|target| editable_wire_sort_key(*target));
        let crossed_connection_ids = crossed_wires
            .iter()
            .filter_map(|target| match target {
                NodeEditorEditableWire::ProjectConnection { connection_id } => Some(*connection_id),
                NodeEditorEditableWire::OutputBinding { .. } => None,
            })
            .collect::<Vec<_>>();
        crate::qa::register_component_with_metadata(
            "node_editor.knife_gesture",
            "node_editor_knife_gesture",
            rect,
            true,
            Some(serde_json::json!({
                "action": "knife",
                "crossed_wires": crossed_wires
                    .iter()
                    .copied()
                    .map(editable_wire_qa_value)
                    .collect::<Vec<_>>(),
                "crossed_connection_ids": crossed_connection_ids,
                "point_count": gesture.points.len(),
                "start_accepted": true,
                "canvas_transform": {
                    "scale": frame.to_global.scaling,
                    "translation": {
                        "x": frame.to_global.translation.x,
                        "y": frame.to_global.translation.y,
                    },
                },
            })),
        );
    }

    if primary_released {
        if let Some(gesture) = state.wire_knife.take() {
            let mut wires = gesture.crossed_wires.into_iter().collect::<Vec<_>>();
            wires.sort_by_key(|target| editable_wire_sort_key(*target));
            if !wires.is_empty() {
                if state.selected_connection_id.is_some_and(|selected| {
                    wires.contains(&NodeEditorEditableWire::ProjectConnection {
                        connection_id: selected,
                    })
                }) {
                    state.selected_connection_id = None;
                }
                return vec![QueuedNodeEdit::Atomic(NodeEdit::DisconnectWires { wires })];
            }
        }
    }
    Vec::new()
}

fn paint_wire_interaction(
    ui: &egui::Ui,
    edge: &RenderedEdge,
    gesture: Option<&NodeEditorWireGesture>,
    canvas_clip: egui::Rect,
) {
    let mut points = [edge.start, edge.control_a, edge.control_b, edge.end];
    if let Some(gesture) = gesture {
        match gesture.kind {
            NodeEditorWireDragKind::ReconnectSource => {
                points[0] = gesture.current;
                points[1] = gesture.current + egui::vec2(72.0, 0.0);
            }
            NodeEditorWireDragKind::ReconnectTarget => {
                points[2] = gesture.current - egui::vec2(72.0, 0.0);
                points[3] = gesture.current;
            }
            NodeEditorWireDragKind::Disconnect => {}
        }
    }
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("node_editor_wire_interaction"),
        ))
        .with_clip_rect(canvas_clip);
    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
        points,
        false,
        Color32::TRANSPARENT,
        egui::Stroke::new(5.0, Color32::from_rgb(255, 196, 72)),
    ));
    let displayed = RenderedEdge {
        kind: edge.kind,
        start: points[0],
        control_a: points[1],
        control_b: points[2],
        end: points[3],
    };
    for kind in [
        NodeEditorWireDragKind::ReconnectSource,
        NodeEditorWireDragKind::ReconnectTarget,
    ] {
        if let Some(center) = reconnect_handle_position(&displayed, kind) {
            painter.circle_filled(
                center,
                WIRE_RECONNECT_HANDLE_RADIUS,
                Color32::from_rgb(38, 38, 42),
            );
            painter.circle_stroke(
                center,
                WIRE_RECONNECT_HANDLE_RADIUS,
                egui::Stroke::new(1.5, Color32::from_rgb(255, 196, 72)),
            );
        }
    }
}

fn paint_normal_connect_interaction(
    ui: &egui::Ui,
    gesture: &NodeEditorNormalConnectGesture,
    canvas_clip: egui::Rect,
) {
    let points = [
        gesture.start,
        gesture.start + egui::vec2(72.0, 0.0),
        gesture.current - egui::vec2(72.0, 0.0),
        gesture.current,
    ];
    let painter = ui
        .ctx()
        .layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("node_editor_normal_connect"),
        ))
        .with_clip_rect(canvas_clip);
    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
        points,
        false,
        Color32::TRANSPARENT,
        egui::Stroke::new(3.0, Color32::from_rgb(255, 196, 72)),
    ));
    crate::qa::register_component_with_metadata(
        "node_editor.normal_connect_gesture",
        "node_editor_normal_connect_gesture",
        egui::Rect::from_points(&points).expand(6.0),
        true,
        Some(serde_json::json!({
            "action": "fan_out",
            "from_owner": qa_container_key(gesture.from.owner),
            "from_port": gesture.from.port,
            "start": {"x": gesture.start.x, "y": gesture.start.y},
            "current": {"x": gesture.current.x, "y": gesture.current.y},
        })),
    );
}

pub(in crate::ui::panels::node_editor) fn wire_interactions(
    ui: &mut egui::Ui,
    state: &mut NodeEditorState,
    frame: WireInteractionFrame<'_>,
) -> Vec<QueuedNodeEdit> {
    if state.merge_layer_reorder.is_some() {
        state.wire_gesture = None;
        state.normal_connect_gesture = None;
        state.normal_wire_drag_active = false;
        return Vec::new();
    }
    if state.selected_connection_id.is_some_and(|connection_id| {
        !frame
            .project
            .connections
            .iter()
            .any(|connection| connection.id == connection_id)
    }) {
        state.selected_connection_id = None;
    }
    if state
        .wire_gesture
        .as_ref()
        .is_some_and(|gesture| !editable_wire_is_current(frame.project, gesture.wire))
    {
        state.wire_gesture = None;
    }

    let (primary_pressed, primary_down, primary_released, pointer, escape_pressed) =
        ui.input(|input| {
            (
                input.pointer.primary_pressed(),
                input.pointer.primary_down(),
                input.pointer.primary_released(),
                input.pointer.interact_pos(),
                input.key_pressed(egui::Key::Escape),
            )
        });
    if escape_pressed {
        state.selected_connection_id = None;
        state.wire_context_menu = None;
        if state.wire_gesture.take().is_some() {
            return Vec::new();
        }
    }
    if state.normal_connect_cancel_pending_release {
        if !primary_down {
            state.normal_connect_cancel_pending_release = false;
        }
        return Vec::new();
    }
    if let (Some(position), Some(gesture)) = (pointer, state.normal_connect_gesture.as_mut()) {
        gesture.current = position;
    }
    if state.normal_connect_gesture.is_some() {
        if primary_down && !escape_pressed {
            if let Some(gesture) = state.normal_connect_gesture.as_ref() {
                paint_normal_connect_interaction(ui, gesture, frame.canvas_clip);
            }
            ui.ctx().request_repaint();
            return Vec::new();
        }

        let Some(gesture) = state.normal_connect_gesture.take() else {
            return Vec::new();
        };
        if escape_pressed {
            state.normal_connect_cancel_pending_release = primary_down;
            return Vec::new();
        }
        if !primary_released
            || gesture.current.distance(gesture.start) < WIRE_DRAG_THRESHOLD
            || frame
                .project
                .port_definition(&gesture.from, PortDirection::Output)
                .is_none()
        {
            return Vec::new();
        }
        let target = frame.rendered_ports.lock().ok().and_then(|ports| {
            rendered_port_at_position(
                &ports,
                PortDirection::Input,
                gesture.current,
                frame.canvas_clip,
            )
        });
        return target
            .and_then(|to| edit_for_port_addresses(frame.project, gesture.from, to, true))
            .map_or_else(Vec::new, |edit| vec![QueuedNodeEdit::Atomic(edit)]);
    }
    if state.normal_wire_drag_active {
        if escape_pressed || !primary_down {
            state.normal_wire_drag_active = false;
        }
        return Vec::new();
    }

    // The foreground menu owns pointer input while open. A menu button can
    // overlap the underlying Bezier or pin, so it must win before either wire
    // gesture path claims the press.
    if state.wire_context_menu.is_some() {
        return Vec::new();
    }

    if let Some((position, edge)) = pointer.and_then(|position| {
        rendered_edge_at_position(frame.edges, position).map(|edge| (position, edge))
    }) {
        let hover_text = match edge.kind {
            RenderedEdgeKind::OutputBinding { .. } => Some(
                "Container output binding. Drag its source endpoint to rebind; right-click or use the Alt knife to clear it.",
            ),
            RenderedEdgeKind::ProjectConnection { .. } => None,
        };
        if let Some(hover_text) = hover_text {
            let hover_rect = egui::Rect::from_center_size(position, egui::vec2(2.0, 2.0));
            ui.interact(
                hover_rect,
                ui.make_persistent_id(("node_editor_wire_hover", edge.kind)),
                egui::Sense::hover(),
            )
            .on_hover_text(hover_text);
        }
    }

    let selected_handle = state.selected_connection_id.and_then(|connection_id| {
        let edge = frame
            .edges
            .iter()
            .find(|edge| edge.kind.connection_id() == Some(connection_id))?;
        pointer
            .and_then(|position| reconnect_handle_at_position(edge, position))
            .map(|kind| (edge, kind))
    });
    let selected_handle_edge = selected_handle.map(|(edge, _)| edge);
    let port_interactions_enabled = node_editor_port_interactions_enabled(frame.to_global.scaling);
    let priority_container_output = if selected_handle_edge.is_none() && port_interactions_enabled {
        pointer.and_then(|position| {
            frame.rendered_ports.lock().ok().and_then(|ports| {
                rendered_container_output_at_position(
                    frame.project,
                    &ports,
                    position,
                    frame.canvas_clip,
                )
            })
        })
    } else {
        None
    };
    let normal_port = if selected_handle_edge.is_some() {
        None
    } else {
        priority_container_output.clone().or_else(|| {
            port_interactions_enabled.then(|| {
                pointer.and_then(|position| {
                    frame.rendered_ports.lock().ok().and_then(|ports| {
                        rendered_normal_port_at_position(&ports, position, frame.canvas_clip)
                    })
                })
            })?
        })
    };
    let binding_on_normal_port = normal_port
        .as_ref()
        .filter(|port| {
            !frame
                .project
                .connections
                .iter()
                .any(|connection| connection.from == port.address)
        })
        .and_then(|port| output_binding_edge_for_port(frame.edges, port));
    let pointer_on_other_normal_port = normal_port.is_some() && binding_on_normal_port.is_none();
    let hovered = selected_handle_edge.or(binding_on_normal_port).or_else(|| {
        pointer
            .filter(|_| !pointer_on_other_normal_port)
            .and_then(|position| {
                let edge = rendered_edge_at_position(frame.edges, position)?;
                edge.kind.editable_wire()?;
                let endpoint = rendered_wire_drag_kind(edge, position);
                let graph_position = frame.to_global.inverse() * position;
                let over_graph_item = frame
                    .graph_item_rects
                    .iter()
                    .any(|rect| rect.contains(graph_position));
                (!over_graph_item || endpoint != NodeEditorWireDragKind::Disconnect).then_some(edge)
            })
    });
    if primary_pressed && hovered.is_none() {
        state.selected_connection_id = None;
    }
    if primary_pressed && binding_on_normal_port.is_none() {
        if let (Some(port), Some(position)) = (normal_port.as_ref(), pointer) {
            let custom_connect = port.direction == PortDirection::Output
                && (priority_container_output.is_some()
                    || frame
                        .project
                        .connections
                        .iter()
                        .any(|connection| connection.from == port.address));
            if custom_connect {
                // Snarl registers its broad node/container frame before it
                // renders the pin. Explicitly replace that potential drag ID
                // so the complete physical press/drag/release belongs to the
                // wire even when the press lands in the socket's QA/drop
                // padding rather than its smaller normal-start rectangle.
                ui.ctx().set_dragged_id(ui.make_persistent_id((
                    "node_editor_port_wire_owner",
                    qa_container_key(port.address.owner),
                    port.address.port.as_str(),
                )));
                state.container_resize = None;
                state.normal_connect_gesture = Some(NodeEditorNormalConnectGesture {
                    from: port.address.clone(),
                    start: position,
                    current: position,
                    canvas_transform: frame.to_global,
                });
            } else {
                // Snarl has already received this frame's press. Preserve that
                // ownership until release so foreground wire surfaces cannot
                // claim the rest of the physical gesture.
                state.container_resize = None;
                state.normal_wire_drag_active = true;
            }
            return Vec::new();
        }
    }

    let knife_was_active = state.wire_knife.is_some();
    let knife_edits = wire_knife_interaction(ui, state, &frame);
    if knife_was_active || state.wire_knife.is_some() || !knife_edits.is_empty() {
        return knife_edits;
    }
    // At detail zoom an explicit connected output pin keeps Snarl's normal
    // fan-out gesture. A container binding has no canonical ProjectConnection
    // to fan out, so its exact source pin belongs to typed endpoint rebind.
    let active_wire = state.wire_gesture.as_ref().map(|gesture| gesture.wire);
    let interaction_edge = active_wire
        .and_then(|wire| {
            frame
                .edges
                .iter()
                .find(|edge| edge.kind.editable_wire() == Some(wire))
        })
        .or(hovered);
    let mut edits = Vec::new();

    if let Some(edge) = interaction_edge {
        let Some(wire) = edge.kind.editable_wire() else {
            return edits;
        };
        let response = ui.interact(
            frame.canvas_clip,
            ui.make_persistent_id(("node_editor_wire", editable_wire_stable_key(wire))),
            egui::Sense::click_and_drag(),
        );
        if response.clicked_by(egui::PointerButton::Primary) {
            state.selected_connection_id = match wire {
                NodeEditorEditableWire::ProjectConnection { connection_id } => Some(connection_id),
                NodeEditorEditableWire::OutputBinding { .. } => None,
            };
        }
        let (primary_pressed, primary_down, primary_released, pointer_position) =
            ui.input(|input| {
                (
                    input.pointer.primary_pressed(),
                    input.pointer.primary_down(),
                    input.pointer.primary_released(),
                    input.pointer.interact_pos(),
                )
            });
        let pointer_started_on_edge =
            hovered.is_some_and(|hovered_edge| hovered_edge.kind.editable_wire() == Some(wire));
        if primary_pressed && pointer_started_on_edge {
            if let Some(position) = pointer_position {
                ui.ctx().set_dragged_id(ui.make_persistent_id((
                    "node_editor_wire_pointer_owner",
                    editable_wire_stable_key(wire),
                )));
                state.container_resize = None;
                state.selected_connection_id = match wire {
                    NodeEditorEditableWire::ProjectConnection { connection_id } => {
                        Some(connection_id)
                    }
                    NodeEditorEditableWire::OutputBinding { .. } => None,
                };
                state.wire_context_menu = None;
                state.wire_gesture = Some(NodeEditorWireGesture {
                    wire,
                    kind: selected_handle
                        .filter(|(handle_edge, _)| handle_edge.kind == edge.kind)
                        .map_or_else(|| rendered_wire_drag_kind(edge, position), |(_, kind)| kind),
                    start: position,
                    current: position,
                    canvas_transform: frame.to_global,
                });
            }
        }
        if primary_down {
            if let (Some(position), Some(gesture)) = (pointer_position, state.wire_gesture.as_mut())
            {
                gesture.current = position;
                ui.ctx().request_repaint();
            }
        }
        if primary_released {
            if let Some(gesture) = state.wire_gesture.take() {
                if gesture.wire == wire
                    && gesture.current.distance(gesture.start) >= WIRE_DRAG_THRESHOLD
                {
                    match gesture.kind {
                        NodeEditorWireDragKind::Disconnect => match gesture.wire {
                            NodeEditorEditableWire::ProjectConnection { connection_id } => {
                                edits.push(QueuedNodeEdit::Atomic(
                                    NodeEdit::DisconnectConnection { connection_id },
                                ));
                            }
                            NodeEditorEditableWire::OutputBinding { .. } => {
                                edits.push(QueuedNodeEdit::Atomic(NodeEdit::DisconnectWires {
                                    wires: vec![gesture.wire],
                                }));
                            }
                        },
                        endpoint_kind => {
                            let direction =
                                if endpoint_kind == NodeEditorWireDragKind::ReconnectSource {
                                    PortDirection::Output
                                } else {
                                    PortDirection::Input
                                };
                            let ports = frame.rendered_ports.lock().ok().and_then(|ports| {
                                rendered_port_at_position(
                                    &ports,
                                    direction,
                                    gesture.current,
                                    frame.canvas_clip,
                                )
                            });
                            if let Some(edit) = ports.and_then(|port| {
                                reconnect_edit(frame.project, gesture.wire, endpoint_kind, port)
                            }) {
                                edits.push(QueuedNodeEdit::Atomic(edit));
                            }
                        }
                    }
                }
            }
        }
    }

    let delete_pressed = ui.input(|input| {
        input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace)
    });
    if delete_pressed && !ui.ctx().wants_keyboard_input() {
        if let Some(connection_id) = state.selected_connection_id.take() {
            edits.push(QueuedNodeEdit::Atomic(NodeEdit::DisconnectConnection {
                connection_id,
            }));
        }
    }

    if let Some(gesture) = state.wire_gesture.as_ref() {
        if let Some(edge) = frame
            .edges
            .iter()
            .find(|edge| edge.kind.editable_wire() == Some(gesture.wire))
        {
            paint_wire_interaction(ui, edge, Some(gesture), frame.canvas_clip);
        }
    } else if let Some(connection_id) = state.selected_connection_id {
        if let Some(edge) = frame
            .edges
            .iter()
            .find(|edge| edge.kind.connection_id() == Some(connection_id))
        {
            paint_wire_interaction(ui, edge, None, frame.canvas_clip);
        }
    }
    edits
}

pub(in crate::ui::panels::node_editor) fn overview_wire_graph_points(
    screen_points: [egui::Pos2; 4],
    to_global: egui::emath::TSTransform,
) -> Option<[egui::Pos2; 4]> {
    if !to_global.scaling.is_finite()
        || to_global.scaling <= 0.0
        || !to_global.translation.x.is_finite()
        || !to_global.translation.y.is_finite()
    {
        return None;
    }
    let from_global = to_global.inverse();
    let graph_points = screen_points.map(|position| from_global * position);
    graph_points
        .iter()
        .all(|position| position.x.is_finite() && position.y.is_finite())
        .then_some(graph_points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::HistoryManager;
    use crate::test_support::media_node_for_canvas;
    use crate::ui::panels::node_editor::apply_queued_node_edits;
    use library::editor::project_service::MediaNodeRequest;
    use library::model::Clip;
    use library::model::asset::{Asset, AssetKind};
    use library::model::project::{AUDIO_OUTPUT_PORT, Composition, NodeContainer};

    fn add_audio_media(project: &mut Project, name: &str) -> uuid::Uuid {
        let asset = Asset::new(name, &format!("/fixture/{name}.wav"), AssetKind::Audio);
        let node = media_node_for_canvas(
            name,
            MediaNodeRequest::Audio {
                asset_id: asset.id,
                file_path: asset.path.clone(),
                audio_stream_index: None,
            },
            64,
            64,
            1,
            1,
        );
        let node_id = node.id;
        project.assets.push(asset);
        project.add_node(node);
        node_id
    }

    fn run_frames(
        project: &Project,
        edge: &RenderedEdge,
        rendered_ports: &Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
        state: &mut NodeEditorState,
        frames: Vec<Vec<egui::Event>>,
    ) -> Vec<QueuedNodeEdit> {
        let context = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 420.0));
        let mut queued = Vec::new();
        for (frame_number, events) in frames.into_iter().enumerate() {
            drop(context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame_number as f64 / 60.0),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        queued.extend(wire_interactions(
                            ui,
                            state,
                            WireInteractionFrame {
                                project,
                                edges: std::slice::from_ref(edge),
                                rendered_ports,
                                canvas_clip: screen,
                                graph_item_rects: &[],
                                to_global: egui::emath::TSTransform::IDENTITY,
                            },
                        ));
                    });
                },
            ));
        }
        queued
    }

    fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn audio_output_binding_source_endpoint_rebinds_in_one_undoable_gesture() {
        let mut project = Project::new("typed binding endpoint reconnect");
        let (composition, track) = Composition::new("Main", 64, 64, 24.0, 2.0);
        let track_id = track.id;
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        let clip = Clip::new("Audio", 0.0, 2.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let original_id = add_audio_media(&mut project, "original");
        let replacement_id = add_audio_media(&mut project, "replacement");
        for node_id in [original_id, replacement_id] {
            project
                .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
                .unwrap();
        }
        project
            .set_audio_output_node(NodeContainer::Clip(clip_id), Some(original_id))
            .unwrap();
        let initial = project.clone();

        let source = egui::pos2(120.0, 180.0);
        let replacement = egui::pos2(480.0, 260.0);
        let edge = RenderedEdge {
            kind: RenderedEdgeKind::OutputBinding {
                owner: PortOwner::Clip(clip_id),
                node_id: original_id,
                data_type: library::model::project::PortDataType::Audio,
            },
            start: source,
            control_a: egui::pos2(200.0, 180.0),
            control_b: egui::pos2(300.0, 180.0),
            end: egui::pos2(380.0, 180.0),
        };
        let rendered_ports = Arc::new(Mutex::new(HashMap::from([
            (
                RenderedPortKey {
                    address: PortAddress::new(PortOwner::Node(original_id), AUDIO_OUTPUT_PORT),
                    direction: PortDirection::Output,
                    connection_id: None,
                },
                egui::Rect::from_center_size(source, egui::vec2(14.0, 14.0)),
            ),
            (
                RenderedPortKey {
                    address: PortAddress::new(PortOwner::Node(replacement_id), AUDIO_OUTPUT_PORT),
                    direction: PortDirection::Output,
                    connection_id: None,
                },
                egui::Rect::from_center_size(replacement, egui::vec2(14.0, 14.0)),
            ),
        ])));
        let mut state = NodeEditorState::default();
        let edits = run_frames(
            &project,
            &edge,
            &rendered_ports,
            &mut state,
            vec![
                vec![egui::Event::PointerMoved(source)],
                vec![pointer_button(source, true)],
                vec![egui::Event::PointerMoved(replacement)],
                vec![pointer_button(replacement, false)],
            ],
        );
        assert!(matches!(
            edits.as_slice(),
            [QueuedNodeEdit::Atomic(NodeEdit::SetAudioOutputNode {
                owner: PortOwner::Clip(owner),
                node_id: Some(node_id),
            })] if *owner == clip_id && *node_id == replacement_id
        ));
        assert!(state.wire_gesture.is_none());

        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        assert!(apply_queued_node_edits(
            &mut project,
            edits,
            &mut history,
            &mut state,
        ));
        assert_eq!(
            project.get_clip(clip_id).unwrap().audio_output_node_id,
            Some(replacement_id)
        );
        let edited = project.clone();
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(&edited), Some(initial.clone()));
        assert_eq!(history.redo(&initial), Some(edited));
    }
}
