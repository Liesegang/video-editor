use eframe::egui;
use library::model::project::{PortDataType, PortOwner};
use uuid::Uuid;

pub(super) const CONTAINER_HEADER_HEIGHT: f32 = 64.0;
pub(super) const CONTAINER_CONTROL_OFFSET: egui::Vec2 = egui::vec2(14.0, 10.0);
pub(super) const CONTAINER_PORT_Y: f32 = 86.0;
pub(super) const EMBEDDED_PORT_LABEL_INSET: f32 = 18.0;
pub(super) const RESIZE_HIT_WIDTH: f32 = 7.0;
pub(super) const RESIZE_CORNER_SIZE: f32 = 15.0;
pub(super) const NODE_BODY_WIDTH: f32 = 200.0;
pub(super) const MERGE_BODY_WIDTH: f32 = 230.0;
pub(super) const NODE_HEADER_WIDTH: f32 = 190.0;
pub(super) const PORT_LABEL_WIDTH: f32 = 96.0;
pub(super) const PORT_ROW_HEIGHT: f32 = 22.0;
pub(super) const PROPERTY_LABEL_WIDTH: f32 = 58.0;
pub(super) const INLINE_CONTROL_WIDTH: f32 = 126.0;
pub(super) const WIRE_HIT_RADIUS: f32 = 8.0;
pub(super) const WIRE_ENDPOINT_RADIUS: f32 = 12.0;
/// The custom endpoint-reconnect path accepts a drop this many screen points
/// around a rendered port, including at overview zoom where Snarl's normal
/// port interaction is deliberately disabled.
pub(super) const WIRE_PORT_DROP_RADIUS: f32 = 5.0;
pub(super) const WIRE_DRAG_THRESHOLD: f32 = 6.0;
/// Crossing a container boundary is a semantic edit, so a tiny title jitter
/// must not change ownership. The threshold is measured in screen points and
/// therefore remains stable under Node Editor zoom.
pub(super) const NODE_REPARENT_DRAG_THRESHOLD: f32 = 8.0;
/// A pointer inside a candidate can select it before the Node center crosses
/// only when a meaningful portion of the final rendered Node is already in
/// that candidate. This keeps header-offset drags usable without pointer-only
/// reparenting at a boundary.
pub(super) const NODE_REPARENT_POINTER_OVERLAP_THRESHOLD: f32 = 0.35;
pub(super) const MIN_CONTAINER_SIZE: egui::Vec2 = egui::vec2(360.0, 220.0);
pub(super) const AUTO_LAYOUT_COLUMN_GAP: f32 = 112.0;
pub(super) const AUTO_LAYOUT_ROW_GAP: f32 = 52.0;
pub(super) const AUTO_LAYOUT_NODE_PADDING: f32 = 24.0;
pub(super) const DETACHED_GRAPH_NODE_GAP: f32 = AUTO_LAYOUT_NODE_PADDING + 0.5;
pub(super) const AUTO_LAYOUT_CLIP_GAP: f32 = 64.0;
pub(super) const AUTO_LAYOUT_TRACK_GAP: f32 = 80.0;
pub(super) const CONTAINER_CONTROL_CHROME_HEIGHT: f32 = 54.0;
pub(super) const CONTAINER_STANDARD_BODY_ROWS: f32 = 2.0;
pub(super) const CONTAINER_CLIP_BODY_ROWS: f32 = 6.0;
const fn reserved_control_top(body_rows: f32) -> f32 {
    CONTAINER_CONTROL_OFFSET.y
        + CONTAINER_CONTROL_CHROME_HEIGHT
        + body_rows * PORT_ROW_HEIGHT
        + AUTO_LAYOUT_NODE_PADDING
}
pub(super) const AUTO_LAYOUT_COMPOSITION_TOP: f32 =
    reserved_control_top(CONTAINER_STANDARD_BODY_ROWS);
pub(super) const AUTO_LAYOUT_COMPOSITION_LEFT: f32 = 190.0;
pub(super) const AUTO_LAYOUT_COMPOSITION_RIGHT: f32 = 100.0;
pub(super) const AUTO_LAYOUT_COMPOSITION_BOTTOM: f32 = 100.0;
pub(super) const AUTO_LAYOUT_TRACK_TOP: f32 = reserved_control_top(CONTAINER_STANDARD_BODY_ROWS);
pub(super) const AUTO_LAYOUT_CLIP_TOP: f32 = reserved_control_top(CONTAINER_CLIP_BODY_ROWS);
pub(super) const AUTO_LAYOUT_TRACK_LEFT: f32 = 184.0;
pub(super) const AUTO_LAYOUT_TRACK_RIGHT: f32 = 150.0;
pub(super) const AUTO_LAYOUT_TRACK_BOTTOM: f32 = 70.0;

/// Ephemeral Snarl payload. It contains identity and visual role only; all
/// editable values continue to live in `Project`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum GraphItem {
    Node(Uuid),
    Container(PortOwner),
    PortAnchor {
        owner: PortOwner,
        kind: PortAnchorKind,
    },
}

/// Zero-chrome Snarl anchors keep real pin drag semantics while the visible
/// sockets and labels are embedded directly in the single container chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum PortAnchorKind {
    ExternalInputs,
    InternalMetadata,
    ImageSink,
    ExternalImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ContainerKind {
    Composition,
    Track,
    Clip,
}

#[derive(Clone, Debug)]
pub(super) struct ContainerVisual {
    pub(super) owner: PortOwner,
    pub(super) kind: ContainerKind,
    pub(super) position: [f32; 2],
    pub(super) size: [f32; 2],
    pub(super) collapsed: bool,
}

impl ContainerVisual {
    pub(super) fn rect(&self) -> egui::Rect {
        let height = if self.collapsed {
            CONTAINER_HEADER_HEIGHT
        } else {
            self.size[1].max(MIN_CONTAINER_SIZE.y)
        };
        egui::Rect::from_min_size(
            egui::pos2(self.position[0], self.position[1]),
            egui::vec2(self.size[0].max(MIN_CONTAINER_SIZE.x), height),
        )
    }
}

#[derive(Clone)]
pub(super) struct PinDefinition {
    pub(super) key: String,
    pub(super) name: String,
    pub(super) data_type: PortDataType,
}
