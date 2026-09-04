use egui::Rect;

use crate::wire::ReconnectEndpoint;
use crate::GraphFrame;
use crate::ItemId;

use super::{Gesture, InteractionState};

pub(super) fn paint<NodeId, PortId, WireId, GroupId, Key>(
    ui: &egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &InteractionState<NodeId, PortId, WireId, GroupId>,
) where
    PortId: Eq,
    WireId: Eq,
{
    let painter = ui.painter().with_clip_rect(frame.viewport);
    paint_reconnect_handles(&painter, frame);
    match state.gesture.as_ref() {
        Some(Gesture::Marquee { start, current, .. }) => {
            painter.rect(
                Rect::from_two_pos(*start, *current).intersect(frame.viewport),
                0.0,
                egui::Color32::from_rgba_premultiplied(76, 146, 255, 30),
                egui::Stroke::new(1.0, egui::Color32::from_rgb(105, 165, 255)),
                egui::StrokeKind::Inside,
            );
        }
        Some(Gesture::Connect {
            from,
            current,
            transform,
        }) => {
            if let Some(port) = frame.ports.iter().find(|port| port.id == *from) {
                painter.line_segment(
                    [*transform * port.center, *current],
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(110, 174, 255)),
                );
            }
        }
        Some(Gesture::Reconnect {
            wire,
            endpoint,
            current,
            ..
        }) => {
            if let Some(wire) = frame.wires.iter().find(|candidate| candidate.id == *wire) {
                let fixed_graph = wire.curve.endpoint(match endpoint {
                    ReconnectEndpoint::Source => ReconnectEndpoint::Target,
                    ReconnectEndpoint::Target => ReconnectEndpoint::Source,
                });
                let fixed = frame.screen_position(fixed_graph);
                let (start, end) = match endpoint {
                    ReconnectEndpoint::Source => (*current, fixed),
                    ReconnectEndpoint::Target => (fixed, *current),
                };
                let handle = ((end.x - start.x).abs() * 0.5).max(48.0);
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [
                        start,
                        start + egui::vec2(handle, 0.0),
                        end - egui::vec2(handle, 0.0),
                        end,
                    ],
                    false,
                    egui::Color32::TRANSPARENT,
                    egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 196, 72)),
                ));
            }
        }
        Some(Gesture::Hold { .. })
        | Some(Gesture::Move { .. })
        | Some(Gesture::Resize { .. })
        | Some(Gesture::LayoutSwipe(_))
        | None => {}
    }
}

fn paint_reconnect_handles<NodeId, PortId, WireId, GroupId, Key>(
    painter: &egui::Painter,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
) where
    WireId: Eq,
{
    const HANDLE_RADIUS: f32 = 4.0;
    let selected = match frame.selection.primary.as_ref() {
        Some(ItemId::Wire(wire)) => Some(wire),
        _ => frame.selection.items.iter().find_map(|item| match item {
            ItemId::Wire(wire) => Some(wire),
            ItemId::Node(_) | ItemId::Group(_) => None,
        }),
    };
    let Some(wire) = selected.and_then(|selected| {
        frame
            .wires
            .iter()
            .find(|wire| wire.editable && wire.id == *selected)
    }) else {
        return;
    };
    let scale = frame.transform.scaling.abs().max(f32::EPSILON);
    let geometry = wire.curve.interaction_geometry(scale);
    for endpoint in [ReconnectEndpoint::Source, ReconnectEndpoint::Target] {
        let center = frame.screen_position(geometry.reconnect_handle(endpoint));
        painter.circle_filled(center, HANDLE_RADIUS, egui::Color32::from_rgb(38, 38, 42));
        painter.circle_stroke(
            center,
            HANDLE_RADIUS,
            egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 196, 72)),
        );
    }
}
