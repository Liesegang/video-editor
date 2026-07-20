use crate::state::context_types::NodeEditorEditableWire;
use eframe::egui::{self, Color32};
use egui_snarl::ui::{PinInfo, PinWireInfo, SnarlPin, SnarlStyle};
use library::model::project::{PortAddress, PortDirection, PortOwner};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[cfg(test)]
use crate::ui::panels::node_editor::capture_test_rect;
use crate::ui::panels::node_editor::{
    clipped_qa_rect, node_editor_port_interactions_enabled, qa_rect_metadata, wire_port_drop_rect,
};

pub(in crate::ui::panels::node_editor) struct QaPin {
    pub(in crate::ui::panels::node_editor) info: PinInfo,
    pub(in crate::ui::panels::node_editor) component_id: String,
    pub(in crate::ui::panels::node_editor) to_global: egui::emath::TSTransform,
    pub(in crate::ui::panels::node_editor) graph_center: Option<egui::Pos2>,
    pub(in crate::ui::panels::node_editor) address: Option<PortAddress>,
    pub(in crate::ui::panels::node_editor) direction: PortDirection,
    pub(in crate::ui::panels::node_editor) connected: bool,
    pub(in crate::ui::panels::node_editor) connection_id: Option<Uuid>,
    pub(in crate::ui::panels::node_editor) canvas_clip: egui::Rect,
    pub(in crate::ui::panels::node_editor) rendered_ports:
        Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui::panels::node_editor) struct RenderedPortKey {
    pub(in crate::ui::panels::node_editor) address: PortAddress,
    pub(in crate::ui::panels::node_editor) direction: PortDirection,
    /// Merge variadic inputs have one rendered endpoint per canonical wire.
    /// Every other socket, including the vacant variadic input, uses `None`.
    pub(in crate::ui::panels::node_editor) connection_id: Option<Uuid>,
}

#[derive(Clone, Debug)]
pub(in crate::ui::panels::node_editor) struct RenderedEdge {
    pub(in crate::ui::panels::node_editor) kind: RenderedEdgeKind,
    pub(in crate::ui::panels::node_editor) start: egui::Pos2,
    pub(in crate::ui::panels::node_editor) control_a: egui::Pos2,
    pub(in crate::ui::panels::node_editor) control_b: egui::Pos2,
    pub(in crate::ui::panels::node_editor) end: egui::Pos2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui::panels::node_editor) enum RenderedEdgeKind {
    ProjectConnection { connection_id: Uuid },
    OutputBinding { owner: PortOwner, node_id: Uuid },
    DerivedOutput { owner: PortOwner, source: PortOwner },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::node_editor) enum WireSecondaryClickHit {
    Editable(NodeEditorEditableWire),
    DisplayOnly,
}

impl RenderedEdgeKind {
    pub(in crate::ui::panels::node_editor) fn metadata_kind(self) -> &'static str {
        match self {
            Self::ProjectConnection { .. } => "explicit",
            Self::OutputBinding { .. } => "output_binding",
            Self::DerivedOutput { .. } => "derived_output",
        }
    }

    pub(in crate::ui::panels::node_editor) fn editable_wire(
        self,
    ) -> Option<NodeEditorEditableWire> {
        match self {
            Self::ProjectConnection { connection_id } => {
                Some(NodeEditorEditableWire::ProjectConnection { connection_id })
            }
            Self::OutputBinding { owner, node_id } => {
                Some(NodeEditorEditableWire::OutputBinding { owner, node_id })
            }
            Self::DerivedOutput { .. } => None,
        }
    }

    pub(in crate::ui::panels::node_editor) fn connection_id(self) -> Option<Uuid> {
        match self {
            Self::ProjectConnection { connection_id } => Some(connection_id),
            Self::OutputBinding { .. } | Self::DerivedOutput { .. } => None,
        }
    }

    pub(in crate::ui::panels::node_editor) fn blocked_reason(self) -> Option<&'static str> {
        matches!(self, Self::DerivedOutput { .. }).then_some(
            "Derived wire follows authoritative containment; reparent or remove the child instead",
        )
    }
}

pub(in crate::ui::panels::node_editor) struct EdgeComponent<'a> {
    pub(in crate::ui::panels::node_editor) id: String,
    pub(in crate::ui::panels::node_editor) kind: RenderedEdgeKind,
    pub(in crate::ui::panels::node_editor) from: &'a PortAddress,
    pub(in crate::ui::panels::node_editor) to: &'a PortAddress,
    pub(in crate::ui::panels::node_editor) wire_color: Color32,
    pub(in crate::ui::panels::node_editor) authored_order: Option<i64>,
    pub(in crate::ui::panels::node_editor) back_to_front_index: Option<usize>,
    pub(in crate::ui::panels::node_editor) layer_count: Option<usize>,
    pub(in crate::ui::panels::node_editor) authored_blend_mode: Option<&'static str>,
    pub(in crate::ui::panels::node_editor) authored_blend_available: bool,
}

#[derive(Clone, Copy)]
pub(in crate::ui::panels::node_editor) struct OverviewWirePainter<'a> {
    pub(in crate::ui::panels::node_editor) painter: &'a egui::Painter,
    pub(in crate::ui::panels::node_editor) to_global: egui::emath::TSTransform,
}

impl SnarlPin for QaPin {
    fn pin_rect(&self, x: f32, y0: f32, y1: f32, size: f32) -> egui::Rect {
        let center = self
            .graph_center
            .unwrap_or_else(|| egui::pos2(x, (y0 + y1) * 0.5));
        // Tiny graph-space sockets are not useful at overview scale. Making
        // their interaction rect empty also prevents an accidental wire drag
        // from stealing the gesture intended to pan the overview.
        let interaction_size = if node_editor_port_interactions_enabled(self.to_global.scaling) {
            size
        } else {
            0.0
        };
        let rect =
            egui::Rect::from_center_size(center, egui::vec2(interaction_size, interaction_size));
        let unclipped_global_rect = self.to_global * rect;
        // QA publishes the real custom reconnect drop target, not only
        // Snarl's normal wire-start target. The latter intentionally becomes
        // a point at overview zoom, while `rendered_port_at_position` still
        // accepts endpoint drops in this screen-space radius.
        let unclipped_drop_rect = wire_port_drop_rect(unclipped_global_rect);
        let drop_rect = clipped_qa_rect(unclipped_drop_rect, self.canvas_clip);
        if let Some(address) = &self.address {
            if let Ok(mut ports) = self.rendered_ports.lock() {
                ports.insert(
                    RenderedPortKey {
                        address: address.clone(),
                        direction: self.direction,
                        connection_id: self.connection_id,
                    },
                    unclipped_global_rect,
                );
            }
        }
        #[cfg(test)]
        capture_test_rect(&self.component_id, drop_rect);
        crate::qa::register_component_with_metadata(
            self.component_id.clone(),
            "node_port",
            drop_rect,
            true,
            Some(serde_json::json!({
                "action": "connect_or_reconnect",
                "connected": self.connected,
                "direction": match self.direction {
                    PortDirection::Input => "input",
                    PortDirection::Output => "output",
                },
                "normal_interaction_enabled": interaction_size > 0.0,
                "connection_id": self.connection_id,
                "unclipped_rect": qa_rect_metadata(unclipped_drop_rect),
                "visible_in_canvas": drop_rect.is_positive(),
            })),
        );
        rect
    }

    fn draw(
        self,
        snarl_style: &SnarlStyle,
        style: &egui::Style,
        rect: egui::Rect,
        painter: &egui::Painter,
    ) -> PinWireInfo {
        self.info.draw(snarl_style, style, rect, painter)
    }
}
