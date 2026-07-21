use eframe::egui;
use library::model::project::{PortDataType, PortOwner};
use uuid::Uuid;

pub(super) const CONTAINER_HEADER_HEIGHT: f32 = 48.0;
pub(super) const CONTAINER_CONTROL_OFFSET: egui::Vec2 = egui::vec2(10.0, 6.0);
/// First left-edge metadata socket. Additional sockets stack downward without
/// reserving the same vertical strip across the full container width.
pub(super) const CONTAINER_PORT_Y: f32 = 66.0;
/// Right-edge container outputs live in the header and stack compactly. This
/// keeps their hit regions away from the container's usable body.
pub(super) const CONTAINER_RIGHT_PORT_Y: f32 = 12.0;
pub(super) const CONTAINER_RIGHT_PORT_ROW_HEIGHT: f32 = 24.0;
pub(super) const PORT_SOCKET_SIZE: f32 = 13.0;
pub(super) const EMBEDDED_PORT_LABEL_INSET: f32 = 18.0;
pub(super) const RESIZE_HIT_WIDTH: f32 = 7.0;
pub(super) const RESIZE_CORNER_SIZE: f32 = 15.0;
pub(super) const NODE_BODY_WIDTH: f32 = 200.0;
pub(super) const MERGE_BODY_WIDTH: f32 = 242.0;
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
/// Container content starts just below the compact header. Left-edge metadata
/// labels get a narrow, fixed rail; their number no longer pushes the entire
/// body down. The right rail is only wide enough for a localized socket hit.
pub(super) const AUTO_LAYOUT_COMPOSITION_TOP: f32 = CONTAINER_HEADER_HEIGHT + 12.0;
pub(super) const AUTO_LAYOUT_COMPOSITION_LEFT: f32 = 80.0;
pub(super) const AUTO_LAYOUT_COMPOSITION_RIGHT: f32 = 24.0;
pub(super) const AUTO_LAYOUT_COMPOSITION_BOTTOM: f32 = 24.0;
pub(super) const AUTO_LAYOUT_TRACK_TOP: f32 = CONTAINER_HEADER_HEIGHT + 12.0;
pub(super) const AUTO_LAYOUT_CLIP_TOP: f32 = CONTAINER_HEADER_HEIGHT + 12.0;
pub(super) const AUTO_LAYOUT_TRACK_LEFT: f32 = 80.0;
pub(super) const AUTO_LAYOUT_TRACK_RIGHT: f32 = 24.0;
pub(super) const AUTO_LAYOUT_TRACK_BOTTOM: f32 = 24.0;

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
    OutputSinks,
    ExternalOutputs,
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

    pub(super) fn content_rect(&self) -> Option<egui::Rect> {
        if self.collapsed {
            return None;
        }
        let rect = self.rect();
        let (left, top, right, bottom) = match self.kind {
            ContainerKind::Composition => (
                AUTO_LAYOUT_COMPOSITION_LEFT,
                AUTO_LAYOUT_COMPOSITION_TOP,
                AUTO_LAYOUT_COMPOSITION_RIGHT,
                AUTO_LAYOUT_COMPOSITION_BOTTOM,
            ),
            ContainerKind::Track => (
                AUTO_LAYOUT_TRACK_LEFT,
                AUTO_LAYOUT_TRACK_TOP,
                AUTO_LAYOUT_TRACK_RIGHT,
                AUTO_LAYOUT_TRACK_BOTTOM,
            ),
            ContainerKind::Clip => (
                AUTO_LAYOUT_TRACK_LEFT,
                AUTO_LAYOUT_CLIP_TOP,
                AUTO_LAYOUT_TRACK_RIGHT,
                AUTO_LAYOUT_TRACK_BOTTOM,
            ),
        };
        let content = egui::Rect::from_min_max(
            rect.min + egui::vec2(left, top),
            rect.max - egui::vec2(right, bottom),
        );
        content.is_positive().then_some(content)
    }

    pub(super) fn embedded_port_center(&self, kind: PortAnchorKind, index: usize) -> egui::Pos2 {
        let rect = self.rect();
        let left_row_y = if self.collapsed {
            rect.top() + 11.0 + index as f32 * 9.0
        } else {
            rect.top() + CONTAINER_PORT_Y + index as f32 * PORT_ROW_HEIGHT
        };
        let right_row_y =
            rect.top() + CONTAINER_RIGHT_PORT_Y + index as f32 * CONTAINER_RIGHT_PORT_ROW_HEIGHT;
        match kind {
            PortAnchorKind::ExternalInputs => egui::pos2(rect.left() - 7.0, left_row_y),
            PortAnchorKind::InternalMetadata => egui::pos2(rect.left() + 7.0, left_row_y),
            PortAnchorKind::OutputSinks => egui::pos2(rect.right() - 32.0, right_row_y),
            PortAnchorKind::ExternalOutputs => egui::pos2(rect.right(), right_row_y),
        }
    }

    /// Unit-scale screen hit used by QA geometry tests. Runtime projection
    /// applies canvas scale first and then the fixed screen-space drop radius.
    #[cfg(test)]
    pub(super) fn unit_scale_port_hit_rect(
        &self,
        kind: PortAnchorKind,
        index: usize,
    ) -> egui::Rect {
        egui::Rect::from_center_size(
            self.embedded_port_center(kind, index),
            egui::Vec2::splat(PORT_SOCKET_SIZE),
        )
        .expand(WIRE_PORT_DROP_RADIUS)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PinDefinition {
    pub(super) key: String,
    pub(super) name: String,
    pub(super) data_type: PortDataType,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expanded_visual(kind: ContainerKind) -> ContainerVisual {
        ContainerVisual {
            owner: PortOwner::Composition(Uuid::nil()),
            kind,
            position: [100.0, 80.0],
            size: [800.0, 500.0],
            collapsed: false,
        }
    }

    #[test]
    fn container_content_reclaims_body_beside_localized_port_rails() {
        for kind in [
            ContainerKind::Composition,
            ContainerKind::Track,
            ContainerKind::Clip,
        ] {
            let visual = expanded_visual(kind);
            let frame = visual.rect();
            let content = visual.content_rect().expect("expanded content rect");
            let area_ratio = content.width() * content.height() / (frame.width() * frame.height());

            assert!(area_ratio > 0.65, "{kind:?} content ratio={area_ratio}");
            assert_eq!(frame.right() - content.right(), 24.0);
            assert!(content.contains(egui::pos2(frame.right() - 40.0, content.center().y,)));

            for (anchor, index) in [
                (PortAnchorKind::InternalMetadata, 0),
                (PortAnchorKind::InternalMetadata, 5),
                (PortAnchorKind::OutputSinks, 0),
                (PortAnchorKind::ExternalOutputs, 0),
            ] {
                let hit = visual.unit_scale_port_hit_rect(anchor, index);
                assert_eq!(hit.size(), egui::Vec2::splat(23.0));
                assert!(
                    !hit.intersects(content),
                    "{kind:?} {anchor:?} hit {hit:?} overlaps content {content:?}",
                );
            }
            assert!(!visual
                .unit_scale_port_hit_rect(PortAnchorKind::OutputSinks, 0)
                .intersects(visual.unit_scale_port_hit_rect(PortAnchorKind::ExternalOutputs, 0,)));
        }
    }

    #[test]
    fn stacked_external_outputs_have_disjoint_header_hits() {
        let visual = expanded_visual(ContainerKind::Track);
        let frame = visual.rect();
        let image = visual.unit_scale_port_hit_rect(PortAnchorKind::ExternalOutputs, 0);
        let audio = visual.unit_scale_port_hit_rect(PortAnchorKind::ExternalOutputs, 1);

        assert!(!image.intersects(audio));
        assert!(image.top() >= frame.top());
        assert!(audio.bottom() <= frame.top() + CONTAINER_HEADER_HEIGHT);
        assert_eq!(audio.center().y - image.center().y, 24.0);

        let image_sink = visual.unit_scale_port_hit_rect(PortAnchorKind::OutputSinks, 0);
        let audio_sink = visual.unit_scale_port_hit_rect(PortAnchorKind::OutputSinks, 1);
        assert!(!image_sink.intersects(audio_sink));
        assert!(!image_sink.intersects(image));
        assert!(!audio_sink.intersects(audio));
    }

    #[test]
    fn collapsed_container_has_no_node_placement_surface() {
        let mut visual = expanded_visual(ContainerKind::Clip);
        visual.collapsed = true;
        assert!(visual.content_rect().is_none());
    }
}
