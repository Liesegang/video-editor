use eframe::egui;
use library::model::project::PortOwner;
use library::model::Project;
use uuid::Uuid;

use crate::ui::panels::node_editor::{GraphItem, PortAnchorKind, WIRE_PORT_DROP_RADIUS};

pub(in crate::ui::panels::node_editor) fn qa_container_key(owner: PortOwner) -> String {
    match owner {
        PortOwner::Composition(id) => format!("composition:{id}"),
        PortOwner::Track(id) => format!("track:{id}"),
        PortOwner::Clip(id) => format!("clip:{id}"),
        PortOwner::Node(id) => format!("node:{id}"),
    }
}

/// Return the part of `rect` that can actually be seen and clicked in the
/// Node Editor canvas. Fully clipped components collapse to a zero-area point
/// on the nearest canvas edge so the QA registry reports them as invisible
/// without publishing negative dimensions.
pub(in crate::ui::panels::node_editor) fn clipped_qa_rect(
    rect: egui::Rect,
    canvas_clip: egui::Rect,
) -> egui::Rect {
    let intersection = rect.intersect(canvas_clip);
    if intersection.is_positive() {
        intersection
    } else {
        let point = egui::pos2(
            rect.center()
                .x
                .clamp(canvas_clip.left(), canvas_clip.right()),
            rect.center()
                .y
                .clamp(canvas_clip.top(), canvas_clip.bottom()),
        );
        egui::Rect::from_min_max(point, point)
    }
}

pub(in crate::ui::panels::node_editor) fn wire_port_drop_rect(
    rendered_port_rect: egui::Rect,
) -> egui::Rect {
    rendered_port_rect.expand(WIRE_PORT_DROP_RADIUS)
}

pub(in crate::ui::panels::node_editor) fn qa_rect_metadata(rect: egui::Rect) -> serde_json::Value {
    serde_json::json!({
        "min_x": rect.min.x,
        "min_y": rect.min.y,
        "max_x": rect.max.x,
        "max_y": rect.max.y,
        "width": rect.width(),
        "height": rect.height(),
    })
}

pub(in crate::ui::panels::node_editor) fn edge_endpoint_qa_metadata(
    connection_id: Uuid,
    role: &str,
    position: egui::Pos2,
    unclipped_rect: egui::Rect,
) -> serde_json::Value {
    serde_json::json!({
        "action": "reconnect",
        "connection_id": connection_id,
        "endpoint": role,
        "position": {"x": position.x, "y": position.y},
        "unclipped_rect": qa_rect_metadata(unclipped_rect),
    })
}

pub(in crate::ui::panels::node_editor) fn qa_port_id(
    _project: &Project,
    item: Option<GraphItem>,
    direction: &str,
    port_key: &str,
) -> String {
    match item {
        Some(GraphItem::Node(id)) => {
            format!("node_editor.port.node:{id}.{direction}:{port_key}")
        }
        Some(GraphItem::PortAnchor { owner, kind }) => {
            let role = match kind {
                PortAnchorKind::ExternalInputs => "external_input",
                PortAnchorKind::InternalMetadata => "internal_output",
                PortAnchorKind::ImageSink => "image_sink",
                PortAnchorKind::ExternalOutputs => "external_output",
            };
            format!(
                "node_editor.container_port.{}.{role}:{port_key}",
                qa_container_key(owner)
            )
        }
        Some(GraphItem::Container(owner)) => format!(
            "node_editor.container_port.{}.{}:{port_key}",
            qa_container_key(owner),
            direction
        ),
        None => format!("node_editor.port.missing.{direction}:{port_key}"),
    }
}
