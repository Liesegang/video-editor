use crate::state::context_types::{
    NodeEditorEditableWire, NodeEditorNormalConnectGesture, NodeEditorState,
    NodeEditorWireDragKind, NodeEditorWireGesture, NodeEditorWireKnifeGesture,
};
use eframe::egui::{self, Color32};
use library::model::project::PortDirection;
use library::model::Project;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::ui::panels::node_editor::{
    editable_wire_qa_value, editable_wire_sort_key, knife_segment_hits_edge,
    node_editor_port_interactions_enabled, qa_container_key, rendered_edge_at_position,
    rendered_normal_port_at_position, rendered_port_at_position, rendered_wire_drag_kind, NodeEdit,
    QueuedNodeEdit, RenderedEdge, RenderedEdgeKind, RenderedPortKey, WIRE_DRAG_THRESHOLD,
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
    if state.selected_connection_id.is_some_and(|connection_id| {
        !frame
            .project
            .connections
            .iter()
            .any(|connection| connection.id == connection_id)
    }) {
        state.selected_connection_id = None;
    }
    if state.wire_gesture.as_ref().is_some_and(|gesture| {
        !frame
            .project
            .connections
            .iter()
            .any(|connection| connection.id == gesture.connection_id)
    }) {
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
            || !frame
                .project
                .connections
                .iter()
                .any(|connection| connection.from == gesture.from)
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
        return target.map_or_else(Vec::new, |to| {
            vec![QueuedNodeEdit::Atomic(NodeEdit::Connect {
                from: gesture.from,
                to,
            })]
        });
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
                "Container output binding. Right-click to clear it, or cross it with the Alt knife.",
            ),
            RenderedEdgeKind::DerivedOutput { .. } => edge.kind.blocked_reason(),
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
            if matches!(edge.kind, RenderedEdgeKind::DerivedOutput { .. }) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
            }
        }
    }

    let normal_port = node_editor_port_interactions_enabled(frame.to_global.scaling)
        .then(|| {
            pointer.and_then(|position| {
                frame.rendered_ports.lock().ok().and_then(|ports| {
                    rendered_normal_port_at_position(&ports, position, frame.canvas_clip)
                })
            })
        })
        .flatten();
    let pointer_on_normal_port = normal_port.is_some();
    let hovered = pointer
        .filter(|_| !pointer_on_normal_port)
        .and_then(|position| {
            let edge = rendered_edge_at_position(frame.edges, position)?;
            edge.kind.connection_id()?;
            let endpoint = rendered_wire_drag_kind(edge, position);
            let graph_position = frame.to_global.inverse() * position;
            let over_graph_item = frame
                .graph_item_rects
                .iter()
                .any(|rect| rect.contains(graph_position));
            (!over_graph_item || endpoint != NodeEditorWireDragKind::Disconnect).then_some(edge)
        });
    if primary_pressed && hovered.is_none() {
        state.selected_connection_id = None;
    }
    if let (true, Some(port), Some(position)) = (primary_pressed, normal_port, pointer) {
        if port.direction == PortDirection::Output
            && frame
                .project
                .connections
                .iter()
                .any(|connection| connection.from == port.address)
        {
            state.normal_connect_gesture = Some(NodeEditorNormalConnectGesture {
                from: port.address,
                start: position,
                current: position,
                canvas_transform: frame.to_global,
            });
        } else {
            // Snarl has already received this frame's press. Preserve that
            // ownership until release so foreground wire surfaces cannot
            // claim the rest of the physical gesture.
            state.normal_wire_drag_active = true;
        }
        return Vec::new();
    }

    let knife_was_active = state.wire_knife.is_some();
    let knife_edits = wire_knife_interaction(ui, state, &frame);
    if knife_was_active || state.wire_knife.is_some() || !knife_edits.is_empty() {
        return knife_edits;
    }
    // At detail zoom the exact pin center belongs to Snarl's normal
    // connection gesture. A connected port is also the endpoint of an
    // existing curve; letting the foreground reconnect hit win there makes
    // fan-out impossible. The surrounding wire endpoint radius remains the
    // reconnect target, and overview mode still uses custom endpoint hits.
    let active_id = state
        .wire_gesture
        .as_ref()
        .map(|gesture| gesture.connection_id);
    let interaction_edge = active_id
        .and_then(|connection_id| {
            frame
                .edges
                .iter()
                .find(|edge| edge.kind.connection_id() == Some(connection_id))
        })
        .or(hovered);
    let mut edits = Vec::new();

    if let Some(edge) = interaction_edge {
        let Some(connection_id) = edge.kind.connection_id() else {
            return edits;
        };
        let response = ui.interact(
            frame.canvas_clip,
            ui.make_persistent_id(("node_editor_wire", connection_id)),
            egui::Sense::click_and_drag(),
        );
        if response.clicked_by(egui::PointerButton::Primary) {
            state.selected_connection_id = Some(connection_id);
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
        let pointer_started_on_edge = hovered
            .is_some_and(|hovered_edge| hovered_edge.kind.connection_id() == Some(connection_id));
        if primary_pressed && pointer_started_on_edge {
            if let Some(position) = pointer_position {
                state.selected_connection_id = Some(connection_id);
                state.wire_context_menu = None;
                state.wire_gesture = Some(NodeEditorWireGesture {
                    connection_id,
                    kind: rendered_wire_drag_kind(edge, position),
                    start: position,
                    current: position,
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
                if gesture.connection_id == connection_id
                    && gesture.current.distance(gesture.start) >= WIRE_DRAG_THRESHOLD
                {
                    match gesture.kind {
                        NodeEditorWireDragKind::Disconnect => {
                            edits.push(QueuedNodeEdit::Atomic(NodeEdit::DisconnectConnection {
                                connection_id: gesture.connection_id,
                            }));
                        }
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
                            if let (Some(port), Some(connection)) = (
                                ports,
                                frame
                                    .project
                                    .connections
                                    .iter()
                                    .find(|connection| connection.id == gesture.connection_id),
                            ) {
                                let (from, to) =
                                    if endpoint_kind == NodeEditorWireDragKind::ReconnectSource {
                                        (port, connection.to.clone())
                                    } else {
                                        (connection.from.clone(), port)
                                    };
                                edits.push(QueuedNodeEdit::Atomic(NodeEdit::ReconnectConnection {
                                    connection_id: gesture.connection_id,
                                    from,
                                    to,
                                }));
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

    if let Some(connection_id) = state.selected_connection_id {
        if let Some(edge) = frame
            .edges
            .iter()
            .find(|edge| edge.kind.connection_id() == Some(connection_id))
        {
            paint_wire_interaction(ui, edge, state.wire_gesture.as_ref(), frame.canvas_clip);
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
