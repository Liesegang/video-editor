use egui::Rect;

use crate::GraphFrame;

use super::{Gesture, InteractionState};

pub(super) fn paint<NodeId, PortId, WireId, GroupId, Key>(
    ui: &egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &InteractionState<NodeId, PortId, WireId, GroupId>,
) where
    PortId: Eq,
{
    let painter = ui.painter().with_clip_rect(frame.viewport);
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
        Some(Gesture::Hold { .. })
        | Some(Gesture::Move { .. })
        | Some(Gesture::Resize { .. })
        | Some(Gesture::LayoutSwipe(_))
        | None => {}
    }
}
